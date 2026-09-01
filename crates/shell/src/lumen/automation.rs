//! Read-only answers the shell gives to the automation surfaces (driver API,
//! MCP, BiDi) plus the two commands that need shell state to execute.
//!
//! Every method here is polled or called from `crate::app::about_to_wait`,
//! which is where an `AutomationCommand` is dequeued; none of them is reachable
//! from ordinary user input. `check_wait_condition` in particular is polled
//! once per frame until it answers `true` or its deadline passes.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`, including the
//! eight-space indentation the region carried there. Behaviour and method
//! bodies are unchanged; only the module path and visibility differ.

use crate::*;

impl Lumen {
        /// Resolve an automation click target to `handle_click_at`'s expected
        /// OS-window CSS-pixel coordinates.
        ///
        /// `NodeId`/`Selector` rects come out of the layout tree in *page*
        /// (document) space; `handle_click_at` expects *OS window* space (what
        /// a real OS mouse event reports — see `page_point`, which converts the
        /// other way: `page = window - tab_bar/panel_offset + scroll`). This
        /// applies the inverse (`window = page - scroll + tab_bar/panel_offset`)
        /// so a click lands on the resolved element instead of wherever
        /// page-space coordinates happen to fall in window space (off by the
        /// tab-bar height and current scroll — silently "worked" only by
        /// coincidence when scroll was 0 and the target sat within the
        /// tab-bar-height band).
        ///
        /// `Target::Point` gets a *different* correction: BiDi/MCP callers
        /// (`input.performActions` pointer coordinates) supply pixels in the
        /// rendered *content-viewport* space — the same space `captureScreenshot`
        /// renders (no scroll subtraction needed, since it's relative to the
        /// already-scrolled visible viewport, not absolute document position) —
        /// so only the tab-bar/toolbar/panel offset is added, not scroll. Confirmed
        /// by hand: without this, `input.performActions` clicks landed above the
        /// target by exactly `toolbar::CHROME_H` (real pixel offset validated with
        /// a manual BiDi click→navigate scenario; DS-9 widened the offset from
        /// the tab-bar-only height to include the new toolbar row).
        ///
        /// CC-14: the offset itself is [`Self::page_offset`], not a hardcoded
        /// `(left_dock width, toolbar::CHROME_H)` pair — the content area's
        /// real origin is `chrome_page_host_rect`'s, which can differ from the
        /// legacy toolbar/sidebar geometry (e.g. the web/AI sidebar occupies
        /// chrome layout width but is not a `left_dock()` entry at all,
        /// see [`Self::dockable_sidebars`]). Using the wrong offset here would
        /// silently misfire every MCP/BiDi click/type once engine chrome is
        /// the default, since `page_offset()` is otherwise the single source
        /// of truth for this conversion (real mouse input already uses it).
        pub(crate) fn resolve_automation_target(&self, target: &lumen_driver::Target) -> Option<(f32, f32)> {
            use lumen_driver::Target;
            let (offset_x, offset_y) = self.page_offset();
            let page_to_viewport = |px: f32, py: f32| {
                (px - self.scroll_x + offset_x, py - self.scroll_y + offset_y)
            };
            match target {
                Target::Point { x, y } => Some((x + offset_x, y + offset_y)),
                Target::NodeId(id) => {
                    let lb = self.layout_box.as_ref()?;
                    let node = lumen_dom::NodeId::from_index(*id as usize);
                    let rect = forms::find_box_rect(lb, node)?;
                    Some(page_to_viewport(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0))
                }
                Target::Selector(selector) => {
                    let lb = self.layout_box.as_ref()?;
                    let doc = self.layout_source.as_ref()?.document.lock().ok()?;
                    let rect = lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector)
                        .first()?
                        .rect;
                    Some(page_to_viewport(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0))
                }
            }
        }

        /// Find DOM nodes by CSS selector for `AutomationCommand::Query` (SDC-2).
        ///
        /// Returns an empty vector if no page is loaded or nothing matches —
        /// mirrors `InProcessSession::query`'s behavior for the same case.
        pub(crate) fn query_automation_nodes(&self, selector: &str) -> Vec<lumen_driver::NodeRef> {
            let Some(lb) = self.layout_box.as_ref() else { return Vec::new() };
            let Some(source) = self.layout_source.as_ref() else { return Vec::new() };
            let Ok(doc) = source.document.lock() else { return Vec::new() };
            lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector)
                .into_iter()
                .map(|found| {
                    let tag_name = match &doc.get(found.node).data {
                        NodeData::Element { name, .. } => name.local.to_string(),
                        _ => String::new(),
                    };
                    let mut text_content = String::new();
                    collect_automation_text(&doc, found.node, &mut text_content);
                    lumen_driver::NodeRef {
                        node_id: found.node.index() as u32,
                        tag_name,
                        text_content,
                        bounding_rect: found.rect,
                    }
                })
                .collect()
        }

        /// Build the accessibility tree for `AutomationCommand::A11yTree` (SDC-2).
        ///
        /// Returns `None` if no page is loaded.
        pub(crate) fn automation_a11y_tree(&self) -> Option<lumen_driver::A11yNode> {
            let source = self.layout_source.as_ref()?;
            let doc = source.document.lock().ok()?;
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            let ax_tree = lumen_a11y::build_ax_tree(&doc, doc.root(), &flat_tree);
            let chrome = self.chrome_ax_nodes();
            let ax_tree = lumen_a11y::chrome::attach_chrome(ax_tree, chrome);
            Some(automation_ax_node(&ax_tree.root))
        }

        /// Box-model snapshot of the whole page for `AutomationCommand::LayoutSnapshot`
        /// (DEVX-14, wires `resource://layout` to the live window).
        ///
        /// Empty if no page is loaded — mirrors `InProcessSession::layout_snapshot`'s
        /// behavior on the equivalent state.
        pub(crate) fn automation_layout_snapshot(&self) -> Vec<BoxModel> {
            let Some(lb) = self.layout_box.as_ref() else { return Vec::new() };
            let Some(source) = self.layout_source.as_ref() else { return Vec::new() };
            let Ok(doc) = source.document.lock() else { return Vec::new() };
            let mut out = Vec::new();
            lumen_driver::scope::collect_boxes(lb, &doc, &mut out);
            out
        }

        /// Network request log for `AutomationCommand::NetworkLog` (DEVX-14,
        /// wires `resource://network` to the live window) — reads the same
        /// shared `NetworkLog` the DevTools network panel renders from,
        /// regardless of whether that panel is currently open.
        ///
        /// `size_bytes` is always 0: unlike `InProcessSession`'s network log,
        /// the DevTools panel's `NetworkEntry` doesn't track response size.
        pub(crate) fn automation_network_log(&self) -> Vec<DriverNetworkEntry> {
            self.network_panel
                .entries_clone()
                .iter()
                .map(|e| DriverNetworkEntry {
                    url: e.url.clone(),
                    method: e.method.clone(),
                    status: e.status.unwrap_or(0),
                    size_bytes: 0,
                })
                .collect()
        }

        /// Poll an `AutomationCommand::Wait` condition against current shell
        /// state (SDC-1b). Never blocks — called once per frame from
        /// `about_to_wait` via `self.pending_waits` until it returns `true` or
        /// the wait's deadline passes.
        ///
        /// `NetworkIdle` and `Stable` are conservative approximations (no
        /// in-flight-request counter or cross-frame rect history exists yet in
        /// the shell — same simplification `InProcessSession::check_wait_condition`
        /// uses headless): `NetworkIdle` falls back to `DocumentReady`, and
        /// `Stable` only checks that the selector currently matches an element.
        ///
        /// `DocumentReady` reads the real `document.readyState` from the JS
        /// runtime (P2-wpt S1) rather than approximating via `self.layout_box`
        /// — the layout box exists as soon as the *previous* page's box tree
        /// is still around (it is not reset on ordinary navigation, only on
        /// `reset_to_blank_tab`), so it was `true` immediately on repeat
        /// navigations even before the new page finished loading.
        ///
        /// When a real JS context is available, also gated on
        /// `self.nav_start.is_none()`: on the non-blocking streaming
        /// navigation path (`reload`/`navigate_to` with a window already
        /// open), `self.js_ctx` still holds the *previous* page's context
        /// until `apply_loaded_page` installs the new one — reading
        /// `document.readyState` without this gate would see the old page's
        /// already-`"complete"` state and report ready immediately,
        /// reproducing the exact bug this fixes. `nav_start` is set at the
        /// start of every navigation and only cleared once
        /// `apply_loaded_page` (which also installs the fresh JS context and
        /// fires the real `load` event) has run — see `RenderDone` handling.
        /// (`nav_start` is only cleared under `#[cfg(feature = "v8")]`,
        /// so the gate is scoped to the branch that actually has a JS
        /// context — the `layout_box` fallback below stays independent of it
        /// for JS-less builds/tabs, matching the pre-S1 behavior there.)
        pub(crate) fn check_wait_condition(&self, cond: &WaitCondition) -> bool {
            match cond {
                WaitCondition::DocumentReady | WaitCondition::NetworkIdle => {
                    // A settled navigation error (network/HTTP failure) is "done
                    // loading" — resolve immediately instead of hanging until the
                    // wait's deadline (BUG-308). Without this, a nav that ends in
                    // `LoadError` with no JS context and no prior `layout_box`
                    // (e.g. `about:blank` → an anti-bot 403) never satisfies
                    // either readiness branch below, so `wait{document_ready}`
                    // blocks for minutes. Still gated on `nav_start.is_none()` so
                    // a stale flag from a superseded nav can't win a race.
                    if self.nav_start.is_none() && self.load_failed {
                        return true;
                    }
                    // ADR-016: eval через `route_query_js`, тот же паттерн, что
                    // `WaitCondition::JsIdle` ниже.
                    match route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
                        j.eval_js_value("document.readyState")
                    }) {
                        Some(Ok(json)) => self.nav_start.is_none() && json == "\"complete\"",
                        // No JS context at all (v8 disabled, or a
                        // JS-less blank tab) — fall back to the coarser
                        // layout signal so `Wait` doesn't hang forever on a
                        // readiness signal that will never arrive. Still gated
                        // on `nav_start.is_none()` (found while diagnosing
                        // P2-wpt S4): without this gate, a navigation issued
                        // from a JS-less blank tab (or racing the brief window
                        // before the new page's JS context is installed) could
                        // see `js_ctx` as the *old* tab's `None`, and report
                        // ready from the *previous* page's already-populated
                        // `layout_box` before the new page had even started
                        // loading — the same "stale state wins the race"
                        // pattern BUG-296 fixed for session restore.
                        _ => self.nav_start.is_none() && self.layout_box.is_some(),
                    }
                }
                WaitCondition::Visible(selector) => {
                    let Some(lb) = self.layout_box.as_ref() else { return false };
                    let Some(source) = self.layout_source.as_ref() else { return false };
                    let Ok(doc) = source.document.lock() else { return false };
                    lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector)
                        .first()
                        .is_some_and(|b| b.rect.width > 0.0 && b.rect.height > 0.0)
                }
                WaitCondition::Stable(selector) => {
                    let Some(lb) = self.layout_box.as_ref() else { return false };
                    let Some(source) = self.layout_source.as_ref() else { return false };
                    let Ok(doc) = source.document.lock() else { return false };
                    !lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector).is_empty()
                }
                // ADR-016 M2.2c-2d: последнее прямое `self.js_ctx`-чтение в wait-poll —
                // `has_raf_pending` через `route_query_js` (под флагом — блокирующий
                // `query`; внешний `None` = «без JS» → idle, как прежний `is_none_or`).
                WaitCondition::JsIdle => {
                    !route_query_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |c| {
                        c.has_raf_pending()
                    })
                    .unwrap_or(false)
                }
            }
        }

        /// Apply scroll delta with bounds clamping.
        pub(crate) fn scroll_by_delta(&mut self, dx: f32, dy: f32) {
            self.scroll_x = (self.scroll_x + dx).max(0.0);
            self.scroll_y = (self.scroll_y + dy).max(0.0);
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }

        /// Render the currently loaded page's content area to PNG bytes
        /// (`AutomationCommand::Screenshot`, SDC-1b).
        ///
        /// Renders `self.display_list` — the page content only, not the browser
        /// chrome (tab strip/panels) — through the deterministic CPU rasterizer
        /// (same renderer as `--screenshot`/`--ipc-server`), at the current
        /// window's content viewport size and scroll offset.
        ///
        /// BUG-729: the image set comes from `self.image_cache`, whose keys are
        /// the very strings `register_image` gets — i.e. exactly what the
        /// display list's `DrawImage`/`LazyImageSlot`/background-image commands
        /// look up. Passing an empty slice here (the SDC-1b behaviour) made
        /// *every* picture on the page rasterize as the grey placeholder, so an
        /// automation screenshot of a perfectly rendering page read as "the
        /// browser draws no images at all". Canvas 2D bitmaps are still absent:
        /// they live in the JS runtime and reach paint only through the per-frame
        /// `flush_canvas_updates` drain into the GPU renderer, never through this
        /// CPU-side cache.
        pub(crate) fn render_current_page_to_png(&self) -> Result<Vec<u8>, String> {
            use lumen_paint::Renderer;
            let width = (self.viewport_width_css().max(1.0)) as u32;
            let height = (self.viewport_height_css().max(1.0)) as u32;
            let images = self.image_cache.snapshot();
            let image = Renderer::render_to_image_cpu(
                width,
                height,
                &self.display_list,
                &images,
                self.scroll_x,
                self.scroll_y,
            )
            .map_err(|e| format!("render_to_image_cpu: {e}"))?;
            lumen_image::encode_png_rgba8(&image).map_err(|e| format!("PNG encoding: {e}"))
        }

    /// Return a cloneable [`InputSender`] for injecting synthetic input events.
    ///
    /// Callers on any thread can use the sender to enqueue [`InputCommand`]s;
    /// they are drained and dispatched in `about_to_wait`.
    #[allow(dead_code)]
    pub fn input_sender(&self) -> input::InputSender {
        self.input_tx.clone()
    }

    /// Return a cloneable handle for driving this window's automation channel (SDC-2).
    ///
    /// Callers on any thread can use this to send [`AutomationCommand`]s and
    /// block for their reply; commands are drained and dispatched in `about_to_wait`.
    #[allow(dead_code)]
    pub fn automation_handle(&self) -> AutomationHandle {
        AutomationHandle::new(self.automation_cmd_tx.clone())
    }
}

/// Collect the concatenated text content of `id`'s subtree (SDC-2 `Query` support).
pub(crate) fn collect_automation_text(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children {
        collect_automation_text(doc, child, out);
    }
}

/// Convert a `lumen_a11y::AXNode` into the driver's public `A11yNode` reply
/// type (SDC-2 `A11yTree` support). Mirrors `lumen_driver`'s own private
/// conversion in `session.rs`/`winit_session.rs` — kept local here since the
/// shell has no dependency the other direction.
pub(crate) fn automation_ax_node(ax: &lumen_a11y::AXNode) -> lumen_driver::A11yNode {
    let state = lumen_driver::A11yState {
        disabled: ax.state.disabled,
        checked: ax.state.checked,
        expanded: ax.state.expanded,
        hidden: ax.state.hidden,
        selected: ax.state.selected,
        pressed: ax.state.pressed,
        required: ax.state.required,
        readonly: ax.state.readonly,
        invalid: ax.state.invalid,
        level: ax.state.level,
    };
    lumen_driver::A11yNode {
        node_id: ax.node_id.index() as u32,
        role: ax.role.as_str().to_owned(),
        name: ax.name.clone(),
        description: ax.description.clone(),
        placeholder: ax.placeholder.clone(),
        state,
        children: ax.children.iter().map(automation_ax_node).collect(),
    }
}

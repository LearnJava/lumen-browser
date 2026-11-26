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
        pub(crate) fn resolve_automation_target(&self, target: &lumen_driver::Target) -> Option<ResolvedTarget> {
            use lumen_driver::Target;
            let (offset_x, offset_y) = self.page_offset();
            let page_to_viewport = |px: f32, py: f32| {
                (px - self.scroll_x + offset_x, py - self.scroll_y + offset_y)
            };
            match target {
                Target::Point { x, y } => Some(ResolvedTarget {
                    x: x + offset_x,
                    y: y + offset_y,
                    node: None,
                }),
                Target::NodeId(id) => {
                    let lb = self.layout_box.as_ref()?;
                    let node = lumen_dom::NodeId::from_index(*id as usize);
                    // Inline elements own no LayoutBox — fall back to the
                    // per-fragment rects `collect_client_rects` already maps
                    // for them (same source `getClientRects` uses).
                    let rect = forms::find_box_rect(lb, node).or_else(|| {
                        let doc = self.layout_source.as_ref()?.document.lock().ok()?;
                        let rs = lumen_layout::collect_client_rects(lb, &doc)
                            .get(&(node.index() as u32))?
                            .clone();
                        union_inline_rects(&rs)
                    })?;
                    let (x, y) = page_to_viewport(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                    Some(ResolvedTarget { x, y, node: Some(node) })
                }
                Target::Selector(selector) => {
                    let lb = self.layout_box.as_ref()?;
                    let doc = self.layout_source.as_ref()?.document.lock().ok()?;
                    if let Some(found) =
                        lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector).first()
                    {
                        let rect = found.rect;
                        let (x, y) = page_to_viewport(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                        return Some(ResolvedTarget { x, y, node: Some(found.node) });
                    }
                    // Same inline-element gap as `Target::NodeId`: the DOM
                    // walk finds `<a>`/`<span>`/…, `collect_client_rects`
                    // supplies the geometry the layout walk cannot.
                    let nid = *lumen_layout::selector_query::query_all(&doc, selector).first()?;
                    let rs = lumen_layout::collect_client_rects(lb, &doc)
                        .get(&(nid.index() as u32))?
                        .clone();
                    let rect = union_inline_rects(&rs)?;
                    let (x, y) = page_to_viewport(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                    Some(ResolvedTarget { x, y, node: Some(nid) })
                }
            }
        }

        /// BUG-1044: does a synthetic click at the resolved point actually reach
        /// the element the caller named? `Some(message)` = it does not, and the
        /// automation channel must answer that message instead of `success`.
        ///
        /// Resolution puts the point at the centre of the target's *own* box, so
        /// a mismatch means something else is painted over that centre (or the
        /// target owns no box at that position at all) — a click delivered there
        /// would be dispatched at a foreign element, do nothing the caller asked
        /// for, and still report `Ack`. That "succeeds but does nothing"
        /// signature is what BUG-436 and BUG-1044 were both filed for, and it is
        /// unrecoverable for the caller: a script cannot distinguish it from a
        /// page that ignored the click.
        ///
        /// `Target::Point` has no element to belong to, so it is never checked —
        /// a raw coordinate means exactly "click here", whatever is there.
        ///
        /// The hit test is [`Self::pointer_target`] — the very one
        /// `handle_click_at` runs — so this answers what *that* click will see,
        /// not a second, independently-drifting notion of the same question.
        pub(crate) fn automation_hit_mismatch(
            &self,
            resolved: &ResolvedTarget,
        ) -> Option<String> {
            let node = resolved.node?;
            let hit = self.pointer_target(resolved.x, resolved.y).page;
            let intercepted = match hit {
                None => {
                    return Some(format!(
                        "Element click intercepted: point ({:.0}, {:.0}) hits no element at all, not the target (node {})",
                        resolved.x,
                        resolved.y,
                        node.index(),
                    ));
                }
                Some(hit) => {
                    // The document guard is scoped to this block on purpose:
                    // `automation_node_label` below takes the same (non-reentrant)
                    // lock, and a guard kept alive across the `format!` deadlocked
                    // the `lumen-browser` thread — the only one that drains
                    // automation commands — on every intercepted click.
                    let doc = self
                        .layout_source
                        .as_ref()
                        .and_then(|s| s.document.lock().ok());
                    if hit_belongs_to_target(&hit, node, doc.as_deref()) {
                        None
                    } else {
                        Some(hit.source_node)
                    }
                }
            };
            intercepted.map(|hit_node| {
                format!(
                    "Element click intercepted: point ({:.0}, {:.0}) hits {} instead of the target element (node {})",
                    resolved.x,
                    resolved.y,
                    self.automation_node_label(hit_node),
                    node.index(),
                )
            })
        }

        /// `<tag id>`-style label of a page node for automation diagnostics;
        /// falls back to the bare node index when the document is unavailable
        /// or the node is not an element (a text node reports its parent tag,
        /// which is what `source_node` usually is inside an inline run).
        fn automation_node_label(&self, node: lumen_dom::NodeId) -> String {
            let Some(source) = self.layout_source.as_ref() else { return format!("node {}", node.index()) };
            let Ok(doc) = source.document.lock() else { return format!("node {}", node.index()) };
            let named = match &doc.get(node).data {
                NodeData::Element { .. } => Some(node),
                _ => doc.get(node).parent,
            };
            match named.map(|n| (n, &doc.get(n).data)) {
                Some((n, NodeData::Element { name, attrs })) => {
                    let id = attrs
                        .iter()
                        .find(|a| a.name.local == "id")
                        .map(|a| format!("#{}", a.value))
                        .unwrap_or_default();
                    format!("<{}>{id} (node {})", name.local, n.index())
                }
                _ => format!("node {}", node.index()),
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
            let mut seen = std::collections::HashSet::new();
            let mut out: Vec<lumen_driver::NodeRef> =
                lumen_layout::selector_query::find_all_by_selector(lb, &doc, selector)
                    .into_iter()
                    .map(|found| {
                        seen.insert(found.node.index() as u32);
                        self.node_ref_for(&doc, found.node, found.rect)
                    })
                    .collect();
            // Inline elements (`<a>`, `<span>`, …) merge into anonymous
            // `InlineRun` boxes, so the layout walk above never sees them.
            // `query_all` walks the DOM instead; `collect_client_rects`
            // already maps each inline element ancestor to its per-line frag
            // rects — union them into the reported bounding box.
            let dom_only: Vec<lumen_dom::NodeId> =
                lumen_layout::selector_query::query_all(&doc, selector)
                    .into_iter()
                    .filter(|n| !seen.contains(&(n.index() as u32)))
                    .collect();
            if !dom_only.is_empty() {
                let rects = lumen_layout::collect_client_rects(lb, &doc);
                for nid in dom_only {
                    let Some(rs) = rects.get(&(nid.index() as u32)) else { continue };
                    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                    for r in rs {
                        x0 = x0.min(r[0]);
                        y0 = y0.min(r[1]);
                        x1 = x1.max(r[0] + r[2]);
                        y1 = y1.max(r[1] + r[3]);
                    }
                    if x0 > x1 {
                        continue;
                    }
                    out.push(self.node_ref_for(
                        &doc,
                        nid,
                        lumen_core::geom::Rect::new(x0, y0, x1 - x0, y1 - y0),
                    ));
                }
            }
            out
        }

        /// Shared `NodeRef` builder for [`Self::query_automation_nodes`] —
        /// tag/text from the DOM, rect supplied by the caller (layout box or
        /// unioned inline frag rects).
        fn node_ref_for(
            &self,
            doc: &lumen_dom::Document,
            node: lumen_dom::NodeId,
            rect: lumen_core::geom::Rect,
        ) -> lumen_driver::NodeRef {
            let tag_name = match &doc.get(node).data {
                NodeData::Element { name, .. } => name.local.to_string(),
                _ => String::new(),
            };
            let mut text_content = String::new();
            collect_automation_text(doc, node, &mut text_content);
            lumen_driver::NodeRef {
                node_id: node.index() as u32,
                tag_name,
                text_content,
                bounding_rect: rect,
            }
        }

        /// Build the accessibility tree for `AutomationCommand::A11yTree` (SDC-2).
        ///
        /// Returns `None` if no page is loaded.
        pub(crate) fn automation_a11y_tree(&self) -> Option<lumen_driver::A11yNode> {
            let source = self.layout_source.as_ref()?;
            let doc = source.document.lock().ok()?;
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            let ax_tree = lumen_a11y::build_ax_tree(&doc, doc.root(), &flat_tree);
            // Chrome nodes carry synthetic ids — resolve them back through
            // `chrome_ax_nodes_mapped` and measure each against
            // `chrome_layout` so automation gets window-space bounds for
            // toolbar/sidebar controls (they are unreachable via `query`).
            let (chrome, bounds) = match self.chrome_ax_nodes_mapped() {
                Some((root, id_map)) => {
                    let mut bounds = std::collections::HashMap::new();
                    if let Some((layout, _)) = &self.chrome_layout {
                        for (syn, doc_nid) in &id_map {
                            if let Some(b) = lumen_layout::find_box_by_node(layout, *doc_nid) {
                                bounds.insert(
                                    syn.raw(),
                                    lumen_driver::A11yBounds {
                                        x: b.rect.x as f64,
                                        y: b.rect.y as f64,
                                        width: b.rect.width as f64,
                                        height: b.rect.height as f64,
                                    },
                                );
                            }
                        }
                    }
                    (vec![root], bounds)
                }
                None => (self.chrome_ax_nodes(), std::collections::HashMap::new()),
            };
            let ax_tree = lumen_a11y::chrome::attach_chrome(ax_tree, chrome);
            Some(automation_ax_node(&ax_tree.root, &bounds))
        }

        /// Role and accessible name of the element a selector chain resolves to
        /// (`AutomationCommand::ComputedA11y`, BUG-1014). Each selector after
        /// the first is searched inside the previous match's shadow root.
        /// `None` = no page or no such element.
        pub(crate) fn automation_computed_a11y(&self, selectors: &[String]) -> Option<(String, String)> {
            let source = self.layout_source.as_ref()?;
            let doc = source.document.lock().ok()?;
            let mut scope = doc.root();
            let mut found = None;
            for (i, sel) in selectors.iter().enumerate() {
                let el = *lumen_layout::selector_query::query_all_within(&doc, scope, sel).first()?;
                found = Some(el);
                if i + 1 < selectors.len() {
                    scope = doc.shadow_root_of(el)?;
                }
            }
            let flat_tree = lumen_dom::build_flat_tree(&doc);
            Some(lumen_a11y::computed_role_and_name(&doc, found?, &flat_tree))
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

        /// Non-expired cookies of the active profile's jar for
        /// `AutomationCommand::GetCookies` (DEVX-20, `storage.getCookies`).
        /// The jar is the one pages read through `document.cookie` and requests
        /// send from. BUG-1628: хранилище разбито по сайтам верхнего уровня,
        /// а BiDi-запрос раздела не несёт — отдаём объединение всех разделов,
        /// как браузерный менеджер cookie.
        pub(crate) fn automation_get_cookies(&self) -> Vec<lumen_driver::AutomationCookie> {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
            let Ok(cookies) = self.active_cookie_jar().list_all_partitions(now) else { return Vec::new() };
            cookies
                .into_iter()
                .map(|c| lumen_driver::AutomationCookie {
                    name: c.name,
                    value: c.value,
                    domain: c.domain,
                    path: c.path,
                    secure: c.secure,
                    http_only: c.http_only,
                    same_site: match c.same_site {
                        lumen_storage::SameSite::Strict => "strict",
                        lumen_storage::SameSite::Lax => "lax",
                        lumen_storage::SameSite::None => "none",
                        lumen_storage::SameSite::Unspecified => "default",
                    }
                    .to_owned(),
                    expiry: c.expires_at.and_then(|t| u64::try_from(t).ok()).unwrap_or(0),
                })
                .collect()
        }

        /// Store `cookie` in the active profile's jar for
        /// `AutomationCommand::SetCookie` (DEVX-20, `storage.setCookie`).
        /// A script-invisible (`HttpOnly`) cookie is allowed here: unlike
        /// `document.cookie`, the automation API is a "non-script" writer.
        pub(crate) fn automation_set_cookie(&self, cookie: lumen_driver::AutomationCookie) -> Result<(), String> {
            let same_site = match cookie.same_site.as_str() {
                "strict" => lumen_storage::SameSite::Strict,
                "lax" => lumen_storage::SameSite::Lax,
                "none" => lumen_storage::SameSite::None,
                _ => lumen_storage::SameSite::Unspecified,
            };
            let stored = lumen_storage::Cookie {
                domain: cookie.domain,
                path: cookie.path,
                name: cookie.name,
                value: cookie.value,
                expires_at: (cookie.expiry > 0).then(|| i64::try_from(cookie.expiry).unwrap_or(i64::MAX)),
                secure: cookie.secure,
                http_only: cookie.http_only,
                same_site,
            };
            // BUG-1628: запись идёт в раздел текущей верхней страницы —
            // тем же ключом, что читают её запросы и `document.cookie`.
            let partition = self
                .document_resource_base()
                .and_then(|b| b.url())
                .map(|u| crate::config::connection_site(&u));
            self.active_cookie_jar().set(stored, partition.as_deref()).map_err(|e| e.to_string())
        }

        /// Remove one cookie from the active profile's jar for
        /// `AutomationCommand::DeleteCookie` (DEVX-20, `storage.deleteCookies`).
        pub(crate) fn automation_delete_cookie(&self, domain: &str, path: &str, name: &str) -> Result<(), String> {
            // BUG-1628: удаление из раздела текущей верхней страницы — см.
            // `automation_set_cookie`.
            let partition = self
                .document_resource_base()
                .and_then(|b| b.url())
                .map(|u| crate::config::connection_site(&u));
            self.active_cookie_jar().delete(domain, path, name, partition.as_deref()).map_err(|e| e.to_string())
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
            // ADR-032, срез 3: база — смещение, которое уже набрал рендер-поток.
            // Сам сдвиг уходит ему следующим кадром: смещение, заданное потоком
            // браузера, помечено `ACK_BROWSER_SET` и главнее его собственного.
            self.adopt_scroll_feedback();
            // Те же пределы, что у колеса (`max_scroll`/`max_scroll_x`): без
            // верхнего зажима automation-скролл уводил страницу за пределы
            // документа — вьюпорт и CPU-скриншот automation'а белели.
            self.scroll_x = clamp_scroll(self.scroll_x + dx, self.max_scroll_x());
            self.scroll_y = clamp_scroll(self.scroll_y + dy, self.max_scroll());
            self.issue_scroll_command();
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
            // BUG-1106: same canvas clear the live frame gets via `set_canvas_background`.
            let canvas_bg = self.layout_box.as_ref().and_then(lumen_layout::canvas_background_color);
            let image = Renderer::render_to_image_cpu_with_fonts(
                width,
                height,
                &self.display_list,
                &images,
                self.scroll_x,
                self.scroll_y,
                None,
                canvas_bg,
            )
            .map_err(|e| format!("render_to_image_cpu: {e}"))?;
            lumen_image::encode_png_rgba8(&image).map_err(|e| format!("PNG encoding: {e}"))
        }

        /// Open tabs in strip order for `list_tabs` (COMPAT-CORPUS-3-TABS).
        ///
        /// The address is the tab's page source (the requested URL; a server
        /// redirect is not reflected until the page itself navigates), the
        /// title is the page title once loaded, the strip title before that.
        pub(crate) fn automation_tab_list(&self) -> Vec<lumen_driver::TabInfo> {
            (0..self.tab_strip.len())
                .map(|idx| {
                    let entry = &self.tab_strip.tabs[idx];
                    let (url, title) = match self.closed_tab_record(idx) {
                        Some(rec) => (rec.url, rec.title),
                        None => ("about:blank".to_owned(), entry.title.clone()),
                    };
                    lumen_driver::TabInfo { id: entry.id, url, title, active: idx == self.tab_strip.active }
                })
                .collect()
        }

        /// Stable id of [`Self::automation_tab`] while a page-driven tab
        /// switch (`window.open()`, `target=_blank`) keeps it parked in
        /// `bg_tabs`; `None` when it is the active tab or no longer open
        /// (BUG-1199).
        pub(crate) fn automation_tab_in_background(&self) -> Option<usize> {
            self.automation_tab.filter(|&id| self.tab_strip.inactive_index_of(id).is_some())
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

/// An automation `Target` resolved against the live layout tree: the OS-window
/// CSS-pixel point a click should be delivered at, plus the page node that point
/// was derived from. `node` is `None` for `Target::Point` — a raw coordinate
/// names no element (BUG-1044).
pub(crate) struct ResolvedTarget {
    /// OS-window CSS x, in `handle_click_at`'s space.
    pub(crate) x: f32,
    /// OS-window CSS y, in `handle_click_at`'s space.
    pub(crate) y: f32,
    /// The element the caller named, when it named one.
    pub(crate) node: Option<lumen_dom::NodeId>,
}

/// Is `hit` the automation target `node` itself, or something inside it
/// (BUG-1044)?
///
/// `HitTestResult::path` is the hit's ancestor chain bottom-up (`path[0] ==
/// hit.node`, last entry = layout root), so `target ∈ path` answers both halves
/// at once: equal to the hit, or an ancestor of it — i.e. the hit is the target
/// or its descendant. `source_node` is checked separately because an `InlineRun`
/// reports the *element* owning the run as `node` and the text node that was
/// actually under the point as `source_node`; the latter is not on the path, so
/// a caller naming that text node by id would otherwise read as a miss.
pub(crate) fn hit_belongs_to_target(
    hit: &lumen_paint::HitTestResult,
    target: lumen_dom::NodeId,
    doc: Option<&lumen_dom::Document>,
) -> bool {
    if hit.source_node == target || hit.path.contains(&target) {
        return true;
    }
    // Inline elements own no LayoutBox: a click inside `<a>` reports the
    // frag's *text* node as `source_node`, and the run's `path` holds the
    // block container — neither equals the `<a>` itself. Walk the source
    // node's ancestors instead: the target belongs iff it contains the
    // hit text, which is exactly what a real click on the element means.
    // `doc == None` keeps the strict pre-inline behavior (tests).
    let Some(doc) = doc else { return false };
    let mut cur = doc.get(hit.source_node).parent;
    while let Some(id) = cur {
        if id == target {
            return true;
        }
        cur = doc.get(id).parent;
    }
    false
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
pub(crate) fn automation_ax_node(
    ax: &lumen_a11y::AXNode,
    bounds: &std::collections::HashMap<u32, lumen_driver::A11yBounds>,
) -> lumen_driver::A11yNode {
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
        node_id: ax.node_id.raw(),
        role: ax.role.as_str().to_owned(),
        name: ax.name.clone(),
        description: ax.description.clone(),
        placeholder: ax.placeholder.clone(),
        state,
        bounds: bounds.get(&ax.node_id.raw()).copied(),
        children: ax
            .children
            .iter()
            .map(|c| automation_ax_node(c, bounds))
            .collect(),
    }
}

/// Union of `[x, y, w, h]` frag rects into one `Rect` — `None` on empty.
/// Shared by the inline-element fallbacks in `resolve_automation_target`.
fn union_inline_rects(rs: &[[f32; 4]]) -> Option<lumen_core::geom::Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for r in rs {
        x0 = x0.min(r[0]);
        y0 = y0.min(r[1]);
        x1 = x1.max(r[0] + r[2]);
        y1 = y1.max(r[1] + r[3]);
    }
    (x0 <= x1 && y0 <= y1).then(|| lumen_core::geom::Rect::new(x0, y0, x1 - x0, y1 - y0))
}

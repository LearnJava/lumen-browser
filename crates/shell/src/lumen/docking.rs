//! Where the dockable side panels sit and how wide they are: which side each
//! one is docked to, the horizontal origin that follows from it, the drag of
//! a dock edge, and the page a sidebar renders inside itself.
//!
//! The dock arithmetic that needs no shell state is `crate::panel_layout`; the
//! panels themselves are `crate::panels`. What is here is the part that has to
//! read live state - which panel is open, what the window is currently wide,
//! and what the last drag left behind - plus the offsets every overlay display
//! list is shifted by so it clears whatever is docked.

use crate::*;

impl Lumen {
    /// The cross-dockable docked sidebars as `(persist id, visible, default
    /// width)`, in side-resolution priority order (outermost first). Each can be
    /// flipped to either window edge; [`Self::left_dock`] / [`Self::right_dock`]
    /// pick the first visible one whose effective side matches.
    ///
    /// CC-10b/CC-15-6: `ID_AI`/`ID_SIDEBAR` are **not** listed — they paint as
    /// `#rightSidebar`, a real flex sibling of `#contentArea` in the engine
    /// chrome layout, so `chrome_page_host_rect` ([`Self::page_offset`]) already
    /// reflects their width. Listing them would make
    /// [`Self::page_content_width_css`]'s horizontal scroll-clamp bound subtract
    /// the same width twice. (They were entries reporting `visible: false` while
    /// the `LUMEN_LEGACY_CHROME` rollback flag existed.)
    fn dockable_sidebars(&self) -> [(&'static str, bool, f32); 2] {
        [
            (
                panel_layout::ID_VERTICAL_TABS,
                self.vertical_tabs.visible,
                panels::vertical_tabs::PANEL_WIDTH,
            ),
            (
                panel_layout::ID_TREE_TABS,
                self.tree_tabs.visible,
                panels::tree_tabs::PANEL_WIDTH,
            ),
        ]
    }

    /// Effective dock side of a cross-dockable sidebar: its persisted override,
    /// falling back to [`panel_layout::default_dock`].
    pub(crate) fn sidebar_dock_side(&self, id: &'static str) -> panel_layout::Dock {
        self.panel_layout.dock_for(id, panel_layout::default_dock(id))
    }

    /// Left x-origin (CSS px) of a docked sidebar of `width` on `side`: left
    /// docks hug `x = 0`, right docks hug the window's right edge.
    pub(crate) fn dock_origin_x(&self, side: panel_layout::Dock, width: f32) -> f32 {
        match side {
            panel_layout::Dock::Left => 0.0,
            panel_layout::Dock::Right => (self.viewport_width_css() - width).max(0.0),
        }
    }

    /// Active left-docked sidebar as `(persist id, current width CSS px)`, or
    /// `None` when no left sidebar is visible. Honours per-panel cross-dock side
    /// overrides: a sidebar moved to the right edge no longer counts here.
    pub(crate) fn left_dock(&self) -> Option<(&'static str, f32)> {
        self.dockable_sidebars().into_iter().find_map(|(id, visible, default_w)| {
            (visible && self.sidebar_dock_side(id) == panel_layout::Dock::Left)
                .then(|| (id, self.panel_layout.width_for(id, default_w)))
        })
    }

    /// Active right-docked sidebar as `(persist id, current width CSS px)`, or
    /// `None` when none is visible. Resolved in [`Self::dockable_sidebars`]
    /// priority order: a tab sidebar flipped to the right edge precedes the AI
    /// panel, which precedes the web sidebar — mirroring
    /// [`Self::page_content_width_css`].
    fn right_dock(&self) -> Option<(&'static str, f32)> {
        self.dockable_sidebars().into_iter().find_map(|(id, visible, default_w)| {
            (visible && self.sidebar_dock_side(id) == panel_layout::Dock::Right)
                .then(|| (id, self.panel_layout.width_for(id, default_w)))
        })
    }

    /// Move the active docked sidebar to the opposite window edge, persist the
    /// choice, and relayout. The "active" sidebar is the first visible one in
    /// [`Self::dockable_sidebars`] order (tab sidebars, then AI, then web).
    /// Refuses the move when the target edge is already occupied by another
    /// docked panel (avoids overlap), and is a no-op when no sidebar is open.
    /// Returns `true` if a panel was moved.
    pub(crate) fn flip_active_sidebar_dock(&mut self) -> bool {
        let Some((id, _, _)) = self
            .dockable_sidebars()
            .into_iter()
            .find(|(_, visible, _)| *visible)
        else {
            return false;
        };
        let target = self.sidebar_dock_side(id).opposite();
        let occupied = match target {
            panel_layout::Dock::Left => self.left_dock().is_some(),
            panel_layout::Dock::Right => self.right_dock().is_some(),
        };
        if occupied {
            return false;
        }
        self.panel_layout.set_dock(id, target);
        self.panel_layout.save();
        // ADR-016 M2.2b: dock side flip shifts the content viewport; async-safe.
        self.relayout_chrome();
        true
    }

    /// Shift every rect-bearing command in `cmds` right by `dx` CSS px.
    ///
    /// Used to re-home a left-relative sidebar display list onto the right edge
    /// when its dock side is flipped. The tab sidebars emit only `FillRect`,
    /// `FillRoundedRect`, and `DrawText`; other variants are left untouched.
    pub(crate) fn offset_overlay_x(cmds: &mut lumen_paint::DisplayList, dx: f32) {
        if dx == 0.0 {
            return;
        }
        for cmd in cmds.iter_mut() {
            match cmd {
                lumen_paint::DisplayCommand::FillRect { rect, .. }
                | lumen_paint::DisplayCommand::FillRoundedRect { rect, .. }
                | lumen_paint::DisplayCommand::DrawText { rect, .. } => rect.x += dx,
                _ => {}
            }
        }
    }

    /// `(left, right)` docked-sidebar widths in CSS px (0 when not visible).
    pub(crate) fn docked_panel_offsets(&self) -> (f32, f32) {
        (
            self.left_dock().map_or(0.0, |(_, w)| w),
            self.right_dock().map_or(0.0, |(_, w)| w),
        )
    }

    /// If the cursor at `(x_css, y_css)` is within [`panel_layout::RESIZE_GRAB`]
    /// of a visible docked sidebar's inner edge (and below the tab bar), return
    /// the `(dock side, panel id)` a press there would start resizing.
    ///
    /// Left docks have their handle at `x = width`; right docks at
    /// `x = viewport_width − width`.
    pub(crate) fn resize_edge_at(&self, x_css: f32, y_css: f32) -> Option<(panel_layout::Dock, &'static str)> {
        if y_css < toolbar::CHROME_H {
            return None;
        }
        let grab = panel_layout::RESIZE_GRAB;
        if let Some((id, w)) = self.left_dock()
            && (x_css - w).abs() <= grab
        {
            return Some((panel_layout::Dock::Left, id));
        }
        if let Some((id, w)) = self.right_dock()
            && (x_css - (self.viewport_width_css() - w)).abs() <= grab
        {
            return Some((panel_layout::Dock::Right, id));
        }
        None
    }

    /// Apply an in-flight docked-panel resize drag: turn the cursor x into a new
    /// width for the dragged dock, store it (clamped) in [`Self::panel_layout`],
    /// and relayout the page. Returns `true` if the width changed.
    pub(crate) fn drag_panel_resize(&mut self, x_css: f32) -> bool {
        let Some((dock, id)) = self.panel_resize else {
            return false;
        };
        let new_w = dock.width_from_cursor(x_css, self.viewport_width_css());
        if self.panel_layout.set_width(id, new_w) {
            // ADR-016 M2.2b: docked-panel resize changes the content viewport
            // width; async-safe (the drag edge itself follows the cursor via the
            // immediate redraw, only the page reflow underneath is deferred).
            self.relayout_chrome();
            true
        } else {
            false
        }
    }

    /// Open the sidebar with `url` and populate it with a freshly-laid-out page.
    ///
    /// Parses `html_bytes` as HTML, lays it out at [`PANEL_WIDTH`]-wide viewport,
    /// and stores the display list in the sidebar panel.  Triggers a relayout of
    /// the main page when the sidebar becomes visible (page width changes).
    pub(crate) fn open_sidebar_page(&mut self, url: String, html_bytes: &[u8], page_title: String) {
        let was_visible = self.sidebar.visible;
        self.sidebar.open(url.clone());

        // Decode bytes and parse HTML.
        let encoding = lumen_encoding::detect(html_bytes, None);
        let source_str = lumen_encoding::decode(encoding, html_bytes);
        let doc = lumen_html_parser::parse(&source_str);
        let doc_title = if page_title.is_empty() {
            extract_title(&doc).unwrap_or_default()
        } else {
            page_title
        };

        // Collect inline <style> blocks (no external CSS fetch for sidebar).
        let css_text = extract_style_blocks(&doc);
        let sheet = lumen_css_parser::parse(&css_text);

        let doc_arc = Arc::new(Mutex::new(doc));
        let src = LayoutSource {
            document: doc_arc,
            stylesheet: Arc::new(sheet),
            html_source: None,
            // Sidebar panel, not the main navigable page — not bfcache-tracked.
            cache_control_no_store: false,
            // BUG-743: the sidebar page runs no scripts, so no `<style>` can
            // appear in it after this build.
            dynamic_css: None,
        };

        let sidebar_vp = Size::new(
            self.panel_layout
                .width_for(panel_layout::ID_SIDEBAR, panels::sidebar_panel::PANEL_WIDTH),
            self.viewport_height_css().max(100.0),
        );
        let (dl, _lb) = relayout_page(&src, sidebar_vp, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        let content_h = content_height_of(&dl);
        self.sidebar.set_page(dl, doc_title, content_h);
        // Retain the parsed source so a later drag-resize can reflow the page to
        // the new width (F2-6) instead of stretching the frozen display list.
        self.sidebar_source = Some(src);

        if !was_visible {
            // ADR-016 M2.2b: the sidebar becoming visible narrows the main page's
            // content viewport; async-safe chrome-inset relayout.
            self.relayout_chrome();
        }
        self.request_redraw();
    }

    /// Reflow the web sidebar page to the current sidebar width.
    ///
    /// Re-runs layout over the retained [`Self::sidebar_source`] at the panel's
    /// active `panel_layout` width, replacing the frozen display list while
    /// preserving the title and clamping the scroll offset. No-op when the
    /// sidebar has no open page. Called on a sidebar resize drag release (F2-6).
    pub(crate) fn relayout_sidebar(&mut self) {
        let Some(src) = self.sidebar_source.as_ref() else {
            return;
        };
        let width = self
            .panel_layout
            .width_for(panel_layout::ID_SIDEBAR, panels::sidebar_panel::PANEL_WIDTH);
        let vp = Size::new(width, self.viewport_height_css().max(100.0));
        let (dl, _lb) = relayout_page(src, vp, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        let content_h = content_height_of(&dl);
        self.sidebar.update_page(dl, content_h);
        self.request_redraw();
    }
}

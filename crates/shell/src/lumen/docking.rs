//! The page a sidebar renders inside itself.
//!
//! UX-CHROME-PANELS 15: the cross-dock machinery (dock side, edge resize drag,
//! window offsets) is gone - every side panel is a flex sibling of
//! `#contentArea` in the engine chrome, which counts their width itself.

use crate::*;

impl Lumen {
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
        // GAP-CSPENF срез 21: sidebar has no CSP concept of its own (a
        // separate navigable, not the main document's cascade) — not gated.
        let (css_text, _blocked) = extract_style_blocks(&doc, None);
        let sheet = lumen_css_parser::parse(&css_text);

        let doc_arc = Arc::new(Mutex::new(doc));
        let src = LayoutSource {
            document: doc_arc,
            stylesheet: Arc::new(sheet),
            // CSSOM-1 срез 2: sidebar-панель не строит per-узловой реестр —
            // document.styleSheets в ней не нужен.
            stylesheet_nodes: Arc::new(Vec::new()),
            html_source: None,
            // Sidebar panel, not the main navigable page — not bfcache-tracked.
            cache_control_no_store: false,
            // BUG-743: the sidebar page runs no scripts, so no `<style>` can
            // appear in it after this build.
            dynamic_css: None,
        };

        let sidebar_vp = Size::new(
            panels::sidebar_panel::PANEL_WIDTH,
            self.viewport_height_css().max(100.0),
        );
        let (dl, _lb) = relayout_page(&src, sidebar_vp, &*self.hyp_provider, self.dark_mode, &self.web_fonts);
        let content_h = content_height_of(&dl);
        self.sidebar.set_page(dl, doc_title, content_h);

        if !was_visible {
            // ADR-016 M2.2b: the sidebar becoming visible narrows the main page's
            // content viewport; async-safe chrome-inset relayout.
            self.relayout_chrome();
        }
        self.request_redraw();
    }
}

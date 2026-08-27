//! The two alternative renderings of the page that is already open: reader
//! view and view-source.
//!
//! Both work by swapping the live document for one the shell generates
//! (`crate::reader_view`, `crate::source_view`) while keeping the tab, its
//! history entry and its address bar text - which is why they are `Lumen`
//! methods and not a plain navigation: leaving reader view has to put the
//! original document back, not re-fetch it.

use crate::*;

impl Lumen {
    /// Reload the bookmark list from storage into the panel cache.
    ///
    /// Call this after every bookmark mutation (add / delete / move) so the
    /// panel renders up-to-date rows on the next redraw.
    /// Reload the read-later entry list from the in-memory store into the panel cache.
    ///
    /// Called after every save/delete and when the panel opens.  Shows the 50
    /// most recent items (unread first, then read, then archived).
    /// Toggle Reader View (В§D-3, F9).
    ///
    /// When entering reader mode: extracts the article region from the current
    /// page's HTML source, wraps it in a clean reading template, and re-renders
    /// it as an in-memory `PageSource::Snapshot` without a network round-trip.
    /// The original source is stashed in `reader_original_source`.
    ///
    /// When exiting: restores the stashed source and reloads.
    pub(crate) fn toggle_reader_view(&mut self) {
        if let Some(original) = self.reader_original_source.take() {
            // Exit reader mode вЂ” restore original page.
            self.source = original;
            self.reload();
            return;
        }

        // Enter reader mode вЂ” extract article from current HTML source.
        let html = match self.layout_source.as_ref().and_then(|ls| ls.html_source.as_deref()) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return, // nothing to extract from
        };

        let Some(article) = reader_view::extract_article(&html) else { return };
        let reader_html = reader_view::build_reader_html(&article);

        let base_url = self.source.url_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "about:reader".to_owned());

        self.reader_original_source = Some(self.source.clone());
        self.source = PageSource::Snapshot { html: reader_html, base_url };
        self.reload();
    }

    /// Show syntax-highlighted source of the current page (Ctrl+U, В§D-2).
    ///
    /// Uses the already-parsed HTML stored in `layout_source.html_source`.
    /// No-op when the page has no HTML source (e.g. empty tab).
    pub(crate) fn show_view_source(&mut self) {
        let html = match self.layout_source.as_ref().and_then(|ls| ls.html_source.as_deref()) {
            Some(s) if !s.is_empty() => s.to_owned(),
            _ => return,
        };
        let url = self.source.url_str()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "about:source".to_owned());
        let source_html = source_view::build_view_source_html(&url, &html);
        self.navigate_to(PageSource::Snapshot {
            html: source_html,
            base_url: format!("view-source:{url}"),
        });
    }

    /// Fetch `url` and display its raw bytes as syntax-highlighted source (В§D-2).
    ///
    /// Used when the user types `view-source:<url>` in the address bar.
    pub(crate) fn show_view_source_for_url(&mut self, url: &str) {
        let source = PageSource::from_arg(Some(url));
        let sink = Arc::clone(&self.event_sink);
        let jar = self.active_cookie_jar();
        match source.load_bytes(sink, Some(jar)) {
            Ok(raw) => {
                let html_str = String::from_utf8_lossy(&raw.bytes).into_owned();
                let source_html = source_view::build_view_source_html(url, &html_str);
                self.navigate_to(PageSource::Snapshot {
                    html: source_html,
                    base_url: format!("view-source:{url}"),
                });
            }
            Err(e) => {
                eprintln!("view-source: РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РіСЂСѓР·РёС‚СЊ {url}: {e}");
            }
        }
    }
}

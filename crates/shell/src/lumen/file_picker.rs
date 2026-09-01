//! The OS file dialog behind `<input type="file">`.
//!
//! The dialog itself is `crate::platform::file_dialog` and blocks the event
//! loop for as long as it is open; what is here is the shell side of one pick:
//! reading `accept`/`multiple` off the DOM, and handing the result to the page
//! as opaque tokens rather than filesystem paths (BUG-371), which is why the
//! delivery goes through `lumen_js::file_input` instead of straight into JS.

use crate::*;

impl Lumen {
    /// Open the OS native file-picker for `<input type="file">` at `id`.
    ///
    /// Reads the `accept` and `multiple` attributes from the DOM, invokes the
    /// platform file dialog (blocking), then delivers the result to JS via
    /// `_lumen_deliver_file_list(nid, json)`.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub(crate) fn open_file_picker(&mut self, id: NodeId) {
        let (accept, multiple) = if let Some(src) = self.layout_source.as_ref() {
            let doc = src.document.lock().unwrap();
            let n = doc.get(id);
            let accept = n.get_attr("accept").unwrap_or("").to_string();
            let multiple = n.get_attr("multiple").is_some();
            (accept, multiple)
        } else {
            (String::new(), false)
        };
        let entries = platform::file_dialog::open_file_dialog(&accept, multiple);
        if entries.is_empty() {
            // User cancelled — no event fired (HTML LS §4.10.5.1.16.3 step 3).
            return;
        }
        #[cfg(feature = "v8")]
        if self.js_present {
            // Register each path with an opaque token before delivering to JS.
            // JS never receives raw filesystem paths — only tokens.
            // BUG-371: the grant is bound to an origin, and only the origin the
            // page's own bindings were installed with can redeem it. Read back
            // from the install path rather than re-derived from `self.source` —
            // a mismatch would not fail loudly, every read would just come back
            // empty.
            let origin = lumen_js::file_input::active_document_origin();
            let tokens: Vec<String> = entries
                .iter()
                .map(|e| lumen_js::file_input::register_file_token(&e.path, &origin))
                .collect();
            let json = platform::file_dialog::entries_to_json_with_tokens(&entries, &tokens);
            // ADR-016 M2.2c-2d: fire-and-forget file-list delivery через маршрутизатор —
            // токены регистрируются на UI-потоке (до постановки в очередь), сам `eval_js`
            // под флагом уходит off-UI-thread, без флага байт-идентично прежнему `js.eval_js`.
            let script = format!("_lumen_deliver_file_list({}, {})", id.index(), json);
            route_eval_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), script);
        }
        #[cfg(not(feature = "v8"))]
        let _ = entries;
    }
}

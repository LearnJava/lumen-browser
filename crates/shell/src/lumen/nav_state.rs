//! The Navigation API side of a navigation: pushing the shell's own history
//! stacks into JS and firing what the page is owed once a navigation settles.
//!
//! Everything here answers the question “what does the page get to see”, not
//! “what does the shell do” — the stacks themselves are shuffled by
//! [`super::navigation`], and `commit_nav_state` is the single point every
//! navigation path funnels through once `source`/`display_url` are final.

use crate::*;

impl Lumen {
    /// Commit the current navigation state to the JS side so that
    /// `window.navigation.entries()` and `currentEntry` reflect the truth.
    ///
    /// Builds a serialised JSON of `nav_back` + current + `nav_fwd` with the
    /// shell-assigned `nav_key` for each entry and pushes it via
    /// `_lumen_navigation_set_state`.
    ///
    /// BUG-352: also the single point every navigation path (full-document
    /// load, same-document popstate, JS-intercepted push/replace) funnels
    /// through once `self.source`/`self.display_url` has its final value —
    /// so it doubles as the trigger to refresh the engine-drawn chrome's
    /// `#omniInput` (`relayout_chrome_host`/`chrome_omnibox_value` reads
    /// `current_display_url()`). Without this, the omnibox only ever
    /// refreshed from the omnibox's own key handler (CC-7) — every other
    /// way the URL can change (a clicked link, `history.back()`/`forward()`,
    /// BiDi/MCP `navigate`, which is exactly `wptrunner`'s navigation model)
    /// left it showing whatever URL was on screen at the last keystroke,
    /// click or resize, indefinitely.
    pub(crate) fn commit_nav_state(&mut self) {
        fn state_value(raw: Option<&str>) -> serde_json::Value {
            match raw {
                Some(s) => serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.to_owned())),
                None => serde_json::Value::Null,
            }
        }
        // FRAME-4: a `frame_target` entry is a subframe navigation step, not
        // a page one — per spec `window.navigation` only ever sees entries of
        // its OWN Document, so these are skipped here rather than reported
        // as bogus duplicate-URL entries. `idx` (the current entry's position)
        // is counted over the FILTERED list for the same reason.
        let mut entries: Vec<serde_json::Value> = Vec::new();
        let mut idx = 0usize;
        for e in &self.nav_back {
            if e.frame_target.is_some() {
                continue;
            }
            entries.push(serde_json::json!({
                "url": e.source.url_str().unwrap_or(""),
                "key": e.nav_key,
                "id": format!("id-{}", e.nav_key.strip_prefix("nav-").unwrap_or("0")),
                "state": state_value(e.same_doc_state_json.as_deref()),
            }));
            idx += 1;
        }
        let cur_url = self.source.url_str().unwrap_or("");
        let cur_key = self.current_nav_key.clone();
        entries.push(serde_json::json!({
            "url": cur_url,
            "key": cur_key,
            "id": format!("id-{}", cur_key.strip_prefix("nav-").unwrap_or("0")),
            "state": state_value(Some(&self.current_history_state_json)),
        }));
        for e in &self.nav_fwd {
            if e.frame_target.is_some() {
                continue;
            }
            entries.push(serde_json::json!({
                "url": e.source.url_str().unwrap_or(""),
                "key": e.nav_key,
                "id": format!("id-{}", e.nav_key.strip_prefix("nav-").unwrap_or("0")),
                "state": state_value(e.same_doc_state_json.as_deref()),
            }));
        }
        let state = serde_json::json!({ "entries": entries, "index": idx });
        // The native binding takes a String argument, so the JSON text must
        // be embedded as a JS string literal (double encoding) — passing a
        // bare object literal makes the arg conversion fail and the state
        // silently never reaches the runtime.
        let Ok(json) = serde_json::to_string(&state) else { return };
        let Ok(quoted) = serde_json::to_string(&json) else { return };
        // ADR-016 M2.2d: fire-and-forget eval via route_eval_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_eval_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            format!("_lumen_navigation_set_state({quoted})"),
        );
        // BUG-352: see doc comment above — keeps the omnibox in sync with
        // every navigation, not just omnibox-driven ones. No-op off the
        // flag (`relayout_chrome_host` early-returns when `chrome_doc`/the
        // renderer aren't ready yet, e.g. the very first call before the
        // window exists).
        self.relayout_chrome_host();
    }

    pub(crate) fn fire_navigate_success(&self) {
        // ADR-016 M2.2d: fire-and-forget void via route_task_js (off-UI-thread
        // under LUMEN_ENGINE_THREAD=1; byte-identical sync call when off).
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.fire_navigate_success();
        });
    }

    pub(crate) fn fire_navigate_error(&self) {
        // ADR-016 M2.2d: fire-and-forget void via route_task_js.
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.fire_navigate_error();
        });
    }

    pub(crate) fn fire_current_entry_change(&self) {
        // ADR-016 M2.2d: fire-and-forget void via route_task_js.
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), |j| {
            j.fire_current_entry_change();
        });
    }

    /// Apply a same-document `history.go(n)` destination whose containing
    /// document had to be (re)loaded first (see `pending_post_reload_traversal`):
    /// fires `popstate` with `state_json`, updates the address bar to
    /// `display_url`, and fires `currententrychange` — the same tail as the
    /// ordinary same-document branch in `navigate_back`/`navigate_forward`,
    /// just run once the correct document's JS runtime actually exists.
    pub(crate) fn apply_post_reload_traversal(&mut self, state_json: String, display_url: Option<String>) {
        self.current_history_state_json = state_json.clone();
        self.display_url = display_url.clone();
        let url = display_url.unwrap_or_default();
        route_task_js(self.engine_thread.as_ref(), self.js_ctx.as_ref(), move |j| {
            j.fire_popstate(&state_json, &url);
        });
        self.fire_current_entry_change();
        self.request_redraw();
    }
}

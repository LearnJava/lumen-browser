//! The browser's session-history entries: [`NavEntry`] (one step of the
//! back/forward stack, with the two helpers that shuttle the cursor through
//! a multi-step `history.go(n)`) and [`JsNavigateRequest`] (a navigation the
//! page asked for from script, parked until `about_to_wait` runs it).
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3b); behaviour and
//! signatures are unchanged.

use crate::*;

/// Р—Р°РїРёСЃСЊ РІ СЃС‚РµРєРµ РёСЃС‚РѕСЂРёРё РЅР°РІРёРіР°С†РёРё Р±СЂР°СѓР·РµСЂР°.
pub(crate) struct NavEntry {
    pub(crate) source: PageSource,
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
    /// Overrides `source.url_str()` in the address bar for same-document entries.
    /// `None` for full-document navigation entries; `Some(url)` when this entry
    /// was created by `history.pushState` (the virtual URL at that point).
    pub(crate) display_url: Option<String>,
    /// State JSON for a same-document `history.pushState` entry.
    /// `None` в†’ full navigation (popping this entry reloads the page).
    /// `Some(json)` в†’ same-document (popping fires `popstate` with this state).
    pub(crate) same_doc_state_json: Option<String>,
    /// Navigation API key assigned by the shell for this entry.
    /// Used by `navigation.traverseTo(key)` and reported via
    /// `_lumen_navigation_entries_json` so JS can correlate entries.
    pub(crate) nav_key: String,
}

impl NavEntry {
    /// Move the history cursor one intermediate hop toward `back` (true) or
    /// forward (false) WITHOUT rendering or firing events вЂ” the building block of
    /// a multi-step `history.go(n)` (see `Lumen::navigate_by`). The current entry
    /// `cur` is pushed onto the opposite stack and the popped target entry is
    /// returned as the new current.
    ///
    /// The caller MUST have range-checked that the source stack is non-empty;
    /// `pop` is therefore expected to succeed.
    #[allow(clippy::expect_used)]  // СѓРЅР°СЃР»РµРґРѕРІР°РЅРѕ, docs/lint-policy.md В§10
    pub(crate) fn shift_history_entry(
        nav_back: &mut Vec<NavEntry>,
        nav_fwd: &mut Vec<NavEntry>,
        cur: NavEntry,
        back: bool,
    ) -> NavEntry {
        if back {
            let popped = nav_back.pop().expect("source stack must be non-empty");
            nav_fwd.push(cur);
            popped
        } else {
            let popped = nav_fwd.pop().expect("source stack must be non-empty");
            nav_back.push(cur);
            popped
        }
    }

    /// Shuttle `cur` through `steps - 1` intermediate hops via
    /// [`Self::shift_history_entry`], additionally tracking whether any hop
    /// crossed a full-document entry (`same_doc_state_json.is_none()`) along
    /// the way вЂ” i.e. whether the entry one hop short of the final
    /// destination belongs to a different loaded document than `cur` started
    /// in. `Lumen::navigate_by` uses the returned flag to decide whether a
    /// same-document destination needs `Lumen::pending_post_reload_traversal`
    /// (the loaded document is stale relative to it) instead of firing
    /// `popstate` directly.
    pub(crate) fn shift_multi_step(
        nav_back: &mut Vec<NavEntry>,
        nav_fwd: &mut Vec<NavEntry>,
        mut cur: NavEntry,
        steps: usize,
        back: bool,
    ) -> (NavEntry, bool) {
        let mut crossed_document = false;
        for _ in 1..steps {
            cur = Self::shift_history_entry(nav_back, nav_fwd, cur, back);
            if cur.same_doc_state_json.is_none() {
                crossed_document = true;
            }
        }
        (cur, crossed_document)
    }
}

/// РќР°РІРёРіР°С†РёРѕРЅРЅС‹Р№ Р·Р°РїСЂРѕСЃ РѕС‚ JS (location.href=, assign, replace, reload).
/// РҐСЂР°РЅРёС‚СЃСЏ РІ `Lumen::pending_js_navigate` Рё РІС‹РїРѕР»РЅСЏРµС‚СЃСЏ РІ `about_to_wait`.
#[cfg_attr(not(feature = "v8"), allow(dead_code))]
pub(crate) enum JsNavigateRequest {
    /// РџРµСЂРµР№С‚Рё РЅР° URL, РґРѕР±Р°РІРёС‚СЊ Р·Р°РїРёСЃСЊ РІ РёСЃС‚РѕСЂРёСЋ.
    Push(String),
    /// РџРµСЂРµР№С‚Рё РЅР° URL, Р·Р°РјРµРЅРёС‚СЊ С‚РµРєСѓС‰СѓСЋ Р·Р°РїРёСЃСЊ РёСЃС‚РѕСЂРёРё (Р±РµР· push).
    Replace(String),
    /// РџРµСЂРµР·Р°РіСЂСѓР·РёС‚СЊ С‚РµРєСѓС‰СѓСЋ СЃС‚СЂР°РЅРёС†Сѓ.
    Reload,
    /// Р’С‹РїРѕР»РЅРёС‚СЊ РѕС‚РїСЂР°РІРєСѓ С„РѕСЂРјС‹, Р·Р°РїСЂРѕС€РµРЅРЅСѓСЋ СЃС‚СЂР°РЅРёС†РµР№ РёР· СЃРєСЂРёРїС‚Р°
    /// (`form.submit()` / `form.requestSubmit()`, BUG-383).
    SubmitForm {
        /// РРЅРґРµРєСЃ СѓР·Р»Р° `<form>`.
        form: u32,
        /// РРЅРґРµРєСЃ СѓР·Р»Р°-СЃР°Р±РјРёС‚С‚РµСЂР°, Р»РёР±Рѕ `-1`, РµСЃР»Рё РµРіРѕ РЅРµС‚.
        submitter: i32,
    },
}

/// Pending intercepted navigation awaiting handler completion.
pub(crate) enum PendingIntercepted {
    Push { url: String, handler_started: bool },
    Replace { url: String, handler_started: bool },
    Back { handler_started: bool },
    Forward { handler_started: bool },
}

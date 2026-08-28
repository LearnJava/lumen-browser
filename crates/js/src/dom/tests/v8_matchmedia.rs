//! Тесты `v8_matchmedia`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`]: same fixture document, same
/// `install_dom` argument list, same `_LUMEN_EXTENSION_ACTIVE` pre-eval.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// ── window.matchMedia / MediaQueryList (CSS MQ L4 §4.2) ───────────────────

#[test]
fn match_media_exists_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval("typeof window.matchMedia === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt.eval("typeof matchMedia === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt.eval("typeof window.MediaQueryList === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt.eval("typeof window.MediaQueryListEvent === 'function'").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn match_media_screen_always_matches() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    let r = rt.eval("matchMedia('screen').matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn match_media_min_width_matches_when_viewport_wide_enough() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    let r = rt.eval("matchMedia('(min-width: 100px)').matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn match_media_min_width_misses_when_viewport_too_narrow() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    let r = rt.eval("matchMedia('(min-width: 900px)').matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn match_media_max_width_matches() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    let r = rt.eval("matchMedia('(max-width: 1000px)').matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn match_media_print_does_not_match_screen() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    let r = rt.eval("matchMedia('print').matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
}

#[test]
fn match_media_returns_object_with_media_property() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    let r = rt
        .eval("matchMedia('(min-width: 500px)').media")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("(min-width: 500px)".into()));
    let r = rt
        .eval("matchMedia('(min-width: 500px)') instanceof MediaQueryList")
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn match_media_add_remove_listener_noop_when_no_change() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    // Legacy addListener/removeListener API (deprecated but widely used).
    rt.eval(
        r"
                var _mm_calls = 0;
                var _mm = matchMedia('(min-width: 100px)');
                var _mm_cb = function() { _mm_calls++; };
                _mm.addListener(_mm_cb);
                _mm.removeListener(_mm_cb);
                ",
    )
    .unwrap();
    let r = rt.eval("_mm_calls").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn match_media_change_event_fires_when_matches_flips() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        r"
                var _mm_calls = 0;
                var _mm_last_matches = null;
                var _mm_last_media = null;
                var _mm = matchMedia('(min-width: 900px)');
                _mm.addEventListener('change', function(ev) {
                    _mm_calls++;
                    _mm_last_matches = ev.matches;
                    _mm_last_media = ev.media;
                });
                ",
    )
    .unwrap();
    // Initial state: not matching (800 < 900).
    let r = rt.eval("_mm.matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
    // Viewport grows to 1000 — now matches.
    rt.eval("_lumen_deliver_media_changes(1000, 600, false, false)").unwrap();
    let r = rt.eval("_mm_calls").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
    let r = rt.eval("_mm_last_matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt.eval("_mm_last_media").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("(min-width: 900px)".into()));
    let r = rt.eval("_mm.matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn match_media_change_event_does_not_fire_when_no_flip() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        r"
                var _mm_calls = 0;
                var _mm = matchMedia('(min-width: 100px)');
                _mm.addEventListener('change', function() { _mm_calls++; });
                ",
    )
    .unwrap();
    // Already matches; reapply same context → no flip → no fire.
    rt.eval("_lumen_deliver_media_changes(900, 600, false, false)").unwrap();
    rt.eval("_lumen_deliver_media_changes(1200, 600, false, false)").unwrap();
    let r = rt.eval("_mm_calls").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

#[test]
fn match_media_onchange_callback_fires() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        r"
                var _mm_onchange_calls = 0;
                var _mm = matchMedia('(min-width: 1000px)');
                _mm.onchange = function() { _mm_onchange_calls++; };
                ",
    )
    .unwrap();
    rt.eval("_lumen_deliver_media_changes(1100, 600, false, false)").unwrap();
    let r = rt.eval("_mm_onchange_calls").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn match_media_prefers_color_scheme_dark() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    // Initially: dark = false (default).
    let r = rt.eval("matchMedia('(prefers-color-scheme: dark)').matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(false));
    // Flip to dark via the shell delivery path.
    rt.eval(
        r"
                var _mm_dark_calls = 0;
                var _mm_dark = matchMedia('(prefers-color-scheme: dark)');
                _mm_dark.addEventListener('change', function(ev) { _mm_dark_calls++; });
                _lumen_deliver_media_changes(800, 600, true, false);
                ",
    )
    .unwrap();
    let r = rt.eval("_mm_dark.matches").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt.eval("_mm_dark_calls").unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(1.0));
}

#[test]
fn match_media_event_is_media_query_list_event() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        r"
                var _mm_ev_type = null;
                var _mm_ev_is_mqle = false;
                var _mm_ev_is_event = false;
                var _mm = matchMedia('(min-width: 1500px)');
                _mm.addEventListener('change', function(ev) {
                    _mm_ev_type = ev.type;
                    _mm_ev_is_mqle = ev instanceof MediaQueryListEvent;
                    _mm_ev_is_event = ev instanceof Event;
                });
                _lumen_deliver_media_changes(1600, 600, false, false);
                ",
    )
    .unwrap();
    let r = rt.eval("_mm_ev_type").unwrap();
    assert_eq!(r, lumen_core::JsValue::String("change".into()));
    let r = rt.eval("_mm_ev_is_mqle").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
    let r = rt.eval("_mm_ev_is_event").unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

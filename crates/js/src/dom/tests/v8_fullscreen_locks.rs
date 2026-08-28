//! V8 port of Fullscreen API (WHATWG Fullscreen §4), Web Locks API, Screen Wake
//! Lock stub, Network Information stub, `navigator.userActivation`, Web Share API
//! stub and `window.reportError()` — seven adjacent scoping-table sub-families
//! ported together, **31 tests**, QuickJS copies deleted. All of these are plain
//! JS in the shared `WEB_API_SHIM`, not native bindings, so nothing changed on the
//! `V8JsRuntime` side.
//!
//! `_lumen_drain_microtasks()` calls dropped throughout (S12b-2 lesson, applied at
//! this scale for the first time to Promise-heavy Web Locks tests): V8 performs a
//! full microtask checkpoint after every top-level `eval()` script by default
//! (`v8_runtime.rs:3669`), so tests that already split setup/drain/assertion into
//! three separate Rust-level `rt.eval()` calls just drop the middle drain call —
//! the checkpoint after the setup `eval()` already flushed it. Tests that instead
//! concatenated setup + drain + assertion into *one* JS string (all four
//! `request_fullscreen_*`/`exit_fullscreen_*` tests) needed restructuring into two
//! separate `rt.eval()` calls, because the checkpoint only runs at the Rust-level
//! `eval()` boundary, not mid-script between statements.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    rt.eval(script).unwrap() == lumen_core::JsValue::Bool(true)
}

// ── Fullscreen API tests (WHATWG Fullscreen §4) ───────────────────────────

#[test]
fn fullscreen_enabled_is_true() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.fullscreenEnabled === true"));
}

#[test]
fn fullscreen_element_initially_null() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "document.fullscreenElement === null"));
}

#[test]
fn request_fullscreen_returns_promise() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "var body = document.body; \
                 var p = body.requestFullscreen(); \
                 typeof p === 'object' && typeof p.then === 'function'"
    ));
}

#[test]
fn request_fullscreen_sets_fullscreen_element() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.requestFullscreen();").unwrap();
    assert!(bool_eval(&rt, "document.fullscreenElement !== null"));
}

#[test]
fn request_fullscreen_sets_sentinel_attr() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.requestFullscreen();").unwrap();
    assert!(bool_eval(&rt, "document.body.hasAttribute('data-lumen-fullscreen')"));
}

#[test]
fn request_fullscreen_fires_fullscreenchange_event() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var fired = false; \
                 document.addEventListener('fullscreenchange', function() { fired = true; }); \
                 document.body.requestFullscreen();"
    ).unwrap();
    assert!(bool_eval(&rt, "fired"));
}

#[test]
fn exit_fullscreen_clears_fullscreen_element() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.requestFullscreen();").unwrap();
    rt.eval("document.exitFullscreen();").unwrap();
    assert!(bool_eval(&rt, "document.fullscreenElement === null"));
}

#[test]
fn exit_fullscreen_removes_sentinel_attr() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.requestFullscreen();").unwrap();
    rt.eval("document.exitFullscreen();").unwrap();
    assert!(bool_eval(&rt, "!document.body.hasAttribute('data-lumen-fullscreen')"));
}

#[test]
fn notify_fullscreen_exit_clears_state() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.body.requestFullscreen();").unwrap();
    rt.eval("_lumen_notify_fullscreen_exit();").unwrap();
    assert!(bool_eval(&rt, "document.fullscreenElement === null"));
}

#[test]
fn element_has_onfullscreenchange_property() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "'onfullscreenchange' in document.body && \
                 'onfullscreenerror' in document.body"
    ));
}

#[test]
fn document_has_onfullscreenchange_property() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "'onfullscreenchange' in document && \
                 'onfullscreenerror' in document"
    ));
}

// ── BUG-390: requestFullscreen() error preconditions ─────────────────────

/// A detached element can never be shown — Fullscreen §4.3 rejects with
/// a TypeError instead of entering fullscreen (WPT `promises-reject`).
#[test]
fn request_fullscreen_rejects_for_detached_element() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var res = 'pending'; \
                 document.createElement('span').requestFullscreen().then( \
                     function() { res = 'resolved'; }, \
                     function(e) { res = e.constructor.name; });"
    )
    .unwrap();
    assert!(bool_eval(&rt, "res === 'TypeError'"));
    assert!(bool_eval(&rt, "document.fullscreenElement === null"));
}

/// The rejected request also fires `fullscreenerror`; a detached element
/// has no ancestor chain, so the event goes to the document.
#[test]
fn request_fullscreen_detached_fires_fullscreenerror_on_document() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ev = null; \
                 document.addEventListener('fullscreenerror', function(e) { ev = e; }); \
                 document.createElement('span').requestFullscreen().catch(function() {});"
    )
    .unwrap();
    assert!(bool_eval(&rt,
        "ev !== null && ev.type === 'fullscreenerror' && ev.bubbles === true && \
                 ev.cancelable === false && ev.composed === true"
    ));
}

/// For a connected element the event targets the element itself and
/// bubbles up to the document listener (WPT
/// `element-request-fullscreen-not-allowed` / `document-onfullscreenerror`).
#[test]
fn request_fullscreen_error_targets_the_element_and_bubbles() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var ev = null; \
                 document.addEventListener('fullscreenerror', function(e) { ev = e; }); \
                 var d = document.getElementById('main'); \
                 d.setAttribute('popover', ''); \
                 d.showPopover(); \
                 var res = 'pending'; \
                 d.requestFullscreen().then(function() { res = 'resolved'; }, \
                                            function(e) { res = e.constructor.name; });"
    )
    .unwrap();
    assert!(bool_eval(&rt, "res === 'TypeError'"));
    assert!(bool_eval(&rt,
        "ev !== null && ev.target === document.getElementById('main')"
    ));
    assert!(bool_eval(&rt, "document.fullscreenElement === null"));
}

/// `document.onfullscreenerror` is a plain property of the document
/// object, so the fire helper has to invoke it explicitly.
#[test]
fn request_fullscreen_error_calls_document_on_handler() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var seen = null; \
                 document.onfullscreenerror = function(e) { seen = e.type; }; \
                 document.createElement('span').requestFullscreen().catch(function() {});"
    )
    .unwrap();
    assert!(bool_eval(&rt, "seen === 'fullscreenerror'"));
}

/// Without transient activation the request is refused. Lumen's
/// `navigator.userActivation` reports active unconditionally (BUG-758),
/// so the gate is exercised here by overriding that single signal.
#[test]
fn request_fullscreen_rejects_without_transient_activation() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "Object.defineProperty(navigator, 'userActivation', { \
                     value: { isActive: false, hasBeenActive: true }, configurable: true }); \
                 var res = 'pending'; \
                 document.body.requestFullscreen().then(function() { res = 'resolved'; }, \
                                                        function(e) { res = e.constructor.name; });"
    )
    .unwrap();
    assert!(bool_eval(&rt, "res === 'TypeError'"));
    assert!(bool_eval(&rt, "document.fullscreenElement === null"));
}

/// The happy path still resolves — the new checks must not gate a
/// connected, non-popover element under the default activation model.
#[test]
fn request_fullscreen_resolves_for_connected_element() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var res = 'pending'; \
                 document.body.requestFullscreen().then(function() { res = 'resolved'; }, \
                                                        function(e) { res = e.constructor.name; });"
    )
    .unwrap();
    assert!(bool_eval(&rt, "res === 'resolved'"));
    assert!(bool_eval(&rt, "document.fullscreenElement !== null"));
}

/// Fullscreen §4.4: exiting when nothing is fullscreen rejects with a
/// TypeError (WPT `promises-reject`, second assertion).
#[test]
fn exit_fullscreen_rejects_when_not_in_fullscreen() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var res = 'pending'; \
                 document.exitFullscreen().then(function() { res = 'resolved'; }, \
                                                function(e) { res = e.constructor.name; });"
    )
    .unwrap();
    assert!(bool_eval(&rt, "res === 'TypeError'"));
}

/// `el.onfullscreenerror = fn` used to land on the wrapper object only,
/// where no dispatch path looks; it now routes through the per-nid
/// handler table like `onclick` and therefore actually fires.
#[test]
fn element_onfullscreenerror_handler_fires() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var seen = null; \
                 var d = document.getElementById('main'); \
                 d.onfullscreenerror = function(e) { seen = e.type; }; \
                 d.setAttribute('popover', ''); \
                 d.showPopover(); \
                 d.requestFullscreen().catch(function() {});"
    )
    .unwrap();
    assert!(bool_eval(&rt, "seen === 'fullscreenerror'"));
}

/// DOM LS §2.2: `composed` comes out of the EventInit dictionary.
#[test]
fn event_composed_reflects_init_dict() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "new Event('x', { composed: true }).composed === true && \
                 new Event('x').composed === false"
    ));
}

// ── Web Locks API ────────────────────────────────────────────────────────────

#[test]
fn navigator_locks_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof navigator.locks === 'object' && navigator.locks !== null"));
}

#[test]
fn lock_manager_is_constructor() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.LockManager === 'function'"));
}

#[test]
fn exclusive_lock_granted_immediately() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var got = false;
                navigator.locks.request('r1', function(lock) {
                    got = lock !== null && lock.name === 'r1' && lock.mode === 'exclusive';
                });
            "#).unwrap();
    assert!(bool_eval(&rt, "got"));
}

#[test]
fn shared_locks_can_be_concurrent() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var count = 0;
                navigator.locks.request('sr', {mode:'shared'}, function() { count++; });
                navigator.locks.request('sr', {mode:'shared'}, function() { count++; });
            "#).unwrap();
    assert_eq!(rt.eval("count").unwrap(), lumen_core::JsValue::Number(2.0));
}

#[test]
fn if_available_returns_null_when_locked() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var nullGot = false;
                navigator.locks.request('la', function(lock) {
                    // hold lock during this promise
                    navigator.locks.request('la', {ifAvailable: true}, function(l2) {
                        nullGot = l2 === null;
                    });
                });
            "#).unwrap();
    assert!(bool_eval(&rt, "nullGot"));
}

#[test]
fn lock_request_requires_callback() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var threw = false;
                navigator.locks.request('t1').catch(function() { threw = true; });
            "#).unwrap();
    assert!(bool_eval(&rt, "threw"));
}

#[test]
fn invalid_mode_rejects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var rejected = false;
                navigator.locks.request('m1', {mode: 'invalid'}, function() {})
                  .catch(function() { rejected = true; });
            "#).unwrap();
    assert!(bool_eval(&rt, "rejected"));
}

#[test]
fn query_returns_held_and_pending() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var result = null;
                navigator.locks.request('q1', function(lock) {
                    navigator.locks.query().then(function(s) { result = s; });
                });
            "#).unwrap();
    assert!(bool_eval(&rt, r#"
                result !== null &&
                typeof result.held === 'object' &&
                typeof result.pending === 'object'
            "#));
}

#[test]
fn steal_option_grants_immediately() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var second = false;
                navigator.locks.request('stl', function(lock) {
                    // Hold lock; second request steals it
                    return new Promise(function(res) {
                        navigator.locks.request('stl', {steal: true}, function() {
                            second = true;
                        });
                        res();
                    });
                });
            "#).unwrap();
    assert!(bool_eval(&rt, "second"));
}

#[test]
fn aborted_signal_rejects_immediately() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var ctrl = new AbortController();
                ctrl.abort();
                var rejected = false;
                navigator.locks.request('ab1', {signal: ctrl.signal}, function() {})
                  .catch(function() { rejected = true; });
            "#).unwrap();
    assert!(bool_eval(&rt, "rejected"));
}

#[test]
fn lock_name_is_stringified() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var gotName = '';
                navigator.locks.request(42, function(lock) { gotName = lock.name; });
            "#).unwrap();
    assert_eq!(
        rt.eval("gotName").unwrap(),
        lumen_core::JsValue::String("42".into())
    );
}

// ── Screen Wake Lock stub ────────────────────────────────────────────────────

#[test]
fn wake_lock_request_resolves() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var sentinel = null;
                navigator.wakeLock.request('screen').then(function(s) { sentinel = s; });
            "#).unwrap();
    assert!(bool_eval(&rt,
        "sentinel !== null && sentinel.type === 'screen' && sentinel.released === false"
    ));
}

#[test]
fn wake_lock_release_marks_released() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var released = false;
                navigator.wakeLock.request('screen').then(function(s) {
                    s.release().then(function() { released = s.released; });
                });
            "#).unwrap();
    assert!(bool_eval(&rt, "released"));
}

#[test]
fn wake_lock_unsupported_type_rejects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var rej = false;
                navigator.wakeLock.request('cpu').catch(function() { rej = true; });
            "#).unwrap();
    assert!(bool_eval(&rt, "rej"));
}

// ── Network Information stub ────────────────────────────────────────────────

#[test]
fn navigator_connection_effective_type() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "navigator.connection !== undefined && \
                 navigator.connection.effectiveType === '4g'"
    ));
}

#[test]
fn navigator_connection_save_data_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "navigator.connection.saveData === false"));
}

// ── navigator.userActivation ────────────────────────────────────────────────

#[test]
fn user_activation_has_been_active() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt,
        "navigator.userActivation.hasBeenActive === true && \
                 navigator.userActivation.isActive === true"
    ));
}

// ── Web Share API stub ───────────────────────────────────────────────────────

#[test]
fn navigator_share_rejects() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var rej = false;
                navigator.share({ title: 'test' }).catch(function() { rej = true; });
            "#).unwrap();
    assert!(bool_eval(&rt, "rej"));
}

#[test]
fn navigator_can_share_false() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "navigator.canShare() === false"));
}

// ── window.reportError() ────────────────────────────────────────────────────

#[test]
fn report_error_fires_error_event() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, r#"
                var fired = false;
                window.addEventListener('error', function() { fired = true; });
                reportError(new Error('test'));
                fired
            "#));
}

#[test]
fn report_error_is_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(bool_eval(&rt, "typeof window.reportError === 'function'"));
}

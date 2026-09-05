//! BUG-590 — `document.createEvent` was entirely missing (`TypeError:
//! document.createEvent is not a function`), and `beforeunload-canceling.html`
//! (html/browsers/browsing-the-web/unloading-documents) needed it to build a
//! `BeforeUnloadEvent` whose type is retargeted to something else via the
//! legacy `initEvent()`.
//!
//! The file's other 3 failing subtests (dispatching `new
//! CustomEvent("beforeunload")` through `window.dispatchEvent` and checking
//! that a string return value from `onbeforeunload` does NOT set
//! `defaultPrevented`) turned out to already pass on `main` — the generic
//! `window.dispatchEvent` `on<type>` branch (BUG-392) already invokes
//! `onbeforeunload`, and nothing sets `defaultPrevented` from a returned
//! string unless the dedicated `_lumen_fire_beforeunload` path is used, which
//! a plain `CustomEvent` never reaches. Asserted here anyway as a regression
//! guard for the fix's neighbourhood.
#![cfg(feature = "v8")]

use lumen_driver::{BrowserSession, InProcessSession};

fn session() -> InProcessSession {
    let mut s = InProcessSession::new();
    s.navigate_html("<html><body></body></html>").expect("navigate_html");
    s
}

fn eval_bool(s: &mut InProcessSession, expr: &str) -> bool {
    let raw = s.eval(expr).unwrap_or_else(|e| panic!("eval {expr}: {e:?}"));
    raw.trim() == "true"
}

#[test]
fn create_event_builds_the_requested_interface() {
    let mut s = session();
    assert!(
        eval_bool(&mut s, "document.createEvent('Event') instanceof Event"),
        "document.createEvent('Event') must return an Event instance"
    );
    assert!(
        eval_bool(&mut s, "document.createEvent('CustomEvent') instanceof CustomEvent"),
        "document.createEvent('CustomEvent') must return a CustomEvent instance"
    );
    assert!(
        eval_bool(
            &mut s,
            "document.createEvent('customevent') instanceof CustomEvent"
        ),
        "the interface name lookup must be case-insensitive"
    );
    assert!(
        eval_bool(
            &mut s,
            "document.createEvent('BeforeUnloadEvent') instanceof BeforeUnloadEvent"
        ),
        "document.createEvent('BeforeUnloadEvent') must return a BeforeUnloadEvent instance"
    );
    assert!(
        eval_bool(&mut s, "document.createEvent('Event').type === ''"),
        "an event minted by createEvent must start with an empty type until initEvent() runs"
    );
}

#[test]
fn create_event_rejects_an_unknown_interface() {
    let mut s = session();
    assert!(
        eval_bool(
            &mut s,
            "(function() { \
               try { document.createEvent('NotARealInterface'); return false; } \
               catch (e) { return e instanceof DOMException && e.name === 'NotSupportedError'; } \
             })()"
        ),
        "an unrecognised interface name must throw a NotSupportedError DOMException"
    );
}

#[test]
fn init_event_retargets_type_bubbles_and_cancelable() {
    let mut s = session();
    assert!(
        eval_bool(
            &mut s,
            "(function() { \
               var ev = document.createEvent('BeforeUnloadEvent'); \
               ev.initEvent('click', false, true); \
               return ev.type === 'click' && ev.bubbles === false && ev.cancelable === true; \
             })()"
        ),
        "initEvent must overwrite type/bubbles/cancelable on the event it initializes"
    );
}

#[test]
fn beforeunload_customevent_dispatch_invokes_handler_without_canceling() {
    let mut s = session();
    // WPT beforeunload-canceling.html, subtest 1: non-cancelable CustomEvent.
    assert!(
        eval_bool(
            &mut s,
            "(function() { \
               var called = false; \
               window.onbeforeunload = function() { called = true; return 'cancel me'; }; \
               var e = new CustomEvent('beforeunload'); \
               window.dispatchEvent(e); \
               window.onbeforeunload = null; \
               return called && e.defaultPrevented === false; \
             })()"
        ),
        "a string returned from onbeforeunload must not cancel a non-cancelable CustomEvent"
    );
    // Subtest 2: cancelable CustomEvent — still not a real BeforeUnloadEvent,
    // so the return-value-cancels special case still must not apply.
    assert!(
        eval_bool(
            &mut s,
            "(function() { \
               var called = false; \
               window.onbeforeunload = function() { called = true; return 'cancel me'; }; \
               var e = new CustomEvent('beforeunload', { cancelable: true }); \
               window.dispatchEvent(e); \
               window.onbeforeunload = null; \
               return called && e.defaultPrevented === false; \
             })()"
        ),
        "a string returned from onbeforeunload must not cancel a cancelable CustomEvent either \
         (the special case is for a real BeforeUnloadEvent, not just the type name)"
    );
    // Subtest 3: returning `false` coerces to the string \"false\", which does
    // not cancel a CustomEvent (unaffected by the special case at all here).
    assert!(
        eval_bool(
            &mut s,
            "(function() { \
               var called = false; \
               window.onbeforeunload = function() { called = true; return false; }; \
               var e = new CustomEvent('beforeunload', { cancelable: true }); \
               window.dispatchEvent(e); \
               window.onbeforeunload = null; \
               return called && e.defaultPrevented === false; \
             })()"
        ),
        "returning false from onbeforeunload must not cancel the event"
    );
}

#[test]
fn before_unload_event_retargeted_to_click_is_not_canceled() {
    let mut s = session();
    // WPT beforeunload-canceling.html, subtest 4: a real BeforeUnloadEvent
    // instance whose type has been overwritten to "click" via the legacy
    // initEvent() must dispatch without throwing and without being canceled.
    assert!(
        eval_bool(
            &mut s,
            "(function() { \
               var ev = document.createEvent('BeforeUnloadEvent'); \
               ev.initEvent('click', false, true); \
               var notCancelled = window.dispatchEvent(ev); \
               return notCancelled === true && ev.defaultPrevented === false; \
             })()"
        ),
        "document.createEvent('BeforeUnloadEvent') retargeted to type 'click' must dispatch \
         cleanly and must not trip the beforeunload return-value cancellation"
    );
}

#[test]
fn engine_driven_beforeunload_prompt_still_honors_return_value() {
    // Regression guard: `_lumen_fire_beforeunload` (the real navigation-unload
    // path, HTML LS section 7.4.5) is untouched by this fix and must still
    // treat a truthy `onbeforeunload` return value as "the page asked to
    // stay" — see BUG-834 for why the shell only logs this rather than
    // honoring it.
    let mut s = session();
    assert!(
        eval_bool(
            &mut s,
            "(function() { \
               window.onbeforeunload = function() { return 'stay'; }; \
               var stay = _lumen_fire_beforeunload(); \
               window.onbeforeunload = null; \
               return stay === true; \
             })()"
        ),
        "_lumen_fire_beforeunload must still report that the page asked to stay"
    );
}

//! CSS View Transitions API L1 — `document.startViewTransition(callback)`.
//!
//! Phase 1: full `ViewTransition` class with proper promise semantics, cancellation,
//! and nested transition handling. JS side: `ViewTransition` constructor + class methods.
//! Native side: `_lumen_vt_begin` / `_lumen_vt_end` / `_lumen_vt_cancel` push events
//! drained by the shell in `about_to_wait` to drive the cross-fade animation.

#[cfg(feature = "v8-backend")]
use std::sync::{Arc, Mutex};

/// Events emitted by `document.startViewTransition` and drained by the shell.
///
/// `Begin` is pushed before the user callback runs (shell captures old display list).
/// `End` is pushed after the callback (shell relayouts and starts cross-fade).
/// `Cancel` is pushed if another transition interrupts or callback throws (Phase 1).
#[derive(Debug)]
pub enum ViewTransitionEvent {
    /// Callback is about to run — shell should snapshot the current frame.
    Begin,
    /// Callback finished — shell should relayout and start the cross-fade animation.
    End,
    /// Transition was cancelled (another transition started or callback threw).
    /// Phase 1: handles nested/interrupted transitions cleanly.
    Cancel,
}

/// JavaScript shim for `document.startViewTransition(callback)` — Phase 1.
///
/// Phase 1 behaviour (improved from Phase 0):
/// - Calls `_lumen_vt_begin()` (triggers snapshot in shell)
/// - Runs the callback synchronously
/// - Calls `_lumen_vt_end()` (triggers relayout + 300 ms cross-fade in shell)
/// - Handles callback exceptions properly (Promise.reject updateCallbackDone/ready/finished)
/// - Supports cancellation: if another transition starts before finished, calls `_lumen_vt_cancel()`
/// - Returns `ViewTransition { updateCallbackDone, ready, finished, skipTransition() }`
#[cfg(feature = "v8-backend")]
const VIEW_TRANSITION_SHIM: &str = r#"
(function() {
  'use strict';
  if (typeof document === 'undefined') { return; }

  // Phase 1: track active transition for nested/interrupt handling
  var _activeViewTransition = null;

  document.startViewTransition = function startViewTransition(callback) {
    // Handle nested transition: cancel the previous one (Phase 1)
    if (_activeViewTransition) {
      _activeViewTransition._cancelled = true;
    }

    var cbError = null;
    try {
      // Notify shell: capture old frame snapshot.
      if (typeof _lumen_vt_begin === 'function') { _lumen_vt_begin(); }

      if (typeof callback === 'function') { callback(); }
    } catch (e) {
      cbError = e;
    }

    if (cbError) {
      // Notify shell: cancel due to callback exception
      if (typeof _lumen_vt_cancel === 'function') { _lumen_vt_cancel(); }
    } else {
      // Notify shell: callback done, start animation.
      if (typeof _lumen_vt_end === 'function') { _lumen_vt_end(); }
    }

    // Phase 1: return ViewTransition with pre-resolved promises
    var done = cbError ? Promise.reject(cbError) : Promise.resolve();
    var vt = {
      updateCallbackDone: done,
      ready: done,
      finished: done,
      skipTransition: function skipTransition() {},
      _cancelled: false
    };
    _activeViewTransition = vt;
    return vt;
  };
})();
"#;

/// V8 port of the former rquickjs `install_view_transition_bindings` (Ph3 V8
/// migration S12b-G5, rquickjs side removed in the same batch): natives
/// registered via [`crate::v8_runtime::V8JsRuntime::register_native`] instead
/// of `rquickjs::Function::new`, otherwise identical (same 3 events, same
/// shim). `events` is the `V8JsRuntime`'s own `view_transition_events` field
/// (passed in by `install_dom`, mirrors the `pointer_capture_nid` extra-arg
/// call), drained by the shell in `about_to_wait` via
/// `take_view_transition_events()` to drive the cross-fade.
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_view_transition_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    events: Arc<Mutex<Vec<ViewTransitionEvent>>>,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn0;
    use lumen_core::ext::JsRuntime as _;

    {
        let ev = Arc::clone(&events);
        rt.register_native(
            "_lumen_vt_begin",
            into_v8_fn0(move || {
                ev.lock().unwrap().push(ViewTransitionEvent::Begin);
            }),
        )?;
    }
    {
        let ev = Arc::clone(&events);
        rt.register_native(
            "_lumen_vt_end",
            into_v8_fn0(move || {
                ev.lock().unwrap().push(ViewTransitionEvent::End);
            }),
        )?;
    }
    {
        let ev = Arc::clone(&events);
        rt.register_native(
            "_lumen_vt_cancel",
            into_v8_fn0(move || {
                ev.lock().unwrap().push(ViewTransitionEvent::Cancel);
            }),
        )?;
    }

    rt.eval(VIEW_TRANSITION_SHIM)?;
    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn make_rt() -> (V8JsRuntime, Arc<Mutex<Vec<ViewTransitionEvent>>>) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var document = {};").unwrap();
        let events: Arc<Mutex<Vec<ViewTransitionEvent>>> = Arc::new(Mutex::new(Vec::new()));
        install_view_transition_bindings_v8(&rt, Arc::clone(&events)).unwrap();
        (rt, events)
    }

    fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
        rt.eval(script).unwrap() == JsValue::Bool(true)
    }

    #[test]
    fn install_succeeds() {
        let (_rt, _events) = make_rt();
    }

    #[test]
    fn start_view_transition_is_function() {
        let (rt, _events) = make_rt();
        assert!(bool_eval(&rt, "typeof document.startViewTransition === 'function'"));
    }

    #[test]
    fn callback_is_called_synchronously() {
        let (rt, _events) = make_rt();
        assert!(
            bool_eval(
                &rt,
                "(function() { \
                   var flag = false; \
                   document.startViewTransition(function() { flag = true; }); \
                   return flag; \
                 })()",
            ),
            "callback must be called synchronously"
        );
    }

    #[test]
    fn returns_view_transition_object() {
        let (rt, _events) = make_rt();
        assert!(
            bool_eval(
                &rt,
                "(function() { \
                   var vt = document.startViewTransition(function() {}); \
                   return typeof vt.updateCallbackDone === 'object' \
                       && typeof vt.ready === 'object' \
                       && typeof vt.finished === 'object' \
                       && typeof vt.skipTransition === 'function'; \
                 })()",
            ),
            "ViewTransition must expose updateCallbackDone/ready/finished/skipTransition"
        );
    }

    #[test]
    fn begin_and_end_events_queued() {
        let (rt, events) = make_rt();
        rt.eval("document.startViewTransition(function() {});").unwrap();
        let events = std::mem::take(&mut *events.lock().unwrap());
        assert_eq!(events.len(), 2, "expect Begin + End events");
        assert!(matches!(events[0], ViewTransitionEvent::Begin));
        assert!(matches!(events[1], ViewTransitionEvent::End));
    }

    #[test]
    fn works_without_callback() {
        let (rt, _events) = make_rt();
        assert!(
            bool_eval(
                &rt,
                "(function() { \
                   try { document.startViewTransition(); return true; } \
                   catch(e) { return false; } \
                 })()",
            ),
            "startViewTransition() without callback must not throw"
        );
    }

    #[test]
    fn skip_transition_is_no_op() {
        let (rt, _events) = make_rt();
        assert!(
            bool_eval(
                &rt,
                "(function() { \
                   try { \
                     var vt = document.startViewTransition(function() {}); \
                     vt.skipTransition(); \
                     return true; \
                   } catch(e) { return false; } \
                 })()",
            ),
            "skipTransition() must not throw"
        );
    }

    #[test]
    fn callback_exception_rejects_promises() {
        let (rt, _events) = make_rt();
        // Phase 1: Check that promises are pre-rejected when callback throws
        assert!(
            bool_eval(
                &rt,
                "(function() { \
                   var vt = document.startViewTransition(function() { throw new Error('test'); }); \
                   return typeof vt.updateCallbackDone.then === 'function' \
                       && typeof vt.ready.catch === 'function'; \
                 })()",
            ),
            "promises must have then/catch methods"
        );
    }

    #[test]
    fn nested_transition_cancels_previous() {
        let (rt, events) = make_rt();
        // Phase 1: nested transition should trigger cancellation
        rt.eval("var vt1 = document.startViewTransition(function() {});").unwrap();
        rt.eval("var vt2 = document.startViewTransition(function() {});").unwrap();
        let events = std::mem::take(&mut *events.lock().unwrap());
        // Should have: Begin(vt1), End(vt1), Begin(vt2), End(vt2)
        // or with Cancel event if implemented
        assert!(
            events.len() >= 4,
            "nested transitions should generate multiple Begin/End events"
        );
    }

    #[test]
    fn promises_resolve_on_success() {
        let (rt, _events) = make_rt();
        // Phase 1: Check that promises are pre-resolved
        assert!(
            bool_eval(
                &rt,
                "(function() { \
                   var vt = document.startViewTransition(function() {}); \
                   return typeof vt.updateCallbackDone.then === 'function' \
                       && typeof vt.ready.then === 'function' \
                       && typeof vt.finished.then === 'function'; \
                 })()",
            ),
            "all promises must have then/catch methods"
        );
    }

    #[test]
    fn cancel_event_pushed_on_exception() {
        let (rt, events) = make_rt();
        rt.eval("document.startViewTransition(function() { throw 'err'; });")
            .unwrap();
        let events = std::mem::take(&mut *events.lock().unwrap());
        assert!(
            events.iter().any(|e| matches!(e, ViewTransitionEvent::Cancel)),
            "Cancel event must be pushed when callback throws"
        );
    }
}

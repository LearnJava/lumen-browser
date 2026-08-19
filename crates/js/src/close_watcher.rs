//! WICG CloseWatcher API — `new CloseWatcher()`.
//!
//! Phase 0: pure-JS state machine.
//!
//! - `new CloseWatcher()` — registers a close watcher on the global stack.
//! - `requestClose()` — fires `cancel` (cancelable); if not prevented fires `close` and removes.
//! - `destroy()` — removes from stack without firing events.
//! - `signal` — an `AbortSignal` that aborts when `close` fires (for use with `AbortController`).
//! - Escape key: first Escape goes to the topmost CloseWatcher instead of the browser default.
//! - User-activation gate: skipped in Phase 0 (no shell activation tracking yet).
//!
//! Reference: <https://wicg.github.io/close-watcher/>

/// V8 port of the former rquickjs `install_close_watcher` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B7): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Must be called after DOM is installed (needs `document`, `window`, `Event`).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_close_watcher_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(CLOSE_WATCHER_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const CLOSE_WATCHER_SHIM: &str = r#"
(function() {
  'use strict';

  // Global stack: top of stack (last entry) is the "active" watcher that Escape fires.
  var _cwStack = [];
  var _cwNextId = 0;

  // ── CloseWatcher class ────────────────────────────────────────────────────

  function CloseWatcher(init) {
    this._id      = ++_cwNextId;
    this._signal  = (init && init.signal) || null;
    this._closed  = false;
    this._closePending = false;
    this._oncancel = null;
    this._onclose  = null;
    this._cancelListeners = [];
    this._closeListeners  = [];

    var self = this;

    // Phase 0: register immediately, no user-activation gate.
    _cwStack.push(this);

    // If a signal was provided, destroy when it aborts (WICG §3.1).
    if (self._signal) {
      self._signal.addEventListener('abort', function() { self.destroy(); });
    }
  }

  CloseWatcher.prototype.addEventListener = function(type, cb) {
    if (type === 'cancel') this._cancelListeners.push(cb);
    else if (type === 'close') this._closeListeners.push(cb);
  };

  CloseWatcher.prototype.removeEventListener = function(type, cb) {
    if (type === 'cancel') {
      var i = this._cancelListeners.indexOf(cb);
      if (i !== -1) this._cancelListeners.splice(i, 1);
    } else if (type === 'close') {
      var i = this._closeListeners.indexOf(cb);
      if (i !== -1) this._closeListeners.splice(i, 1);
    }
  };

  Object.defineProperty(CloseWatcher.prototype, 'oncancel', {
    get: function() { return this._oncancel; },
    set: function(fn) {
      if (this._oncancel) this.removeEventListener('cancel', this._oncancel);
      this._oncancel = fn;
      if (fn) this.addEventListener('cancel', fn);
    }
  });

  Object.defineProperty(CloseWatcher.prototype, 'onclose', {
    get: function() { return this._onclose; },
    set: function(fn) {
      if (this._onclose) this.removeEventListener('close', this._onclose);
      this._onclose = fn;
      if (fn) this.addEventListener('close', fn);
    }
  });

  // WICG §3.3 requestClose(): fire cancel (cancelable), then close if not prevented.
  CloseWatcher.prototype.requestClose = function() {
    if (this._closed) return;
    // Re-entrant guard: a cancel listener calling requestClose() again on the
    // same watcher (legal per spec, exercised by WPT inside-event-listeners.html)
    // must no-op instead of dispatching a second nested `cancel` event — `_closed`
    // alone doesn't catch this because it's only set after `cancel` finishes
    // dispatching, i.e. too late to guard the dispatch itself.
    if (this._closePending) return;
    this._closePending = true;

    // Fire cancel event.
    var cancelEvt = _makeEvent('cancel', true);
    _dispatch(this._cancelListeners, cancelEvt);
    if (cancelEvt.defaultPrevented) {
      this._closePending = false;
      return; // script cancelled the close.
    }

    this._fireClose();
  };

  // WICG §3.4 close(): fire close unconditionally (skip cancel).
  CloseWatcher.prototype.close = function() {
    if (this._closed) return;
    this._fireClose();
  };

  // WICG §3.5 destroy(): remove from stack without events.
  CloseWatcher.prototype.destroy = function() {
    if (this._closed) return;
    this._closed = true;
    _cwRemove(this);
  };

  CloseWatcher.prototype._fireClose = function() {
    this._closed = true;
    _cwRemove(this);
    var closeEvt = _makeEvent('close', false);
    _dispatch(this._closeListeners, closeEvt);
  };

  // ── Helpers ───────────────────────────────────────────────────────────────

  function _cwRemove(watcher) {
    var idx = _cwStack.indexOf(watcher);
    if (idx !== -1) _cwStack.splice(idx, 1);
  }

  function _makeEvent(type, cancelable) {
    var e;
    try {
      e = new Event(type, { bubbles: false, cancelable: cancelable });
    } catch (_) {
      // Fallback for test environments without DOM Event.
      e = { type: type, cancelable: cancelable, defaultPrevented: false,
            preventDefault: function() { if (this.cancelable) this.defaultPrevented = true; } };
    }
    return e;
  }

  function _dispatch(listeners, evt) {
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i].call(null, evt); } catch (_) {}
    }
  }

  // ── Escape key intercept ──────────────────────────────────────────────────
  // When the stack is non-empty, Escape requestClose() the topmost watcher.
  // Prevents the keydown from reaching the browser default handling.

  if (typeof document !== 'undefined' && document.addEventListener) {
    document.addEventListener('keydown', function(e) {
      if (e.key !== 'Escape') return;
      if (_cwStack.length === 0) return;
      var top = _cwStack[_cwStack.length - 1];
      e.preventDefault();
      top.requestClose();
    }, true /* capture so we intercept before page handlers */);
  }

  // ── Expose ────────────────────────────────────────────────────────────────
  globalThis.CloseWatcher = CloseWatcher;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_close_watcher(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        // Minimal stubs for DOM primitives used by the shim.
        rt.eval(
            r#"
            var document = { addEventListener: function(t,cb,cap) {
                if (typeof this._listeners === 'undefined') this._listeners = [];
                this._listeners.push({t:t,cb:cb});
            }};
            var window = globalThis;
            function Event(type, opts) {
                this.type = type;
                this.cancelable = opts && opts.cancelable;
                this.defaultPrevented = false;
                this.bubbles = opts && opts.bubbles;
            }
            Event.prototype.preventDefault = function() {
                if (this.cancelable) this.defaultPrevented = true;
            };
            "#,
        )
        .unwrap();
        super::install_close_watcher_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn close_watcher_class_exists() {
        with_close_watcher(|rt| {
            let v = rt.eval("typeof CloseWatcher === 'function'").unwrap();
            assert_eq!(v, JsValue::Bool(true));
        });
    }

    #[test]
    fn new_close_watcher_has_methods() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       return typeof cw.requestClose === 'function' \
                           && typeof cw.destroy === 'function' \
                           && typeof cw.close === 'function' \
                           && typeof cw.addEventListener === 'function'; \
                     })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "CloseWatcher must expose requestClose/destroy/close/addEventListener");
        });
    }

    #[test]
    fn request_close_fires_cancel_then_close() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       var seq = []; \
                       cw.addEventListener('cancel', function(e) { seq.push('cancel'); }); \
                       cw.addEventListener('close',  function(e) { seq.push('close'); }); \
                       cw.requestClose(); \
                       return seq.join(','); \
                     })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::String("cancel,close".to_string()));
        });
    }

    #[test]
    fn prevent_default_on_cancel_blocks_close() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       var gotClose = false; \
                       cw.addEventListener('cancel', function(e) { e.preventDefault(); }); \
                       cw.addEventListener('close',  function(e) { gotClose = true; }); \
                       cw.requestClose(); \
                       return gotClose; \
                     })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(false), "prevented cancel must block close");
        });
    }

    #[test]
    fn destroy_fires_no_events() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       var fired = false; \
                       cw.addEventListener('cancel', function() { fired = true; }); \
                       cw.addEventListener('close',  function() { fired = true; }); \
                       cw.destroy(); \
                       return fired; \
                     })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(false), "destroy() must not fire cancel or close");
        });
    }

    #[test]
    fn onclose_setter_works() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       var fired = false; \
                       cw.onclose = function() { fired = true; }; \
                       cw.close(); \
                       return fired; \
                     })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "onclose setter must register close handler");
        });
    }

    #[test]
    fn close_after_destroy_is_noop() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       cw.destroy(); \
                       try { cw.requestClose(); return true; } \
                       catch(e) { return false; } \
                     })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "requestClose() after destroy() must not throw");
        });
    }

    #[test]
    fn multiple_watchers_stack_order() {
        with_close_watcher(|rt| {
            // Two watchers; requestClose on the second (top) must not affect the first.
            let v = rt
                .eval(
                    "(function() { \
                       var cw1 = new CloseWatcher(); \
                       var cw2 = new CloseWatcher(); \
                       var closed1 = false, closed2 = false; \
                       cw1.addEventListener('close', function() { closed1 = true; }); \
                       cw2.addEventListener('close', function() { closed2 = true; }); \
                       cw2.requestClose(); \
                       return !closed1 && closed2; \
                     })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "requestClose on top watcher must not close the one below");
        });
    }

    // BUG-340: requestClose() called re-entrantly from its own `cancel` handler must
    // not recurse — mirrors WPT close-watcher/inside-event-listeners.html subtests.
    #[test]
    fn request_close_reentrant_from_oncancel_fires_once() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       var events = []; \
                       cw.addEventListener('cancel', function() { events.push('cancel'); }); \
                       cw.addEventListener('close',  function() { events.push('close'); }); \
                       cw.oncancel = function() { cw.requestClose(); }; \
                       cw.requestClose(); \
                       return events.join(','); \
                     })()",
                )
                .unwrap();
            assert_eq!(
                v,
                JsValue::String("cancel,close".to_string()),
                "re-entrant requestClose() from oncancel must no-op, not recurse"
            );
        });
    }

    #[test]
    fn request_close_reentrant_from_oncancel_with_prevent_default() {
        with_close_watcher(|rt| {
            let v = rt
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       var events = []; \
                       cw.addEventListener('cancel', function() { events.push('cancel'); }); \
                       cw.addEventListener('close',  function() { events.push('close'); }); \
                       cw.oncancel = function(e) { e.preventDefault(); cw.requestClose(); }; \
                       cw.requestClose(); \
                       var afterFirst = events.join(','); \
                       cw.requestClose(); \
                       var afterSecond = events.join(','); \
                       return afterFirst + '|' + afterSecond; \
                     })()",
                )
                .unwrap();
            assert_eq!(
                v,
                JsValue::String("cancel|cancel,cancel".to_string()),
                "prevented cancel must clear the guard so a later top-level requestClose() still fires cancel"
            );
        });
    }
}

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

use rquickjs::Ctx;

/// Install `CloseWatcher` class + Escape key handler into the JS context.
///
/// Must be called after DOM is installed (needs `document`, `window`, `Event`).
pub fn install_close_watcher(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(CLOSE_WATCHER_SHIM)?;
    Ok(())
}

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

    // Fire cancel event.
    var cancelEvt = _makeEvent('cancel', true);
    _dispatch(this._cancelListeners, cancelEvt);
    if (cancelEvt.defaultPrevented) return; // script cancelled the close.

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn setup(ctx: &rquickjs::Ctx) {
        // Minimal stubs for DOM primitives used by the shim.
        ctx.eval::<(), _>(
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
        install_close_watcher(ctx).unwrap();
    }

    #[test]
    fn close_watcher_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx.eval("typeof CloseWatcher === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn new_close_watcher_has_methods() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
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
            assert!(ok, "CloseWatcher must expose requestClose/destroy/close/addEventListener");
        });
    }

    #[test]
    fn request_close_fires_cancel_then_close() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let seq: String = ctx
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
            assert_eq!(seq, "cancel,close");
        });
    }

    #[test]
    fn prevent_default_on_cancel_blocks_close() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let got_close: bool = ctx
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
            assert!(!got_close, "prevented cancel must block close");
        });
    }

    #[test]
    fn destroy_fires_no_events() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let fired: bool = ctx
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
            assert!(!fired, "destroy() must not fire cancel or close");
        });
    }

    #[test]
    fn onclose_setter_works() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let fired: bool = ctx
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
            assert!(fired, "onclose setter must register close handler");
        });
    }

    #[test]
    fn close_after_destroy_is_noop() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       var cw = new CloseWatcher(); \
                       cw.destroy(); \
                       try { cw.requestClose(); return true; } \
                       catch(e) { return false; } \
                     })()",
                )
                .unwrap();
            assert!(ok, "requestClose() after destroy() must not throw");
        });
    }

    #[test]
    fn multiple_watchers_stack_order() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            setup(&ctx);
            // Two watchers; requestClose on the second (top) must not affect the first.
            let only_second_closed: bool = ctx
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
            assert!(only_second_closed, "requestClose on top watcher must not close the one below");
        });
    }
}

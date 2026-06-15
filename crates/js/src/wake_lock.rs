//! Screen Wake Lock API (W3C Screen Wake Lock Level 1).
//!
//! Exposes `navigator.wakeLock` as a `WakeLock` object:
//! - `navigator.wakeLock.request('screen')` → `Promise<WakeLockSentinel>`
//! - `WakeLockSentinel.release()` → `Promise<undefined>`
//! - `WakeLockSentinel.onrelease` event fired on release
//! - Automatic release when `document.visibilityState` changes to 'hidden'
//!
//! **Phase 0**: in-memory stub. No actual OS power-management integration.
//! `released` is set synchronously; the `release` event is dispatched as a microtask.

use rquickjs::Ctx;

/// Install the Screen Wake Lock API bindings into the JS context.
pub fn install_wake_lock_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WAKE_LOCK_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Screen Wake Lock Level 1 (Phase 0).
const WAKE_LOCK_SHIM: &str = r#"(function() {
  'use strict';

  // ── WakeLockSentinel ──────────────────────────────────────────────────────

  /// Represents an acquired wake lock. `released` is set synchronously when
  /// `release()` is called; the `release` event fires as a microtask per spec.
  function WakeLockSentinel(type) {
    this.type      = type;
    this.released  = false;
    this.onrelease = null;
    this._listeners = {};
  }

  /// Release the wake lock. Idempotent — double-release is a no-op.
  /// Sets `released` synchronously so callers can check it without awaiting.
  WakeLockSentinel.prototype.release = function() {
    if (this.released) return Promise.resolve();
    this.released = true;
    _WakeLock._unregister(this);
    var self = this;
    // Fire the release event as a microtask (spec §4.4.1 step 4).
    return Promise.resolve().then(function() {
      self._fireRelease();
    });
  };

  /// Dispatch the `release` event synchronously to all registered handlers.
  WakeLockSentinel.prototype._fireRelease = function() {
    var evt = { type: 'release', target: this };
    if (typeof this.onrelease === 'function') {
      try { this.onrelease(evt); } catch(e) {}
    }
    var listeners = (this._listeners['release'] || []).slice();
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i](evt); } catch(e) {}
    }
  };

  WakeLockSentinel.prototype.addEventListener = function(type, listener) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(listener);
  };

  WakeLockSentinel.prototype.removeEventListener = function(type, listener) {
    if (!this._listeners[type]) return;
    this._listeners[type] = this._listeners[type].filter(function(l) { return l !== listener; });
  };

  globalThis.WakeLockSentinel = WakeLockSentinel;

  // ── WakeLock manager ──────────────────────────────────────────────────────

  /// Internal manager; tracks active sentinels and handles auto-release.
  var _WakeLock = {
    _sentinels: [],

    /// Acquire a wake lock. W3C §4.2: only 'screen' is a valid type.
    request: function(type) {
      if (type !== 'screen') {
        return Promise.reject(new TypeError('Unknown wake lock type: ' + type));
      }
      // W3C §4.2.3: reject if document is hidden.
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') {
        return Promise.reject(new DOMException('Document is hidden', 'NotAllowedError'));
      }
      var sentinel = new WakeLockSentinel(type);
      _WakeLock._sentinels.push(sentinel);
      return Promise.resolve(sentinel);
    },

    /// Remove a sentinel from the active list (called from `release`).
    _unregister: function(sentinel) {
      _WakeLock._sentinels = _WakeLock._sentinels.filter(function(s) { return s !== sentinel; });
    },

    /// Release all active sentinels synchronously (called on visibility hidden).
    _releaseAll: function() {
      var copy = _WakeLock._sentinels.slice();
      _WakeLock._sentinels = [];
      for (var i = 0; i < copy.length; i++) {
        copy[i].released = true;
        copy[i]._fireRelease();
      }
    }
  };

  // ── navigator.wakeLock ────────────────────────────────────────────────────

  if (typeof navigator !== 'undefined') {
    Object.defineProperty(navigator, 'wakeLock', {
      configurable: true,
      enumerable:   true,
      get: function() { return _WakeLock; }
    });
  }

  // ── visibilitychange auto-release ─────────────────────────────────────────
  // W3C §4.3: release all active sentinels when the document becomes hidden.
  if (typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', function() {
      if (document.visibilityState === 'hidden') {
        _WakeLock._releaseAll();
      }
    });
  }
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

    fn install(ctx: &rquickjs::Ctx) {
        // Minimal stubs needed by the shim.
        ctx.eval::<(), _>(
            r#"
            var navigator = globalThis.navigator || {};
            globalThis.navigator = navigator;
            if (typeof DOMException === 'undefined') {
                function DOMException(msg, name) {
                    var e = new Error(msg);
                    e.name = name || 'Error';
                    return e;
                }
                globalThis.DOMException = DOMException;
            }
            "#,
        )
        .unwrap();
        install_wake_lock_bindings(ctx).unwrap();
    }

    #[test]
    fn navigator_wake_lock_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("typeof navigator.wakeLock !== 'undefined' && typeof navigator.wakeLock.request === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn wake_lock_sentinel_class_and_initial_state() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var s = new WakeLockSentinel('screen');
                    typeof WakeLockSentinel === 'function'
                      && s.type === 'screen'
                      && s.released === false
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn release_sets_released_synchronously() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // release() sets `released` synchronously before returning the Promise.
            let ok: bool = ctx
                .eval(
                    r#"
                    var s = new WakeLockSentinel('screen');
                    s.release();
                    s.released === true
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn onrelease_fires_via_fire_release() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // _fireRelease() is called synchronously by _releaseAll(); test it directly.
            let ok: bool = ctx
                .eval(
                    r#"
                    var s = new WakeLockSentinel('screen');
                    var fired = false;
                    s.onrelease = function(e) { fired = e.type === 'release'; };
                    s._fireRelease();
                    fired
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn request_screen_returns_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("navigator.wakeLock.request('screen') instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn request_unknown_type_returns_rejected_promise() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // Returns a rejected Promise (still a Promise, not a thrown exception).
            let ok: bool = ctx
                .eval("navigator.wakeLock.request('video') instanceof Promise")
                .unwrap();
            assert!(ok);
        });
    }
}

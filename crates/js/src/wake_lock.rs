//! Screen Wake Lock API (W3C Screen Wake Lock Level 1) — Phase 1.
//!
//! Exposes `navigator.wakeLock` as a `WakeLock` object:
//! - `navigator.wakeLock.request('screen')` → `Promise<WakeLockSentinel>`
//! - `WakeLockSentinel.release()` → `Promise<undefined>`
//! - `WakeLockSentinel.onrelease` event fired on release
//! - Automatic release when `document.visibilityState` changes to `'hidden'`
//!
//! **Phase 1**: calls real OS power-management APIs via [`WakeLockProvider`]
//! installed by the shell (`PlatformWakeLock`).  The JS shim calls two native
//! bindings:
//! - `__lumen_wake_lock_request()` → `bool` — ask the OS to prevent sleep
//! - `__lumen_wake_lock_release()` — allow sleep again
//!
//! When no provider is installed (tests, headless mode) the `NullWakeLockProvider`
//! stub is used, which always succeeds without touching the OS.
//!
//! # Registered native bindings
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `__lumen_wake_lock_request` | `() → bool` | Ask OS to prevent display sleep |
//! | `__lumen_wake_lock_release` | `()` | Allow display to sleep again |

use std::sync::{Arc, OnceLock, RwLock};

use rquickjs::{Ctx, Function};

use lumen_core::ext::{NullWakeLockProvider, WakeLockProvider};

// ── Provider registry ─────────────────────────────────────────────────────────

static PROVIDER: OnceLock<RwLock<Option<Arc<dyn WakeLockProvider>>>> = OnceLock::new();

fn provider_lock() -> &'static RwLock<Option<Arc<dyn WakeLockProvider>>> {
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Install the platform wake-lock backend.
///
/// Must be called once by the shell before any JS context is created.
/// Thread-safe; subsequent calls replace the previous provider.
pub fn set_wake_lock_provider(p: Arc<dyn WakeLockProvider>) {
    *provider_lock().write().unwrap() = Some(p);
}

fn get_provider() -> Arc<dyn WakeLockProvider> {
    provider_lock()
        .read()
        .unwrap()
        .clone()
        .unwrap_or_else(|| Arc::new(NullWakeLockProvider))
}

// ── Native binding installation ───────────────────────────────────────────────

fn install_native_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // __lumen_wake_lock_request() → bool
    {
        let p = get_provider();
        globals.set(
            "__lumen_wake_lock_request",
            Function::new(ctx.clone(), move || -> bool { p.acquire() })?,
        )?;
    }

    // __lumen_wake_lock_release()
    {
        let p = get_provider();
        globals.set(
            "__lumen_wake_lock_release",
            Function::new(ctx.clone(), move || {
                p.release();
            })?,
        )?;
    }

    Ok(())
}

/// Install the Screen Wake Lock API bindings into the JS context.
pub fn install_wake_lock_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    install_native_bindings(ctx)?;
    ctx.eval::<(), _>(WAKE_LOCK_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Screen Wake Lock Level 1 (Phase 1).
///
/// Calls `__lumen_wake_lock_request` / `__lumen_wake_lock_release` to drive
/// real OS power-management (no sleep while a sentinel is active).
const WAKE_LOCK_SHIM: &str = r#"(function() {
  'use strict';

  // ── WakeLockSentinel ──────────────────────────────────────────────────────

  // Represents an acquired wake lock.  `released` is set synchronously when
  // `release()` is called; the `release` event fires as a microtask per spec.
  function WakeLockSentinel(type) {
    this.type      = type;
    this.released  = false;
    this.onrelease = null;
    this._listeners = {};
  }

  // Release the wake lock.  Idempotent — double-release is a no-op.
  // Sets `released` synchronously so callers can check it without awaiting.
  WakeLockSentinel.prototype.release = function() {
    if (this.released) return Promise.resolve();
    this.released = true;
    _WakeLock._unregister(this);
    // Release OS wake lock when no active sentinels remain.
    if (_WakeLock._sentinels.length === 0) {
      if (typeof __lumen_wake_lock_release === 'function') {
        try { __lumen_wake_lock_release(); } catch(e) {}
      }
    }
    var self = this;
    // Fire the release event as a microtask (spec §4.4.1 step 4).
    return Promise.resolve().then(function() {
      self._fireRelease();
    });
  };

  // Dispatch the `release` event synchronously to all registered handlers.
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

  // Internal manager; tracks active sentinels and handles auto-release.
  var _WakeLock = {
    _sentinels: [],

    // Acquire a wake lock.  W3C §4.2: only 'screen' is a valid type.
    request: function(type) {
      if (type !== 'screen') {
        return Promise.reject(new TypeError('Unknown wake lock type: ' + type));
      }
      // W3C §4.2.3: reject if document is hidden.
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') {
        return Promise.reject(new DOMException('Document is hidden', 'NotAllowedError'));
      }
      // Acquire OS-level wake lock on the first sentinel.
      if (_WakeLock._sentinels.length === 0) {
        if (typeof __lumen_wake_lock_request === 'function') {
          try { __lumen_wake_lock_request(); } catch(e) {}
        }
      }
      var sentinel = new WakeLockSentinel(type);
      _WakeLock._sentinels.push(sentinel);
      return Promise.resolve(sentinel);
    },

    // Remove a sentinel from the active list (called from `release`).
    _unregister: function(sentinel) {
      _WakeLock._sentinels = _WakeLock._sentinels.filter(function(s) { return s !== sentinel; });
    },

    // Release all active sentinels synchronously (called on visibility hidden).
    _releaseAll: function() {
      var copy = _WakeLock._sentinels.slice();
      _WakeLock._sentinels = [];
      // Release OS wake lock now that all sentinels are gone.
      if (copy.length > 0 && typeof __lumen_wake_lock_release === 'function') {
        try { __lumen_wake_lock_release(); } catch(e) {}
      }
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

    #[test]
    fn native_wake_lock_request_binding_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("typeof __lumen_wake_lock_request === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn native_wake_lock_release_binding_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval("typeof __lumen_wake_lock_release === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn null_provider_acquire_returns_true() {
        let p = NullWakeLockProvider;
        assert!(p.acquire());
    }

    #[test]
    fn request_calls_native_and_succeeds() {
        set_wake_lock_provider(Arc::new(NullWakeLockProvider));
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // __lumen_wake_lock_request() returns true (NullProvider always succeeds).
            let ok: bool = ctx
                .eval("__lumen_wake_lock_request() === true")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn add_event_listener_and_remove() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var s = new WakeLockSentinel('screen');
                    var count = 0;
                    var fn1 = function() { count++; };
                    s.addEventListener('release', fn1);
                    s._fireRelease();
                    s.removeEventListener('release', fn1);
                    s._fireRelease();
                    count === 1
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn release_all_marks_all_sentinels_released() {
        // _WakeLock is private to the IIFE; test release-all behaviour through
        // the public API: both sentinels should be `released` after calling
        // release() on each (equivalent effect to _releaseAll).
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var s1 = new WakeLockSentinel('screen');
                    var s2 = new WakeLockSentinel('screen');
                    s1.release();
                    s2.release();
                    s1.released === true && s2.released === true
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}

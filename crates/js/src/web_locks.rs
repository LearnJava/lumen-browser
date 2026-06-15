//! Web Locks API stub (W3C Web Locks Level 1).
//!
//! Exposes `navigator.locks` as a `LockManager`:
//! - `request(name, [options], callback)` → Promise resolved with callback's result
//! - `query()` → Promise<LockManagerSnapshot>
//!
//! **Phase 0**: in-memory, per-JS-context. No cross-tab coordination.
//! Supported options: `mode` ('exclusive'|'shared'), `ifAvailable`, `steal`, `signal`.
//! `Lock` objects are plain `{name, mode}` objects passed to the callback.

use rquickjs::Ctx;

/// Install the Web Locks API bindings into the JS context.
pub fn install_web_locks_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(WEB_LOCKS_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing W3C Web Locks Level 1 (Phase 0).
const WEB_LOCKS_SHIM: &str = r#"(function() {
  'use strict';

  // ── internal state ────────────────────────────────────────────────────────
  // _held[name] = {mode, release: fn}
  var _held = {};
  // _pending[name] = [{mode, callback, resolve, reject, signal}]
  var _pending = {};

  // ── helpers ───────────────────────────────────────────────────────────────

  // Drain pending queue for a given lock name after the lock is released.
  function _processQueue(name) {
    if (!_pending[name] || _pending[name].length === 0) return;
    if (_held[name]) return;

    while (_pending[name] && _pending[name].length > 0) {
      var entry = _pending[name].shift();
      // Skip aborted requests
      if (entry.signal && entry.signal.aborted) continue;
      _grantEntry(name, entry);
      return;
    }
  }

  // Grant a pending entry the lock for `name`.
  function _grantEntry(name, entry) {
    var released = false;
    function releaseLock() {
      if (released) return;
      released = true;
      delete _held[name];
      _processQueue(name);
    }
    _held[name] = { mode: entry.mode, release: releaseLock };

    var lock = { name: name, mode: entry.mode };
    var cbResult;
    try {
      cbResult = entry.callback(lock);
    } catch (e) {
      releaseLock();
      entry.reject(e);
      return;
    }
    Promise.resolve(cbResult).then(function(r) {
      releaseLock();
      entry.resolve(r);
    }).catch(function(err) {
      releaseLock();
      entry.reject(err);
    });
  }

  // ── LockManager ───────────────────────────────────────────────────────────

  function LockManager() {}

  // request(name, [options], callback) → Promise
  LockManager.prototype.request = function(name, optionsOrCb, maybeCb) {
    var options, callback;
    if (typeof optionsOrCb === 'function') {
      options = {};
      callback = optionsOrCb;
    } else {
      options = optionsOrCb || {};
      callback = maybeCb;
    }
    // W3C spec: name is stringified (String(name))
    name = String(name);
    if (typeof callback !== 'function') {
      return Promise.reject(new TypeError('Lock callback must be a function'));
    }

    var mode      = options.mode || 'exclusive';
    var ifAvail   = !!options.ifAvailable;
    var steal     = !!options.steal;
    var signal    = options.signal || null;

    // Validate mode
    if (mode !== 'exclusive' && mode !== 'shared') {
      return Promise.reject(new TypeError('Invalid lock mode: ' + mode));
    }

    return new Promise(function(resolve, reject) {
      // Already aborted before we start
      if (signal && signal.aborted) {
        var reason = signal.reason;
        if (!reason) {
          try { reason = new DOMException('Lock request aborted', 'AbortError'); } catch(_) { reason = new Error('AbortError'); }
        }
        reject(reason);
        return;
      }

      // steal: forcibly release any held exclusive lock
      if (steal) {
        if (_held[name]) {
          _held[name].release();
        }
        _grantEntry(name, {mode: mode, callback: callback, resolve: resolve, reject: reject, signal: signal});
        return;
      }

      // Lock is free
      if (!_held[name]) {
        _grantEntry(name, {mode: mode, callback: callback, resolve: resolve, reject: reject, signal: signal});
        return;
      }

      // ifAvailable: call callback with null when lock busy
      if (ifAvail) {
        var cbResult;
        try { cbResult = callback(null); } catch (e) { reject(e); return; }
        Promise.resolve(cbResult).then(resolve).catch(reject);
        return;
      }

      // Queue the request
      if (!_pending[name]) _pending[name] = [];
      var entry = {mode: mode, callback: callback, resolve: resolve, reject: reject, signal: signal};
      _pending[name].push(entry);

      // Handle AbortSignal
      if (signal) {
        signal.addEventListener('abort', function() {
          var queue = _pending[name];
          if (queue) {
            var idx = queue.indexOf(entry);
            if (idx >= 0) queue.splice(idx, 1);
          }
          var reason = signal.reason;
          if (!reason) {
            try { reason = new DOMException('Lock request aborted', 'AbortError'); } catch(_) { reason = new Error('AbortError'); }
          }
          reject(reason);
        });
      }
    });
  };

  // query() → Promise<{held: LockInfo[], pending: LockInfo[]}>
  LockManager.prototype.query = function() {
    var held = [];
    var pending = [];

    Object.keys(_held).forEach(function(name) {
      held.push({ name: name, mode: _held[name].mode });
    });
    Object.keys(_pending).forEach(function(name) {
      var arr = _pending[name] || [];
      arr.forEach(function(req) {
        if (!req.signal || !req.signal.aborted) {
          pending.push({ name: name, mode: req.mode });
        }
      });
    });

    return Promise.resolve({ held: held, pending: pending });
  };

  // ── install on navigator ──────────────────────────────────────────────────
  var _lockManager = new LockManager();

  // navigator may not yet exist in test contexts; install safely.
  if (typeof navigator === 'undefined') {
    globalThis.navigator = {};
  }
  Object.defineProperty(navigator, 'locks', {
    get: function() { return _lockManager; },
    configurable: true,
    enumerable: true,
  });

  globalThis.LockManager = LockManager;
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

    fn with_web_locks(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Minimal stubs needed by the shim
            ctx.eval::<(), _>(
                r#"
                var window = globalThis;
                function DOMException(msg, name) {
                    var e = new Error(msg);
                    e.name = name || 'Error';
                    return e;
                }
                globalThis.DOMException = DOMException;
                "#,
            )
            .unwrap();
            install_web_locks_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn lock_manager_class_exists() {
        with_web_locks(|ctx| {
            let ok: bool = ctx
                .eval("typeof LockManager === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn navigator_locks_is_lock_manager() {
        with_web_locks(|ctx| {
            let ok: bool = ctx
                .eval("navigator.locks instanceof LockManager")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn request_returns_promise() {
        with_web_locks(|ctx| {
            let ok: bool = ctx
                .eval(
                    "typeof navigator.locks.request('mylock', function(l) { return Promise.resolve(); }) === 'object'",
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn request_callback_receives_lock_object() {
        with_web_locks(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var sawName = false;
                    var sawMode = false;
                    navigator.locks.request('r', function(lock) {
                        sawName = lock && lock.name === 'r';
                        sawMode = lock && lock.mode === 'exclusive';
                        return Promise.resolve();
                    });
                    sawName && sawMode
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn query_returns_promise_with_snapshot() {
        with_web_locks(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var p = navigator.locks.query();
                    typeof p === 'object' && typeof p.then === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn if_available_calls_callback_with_null_when_held() {
        with_web_locks(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var gotNull = false;
                    // Acquire the lock and hold it (return a never-resolving Promise)
                    navigator.locks.request('busy', function(l) {
                        // Hold indefinitely
                        return new Promise(function() {});
                    });
                    // Now try ifAvailable — should call callback(null)
                    navigator.locks.request('busy', {ifAvailable: true}, function(lock) {
                        gotNull = (lock === null);
                        return Promise.resolve();
                    });
                    gotNull
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}

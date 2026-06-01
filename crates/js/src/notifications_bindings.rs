//! Web Notifications API (W3C Notifications API Level 1).
//!
//! Implements `new Notification(title, opts)`, `Notification.requestPermission()`,
//! and the `Notification.permission` static property.
//!
//! Phase 0 scope:
//! - Full `Notification` constructor and instance API (title/body/icon/tag/data/etc.)
//! - `requestPermission()` → Promise<"granted"|"denied">
//! - Events: `show`, `close`, `click`, `error` via `onclick`/`onclose`/etc.
//! - `close()` method
//! - Shell integration: `_lumen_show_notification` queues requests for OS delivery.
//! - Default permission: `"denied"` (privacy-first). Shell may enable via `allow=true`.

use rquickjs::{Ctx, Function};
use std::sync::{Arc, Mutex};

/// A notification request queued by `new Notification(...)` in JS.
///
/// Shell drains this queue in `about_to_wait` and delivers each entry to the OS
/// notification subsystem via `notification::show_os_notification`.
pub struct NotificationRequest {
    /// Browser-assigned sequential ID. Matches the `_id` field in the JS object.
    pub id: u32,
    /// Notification title (required, always a non-empty string).
    pub title: String,
    /// Notification body text (`options.body`). Empty string if not provided.
    pub body: String,
}

/// Shared queue of pending notification requests.
///
/// Written by the `_lumen_show_notification` native binding (JS thread);
/// drained by the shell in `about_to_wait` (main thread).
pub type NotificationQueue = Arc<Mutex<Vec<NotificationRequest>>>;

/// Install Web Notifications API globals into the JS context.
///
/// Registers three native bindings:
/// - `_lumen_show_notification(id, title, body)` → pushes to `pending` queue.
/// - `_lumen_notification_close(id)` → no-op stub (OS dismissal not wired yet).
/// - `_lumen_notification_request_permission()` → returns permission string.
///
/// Then evaluates `NOTIFICATIONS_SHIM` which defines `window.Notification`.
///
/// `allow` controls the initial `Notification.permission` value:
/// - `false` (default) → `"denied"` — sites cannot show notifications without
///   explicit user opt-in in the permission UI.
/// - `true` → `"granted"` — shell opted in (e.g. user toggled in per-site prefs).
///
/// Must be called **after** `dom::install_dom_api` so that `Event`, `Promise`,
/// and `queueMicrotask` are already defined.
pub fn install_notifications_bindings(
    ctx: &Ctx<'_>,
    pending: NotificationQueue,
    allow: bool,
) -> rquickjs::Result<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals()
                .set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // _lumen_show_notification(id, title, body) → bool
    // Enqueues the request for OS delivery. Returns false if the queue lock
    // is poisoned (should never happen in practice).
    {
        let q = Arc::clone(&pending);
        reg!(
            "_lumen_show_notification",
            move |id: u32, title: String, body: String| -> bool {
                match q.lock() {
                    Ok(mut queue) => {
                        queue.push(NotificationRequest { id, title, body });
                        true
                    }
                    Err(_) => false,
                }
            }
        );
    }

    // _lumen_notification_close(id)
    // Phase 0 stub — OS-side close not yet implemented.
    reg!("_lumen_notification_close", |_id: u32| {});

    // _lumen_notification_request_permission() → "granted" | "denied"
    // Returns the permission level the shell configured at init time.
    // No interactive dialog in Phase 0.
    let perm = if allow { "granted" } else { "denied" };
    reg!(
        "_lumen_notification_request_permission",
        move || -> String { perm.to_string() }
    );

    // Inject initial permission so the JS shim can read it before the first call.
    let init = format!("globalThis.__LUMEN_NOTIF_PERM = '{perm}';");
    ctx.eval::<(), _>(init.as_str())?;

    ctx.eval::<(), _>(NOTIFICATIONS_SHIM)?;
    Ok(())
}

/// Drain all pending notification requests from the queue.
///
/// Called by the shell in `about_to_wait` to retrieve notifications queued
/// by JS since the last drain.  Returns an empty vec when nothing is pending.
pub fn drain_notifications(queue: &NotificationQueue) -> Vec<NotificationRequest> {
    match queue.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => Vec::new(),
    }
}

// ─── JavaScript shim ─────────────────────────────────────────────────────────

const NOTIFICATIONS_SHIM: &str = r#"(function() {
  'use strict';

  // Permission state shared by all Notification instances on this page.
  var _permission = (typeof __LUMEN_NOTIF_PERM !== 'undefined')
    ? __LUMEN_NOTIF_PERM : 'default';
  try { delete globalThis.__LUMEN_NOTIF_PERM; } catch(e) {}

  var _next_id = 1;
  // Active (not yet closed) Notification instances keyed by id.
  // Kept so future click/close delivery can find the right instance.
  var _active = {};

  // ── constructor ────────────────────────────────────────────────────────────

  /**
   * Notification(title[, options]) — W3C Notifications API Level 1 §2.
   *
   * Fires 'show' immediately when permission is 'granted'.
   * Does nothing (silent drop) when permission is 'denied'.
   */
  function Notification(title, options) {
    if (!(this instanceof Notification)) {
      throw new TypeError(
        "Failed to construct 'Notification': Please use the 'new' operator."
      );
    }
    if (arguments.length === 0) {
      throw new TypeError(
        "Failed to construct 'Notification': 1 argument required, but 0 present."
      );
    }

    options = (options !== null && typeof options === 'object') ? options : {};

    this._id = _next_id++;
    this._closed = false;
    this._listeners = Object.create(null);

    // Required
    this.title = String(title);

    // Optional option bag
    this.dir           = options.dir   || 'auto';
    this.lang          = options.lang  || '';
    this.body          = (typeof options.body  === 'string') ? options.body  : '';
    this.tag           = (typeof options.tag   === 'string') ? options.tag   : '';
    this.icon          = (typeof options.icon  === 'string') ? options.icon  : '';
    this.badge         = (typeof options.badge === 'string') ? options.badge : '';
    this.image         = (typeof options.image === 'string') ? options.image : '';
    this.data          = (options.data !== undefined)        ? options.data  : null;
    this.requireInteraction = !!options.requireInteraction;
    this.silent        = options.silent === true;
    this.renotify      = !!options.renotify;
    this.timestamp     = (typeof options.timestamp === 'number')
      ? options.timestamp : Date.now();
    this.vibrate       = Array.isArray(options.vibrate) ? options.vibrate : [];

    // Event handlers
    this.onclick  = null;
    this.onclose  = null;
    this.onerror  = null;
    this.onshow   = null;

    _active[this._id] = this;

    // Spec §6: if permission is granted, queue a task to show the notification.
    if (_permission === 'granted') {
      var self = this;
      try {
        _lumen_show_notification(this._id, this.title, this.body);
      } catch(e) {}
      queueMicrotask(function() {
        if (!self._closed) {
          self._fire('show');
        }
      });
    }
  }

  // ── instance methods ───────────────────────────────────────────────────────

  /**
   * close() — dismiss the notification and fire the 'close' event.
   */
  Notification.prototype.close = function() {
    if (this._closed) return;
    this._closed = true;
    try { _lumen_notification_close(this._id); } catch(e) {}
    delete _active[this._id];
    this._fire('close');
  };

  Notification.prototype.addEventListener = function(type, fn, _opts) {
    if (typeof fn !== 'function') return;
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
  };

  Notification.prototype.removeEventListener = function(type, fn) {
    var lst = this._listeners[type];
    if (!lst) return;
    this._listeners[type] = lst.filter(function(f) { return f !== fn; });
  };

  Notification.prototype.dispatchEvent = function(event) {
    this._fire(event.type, event);
    return true;
  };

  /** Internal: construct and dispatch a Notification event. */
  Notification.prototype._fire = function(type, eventArg) {
    var ev = eventArg || new Event(type);
    var handler = this['on' + type];
    if (typeof handler === 'function') {
      try { handler.call(this, ev); } catch(e) {}
    }
    var lst = this._listeners[type];
    if (lst) {
      var copy = lst.slice();
      for (var i = 0; i < copy.length; i++) {
        try { copy[i].call(this, ev); } catch(e) {}
      }
    }
  };

  // ── static members ─────────────────────────────────────────────────────────

  /**
   * Notification.permission — read-only static string.
   * One of: "default" | "granted" | "denied".
   */
  Object.defineProperty(Notification, 'permission', {
    get: function() { return _permission; },
    enumerable: true,
    configurable: false,
  });

  /**
   * Notification.maxActions — maximum number of actions supported.
   * Phase 0: 0 (actions not implemented).
   */
  Object.defineProperty(Notification, 'maxActions', {
    value: 0,
    writable: false,
    enumerable: true,
    configurable: false,
  });

  /**
   * Notification.requestPermission([callback]) → Promise<"granted"|"denied">
   *
   * W3C spec §6.1: asks the shell for the current permission level.
   * Phase 0: no interactive dialog — shell returns a fixed value at init.
   */
  Notification.requestPermission = function(callback) {
    return new Promise(function(resolve) {
      var result;
      try {
        result = _lumen_notification_request_permission();
      } catch(e) {
        result = 'denied';
      }
      _permission = result;
      if (typeof callback === 'function') {
        try { callback(result); } catch(e) {}
      }
      resolve(result);
    });
  };

  window.Notification = Notification;
})();"#;

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn setup_dom_stubs(ctx: &Context) {
        ctx.with(|c| {
            // globalThis is a QuickJS built-in — assign globals directly on it
            // so they are accessible as free variables in subsequent evals.
            c.eval::<(), _>(
                r#"
globalThis.window = globalThis;
globalThis.Event = function(type, _init) { this.type = type; };
globalThis.queueMicrotask = function(fn) { fn(); };
// Synchronous Promise stub — executor runs immediately; .then fires synchronously.
// Required so requestPermission().then(cb) resolves in the same tick for tests.
globalThis.Promise = (function() {
  function P(exec) {
    var self = this;
    self._val = undefined;
    self._cbs = [];
    self._done = false;
    exec(function(v) {
      self._val = v;
      self._done = true;
      for (var i = 0; i < self._cbs.length; i++) { self._cbs[i](v); }
    });
  }
  P.prototype.then = function(fn) {
    if (this._done) { fn(this._val); } else { this._cbs.push(fn); }
    return this;
  };
  return P;
})();
"#,
            )
            .unwrap();
        });
    }

    fn install(ctx: &Context, allow: bool) -> NotificationQueue {
        let q: NotificationQueue = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|c| {
            install_notifications_bindings(&c, Arc::clone(&q), allow).unwrap();
        });
        q
    }

    #[test]
    fn permission_denied_by_default() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let perm: String = ctx
            .with(|c| c.eval("Notification.permission"))
            .unwrap();
        assert_eq!(perm, "denied");
    }

    #[test]
    fn permission_granted_when_allowed() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, true);
        let perm: String = ctx
            .with(|c| c.eval("Notification.permission"))
            .unwrap();
        assert_eq!(perm, "granted");
    }

    #[test]
    fn request_permission_returns_string() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let perm: String = ctx
            .with(|c| {
                c.eval(
                    r#"
var result = '';
Notification.requestPermission().then(function(p) { result = p; });
result
"#,
                )
            })
            .unwrap();
        assert_eq!(perm, "denied");
    }

    #[test]
    fn request_permission_callback_called() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, true);
        let perm: String = ctx
            .with(|c| {
                c.eval(
                    r#"
var cbResult = '';
Notification.requestPermission(function(p) { cbResult = p; });
cbResult
"#,
                )
            })
            .unwrap();
        assert_eq!(perm, "granted");
    }

    #[test]
    fn notification_shows_when_granted() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        let q = install(&ctx, true);
        ctx.with(|c| {
            c.eval::<(), _>(
                r#"var n = new Notification('Hello', { body: 'World' });"#,
            )
            .unwrap();
        });
        let drained = drain_notifications(&q);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].title, "Hello");
        assert_eq!(drained[0].body, "World");
    }

    #[test]
    fn notification_silent_when_denied() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        let q = install(&ctx, false);
        ctx.with(|c| {
            c.eval::<(), _>("var n = new Notification('Hello');").unwrap();
        });
        assert!(drain_notifications(&q).is_empty());
    }

    #[test]
    fn notification_properties() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let title: String = ctx
            .with(|c| {
                c.eval(
                    r#"
var n = new Notification('Title', { body: 'Body', tag: 'my-tag', silent: true });
n.title
"#,
                )
            })
            .unwrap();
        assert_eq!(title, "Title");

        let body: String = ctx
            .with(|c| c.eval("n.body"))
            .unwrap();
        assert_eq!(body, "Body");

        let tag: String = ctx
            .with(|c| c.eval("n.tag"))
            .unwrap();
        assert_eq!(tag, "my-tag");

        let silent: bool = ctx
            .with(|c| c.eval("n.silent"))
            .unwrap();
        assert!(silent);
    }

    #[test]
    fn notification_close_fires_event() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let closed: bool = ctx
            .with(|c| {
                c.eval(
                    r#"
var n = new Notification('Test');
var fired = false;
n.onclose = function() { fired = true; };
n.close();
fired
"#,
                )
            })
            .unwrap();
        assert!(closed);
    }

    #[test]
    fn notification_close_idempotent() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let count: i32 = ctx
            .with(|c| {
                c.eval(
                    r#"
var n = new Notification('Test');
var count = 0;
n.onclose = function() { count++; };
n.close();
n.close();
count
"#,
                )
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn notification_add_remove_listener() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let fired: bool = ctx
            .with(|c| {
                c.eval(
                    r#"
var n = new Notification('Test');
var count = 0;
function handler() { count++; }
n.addEventListener('close', handler);
n.removeEventListener('close', handler);
n.close();
count === 0
"#,
                )
            })
            .unwrap();
        assert!(fired);
    }

    #[test]
    fn show_queued_when_granted() {
        // Verifies that a notification shown with 'granted' permission is delivered
        // to the OS queue (separate from the JS 'show' event).
        // The 'show' event fires via queueMicrotask (asynchronous in production),
        // so we verify the OS-side queue rather than the JS event handler here.
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        let q = install(&ctx, true);
        ctx.with(|c| {
            c.eval::<(), _>("new Notification('Queued');").unwrap();
        });
        let items = drain_notifications(&q);
        assert_eq!(items.len(), 1, "expected 1 queued notification");
        assert_eq!(items[0].title, "Queued");
    }

    #[test]
    fn max_actions_is_zero() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let max: i32 = ctx
            .with(|c| c.eval("Notification.maxActions"))
            .unwrap();
        assert_eq!(max, 0);
    }

    #[test]
    fn notification_requires_new() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let threw: bool = ctx
            .with(|c| {
                c.eval(
                    r#"
var threw = false;
try { Notification('no-new'); } catch(e) { threw = true; }
threw
"#,
                )
            })
            .unwrap();
        assert!(threw);
    }

    #[test]
    fn drain_empty_queue_returns_empty() {
        let q: NotificationQueue = Arc::new(Mutex::new(Vec::new()));
        assert!(drain_notifications(&q).is_empty());
    }

    #[test]
    fn multiple_notifications_queued() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        let q = install(&ctx, true);
        ctx.with(|c| {
            c.eval::<(), _>(
                r#"
new Notification('First');
new Notification('Second', { body: 'body2' });
new Notification('Third');
"#,
            )
            .unwrap();
        });
        let drained = drain_notifications(&q);
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].title, "First");
        assert_eq!(drained[1].title, "Second");
        assert_eq!(drained[2].title, "Third");
    }

    #[test]
    fn drain_clears_queue() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        let q = install(&ctx, true);
        ctx.with(|c| {
            c.eval::<(), _>("new Notification('X');").unwrap();
        });
        let first = drain_notifications(&q);
        let second = drain_notifications(&q);
        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }

    #[test]
    fn notification_title_coerced_to_string() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let title: String = ctx
            .with(|c| {
                c.eval(
                    r#"
var n = new Notification(42);
n.title
"#,
                )
            })
            .unwrap();
        assert_eq!(title, "42");
    }

    #[test]
    fn no_args_throws_type_error() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let threw: bool = ctx
            .with(|c| {
                c.eval(
                    r#"
var threw = false;
try { new Notification(); } catch(e) { threw = e instanceof TypeError; }
threw
"#,
                )
            })
            .unwrap();
        assert!(threw);
    }

    #[test]
    fn permission_mutation_via_request_permission() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, true);
        let perm: String = ctx
            .with(|c| {
                c.eval(
                    r#"
var result = '';
Notification.requestPermission().then(function(p) { result = p; });
result
"#,
                )
            })
            .unwrap();
        assert_eq!(perm, "granted");
    }

    #[test]
    fn onshow_not_fired_when_denied() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let shown: bool = ctx
            .with(|c| {
                c.eval(
                    r#"
var shown = false;
var n = new Notification('Hi');
n.onshow = function() { shown = true; };
shown
"#,
                )
            })
            .unwrap();
        assert!(!shown);
    }

    #[test]
    fn notification_data_preserved() {
        let (_rt, ctx) = make_ctx();
        setup_dom_stubs(&ctx);
        install(&ctx, false);
        let val: i32 = ctx
            .with(|c| {
                c.eval(
                    r#"
var n = new Notification('X', { data: 42 });
n.data
"#,
                )
            })
            .unwrap();
        assert_eq!(val, 42);
    }
}

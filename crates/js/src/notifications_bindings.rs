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

/// Install Web Notifications API globals into the JS context (Ph3 V8 migration
/// S5-S7 batch 3; the rquickjs twin was removed in S12b-B19): the three
/// natives capture a clone of `rt`'s queue (accessed via
/// [`crate::v8_runtime::V8JsRuntime::notification_queue`]). Then evaluates
/// `NOTIFICATIONS_SHIM` which defines `window.Notification`.
///
/// `allow` controls the initial `Notification.permission` value:
/// - `false` (default) → `"denied"` — sites cannot show notifications without
///   explicit user opt-in in the permission UI.
/// - `true` → `"granted"` — shell opted in (e.g. user toggled in per-site prefs).
///
/// Must be called after the core DOM install.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_notifications_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    allow: bool,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn3};
    use lumen_core::ext::JsRuntime as _;

    let queue = rt.notification_queue();

    let show = {
        let q = Arc::clone(&queue);
        into_v8_fn3(move |id: u32, title: String, body: String| -> bool {
            match q.lock() {
                Ok(mut queue) => {
                    queue.push(NotificationRequest { id, title, body });
                    true
                }
                Err(_) => false,
            }
        })
    };
    rt.register_native("_lumen_show_notification", show)?;

    let close = into_v8_fn1(move |_id: u32| {});
    rt.register_native("_lumen_notification_close", close)?;

    let perm = if allow { "granted" } else { "denied" };
    let request_permission = into_v8_fn0(move || -> String { perm.to_string() });
    rt.register_native("_lumen_notification_request_permission", request_permission)?;

    rt.eval(&format!("globalThis.__LUMEN_NOTIF_PERM = '{perm}';"))?;
    rt.eval(NOTIFICATIONS_SHIM)?;
    Ok(())
}

// ─── JavaScript shim ─────────────────────────────────────────────────────────

#[cfg(feature = "v8-backend")]
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
      var moved = (result !== _permission);
      _permission = result;
      // W3C Permissions §"permission state change": navigator.permissions
      // reports 'notifications' straight off Notification.permission, so any
      // PermissionStatus the page holds for it has to hear about the move
      // (BUG-386). Resolved at call time, not captured at install: the
      // Permissions shim installs after this one.
      if (moved && typeof _lumen_permission_state_changed === 'function') {
        try { _lumen_permission_state_changed('notifications'); } catch(e) {}
      }
      if (typeof callback === 'function') {
        try { callback(result); } catch(e) {}
      }
      resolve(result);
    });
  };

  window.Notification = Notification;

  // ── ServiceWorkerRegistration integration (W3C Notifications API §5) ──────

  // showNotification(title, options) → Promise<undefined>
  // Creates a Notification through the SW registration, delegating to the
  // Notification constructor so it goes through the same permission check and
  // OS queue. Resolves immediately (Phase 0 — no delivery confirmation).
  //
  // getNotifications(filter?) → Promise<Notification[]>
  // Phase 0: returns an empty array. Persistent notification registry and
  // tag-based filtering are deferred to a future phase.
  if (typeof ServiceWorkerRegistration !== 'undefined') {
    ServiceWorkerRegistration.prototype.showNotification = function(title, options) {
      return new Promise(function(resolve) {
        new Notification(title, options);
        resolve(undefined);
      });
    };

    ServiceWorkerRegistration.prototype.getNotifications = function(_filter) {
      return new Promise(function(resolve) { resolve([]); });
    };
  }
})();"#;

// ─── tests ────────────────────────────────────────────────────────────────────

/// V8 test coverage for the Notifications API shim (the rquickjs twin was
/// removed in S12b-B19; this module ports its 26 tests to V8 verbatim).
#[cfg(all(test, feature = "v8-backend"))]
mod tests_v8 {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    // `Event`/`window`/`queueMicrotask` stubs — V8 has no DOM, so the shim's
    // `new Event(...)`, `window.Notification = ...` and 'show' dispatch need
    // minimal globals. Real V8 Promises are used as-is: `.then()` callbacks
    // run as microtasks, which V8 auto-drains between separate `eval` calls
    // (not within a single one — tests reading a `.then`-set variable split
    // setup and read into two `eval` calls for that reason).
    const STUBS: &str = r#"
        var window = globalThis;
        globalThis.Event = function(type, _init) { this.type = type; };
        globalThis.queueMicrotask = function(fn) { fn(); };
    "#;

    fn rt_with_notifications(allow: bool) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        install_notifications_bindings_v8(&rt, allow).unwrap();
        rt
    }

    fn rt_with_sw_registration(allow: bool) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(STUBS).unwrap();
        rt.eval("globalThis.ServiceWorkerRegistration = function() {};")
            .unwrap();
        install_notifications_bindings_v8(&rt, allow).unwrap();
        rt
    }

    #[test]
    fn permission_denied_by_default() {
        let rt = rt_with_notifications(false);
        assert_eq!(
            rt.eval("Notification.permission").unwrap(),
            JsValue::String("denied".to_string())
        );
    }

    #[test]
    fn permission_granted_when_allowed() {
        let rt = rt_with_notifications(true);
        assert_eq!(
            rt.eval("Notification.permission").unwrap(),
            JsValue::String("granted".to_string())
        );
    }

    #[test]
    fn request_permission_returns_string() {
        // V8 auto-drains microtasks between separate `eval` calls, so the
        // `.then()` scheduled below has already run by the time the second
        // `eval` reads `result` — unlike within a single `eval`.
        let rt = rt_with_notifications(false);
        rt.eval(
            r#"
var result = '';
Notification.requestPermission().then(function(p) { result = p; });
"#,
        )
        .unwrap();
        let perm = rt.eval("result").unwrap();
        assert_eq!(perm, JsValue::String("denied".to_string()));
    }

    #[test]
    fn request_permission_callback_called() {
        let rt = rt_with_notifications(true);
        let perm = rt
            .eval(
                r#"
var cbResult = '';
Notification.requestPermission(function(p) { cbResult = p; });
cbResult
"#,
            )
            .unwrap();
        assert_eq!(perm, JsValue::String("granted".to_string()));
    }

    #[test]
    fn notification_shows_when_granted() {
        let rt = rt_with_notifications(true);
        rt.eval("var n = new Notification('Hello', { body: 'World' });")
            .unwrap();
        let drained = rt.take_notification_requests();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].title, "Hello");
        assert_eq!(drained[0].body, "World");
    }

    #[test]
    fn notification_silent_when_denied() {
        let rt = rt_with_notifications(false);
        rt.eval("var n = new Notification('Hello');").unwrap();
        assert!(rt.take_notification_requests().is_empty());
    }

    #[test]
    fn notification_properties() {
        let rt = rt_with_notifications(false);
        rt.eval(
            r#"var n = new Notification('Title', { body: 'Body', tag: 'my-tag', silent: true });"#,
        )
        .unwrap();
        assert_eq!(rt.eval("n.title").unwrap(), JsValue::String("Title".to_string()));
        assert_eq!(rt.eval("n.body").unwrap(), JsValue::String("Body".to_string()));
        assert_eq!(rt.eval("n.tag").unwrap(), JsValue::String("my-tag".to_string()));
        assert_eq!(rt.eval("n.silent").unwrap(), JsValue::Bool(true));
    }

    #[test]
    fn notification_close_fires_event() {
        let rt = rt_with_notifications(false);
        let closed = rt
            .eval(
                r#"
var n = new Notification('Test');
var fired = false;
n.onclose = function() { fired = true; };
n.close();
fired
"#,
            )
            .unwrap();
        assert_eq!(closed, JsValue::Bool(true));
    }

    #[test]
    fn notification_close_idempotent() {
        let rt = rt_with_notifications(false);
        let count = rt
            .eval(
                r#"
var n = new Notification('Test');
var count = 0;
n.onclose = function() { count++; };
n.close();
n.close();
count
"#,
            )
            .unwrap();
        assert_eq!(count, JsValue::Number(1.0));
    }

    #[test]
    fn notification_add_remove_listener() {
        let rt = rt_with_notifications(false);
        let fired = rt
            .eval(
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
            .unwrap();
        assert_eq!(fired, JsValue::Bool(true));
    }

    #[test]
    fn show_queued_when_granted() {
        // Verifies that a notification shown with 'granted' permission is delivered
        // to the OS queue (separate from the JS 'show' event).
        let rt = rt_with_notifications(true);
        rt.eval("new Notification('Queued');").unwrap();
        let items = rt.take_notification_requests();
        assert_eq!(items.len(), 1, "expected 1 queued notification");
        assert_eq!(items[0].title, "Queued");
    }

    #[test]
    fn max_actions_is_zero() {
        let rt = rt_with_notifications(false);
        assert_eq!(
            rt.eval("Notification.maxActions").unwrap(),
            JsValue::Number(0.0)
        );
    }

    #[test]
    fn notification_requires_new() {
        let rt = rt_with_notifications(false);
        let threw = rt
            .eval(
                r#"
var threw = false;
try { Notification('no-new'); } catch(e) { threw = true; }
threw
"#,
            )
            .unwrap();
        assert_eq!(threw, JsValue::Bool(true));
    }

    #[test]
    fn drain_empty_queue_returns_empty() {
        let q: NotificationQueue = Arc::new(Mutex::new(Vec::new()));
        assert!(drain_notifications(&q).is_empty());
    }

    #[test]
    fn multiple_notifications_queued() {
        let rt = rt_with_notifications(true);
        rt.eval(
            r#"
new Notification('First');
new Notification('Second', { body: 'body2' });
new Notification('Third');
"#,
        )
        .unwrap();
        let drained = rt.take_notification_requests();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].title, "First");
        assert_eq!(drained[1].title, "Second");
        assert_eq!(drained[2].title, "Third");
    }

    #[test]
    fn drain_clears_queue() {
        let rt = rt_with_notifications(true);
        rt.eval("new Notification('X');").unwrap();
        let first = rt.take_notification_requests();
        let second = rt.take_notification_requests();
        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }

    #[test]
    fn notification_title_coerced_to_string() {
        let rt = rt_with_notifications(false);
        rt.eval("var n = new Notification(42);").unwrap();
        assert_eq!(rt.eval("n.title").unwrap(), JsValue::String("42".to_string()));
    }

    #[test]
    fn no_args_throws_type_error() {
        let rt = rt_with_notifications(false);
        let threw = rt
            .eval(
                r#"
var threw = false;
try { new Notification(); } catch(e) { threw = e instanceof TypeError; }
threw
"#,
            )
            .unwrap();
        assert_eq!(threw, JsValue::Bool(true));
    }

    #[test]
    fn permission_mutation_via_request_permission() {
        let rt = rt_with_notifications(true);
        rt.eval(
            r#"
var result = '';
Notification.requestPermission().then(function(p) { result = p; });
"#,
        )
        .unwrap();
        let perm = rt.eval("result").unwrap();
        assert_eq!(perm, JsValue::String("granted".to_string()));
    }

    /// BUG-386: `navigator.permissions` reads 'notifications' straight off
    /// `Notification.permission`, so a `PermissionStatus` the page is holding
    /// has to be told when `requestPermission()` moves it. The native is
    /// swapped here because the shell currently hands the same value to both
    /// halves of the install, so the value never moves on its own.
    #[test]
    fn request_permission_notifies_the_permissions_api_on_a_move() {
        let rt = rt_with_notifications(false);
        rt.eval(
            r#"
var notified = [];
globalThis._lumen_permission_state_changed = function(name) { notified.push(name); };
globalThis._lumen_notification_request_permission = function() { return 'granted'; };
Notification.requestPermission();
"#,
        )
        .unwrap();
        assert_eq!(
            rt.eval("notified.join(',')").unwrap(),
            JsValue::String("notifications".to_string())
        );
    }

    /// A second call changes nothing, so nothing is announced — a `change`
    /// event for an unmoved value would misreport the engine's state.
    #[test]
    fn request_permission_is_silent_when_the_value_does_not_move() {
        let rt = rt_with_notifications(false);
        rt.eval(
            r#"
var notified = [];
globalThis._lumen_permission_state_changed = function(name) { notified.push(name); };
Notification.requestPermission();
Notification.requestPermission();
"#,
        )
        .unwrap();
        assert_eq!(rt.eval("notified.length === 0").unwrap(), JsValue::Bool(true));
    }

    #[test]
    fn onshow_not_fired_when_denied() {
        let rt = rt_with_notifications(false);
        let shown = rt
            .eval(
                r#"
var shown = false;
var n = new Notification('Hi');
n.onshow = function() { shown = true; };
shown
"#,
            )
            .unwrap();
        assert_eq!(shown, JsValue::Bool(false));
    }

    #[test]
    fn notification_data_preserved() {
        let rt = rt_with_notifications(false);
        rt.eval("var n = new Notification('X', { data: 42 });").unwrap();
        assert_eq!(rt.eval("n.data").unwrap(), JsValue::Number(42.0));
    }

    // ── ServiceWorkerRegistration tests ───────────────────────────────────────

    #[test]
    fn sw_show_notification_returns_promise() {
        let rt = rt_with_sw_registration(true);
        let is_promise = rt
            .eval(
                r#"
var reg = new ServiceWorkerRegistration();
var p = reg.showNotification('SW hello', { body: 'world' });
typeof p.then === 'function'
"#,
            )
            .unwrap();
        assert_eq!(
            is_promise,
            JsValue::Bool(true),
            "showNotification() should return a thenable"
        );
    }

    #[test]
    fn sw_show_notification_queues_to_os() {
        let rt = rt_with_sw_registration(true);
        rt.eval(
            r#"
var reg = new ServiceWorkerRegistration();
reg.showNotification('SW push', { body: 'payload' });
"#,
        )
        .unwrap();
        let drained = rt.take_notification_requests();
        assert_eq!(drained.len(), 1, "notification should reach OS queue");
        assert_eq!(drained[0].title, "SW push");
        assert_eq!(drained[0].body, "payload");
    }

    #[test]
    fn sw_show_notification_silent_when_denied() {
        let rt = rt_with_sw_registration(false);
        rt.eval(
            r#"
var reg = new ServiceWorkerRegistration();
reg.showNotification('Silent');
"#,
        )
        .unwrap();
        assert!(
            rt.take_notification_requests().is_empty(),
            "showNotification() must respect denied permission"
        );
    }

    #[test]
    fn sw_get_notifications_returns_empty_array() {
        let rt = rt_with_sw_registration(true);
        rt.eval(
            r#"
var reg = new ServiceWorkerRegistration();
var result = -1;
reg.getNotifications().then(function(list) { result = list.length; });
"#,
        )
        .unwrap();
        let len = rt.eval("result").unwrap();
        assert_eq!(
            len,
            JsValue::Number(0.0),
            "getNotifications() should resolve with empty array"
        );
    }

    #[test]
    fn sw_get_notifications_with_filter_returns_empty_array() {
        let rt = rt_with_sw_registration(true);
        rt.eval(
            r#"
var reg = new ServiceWorkerRegistration();
var result = -1;
reg.getNotifications({ tag: 'news' }).then(function(list) { result = list.length; });
"#,
        )
        .unwrap();
        let len = rt.eval("result").unwrap();
        assert_eq!(
            len,
            JsValue::Number(0.0),
            "getNotifications({{tag}}) should still resolve with empty array in Phase 0"
        );
    }
}

//! Idle Detection API (WICG Idle Detection) — Phase 1.
//!
//! **Phase 0** (prior): `requestPermission()` → `'granted'`, `start()` resolves
//! immediately with fixed `{userState:'active', screenState:'unlocked'}`.
//! The `'change'` event never fired.
//!
//! **Phase 1** (this file): `__lumen_idle_get_idle_ms()` native binding
//! returns milliseconds since the last user input via OS APIs:
//! - **Windows**: `GetLastInputInfo` + `GetTickCount` (Win32).
//! - **Linux / macOS**: returns 0 — idle state stays `'active'`.
//!
//! The `IdleDetector.start()` JS method now schedules a polling interval
//! at `max(30 000, threshold / 2)` ms.  On each tick it compares the OS
//! idle time to `options.threshold` and dispatches a `'change'` event
//! whenever `userState` transitions between `'active'` and `'idle'`.
//!
//! # Registered native bindings
//!
//! | Name | Signature | Description |
//! |---|---|---|
//! | `__lumen_idle_get_idle_ms` | `() → number` | Milliseconds since last user input |

// ── OS idle-time query ────────────────────────────────────────────────────────

/// Returns milliseconds elapsed since the last user-input event.
///
/// On Windows, queries `GetLastInputInfo` + `GetTickCount` (Win32).
/// On other platforms returns 0 so the idle state always stays `'active'`.
#[cfg(all(feature = "v8-backend", target_os = "windows"))]
fn user_idle_ms() -> u64 {
    use std::mem;

    #[repr(C)]
    struct LastInputInfo {
        cb_size: u32,
        dw_time: u32,
    }

    // SAFETY: GetLastInputInfo and GetTickCount are pure OS reads with no memory hazards.
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetLastInputInfo(plii: *mut LastInputInfo) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount() -> u32;
    }

    let mut info = LastInputInfo {
        cb_size: mem::size_of::<LastInputInfo>() as u32,
        dw_time: 0,
    };
    // SAFETY: `info` is a valid LASTINPUTINFO with cb_size initialised.
    unsafe {
        if GetLastInputInfo(&mut info) != 0 {
            let now = GetTickCount();
            now.wrapping_sub(info.dw_time) as u64
        } else {
            0
        }
    }
}

#[cfg(all(feature = "v8-backend", not(target_os = "windows")))]
fn user_idle_ms() -> u64 {
    0
}

// ── Native binding installation ───────────────────────────────────────────────

/// Install Idle Detection API bindings into the JS context (Ph3 V8 migration
/// S5-S7 batch 2): the native goes through the compat layer, the JS shim
/// evaluates unchanged.
///
/// Must run after the DOM shim so that `window`, `EventTarget`, `Promise`,
/// `setInterval`, and `clearInterval` are available.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_idle_detection_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn0;
    use lumen_core::ext::JsRuntime as _;

    let native = into_v8_fn0(move || -> f64 { user_idle_ms() as f64 });
    rt.register_native("__lumen_idle_get_idle_ms", native)?;
    rt.eval(IDLE_DETECTION_SHIM)?;
    Ok(())
}

/// JavaScript shim implementing WICG Idle Detection API (Phase 1).
///
/// Polls `__lumen_idle_get_idle_ms()` every `max(30_000, threshold/2)` ms
/// and dispatches a `'change'` event when `userState` transitions.
#[cfg(feature = "v8-backend")]
const IDLE_DETECTION_SHIM: &str = r#"
(function(global) {
  'use strict';

  // IdleDetector — detects user/screen idle state (WICG Idle Detection API).
  // Extends EventTarget so callers can use addEventListener('change', ...).
  class IdleDetector extends EventTarget {
    constructor() {
      super();
      this._userState   = null;
      this._screenState = null;
      this._started     = false;
      this._threshold   = 60000;
      this._timer       = null;
    }

    // WICG §4.2: current user-activity state.
    // Returns 'active' | 'idle' | null (null before start()).
    get userState()   { return this._userState;   }

    // WICG §4.2: current screen-lock state.
    // Returns 'locked' | 'unlocked' | null (null before start()).
    get screenState() { return this._screenState; }

    // WICG §4.3: request permission to observe idle state.
    // Auto-granted — Lumen does not expose a permission prompt for this API.
    static requestPermission() {
      return Promise.resolve('granted');
    }

    // WICG §4.4: begin observing idle state.
    // threshold — minimum idle duration in ms before userState flips to 'idle'.
    start(options) {
      var threshold = (options && typeof options.threshold === 'number')
        ? options.threshold : 60000;
      if (threshold < 60000) {
        return Promise.reject(
          new RangeError('Idle detection threshold must be at least 60 seconds.')
        );
      }

      this._threshold   = threshold;
      this._userState   = 'active';
      this._screenState = 'unlocked';
      this._started     = true;

      // Poll at half the threshold (minimum 30 s per spec minimum threshold).
      var pollMs = Math.max(30000, Math.floor(threshold / 2));
      var self = this;

      this._timer = setInterval(function() {
        if (!self._started) {
          clearInterval(self._timer);
          self._timer = null;
          return;
        }

        // Query OS idle time; fall back to 0 on unsupported platforms.
        var idleMs = (typeof __lumen_idle_get_idle_ms === 'function')
          ? __lumen_idle_get_idle_ms()
          : 0;

        var newUserState = (idleMs >= self._threshold) ? 'idle' : 'active';
        if (newUserState !== self._userState) {
          self._userState = newUserState;
          self.dispatchEvent(new Event('change'));
        }
      }, pollMs);

      return Promise.resolve(undefined);
    }

    // WICG §4.5: stop observing idle state.
    stop() {
      this._started = false;
      if (this._timer != null) {
        clearInterval(this._timer);
        this._timer = null;
      }
      this._userState   = null;
      this._screenState = null;
    }
  }

  global.IdleDetector = IdleDetector;
})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    // V8 auto-drains microtasks between separate `eval` calls (S12b-B8 finding),
    // so a `.then()`/`.catch()` scheduled in one `eval` has already run by the
    // time the next `eval` reads the global it wrote to.
    fn promise_result(rt: &V8JsRuntime, setup_js: &str, global: &str) -> JsValue {
        rt.eval(setup_js).unwrap();
        rt.eval(global).unwrap()
    }

    fn with_idle_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        // Minimal DOM shim: window + EventTarget + Event + timer stubs.
        rt.eval(
            r#"
            var window = globalThis;
            if (typeof EventTarget === 'undefined') {
              class EventTarget {
                constructor() { this._listeners = {}; }
                addEventListener(type, fn) {
                  if (!this._listeners[type]) this._listeners[type] = [];
                  this._listeners[type].push(fn);
                }
                removeEventListener(type, fn) {
                  if (!this._listeners[type]) return;
                  this._listeners[type] =
                    this._listeners[type].filter(function(f){ return f !== fn; });
                }
                dispatchEvent(event) {
                  var list = this._listeners[event.type] || [];
                  list.forEach(function(fn){ fn(event); });
                  return true;
                }
              }
              globalThis.EventTarget = EventTarget;
            }
            if (typeof Event === 'undefined') {
              function Event(type) { this.type = type; }
              globalThis.Event = Event;
            }
            // Timer stubs — record registered intervals so tests can fire them manually.
            var __timers = [];
            var __timer_id = 0;
            function setInterval(fn, ms) {
              var id = ++__timer_id;
              __timers.push({ id: id, fn: fn, ms: ms, cleared: false });
              return id;
            }
            function clearInterval(id) {
              var t = __timers.find(function(t){ return t.id === id; });
              if (t) t.cleared = true;
            }
            globalThis.setInterval   = setInterval;
            globalThis.clearInterval = clearInterval;
            // Fire the first non-cleared timer once.
            globalThis.__fire_timer = function() {
              var t = __timers.find(function(t){ return !t.cleared; });
              if (t) t.fn();
            };
            "#,
        )
        .unwrap();
        install_idle_detection_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn idle_detector_class_exists() {
        with_idle_api(|rt| {
            let ok = rt
                .eval("typeof window.IdleDetector === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn request_permission_returns_granted() {
        with_idle_api(|rt| {
            let ok = promise_result(
                rt,
                "var __perm = null; \
                 IdleDetector.requestPermission().then(function(v) { __perm = v; });",
                "__perm === 'granted'",
            );
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn initial_state_is_null() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.userState === null && d.screenState === null
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn start_resolves_and_sets_active_state() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.start({ threshold: 60000 });
                    d.userState === 'active' && d.screenState === 'unlocked'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn start_rejects_threshold_below_60s() {
        with_idle_api(|rt| {
            let ok = promise_result(
                rt,
                r#"
                var __rejected = false;
                var d = new IdleDetector();
                d.start({ threshold: 1000 }).catch(function(e) {
                  __rejected = e instanceof RangeError;
                });
                "#,
                "__rejected",
            );
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn stop_resets_state_to_null() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.start({ threshold: 60000 });
                    d.stop();
                    d.userState === null && d.screenState === null
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn stop_clears_interval_timer() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.start({ threshold: 60000 });
                    var hadTimer = d._timer != null;
                    d.stop();
                    hadTimer && d._timer === null
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn idle_detector_supports_add_event_listener() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    typeof d.addEventListener === 'function' &&
                    typeof d.removeEventListener === 'function'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn default_threshold_is_accepted() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.start({});
                    d.userState === 'active'
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn native_idle_binding_exists_and_returns_number() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    "typeof __lumen_idle_get_idle_ms === 'function' && \
                     typeof __lumen_idle_get_idle_ms() === 'number'",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn native_idle_ms_is_non_negative() {
        with_idle_api(|rt| {
            let ok = rt.eval("__lumen_idle_get_idle_ms() >= 0").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn change_event_fires_when_mock_idle_exceeds_threshold() {
        with_idle_api(|rt| {
            rt.eval(
                r#"
                // Override native binding: simulate 2 minutes of idle.
                globalThis.__lumen_idle_get_idle_ms = function() { return 120000; };

                var d = new IdleDetector();
                var changeCount = 0;
                d.addEventListener('change', function() { changeCount++; });
                d.start({ threshold: 60000 });
                // Fire polling timer once — idle (120000ms) >= threshold (60000ms).
                __fire_timer();
                "#,
            )
            .unwrap();
            let ok = rt
                .eval("changeCount === 1 && d.userState === 'idle'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn change_event_fires_on_return_to_active() {
        with_idle_api(|rt| {
            rt.eval(
                r#"
                var idleMs = 120000;
                globalThis.__lumen_idle_get_idle_ms = function() { return idleMs; };

                var d = new IdleDetector();
                var changes = [];
                d.addEventListener('change', function() { changes.push(d.userState); });
                d.start({ threshold: 60000 });

                __fire_timer(); // idle
                idleMs = 0;
                __fire_timer(); // active again
                "#,
            )
            .unwrap();
            let ok = rt
                .eval(
                    "changes.length === 2 && \
                     changes[0] === 'idle' && \
                     changes[1] === 'active' && \
                     d.userState === 'active'",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn no_change_event_when_state_unchanged() {
        with_idle_api(|rt| {
            rt.eval(
                r#"
                globalThis.__lumen_idle_get_idle_ms = function() { return 0; };

                var d = new IdleDetector();
                var changeCount = 0;
                d.addEventListener('change', function() { changeCount++; });
                d.start({ threshold: 60000 });

                __fire_timer(); // still active
                __fire_timer(); // still active
                "#,
            )
            .unwrap();
            let count = rt.eval("changeCount").unwrap();
            assert_eq!(count, JsValue::Number(0.0), "no change event when state does not transition");
        });
    }

    #[test]
    fn poll_interval_is_half_threshold() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.start({ threshold: 120000 }); // 2-minute threshold
                    // Expected poll: max(30000, 120000/2) = 60000 ms.
                    var timer = __timers.find(function(t){ return t.id === d._timer; });
                    timer && timer.ms === 60000
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn poll_interval_minimum_is_30s() {
        with_idle_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.start({ threshold: 60000 }); // minimum threshold
                    // Expected poll: max(30000, 60000/2) = 30000 ms.
                    var timer = __timers.find(function(t){ return t.id === d._timer; });
                    timer && timer.ms === 30000
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    /// Verify that the Rust `user_idle_ms()` returns a sane value on the host OS.
    #[test]
    fn rust_user_idle_ms_is_non_negative() {
        let ms = user_idle_ms();
        assert!(ms < 86_400_000 * 365, "idle ms unreasonably large: {ms}");
    }
}

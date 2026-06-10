/// Idle Detection API stub (WICG Idle Detection).
///
/// Phase 0: `IdleDetector.requestPermission()` → `'granted'`, `start()` resolves
/// immediately with fixed state `{userState:'active', screenState:'unlocked'}`.
/// The `'change'` event never fires — no OS idle polling in Phase 0.
///
/// Phase 1: wire `_lumen_idle_query_user_state` / `_lumen_idle_query_screen_state`
/// native hooks to OS idle-time APIs (Win32 `GetLastInputInfo`, Wayland
/// `ext-idle-notify-v1`, macOS `CGEventSourceSecondsSinceLastEventType`).
use rquickjs::Ctx;

/// Install Idle Detection API bindings into the JS context.
///
/// Must run after the DOM shim so that `window`, `EventTarget`, and `Promise` are available.
pub fn install_idle_detection_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(IDLE_DETECTION_SHIM)?;
    Ok(())
}

const IDLE_DETECTION_SHIM: &str = r#"
(function(global) {
  'use strict';

  // IdleDetector — detects user/screen idle state (WICG Idle Detection API).
  // Extends EventTarget so callers can use addEventListener('change', ...).
  class IdleDetector extends EventTarget {
    constructor() {
      super();
      // null until start() resolves
      this._userState = null;
      this._screenState = null;
      this._started = false;
    }

    // WICG §4.2: current user-activity state.
    // Returns 'active' | 'idle' | null (null before start()).
    get userState() {
      return this._userState;
    }

    // WICG §4.2: current screen-lock state.
    // Returns 'locked' | 'unlocked' | null (null before start()).
    get screenState() {
      return this._screenState;
    }

    // WICG §4.3: request permission to observe idle state.
    // Phase 0: auto-grant without prompting the user.
    static requestPermission() {
      return Promise.resolve('granted');
    }

    // WICG §4.4: begin observing idle state.
    // threshold — minimum idle duration in ms before userState flips to 'idle'.
    // Phase 0: resolves immediately; state is always {active, unlocked}.
    start(options) {
      var threshold = (options && typeof options.threshold === 'number')
        ? options.threshold : 60000;
      if (threshold < 60000) {
        return Promise.reject(
          new RangeError('Idle detection threshold must be at least 60 seconds.')
        );
      }
      this._userState = 'active';
      this._screenState = 'unlocked';
      this._started = true;
      return Promise.resolve(undefined);
    }

    // WICG §4.5: stop observing idle state.
    stop() {
      this._started = false;
      this._userState = null;
      this._screenState = null;
    }
  }

  global.IdleDetector = IdleDetector;
})(typeof globalThis !== 'undefined' ? globalThis : this);
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

    fn with_idle_api(f: impl FnOnce(&rquickjs::Ctx)) {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            // Minimal DOM shim: window alias + EventTarget stub.
            ctx.eval::<(), _>(
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
                      this._listeners[type] = this._listeners[type].filter(function(f){ return f !== fn; });
                    }
                    dispatchEvent(event) {
                      var list = this._listeners[event.type] || [];
                      list.forEach(function(fn){ fn(event); });
                      return true;
                    }
                  }
                  globalThis.EventTarget = EventTarget;
                }
                "#,
            )
            .unwrap();
            install_idle_detection_bindings(&ctx).unwrap();
            f(&ctx);
        });
    }

    #[test]
    fn idle_detector_class_exists() {
        with_idle_api(|ctx| {
            let ok: bool = ctx
                .eval("typeof window.IdleDetector === 'function'")
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn request_permission_returns_granted() {
        with_idle_api(|ctx| {
            ctx.eval::<(), _>(
                "var __perm = null; IdleDetector.requestPermission().then(function(v) { __perm = v; });",
            )
            .unwrap();
            loop {
                if !ctx.execute_pending_job() {
                    break;
                }
            }
            let ok: bool = ctx.eval("__perm === 'granted'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn initial_state_is_null() {
        with_idle_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.userState === null && d.screenState === null
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn start_resolves_and_sets_state() {
        with_idle_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new IdleDetector();
                    var resolved = false;
                    d.start({ threshold: 60000 }).then(function() { resolved = true; });
                    d.userState === 'active' && d.screenState === 'unlocked'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn start_rejects_threshold_below_60s() {
        with_idle_api(|ctx| {
            ctx.eval::<(), _>(
                r#"
                var __rejected = false;
                var d = new IdleDetector();
                d.start({ threshold: 1000 }).catch(function(e) { __rejected = e instanceof RangeError; });
                "#,
            )
            .unwrap();
            loop {
                if !ctx.execute_pending_job() {
                    break;
                }
            }
            let ok: bool = ctx.eval("__rejected").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn stop_resets_state_to_null() {
        with_idle_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new IdleDetector();
                    d.start({ threshold: 60000 });
                    d.stop();
                    d.userState === null && d.screenState === null
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn idle_detector_supports_add_event_listener() {
        with_idle_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new IdleDetector();
                    typeof d.addEventListener === 'function' &&
                    typeof d.removeEventListener === 'function'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn default_threshold_is_accepted() {
        with_idle_api(|ctx| {
            let ok: bool = ctx
                .eval(
                    r#"
                    var d = new IdleDetector();
                    var ok = false;
                    d.start({}).then(function() { ok = true; });
                    d.userState === 'active'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}

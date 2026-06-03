//! CSS View Transitions API L1 — `document.startViewTransition(callback)`.
//!
//! JS side: defines `ViewTransition` class + `document.startViewTransition`.
//! Native side: `_lumen_vt_begin` / `_lumen_vt_end` push events to a queue
//! drained by the shell in `about_to_wait` to drive the cross-fade animation.

use rquickjs::{Ctx, Function};
use std::sync::{Arc, Mutex};

type QjResult<T> = rquickjs::Result<T>;

/// Events emitted by `document.startViewTransition` and drained by the shell.
///
/// `Begin` is pushed before the user callback runs (shell captures old display list).
/// `End` is pushed after the callback (shell relayouts and starts cross-fade).
#[derive(Debug)]
pub enum ViewTransitionEvent {
    /// Callback is about to run — shell should snapshot the current frame.
    Begin,
    /// Callback finished — shell should relayout and start the cross-fade animation.
    End,
}

/// JavaScript shim for `document.startViewTransition(callback)`.
///
/// Phase 0 behaviour:
/// - Calls `_lumen_vt_begin()` (triggers snapshot in shell)
/// - Runs the callback synchronously
/// - Calls `_lumen_vt_end()` (triggers relayout + 300 ms cross-fade in shell)
/// - Returns `ViewTransition { updateCallbackDone, ready, finished, skipTransition }`
///   with all three promises pre-resolved (Phase 0 — no real async callback support)
const VIEW_TRANSITION_SHIM: &str = r#"
(function() {
  'use strict';
  if (typeof document === 'undefined') { return; }
  document.startViewTransition = function startViewTransition(callback) {
    // Notify shell: capture old frame snapshot.
    if (typeof _lumen_vt_begin === 'function') { _lumen_vt_begin(); }

    var cbError = null;
    try {
      if (typeof callback === 'function') { callback(); }
    } catch (e) {
      cbError = e;
    }

    // Notify shell: callback done, start animation.
    if (typeof _lumen_vt_end === 'function') { _lumen_vt_end(); }

    // Phase 0: return a ViewTransition with pre-resolved promises.
    var done = cbError ? Promise.reject(cbError) : Promise.resolve();
    var vt = {
      updateCallbackDone: done,
      ready: cbError ? Promise.reject(cbError) : Promise.resolve(),
      finished: done,
      skipTransition: function skipTransition() {}
    };
    return vt;
  };
})();
"#;

/// Register `_lumen_vt_begin` / `_lumen_vt_end` native functions and install
/// the `document.startViewTransition` JavaScript shim.
///
/// Call after `install_dom_api` so `document` is already defined.
/// `events` is drained by the shell in `about_to_wait` to drive the cross-fade.
pub fn install_view_transition_bindings(
    ctx: &Ctx<'_>,
    events: Arc<Mutex<Vec<ViewTransitionEvent>>>,
) -> QjResult<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals().set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    {
        let ev = Arc::clone(&events);
        reg!("_lumen_vt_begin", move || {
            ev.lock().unwrap().push(ViewTransitionEvent::Begin);
        });
    }
    {
        let ev = Arc::clone(&events);
        reg!("_lumen_vt_end", move || {
            ev.lock().unwrap().push(ViewTransitionEvent::End);
        });
    }

    ctx.eval::<(), _>(VIEW_TRANSITION_SHIM)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install(ctx: &rquickjs::Ctx, events: Arc<Mutex<Vec<ViewTransitionEvent>>>) {
        ctx.eval::<(), _>("var document = {};").unwrap();
        install_view_transition_bindings(ctx, events).unwrap();
    }

    #[test]
    fn install_succeeds() {
        let (_rt, ctx) = make_ctx();
        let ev = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install(&ctx, Arc::clone(&ev));
        });
    }

    #[test]
    fn start_view_transition_is_function() {
        let (_rt, ctx) = make_ctx();
        let ev = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install(&ctx, Arc::clone(&ev));
            let ty: String = ctx.eval("typeof document.startViewTransition").unwrap();
            assert_eq!(ty, "function");
        });
    }

    #[test]
    fn callback_is_called() {
        let (_rt, ctx) = make_ctx();
        let ev = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install(&ctx, Arc::clone(&ev));
            // The callback should run synchronously and set a flag.
            let called: bool = ctx
                .eval(
                    "(function() { \
                       var flag = false; \
                       document.startViewTransition(function() { flag = true; }); \
                       return flag; \
                     })()",
                )
                .unwrap();
            assert!(called, "callback must be called synchronously");
        });
    }

    #[test]
    fn returns_view_transition_object() {
        let (_rt, ctx) = make_ctx();
        let ev = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install(&ctx, Arc::clone(&ev));
            let has_props: bool = ctx
                .eval(
                    "(function() { \
                       var vt = document.startViewTransition(function() {}); \
                       return typeof vt.updateCallbackDone === 'object' \
                           && typeof vt.ready === 'object' \
                           && typeof vt.finished === 'object' \
                           && typeof vt.skipTransition === 'function'; \
                     })()",
                )
                .unwrap();
            assert!(has_props, "ViewTransition must expose updateCallbackDone/ready/finished/skipTransition");
        });
    }

    #[test]
    fn begin_and_end_events_queued() {
        let (_rt, ctx) = make_ctx();
        let ev: Arc<Mutex<Vec<ViewTransitionEvent>>> = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install(&ctx, Arc::clone(&ev));
            ctx.eval::<(), _>("document.startViewTransition(function() {});").unwrap();
        });
        let events = std::mem::take(&mut *ev.lock().unwrap());
        assert_eq!(events.len(), 2, "expect Begin + End events");
        assert!(matches!(events[0], ViewTransitionEvent::Begin));
        assert!(matches!(events[1], ViewTransitionEvent::End));
    }

    #[test]
    fn works_without_callback() {
        let (_rt, ctx) = make_ctx();
        let ev = Arc::new(Mutex::new(Vec::new()));
        ctx.with(|ctx| {
            install(&ctx, Arc::clone(&ev));
            // startViewTransition with no callback should not throw.
            let ok: bool = ctx
                .eval(
                    "(function() { \
                       try { document.startViewTransition(); return true; } \
                       catch(e) { return false; } \
                     })()",
                )
                .unwrap();
            assert!(ok, "startViewTransition() without callback must not throw");
        });
    }
}

//! CSS View Transitions API L1/L2 — `document.startViewTransition(...)`, the
//! `ViewTransition` / `ViewTransitionTypeSet` interfaces, `document.activeViewTransition`.
//!
//! The JS half is `shim/view_transitions_shim.js` (state machine, promises, types).
//! Native side: `_lumen_vt_begin` / `_lumen_vt_end` / `_lumen_vt_cancel` push events
//! drained by the shell in `about_to_wait` to drive the cross-fade animation.

#[cfg(feature = "v8-backend")]
use std::sync::{Arc, Mutex};

/// Events emitted by `document.startViewTransition` and drained by the shell.
///
/// `Begin` is pushed right before the update callback runs (shell captures old display list).
/// `End` is pushed once the callback's promise settled (shell relayouts and starts cross-fade).
/// `Cancel` is pushed when the transition is skipped or the callback fails after `Begin`.
#[derive(Debug)]
pub enum ViewTransitionEvent {
    /// Callback is about to run — shell should snapshot the current frame.
    Begin,
    /// Callback finished — shell should relayout and start the cross-fade animation.
    End,
    /// Transition was cancelled (another transition started or callback threw).
    /// Phase 1: handles nested/interrupted transitions cleanly.
    Cancel,
}

/// JavaScript half of the View Transitions API (`shim/view_transitions_shim.js`):
/// the `ViewTransition` / `ViewTransitionTypeSet` / `PageRevealEvent` /
/// `PageSwapEvent` interfaces and `Document.prototype.startViewTransition` /
/// `activeViewTransition`. It drives the §6.3 state machine and reports to the shell
/// through `_lumen_vt_begin` (snapshot the frame on screen, pushed right before the
/// update callback runs), `_lumen_vt_end` (new state ready — start the cross-fade) and
/// `_lumen_vt_cancel` (skipped or callback threw). `_lumen_vt_named_elements` (installed
/// with the computed-style natives) feeds its duplicate-`view-transition-name` check.
#[cfg(feature = "v8-backend")]
const VIEW_TRANSITION_SHIM: &str = include_str!("shim/view_transitions_shim.js");

/// V8 port of the former rquickjs `install_view_transition_bindings` (Ph3 V8
/// migration S12b-G5, rquickjs side removed in the same batch): natives
/// registered via [`crate::v8_runtime::V8JsRuntime::register_native`] instead
/// of `rquickjs::Function::new`, otherwise identical (same 3 events, same
/// shim). `events` is the `V8JsRuntime`'s own `view_transition_events` field
/// (passed in by `install_dom`, mirrors the `pointer_capture_nid` extra-arg
/// call), drained by the shell in `about_to_wait` via
/// `take_view_transition_events()` to drive the cross-fade.
#[cfg(feature = "v8-backend")]
#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
pub(crate) fn install_view_transition_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    events: Arc<Mutex<Vec<ViewTransitionEvent>>>,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::into_v8_fn0;
    use lumen_core::ext::JsRuntime as _;

    {
        let ev = Arc::clone(&events);
        rt.register_native(
            "_lumen_vt_begin",
            into_v8_fn0(move || {
                ev.lock().unwrap().push(ViewTransitionEvent::Begin);
            }),
        )?;
    }
    {
        let ev = Arc::clone(&events);
        rt.register_native(
            "_lumen_vt_end",
            into_v8_fn0(move || {
                ev.lock().unwrap().push(ViewTransitionEvent::End);
            }),
        )?;
    }
    {
        let ev = Arc::clone(&events);
        rt.register_native(
            "_lumen_vt_cancel",
            into_v8_fn0(move || {
                ev.lock().unwrap().push(ViewTransitionEvent::Cancel);
            }),
        )?;
    }

    rt.eval(VIEW_TRANSITION_SHIM)?;
    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    // The behaviour of the state machine (timers, events, promises) is covered with a
    // full DOM in `dom/tests/v8_vt_api.rs`; this module only checks that the shim loads
    // into a bare runtime.
    fn make_rt() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var document = {};").unwrap();
        let events: Arc<Mutex<Vec<ViewTransitionEvent>>> = Arc::new(Mutex::new(Vec::new()));
        install_view_transition_bindings_v8(&rt, events).unwrap();
        rt
    }

    fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
        rt.eval(script).unwrap() == JsValue::Bool(true)
    }

    #[test]
    fn install_succeeds() {
        let _rt = make_rt();
    }

    #[test]
    fn start_view_transition_returns_a_view_transition() {
        let rt = make_rt();
        assert!(bool_eval(&rt, "typeof document.startViewTransition === 'function'"));
        assert!(bool_eval(&rt, "document.startViewTransition() instanceof ViewTransition"));
    }

    #[test]
    fn view_transition_is_not_constructible() {
        let rt = make_rt();
        assert!(bool_eval(
            &rt,
            "(function() { try { new ViewTransition(); return false; }                catch (e) { return e instanceof TypeError; } })()",
        ));
    }
}

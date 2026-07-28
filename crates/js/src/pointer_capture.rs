//! W3C Pointer Events Level 3 §4.1 — pointer capture bindings.
//!
//! Registers three native functions called by the JS shim when
//! `Element.setPointerCapture(id)` / `Element.releasePointerCapture(id)` /
//! `Element.hasPointerCapture(id)` are called:
//!
//! * `_lumen_set_capture_state(nid)` — records the capture target so the shell
//!   routes all subsequent pointer events to that element instead of hit-testing.
//! * `_lumen_release_capture_state()` — clears the capture, restoring normal routing.
//! * `_lumen_get_capture_nid()` — returns the current capture target, if any.
//!
//! The state is shared via `Arc<Mutex<Option<u32>>>` with `V8JsRuntime`, which
//! exposes `pointer_capture_nid()` and `take_pointer_capture()` for the shell.

#[cfg(feature = "v8-backend")]
use std::sync::{Arc, Mutex};

/// V8 port of the former rquickjs `install_pointer_capture_bindings` (Ph3 V8
/// migration S12b-20, rquickjs side removed in the same slice): same three
/// natives, registered via [`crate::v8_runtime::V8JsRuntime::register_native`]
/// instead of `rquickjs::Function::new`. `capture_nid` is the runtime's own
/// `pointer_capture_nid` field (cloned in by the caller), so the shell's
/// `pointer_capture_nid()`/`take_pointer_capture()` observe the same state
/// the natives mutate.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_pointer_capture_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    capture_nid: Arc<Mutex<Option<u32>>>,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1};

    {
        let cap = Arc::clone(&capture_nid);
        rt.register_native(
            "_lumen_set_capture_state",
            into_v8_fn1(move |nid: u32| {
                *cap.lock().unwrap() = Some(nid);
            }),
        )?;
    }
    {
        let cap = Arc::clone(&capture_nid);
        rt.register_native(
            "_lumen_release_capture_state",
            into_v8_fn0(move || {
                *cap.lock().unwrap() = None;
            }),
        )?;
    }
    {
        let cap = Arc::clone(&capture_nid);
        rt.register_native(
            "_lumen_get_capture_nid",
            into_v8_fn0(move || -> Option<u32> { *cap.lock().unwrap() }),
        )?;
    }
    Ok(())
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_capture(nid: Arc<Mutex<Option<u32>>>, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        install_pointer_capture_bindings_v8(&rt, nid).unwrap();
        f(&rt);
    }

    #[test]
    fn set_capture_state_updates_arc() {
        let nid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        with_capture(Arc::clone(&nid), |rt| {
            rt.eval("_lumen_set_capture_state(42)").unwrap();
        });
        assert_eq!(*nid.lock().unwrap(), Some(42));
    }

    #[test]
    fn release_capture_state_clears_arc() {
        let nid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(Some(7)));
        with_capture(Arc::clone(&nid), |rt| {
            rt.eval("_lumen_release_capture_state()").unwrap();
        });
        assert_eq!(*nid.lock().unwrap(), None);
    }

    #[test]
    fn set_then_release_roundtrip() {
        let nid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        with_capture(Arc::clone(&nid), |rt| {
            rt.eval("_lumen_set_capture_state(99); _lumen_release_capture_state();")
                .unwrap();
        });
        assert_eq!(*nid.lock().unwrap(), None);
    }

    #[test]
    fn get_capture_nid_reflects_state() {
        let nid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        with_capture(Arc::clone(&nid), |rt| {
            let before = rt.eval("_lumen_get_capture_nid()").unwrap();
            assert_eq!(before, JsValue::Null);
            rt.eval("_lumen_set_capture_state(5)").unwrap();
            let after = rt.eval("_lumen_get_capture_nid()").unwrap();
            assert_eq!(after, JsValue::Number(5.0));
        });
    }
}

//! W3C Pointer Events Level 3 §4.1 — pointer capture bindings.
//!
//! Registers two native functions called by the JS shim when
//! `Element.setPointerCapture(id)` / `Element.releasePointerCapture(id)` are called:
//!
//! * `_lumen_set_capture_state(nid)` — records the capture target so the shell
//!   routes all subsequent pointer events to that element instead of hit-testing.
//! * `_lumen_release_capture_state()` — clears the capture, restoring normal routing.
//!
//! The state is shared via `Arc<Mutex<Option<u32>>>` with `QuickJsRuntime`, which
//! exposes `pointer_capture_nid()` and `take_pointer_capture()` for the shell.

use rquickjs::{Ctx, Function};
use std::sync::{Arc, Mutex};

type QjResult<T> = rquickjs::Result<T>;

/// Install `_lumen_set_capture_state` and `_lumen_release_capture_state` into the
/// QuickJS global object.
///
/// Both functions update `capture_nid` atomically under the mutex.
/// Called once from `install_dom` after the DOM shim is evaluated.
pub fn install_pointer_capture_bindings(
    ctx: &Ctx<'_>,
    capture_nid: Arc<Mutex<Option<u32>>>,
) -> QjResult<()> {
    {
        let cap = Arc::clone(&capture_nid);
        ctx.globals().set(
            "_lumen_set_capture_state",
            Function::new(ctx.clone(), move |nid: u32| {
                *cap.lock().unwrap() = Some(nid);
            })?,
        )?;
    }
    {
        let cap = Arc::clone(&capture_nid);
        ctx.globals().set(
            "_lumen_release_capture_state",
            Function::new(ctx.clone(), move || {
                *cap.lock().unwrap() = None;
            })?,
        )?;
    }
    {
        let cap = Arc::clone(&capture_nid);
        ctx.globals().set(
            "_lumen_get_capture_nid",
            Function::new(ctx.clone(), move || -> Option<u32> {
                *cap.lock().unwrap()
            })?,
        )?;
    }
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

    #[test]
    fn set_capture_state_updates_arc() {
        let (rt, ctx) = make_ctx();
        let nid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        ctx.with(|ctx| {
            install_pointer_capture_bindings(&ctx, Arc::clone(&nid)).unwrap();
            ctx.eval::<(), _>("_lumen_set_capture_state(42)").unwrap();
        });
        drop(rt);
        assert_eq!(*nid.lock().unwrap(), Some(42));
    }

    #[test]
    fn release_capture_state_clears_arc() {
        let (rt, ctx) = make_ctx();
        let nid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(Some(7)));
        ctx.with(|ctx| {
            install_pointer_capture_bindings(&ctx, Arc::clone(&nid)).unwrap();
            ctx.eval::<(), _>("_lumen_release_capture_state()").unwrap();
        });
        drop(rt);
        assert_eq!(*nid.lock().unwrap(), None);
    }

    #[test]
    fn set_then_release_roundtrip() {
        let (rt, ctx) = make_ctx();
        let nid: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        ctx.with(|ctx| {
            install_pointer_capture_bindings(&ctx, Arc::clone(&nid)).unwrap();
            ctx.eval::<(), _>(
                "_lumen_set_capture_state(99); _lumen_release_capture_state();",
            )
            .unwrap();
        });
        drop(rt);
        assert_eq!(*nid.lock().unwrap(), None);
    }
}

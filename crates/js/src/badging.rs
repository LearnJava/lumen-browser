//! Badging API (W3C Badging API).
//!
//! Phase 0: no-op stubs.
//! - `navigator.setAppBadge(count?)` → Promise\<undefined\>
//! - `navigator.clearAppBadge()` → Promise\<undefined\>
//!
//! Native binding `_lumen_set_app_badge(count)` is a no-op hook for shell Phase 1
//! (OS badge integration via Win32 taskbar / Linux Unity counter / macOS dock).

/// V8 port of the former rquickjs `install_badging_bindings` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-1): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_badging_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(BADGING_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const BADGING_SHIM: &str = r#"
(function() {
  // W3C Badging API §3 — native hook for shell Phase 1 integration.
  // Phase 0: no-op; shell installs a real handler in Phase 1.
  if (typeof _lumen_set_app_badge === 'undefined') {
    globalThis._lumen_set_app_badge = function(_count) {};
  }

  // W3C Badging API §4.1: navigator.setAppBadge(contents?)
  // contents is either undefined (flag badge) or a non-negative integer.
  navigator.setAppBadge = function(contents) {
    var count = (typeof contents === 'undefined') ? null : (contents >>> 0);
    _lumen_set_app_badge(count);
    return Promise.resolve(undefined);
  };

  // W3C Badging API §4.2: navigator.clearAppBadge()
  navigator.clearAppBadge = function() {
    _lumen_set_app_badge(null);
    return Promise.resolve(undefined);
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_badging(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var window = globalThis; var navigator = {};")
            .unwrap();
        install_badging_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn set_app_badge_exists() {
        with_badging(|rt| {
            let ok = rt
                .eval("typeof navigator.setAppBadge === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn clear_app_badge_exists() {
        with_badging(|rt| {
            let ok = rt
                .eval("typeof navigator.clearAppBadge === 'function'")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn set_app_badge_returns_promise() {
        with_badging(|rt| {
            let ok = rt
                .eval("navigator.setAppBadge(3) instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn clear_app_badge_returns_promise() {
        with_badging(|rt| {
            let ok = rt
                .eval("navigator.clearAppBadge() instanceof Promise")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}

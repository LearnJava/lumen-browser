//! Battery Status API disable stub (ADR-007 Layer 4, 9D.4).
//!
//! The Battery Status API (`navigator.getBattery()`) is a high-entropy
//! fingerprinting source: battery level, charging state, and charge/discharge
//! time together form a near-unique device signature. This module disables the
//! API by replacing `navigator.getBattery` with a function that returns a
//! rejected `Promise`, matching Chrome's behavior when the API is removed via
//! Permissions Policy.
//!
//! Must be called **after** `dom::install_dom_api` (requires `navigator` to exist).

/// V8 port of the former rquickjs `install_battery_bindings` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-4): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_battery_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(BATTERY_SHIM)?;
    Ok(())
}

/// JavaScript shim: override `navigator.getBattery` to return a rejected Promise.
#[cfg(feature = "v8-backend")]
const BATTERY_SHIM: &str = r#"(function() {
  if (typeof navigator === 'undefined') return;
  navigator.getBattery = function() {
    return Promise.reject(new Error('Battery Status API is not supported'));
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_battery(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var navigator = {};").unwrap();
        install_battery_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn install_succeeds_without_navigator() {
        // Should not panic even if navigator is undefined.
        let rt = V8JsRuntime::new().unwrap();
        install_battery_bindings_v8(&rt).expect("install should succeed");
    }

    #[test]
    fn install_succeeds_with_navigator() {
        with_battery(|_rt| {});
    }

    #[test]
    fn get_battery_is_function() {
        with_battery(|rt| {
            let ok = rt.eval("typeof navigator.getBattery === 'function'").unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_battery_returns_thenable() {
        with_battery(|rt| {
            let ok = rt
                .eval(
                    "(function() { \
                       var p = navigator.getBattery(); \
                       return typeof p === 'object' && typeof p.then === 'function'; \
                     })()",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_battery_has_catch_method() {
        with_battery(|rt| {
            let ok = rt
                .eval(
                    "(function() { \
                       var p = navigator.getBattery(); \
                       return typeof p.catch === 'function'; \
                     })()",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}

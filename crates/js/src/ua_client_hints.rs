//! User-Agent Client Hints (W3C UA-CH §4–6)
//! Phase 0: static Chrome 114 profile — all values are fixed.
//! `navigator.userAgentData` exposes low-entropy values directly.
//! `getHighEntropyValues(hints)` returns Promise<UADataValues> with static fields.

/// V8 port of the former rquickjs `install_ua_client_hints_bindings` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-B3): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Adds `navigator.userAgentData` (a `NavigatorUAData` instance) and exports
/// `NavigatorUAData` on `globalThis`. Phase 0: static Chrome 114 / Windows 10 profile.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_ua_client_hints_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(UA_CLIENT_HINTS_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const UA_CLIENT_HINTS_SHIM: &str = r#"
(function() {
  // NavigatorUABrandVersion — one entry in the brands / fullVersionList arrays.
  function NavigatorUABrandVersion(brand, version) {
    this.brand = brand;
    this.version = version;
  }
  NavigatorUABrandVersion.prototype.toJSON = function() {
    return { brand: this.brand, version: this.version };
  };

  // Low-entropy brand list (reported without permission).
  var _brands = [
    new NavigatorUABrandVersion("Not A;Brand", "99"),
    new NavigatorUABrandVersion("Chromium", "114"),
    new NavigatorUABrandVersion("Google Chrome", "114")
  ];

  // High-entropy full-version list.
  var _fullVersionList = [
    new NavigatorUABrandVersion("Not A;Brand", "99.0.0.0"),
    new NavigatorUABrandVersion("Chromium", "114.0.5735.133"),
    new NavigatorUABrandVersion("Google Chrome", "114.0.5735.133")
  ];

  // Static high-entropy values (Phase 0: fixed Chrome 114 / Windows 10 x64).
  var _highEntropy = {
    platform:        "Windows",
    platformVersion: "10.0.0",
    architecture:    "x86",
    bitness:         "64",
    model:           "",
    uaFullVersion:   "114.0.5735.133",
    wow64:           false
  };

  // NavigatorUAData — the object exposed as navigator.userAgentData.
  function NavigatorUAData() {}

  // Low-entropy accessors.
  Object.defineProperty(NavigatorUAData.prototype, 'brands', {
    get: function() { return _brands.slice(); },
    enumerable: true, configurable: true
  });
  Object.defineProperty(NavigatorUAData.prototype, 'mobile', {
    get: function() { return false; },
    enumerable: true, configurable: true
  });
  Object.defineProperty(NavigatorUAData.prototype, 'platform', {
    get: function() { return "Windows"; },
    enumerable: true, configurable: true
  });

  // High-entropy accessor — returns Promise<UADataValues>.
  // Resolves immediately with the requested subset of static values.
  NavigatorUAData.prototype.getHighEntropyValues = function(hints) {
    if (!Array.isArray(hints)) {
      return Promise.reject(new TypeError('hints must be an array'));
    }
    var result = {};
    // Always include low-entropy fields in the resolved object.
    result.brands   = _brands.slice();
    result.mobile   = false;
    result.platform = "Windows";
    // Add each requested high-entropy hint.
    for (var i = 0; i < hints.length; i++) {
      var h = hints[i];
      if (h === 'fullVersionList') {
        result.fullVersionList = _fullVersionList.slice();
      } else if (h in _highEntropy) {
        result[h] = _highEntropy[h];
      }
    }
    return Promise.resolve(result);
  };

  // W3C §6.1 serialisation.
  NavigatorUAData.prototype.toJSON = function() {
    return {
      brands:   _brands.map(function(b) { return b.toJSON(); }),
      mobile:   false,
      platform: "Windows"
    };
  };

  // Expose class on globalThis (window alias when available).
  globalThis.NavigatorUAData = NavigatorUAData;

  // Install navigator.userAgentData.
  if (typeof navigator !== 'undefined') {
    try {
      Object.defineProperty(navigator, 'userAgentData', {
        value: new NavigatorUAData(),
        writable: false,
        configurable: true,
        enumerable: true
      });
    } catch(_) {}
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_ua_hints_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var window = globalThis; var navigator = {}; globalThis.navigator = navigator;")
            .unwrap();
        install_ua_client_hints_bindings_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn navigator_ua_data_exists() {
        with_ua_hints_api(|rt| {
            let ok = rt
                .eval("typeof navigator.userAgentData === 'object' && navigator.userAgentData !== null")
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn low_entropy_brands_mobile_platform() {
        with_ua_hints_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var d = navigator.userAgentData;
                    d.brands.length === 3 &&
                    d.mobile === false &&
                    d.platform === "Windows"
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_high_entropy_values_returns_promise() {
        with_ua_hints_api(|rt| {
            let ok = rt
                .eval(
                    "navigator.userAgentData.getHighEntropyValues(['platformVersion']) instanceof Promise",
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }

    #[test]
    fn get_high_entropy_values_resolves_with_platform_version() {
        with_ua_hints_api(|rt| {
            // V8 auto-drains microtasks between separate `eval` calls, so the
            // `.then()` callback below has already run by the time we read `_result`.
            rt.eval(
                r#"
                var _result = null;
                navigator.userAgentData
                  .getHighEntropyValues(['platformVersion', 'architecture', 'bitness'])
                  .then(function(v) { _result = v; });
                "#,
            )
            .unwrap();
            let ok = rt
                .eval(
                    r#"
                    _result !== null &&
                    _result.platformVersion === "10.0.0" &&
                    _result.architecture === "x86" &&
                    _result.bitness === "64"
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}

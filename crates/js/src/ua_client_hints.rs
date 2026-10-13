//! User-Agent Client Hints (W3C UA-CH §4–6)
//! `navigator.userAgentData` exposes low-entropy values directly;
//! `getHighEntropyValues(hints)` returns Promise<UADataValues>.
//!
//! The brands, versions and platform come from the process-global
//! [`lumen_core::Identity`] (BUG-1614) — the same source the HTTP `Sec-CH-UA*`
//! headers and `navigator.userAgent` use, so the layers cannot disagree.
//! Non-Chromium identities (Firefox, Safari, Tor, Lumen) have no
//! `userAgentData` at all, like the real browsers.

#[cfg(feature = "v8-backend")]
use lumen_core::UaData;

#[cfg(feature = "v8-backend")]
use crate::navigator_bindings::json_string as js_str;

#[cfg(feature = "v8-backend")]
fn brand_list(list: &[(String, String)]) -> String {
    let items: Vec<String> = list
        .iter()
        .map(|(b, v)| format!("new NavigatorUABrandVersion({}, {})", js_str(b), js_str(v)))
        .collect();
    format!("[{}]", items.join(", "))
}

/// JS expression (an IIFE) that defines `globalThis.NavigatorUAData` and
/// evaluates to the single `navigator.userAgentData` instance for `d`.
/// Shared by the page install and the worker `NavigatorID` shim.
#[cfg(feature = "v8-backend")]
pub(crate) fn ua_data_instance_expr(d: &UaData) -> String {
    format!(
        r#"(function() {{
  function NavigatorUABrandVersion(brand, version) {{
    this.brand = brand;
    this.version = version;
  }}
  NavigatorUABrandVersion.prototype.toJSON = function() {{
    return {{ brand: this.brand, version: this.version }};
  }};
  var _brands = {brands};
  var _fullVersionList = {full_version_list};
  var _platform = {platform};
  var _highEntropy = {{
    platform:        {platform},
    platformVersion: {platform_version},
    architecture:    "x86",
    bitness:         "64",
    model:           "",
    uaFullVersion:   {full_version},
    wow64:           false
  }};
  function NavigatorUAData() {{ throw new TypeError('Illegal constructor'); }}
  Object.defineProperty(NavigatorUAData.prototype, Symbol.toStringTag, {{
    value: 'NavigatorUAData', configurable: true
  }});
  Object.defineProperty(NavigatorUAData.prototype, 'brands', {{
    get: function() {{ return _brands.slice(); }},
    enumerable: true, configurable: true
  }});
  Object.defineProperty(NavigatorUAData.prototype, 'mobile', {{
    get: function() {{ return false; }},
    enumerable: true, configurable: true
  }});
  Object.defineProperty(NavigatorUAData.prototype, 'platform', {{
    get: function() {{ return _platform; }},
    enumerable: true, configurable: true
  }});
  NavigatorUAData.prototype.getHighEntropyValues = function(hints) {{
    if (!Array.isArray(hints)) {{
      return Promise.reject(new TypeError('hints must be an array'));
    }}
    var result = {{}};
    result.brands   = _brands.slice();
    result.mobile   = false;
    result.platform = _platform;
    for (var i = 0; i < hints.length; i++) {{
      var h = hints[i];
      if (h === 'fullVersionList') {{
        result.fullVersionList = _fullVersionList.slice();
      }} else if (h in _highEntropy) {{
        result[h] = _highEntropy[h];
      }}
    }}
    return Promise.resolve(result);
  }};
  NavigatorUAData.prototype.toJSON = function() {{
    return {{
      brands:   _brands.map(function(b) {{ return b.toJSON(); }}),
      mobile:   false,
      platform: _platform
    }};
  }};
  globalThis.NavigatorUAData = NavigatorUAData;
  return Object.create(NavigatorUAData.prototype);
}})()"#,
        brands = brand_list(&d.brands),
        full_version_list = brand_list(&d.full_version_list),
        platform = js_str(&d.platform),
        platform_version = js_str(&d.platform_version),
        full_version = js_str(&d.full_version),
    )
}

/// Script that (re)defines `navigator.userAgentData` for `ua_data`, or removes
/// it (and `NavigatorUAData`) when the identity has none. Also used by the
/// `emulation.setUserAgentOverride` path so an overridden UA never keeps a
/// stale Chromium `userAgentData`.
#[cfg(feature = "v8-backend")]
pub(crate) fn ua_data_install_script(ua_data: Option<&UaData>) -> String {
    match ua_data {
        Some(d) => format!(
            "(function() {{ var inst = {}; \
               if (typeof navigator === 'undefined') return; \
               try {{ Object.defineProperty(navigator, 'userAgentData', {{ \
                 value: inst, writable: false, configurable: true, enumerable: true }}); }} catch(_) {{}} \
             }})();",
            ua_data_instance_expr(d)
        ),
        None => "(function() { \
               if (typeof navigator !== 'undefined') { try { delete navigator.userAgentData; } catch(_) {} } \
               try { delete globalThis.NavigatorUAData; } catch(_) {} \
             })();"
            .to_string(),
    }
}

/// Install `navigator.userAgentData` from the process-global identity
/// (V8 port of the former rquickjs `install_ua_client_hints_bindings`).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_ua_client_hints_bindings_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    install_ua_client_hints_bindings_v8_with(rt, &crate::navigator_bindings::current_identity())
}

/// [`install_ua_client_hints_bindings_v8`] against an explicit identity.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_ua_client_hints_bindings_v8_with(
    rt: &crate::v8_runtime::V8JsRuntime,
    id: &lumen_core::Identity,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    if id.ua_data.is_some() {
        rt.eval(&ua_data_install_script(id.ua_data.as_ref()))?;
    }
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

    fn with_ua_hints_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var window = globalThis; var navigator = {}; globalThis.navigator = navigator;")
            .unwrap();
        let id = lumen_core::Identity::from_user_agent(lumen_core::CHROME_USER_AGENT);
        install_ua_client_hints_bindings_v8_with(&rt, &id).unwrap();
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
                    d.brands.length === 2 &&
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

    #[test]
    fn navigator_ua_data_is_not_constructible_and_has_tag() {
        with_ua_hints_api(|rt| {
            let ok = rt
                .eval(
                    r#"
                    var threw = false;
                    try { new NavigatorUAData(); } catch (e) { threw = e instanceof TypeError; }
                    threw &&
                    Object.prototype.toString.call(navigator.userAgentData) === "[object NavigatorUAData]" &&
                    navigator.userAgentData instanceof NavigatorUAData
                    "#,
                )
                .unwrap();
            assert_eq!(ok, JsValue::Bool(true));
        });
    }
}

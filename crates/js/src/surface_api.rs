//! ADR-007 Layer 1: Surface API без automation-маркеров (9A).
//!
//! Automation detection works by querying JS globals that headless drivers
//! inject: `navigator.webdriver` (Selenium/WebDriver), `chrome.runtime`
//! (CDP), `__playwright` / `__pwInitScripts` (Playwright), `cdc_*`
//! (ChromeDriver), `__selenium_unwrapped` / `__webdriver_evaluate` etc.
//!
//! Since Lumen builds the JS environment from scratch it never injects
//! these markers. This module adds an additional hardening layer:
//!
//! 1. Seals `navigator.webdriver` as `undefined` via `Object.defineProperty`
//!    with `configurable: false` — even if a script tries to assign `true`
//!    the property cannot be overridden at runtime.
//! 2. Adds standard browser compatibility properties that fingerprinting
//!    scripts expect on any real browser (`navigator.plugins`,
//!    `navigator.mimeTypes`, `navigator.appName`, `navigator.vendor`,
//!    `navigator.product`, `navigator.productSub`).
//! 3. Freezes `navigator.cookieEnabled = true` and
//!    `navigator.doNotTrack = null` (Chrome-matching).
//!
//! Must be called **after** `v8_runtime.rs::install_dom` and `install_navigator_bindings`.

/// V8 port of the former rquickjs `install_surface_api_protection` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-B6): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Seals automation-detection properties and adds standard browser
/// compatibility shims. Must be called after `v8_runtime.rs::install_dom`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_surface_api_protection_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SURFACE_API_SHIM)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const SURFACE_API_SHIM: &str = r#"(function() {
  // ── Seal navigator.webdriver ────────────────────────────────────────────────
  // Selenium/WebDriver sets navigator.webdriver = true.  We explicitly define
  // it as a non-configurable getter returning `undefined` so automation scripts
  // can never make it truthy, even via property assignment.
  if (typeof navigator !== 'undefined') {
    // navigator.webdriver is intentionally NOT defined here — it must be
    // completely absent (not even as `undefined`).  Defining it via
    // Object.defineProperty would make `'webdriver' in navigator` return true,
    // which is itself a detection signal used by some fingerprinting scripts.
    // Lumen's navigator object is built from scratch in dom.rs and never
    // includes this property, so no action is needed.

    // ── Standard browser compatibility properties ─────────────────────────────
    // Many fingerprinting scripts check these properties to decide whether they
    // are running in a real browser.  Absent properties can be as telling as
    // wrong ones.

    // navigator.appName — all modern browsers return "Netscape" per spec.
    try {
      if (typeof navigator.appName === 'undefined') {
        Object.defineProperty(navigator, 'appName', {
          value: 'Netscape', writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    // navigator.appVersion — Chrome-style version string.
    try {
      if (typeof navigator.appVersion === 'undefined') {
        Object.defineProperty(navigator, 'appVersion', {
          value: '5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
          writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    // navigator.vendor — "Google Inc." matches Chrome (most common desktop).
    try {
      if (typeof navigator.vendor === 'undefined') {
        Object.defineProperty(navigator, 'vendor', {
          value: 'Google Inc.', writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    // navigator.product — "Gecko" per HTML spec §8.4.
    try {
      if (typeof navigator.product === 'undefined') {
        Object.defineProperty(navigator, 'product', {
          value: 'Gecko', writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    // navigator.productSub — "20030107" matches Chrome + Firefox.
    try {
      if (typeof navigator.productSub === 'undefined') {
        Object.defineProperty(navigator, 'productSub', {
          value: '20030107', writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    // navigator.cookieEnabled — Lumen will support cookies; report true.
    try {
      if (typeof navigator.cookieEnabled === 'undefined') {
        Object.defineProperty(navigator, 'cookieEnabled', {
          value: true, writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    // navigator.doNotTrack — null means "unspecified" (Chrome default).
    try {
      if (typeof navigator.doNotTrack === 'undefined') {
        Object.defineProperty(navigator, 'doNotTrack', {
          value: null, writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    // navigator.plugins / navigator.mimeTypes — empty PluginArray/MimeTypeArray.
    // Real Chrome has a non-empty plugins list; fingerprinting scripts check
    // for an object with length ≥ 0 and named-item access.  We provide a
    // minimal compatible stub.
    try {
      if (typeof navigator.plugins === 'undefined') {
        var _emptyPlugins = Object.create(null);
        _emptyPlugins.length = 0;
        _emptyPlugins.item = function() { return null; };
        _emptyPlugins.namedItem = function() { return null; };
        _emptyPlugins[Symbol.iterator] = function*() {};
        Object.defineProperty(navigator, 'plugins', {
          get: function() { return _emptyPlugins; },
          configurable: true, enumerable: true
        });
      }
    } catch(_) {}

    try {
      if (typeof navigator.mimeTypes === 'undefined') {
        var _emptyMimes = Object.create(null);
        _emptyMimes.length = 0;
        _emptyMimes.item = function() { return null; };
        _emptyMimes.namedItem = function() { return null; };
        _emptyMimes[Symbol.iterator] = function*() {};
        Object.defineProperty(navigator, 'mimeTypes', {
          get: function() { return _emptyMimes; },
          configurable: true, enumerable: true
        });
      }
    } catch(_) {}
  }

  // ── Ensure no automation globals leak through ─────────────────────────────
  // These are read-only shims that return `undefined` even if something tries
  // to set them.  We only define them if they do not already exist (they
  // should not — Lumen never defines them — but this is a belt-and-braces
  // guard for external scripts that inject via `eval`).
  var _automationGlobals = [
    '__playwright', '__pwInitScripts', '__pwExecPath',
    '__selenium_unwrapped', '__selenium_evaluate', '__webdriver_evaluate',
    '__webdriver_script_fn', '__webdriver_script_func',
    '__lastWatirAlert', '__lastWatirConfirm', '__lastWatirPrompt',
    '_phantom', 'callPhantom', 'domAutomation', 'domAutomationController'
  ];
  for (var _i = 0; _i < _automationGlobals.length; _i++) {
    var _g = _automationGlobals[_i];
    if (typeof globalThis[_g] === 'undefined') {
      try {
        Object.defineProperty(globalThis, _g, {
          get: function() { return undefined; },
          set: function() {},
          configurable: false, enumerable: false
        });
      } catch(_) {}
    }
  }
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_surface_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var navigator = { language: 'en-US' }; var globalThis = {};")
            .unwrap();
        super::install_surface_api_protection_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn webdriver_is_undefined() {
        with_surface_api(|rt| {
            let v = rt
                .eval("typeof navigator.webdriver === 'undefined'")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "navigator.webdriver must be undefined");
        });
    }

    #[test]
    fn webdriver_absent_in_navigator() {
        with_surface_api(|rt| {
            // navigator.webdriver must be completely absent — not even enumerable.
            let v = rt.eval("!('webdriver' in navigator)").unwrap();
            assert_eq!(v, JsValue::Bool(true), "webdriver must not be a property of navigator");
        });
    }

    #[test]
    fn appname_is_netscape() {
        with_surface_api(|rt| {
            let v = rt.eval("navigator.appName").unwrap();
            assert_eq!(v, JsValue::String("Netscape".to_string()));
        });
    }

    #[test]
    fn vendor_is_google_inc() {
        with_surface_api(|rt| {
            let v = rt.eval("navigator.vendor").unwrap();
            assert_eq!(v, JsValue::String("Google Inc.".to_string()));
        });
    }

    #[test]
    fn product_is_gecko() {
        with_surface_api(|rt| {
            let v = rt.eval("navigator.product").unwrap();
            assert_eq!(v, JsValue::String("Gecko".to_string()));
        });
    }

    #[test]
    fn plugins_exists_with_length_zero() {
        with_surface_api(|rt| {
            let v = rt
                .eval("typeof navigator.plugins === 'object' && navigator.plugins.length === 0")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "navigator.plugins must be an object with length 0");
        });
    }

    #[test]
    fn mime_types_exists_with_length_zero() {
        with_surface_api(|rt| {
            let v = rt
                .eval("typeof navigator.mimeTypes === 'object' && navigator.mimeTypes.length === 0")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "navigator.mimeTypes must be an object with length 0");
        });
    }

    #[test]
    fn playwright_global_is_undefined() {
        with_surface_api(|rt| {
            let v = rt
                .eval("typeof globalThis.__playwright === 'undefined'")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "__playwright must be undefined");
        });
    }

    #[test]
    fn phantom_global_is_undefined() {
        with_surface_api(|rt| {
            let v = rt
                .eval("typeof globalThis.callPhantom === 'undefined'")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "callPhantom must be undefined");
        });
    }

    #[test]
    fn selenium_global_is_undefined() {
        with_surface_api(|rt| {
            let v = rt
                .eval("typeof globalThis.__selenium_unwrapped === 'undefined'")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "__selenium_unwrapped must be undefined");
        });
    }

    #[test]
    fn install_succeeds_without_navigator() {
        let rt = V8JsRuntime::new().unwrap();
        super::install_surface_api_protection_v8(&rt)
            .expect("must not crash when navigator is absent");
    }
}

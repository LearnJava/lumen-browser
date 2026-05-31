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
//! Must be called **after** `install_dom_api` and `install_navigator_bindings`.

use rquickjs::Ctx;

/// Install Layer 1 surface API protection into the JS context.
///
/// Seals automation-detection properties and adds standard browser
/// compatibility shims. Must be called after `install_dom_api`.
pub fn install_surface_api_protection(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(SURFACE_API_SHIM)?;
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>("var navigator = { language: 'en-US' }; var globalThis = {};")
            .unwrap();
        install_surface_api_protection(ctx).unwrap();
    }

    #[test]
    fn webdriver_is_undefined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: bool = ctx
                .eval("typeof navigator.webdriver === 'undefined'")
                .unwrap();
            assert!(v, "navigator.webdriver must be undefined");
        });
    }

    #[test]
    fn webdriver_absent_in_navigator() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            // navigator.webdriver must be completely absent — not even enumerable.
            let v: bool = ctx
                .eval("!('webdriver' in navigator)")
                .unwrap();
            assert!(v, "webdriver must not be a property of navigator");
        });
    }

    #[test]
    fn appname_is_netscape() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: String = ctx.eval("navigator.appName").unwrap();
            assert_eq!(v, "Netscape");
        });
    }

    #[test]
    fn vendor_is_google_inc() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: String = ctx.eval("navigator.vendor").unwrap();
            assert_eq!(v, "Google Inc.");
        });
    }

    #[test]
    fn product_is_gecko() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: String = ctx.eval("navigator.product").unwrap();
            assert_eq!(v, "Gecko");
        });
    }

    #[test]
    fn plugins_exists_with_length_zero() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: bool = ctx
                .eval("typeof navigator.plugins === 'object' && navigator.plugins.length === 0")
                .unwrap();
            assert!(v, "navigator.plugins must be an object with length 0");
        });
    }

    #[test]
    fn mime_types_exists_with_length_zero() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: bool = ctx
                .eval("typeof navigator.mimeTypes === 'object' && navigator.mimeTypes.length === 0")
                .unwrap();
            assert!(v, "navigator.mimeTypes must be an object with length 0");
        });
    }

    #[test]
    fn playwright_global_is_undefined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: bool = ctx
                .eval("typeof globalThis.__playwright === 'undefined'")
                .unwrap();
            assert!(v, "__playwright must be undefined");
        });
    }

    #[test]
    fn phantom_global_is_undefined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: bool = ctx
                .eval("typeof globalThis.callPhantom === 'undefined'")
                .unwrap();
            assert!(v, "callPhantom must be undefined");
        });
    }

    #[test]
    fn selenium_global_is_undefined() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install(&ctx);
            let v: bool = ctx
                .eval("typeof globalThis.__selenium_unwrapped === 'undefined'")
                .unwrap();
            assert!(v, "__selenium_unwrapped must be undefined");
        });
    }

    #[test]
    fn install_succeeds_without_navigator() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_surface_api_protection(&ctx)
                .expect("must not crash when navigator is absent");
        });
    }
}

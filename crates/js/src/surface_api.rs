//! ADR-007 Layer 1: Surface API без automation-маркеров (9A).
//!
//! Automation detection works by querying JS globals that headless drivers
//! inject: `navigator.webdriver` (Selenium/WebDriver), `chrome.runtime`
//! (CDP), `__playwright` / `__pwInitScripts` (Playwright), `cdc_*`
//! (ChromeDriver), `__selenium_unwrapped` / `__webdriver_evaluate` etc.
//!
//! Since Lumen builds the JS environment from scratch it never injects
//! these markers, so the module defines **none** of them: an absent property
//! and a property that reads as `undefined` are different observable states,
//! and detectors query the difference via `getOwnPropertyNames` / `in` /
//! `hasOwnProperty` (BUG-379). What this module does add:
//!
//! 1. Standard browser compatibility properties that fingerprinting
//!    scripts expect on any real browser (`navigator.plugins`,
//!    `navigator.mimeTypes`, `navigator.appName`, `navigator.vendor`,
//!    `navigator.product`, `navigator.productSub`).
//! 2. Freezes `navigator.cookieEnabled = true` and
//!    `navigator.doNotTrack = null` (Chrome-matching).
//! 3. Defines `navigator.globalPrivacyControl = true` when — and only when —
//!    the network layer sends `Sec-GPC: 1` (BUG-397, see
//!    [`set_global_privacy_control`]).
//!
//! Must be called **after** `v8_runtime.rs::install_dom` and `install_navigator_bindings`.

/// V8 port of the former rquickjs `install_surface_api_protection` (Ph3 V8 migration
/// S5-S7, rquickjs side removed in S12b-B6): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Adds standard browser compatibility shims to `navigator` and defines no
/// automation-detection property at all (BUG-379). Must be called after
/// `v8_runtime.rs::install_dom`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_surface_api_protection_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    install_surface_api_protection_v8_with(rt, global_privacy_control_enabled())
}

/// Install the surface-API shim with an explicit Global Privacy Control state,
/// ignoring the process-global flag. Used by tests that want full control
/// without racing other tests over the global slot (same split as
/// `navigator_bindings::install_navigator_bindings_v8_with`).
#[cfg(feature = "v8-backend")]
pub(crate) fn install_surface_api_protection_v8_with(
    rt: &crate::v8_runtime::V8JsRuntime,
    global_privacy_control: bool,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(SURFACE_API_SHIM)?;
    if global_privacy_control {
        rt.eval(GPC_SHIM)?;
    }
    Ok(())
}

/// Process-global Global Privacy Control state (BUG-397).
///
/// `false` (the default) means the page must not see the property at all —
/// see [`set_global_privacy_control`] for why absence, not `false`.
static GPC_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enable or disable the Global Privacy Control signal on the JS side
/// (`navigator.globalPrivacyControl`, <https://www.w3.org/TR/gpc/>).
///
/// The shell calls this once at startup with
/// `lumen_network::sends_global_privacy_control(http_profile)` — the network
/// predicate is the single source of truth, so the JS property and the
/// `Sec-GPC: 1` header can never disagree (a page seeing one without the other
/// would have a fingerprinting signal, BUG-397).
///
/// When disabled the property is **absent**, not `false`: the profiles that do
/// not send the header impersonate Chrome/Edge/Safari, which have no native GPC
/// at all, and `'globalPrivacyControl' in navigator` is exactly the check a
/// fingerprinting script runs (the same absent-vs-`undefined` distinction as
/// BUG-379). A supporting-but-off state has no user today: the signal is
/// chosen by picking the `strict` / `lumen` HTTP profile, not by a per-site
/// toggle.
///
/// Must be called before any page loads; later calls affect only JS contexts
/// created afterwards.
pub fn set_global_privacy_control(enabled: bool) {
    GPC_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// The currently configured Global Privacy Control state (default `false`).
#[must_use]
pub fn global_privacy_control_enabled() -> bool {
    GPC_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// `navigator.globalPrivacyControl` — installed only when the signal is on.
///
/// W3C GPC defines a read-only boolean on `Navigator`; Lumen has no separate
/// `WorkerNavigator` interface, so the same `navigator` object serves both.
#[cfg(feature = "v8-backend")]
const GPC_SHIM: &str = r#"(function() {
  if (typeof navigator === 'undefined') { return; }
  // Global Privacy Control (https://www.w3.org/TR/gpc/): `true` means the
  // browser is sending `Sec-GPC: 1` on every request.  Read-only per spec —
  // a page must not be able to flip the user's opt-out.
  try {
    Object.defineProperty(navigator, 'globalPrivacyControl', {
      value: true, writable: false, configurable: true, enumerable: true
    });
  } catch(_) {}
})();
"#;

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

    // navigator.appCodeName — every browser returns "Mozilla" per spec. Added
    // with BUG-776, which found it missing here while making a worker's
    // `WorkerNavigator` answer the same values as the page's `Navigator`: the
    // mixin is `NavigatorID`, so an absent member on one side is a mismatch.
    try {
      if (typeof navigator.appCodeName === 'undefined') {
        Object.defineProperty(navigator, 'appCodeName', {
          value: 'Mozilla', writable: false, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

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

  // ── Automation globals: deliberately NOT defined ──────────────────────────
  // `__playwright`, `__pwInitScripts`, `__pwExecPath`, `__selenium_unwrapped`,
  // `__selenium_evaluate`, `__webdriver_evaluate`, `__webdriver_script_fn`,
  // `__webdriver_script_func`, `__lastWatirAlert`, `__lastWatirConfirm`,
  // `__lastWatirPrompt`, `_phantom`, `callPhantom`, `domAutomation`,
  // `domAutomationController` — Lumen never injects any of them, so there is
  // nothing to seal here and no code runs for them (BUG-379).
  //
  // This block used to define all fifteen as non-configurable `undefined`
  // getters "belt-and-braces, in case an external script injects one via
  // eval". That inverted its own purpose: `undefined` on read is not the same
  // state as absent, and detectors query the difference —
  // `Object.getOwnPropertyNames(window)`, `'__webdriver_evaluate' in window`
  // and `hasOwnProperty` all answered *true* on Lumen and *false* on
  // Chrome/Firefox, so the anti-fingerprint layer was itself a 15-marker
  // fingerprint (and `configurable: false` made it unremovable afterwards).
  // The guard could not work in the first place: a marker injected by a
  // foreign script shows up in `getOwnPropertyNames` whether we reserved the
  // name or not. Intercepting *writes* would need a `globalThis` proxy, which
  // is a different mechanism — see the BUG-379 report before adding one.
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// The fifteen names the shim used to reserve (BUG-379) — none of them may
    /// exist as a property of the global object after installation.
    const AUTOMATION_MARKERS: &[&str] = &[
        "__playwright",
        "__pwInitScripts",
        "__pwExecPath",
        "__selenium_unwrapped",
        "__selenium_evaluate",
        "__webdriver_evaluate",
        "__webdriver_script_fn",
        "__webdriver_script_func",
        "__lastWatirAlert",
        "__lastWatirConfirm",
        "__lastWatirPrompt",
        "_phantom",
        "callPhantom",
        "domAutomation",
        "domAutomationController",
    ];

    fn with_surface_api(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        // Only `navigator` is faked here. `globalThis` must stay the engine's
        // real global object: the earlier harness shadowed it with a plain
        // `{}`, so every marker assertion below was measuring a throwaway
        // object instead of the surface a page actually sees (BUG-379).
        rt.eval("var navigator = { language: 'en-US' };").unwrap();
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

    /// BUG-379: `typeof x === 'undefined'` cannot tell "absent" from "defined
    /// as an `undefined`-returning getter", which is exactly the distinction
    /// automation detectors query — so assert the observable state instead:
    /// no own property, no `in`, no `hasOwnProperty`, nothing in
    /// `getOwnPropertyNames`.
    #[test]
    fn automation_markers_are_not_properties_of_the_global() {
        with_surface_api(|rt| {
            for name in AUTOMATION_MARKERS {
                let v = rt
                    .eval(&format!(
                        "!('{name}' in globalThis) \
                         && !Object.prototype.hasOwnProperty.call(globalThis, '{name}') \
                         && Object.getOwnPropertyNames(globalThis).indexOf('{name}') === -1"
                    ))
                    .unwrap();
                assert_eq!(
                    v,
                    JsValue::Bool(true),
                    "`{name}` must be absent from the global object, not defined as undefined"
                );
            }
        });
    }

    /// Guards the assertion form above from decaying back into the check that
    /// hid BUG-379: define one marker exactly the way the removed loop did and
    /// confirm `typeof === 'undefined'` still answers "absent" while
    /// `in` / `getOwnPropertyNames` correctly report it as present.
    #[test]
    fn undefined_returning_getter_is_not_the_same_as_absent() {
        with_surface_api(|rt| {
            rt.eval(
                "Object.defineProperty(globalThis, '__playwright', { \
                   get: function() { return undefined; }, set: function() {}, \
                   configurable: false, enumerable: false });",
            )
            .unwrap();
            assert_eq!(
                rt.eval("typeof globalThis.__playwright === 'undefined'").unwrap(),
                JsValue::Bool(true),
                "the weak check cannot see the marker — that is the point"
            );
            assert_eq!(
                rt.eval(
                    "'__playwright' in globalThis \
                     && Object.prototype.hasOwnProperty.call(globalThis, '__playwright') \
                     && Object.getOwnPropertyNames(globalThis).indexOf('__playwright') !== -1"
                )
                .unwrap(),
                JsValue::Bool(true),
                "the observable-state check must see a marker defined this way"
            );
        });
    }

    /// The one-line detector from the BUG-379 report: `false` in Chrome and
    /// Firefox, and it must be `false` here too.
    #[test]
    fn one_line_detector_finds_no_marker() {
        with_surface_api(|rt| {
            let v = rt
                .eval(
                    "Object.getOwnPropertyNames(globalThis).some(function(n) { \
                       return /^(__webdriver|__selenium|__playwright|__pw|__lastWatir|_phantom|callPhantom|domAutomation)/.test(n); \
                     })",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(false), "no automation-marker name may be an own property of the global");
        });
    }

    /// Install with an explicit GPC state, bypassing the process-global flag so
    /// the two tests below cannot race each other.
    fn with_gpc(enabled: bool, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var navigator = { language: 'en-US' };").unwrap();
        super::install_surface_api_protection_v8_with(&rt, enabled).unwrap();
        f(&rt);
    }

    /// BUG-397: with the signal on the page sees a read-only `true`.
    #[test]
    fn global_privacy_control_is_true_when_enabled() {
        with_gpc(true, |rt| {
            assert_eq!(rt.eval("navigator.globalPrivacyControl").unwrap(), JsValue::Bool(true));
            // Read-only per spec — a page must not be able to clear the opt-out.
            rt.eval("try { navigator.globalPrivacyControl = false; } catch(_) {}").unwrap();
            assert_eq!(
                rt.eval("navigator.globalPrivacyControl").unwrap(),
                JsValue::Bool(true),
                "globalPrivacyControl must not be writable"
            );
        });
    }

    /// With the signal off the property must be *absent*, not `false`: the
    /// profiles that do not send `Sec-GPC` impersonate browsers without native
    /// GPC, and `in` / `getOwnPropertyNames` see the difference (BUG-379 class).
    #[test]
    fn global_privacy_control_is_absent_when_disabled() {
        with_gpc(false, |rt| {
            let v = rt
                .eval(
                    "!('globalPrivacyControl' in navigator) \
                     && Object.getOwnPropertyNames(navigator).indexOf('globalPrivacyControl') === -1",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "globalPrivacyControl must be absent when GPC is off");
        });
    }

    #[test]
    fn install_succeeds_without_navigator() {
        let rt = V8JsRuntime::new().unwrap();
        super::install_surface_api_protection_v8(&rt)
            .expect("must not crash when navigator is absent");
        // The GPC shim runs in the same navigator-less context (worker-style
        // globals) and must be just as tolerant.
        super::install_surface_api_protection_v8_with(&rt, true)
            .expect("GPC shim must not crash when navigator is absent");
    }
}

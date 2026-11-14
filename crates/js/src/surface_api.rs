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
//!    `navigator.product`, `navigator.productSub`) and the `window.chrome`
//!    shape Chrome exposes on an ordinary page (`app`/`csi`/`loadTimes`,
//!    no `runtime` — BUG-1790).
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
  // ── navigator.webdriver ─────────────────────────────────────────────────────
  // Chrome and Firefox expose `navigator.webdriver === false` outside of
  // automation, so an *absent* property is the distinguishing signal
  // (BUG-754).  Define it as an accessor returning `false`, `configurable`
  // and `enumerable` like Chrome — a non-configurable one would be
  // distinguishable in the same way (BUG-379).
  if (typeof navigator !== 'undefined') {
    try {
      if (!('webdriver' in navigator)) {
        const proto = Object.getPrototypeOf(navigator);
        const target = (proto && proto !== Object.prototype) ? proto : navigator;
        Object.defineProperty(target, 'webdriver', {
          get: function webdriver() { return false; },
          set: undefined, configurable: true, enumerable: true
        });
      }
    } catch(_) {}

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

    // navigator.appVersion / vendor / productSub come from lumen_core::Identity
    // (navigator_bindings.rs), the same source as the HTTP User-Agent (BUG-1614).

    // navigator.product — "Gecko" per HTML spec §8.4.
    try {
      if (typeof navigator.product === 'undefined') {
        Object.defineProperty(navigator, 'product', {
          value: 'Gecko', writable: false, configurable: true, enumerable: true
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

    // navigator.plugins / navigator.mimeTypes — Chrome's PDF-viewer set.
    // Chrome exposes five legacy PDF plugins and two MIME types; an empty
    // PluginArray is a one-line fingerprint that Cloudflare's challenge reads
    // (BUG-1790).  The interfaces sit on Navigator.prototype like Chrome's,
    // not as own properties of `navigator` (own props are detectable).
    try {
      if (typeof navigator.plugins === 'undefined') {
        install_legacy_plugin_array();
      }
    } catch(_) {}
  }

  // ── window.chrome — the shape real Chrome has on an ordinary page ─────────
  // Chrome exposes exactly `loadTimes`, `csi` and `app` here; `runtime` is a
  // CDP/extension marker and stays absent (privacy.md §9.5).  ADR-034 п.1 takes
  // the shape from the identity profile, and its absence is itself a
  // fingerprint the Turnstile challenge reads (BUG-1790).
  try {
    if (typeof globalThis !== 'undefined' && typeof globalThis.chrome === 'undefined') {
      install_window_chrome();
    }
  } catch(_) {}

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

  // ── Helpers ───────────────────────────────────────────────────────────────
  // Legacy NPAPI interfaces.  Lumen has no plugins at all, but a page still
  // sees the PDF-viewer set Chrome reports (BUG-1790); only the shape is
  // simulated, nothing here can load or run a plugin.
  function install_legacy_plugin_array() {
    function legacyClass(clsName) {
      var C = function() { throw new TypeError('Illegal constructor'); };
      Object.defineProperty(C, 'name', { value: clsName, configurable: true });
      Object.defineProperty(C.prototype, 'constructor', { value: C, writable: true, configurable: true });
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: clsName, configurable: true });
      return C;
    }
    function getter(proto, name, fn) {
      Object.defineProperty(proto, name, { get: fn, configurable: true, enumerable: true });
    }
    function arrayClass(clsName, extra) {
      var C = legacyClass(clsName);
      getter(C.prototype, 'length', function() { return this._len; });
      C.prototype.item = function item(i) { return this[i] || null; };
      C.prototype.namedItem = function namedItem(n) { return this[n] || null; };
      if (extra) { C.prototype[extra] = function() {}; }
      return C;
    }
    function fillCycle(target, items, key) {
      for (var i = 0; i < items.length; i++) {
        target[i] = items[i];
        target[key(items[i])] = items[i];
      }
    }

    var MimeType = legacyClass('MimeType');
    getter(MimeType.prototype, 'type', function() { return this._type; });
    getter(MimeType.prototype, 'suffixes', function() { return this._suffixes; });
    getter(MimeType.prototype, 'description', function() { return this._description; });
    getter(MimeType.prototype, 'enabledPlugin', function() { return this._enabledPlugin; });

    var Plugin = legacyClass('Plugin');
    getter(Plugin.prototype, 'name', function() { return this._name; });
    getter(Plugin.prototype, 'filename', function() { return this._filename; });
    getter(Plugin.prototype, 'description', function() { return this._description; });
    getter(Plugin.prototype, 'length', function() { return this._len; });
    Plugin.prototype.item = function item(i) { return this[i] || null; };
    Plugin.prototype.namedItem = function namedItem(n) { return this[n] || null; };

    var MimeTypeArray = arrayClass('MimeTypeArray', null);
    var PluginArray = arrayClass('PluginArray', 'refresh');

    var MIME = 'Portable Document Format';
    var mimes = ['application/pdf', 'text/pdf'].map(function(type) {
      var m = Object.create(MimeType.prototype);
      m._type = type; m._suffixes = 'pdf'; m._description = MIME;
      return m;
    });
    var pluginNames = ['PDF Viewer', 'Chrome PDF Viewer', 'Chromium PDF Viewer',
                       'Microsoft Edge PDF Viewer', 'WebKit built-in PDF'];
    var plugins = pluginNames.map(function(name) {
      var p = Object.create(Plugin.prototype);
      p._name = name; p._filename = 'internal-pdf-viewer'; p._description = MIME;
      Object.defineProperty(p, '_len', { value: mimes.length });
      fillCycle(p, mimes, function(m) { return m.type; });
      return p;
    });
    // Every MIME type is enabled by the first plugin, as in Chrome.
    for (var i = 0; i < mimes.length; i++) { mimes[i]._enabledPlugin = plugins[0]; }

    var mimeTypes = Object.create(MimeTypeArray.prototype);
    Object.defineProperty(mimeTypes, '_len', { value: mimes.length });
    fillCycle(mimeTypes, mimes, function(m) { return m.type; });
    var pluginArray = Object.create(PluginArray.prototype);
    Object.defineProperty(pluginArray, '_len', { value: plugins.length });
    fillCycle(pluginArray, plugins, function(p) { return p.name; });

    var interfaces = [Plugin, PluginArray, MimeType, MimeTypeArray];
    for (var t = 0; t < interfaces.length; t++) {
      if (typeof globalThis[interfaces[t].name] === 'undefined') {
        Object.defineProperty(globalThis, interfaces[t].name, {
          value: interfaces[t], writable: true, configurable: true, enumerable: false
        });
      }
    }

    var proto = Object.getPrototypeOf(navigator);
    var host = (proto && proto !== Object.prototype) ? proto : navigator;
    getter(host, 'plugins', function() { return pluginArray; });
    getter(host, 'mimeTypes', function() { return mimeTypes; });
  }

  // Chrome's `window.chrome` — `app`/`csi`/`loadTimes` only, no `runtime`.
  function install_window_chrome() {
    var origin = (typeof performance !== 'undefined' && performance.timeOrigin)
      ? performance.timeOrigin : Date.now();
    function now() {
      return (typeof performance !== 'undefined' && performance.now) ? performance.now() : 0;
    }
    var onloadT = 0;
    var chrome = {
      loadTimes: function loadTimes() {
        var t = origin / 1000;
        var done = t + now() / 1000;
        return {
          requestTime: t, startLoadTime: t,
          commitLoadTime: t + 0.001, finishDocumentLoadTime: done, finishLoadTime: done,
          firstPaintTime: done, firstPaintAfterLoadTime: 0,
          navigationType: 'Other', wasFetchedViaSpdy: true, wasNpnNegotiated: true,
          npnNegotiatedProtocol: 'h2', wasAlternateProtocolAvailable: false,
          connectionInfo: 'h2'
        };
      },
      csi: function csi() {
        if (!onloadT) { onloadT = Math.round(origin + now()); }
        return { startE: Math.round(origin), onloadT: onloadT, pageT: now(), tran: 15 };
      },
      app: {
        isInstalled: false,
        getDetails: function getDetails() { return null; },
        getIsInstalled: function getIsInstalled() { return false; },
        installState: function installState() { return 'not_installed'; },
        runningState: function runningState() { return 'cannot_run'; },
        InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' },
        RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' }
      }
    };
    Object.defineProperty(globalThis, 'chrome', {
      value: chrome, writable: true, enumerable: true, configurable: false
    });
  }
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
    fn webdriver_is_false() {
        with_surface_api(|rt| {
            let v = rt.eval("navigator.webdriver === false").unwrap();
            assert_eq!(v, JsValue::Bool(true), "navigator.webdriver must be false");
        });
    }

    #[test]
    fn webdriver_present_and_configurable() {
        with_surface_api(|rt| {
            // Like Chrome: present, `false`, configurable accessor (BUG-754).
            let v = rt
                .eval(
                    "'webdriver' in navigator && (function(){ var o = navigator;                      while (o && !Object.getOwnPropertyDescriptor(o, 'webdriver'))                        o = Object.getPrototypeOf(o);                      var d = Object.getOwnPropertyDescriptor(o, 'webdriver');                      return d.configurable && d.enumerable; })()",
                )
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "webdriver must be a configurable property");
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
    fn product_is_gecko() {
        with_surface_api(|rt| {
            let v = rt.eval("navigator.product").unwrap();
            assert_eq!(v, JsValue::String("Gecko".to_string()));
        });
    }

    #[test]
    fn plugins_has_chrome_pdf_set() {
        with_surface_api(|rt| {
            let v = rt
                .eval("Object.prototype.toString.call(navigator.plugins) === '[object PluginArray]' \
                       && navigator.plugins.length === 5 \
                       && navigator.plugins[0].name === 'PDF Viewer' \
                       && navigator.plugins[0].filename === 'internal-pdf-viewer' \
                       && navigator.plugins.namedItem('WebKit built-in PDF') !== null \
                       && navigator.plugins === navigator.plugins")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "navigator.plugins must read as Chrome's PDF set (BUG-1790)");
        });
    }

    #[test]
    fn mime_types_has_chrome_pdf_set() {
        with_surface_api(|rt| {
            let v = rt
                .eval("Object.prototype.toString.call(navigator.mimeTypes) === '[object MimeTypeArray]' \
                       && navigator.mimeTypes.length === 2 \
                       && navigator.mimeTypes[0].type === 'application/pdf' \
                       && navigator.mimeTypes[1].type === 'text/pdf' \
                       && navigator.mimeTypes[0].enabledPlugin === navigator.plugins[0]")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "navigator.mimeTypes must read as Chrome's PDF set (BUG-1790)");
        });
    }

    /// BUG-1790: `window.chrome` is part of Chrome's ordinary surface
    /// (`loadTimes`/`csi`/`app`); `chrome.runtime` stays absent as a CDP marker
    /// (privacy.md §9.5).
    #[test]
    fn window_chrome_has_chrome_shape_without_runtime() {
        with_surface_api(|rt| {
            let v = rt
                .eval("Object.getOwnPropertyNames(globalThis.chrome).join(',') === 'loadTimes,csi,app' \
                       && typeof globalThis.chrome.csi === 'function' \
                       && typeof globalThis.chrome.csi().pageT === 'number' \
                       && typeof globalThis.chrome.loadTimes().requestTime === 'number' \
                       && globalThis.chrome.app.isInstalled === false \
                       && globalThis.chrome.app.runningState() === 'cannot_run' \
                       && typeof globalThis.chrome.runtime === 'undefined'")
                .unwrap();
            assert_eq!(v, JsValue::Bool(true), "window.chrome must have Chrome's shape without runtime");
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

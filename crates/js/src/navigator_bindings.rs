//! Navigator / Screen / Timezone normalization (ADR-007 Layer 4, 9D.6).
//!
//! High-entropy properties exposed by `navigator` and `screen` form a large
//! portion of the browser fingerprint. This module normalises them to common
//! mid-tier device values, defeating passive fingerprinting without breaking
//! feature-detection logic that depends on the API's existence.
//!
//! Properties normalised:
//! - `navigator.hardwareConcurrency` → 2 (Brave-style; exact core count leaks CPU model)
//! - `navigator.deviceMemory`        → 8 (rounds to nearest power-of-two per spec)
//! - `navigator.platform`            → "Win32" (most common desktop value)
//! - `navigator.languages`           → ["en-US", "en"] (single common locale)
//! - `screen.width` / `screen.height`           → 1920 / 1080 (most common desktop resolution)
//! - `screen.availWidth` / `screen.availHeight` → same as width/height
//! - `screen.colorDepth` / `screen.pixelDepth`  → 24 (standard true-colour)
//! - `screen.orientation`                        → stub { type: "landscape-primary", angle: 0 }
//! - `Date.prototype.getTimezoneOffset`          → always returns 0 (UTC normalisation)
//!
//! Must be called **after** `v8_runtime.rs::install_dom` (requires `navigator` to exist).
//!
//! The exact values are taken from a process-global [`NavigatorProfile`] that
//! the shell may override from `fingerprint.toml` (9F.1) via
//! [`set_navigator_profile`]. When unset, the defaults reproduce the historical
//! hardcoded mid-tier device values, so behaviour is unchanged without a config.

use std::sync::{OnceLock, RwLock};

/// High-entropy `navigator` / `screen` / timezone values exposed to JavaScript.
///
/// Each field maps directly to a fingerprinting surface. Defaults reproduce the
/// historical hardcoded values (mid-tier desktop, UTC). The shell can build a
/// custom profile from `fingerprint.toml` and install it process-wide via
/// [`set_navigator_profile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigatorProfile {
    /// `navigator.hardwareConcurrency` — reported logical CPU count.
    pub hardware_concurrency: u32,
    /// `navigator.deviceMemory` — reported RAM in GiB (spec rounds to powers of two).
    pub device_memory: u32,
    /// `navigator.platform` — UA platform string (e.g. `"Win32"`).
    pub platform: String,
    /// `navigator.languages` — ordered locale list; `navigator.language` is the first entry.
    /// Must contain at least one entry; an empty list falls back to `["en-US"]`.
    pub languages: Vec<String>,
    /// `screen.width` / `screen.availWidth` in CSS pixels.
    pub screen_width: u32,
    /// `screen.height` / `screen.availHeight` in CSS pixels.
    pub screen_height: u32,
    /// `screen.colorDepth` / `screen.pixelDepth` in bits.
    pub color_depth: u32,
    /// Value returned by `Date.prototype.getTimezoneOffset()`, in minutes
    /// (positive = behind UTC, matching the JS convention). `0` = UTC.
    pub timezone_offset: i32,
}

impl Default for NavigatorProfile {
    fn default() -> Self {
        Self {
            hardware_concurrency: 2,
            device_memory: 8,
            platform: "Win32".to_string(),
            languages: vec!["en-US".to_string(), "en".to_string()],
            screen_width: 1920,
            screen_height: 1080,
            color_depth: 24,
            timezone_offset: 0,
        }
    }
}

/// Process-global override installed by the shell from `fingerprint.toml`.
///
/// `None` (the default) means use [`NavigatorProfile::default`]. Stored behind a
/// `RwLock` so the shell can set it once at startup before any JS context spins up.
static GLOBAL_PROFILE: OnceLock<RwLock<Option<NavigatorProfile>>> = OnceLock::new();

fn global_slot() -> &'static RwLock<Option<NavigatorProfile>> {
    GLOBAL_PROFILE.get_or_init(|| RwLock::new(None))
}

/// Install a process-wide navigator profile (9F.1). Subsequent calls to the
/// no-argument [`install_navigator_bindings`] use these values.
///
/// Intended to be called once by the shell at startup, before any page loads.
pub fn set_navigator_profile(profile: NavigatorProfile) {
    if let Ok(mut slot) = global_slot().write() {
        *slot = Some(profile);
    }
}

static GLOBAL_IDENTITY: OnceLock<RwLock<Option<lumen_core::Identity>>> = OnceLock::new();

fn identity_slot() -> &'static RwLock<Option<lumen_core::Identity>> {
    GLOBAL_IDENTITY.get_or_init(|| RwLock::new(None))
}

/// Install the process-wide browser identity (BUG-1614). The shell derives it
/// from the same User-Agent the HTTP profile sends, so `navigator.userAgent` /
/// `appVersion` / `vendor` / `productSub` / `userAgentData` (window, workers,
/// iframes) can never contradict the request headers. Call once at startup,
/// before any page loads.
pub fn set_identity(identity: lumen_core::Identity) {
    if let Ok(mut slot) = identity_slot().write() {
        *slot = Some(identity);
    }
}

/// The identity pages observe: a session `navigator.userAgent` override
/// (BiDi `emulation.setUserAgentOverride`) wins; otherwise the identity the
/// shell configured; otherwise the default Chrome one (the default HTTP profile).
pub fn current_identity() -> lumen_core::Identity {
    #[cfg(feature = "v8-backend")]
    if let Some(ua) = crate::v8_runtime::global_user_agent_override() {
        return lumen_core::Identity::from_user_agent(&ua);
    }
    identity_slot()
        .read()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_else(|| lumen_core::Identity::from_user_agent(lumen_core::chrome_user_agent()))
}

/// Return the currently configured profile, or the default if none was set.
pub fn current_navigator_profile() -> NavigatorProfile {
    global_slot()
        .read()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default()
}

/// Install navigator/screen/timezone normalization shim into the JS context,
/// using the process-global [`NavigatorProfile`] (set via [`set_navigator_profile`],
/// otherwise the default).
///
/// Overwrites high-entropy fingerprinting properties on `navigator` and
/// creates a normalised `screen` object on `globalThis`. Also patches
/// `Date.prototype.getTimezoneOffset` to return the profile's offset, so
/// timezone cannot be inferred from JS date arithmetic.
///
/// Must be called after `v8_runtime.rs::install_dom`. V8 port of the former rquickjs
/// `install_navigator_bindings` (Ph3 V8 migration S5-S7): identical JS shim,
/// evaluated via [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_navigator_bindings_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    install_navigator_bindings_v8_with(rt, &current_navigator_profile())
}

/// Install the navigator shim using an explicit [`NavigatorProfile`], ignoring
/// the process-global. Used by tests that want full control without racing
/// other tests over the global profile slot.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_navigator_bindings_v8_with(
    rt: &crate::v8_runtime::V8JsRuntime,
    profile: &NavigatorProfile,
) -> lumen_core::JsResult<()> {
    install_navigator_bindings_v8_with_identity(rt, profile, &current_identity())
}

/// [`install_navigator_bindings_v8_with`] against an explicit identity too.
#[cfg(feature = "v8-backend")]
pub(crate) fn install_navigator_bindings_v8_with_identity(
    rt: &crate::v8_runtime::V8JsRuntime,
    profile: &NavigatorProfile,
    id: &lumen_core::Identity,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(&build_navigator_shim(profile, id))?;
    Ok(())
}

/// `navigator.platform` for the page: the profile's value, except that the
/// default profile's `Win32` follows the identity (a Firefox/Linux or
/// Safari/macOS UA must not sit next to `Win32`).
#[cfg(feature = "v8-backend")]
fn effective_platform<'a>(p: &'a NavigatorProfile, id: &'a lumen_core::Identity) -> &'a str {
    if p.platform == NavigatorProfile::default().platform { &id.platform } else { &p.platform }
}

/// Move every member of the `navigator` singleton onto `Navigator.prototype`
/// (HTML LS §8.9.1, BUG-624).
///
/// `WEB_API_SHIM` creates `navigator` as `Object.create(Navigator.prototype)`,
/// but its own literal and the ~40 per-API modules that run after it in
/// `install_dom` (`permissions.rs`, `webgpu.rs`, this file's own
/// profile shim, …) all put their members on the instance, each with its own
/// idea of the attributes. Rewriting every one of those sites would scatter
/// the WebIDL shape over forty files; instead this pass runs once, at the end
/// of `install_dom` (before the BUG-378 sealing pass), and turns whatever is
/// there into the IDL shape in one place:
///
/// * a function-valued data property is an operation — it moves to the
///   prototype unchanged (writable, enumerable, configurable);
/// * any other data property is a `readonly attribute` — a getter on the
///   prototype that returns the stored value (`[SameObject]` for free);
/// * an accessor keeps its getter, now called with the instance.
///
/// Getters are brand-checked (`this !== navigator` → `TypeError`), as a WebIDL
/// attribute getter is. Afterwards `navigator` has no own properties at all,
/// like in every browser. Non-configurable own properties are left alone —
/// they cannot be deleted from the instance.
#[cfg(feature = "v8-backend")]
pub(crate) fn finalize_navigator_interface_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(FINALIZE_NAVIGATOR_INTERFACE)?;
    Ok(())
}

#[cfg(feature = "v8-backend")]
const FINALIZE_NAVIGATOR_INTERFACE: &str = r#"(function() {
  'use strict';
  if (typeof Navigator !== 'function' || typeof navigator !== 'object' ||
      navigator === null || Object.getPrototypeOf(navigator) !== Navigator.prototype) return;
  var nav = navigator;
  var proto = Navigator.prototype;
  function getterFor(name, read) {
    // An object-literal getter carries the WebIDL name `get <name>` and length 0.
    return Object.getOwnPropertyDescriptor({
      get [name]() {
        if (this !== nav) throw new TypeError('Illegal invocation');
        return read();
      }
    }, name).get;
  }
  Object.getOwnPropertyNames(nav).forEach(function(k) {
    var d = Object.getOwnPropertyDescriptor(nav, k);
    if (!d || !d.configurable) return;
    var pd;
    if ('value' in d && typeof d.value === 'function') {
      pd = { value: d.value, writable: true, enumerable: true, configurable: true };
    } else if ('value' in d) {
      var v = d.value;
      pd = { get: getterFor(k, function() { return v; }), enumerable: true, configurable: true };
    } else if (d.get) {
      var g = d.get;
      pd = { get: getterFor(k, function() { return g.call(nav); }), enumerable: true, configurable: true };
    } else {
      return;
    }
    Object.defineProperty(proto, k, pd);
    delete nav[k];
  });
  Object.defineProperty(proto, Symbol.toStringTag, {
    value: 'Navigator', writable: false, enumerable: false, configurable: true });
  Object.defineProperty(Navigator, 'prototype', { writable: false });
  // Interface objects are non-enumerable own properties of the global.
  Object.defineProperty(globalThis, 'Navigator', { enumerable: false });
})();"#;

/// Render a JS array literal from a locale list, falling back to `["en-US"]`
/// when empty. Each entry is JSON-escaped to stay injection-safe.
#[cfg(feature = "v8-backend")]
fn languages_literal(languages: &[String]) -> String {
    let mut langs: Vec<&str> = languages.iter().map(String::as_str).collect();
    if langs.is_empty() {
        langs.push("en-US");
    }
    let items: Vec<String> = langs.iter().map(|l| json_string(l)).collect();
    format!("[{}]", items.join(", "))
}

/// Escape a string for safe embedding as a JS/JSON string literal (with quotes).
#[cfg(feature = "v8-backend")]
pub(crate) fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Build the `_lumen_navigator_id` object a `WorkerGlobalScope` hangs its
/// `WorkerNavigator` accessors off (BUG-776), using the process-global
/// [`NavigatorProfile`] — the same source the page's own `navigator.platform`/
/// `language`/`languages` come from, so a `fingerprint.toml` override cannot
/// make the two disagree.
///
/// The rest of `NavigatorID` (HTML LS §8.9.1.1) is fixed, and the values are
/// the ones the *page* ends up reporting once its own layers have run:
/// `appCodeName`/`appName`/`product` from [`crate::surface_api`]'s antidetect
/// patch, `userAgent`/`appVersion` from [`lumen_core::Identity`] (the same
/// source the page shim in this module uses). They are spelled out a second time here because a worker
/// runs in its own isolate and neither of those layers is installed in it —
/// `worker_navigator_matches_the_page_navigator` compares the two live scopes
/// member by member so the copies cannot drift apart unnoticed.
///
/// `productSub`/`vendor`/`vendorSub` are deliberately absent: those three
/// members of the mixin are `[Exposed=Window]`.
#[cfg(feature = "v8-backend")]
pub(crate) fn worker_navigator_id_shim() -> String {
    worker_navigator_id_shim_with(&current_navigator_profile())
}

/// [`worker_navigator_id_shim`] against an explicit profile, ignoring the
/// process-global — same split (and same reason) as
/// [`install_navigator_bindings_v8_with`]: a test that pinned the global would
/// race `set_and_read_global_profile`.
#[cfg(feature = "v8-backend")]
pub(crate) fn worker_navigator_id_shim_with(p: &NavigatorProfile) -> String {
    worker_navigator_id_shim_for(p, &current_identity())
}

/// [`worker_navigator_id_shim_with`] against an explicit identity.
#[cfg(feature = "v8-backend")]
pub(crate) fn worker_navigator_id_shim_for(p: &NavigatorProfile, id: &lumen_core::Identity) -> String {
    let user_agent_data = id.ua_data.as_ref().map_or_else(String::new, |d| {
        format!(", userAgentData: {}", crate::ua_client_hints::ua_data_instance_expr(d))
    });
    format!(
        "var _lumen_navigator_id = {{\
           appCodeName: 'Mozilla',\
           appName: 'Netscape',\
           appVersion: {app_version},\
           product: 'Gecko',\
           userAgent: {user_agent},\
           onLine: false,\
           platform: {platform},\
           language: {primary_language},\
           languages: {languages}{user_agent_data}\
         }}; undefined;",
        app_version = json_string(&id.app_version),
        user_agent = json_string(&id.user_agent),
        user_agent_data = user_agent_data,
        platform = json_string(effective_platform(p, id)),
        primary_language = json_string(p.languages.first().map_or("en-US", String::as_str)),
        languages = languages_literal(&p.languages),
    )
}

/// Build the navigator/screen/timezone shim source for the given profile.
#[cfg(feature = "v8-backend")]
fn build_navigator_shim(p: &NavigatorProfile, id: &lumen_core::Identity) -> String {
    let languages = languages_literal(&p.languages);
    let primary_language = json_string(p.languages.first().map_or("en-US", String::as_str));
    let platform = json_string(effective_platform(p, id));
    let user_agent = json_string(&id.user_agent);
    let app_version = json_string(&id.app_version);
    let vendor = json_string(&id.vendor);
    let product_sub = json_string(&id.product_sub);
    format!(
        r#"(function() {{
  // ── navigator properties ────────────────────────────────────────────────────
  if (typeof navigator !== 'undefined') {{
    // Name of the browser (BUG-1614): every field comes from the one
    // lumen_core::Identity the HTTP layer's headers are derived from.
    try {{ navigator.userAgent = {user_agent}; }} catch(_) {{}}
    try {{
      Object.defineProperty(navigator, 'appVersion', {{
        value: {app_version}, writable: false, configurable: true, enumerable: true
      }});
      Object.defineProperty(navigator, 'vendor', {{
        value: {vendor}, writable: false, configurable: true, enumerable: true
      }});
      Object.defineProperty(navigator, 'productSub', {{
        value: {product_sub}, writable: false, configurable: true, enumerable: true
      }});
    }} catch(_) {{}}

    // hardwareConcurrency: report a fixed logical core count.
    try {{
      Object.defineProperty(navigator, 'hardwareConcurrency', {{
        value: {hardware_concurrency}, writable: false, configurable: true, enumerable: true
      }});
    }} catch(_) {{}}

    // deviceMemory: fixed RAM size in GiB (spec rounds to powers of two).
    try {{
      Object.defineProperty(navigator, 'deviceMemory', {{
        value: {device_memory}, writable: false, configurable: true, enumerable: true
      }});
    }} catch(_) {{}}

    // platform: fixed UA platform string.
    try {{
      Object.defineProperty(navigator, 'platform', {{
        value: {platform}, writable: false, configurable: true, enumerable: true
      }});
    }} catch(_) {{}}

    // languages: configured locale list.
    try {{
      Object.defineProperty(navigator, 'languages', {{
        get: function() {{ return {languages}; }},
        configurable: true, enumerable: true
      }});
    }} catch(_) {{}}

    // language: primary locale (keep consistent with languages[0]).
    try {{
      Object.defineProperty(navigator, 'language', {{
        value: {primary_language}, writable: false, configurable: true, enumerable: true
      }});
    }} catch(_) {{}}
  }}

  // ── screen object ───────────────────────────────────────────────────────────
  // Define a normalised screen on globalThis. Sites that read screen.width to
  // guess the display resolution get the configured value instead.
  var _screen = {{
    width: {screen_width},
    height: {screen_height},
    availWidth: {screen_width},
    availHeight: {screen_height},
    colorDepth: {color_depth},
    pixelDepth: {color_depth},
    orientation: {{ type: 'landscape-primary', angle: 0 }}
  }};
  try {{
    // Lock the dimension fields per CSSOM View §4.1's `readonly` attributes.
    // `orientation` is deliberately left writable: screen_orientation.rs
    // replaces this stub with the real ScreenOrientation instance right
    // after this shim runs.
    for (const name of ['width', 'height', 'availWidth', 'availHeight', 'colorDepth', 'pixelDepth']) {{
      Object.defineProperty(_screen, name, {{
        value: _screen[name], writable: false, configurable: true, enumerable: true
      }});
    }}
    Object.defineProperty(globalThis, 'screen', {{
      value: _screen, writable: false, configurable: true, enumerable: true
    }});
  }} catch(_) {{}}

  // ── window position (CSSOM View §5) ─────────────────────────────────────────
  // No real windowing backend to query here, so report the origin; screenLeft/
  // screenTop are legacy aliases and must equal screenX/screenY respectively.
  try {{
    for (const name of ['screenX', 'screenY', 'screenLeft', 'screenTop']) {{
      Object.defineProperty(globalThis, name, {{
        value: 0, writable: false, configurable: true, enumerable: true
      }});
    }}
  }} catch(_) {{}}

  // ── timezone normalisation ──────────────────────────────────────────────────
  // Override getTimezoneOffset to return the configured offset (0 = UTC).
  // Fingerprinting scripts call new Date().getTimezoneOffset() to infer the
  // local timezone; a fixed value collapses users without breaking arithmetic.
  try {{
    Date.prototype.getTimezoneOffset = function() {{ return {timezone_offset}; }};
  }} catch(_) {{}}
}})();
"#,
        hardware_concurrency = p.hardware_concurrency,
        device_memory = p.device_memory,
        platform = platform,
        languages = languages,
        primary_language = primary_language,
        screen_width = p.screen_width,
        screen_height = p.screen_height,
        color_depth = p.color_depth,
        timezone_offset = p.timezone_offset,
    )
}

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::JsValue;
    use lumen_core::ext::JsRuntime as _;

    fn make_rt() -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval("var navigator = { language: 'en-US' };").unwrap();
        rt
    }

    #[test]
    fn install_succeeds() {
        let rt = make_rt();
        install_navigator_bindings_v8(&rt).expect("install should succeed");
    }

    #[test]
    fn install_succeeds_without_navigator() {
        let rt = V8JsRuntime::new().unwrap();
        install_navigator_bindings_v8(&rt)
            .expect("install should succeed even without navigator");
    }

    #[test]
    fn hardware_concurrency_is_two() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let v = rt.eval("navigator.hardwareConcurrency").unwrap();
        assert_eq!(v, JsValue::Number(2.0));
    }

    #[test]
    fn device_memory_is_eight() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let v = rt.eval("navigator.deviceMemory").unwrap();
        assert_eq!(v, JsValue::Number(8.0));
    }

    #[test]
    fn platform_is_win32() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let v = rt.eval("navigator.platform").unwrap();
        assert_eq!(v, JsValue::String("Win32".to_string()));
    }

    #[test]
    fn languages_is_array_en() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let ok = rt
            .eval("navigator.languages[0] === 'en-US' && navigator.languages[1] === 'en'")
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn screen_width_and_height() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let ok = rt.eval("screen.width === 1920 && screen.height === 1080").unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn screen_avail_dimensions_match() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let eq = rt
            .eval("screen.availWidth === screen.width && screen.availHeight === screen.height")
            .unwrap();
        assert_eq!(eq, JsValue::Bool(true), "availWidth/availHeight must equal width/height");
    }

    #[test]
    fn screen_color_depth_is_24() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let ok = rt.eval("screen.colorDepth === 24 && screen.pixelDepth === 24").unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn screen_orientation_landscape() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let ok = rt
            .eval("screen.orientation.type === 'landscape-primary' && screen.orientation.angle === 0")
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn screen_dimensions_are_readonly() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let ok = rt
            .eval(
                "(function() { \
                    screen.width = 0; screen.height = 0; \
                    return screen.width === 1920 && screen.height === 1080; \
                })()",
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true), "screen dimensions must stay immutable");
    }

    #[test]
    fn screen_position_properties_exist() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let ok = rt
            .eval(
                "typeof screenX === 'number' && typeof screenY === 'number' \
                    && screenLeft === screenX && screenTop === screenY",
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn timezone_offset_is_zero() {
        let rt = make_rt();
        install_navigator_bindings_v8_with(&rt, &NavigatorProfile::default()).unwrap();
        let offset = rt.eval("new Date().getTimezoneOffset()").unwrap();
        assert_eq!(offset, JsValue::Number(0.0), "getTimezoneOffset must return 0 (UTC)");
    }

    // ── custom profile (9F.1) ────────────────────────────────────────────────

    fn custom_profile() -> NavigatorProfile {
        NavigatorProfile {
            hardware_concurrency: 8,
            device_memory: 16,
            platform: "Linux x86_64".to_string(),
            languages: vec!["de-DE".to_string(), "de".to_string(), "en".to_string()],
            screen_width: 2560,
            screen_height: 1440,
            color_depth: 30,
            timezone_offset: -120,
        }
    }

    #[test]
    fn custom_profile_applies_all_fields() {
        let rt = make_rt();
        let p = custom_profile();
        install_navigator_bindings_v8_with(&rt, &p).unwrap();
        let ok = rt
            .eval(
                r#"
                navigator.hardwareConcurrency === 8
                  && navigator.deviceMemory === 16
                  && navigator.platform === 'Linux x86_64'
                  && navigator.languages[0] === 'de-DE'
                  && navigator.languages[2] === 'en'
                  && navigator.language === 'de-DE'
                  && screen.width === 2560
                  && screen.height === 1440
                  && screen.colorDepth === 30
                  && new Date().getTimezoneOffset() === -120
                "#,
            )
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn empty_languages_falls_back_to_en_us() {
        let rt = make_rt();
        let p = NavigatorProfile {
            languages: Vec::new(),
            ..Default::default()
        };
        install_navigator_bindings_v8_with(&rt, &p).unwrap();
        let ok = rt
            .eval("navigator.languages[0] === 'en-US' && navigator.language === 'en-US'")
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn language_with_quote_is_escaped_safely() {
        // A malicious/odd locale containing a quote must not break the shim.
        let rt = make_rt();
        let p = NavigatorProfile {
            languages: vec!["en\"-X".to_string()],
            ..Default::default()
        };
        install_navigator_bindings_v8_with(&rt, &p).unwrap();
        let v = rt.eval("navigator.languages[0]").unwrap();
        assert_eq!(v, JsValue::String("en\"-X".to_string()));
    }

    #[test]
    fn default_profile_matches_legacy_values() {
        let p = NavigatorProfile::default();
        assert_eq!(p.hardware_concurrency, 2);
        assert_eq!(p.device_memory, 8);
        assert_eq!(p.platform, "Win32");
        assert_eq!(p.languages, vec!["en-US".to_string(), "en".to_string()]);
        assert_eq!(p.screen_width, 1920);
        assert_eq!(p.screen_height, 1080);
        assert_eq!(p.color_depth, 24);
        assert_eq!(p.timezone_offset, 0);
    }

    /// BUG-776: the fixed half of `NavigatorID` is written out twice — once by
    /// the page's own layers (`dom.rs`'s `navigator` literal plus
    /// `surface_api`'s antidetect patch) and once by
    /// [`worker_navigator_id_shim`], because a worker isolate runs neither of
    /// them. The mixin only means anything if the two agree, and
    /// `interfaces/WorkerUtils/navigator/*` asserts exactly that by comparing
    /// the worker's value against the window's — so compare the two live
    /// scopes here rather than trusting the constants to stay in step.
    ///
    /// `platform`/`language`/`languages` are excluded on purpose: they come
    /// from the process-global profile, which `set_and_read_global_profile`
    /// mutates, so reading them from two runtimes could race. Their agreement
    /// is covered by `worker_navigator_id_takes_platform_and_languages_from_profile`.
    #[test]
    fn worker_navigator_matches_the_page_navigator() {
        use std::sync::{Arc, Mutex};

        let page = V8JsRuntime::new().unwrap();
        page.install_dom(
            Arc::new(Mutex::new(lumen_dom::Document::new())),
            "https://example.test/",
            None, None, None, None, None, None, None, None, None, false, None,
        )
        .unwrap();

        let worker = V8JsRuntime::new().unwrap();
        crate::worker::install_worker_scope_globals_v8(&worker, None).unwrap();

        for name in [
            "appCodeName",
            "appName",
            "appVersion",
            "product",
            "userAgent",
            "onLine",
            "userAgentData",
            "userAgentData.brands.map(function(b) { return b.brand + '/' + b.version; })",
            "userAgentData.platform",
        ] {
            let expr = format!("String(navigator.{name})");
            assert_eq!(
                page.eval(&expr).unwrap(),
                worker.eval(&expr).unwrap(),
                "navigator.{name} differs between the page and a worker scope"
            );
        }
    }

    /// The profile-driven half of the same object: whatever the shell put in
    /// `fingerprint.toml` must reach a worker too, or the page and the worker
    /// would report different platforms under any non-default profile.
    #[test]
    fn worker_navigator_id_takes_platform_and_languages_from_profile() {
        let rt = V8JsRuntime::new().unwrap();
        rt.eval(&worker_navigator_id_shim_with(&custom_profile())).unwrap();

        assert_eq!(
            rt.eval("_lumen_navigator_id.platform").unwrap(),
            JsValue::String("Linux x86_64".to_string())
        );
        assert_eq!(
            rt.eval("_lumen_navigator_id.language").unwrap(),
            JsValue::String("de-DE".to_string())
        );
        assert_eq!(
            rt.eval("_lumen_navigator_id.languages.join(',')").unwrap(),
            JsValue::String("de-DE,de,en".to_string())
        );
    }

    /// BUG-1614 gate (ICR): the page, a worker scope and the HTTP `Sec-CH-UA*`
    /// headers all name the same browser — every value is derived from one
    /// `lumen_core::Identity`.
    #[test]
    fn chrome_identity_is_the_same_in_page_worker_and_client_hints() {
        use std::sync::{Arc, Mutex};

        let id = lumen_core::Identity::from_user_agent(lumen_core::CHROME_USER_AGENT);
        let hints = id.low_entropy_client_hints();

        let page = V8JsRuntime::new().unwrap();
        page.install_dom(
            Arc::new(Mutex::new(lumen_dom::Document::new())),
            "https://example.test/",
            None, None, None, None, None, None, None, None, None, false, None,
        )
        .unwrap();
        let worker = V8JsRuntime::new().unwrap();
        crate::worker::install_worker_scope_globals_v8(&worker, None).unwrap();

        // Re-serialise navigator.userAgentData.brands as a Sec-CH-UA header.
        let sec_ch_ua = "navigator.userAgentData.brands.map(function(b) {             return '\"' + b.brand + '\";v=\"' + b.version + '\"'; }).join(', ')";
        for (ctx, rt) in [("page", &page), ("worker", &worker)] {
            assert_eq!(
                rt.eval("navigator.userAgent").unwrap(),
                JsValue::String(id.user_agent.clone()),
                "{ctx}: userAgent"
            );
            assert_eq!(
                rt.eval("navigator.appVersion").unwrap(),
                JsValue::String(id.app_version.clone()),
                "{ctx}: appVersion"
            );
            assert_eq!(
                rt.eval(sec_ch_ua).unwrap(),
                JsValue::String(hints[0].1.clone()),
                "{ctx}: userAgentData.brands vs Sec-CH-UA"
            );
            assert_eq!(
                rt.eval("'\"' + navigator.userAgentData.platform + '\"'").unwrap(),
                JsValue::String(hints[2].1.clone()),
                "{ctx}: userAgentData.platform vs Sec-CH-UA-Platform"
            );
        }
        assert_eq!(
            page.eval("navigator.vendor + '|' + navigator.productSub").unwrap(),
            JsValue::String("Google Inc.|20030107".to_string())
        );
        assert!(id.app_version.contains(&format!("Chrome/{}", lumen_core::CHROME_MAJOR)));
    }

    /// Non-Chromium identities have no `userAgentData` and Gecko's `vendor`.
    #[test]
    fn firefox_identity_has_no_user_agent_data() {
        let id = lumen_core::Identity::from_user_agent(
            "Mozilla/5.0 (Windows NT 10.0; rv:128.0) Gecko/20100101 Firefox/128.0",
        );
        let rt = make_rt();
        install_navigator_bindings_v8_with_identity(&rt, &NavigatorProfile::default(), &id).unwrap();
        crate::ua_client_hints::install_ua_client_hints_bindings_v8_with(&rt, &id).unwrap();
        assert_eq!(
            rt.eval("typeof navigator.userAgentData + '|' + typeof NavigatorUAData").unwrap(),
            JsValue::String("undefined|undefined".to_string())
        );
        assert_eq!(
            rt.eval("navigator.userAgent + '|' + navigator.vendor + '|' + navigator.productSub").unwrap(),
            JsValue::String(format!("{}||20100101", id.user_agent))
        );
    }

    #[test]
    fn set_and_read_global_profile() {
        // NB: mutates the process-global; uses a distinct value so the assert is
        // unambiguous even if another test reads concurrently.
        let p = custom_profile();
        set_navigator_profile(p.clone());
        assert_eq!(current_navigator_profile(), p);
    }
}

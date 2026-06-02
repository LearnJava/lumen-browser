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
//! Must be called **after** `dom::install_dom_api` (requires `navigator` to exist).
//!
//! The exact values are taken from a process-global [`NavigatorProfile`] that
//! the shell may override from `fingerprint.toml` (9F.1) via
//! [`set_navigator_profile`]. When unset, the defaults reproduce the historical
//! hardcoded mid-tier device values, so behaviour is unchanged without a config.

use rquickjs::Ctx;
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
/// Must be called after `install_dom_api`.
pub fn install_navigator_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    install_navigator_bindings_with(ctx, &current_navigator_profile())
}

/// Install the navigator shim using an explicit [`NavigatorProfile`], ignoring
/// the process-global. Used by tests and callers that want full control.
pub fn install_navigator_bindings_with(
    ctx: &Ctx,
    profile: &NavigatorProfile,
) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(build_navigator_shim(profile))?;
    Ok(())
}

/// Render a JS array literal from a locale list, falling back to `["en-US"]`
/// when empty. Each entry is JSON-escaped to stay injection-safe.
fn languages_literal(languages: &[String]) -> String {
    let mut langs: Vec<&str> = languages.iter().map(String::as_str).collect();
    if langs.is_empty() {
        langs.push("en-US");
    }
    let items: Vec<String> = langs.iter().map(|l| json_string(l)).collect();
    format!("[{}]", items.join(", "))
}

/// Escape a string for safe embedding as a JS/JSON string literal (with quotes).
fn json_string(s: &str) -> String {
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

/// Build the navigator/screen/timezone shim source for the given profile.
fn build_navigator_shim(p: &NavigatorProfile) -> String {
    let languages = languages_literal(&p.languages);
    let primary_language = json_string(p.languages.first().map_or("en-US", String::as_str));
    let platform = json_string(&p.platform);
    format!(
        r#"(function() {{
  // ── navigator properties ────────────────────────────────────────────────────
  if (typeof navigator !== 'undefined') {{
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
    Object.defineProperty(globalThis, 'screen', {{
      value: _screen, writable: false, configurable: true, enumerable: true
    }});
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

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_nav(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>("var navigator = { language: 'en-US' };").unwrap();
    }

    #[test]
    fn install_succeeds() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings(&ctx).expect("install should succeed");
        });
    }

    #[test]
    fn install_succeeds_without_navigator() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_navigator_bindings(&ctx).expect("install should succeed even without navigator");
        });
    }

    #[test]
    fn hardware_concurrency_is_two() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let v: f64 = ctx.eval("navigator.hardwareConcurrency").unwrap();
            assert_eq!(v as u32, 2);
        });
    }

    #[test]
    fn device_memory_is_eight() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let v: f64 = ctx.eval("navigator.deviceMemory").unwrap();
            assert_eq!(v as u32, 8);
        });
    }

    #[test]
    fn platform_is_win32() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let v: String = ctx.eval("navigator.platform").unwrap();
            assert_eq!(v, "Win32");
        });
    }

    #[test]
    fn languages_is_array_en() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let first: String = ctx.eval("navigator.languages[0]").unwrap();
            let second: String = ctx.eval("navigator.languages[1]").unwrap();
            assert_eq!(first, "en-US");
            assert_eq!(second, "en");
        });
    }

    #[test]
    fn screen_width_and_height() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let w: f64 = ctx.eval("screen.width").unwrap();
            let h: f64 = ctx.eval("screen.height").unwrap();
            assert_eq!(w as u32, 1920);
            assert_eq!(h as u32, 1080);
        });
    }

    #[test]
    fn screen_avail_dimensions_match() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let eq: bool = ctx
                .eval("screen.availWidth === screen.width && screen.availHeight === screen.height")
                .unwrap();
            assert!(eq, "availWidth/availHeight must equal width/height");
        });
    }

    #[test]
    fn screen_color_depth_is_24() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let cd: f64 = ctx.eval("screen.colorDepth").unwrap();
            let pd: f64 = ctx.eval("screen.pixelDepth").unwrap();
            assert_eq!(cd as u32, 24);
            assert_eq!(pd as u32, 24);
        });
    }

    #[test]
    fn screen_orientation_landscape() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let ty: String = ctx.eval("screen.orientation.type").unwrap();
            let angle: f64 = ctx.eval("screen.orientation.angle").unwrap();
            assert_eq!(ty, "landscape-primary");
            assert_eq!(angle as i32, 0);
        });
    }

    #[test]
    fn timezone_offset_is_zero() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &NavigatorProfile::default()).unwrap();
            let offset: f64 = ctx.eval("new Date().getTimezoneOffset()").unwrap();
            assert_eq!(offset as i32, 0, "getTimezoneOffset must return 0 (UTC)");
        });
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
        let (_rt, ctx) = make_ctx();
        let p = custom_profile();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &p).unwrap();
            let hc: f64 = ctx.eval("navigator.hardwareConcurrency").unwrap();
            let dm: f64 = ctx.eval("navigator.deviceMemory").unwrap();
            let plat: String = ctx.eval("navigator.platform").unwrap();
            let lang0: String = ctx.eval("navigator.languages[0]").unwrap();
            let lang2: String = ctx.eval("navigator.languages[2]").unwrap();
            let primary: String = ctx.eval("navigator.language").unwrap();
            let w: f64 = ctx.eval("screen.width").unwrap();
            let h: f64 = ctx.eval("screen.height").unwrap();
            let cd: f64 = ctx.eval("screen.colorDepth").unwrap();
            let tz: f64 = ctx.eval("new Date().getTimezoneOffset()").unwrap();
            assert_eq!(hc as u32, 8);
            assert_eq!(dm as u32, 16);
            assert_eq!(plat, "Linux x86_64");
            assert_eq!(lang0, "de-DE");
            assert_eq!(lang2, "en");
            assert_eq!(primary, "de-DE");
            assert_eq!(w as u32, 2560);
            assert_eq!(h as u32, 1440);
            assert_eq!(cd as u32, 30);
            assert_eq!(tz as i32, -120);
        });
    }

    #[test]
    fn empty_languages_falls_back_to_en_us() {
        let (_rt, ctx) = make_ctx();
        let p = NavigatorProfile {
            languages: Vec::new(),
            ..Default::default()
        };
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &p).unwrap();
            let lang0: String = ctx.eval("navigator.languages[0]").unwrap();
            let primary: String = ctx.eval("navigator.language").unwrap();
            assert_eq!(lang0, "en-US");
            assert_eq!(primary, "en-US");
        });
    }

    #[test]
    fn language_with_quote_is_escaped_safely() {
        // A malicious/odd locale containing a quote must not break the shim.
        let (_rt, ctx) = make_ctx();
        let p = NavigatorProfile {
            languages: vec!["en\"-X".to_string()],
            ..Default::default()
        };
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings_with(&ctx, &p).unwrap();
            let lang0: String = ctx.eval("navigator.languages[0]").unwrap();
            assert_eq!(lang0, "en\"-X");
        });
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

    #[test]
    fn set_and_read_global_profile() {
        // NB: mutates the process-global; uses a distinct value so the assert is
        // unambiguous even if another test reads concurrently.
        let p = custom_profile();
        set_navigator_profile(p.clone());
        assert_eq!(current_navigator_profile(), p);
    }
}

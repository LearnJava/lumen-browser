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

use rquickjs::Ctx;

/// Install navigator/screen/timezone normalization shim into the JS context.
///
/// Overwrites high-entropy fingerprinting properties on `navigator` and
/// creates a normalised `screen` object on `globalThis`. Also patches
/// `Date.prototype.getTimezoneOffset` to return 0, so timezone cannot be
/// inferred from JS date arithmetic.
///
/// Must be called after `install_dom_api`.
pub fn install_navigator_bindings(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(NAVIGATOR_SHIM)?;
    Ok(())
}

/// JavaScript shim: normalise navigator, screen, and timezone.
const NAVIGATOR_SHIM: &str = r#"(function() {
  // ── navigator properties ────────────────────────────────────────────────────
  if (typeof navigator !== 'undefined') {
    // hardwareConcurrency: expose 2 instead of real core count.
    try {
      Object.defineProperty(navigator, 'hardwareConcurrency', {
        value: 2, writable: false, configurable: true, enumerable: true
      });
    } catch(_) {}

    // deviceMemory: 8 GB (most common mid-range value in spec rounding).
    try {
      Object.defineProperty(navigator, 'deviceMemory', {
        value: 8, writable: false, configurable: true, enumerable: true
      });
    } catch(_) {}

    // platform: fixed to Win32 (most prevalent desktop UA platform string).
    try {
      Object.defineProperty(navigator, 'platform', {
        value: 'Win32', writable: false, configurable: true, enumerable: true
      });
    } catch(_) {}

    // languages: single common locale list.
    try {
      Object.defineProperty(navigator, 'languages', {
        get: function() { return ['en-US', 'en']; },
        configurable: true, enumerable: true
      });
    } catch(_) {}

    // language: primary locale (keep consistent with languages[0]).
    try {
      Object.defineProperty(navigator, 'language', {
        value: 'en-US', writable: false, configurable: true, enumerable: true
      });
    } catch(_) {}
  }

  // ── screen object ───────────────────────────────────────────────────────────
  // Define a normalised screen on globalThis. Sites that read screen.width to
  // guess the display resolution get a common 1920x1080 value instead.
  var _screen = {
    width: 1920,
    height: 1080,
    availWidth: 1920,
    availHeight: 1080,
    colorDepth: 24,
    pixelDepth: 24,
    orientation: { type: 'landscape-primary', angle: 0 }
  };
  try {
    Object.defineProperty(globalThis, 'screen', {
      value: _screen, writable: false, configurable: true, enumerable: true
    });
  } catch(_) {}

  // ── timezone normalisation ──────────────────────────────────────────────────
  // Override getTimezoneOffset to always return 0 (UTC). Fingerprinting scripts
  // call new Date().getTimezoneOffset() to infer the local timezone; returning
  // 0 collapses all users to UTC without breaking time arithmetic.
  try {
    Date.prototype.getTimezoneOffset = function() { return 0; };
  } catch(_) {}
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
            install_navigator_bindings(&ctx).unwrap();
            let v: f64 = ctx.eval("navigator.hardwareConcurrency").unwrap();
            assert_eq!(v as u32, 2);
        });
    }

    #[test]
    fn device_memory_is_eight() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings(&ctx).unwrap();
            let v: f64 = ctx.eval("navigator.deviceMemory").unwrap();
            assert_eq!(v as u32, 8);
        });
    }

    #[test]
    fn platform_is_win32() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings(&ctx).unwrap();
            let v: String = ctx.eval("navigator.platform").unwrap();
            assert_eq!(v, "Win32");
        });
    }

    #[test]
    fn languages_is_array_en() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_nav(&ctx);
            install_navigator_bindings(&ctx).unwrap();
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
            install_navigator_bindings(&ctx).unwrap();
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
            install_navigator_bindings(&ctx).unwrap();
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
            install_navigator_bindings(&ctx).unwrap();
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
            install_navigator_bindings(&ctx).unwrap();
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
            install_navigator_bindings(&ctx).unwrap();
            let offset: f64 = ctx.eval("new Date().getTimezoneOffset()").unwrap();
            assert_eq!(offset as i32, 0, "getTimezoneOffset must return 0 (UTC)");
        });
    }
}

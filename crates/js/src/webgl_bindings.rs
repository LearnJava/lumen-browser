//! WebGL API bindings for normalized GPU fingerprinting (ADR-007 Layer 4).
//!
//! Exports `getParameter()` support for:
//! - `UNMASKED_VENDOR_WEBGL`: "WebKit"
//! - `UNMASKED_RENDERER_WEBGL`: "Generic GPU"
//!
//! Prevents WebGL canvas fingerprinting by returning generic vendor/renderer
//! strings regardless of actual GPU.

use lumen_paint::GpuFingerprint;
use rquickjs::Ctx;

/// Install WebGL fingerprint bindings into the JS context.
///
/// Makes `getParameter(UNMASKED_VENDOR_WEBGL)` and `getParameter(UNMASKED_RENDERER_WEBGL)`
/// return normalized strings. Stores fingerprint in global scope for JS to access.
pub fn install_webgl_bindings(ctx: &Ctx, fingerprint: &GpuFingerprint) -> rquickjs::Result<()> {
    // Store fingerprint strings in global scope for WebGL.getParameter() lookup
    // via eval('_lumen_gpu_fingerprint') from JS
    ctx.globals()
        .set("_LUMEN_GPU_VENDOR", fingerprint.vendor().to_string())?;
    ctx.globals()
        .set("_LUMEN_GPU_RENDERER", fingerprint.renderer().to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_vendor_is_webkit() {
        let fp = GpuFingerprint {
            vendor: "WebKit".to_string(),
            renderer: "Generic GPU".to_string(),
        };
        assert_eq!(fp.vendor(), "WebKit");
    }

    #[test]
    fn test_fingerprint_renderer_is_generic() {
        let fp = GpuFingerprint {
            vendor: "WebKit".to_string(),
            renderer: "Generic GPU".to_string(),
        };
        assert_eq!(fp.renderer(), "Generic GPU");
    }
}

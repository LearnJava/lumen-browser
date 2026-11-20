//! WebGL/rendering fingerprint normalization (ADR-007 Layer 4).
//!
//! Normalizes wgpu adapter info (renderer/vendor strings) to generic values
//! to prevent GPU fingerprinting. Applied at adapter initialization.
//!
//! Examples (per-profile, see ADR-007 §Fingerprint profiles):
//! - Standard: `(renderer, vendor)` → ("Generic GPU", "WebKit")
//! - Strict: same as Standard
//! - Tor: same as Standard

/// Normalized GPU vendor string across all profiles — the `WEBGL_debug_renderer_info`
/// UNMASKED_VENDOR_WEBGL value. Chrome on Windows reports "Google Inc. (<backend>)";
/// "WebKit"/"Generic GPU" was the pre-ANGLE software-GL tell anti-bot probes flag.
#[cfg(any(feature = "backend-wgpu", test))]
const NORMALIZED_VENDOR: &str = "Google Inc. (Intel)";

/// Normalized GPU renderer string across all profiles — UNMASKED_RENDERER_WEBGL.
/// A stock ANGLE D3D11 descriptor matching the integrated Intel iGPU class;
/// fixed (not adapter-derived) so the value cannot be used to single out a
/// machine, while still looking like a real Chrome/ANGLE backend.
#[cfg(any(feature = "backend-wgpu", test))]
const NORMALIZED_RENDERER: &str =
    "ANGLE (Intel, Intel(R) Iris(R) Plus Graphics Direct3D11 vs_5_0 ps_5_0, D3D11)";

/// GPU fingerprint info: normailzed vendor and renderer strings.
#[derive(Debug, Clone)]
pub struct GpuFingerprint {
    /// Normalized vendor string (always "WebKit").
    pub vendor: String,
    /// Normalized renderer string (always "Generic GPU").
    pub renderer: String,
}

impl GpuFingerprint {
    /// Create normalized GPU fingerprint from wgpu adapter info.
    ///
    /// Always returns ("WebKit", "Generic GPU") regardless of actual
    /// adapter. The actual adapter info is discarded to prevent
    /// WebGL fingerprinting attacks (ADR-007).
    /// Only available with `backend-wgpu` feature.
    #[cfg(feature = "backend-wgpu")]
    pub fn from_adapter_info(_adapter_info: &wgpu::AdapterInfo) -> Self {
        GpuFingerprint {
            vendor: NORMALIZED_VENDOR.to_string(),
            renderer: NORMALIZED_RENDERER.to_string(),
        }
    }

    /// Vendor string: always "WebKit".
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    /// Renderer string: always "Generic GPU".
    pub fn renderer(&self) -> &str {
        &self.renderer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalized_vendor_is_webkit() {
        let fp = GpuFingerprint {
            vendor: NORMALIZED_VENDOR.to_string(),
            renderer: NORMALIZED_RENDERER.to_string(),
        };
        assert_eq!(fp.vendor(), "WebKit");
    }

    #[test]
    fn test_normalized_renderer_is_generic() {
        let fp = GpuFingerprint {
            vendor: NORMALIZED_VENDOR.to_string(),
            renderer: NORMALIZED_RENDERER.to_string(),
        };
        assert_eq!(fp.renderer(), "Generic GPU");
    }

    #[test]
    fn test_fingerprint_immutable() {
        let fp = GpuFingerprint {
            vendor: NORMALIZED_VENDOR.to_string(),
            renderer: NORMALIZED_RENDERER.to_string(),
        };
        // Ensure normalization is consistent
        assert_eq!(fp.vendor, "WebKit");
        assert_eq!(fp.renderer, "Generic GPU");
    }
}

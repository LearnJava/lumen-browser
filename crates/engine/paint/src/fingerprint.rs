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
pub const NORMALIZED_VENDOR: &str = "Google Inc. (Intel)";

/// Normalized GPU renderer string across all profiles — UNMASKED_RENDERER_WEBGL.
/// A stock ANGLE D3D11 descriptor matching the integrated Intel iGPU class;
/// fixed (not adapter-derived) so the value cannot be used to single out a
/// machine, while still looking like a real Chrome/ANGLE backend.
pub const NORMALIZED_RENDERER: &str =
    "ANGLE (Intel, Intel(R) Iris(R) Plus Graphics (0x00008A52) Direct3D11 vs_5_0 ps_5_0, D3D11)";

/// GPU fingerprint info: normailzed vendor and renderer strings.
#[derive(Debug, Clone)]
pub struct GpuFingerprint {
    /// Normalized vendor string (ANGLE backend vendor).
    pub vendor: String,
    /// Normalized renderer string (ANGLE backend renderer descriptor).
    pub renderer: String,
}

impl GpuFingerprint {
    /// The fixed normalized pair for callers that have no wgpu adapter to
    /// derive from (the DOM-only JS runtime path) — identical to what
    /// [`Self::from_adapter_info`] returns for any adapter.
    pub fn normalized() -> Self {
        GpuFingerprint {
            vendor: NORMALIZED_VENDOR.to_string(),
            renderer: NORMALIZED_RENDERER.to_string(),
        }
    }

    /// Create normalized GPU fingerprint from wgpu adapter info.
    ///
    /// The actual adapter info is discarded to prevent WebGL fingerprinting
    /// attacks (ADR-007) — the result is always the fixed ANGLE pair.
    /// Only available with `backend-wgpu` feature.
    #[cfg(feature = "backend-wgpu")]
    pub fn from_adapter_info(_adapter_info: &wgpu::AdapterInfo) -> Self {
        Self::normalized()
    }

    /// Vendor string.
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    /// Renderer string.
    pub fn renderer(&self) -> &str {
        &self.renderer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalized_vendor_is_angle() {
        let fp = GpuFingerprint::normalized();
        assert_eq!(fp.vendor(), NORMALIZED_VENDOR);
        assert_eq!(fp.vendor(), "Google Inc. (Intel)");
    }

    #[test]
    fn test_normalized_renderer_is_angle() {
        let fp = GpuFingerprint::normalized();
        assert_eq!(fp.renderer(), NORMALIZED_RENDERER);
        assert!(fp.renderer().starts_with("ANGLE ("));
    }

    #[test]
    fn test_fingerprint_immutable() {
        let fp = GpuFingerprint::normalized();
        // Ensure normalization is consistent
        assert_eq!(fp.vendor, NORMALIZED_VENDOR);
        assert_eq!(fp.renderer, NORMALIZED_RENDERER);
    }
}

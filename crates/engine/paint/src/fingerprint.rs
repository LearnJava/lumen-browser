//! WebGL/rendering fingerprint normalization (ADR-007 Layer 4).
//!
//! Normalizes wgpu adapter info (renderer/vendor strings) to generic values
//! to prevent GPU fingerprinting. Applied at adapter initialization.
//!
//! Examples (per-profile, see ADR-007 §Fingerprint profiles):
//! - Standard: `(renderer, vendor)` → ("Generic GPU", "WebKit")
//! - Strict: same as Standard
//! - Tor: same as Standard

/// Normalized GPU vendor string across all profiles.
const NORMALIZED_VENDOR: &str = "WebKit";

/// Normalized GPU renderer string across all profiles.
const NORMALIZED_RENDERER: &str = "Generic GPU";

/// GPU fingerprint info: normailzed vendor and renderer strings.
#[derive(Debug, Clone)]
pub struct GpuFingerprint {
    /// Normalized vendor string (always "WebKit").
    pub vendor: String,
    /// Normalized renderer string (always "Generic GPU").
    pub renderer: String,
}

impl GpuFingerprint {
    /// Create normalized GPU fingerprint from adapter info.
    ///
    /// Always returns ("WebKit", "Generic GPU") regardless of actual
    /// adapter. The actual adapter info is discarded to prevent
    /// WebGL fingerprinting attacks (ADR-007).
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

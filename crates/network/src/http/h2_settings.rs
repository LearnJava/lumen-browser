//! HTTP/2 SETTINGS frame values and stream priority matching Chrome.
//!
//! Chrome HTTP/2 SETTINGS are sent in a specific order with specific values.
//! This module implements Chrome-matching HTTP/2 SETTINGS to avoid fingerprinting
//! via HTTP/2 parameter variance (common detection vector for anti-bots).

use crate::http::HttpProfile;

/// HTTP/2 SETTINGS frame values matching Chrome's configuration.
#[derive(Debug, Clone)]
pub struct H2Settings {
    /// SETTINGS_HEADER_TABLE_SIZE (default 4096, Chrome uses 65536).
    pub header_table_size: u32,
    /// SETTINGS_ENABLE_PUSH (default 1).
    pub enable_push: bool,
    /// SETTINGS_MAX_CONCURRENT_STREAMS (default unlimited, Chrome uses 1000).
    pub max_concurrent_streams: Option<u32>,
    /// SETTINGS_INITIAL_WINDOW_SIZE (default 65535, Chrome uses 6291456).
    pub initial_window_size: u32,
    /// SETTINGS_MAX_FRAME_SIZE (default 16384, Chrome uses 16384).
    pub max_frame_size: u32,
    /// SETTINGS_HEADER_COMPRESSION_SIZE_LIMIT (HTTP/2 extension, optional).
    pub header_compression_size_limit: Option<u32>,
}

impl H2Settings {
    /// Create HTTP/2 SETTINGS for the given profile.
    ///
    /// Each profile matches a real browser's HTTP/2 SETTINGS frame values.
    /// Per ADR-007 §«Per-profile HTTP configs», SETTINGS order and values
    /// are a fingerprinting vector — matching the intended browser reduces
    /// false-positive detection on privacy-conscious browsing.
    pub fn for_profile(profile: HttpProfile) -> Self {
        match profile {
            HttpProfile::Chrome | HttpProfile::Strict => {
                // Chrome 130+ HTTP/2 SETTINGS
                Self {
                    header_table_size: 65536,
                    enable_push: true,
                    max_concurrent_streams: Some(1000),
                    initial_window_size: 6291456,   // 6 MB
                    max_frame_size: 16384,
                    header_compression_size_limit: None,
                }
            }
            HttpProfile::Firefox => {
                // Firefox 130+ HTTP/2 SETTINGS
                Self {
                    header_table_size: 65536,
                    enable_push: true,
                    max_concurrent_streams: Some(1000),
                    initial_window_size: 2147483647,  // Very large window (max i32)
                    max_frame_size: 16384,
                    header_compression_size_limit: None,
                }
            }
            HttpProfile::Safari => {
                // Safari 18+ HTTP/2 SETTINGS (conservative)
                Self {
                    header_table_size: 16384,
                    enable_push: true,
                    max_concurrent_streams: Some(500),
                    initial_window_size: 65535,    // RFC default
                    max_frame_size: 16384,
                    header_compression_size_limit: None,
                }
            }
            HttpProfile::Edge => {
                // Edge 130+ HTTP/2 SETTINGS (same as Chrome)
                Self {
                    header_table_size: 65536,
                    enable_push: true,
                    max_concurrent_streams: Some(1000),
                    initial_window_size: 6291456,   // 6 MB (same as Chrome)
                    max_frame_size: 16384,
                    header_compression_size_limit: None,
                }
            }
            HttpProfile::TorBrowser => {
                // Tor Browser HTTP/2 SETTINGS (conservative to avoid unique fingerprint)
                Self {
                    header_table_size: 4096,        // RFC default
                    enable_push: true,
                    max_concurrent_streams: Some(100),  // Conservative
                    initial_window_size: 65535,    // RFC default
                    max_frame_size: 16384,         // RFC default
                    header_compression_size_limit: None,
                }
            }
            HttpProfile::Lumen => {
                // Lumen-native HTTP/2 SETTINGS (optimized for lightweight browser)
                Self {
                    header_table_size: 16384,
                    enable_push: true,
                    max_concurrent_streams: Some(500),
                    initial_window_size: 1048576,   // 1 MB — RAM optimization
                    max_frame_size: 16384,
                    header_compression_size_limit: None,
                }
            }
        }
    }

    /// Convert SETTINGS to HTTP/2 wire format: list of (id, value) pairs.
    ///
    /// Each pair is 6 bytes: 2-byte identifier + 4-byte big-endian value.
    /// Order matches Chrome's transmission order.
    pub fn to_wire_format(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // SETTINGS_HEADER_TABLE_SIZE (0x0001)
        buf.extend_from_slice(&0x0001u16.to_be_bytes());
        buf.extend_from_slice(&self.header_table_size.to_be_bytes());

        // SETTINGS_ENABLE_PUSH (0x0002)
        buf.extend_from_slice(&0x0002u16.to_be_bytes());
        buf.extend_from_slice(&(if self.enable_push { 1u32 } else { 0u32 }).to_be_bytes());

        // SETTINGS_MAX_CONCURRENT_STREAMS (0x0003, optional)
        if let Some(max_streams) = self.max_concurrent_streams {
            buf.extend_from_slice(&0x0003u16.to_be_bytes());
            buf.extend_from_slice(&max_streams.to_be_bytes());
        }

        // SETTINGS_INITIAL_WINDOW_SIZE (0x0004)
        buf.extend_from_slice(&0x0004u16.to_be_bytes());
        buf.extend_from_slice(&self.initial_window_size.to_be_bytes());

        // SETTINGS_MAX_FRAME_SIZE (0x0005)
        buf.extend_from_slice(&0x0005u16.to_be_bytes());
        buf.extend_from_slice(&self.max_frame_size.to_be_bytes());

        // SETTINGS_HEADER_COMPRESSION_SIZE_LIMIT (0x0006, optional, HTTP/2 extension)
        if let Some(limit) = self.header_compression_size_limit {
            buf.extend_from_slice(&0x0006u16.to_be_bytes());
            buf.extend_from_slice(&limit.to_be_bytes());
        }

        buf
    }
}

/// HTTP/2 stream priority information for matching Chrome's priority tree.
#[derive(Debug, Clone)]
pub struct H2StreamPriority {
    /// Stream weight (1–256, default 16).
    pub weight: u8,
    /// Depends on stream ID (0 = root, otherwise parent stream).
    pub depends_on: u32,
    /// Exclusive flag (stream is only dependent on parent).
    pub exclusive: bool,
}

impl H2StreamPriority {
    /// Create default HTTP/2 stream priority for the root stream.
    ///
    /// Chrome uses weight=16, depends_on=0 (root), exclusive=false for initial streams.
    pub fn default_for_profile(_profile: HttpProfile) -> Self {
        Self {
            weight: 16,
            depends_on: 0,
            exclusive: false,
        }
    }

    /// Convert priority to HTTP/2 wire format (PRIORITY frame payload).
    ///
    /// Format: 1 byte exclusive flag + 31-bit stream ID (4 bytes total) + 1 byte weight.
    pub fn to_wire_format(&self) -> [u8; 5] {
        let stream_id_with_exclusive = if self.exclusive {
            self.depends_on | 0x80000000
        } else {
            self.depends_on & 0x7FFFFFFF
        };
        let mut buf = [0u8; 5];
        buf[0..4].copy_from_slice(&stream_id_with_exclusive.to_be_bytes());
        buf[4] = self.weight.saturating_sub(1); // Wire format: weight - 1 (range 0–255)
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_h2_settings_chrome_profile() {
        let settings = H2Settings::for_profile(HttpProfile::Chrome);
        assert_eq!(settings.header_table_size, 65536);
        assert_eq!(settings.max_concurrent_streams, Some(1000));
        assert_eq!(settings.initial_window_size, 6291456);
    }

    #[test]
    fn test_h2_settings_lumen_profile() {
        let settings = H2Settings::for_profile(HttpProfile::Lumen);
        assert_eq!(settings.header_table_size, 16384);
        assert_eq!(settings.max_concurrent_streams, Some(500));
        assert_eq!(settings.initial_window_size, 1048576);
    }

    #[test]
    fn test_h2_settings_tor_browser_profile() {
        let settings = H2Settings::for_profile(HttpProfile::TorBrowser);
        assert_eq!(settings.header_table_size, 4096);
        assert_eq!(settings.max_concurrent_streams, Some(100));
        assert_eq!(settings.initial_window_size, 65535);
    }

    #[test]
    fn test_h2_settings_firefox_profile() {
        let settings = H2Settings::for_profile(HttpProfile::Firefox);
        assert_eq!(settings.header_table_size, 65536);
        assert_eq!(settings.max_concurrent_streams, Some(1000));
        // Firefox uses large initial_window_size
        assert_eq!(settings.initial_window_size, 2147483647);
    }

    #[test]
    fn test_h2_settings_safari_profile() {
        let settings = H2Settings::for_profile(HttpProfile::Safari);
        assert_eq!(settings.header_table_size, 16384);
        assert_eq!(settings.max_concurrent_streams, Some(500));
        assert_eq!(settings.initial_window_size, 65535);
    }

    #[test]
    fn test_h2_settings_edge_profile() {
        let settings = H2Settings::for_profile(HttpProfile::Edge);
        // Edge matches Chrome
        assert_eq!(settings.header_table_size, 65536);
        assert_eq!(settings.max_concurrent_streams, Some(1000));
        assert_eq!(settings.initial_window_size, 6291456);
    }

    #[test]
    fn test_h2_settings_wire_format_non_empty() {
        let settings = H2Settings::for_profile(HttpProfile::Chrome);
        let wire = settings.to_wire_format();
        assert!(!wire.is_empty());
        // Should contain at least: HEADER_TABLE_SIZE + ENABLE_PUSH + MAX_CONCURRENT + WINDOW_SIZE + MAX_FRAME
        assert!(wire.len() >= 6 * 5);
    }

    #[test]
    fn test_h2_priority_default() {
        let priority = H2StreamPriority::default_for_profile(HttpProfile::Chrome);
        assert_eq!(priority.weight, 16);
        assert_eq!(priority.depends_on, 0);
        assert!(!priority.exclusive);
    }

    #[test]
    fn test_h2_priority_wire_format() {
        let priority = H2StreamPriority {
            weight: 16,
            depends_on: 0,
            exclusive: false,
        };
        let wire = priority.to_wire_format();
        assert_eq!(wire.len(), 5);
    }
}

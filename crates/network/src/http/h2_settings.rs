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
    /// SETTINGS_MAX_FRAME_SIZE (default 16384). `None` — не отправлять:
    /// Chrome 157 его не шлёт (замер tls.peet.ws/api/all, BUG-1826).
    pub max_frame_size: Option<u32>,
    /// SETTINGS_MAX_HEADER_LIST_SIZE (id 6). Chrome 157 шлёт 262144;
    /// `None` — не отправлять.
    pub max_header_list_size: Option<u32>,
    /// SETTINGS_HEADER_COMPRESSION_SIZE_LIMIT (HTTP/2 extension, optional).
    pub header_compression_size_limit: Option<u32>,
    /// Increment of the stream-0 WINDOW_UPDATE sent right after the preface
    /// SETTINGS, raising the connection receive window above the 65 535
    /// default (RFC 9113 §6.9.2). Browsers send it, and its value is part of
    /// the HTTP/2 fingerprint. Without it every stream of a multiplexed
    /// connection shares 64 KiB per round trip (PERF-13).
    pub connection_window_increment: u32,
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
                // Chrome 157+ HTTP/2 SETTINGS — значения живут в
                // `chrome_manifest::CHROME_157` (FINGERPRINT-MANIFEST);
                // измерено tls.peet.ws/api/all (akamai `1:65536;2:0;4:6291456;6:262144`),
                // BUG-1826: Chrome больше не шлёт ENABLE_PUSH=1,
                // MAX_CONCURRENT_STREAMS и MAX_FRAME_SIZE, зато добавил
                // MAX_HEADER_LIST_SIZE=256 KiB.
                let m = &super::chrome_manifest::CHROME_157.h2;
                Self {
                    header_table_size: m.header_table_size,
                    enable_push: m.enable_push,
                    max_concurrent_streams: m.max_concurrent_streams,
                    initial_window_size: m.initial_window_size,
                    max_frame_size: m.max_frame_size,
                    max_header_list_size: m.max_header_list_size,
                    header_compression_size_limit: None,
                    connection_window_increment: m.connection_window_increment,
                }
            }
            HttpProfile::Firefox => {
                // Firefox 130+ HTTP/2 SETTINGS
                Self {
                    header_table_size: 65536,
                    enable_push: true,
                    max_concurrent_streams: Some(1000),
                    initial_window_size: 2147483647,  // Very large window (max i32)
                    max_frame_size: Some(16384),
                    max_header_list_size: None,
                    header_compression_size_limit: None,
                    connection_window_increment: 12_517_377,  // 12 MB connection window
                }
            }
            HttpProfile::Safari => {
                // Safari 18+ HTTP/2 SETTINGS (conservative)
                Self {
                    header_table_size: 16384,
                    enable_push: true,
                    max_concurrent_streams: Some(500),
                    initial_window_size: 65535,    // RFC default
                    max_frame_size: Some(16384),
                    max_header_list_size: None,
                    header_compression_size_limit: None,
                    connection_window_increment: 10_420_225,  // 10 MB connection window
                }
            }
            HttpProfile::Edge => {
                // Edge — тот же Chromium-стек: читает тот же манифест Chrome 157.
                H2Settings::for_profile(HttpProfile::Chrome)
            }
            HttpProfile::TorBrowser => {
                // Tor Browser HTTP/2 SETTINGS (conservative to avoid unique fingerprint)
                Self {
                    header_table_size: 4096,        // RFC default
                    enable_push: true,
                    max_concurrent_streams: Some(100),  // Conservative
                    initial_window_size: 65535,    // RFC default
                    max_frame_size: Some(16384),   // RFC default
                    max_header_list_size: None,
                    header_compression_size_limit: None,
                    connection_window_increment: 12_517_377,  // 12 MB connection window
                }
            }
            HttpProfile::Lumen => {
                // Lumen-native HTTP/2 SETTINGS (optimized for lightweight browser)
                Self {
                    header_table_size: 16384,
                    enable_push: true,
                    max_concurrent_streams: Some(500),
                    initial_window_size: 1048576,   // 1 MB — RAM optimization
                    max_frame_size: Some(16384),
                    max_header_list_size: None,
                    header_compression_size_limit: None,
                    connection_window_increment: 15_663_105,  // 15 MB connection window
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

        // SETTINGS_MAX_FRAME_SIZE (0x0005, optional — Chrome его не шлёт)
        if let Some(max_frame) = self.max_frame_size {
            buf.extend_from_slice(&0x0005u16.to_be_bytes());
            buf.extend_from_slice(&max_frame.to_be_bytes());
        }

        // SETTINGS_MAX_HEADER_LIST_SIZE (0x0006, optional)
        if let Some(limit) = self.max_header_list_size {
            buf.extend_from_slice(&0x0006u16.to_be_bytes());
            buf.extend_from_slice(&limit.to_be_bytes());
        }

        // SETTINGS_HEADER_COMPRESSION_SIZE_LIMIT (HTTP/2 extension, optional)
        if let Some(limit) = self.header_compression_size_limit {
            buf.extend_from_slice(&u16::MAX.to_be_bytes());
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
        assert!(!settings.enable_push);
        assert_eq!(settings.max_concurrent_streams, None);
        assert_eq!(settings.initial_window_size, 6291456);
        assert_eq!(settings.max_header_list_size, Some(262144));
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
        assert!(!settings.enable_push);
        assert_eq!(settings.max_concurrent_streams, None);
        assert_eq!(settings.initial_window_size, 6291456);
        assert_eq!(settings.max_header_list_size, Some(262144));
    }

    #[test]
    fn test_h2_settings_wire_format_non_empty() {
        let settings = H2Settings::for_profile(HttpProfile::Chrome);
        let wire = settings.to_wire_format();
        assert!(!wire.is_empty());
        // Chrome шлёт 4 SETTINGS: HEADER_TABLE_SIZE + ENABLE_PUSH + INITIAL_WINDOW + MAX_HEADER_LIST_SIZE
        assert_eq!(wire.len(), 6 * 4);
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

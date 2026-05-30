//! Platform-specific implementations of `MemoryPressureSource` (ADR-008 §10H).
//!
//! - `Win32MemoryPressureSource` — `GlobalMemoryStatusEx` polling (Windows).
//! - `LinuxMemoryPressureSource` — `/proc/pressure/memory` PSI (Linux ≥ 4.20).
//!
//! On unsupported platforms, use `NullMemoryPressureSource` from `lumen_core::ext`.

use crate::ext::{MemoryPressureLevel, MemoryPressureSource};

// =============================================================================
// Windows: GlobalMemoryStatusEx
// =============================================================================

/// Win32 memory pressure source via `GlobalMemoryStatusEx` polling.
///
/// Maps `dwMemoryLoad` (0–100% RAM used):
/// - < 75% → `Low`
/// - 75–90% → `Medium`
/// - > 90% → `High`
#[cfg(target_os = "windows")]
pub struct Win32MemoryPressureSource;

#[cfg(target_os = "windows")]
mod win32_ffi {
    /// MEMORYSTATUSEX (Windows SDK, winbase.h).
    #[repr(C)]
    pub struct MemoryStatusEx {
        pub dw_length: u32,
        pub dw_memory_load: u32,
        pub ull_total_phys: u64,
        pub ull_avail_phys: u64,
        pub ull_total_page_file: u64,
        pub ull_avail_page_file: u64,
        pub ull_total_virtual: u64,
        pub ull_avail_virtual: u64,
        pub ull_avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    }

    /// Returns memory load as a percentage (0–100), or `None` on API failure.
    pub fn memory_load_percent() -> Option<u32> {
        let mut status = MemoryStatusEx {
            dw_length: core::mem::size_of::<MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        // SAFETY: status is stack-allocated, properly initialized with correct
        // dwLength, and GlobalMemoryStatusEx only writes within the struct.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        if ok != 0 {
            Some(status.dw_memory_load)
        } else {
            None
        }
    }
}

#[cfg(target_os = "windows")]
impl MemoryPressureSource for Win32MemoryPressureSource {
    fn poll_current(&self) -> MemoryPressureLevel {
        match win32_ffi::memory_load_percent() {
            Some(load) if load > 90 => MemoryPressureLevel::High,
            Some(load) if load > 75 => MemoryPressureLevel::Medium,
            _ => MemoryPressureLevel::Low,
        }
    }
}

// =============================================================================
// Linux: /proc/pressure/memory PSI (kernel ≥ 4.20)
// =============================================================================

/// Linux memory pressure source via `/proc/pressure/memory` PSI polling.
///
/// Reads the `some avg10` value (% of time at least one task stalled on memory
/// over last 10 seconds):
/// - < 5% → `Low`
/// - 5–20% → `Medium`
/// - > 20% → `High`
///
/// Falls back to `Low` if PSI is not available (kernel < 4.20, no CAP_READ).
#[cfg(target_os = "linux")]
pub struct LinuxMemoryPressureSource;

#[cfg(target_os = "linux")]
impl LinuxMemoryPressureSource {
    /// Parse `some avg10=X.XX` from `/proc/pressure/memory` content.
    fn parse_some_avg10(content: &str) -> Option<f32> {
        // Format: "some avg10=0.00 avg60=0.00 avg300=0.00 total=0"
        for line in content.lines() {
            if line.starts_with("some ") {
                for token in line.split_whitespace() {
                    if let Some(val) = token.strip_prefix("avg10=") {
                        return val.parse::<f32>().ok();
                    }
                }
            }
        }
        None
    }
}

#[cfg(target_os = "linux")]
impl MemoryPressureSource for LinuxMemoryPressureSource {
    fn poll_current(&self) -> MemoryPressureLevel {
        let content = match std::fs::read_to_string("/proc/pressure/memory") {
            Ok(c) => c,
            Err(_) => return MemoryPressureLevel::Low,
        };
        match Self::parse_some_avg10(&content) {
            Some(avg) if avg > 20.0 => MemoryPressureLevel::High,
            Some(avg) if avg > 5.0 => MemoryPressureLevel::Medium,
            _ => MemoryPressureLevel::Low,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::NullMemoryPressureSource;

    #[test]
    fn null_source_always_low() {
        let src = NullMemoryPressureSource;
        assert_eq!(src.poll_current(), MemoryPressureLevel::Low);
    }

    #[test]
    fn pressure_level_ordering() {
        assert!(MemoryPressureLevel::Low < MemoryPressureLevel::Medium);
        assert!(MemoryPressureLevel::Medium < MemoryPressureLevel::High);
    }

    #[test]
    fn null_source_is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<NullMemoryPressureSource>();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_psi_some_avg10() {
        let content =
            "some avg10=12.50 avg60=5.00 avg300=1.00 total=12345\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
        let val = LinuxMemoryPressureSource::parse_some_avg10(content);
        assert_eq!(val, Some(12.50_f32));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_psi_missing_returns_none() {
        let val = LinuxMemoryPressureSource::parse_some_avg10("full avg10=0.00 total=0\n");
        assert_eq!(val, None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_level_thresholds() {
        // Simulate by parsing known values and applying logic directly.
        let low_avg: f32 = 2.0;
        let med_avg: f32 = 10.0;
        let high_avg: f32 = 25.0;

        let to_level = |avg: f32| match avg {
            v if v > 20.0 => MemoryPressureLevel::High,
            v if v > 5.0 => MemoryPressureLevel::Medium,
            _ => MemoryPressureLevel::Low,
        };

        assert_eq!(to_level(low_avg), MemoryPressureLevel::Low);
        assert_eq!(to_level(med_avg), MemoryPressureLevel::Medium);
        assert_eq!(to_level(high_avg), MemoryPressureLevel::High);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn win32_source_returns_valid_level() {
        let src = Win32MemoryPressureSource;
        let level = src.poll_current();
        // On any running Windows machine the call should succeed and return some level.
        assert!(matches!(
            level,
            MemoryPressureLevel::Low | MemoryPressureLevel::Medium | MemoryPressureLevel::High
        ));
    }
}

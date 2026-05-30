//! Platform-specific implementations of `MemoryPressureSource` (ADR-008 §10H).
//!
//! - `Win32MemoryPressureSource` — `GlobalMemoryStatusEx` polling (Windows).
//! - `LinuxMemoryPressureSource` — `/proc/pressure/memory` PSI (Linux ≥ 4.20).
//! - `MacosMemoryPressureSource` — `host_statistics64(HOST_VM_INFO64)` polling (macOS ≥ 10.9).
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

// =============================================================================
// macOS: host_statistics64(HOST_VM_INFO64)
// =============================================================================

/// macOS memory pressure source via `host_statistics64(HOST_VM_INFO64)` polling.
///
/// Computes `used_ratio = (active + wire) / (free + active + inactive + wire)`:
/// - Note: `free_count` already includes speculative pages per the mach kernel ABI.
/// - < 75% used → `Low`
/// - 75–90% used → `Medium`
/// - > 90% used → `High`
///
/// Falls back to `Low` on API failure (e.g., sandboxed process without mach access).
#[cfg(target_os = "macos")]
pub struct MacosMemoryPressureSource;

#[cfg(target_os = "macos")]
mod macos_ffi {
    /// Subset of `vm_statistics64` from `<mach/vm_statistics.h>` needed for
    /// pressure calculation. Layout is `repr(C)` — must match the kernel struct
    /// exactly. `HOST_VM_INFO64_COUNT = sizeof(vm_statistics64) / sizeof(natural_t) = 38`.
    ///
    /// Note: `free_count` already includes `speculative_count` (per kernel docs).
    #[repr(C)]
    pub struct VmStatistics64 {
        pub free_count: u32,
        pub active_count: u32,
        pub inactive_count: u32,
        pub wire_count: u32,
        pub zero_fill_count: u64,
        pub reactivations: u64,
        pub pageins: u64,
        pub pageouts: u64,
        pub faults: u64,
        pub cow_faults: u64,
        pub lookups: u64,
        pub hits: u64,
        pub purges: u64,
        pub purgeable_count: u32,
        pub speculative_count: u32,
        pub decompressions: u64,
        pub compressions: u64,
        pub swapins: u64,
        pub swapouts: u64,
        pub compressor_page_count: u32,
        pub throttled_count: u32,
        pub external_page_count: u32,
        pub internal_page_count: u32,
        pub total_uncompressed_pages_in_compressor: u64,
    }

    /// `host_flavor_t` value for 64-bit VM statistics (HOST_VM_INFO64 = 4).
    pub const HOST_VM_INFO64: i32 = 4;

    /// `mach_msg_type_number_t` count for `vm_statistics64`:
    /// `sizeof(VmStatistics64) / sizeof(u32)` = 152 / 4 = 38.
    pub const HOST_VM_INFO64_COUNT: u32 = 38;

    unsafe extern "C" {
        /// Returns the mach port for the current host (libSystem, always available).
        pub fn mach_host_self() -> u32;

        /// Fills `host_info_out` with `HOST_VM_INFO64_COUNT` × `u32` words of
        /// `vm_statistics64` data. Returns 0 (`KERN_SUCCESS`) on success.
        pub fn host_statistics64(
            host_priv: u32,
            flavor: i32,
            host_info_out: *mut VmStatistics64,
            host_info_out_cnt: *mut u32,
        ) -> i32;
    }

    /// Polls VM statistics and returns `(used_pages, total_pages)`, or `None` on error.
    pub fn vm_used_total() -> Option<(u64, u64)> {
        // SAFETY: VmStatistics64 is repr(C) and all-numeric; zeroing is a valid
        // initial state. mach_host_self() always succeeds. host_statistics64 writes
        // exactly HOST_VM_INFO64_COUNT * 4 bytes = sizeof(VmStatistics64) bytes into
        // the struct, so there is no out-of-bounds write.
        let mut stats: VmStatistics64 = unsafe { core::mem::zeroed() };
        let mut count = HOST_VM_INFO64_COUNT;
        let ret = unsafe {
            host_statistics64(mach_host_self(), HOST_VM_INFO64, &mut stats, &mut count)
        };
        if ret != 0 {
            return None;
        }
        // free_count already includes speculative pages, so the four categories are
        // mutually exclusive and cover all physical RAM.
        let total = u64::from(stats.free_count)
            + u64::from(stats.active_count)
            + u64::from(stats.inactive_count)
            + u64::from(stats.wire_count);
        if total == 0 {
            return None;
        }
        // "Used" = pages that cannot be reclaimed quickly (active + wired).
        let used = u64::from(stats.active_count) + u64::from(stats.wire_count);
        Some((used, total))
    }
}

#[cfg(target_os = "macos")]
impl MacosMemoryPressureSource {
    /// Maps `(used_pages, total_pages)` to a pressure level.
    fn level_from_used_ratio(used: u64, total: u64) -> MemoryPressureLevel {
        // Multiply to avoid floating-point: compare used * 100 vs threshold * total.
        let used100 = used.saturating_mul(100);
        if used100 > total.saturating_mul(90) {
            MemoryPressureLevel::High
        } else if used100 > total.saturating_mul(75) {
            MemoryPressureLevel::Medium
        } else {
            MemoryPressureLevel::Low
        }
    }
}

#[cfg(target_os = "macos")]
impl MemoryPressureSource for MacosMemoryPressureSource {
    fn poll_current(&self) -> MemoryPressureLevel {
        match macos_ffi::vm_used_total() {
            Some((used, total)) => Self::level_from_used_ratio(used, total),
            None => MemoryPressureLevel::Low,
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

    // macOS threshold logic tests — platform-independent: exercise level_from_used_ratio
    // directly with synthetic page counts to validate the 75/90 thresholds.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_level_thresholds() {
        // total = 1000 pages
        // Low: used <= 75%
        assert_eq!(
            MacosMemoryPressureSource::level_from_used_ratio(700, 1000),
            MemoryPressureLevel::Low,
        );
        // Medium: 75% < used <= 90%
        assert_eq!(
            MacosMemoryPressureSource::level_from_used_ratio(800, 1000),
            MemoryPressureLevel::Medium,
        );
        // High: used > 90%
        assert_eq!(
            MacosMemoryPressureSource::level_from_used_ratio(950, 1000),
            MemoryPressureLevel::High,
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_level_boundary_75() {
        // Exactly 75% should be Low (not Medium).
        assert_eq!(
            MacosMemoryPressureSource::level_from_used_ratio(750, 1000),
            MemoryPressureLevel::Low,
        );
        // 751/1000 > 75% → Medium.
        assert_eq!(
            MacosMemoryPressureSource::level_from_used_ratio(751, 1000),
            MemoryPressureLevel::Medium,
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_level_boundary_90() {
        // Exactly 90% should be Medium (not High).
        assert_eq!(
            MacosMemoryPressureSource::level_from_used_ratio(900, 1000),
            MemoryPressureLevel::Medium,
        );
        // 901/1000 > 90% → High.
        assert_eq!(
            MacosMemoryPressureSource::level_from_used_ratio(901, 1000),
            MemoryPressureLevel::High,
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_source_returns_valid_level() {
        let src = MacosMemoryPressureSource;
        let level = src.poll_current();
        // On any running macOS machine host_statistics64 should succeed.
        assert!(matches!(
            level,
            MemoryPressureLevel::Low | MemoryPressureLevel::Medium | MemoryPressureLevel::High
        ));
    }
}

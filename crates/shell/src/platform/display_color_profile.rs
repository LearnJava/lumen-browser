//! Platform display-color-profile provider for `ph3-color-management` Step 1.
//!
//! [`PlatformDisplayColorProfile`] implements [`DisplayColorProfile`] from
//! `lumen-core::ext`, querying the active monitor's ICC profile.
//!
//! - **Windows** — `GetICMProfileA` (gdi32). Falls back to `ColorSpace::Srgb`
//!   on any error.
//! - **Linux / macOS / other** — always `ColorSpace::Srgb` (no OS query yet).

// ── Platform dispatch ──────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use windows_impl::PlatformDisplayColorProfile;

#[cfg(not(target_os = "windows"))]
pub use null_impl::PlatformDisplayColorProfile;

// ── Non-Windows implementation ─────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
mod null_impl {
    use lumen_core::ext::DisplayColorProfile;
    use lumen_core::ColorSpace;

    /// Linux / macOS display-color-profile provider: no OS ICC query yet,
    /// always reports `ColorSpace::Srgb` (same semantics as
    /// `NullDisplayColorProfile`).
    #[derive(Debug, Clone, Default)]
    pub struct PlatformDisplayColorProfile;

    impl PlatformDisplayColorProfile {
        /// Create the provider. No OS resources are acquired.
        pub fn new() -> Self {
            Self
        }
    }

    impl DisplayColorProfile for PlatformDisplayColorProfile {
        fn active_profile(&self) -> ColorSpace {
            ColorSpace::Srgb
        }
    }
}

// ── Windows implementation ─────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows_impl {
    use lumen_core::ext::DisplayColorProfile;
    use lumen_core::ColorSpace;
    use std::ffi::c_void;
    use std::sync::OnceLock;

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetICMProfileA(hdc: *mut c_void, lpcstr: *mut i8, pcb: *mut u32) -> i32;
        fn CreateDCA(
            pDriver: *const i8,
            pDevice: *const i8,
            pOutput: *const i8,
            pInitData: *const i8,
        ) -> *mut c_void;
        fn DeleteDC(hdc: *mut c_void) -> i32;
    }

    /// Query primary display ICC profile path via GDI `GetICMProfileA`.
    fn query_gdi_icc_profile() -> Option<ColorSpace> {
        unsafe {
            let hdc = CreateDCA(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if hdc.is_null() {
                return None;
            }

            let mut buf_size: u32 = 0;
            GetICMProfileA(hdc, std::ptr::null_mut(), &mut buf_size);
            if buf_size == 0 {
                let _ = DeleteDC(hdc);
                return None;
            }

            let mut buf = vec![0u8; buf_size as usize];
            let ok = GetICMProfileA(hdc, buf.as_mut_ptr() as *mut i8, &mut buf_size);
            let _ = DeleteDC(hdc);

            if ok == 0 {
                return None;
            }

            let profile_path =
                String::from_utf8_lossy(&buf[..buf_size as usize - 1]).to_lowercase();

            let cs = if profile_path.contains("display-p3") || profile_path.contains("displayp3") {
                ColorSpace::DisplayP3
            } else if profile_path.contains("rec2020") || profile_path.contains("rec.2020") {
                ColorSpace::Rec2020
            } else {
                ColorSpace::Srgb
            };
            Some(cs)
        }
    }

    /// Windows display-color-profile provider via GDI `GetICMProfile`.
    ///
    /// Queries the primary display's ICC profile path once and caches the result
    /// via `OnceLock`. All subsequent calls return the cached value without
    /// re-querying GDI.
    #[derive(Debug, Clone, Default)]
    pub struct PlatformDisplayColorProfile {
        cached: OnceLock<ColorSpace>,
    }

    impl PlatformDisplayColorProfile {
        pub fn new() -> Self {
            Self { cached: OnceLock::new() }
        }
    }

    impl DisplayColorProfile for PlatformDisplayColorProfile {
        fn active_profile(&self) -> ColorSpace {
            *self.cached.get_or_init(|| query_gdi_icc_profile().unwrap_or(ColorSpace::Srgb))
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use lumen_core::ext::{DisplayColorProfile, NullDisplayColorProfile};
    use lumen_core::ColorSpace;

    #[test]
    fn null_returns_srgb() {
        let p = NullDisplayColorProfile;
        assert_eq!(p.active_profile(), ColorSpace::Srgb);
    }
}

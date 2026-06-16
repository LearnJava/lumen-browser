//! Platform screen capture backend for `navigator.mediaDevices.getDisplayMedia` (PH3-17).
//!
//! [`PlatformScreenCapture`] implements [`ScreenCaptureProvider`] via Win32 GDI
//! `BitBlt` on Windows.  On Linux/macOS Phase 1 it is a type alias for
//! [`NullScreenCaptureProvider`] (no-op).
//!
//! ## Windows capture mechanism
//!
//! Phase 1 uses GDI `BitBlt` to copy the primary monitor into a memory DC, then
//! reads BGRA pixels via `GetDIBits` and swaps B↔R to produce RGBA.
//!
//! Limitation: GDI BitBlt misses hardware-composited content (some games, DRM
//! video) on DWM-era Windows.  DXGI Desktop Duplication API is more reliable and
//! is scheduled for Phase 2.

// BUG-169: imports consumed only by the Windows GDI impl below; on
// Linux/macOS the stub re-exports NullScreenCaptureProvider, so they read as
// unused there. `allow` is no-op on Windows and silences the cross-platform
// false positive without risking the Windows build via cfg-gating.
#[allow(unused_imports)]
use lumen_core::ext::{
    ScreenCaptureConfig, ScreenCaptureError, ScreenCaptureHandle,
    ScreenCaptureProvider, ScreenSourceDescriptor, VideoFrame,
};

// ── Platform dispatch ─────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use windows_impl::PlatformScreenCapture;

/// Platform screen capture provider.
///
/// Windows: GDI BitBlt capture of the primary monitor.
/// Linux/macOS: no-op stub — Phase 2 will add X11/Wayland/CGDisplay support.
#[cfg(not(target_os = "windows"))]
pub use lumen_core::ext::NullScreenCaptureProvider as PlatformScreenCapture;

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use std::ffi::c_void;

    // Win32 constants
    const SM_CXSCREEN: i32 = 0;
    const SM_CYSCREEN: i32 = 1;
    const SRCCOPY: u32 = 0x00CC_0020;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;

    // BITMAPINFOHEADER (wingdi.h)
    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    // BITMAPINFO (wingdi.h) — minimal single-entry color table
    #[repr(C)]
    struct BitmapInfo {
        bmi_header: BitmapInfoHeader,
        bmi_colors: [u32; 1],
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetSystemMetrics(n_index: i32) -> i32;
        fn GetDC(h_wnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(h_wnd: *mut c_void, h_dc: *mut c_void) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(h_dc: *mut c_void) -> *mut c_void;
        fn CreateCompatibleBitmap(h_dc: *mut c_void, cx: i32, cy: i32) -> *mut c_void;
        fn SelectObject(h_dc: *mut c_void, h: *mut c_void) -> *mut c_void;
        fn BitBlt(
            h_dc: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            h_dc_src: *mut c_void,
            x1: i32,
            y1: i32,
            rop: u32,
        ) -> i32;
        fn GetDIBits(
            h_dc: *mut c_void,
            h_bm: *mut c_void,
            start: u32,
            c_lines: u32,
            lp_vbits: *mut c_void,
            lp_bmi: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn DeleteObject(ho: *mut c_void) -> i32;
        fn DeleteDC(h_dc: *mut c_void) -> i32;
    }

    /// Platform screen capture provider using Win32 GDI BitBlt.
    ///
    /// Stateless — each `capture()` opens a new session that captures frames on demand.
    pub struct PlatformScreenCapture;

    impl ScreenCaptureProvider for PlatformScreenCapture {
        fn enumerate_sources(&self) -> Vec<ScreenSourceDescriptor> {
            // SAFETY: GetSystemMetrics is a pure OS query with no memory hazards.
            let (w, h) = unsafe {
                (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
            };
            if w <= 0 || h <= 0 {
                return Vec::new();
            }
            vec![ScreenSourceDescriptor {
                source_id: "screen-0".to_owned(),
                label: "Entire Screen".to_owned(),
                kind: "monitor",
                width: w as u32,
                height: h as u32,
            }]
        }

        fn capture(
            &self,
            _config: ScreenCaptureConfig,
        ) -> Result<Box<dyn ScreenCaptureHandle>, ScreenCaptureError> {
            // SAFETY: GetSystemMetrics is a pure OS query.
            let (w, h) = unsafe {
                (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
            };
            if w <= 0 || h <= 0 {
                return Err(ScreenCaptureError::Other(
                    "Cannot determine primary monitor dimensions".into(),
                ));
            }
            Ok(Box::new(GdiCaptureHandle {
                width: w as u32,
                height: h as u32,
                stopped: false,
            }))
        }
    }

    /// Active GDI capture session.  Captures a frame on each `read_frame()` call.
    struct GdiCaptureHandle {
        width: u32,
        height: u32,
        stopped: bool,
    }

    impl ScreenCaptureHandle for GdiCaptureHandle {
        fn width(&self) -> u32 { self.width }
        fn height(&self) -> u32 { self.height }
        fn source_id(&self) -> &str { "screen-0" }
        fn label(&self) -> &str { "Entire Screen" }

        fn read_frame(&mut self) -> Option<VideoFrame> {
            if self.stopped {
                return None;
            }
            capture_gdi(self.width, self.height)
        }

        fn stop(&mut self) {
            self.stopped = true;
        }
    }

    /// Capture the primary monitor via GDI BitBlt, returning RGBA pixels.
    fn capture_gdi(width: u32, height: u32) -> Option<VideoFrame> {
        let w = width as i32;
        let h = height as i32;

        // SAFETY: All Win32 GDI calls follow documented API contracts.
        // We validate each handle and release all resources before returning.
        unsafe {
            let screen_dc = GetDC(std::ptr::null_mut());
            if screen_dc.is_null() {
                return None;
            }

            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.is_null() {
                ReleaseDC(std::ptr::null_mut(), screen_dc);
                return None;
            }

            let bitmap = CreateCompatibleBitmap(screen_dc, w, h);
            if bitmap.is_null() {
                DeleteDC(mem_dc);
                ReleaseDC(std::ptr::null_mut(), screen_dc);
                return None;
            }

            let old_obj = SelectObject(mem_dc, bitmap);
            let blt_ok = BitBlt(mem_dc, 0, 0, w, h, screen_dc, 0, 0, SRCCOPY);

            let mut pixels = vec![0u8; (width * height * 4) as usize];

            let got_bits = if blt_ok != 0 {
                let mut bmi = BitmapInfo {
                    bmi_header: BitmapInfoHeader {
                        bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                        bi_width: w,
                        // Negative height → top-down DIB (row 0 = top of screen).
                        bi_height: -h,
                        bi_planes: 1,
                        bi_bit_count: 32,
                        bi_compression: BI_RGB,
                        bi_size_image: 0,
                        bi_x_pels_per_meter: 0,
                        bi_y_pels_per_meter: 0,
                        bi_clr_used: 0,
                        bi_clr_important: 0,
                    },
                    bmi_colors: [0],
                };
                GetDIBits(
                    mem_dc,
                    bitmap,
                    0,
                    h as u32,
                    pixels.as_mut_ptr().cast::<c_void>(),
                    &raw mut bmi,
                    DIB_RGB_COLORS,
                ) > 0
            } else {
                false
            };

            // Release all GDI resources.
            SelectObject(mem_dc, old_obj);
            DeleteObject(bitmap);
            DeleteDC(mem_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);

            if !got_bits {
                return None;
            }

            // GDI returns BGRA; swap B↔R to produce RGBA.
            for chunk in pixels.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }

            Some(VideoFrame { width, height, data: pixels })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn enumerate_returns_some_or_empty() {
            let p = PlatformScreenCapture;
            // On headless CI GetSystemMetrics may return 0 — both outcomes acceptable.
            let srcs = p.enumerate_sources();
            for s in &srcs {
                assert!(!s.source_id.is_empty());
                assert_eq!(s.kind, "monitor");
                assert!(s.width > 0 && s.height > 0);
            }
        }

        #[test]
        fn capture_or_error() {
            let p = PlatformScreenCapture;
            match p.capture(ScreenCaptureConfig::default()) {
                Ok(mut h) => {
                    assert!(h.width() > 0);
                    assert!(h.height() > 0);
                    assert_eq!(h.source_id(), "screen-0");
                    // read_frame may return None in CI (no display), both outcomes ok.
                    let _ = h.read_frame();
                    h.stop();
                    assert!(h.read_frame().is_none(), "read_frame after stop must be None");
                }
                Err(ScreenCaptureError::Other(_)) => {
                    // No display available in CI — acceptable.
                }
                Err(e) => panic!("unexpected error: {e:?}"),
            }
        }
    }
}

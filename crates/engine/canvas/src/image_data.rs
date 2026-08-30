//! `getImageData` — the rectangle-cropping half of the canvas ImageData API.
//!
//! Lives beside `lib.rs` rather than in it because that file is at the 2 000-line
//! ceiling (`docs/lint-policy.md` §5.1); as a child module of the crate root it
//! still reaches `Context2D`'s private pixel buffer, so the crop is a plain
//! inherent method.
//!
//! The writing half (`put_image_data`, `create_image_data`) stays in `lib.rs`
//! next to the rasterizer state it mutates.

use crate::Context2D;

impl Context2D {
    /// `getImageData(sx, sy, sw, sh)` — RGBA8 copy of the requested rectangle.
    ///
    /// The rectangle may lie partly or wholly outside the canvas; that part
    /// comes back transparent black, as the spec's "get image data" steps ask.
    /// Cropping here rather than on the JS side is what makes a one-pixel probe
    /// cost four bytes over the binding instead of the whole bitmap (BUG-448).
    /// An area too large to allocate (the arguments are `long`s) answers empty
    /// — the binding reports that to JS rather than aborting the process.
    /// Noise, when enabled, is applied to the crop: it walks whatever buffer it
    /// is handed, so per-call determinism is unchanged.
    pub fn get_image_data_rect(&self, sx: i32, sy: i32, sw: u32, sh: u32) -> Vec<u8> {
        let len = match (sw as usize).checked_mul(sh as usize).map(|px| px * 4) {
            Some(n) if n <= 256 * 1024 * 1024 => n,
            _ => return Vec::new(),
        };
        let mut out = vec![0u8; len];
        let (cw, ch) = (i64::from(self.width), i64::from(self.height));
        for row in 0..sh {
            let src_y = i64::from(sy) + i64::from(row);
            if src_y < 0 || src_y >= ch {
                continue;
            }
            for col in 0..sw {
                let src_x = i64::from(sx) + i64::from(col);
                if src_x < 0 || src_x >= cw {
                    continue;
                }
                let si = ((src_y * cw + src_x) * 4) as usize;
                let di = ((row as usize) * (sw as usize) + col as usize) * 4;
                out[di..di + 4].copy_from_slice(&self.pixels[si..si + 4]);
            }
        }
        // BUG-454: the rectangle's canvas-space origin goes with it, so the pixel at
        // (sx+col, sy+row) is perturbed the same way whatever rectangle asked for it.
        if let Some(noise_gen) = self.noise_generator {
            noise_gen.apply_noise_to_rect(&mut out, sx, sy, sw, sh);
        }
        out
    }
}

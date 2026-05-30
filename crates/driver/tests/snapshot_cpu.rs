//! Deterministic CPU snapshot comparison — task 8A.6 level-3.
//!
//! Renders selected `graphic_tests` pages through the tiny-skia CPU path
//! (`InProcessSession::screenshot_cpu_rgba`, feature `cpu-render`) and compares
//! the result pixel-for-pixel against committed reference PNGs under
//! `graphic_tests/snapshots/cpu/`. tiny-skia is a pure-Rust software rasterizer,
//! so its output is identical across Windows/macOS/Linux — unlike the GPU
//! `screenshot()` path, which varies with the graphics driver. This is the
//! cross-OS-stable regression gate the 8A.6 migration targets.
//!
//! The CPU rasterizer currently covers the geometric primitives
//! (`FillRect` / `FillRoundedRect` / `DrawBorder` / `DrawOutline`), linear,
//! radial and conic gradients (`DrawLinearGradient` / `DrawRadialGradient` /
//! `DrawConicGradient`, all including repeating), tessellated SVG paths
//! (`DrawSvgPath`), rectangular clipping (`PushClipRect` / `PopClip` +
//! `PushScrollLayer` / `PopScrollLayer`, i.e. `overflow: hidden/scroll/auto`),
//! the `<img>` grey placeholder quad (`DrawImage` — the headless CPU path
//! registers no decoded pixels, so every image box paints the solid
//! placeholder, matching the GPU renderer's fallback), and text (`DrawText` —
//! glyphs of the bundled Inter Regular face rasterized via `lumen_font::
//! Rasterizer` and composited through a coverage `Mask`; page
//! `55-text-rendering`), group opacity (`PushOpacity` / `PopOpacity` —
//! the subtree is rendered into an off-screen layer and alpha-blended as a
//! unit; page `13-visibility-opacity`), and 2D transforms (`PushTransform` /
//! `PopTransform` — the subtree renders into an off-screen layer in page
//! coordinates, then composites down through the box's affine via `draw_pixmap`;
//! translate / rotate / scale / skew / matrix2d, page `22-transform`). The chosen
//! pages exercise exactly these primitives, so the references capture meaningful
//! geometry rather than blank frames. As `cpu_raster` grows, add the relevant
//! pages to `PAGES`.
//!
//! Run:        cargo test -p lumen-driver --features cpu-render
//! Regenerate: SAVE_CPU_SNAPSHOTS=1 cargo test -p lumen-driver --features cpu-render -- --nocapture
//!
//! The whole file is gated on the feature; a plain `cargo test -p lumen-driver`
//! compiles it to nothing.
#![cfg(feature = "cpu-render")]

use lumen_driver::{BrowserSession, InProcessSession};
use std::path::{Path, PathBuf};

/// Pages that exercise the CPU primitives (rect / rounded-rect / border /
/// outline / linear+radial+conic gradient / SVG path / clip / image
/// placeholder / text / opacity / transform / blend / filter). Each name is the
/// `graphic_tests/<name>.html` stem and the
/// `graphic_tests/snapshots/cpu/<name>.png` reference stem.
///
/// Every page here was verified to render meaningful geometry through the CPU
/// path (≥2% non-background pixels), so each reference captures real layout
/// output rather than a blank frame. Pages whose *meaning* depends on a still
/// unimplemented primitive are deliberately excluded until it lands. `15-box-shadow`
/// and `52-text-shadow-blur` exercise the `PushFilter`/`PopFilter` Gaussian blur
/// (the deterministic three-box-blur approximation); `52` carries text so, like
/// `55`, it is a snapshot-only page not registered in `run.py`.
/// `18-images` is included because all its `<img>` boxes carry empty `alt`
/// and explicit `width`/`height`, so the grey placeholder fully reproduces the
/// (text-free) GPU headless output. `55-text-rendering` exercises the `DrawText`
/// primitive (bundled Inter glyphs); it is a snapshot-only page, not registered
/// in `run.py`, because glyph anti-aliasing always diverges from Edge.
const PAGES: &[&str] = &[
    "00-calibration",
    "01-sanity",
    "02-color-named",
    "03-color-formats",
    "04-color-alpha",
    "05-border-width",
    "06-border-sides",
    "07-box-sizing",
    "08-padding",
    "09-margin",
    "10-min-max-width",
    "11-min-max-height",
    "12-display",
    "13-visibility-opacity",
    "14-overflow",
    "16-outline",
    "17-calc",
    "18-images",
    "36-border-radius",
    "22-transform",
    "38-z-index",
    "39-gradients",
    "40-conic-gradients",
    "41-table",
    "42-position-sticky",
    "43-intrinsic-sizing",
    "47-svg-basic",
    "55-text-rendering",
    "56-mix-blend-mode",
    "15-box-shadow",
    "52-text-shadow-blur",
];

/// Workspace root (two parents up from the driver crate manifest).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Render `graphic_tests/<page>.html` via the deterministic CPU path → RGBA8.
fn render_cpu(page: &str) -> lumen_image::Image {
    let html = workspace_root().join(format!("graphic_tests/{page}.html"));
    let mut session = InProcessSession::new();
    session
        .navigate(&format!("file://{}", html.display()))
        .unwrap_or_else(|e| panic!("navigate {page}: {e}"));
    session
        .screenshot_cpu_rgba()
        .unwrap_or_else(|e| panic!("screenshot_cpu_rgba {page}: {e}"))
}

/// Path of the committed reference PNG for `page`.
fn ref_path(page: &str) -> PathBuf {
    workspace_root().join(format!("graphic_tests/snapshots/cpu/{page}.png"))
}

#[test]
fn cpu_snapshots_match_references() {
    let save = std::env::var("SAVE_CPU_SNAPSHOTS").is_ok();
    if save {
        std::fs::create_dir_all(workspace_root().join("graphic_tests/snapshots/cpu"))
            .expect("create snapshots/cpu dir");
    }

    let mut failures = Vec::new();

    for &page in PAGES {
        let actual = render_cpu(page);
        let actual_rgba = actual.to_rgba8();
        let path = ref_path(page);

        if save {
            let png = lumen_image::encode_png_rgba8(&actual)
                .unwrap_or_else(|e| panic!("encode {page}: {e}"));
            std::fs::write(&path, &png).unwrap_or_else(|e| panic!("write {page}: {e}"));
            eprintln!("saved {} ({} bytes)", path.display(), png.len());
            continue;
        }

        let ref_bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                failures.push(format!("{page}: missing reference {}: {e}", path.display()));
                continue;
            }
        };
        let ref_img = lumen_image::decode(&ref_bytes)
            .unwrap_or_else(|e| panic!("decode reference {page}: {e}"));
        let ref_rgba = ref_img.to_rgba8();

        if ref_img.width != actual.width || ref_img.height != actual.height {
            failures.push(format!(
                "{page}: size {}x{} vs reference {}x{}",
                actual.width, actual.height, ref_img.width, ref_img.height
            ));
            continue;
        }

        // tiny-skia is deterministic, so the reference must reproduce exactly.
        let diff = ref_rgba
            .iter()
            .zip(actual_rgba.iter())
            .filter(|(a, b)| a != b)
            .count();
        if diff != 0 {
            failures.push(format!(
                "{page}: {diff} differing bytes (of {})",
                ref_rgba.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "CPU snapshot mismatches (regenerate with SAVE_CPU_SNAPSHOTS=1 if intentional):\n{}",
        failures.join("\n")
    );
}

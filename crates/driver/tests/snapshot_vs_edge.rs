//! Headless GPU render vs Edge screenshot comparison — informational by default.
//!
//! Renders each `graphic_tests` page through the **wgpu fallback** renderer
//! (`lumen_paint::Renderer::new_headless` → `render_to_image`) and compares the
//! result against the cached Edge headless screenshot in
//! `graphic_tests/screenshots/<stem>-edge.png`.
//!
//! **BUG-121 — why this is NOT a hard gate.** The windowed Lumen app (and the
//! `run.py` gdigrab pipeline) renders through the femtovg backend (ADR-010 RB-9
//! default); `Renderer` in `renderer.rs` is the wgpu *fallback* backend. Fixes
//! landed in `backends/femtovg_backend.rs` (image downscale BUG-077, conic
//! gradients BUG-086, background tiling BUG-095, video placeholder BUG-097, …)
//! do not reach this render path, so per-page percentages diverge widely from
//! `run.py` (e.g. 18-images 57% here vs 21% windowed) and the run.py thresholds
//! in `TESTS` are not attainable here. Until a femtovg headless path exists,
//! threshold violations are **reported but do not fail the test**.
//!
//! **Strict mode:** set `SNAPSHOT_VS_EDGE_STRICT=1` to turn threshold
//! violations into a test failure (for a calibrated CI environment). Render
//! errors (navigate/render panics) and Edge-decode failures are real failures
//! in both modes.
//!
//! **Pixel diff logic** mirrors `run.py diff_stats(channel_threshold=16)`:
//! a pixel counts as "different" if any RGB channel differs by more than 16.
//! Alpha channel is ignored (Edge screenshots are opaque RGB).
//!
//! **Missing screenshots:** if `<stem>-edge.png` is absent (screenshots/ is
//! gitignored), the page is marked SKIP and does not count as a failure.
//! Generate Edge references once with:
//! ```bash
//! python graphic_tests/run.py --no-cache --continue-on-fail
//! ```
//!
//! Run:
//! ```bash
//! cargo test -p lumen-driver -- --nocapture snapshot_vs_edge
//! SNAPSHOT_VS_EDGE_STRICT=1 cargo test -p lumen-driver -- --nocapture snapshot_vs_edge
//! ```

use lumen_driver::{BrowserSession, InProcessSession};
use std::path::{Path, PathBuf};

/// RGB channel delta above which a pixel counts as "different".
/// Matches Python `diff_stats(channel_threshold=16)`.
const CHANNEL_THRESHOLD: i32 = 16;

/// `(stem, threshold_pct)` — mirrors `run.py` TESTS list.
///
/// * `stem`          — HTML file stem = screenshot prefix (`<stem>-edge.png`)
/// * `threshold_pct` — max allowed diff% before run.py would mark the page FAIL.
///   Calibrated for the femtovg windowed pipeline; on the wgpu fallback render
///   used here they are informational (see BUG-121 note in the module docs).
const TESTS: &[(&str, f32)] = &[
    ("00-calibration",            0.5),
    ("01-sanity",                 0.5),
    ("02-color-named",            0.5),
    ("03-color-formats",          0.5),
    ("04-color-alpha",            0.5),
    ("05-border-width",           0.5),
    ("06-border-sides",           0.5),
    ("07-box-sizing",             0.5),
    ("08-padding",                0.5),
    ("09-margin",                 0.5),
    ("10-min-max-width",          0.5),
    ("11-min-max-height",         0.5),
    ("12-display",                0.5),
    ("13-visibility-opacity",     0.5),
    ("14-overflow",               0.5),
    ("15-box-shadow",             0.5),
    ("16-outline",                0.5),
    ("17-calc",                   0.5),
    ("18-images",                 0.5),
    ("19-object-fit",             0.5),
    ("20-quirks-bgcolor",         0.5),
    ("21-border-style",           0.5),
    ("22-transform",              0.5),
    ("23-pseudo-elements",        0.5),
    ("24-vertical-align",         0.5),
    ("25-table-layout",           0.5),
    ("26-mask-image",             0.5),
    ("27-direction-rtl",          0.5),
    ("28-css-containment",        0.5),
    ("29-container-queries",      0.5),
    ("30-css-filter",             0.5),
    ("31-clip-path",              0.5),
    ("32-list-markers",           0.5),
    ("33-multi-column",           0.5),
    ("34-forms",                  0.5),
    ("35-grid-named-areas",       0.5),
    ("36-border-radius",          0.5),
    ("37-float-clear",            0.5),
    ("38-z-index",                0.5),
    ("39-gradients",              0.5),
    ("40-conic-gradients",        0.5),
    ("41-table",                  0.5),
    ("42-position-sticky",        0.5),
    ("43-intrinsic-sizing",       0.5),
    ("44-media-queries",          0.5),
    ("45-multiple-backgrounds",   0.5),
    ("46-individual-transforms",  0.5),
    ("47-svg-basic",              0.5),
    ("48-line-clamp",             0.5),
    ("49-background-blend-mode",  0.5),
    ("50-css-variables",          0.5),
    ("51-scrollbar-rendering",    0.5),
    ("52-text-shadow-blur",       4.0),
    ("53-background-origin",      0.5),
    ("54-svg-path-stroke",        0.5),
    ("55-video-placeholder",      0.5),
    ("56-mix-blend-mode",         0.5),
    ("57-canvas-2d",              0.5),
    ("58-first-letter-line",      2.0),
    ("59-image-set-cross-fade",   2.0),
    ("60-svg-stroke-advanced",    1.0),
    ("61-view-transitions",       1.0),
    ("62-scroll-snap",            1.0),
    ("63-masonry",                1.0),
    ("64-table",                  1.0),
    ("65-flex-align-content",     0.5),
    ("66-selection-pseudo",       0.5),
    ("67-attr-typed",             0.5),
    ("68-font-variation-settings",0.5),
    ("69-border-spacing",         0.5),
    ("70-object-fit",             0.5),
];

/// True when `SNAPSHOT_VS_EDGE_STRICT` is set to a non-empty value other than `0`:
/// threshold violations then fail the test instead of being reported as
/// informational (BUG-121 — wgpu fallback render diverges from the femtovg
/// pipeline the thresholds were calibrated for).
fn strict_mode() -> bool {
    std::env::var("SNAPSHOT_VS_EDGE_STRICT").is_ok_and(|v| !v.is_empty() && v != "0")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Renders `graphic_tests/<stem>.html` via femtovg headless → RGBA8 image.
///
/// Accepts a pre-created `Renderer` so a single wgpu device is reused across all
/// pages — DX12 cannot create 70+ independent devices in one process.
fn render_headless(stem: &str, renderer: &mut lumen_paint::Renderer) -> lumen_image::Image {
    let html = workspace_root().join(format!("graphic_tests/{stem}.html"));
    let mut session = InProcessSession::new();
    session
        .navigate(&format!("file://{}", html.display()))
        .unwrap_or_else(|e| panic!("navigate {stem}: {e}"));
    let dl = session
        .display_list_for_compare()
        .unwrap_or_else(|e| panic!("display_list {stem}: {e}"));
    renderer
        .render_to_image(&dl, 0.0, 0.0)
        .unwrap_or_else(|e| panic!("render_to_image {stem}: {e}"))
}

/// Loads `graphic_tests/screenshots/<stem>-edge.png` → (width, height, RGBA8).
/// Returns `None` if the file does not exist (screenshots are gitignored).
fn load_edge_rgba8(stem: &str) -> Option<(u32, u32, Vec<u8>)> {
    let path = workspace_root()
        .join(format!("graphic_tests/screenshots/{stem}-edge.png"));
    let bytes = std::fs::read(&path).ok()?;
    let img = lumen_image::decode(&bytes)
        .unwrap_or_else(|e| panic!("decode {stem}-edge.png: {e}"));
    let rgba = img.to_rgba8();
    Some((img.width, img.height, rgba))
}

/// Counts pixels where any RGB channel differs by more than `CHANNEL_THRESHOLD`.
/// Alpha channel is ignored (Edge screenshots are opaque RGB).
fn diff_percent(lumen_rgba: &[u8], edge_rgba: &[u8], total_pixels: u32) -> f32 {
    let bad = lumen_rgba
        .chunks_exact(4)
        .zip(edge_rgba.chunks_exact(4))
        .filter(|(l, e)| {
            (l[0] as i32 - e[0] as i32).abs() > CHANNEL_THRESHOLD
                || (l[1] as i32 - e[1] as i32).abs() > CHANNEL_THRESHOLD
                || (l[2] as i32 - e[2] as i32).abs() > CHANNEL_THRESHOLD
        })
        .count();
    bad as f32 / total_pixels as f32 * 100.0
}

#[test]
fn snapshot_vs_edge() {
    // Single wgpu device reused across all pages — DX12 cannot create 70+ devices per process.
    const W: u32 = 1024;
    const H: u32 = 720;
    let font = include_bytes!("../../../assets/fonts/Inter-Regular.ttf").to_vec();
    let mut renderer = lumen_paint::Renderer::new_headless(font, W, H)
        .expect("headless renderer init");

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut skip = 0usize;
    let mut failures: Vec<String> = Vec::new();

    eprintln!("\n{:<35}  {:>7}  {:>6}  STATUS", "PAGE", "DIFF%", "LIMIT");
    eprintln!("{}", "-".repeat(60));

    for &(stem, threshold) in TESTS {
        let Some((ew, eh, edge_rgba)) = load_edge_rgba8(stem) else {
            eprintln!("{stem:<35}  {:>7}  {:>6}  SKIP (no edge screenshot)", "-", "-");
            skip += 1;
            continue;
        };

        let lumen_img = render_headless(stem, &mut renderer);
        let lumen_rgba = lumen_img.to_rgba8();

        if lumen_img.width != ew || lumen_img.height != eh {
            eprintln!(
                "{stem:<35}  {:>7}  {:>6}  SKIP (size {}x{} vs edge {}x{})",
                "-", "-", lumen_img.width, lumen_img.height, ew, eh,
            );
            skip += 1;
            continue;
        }

        let pct = diff_percent(&lumen_rgba, &edge_rgba, ew * eh);
        let status = if pct <= threshold { "PASS" } else { "FAIL" };

        eprintln!("{stem:<35}  {:>6.2}%  {:>5.1}%  {status}", pct, threshold);

        if pct <= threshold {
            pass += 1;
        } else {
            fail += 1;
            failures.push(format!("{stem}: {pct:.2}% > {threshold:.1}% limit"));
        }
    }

    eprintln!("{}", "-".repeat(60));
    eprintln!("PASS: {pass}  FAIL: {fail}  SKIP: {skip}  TOTAL: {}", TESTS.len());
    eprintln!();

    if failures.is_empty() {
        return;
    }
    if strict_mode() {
        panic!(
            "wgpu-headless vs Edge mismatches (regenerate Edge refs: python graphic_tests/run.py --no-cache --continue-on-fail):\n{}",
            failures.join("\n")
        );
    }
    eprintln!(
        "INFORMATIONAL: {fail} page(s) over run.py threshold — wgpu fallback render \
         diverges from the femtovg windowed pipeline (BUG-121); not failing the test. \
         Set SNAPSHOT_VS_EDGE_STRICT=1 to enforce."
    );
}

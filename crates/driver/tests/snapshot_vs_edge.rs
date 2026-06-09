//! CPU render vs Edge screenshot comparison.
//!
//! Renders each `graphic_tests` page through the deterministic CPU path
//! (`InProcessSession::screenshot_cpu_rgba`) and compares the result against
//! the cached Edge headless screenshot in
//! `graphic_tests/screenshots/<stem>-edge.png`.
//!
//! **Purpose:** replaces the gdigrab+Python pipeline for Lumen-vs-Edge diff
//! measurement — no browser window, no ffmpeg, no external tools. The Edge
//! screenshots are captured once by `run.py` and reused as the reference.
//!
//! **Pixel diff logic** mirrors `run.py diff_stats(channel_threshold=16)`:
//! a pixel counts as "different" if any RGB channel differs by more than 16.
//! Alpha channel is ignored (Edge screenshots are opaque RGB).
//!
//! **Missing screenshots:** if `<stem>-edge.png` is absent (screenshots/ is
//! gitignored), the page is marked SKIP. The test always passes — it is an
//! informational report. Use `run.py` for the hard CI gate.
//!
//! Run:
//! ```bash
//! cargo test -p lumen-driver --features cpu-render -- --nocapture snapshot_vs_edge
//! ```
//!
//! Regenerate Edge references (requires Edge + run.py):
//! ```bash
//! python graphic_tests/run.py --no-cache --continue-on-fail
//! ```

#![cfg(feature = "cpu-render")]

use lumen_driver::{BrowserSession, InProcessSession};
use std::path::{Path, PathBuf};

/// RGB channel delta above which a pixel counts as "different".
/// Matches Python `diff_stats(channel_threshold=16)`.
const CHANNEL_THRESHOLD: i32 = 16;

/// `(stem, threshold_pct)` — mirrors `run.py` TESTS list.
///
/// * `stem`          — HTML file stem = screenshot prefix (`<stem>-edge.png`)
/// * `threshold_pct` — max allowed diff% before run.py would mark the page FAIL
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Renders `graphic_tests/<stem>.html` via the CPU path → RGBA8 image.
fn render_cpu(stem: &str) -> lumen_image::Image {
    let html = workspace_root().join(format!("graphic_tests/{stem}.html"));
    let mut session = InProcessSession::new();
    session
        .navigate(&format!("file://{}", html.display()))
        .unwrap_or_else(|e| panic!("navigate {stem}: {e}"));
    session
        .screenshot_cpu_rgba()
        .unwrap_or_else(|e| panic!("screenshot_cpu_rgba {stem}: {e}"))
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
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut skip = 0usize;

    eprintln!("\n{:<35}  {:>7}  {:>6}  {}", "PAGE", "DIFF%", "LIMIT", "STATUS");
    eprintln!("{}", "-".repeat(60));

    for &(stem, threshold) in TESTS {
        let Some((ew, eh, edge_rgba)) = load_edge_rgba8(stem) else {
            eprintln!("{stem:<35}  {:>7}  {:>6}  SKIP (no edge screenshot)", "-", "-");
            skip += 1;
            continue;
        };

        let lumen_img = render_cpu(stem);
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

        if pct <= threshold { pass += 1; } else { fail += 1; }
    }

    eprintln!("{}", "-".repeat(60));
    eprintln!("PASS: {pass}  FAIL: {fail}  SKIP: {skip}  TOTAL: {}", TESTS.len());
    eprintln!();

    // The test itself always succeeds — this is an informational report.
    // FAIL lines above show where Lumen diverges from Edge; fix the engine to reduce them.
    let _ = (pass, fail, skip);
}

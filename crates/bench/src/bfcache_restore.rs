//! bfcache restore-time benchmark (Ph3 `P3-bfcache`, DoD step 9,
//! `docs/tasks/ph3-bfcache.md`).
//!
//! Measures the cost of thawing a frozen page: `Document::from_bytes()`
//! (DOM arena deserialize) + a full relayout. This mirrors what
//! `Lumen::bfcache_thaw` (`crates/shell/src/main.rs`) actually does on the
//! restore path today — the JS heap resume is gated on 10C.2 (QuickJS heap
//! serialization, blocked by rquickjs bindings), so thaw always re-layouts
//! from the restored DOM rather than reusing a retained `LayoutBox`.
//!
//! Freeze (`Document::to_bytes()`) happens once, outside the measured loop:
//! it runs at navigate-away time, not on the restore path measured here.

use std::hint::black_box;
use std::time::{Duration, Instant};

use lumen_core::geom::Size;
use lumen_dom::Document;

use crate::print_phase;
use crate::util::extract_style_blocks;

const PAGE_HTML: &[u8] = include_bytes!("../../../samples/page.html");
const VIEWPORT: Size = Size { width: 1024.0, height: 720.0 };

/// P50 (median) restore-time budget from `docs/tasks/ph3-bfcache.md` step 9.
pub const P50_LIMIT: Duration = Duration::from_millis(50);

/// Runs the bfcache-restore benchmark, prints min/med/mean/p95/max, and
/// returns the sorted sample durations (median = `samples[len / 2]`).
pub fn run_bfcache_bench(iters: usize, measurer: &lumen_paint::FontMeasurer<'_>) -> Vec<Duration> {
    let (dom_bytes, sheet) = freeze_page();
    println!(
        "=== bfcache restore (samples/page.html) ===  {} bytes frozen DOM",
        dom_bytes.len()
    );

    for _ in 0..10 {
        black_box(restore_once(&dom_bytes, &sheet, measurer));
    }

    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        samples.push(restore_once(&dom_bytes, &sheet, measurer));
    }
    print_phase("restore   ", &mut samples);
    samples
}

/// Parses `PAGE_HTML` once (as a real navigation would) and freezes the
/// resulting DOM to bincode bytes, ready to be thawed repeatedly.
fn freeze_page() -> (Vec<u8>, lumen_css_parser::Stylesheet) {
    let encoding = lumen_encoding::detect(PAGE_HTML, None);
    let source = lumen_encoding::decode(encoding, PAGE_HTML);
    let doc = lumen_html_parser::parse(&source);
    let css = extract_style_blocks(&doc);
    let sheet = lumen_css_parser::parse(&css);
    let dom_bytes = doc
        .to_bytes()
        .expect("Document::to_bytes on parsed samples/page.html");
    (dom_bytes, sheet)
}

/// One thaw: deserialize the frozen DOM + full relayout. Returns elapsed time.
fn restore_once(
    dom_bytes: &[u8],
    sheet: &lumen_css_parser::Stylesheet,
    measurer: &lumen_paint::FontMeasurer<'_>,
) -> Duration {
    let t = Instant::now();
    let doc = Document::from_bytes(dom_bytes).expect("Document::from_bytes on frozen page.html");
    let layout = lumen_layout::layout_measured(&doc, sheet, VIEWPORT, measurer);
    let elapsed = t.elapsed();
    black_box((doc, layout));
    elapsed
}

/// P50 for the CI gate: fewer iterations than the interactive report (speed).
pub fn median_restore_ms(measurer: &lumen_paint::FontMeasurer<'_>) -> f64 {
    const CI_ITERS: usize = 20;
    let mut samples = Vec::with_capacity(CI_ITERS);
    let (dom_bytes, sheet) = freeze_page();
    for _ in 0..3 {
        black_box(restore_once(&dom_bytes, &sheet, measurer));
    }
    for _ in 0..CI_ITERS {
        samples.push(restore_once(&dom_bytes, &sheet, measurer));
    }
    samples.sort();
    samples[samples.len() / 2].as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p50_limit_is_50ms() {
        assert_eq!(P50_LIMIT.as_millis(), 50);
    }

    #[test]
    fn freeze_then_restore_roundtrips() {
        let font = lumen_font::Font::parse(include_bytes!(
            "../../../assets/fonts/Inter-Regular.ttf"
        ))
        .expect("Inter Regular parses");
        let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer builds");
        let (dom_bytes, sheet) = freeze_page();
        assert!(!dom_bytes.is_empty());
        let elapsed = restore_once(&dom_bytes, &sheet, &measurer);
        assert!(elapsed.as_nanos() > 0, "restore should take nonzero time");
    }

    #[test]
    fn median_restore_ms_is_positive() {
        let font = lumen_font::Font::parse(include_bytes!(
            "../../../assets/fonts/Inter-Regular.ttf"
        ))
        .expect("Inter Regular parses");
        let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer builds");
        assert!(median_restore_ms(&measurer) > 0.0);
    }
}

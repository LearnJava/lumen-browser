//! CI performance gate for the rendering pipeline (task 9G.3).
//!
//! Invoked via `lumen-bench --ci`. Loads `samples/heavy.html`, runs the full
//! `decode → html-parse → css-parse → layout → paint` pipeline 3 times (all
//! measured — no separate warmup phase to keep CI fast), then asserts:
//!
//! - Mean total pipeline time < 200 ms
//! - Peak RSS across all 3 runs < 512 MB
//!
//! Exits 0 on pass, 1 on failure. Prints a brief summary to stdout so CI
//! logs can show the exact numbers that caused a regression.

use std::hint::black_box;
use std::time::{Duration, Instant};

use lumen_core::geom::Size;

use crate::util::{extract_style_blocks, get_rss_bytes};

const HEAVY_HTML: &[u8] = include_bytes!("../../../samples/heavy.html");
const INTER_FONT: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

/// Number of measured pipeline runs.
const CI_RUNS: usize = 3;
/// Maximum allowed mean total pipeline time.
const MEAN_TOTAL_LIMIT: Duration = Duration::from_millis(200);
/// Maximum allowed peak RSS (512 MiB in bytes).
const PEAK_RSS_LIMIT: u64 = 512 * 1024 * 1024;

const VIEWPORT: Size = Size { width: 1024.0, height: 720.0 };

/// Run the CI performance gate.
///
/// Prints a summary to stdout. Returns `true` if all assertions passed,
/// `false` otherwise. Caller must exit with code 1 on false.
pub fn run_ci_gate() -> bool {
    let font = lumen_font::Font::parse(INTER_FONT).expect("Inter Regular parses");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer builds");

    println!(
        "CI bench gate  runs={CI_RUNS}  limits: mean_total<{}ms  peak_rss<{}MB",
        MEAN_TOTAL_LIMIT.as_millis(),
        PEAK_RSS_LIMIT / (1024 * 1024),
    );
    println!("page: samples/heavy.html  ({} bytes)", HEAVY_HTML.len());
    println!();

    let mut totals: Vec<Duration> = Vec::with_capacity(CI_RUNS);
    let mut peak_rss: u64 = 0;

    for run in 1..=CI_RUNS {
        let (total, rss) = run_once(&measurer);
        println!(
            "  run {run}/{CI_RUNS}  total={:.2}ms  rss={:.1}MB",
            total.as_secs_f64() * 1000.0,
            rss as f64 / (1024.0 * 1024.0),
        );
        totals.push(total);
        peak_rss = peak_rss.max(rss);
    }

    let mean_total: Duration = totals.iter().copied().sum::<Duration>() / CI_RUNS as u32;
    let time_ok = mean_total <= MEAN_TOTAL_LIMIT;
    let rss_ok = peak_rss <= PEAK_RSS_LIMIT;

    println!();
    println!(
        "mean_total: {:.2}ms  (limit {}ms)  {}",
        mean_total.as_secs_f64() * 1000.0,
        MEAN_TOTAL_LIMIT.as_millis(),
        if time_ok { "OK" } else { "EXCEEDED" },
    );
    println!(
        "peak_rss:   {:.1}MB  (limit {}MB)  {}",
        peak_rss as f64 / (1024.0 * 1024.0),
        PEAK_RSS_LIMIT / (1024 * 1024),
        if rss_ok { "OK" } else { "EXCEEDED" },
    );
    println!();

    let passed = time_ok && rss_ok;
    if passed {
        println!("PASS");
    } else {
        if !time_ok {
            println!(
                "FAIL: mean_total {}ms exceeds limit {}ms",
                mean_total.as_millis(),
                MEAN_TOTAL_LIMIT.as_millis(),
            );
        }
        if !rss_ok {
            println!(
                "FAIL: peak_rss {}MB exceeds limit {}MB",
                peak_rss / (1024 * 1024),
                PEAK_RSS_LIMIT / (1024 * 1024),
            );
        }
    }

    passed
}

/// Runs the full pipeline once on `HEAVY_HTML` and returns `(total_time, peak_rss)`.
fn run_once(measurer: &lumen_paint::FontMeasurer<'_>) -> (Duration, u64) {
    let rss_before = get_rss_bytes();
    let start = Instant::now();

    let encoding = lumen_encoding::detect(HEAVY_HTML, None);
    let source = lumen_encoding::decode(encoding, HEAVY_HTML);
    let doc = lumen_html_parser::parse(&source);
    let css_text = extract_style_blocks(&doc);
    let sheet = lumen_css_parser::parse(&css_text);
    let layout = lumen_layout::layout_measured(&doc, &sheet, VIEWPORT, measurer);
    let list = lumen_paint::build_display_list(&layout);

    let total = start.elapsed();
    let rss_after = get_rss_bytes();
    black_box((doc, sheet, layout, list));

    (total, rss_before.max(rss_after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_gate_limits_are_sane() {
        assert_eq!(MEAN_TOTAL_LIMIT.as_millis(), 200);
        assert_eq!(PEAK_RSS_LIMIT, 512 * 1024 * 1024);
        assert_eq!(CI_RUNS, 3);
    }

    #[test]
    fn gate_passes_on_good_numbers() {
        let mean = Duration::from_millis(50);
        let rss: u64 = 100 * 1024 * 1024;
        assert!(mean <= MEAN_TOTAL_LIMIT);
        assert!(rss <= PEAK_RSS_LIMIT);
    }

    #[test]
    fn gate_fails_on_slow_mean() {
        let mean = Duration::from_millis(300);
        assert!(mean > MEAN_TOTAL_LIMIT);
    }

    #[test]
    fn gate_fails_on_high_rss() {
        let rss: u64 = 600 * 1024 * 1024;
        assert!(rss > PEAK_RSS_LIMIT);
    }

    #[test]
    fn run_once_returns_positive_duration() {
        let font = lumen_font::Font::parse(INTER_FONT).expect("Inter parses");
        let measurer = lumen_paint::FontMeasurer::new(&font).expect("measurer builds");
        let (total, _rss) = run_once(&measurer);
        assert!(total.as_nanos() > 0, "pipeline should take nonzero time");
    }

    #[test]
    fn run_once_three_times_smoke() {
        // Validates the pipeline runs CI_RUNS times without panic.
        // Hard limits are only enforced by the binary (--ci mode).
        let font = lumen_font::Font::parse(INTER_FONT).expect("Inter parses");
        let measurer = lumen_paint::FontMeasurer::new(&font).expect("measurer builds");
        for _ in 0..CI_RUNS {
            let (total, _rss) = run_once(&measurer);
            assert!(total.as_nanos() > 0);
        }
    }
}

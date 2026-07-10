//! Warm-frame benchmark harness (p1-exp-wgpu-only, experimental branch).
//!
//! # Why this exists
//!
//! Every optimization on this branch was measured with `measure_idle.ps1`, which
//! samples process CPU over a 10-second *idle* window. Measurement 2026-07-09
//! (EXPERIMENT.md §12) showed that window contains **four frames** and no
//! `skip (identical frame)` at all: without input the shell never calls
//! `render`. So `t=5s cpu` measures the tail of startup, not the steady state,
//! and every per-frame optimization aimed at it was aimed at nothing.
//!
//! This harness measures the thing a browser actually does: repaint under input.
//!
//! # Modes (`LUMEN_BENCH=<mode>:<frames>`)
//!
//! - `hover:N` — request a redraw N times **without changing any state**. Every
//!   frame hits the skip-identical-frame path, so the sample is the cost of
//!   deciding *not* to draw: `hash_display_list` + `content_generation`. This is
//!   the path a mouse moving over a static page takes (31 skips / 2 renders per
//!   5 s, per EXPERIMENT.md §3).
//! - `scroll:N` — advance page scroll by one CSS px per frame (bouncing at the
//!   ends) and redraw. The display list is untouched, so this measures the real
//!   repaint: hash → collect → encode → submit.
//!
//! Both modes exit the process after printing statistics, so a run is a single
//! reproducible command.
//!
//! # Reading the numbers
//!
//! Report is median / p95 / max, never the mean: frame-time distributions have a
//! cold-start tail (the first frames build framebuffer caches — §12), and a mean
//! silently folds that tail into the steady state. `warmup` frames are measured
//! and then discarded for exactly this reason.
//!
//! Run with `LUMEN_PRESENT=immediate` unless you *want* vsync in the sample:
//! under the default `Fifo` present mode `frame.present()` blocks until the next
//! refresh, which pins every sample at ~16.7 ms and hides everything.

use std::cell::RefCell;
use std::sync::OnceLock;

/// Wall-clock anchor for [`log_first_frame_once`], set by [`mark_process_start`]
/// from the first line of `main()`.
static PROCESS_START: OnceLock<std::time::Instant> = OnceLock::new();

/// Anchors "time since launch" measurements. Call once, first thing in `main()`.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(std::time::Instant::now());
}

/// Prints `launch -> first non-empty frame` once per process.
///
/// Counterpart of the Chromium baseline's `launch->FCP` (Paint Timing API +
/// process spawn wall clock, `scripts/exp/chromium_baseline.py`): both anchor
/// at process creation and stop at the first frame with page content. Not
/// gated on `LUMEN_BENCH` — one stderr line per run, and startup time is a §4
/// score-table metric that every launch should report.
pub fn log_first_frame_once(dl_len: usize) {
    if dl_len == 0 {
        return;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if let Some(t0) = PROCESS_START.get() {
            eprintln!(
                "[bench] first non-empty frame: {:.0}ms since process start",
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
    });
}

/// What the harness perturbs between frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchMode {
    /// Redraw with no state change — exercises the skip-identical-frame path.
    Hover,
    /// Advance page scroll by 1 CSS px per frame — exercises a real repaint.
    Scroll,
}

/// Parsed `LUMEN_BENCH` configuration.
#[derive(Debug, Clone, Copy)]
pub struct BenchCfg {
    /// Which perturbation to apply between frames.
    pub mode: BenchMode,
    /// Frames to measure (after warmup).
    pub frames: u32,
    /// Frames to render and discard before measuring.
    pub warmup: u32,
}

/// Mutable per-run state. The winit event loop is single-threaded, so a
/// `thread_local` keeps this out of the `Lumen` struct and its constructors.
struct BenchRun {
    /// Measured frame durations, in milliseconds.
    samples: Vec<f64>,
    /// Warmup frames still to discard.
    warmup_left: u32,
    /// Measured frames still to collect.
    frames_left: u32,
    /// Scroll direction for `BenchMode::Scroll`: `+1.0` down, `-1.0` up.
    dir: f32,
    /// Set once the run has printed its report; suppresses a second report.
    finished: bool,
    /// `FRAMES_RENDERED` when the first measured frame was recorded.
    rendered_at_start: u64,
    /// `FRAMES_SKIPPED` when the first measured frame was recorded.
    skipped_at_start: u64,
}

thread_local! {
    static RUN: RefCell<Option<BenchRun>> = const { RefCell::new(None) };
}

/// Parses `LUMEN_BENCH` once per process.
///
/// Format: `<hover|scroll>:<frames>[:<warmup>]`. Warmup defaults to 30 — enough
/// to get past the framebuffer-cache warmup measured in EXPERIMENT.md §12
/// (cold `encode` decayed from 116 ms to 3.9 ms by the fourth frame).
///
/// A malformed value is a hard error rather than a silent fallback: a benchmark
/// that quietly measured the wrong thing is worse than one that refuses to run.
pub fn cfg() -> Option<BenchCfg> {
    static CFG: OnceLock<Option<BenchCfg>> = OnceLock::new();
    *CFG.get_or_init(|| {
        let raw = std::env::var("LUMEN_BENCH").ok()?;
        let mut parts = raw.split(':');
        let mode = match parts.next() {
            Some("hover") => BenchMode::Hover,
            Some("scroll") => BenchMode::Scroll,
            other => {
                eprintln!("LUMEN_BENCH: неизвестный режим {other:?}, ожидается hover|scroll");
                std::process::exit(2);
            }
        };
        let frames = parts.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(600);
        let warmup = parts.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(30);
        if frames == 0 {
            eprintln!("LUMEN_BENCH: frames must be > 0");
            std::process::exit(2);
        }
        Some(BenchCfg { mode, frames, warmup })
    })
}

/// `true` when the harness is driving this process.
pub fn active() -> bool {
    cfg().is_some()
}

/// Lazily creates the run state on first use.
fn with_run<R>(f: impl FnOnce(&mut BenchRun) -> R) -> Option<R> {
    let c = cfg()?;
    RUN.with(|cell| {
        let mut slot = cell.borrow_mut();
        let run = slot.get_or_insert_with(|| BenchRun {
            samples: Vec::with_capacity(c.frames as usize),
            warmup_left: c.warmup,
            frames_left: c.frames,
            dir: 1.0,
            finished: false,
            rendered_at_start: 0,
            skipped_at_start: 0,
        });
        Some(f(run))
    })
}

/// Reads the paint-side frame counters.
fn frame_counters() -> (u64, u64) {
    (
        lumen_paint::load_counter(&lumen_paint::FRAMES_RENDERED),
        lumen_paint::load_counter(&lumen_paint::FRAMES_SKIPPED),
    )
}

/// Records one frame's wall duration. Warmup frames are measured and dropped.
pub fn record_frame(ms: f64) {
    with_run(|run| {
        if run.finished {
            return;
        }
        if run.warmup_left > 0 {
            run.warmup_left -= 1;
            if run.warmup_left == 0 {
                // Baseline taken after warmup, so cold frames don't pollute the
                // rendered/skipped ratio the report uses to validate itself.
                let (r, s) = frame_counters();
                run.rendered_at_start = r;
                run.skipped_at_start = s;
            }
        } else if run.frames_left > 0 {
            run.samples.push(ms);
            run.frames_left -= 1;
        }
    });
}

/// `true` once every measured frame has been collected.
pub fn done() -> bool {
    with_run(|run| run.frames_left == 0 && run.warmup_left == 0).unwrap_or(false)
}

/// `true` once the warmup frames are behind us.
///
/// Geometry must be reported from here, not from the first non-empty display
/// list: the HTML parser streams, so an early sample catches a half-built page
/// (`dl 59 cmds` where the final list has 1062, `content_height 711` before the
/// rest of the document arrived).
fn warmup_done() -> bool {
    with_run(|run| run.warmup_left == 0).unwrap_or(false)
}

/// Prints the page geometry the harness depends on, once per process.
///
/// `max_scroll == 0` silently turns `scroll` mode into `hover` mode: the page
/// cannot move, every frame hashes identically, and the report shows the skip
/// path while claiming to show a repaint. Print it rather than infer it.
///
/// Waits until warmup is over: `about_to_wait` runs before the page has loaded
/// (`content_height == 0`, empty display list) and while the parser is still
/// streaming, so an earlier sample reports a half-built page and can print a
/// false "page cannot scroll" warning.
pub fn log_geometry_once(
    content_height: f32,
    viewport_height: f32,
    max_scroll: f32,
    dl_len: usize,
) {
    if !active() || dl_len == 0 || !warmup_done() {
        return;
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        eprintln!(
            "[bench] geometry: content_height {content_height:.0} viewport {viewport_height:.0} \
             max_scroll {max_scroll:.0} | dl {dl_len} cmds"
        );
        if max_scroll <= 0.0 {
            eprintln!(
                "[bench] geometry: max_scroll is 0 — scroll mode cannot repaint. \
                 Use a taller page, or shrink the window with LUMEN_WINDOW=WxH."
            );
        }
    });
}

/// Scroll delta for the next frame, bouncing between `0` and `max_scroll`.
///
/// Bounces rather than wrapping: a wrap to 0 would make one frame in every
/// `max_scroll` a full-viewport jump, and that outlier lands in p95.
pub fn next_scroll(current: f32, max_scroll: f32) -> f32 {
    with_run(|run| {
        if max_scroll <= 0.0 {
            return 0.0;
        }
        let next = current + run.dir;
        if next >= max_scroll {
            run.dir = -1.0;
            max_scroll
        } else if next <= 0.0 {
            run.dir = 1.0;
            0.0
        } else {
            next
        }
    })
    .unwrap_or(current)
}

/// Prints the report. Idempotent — only the first call emits.
pub fn report() {
    let Some(c) = cfg() else { return };
    with_run(|run| {
        if run.finished {
            return;
        }
        run.finished = true;

        if run.samples.is_empty() {
            eprintln!("[bench] no samples collected");
            return;
        }
        let mut s = run.samples.clone();
        s.sort_by(f64::total_cmp);
        let n = s.len();
        let pick = |q: f64| s[((n as f64 - 1.0) * q).round() as usize];
        let sum: f64 = s.iter().sum();

        let mode = match c.mode {
            BenchMode::Hover => "hover (skip-identical path)",
            BenchMode::Scroll => "scroll (full repaint)",
        };
        let (r_now, s_now) = frame_counters();
        let rendered = r_now - run.rendered_at_start;
        let skipped = s_now - run.skipped_at_start;

        eprintln!(
            "[bench] {mode}: n={n} warmup={} | median {:.3}ms p95 {:.3}ms max {:.3}ms \
             min {:.3}ms mean {:.3}ms | total {:.1}ms | rendered {rendered} skipped {skipped}",
            c.warmup,
            pick(0.50),
            pick(0.95),
            s[n - 1],
            s[0],
            sum / n as f64,
            sum,
        );

        // Self-validation. A harness that measured the wrong path must say so
        // loudly: a silent wrong number is what produced three false premises
        // on this branch already (EXPERIMENT.md §8, §9, §12).
        match c.mode {
            BenchMode::Scroll if rendered == 0 => eprintln!(
                "[bench] INVALID: scroll mode repainted nothing — every frame was skipped. \
                 The page probably does not scroll (max_scroll == 0): check content_height \
                 against the viewport. These numbers measure the hash, not a repaint."
            ),
            BenchMode::Scroll if skipped > rendered => eprintln!(
                "[bench] WARNING: {skipped} skips vs {rendered} repaints — the perturbation \
                 is not reaching the frame hash on every frame."
            ),
            BenchMode::Hover if rendered > 0 => eprintln!(
                "[bench] WARNING: hover mode repainted {rendered} frames — something other \
                 than the harness is dirtying the frame (animation? GIF? caret blink?). \
                 The sample is not a pure skip-path measurement."
            ),
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_scroll_bounces_at_both_ends() {
        // No config → the harness is inert and must not move the page.
        assert_eq!(next_scroll(10.0, 100.0), 10.0, "inert without LUMEN_BENCH");
    }

    #[test]
    fn next_scroll_is_a_noop_when_page_fits() {
        assert_eq!(next_scroll(0.0, 0.0), 0.0);
    }
}

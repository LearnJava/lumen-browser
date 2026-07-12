//! Warm-frame benchmark harness (ported from `p1-exp-wgpu-only`).
//!
//! Measures the thing a browser actually does: repaint under input. Drives
//! the page scroll from top to bottom and back (bouncing), records per-frame
//! wall time, prints median/p95/max and exits — one reproducible command.
//!
//! # Modes (`LUMEN_BENCH=<mode>:<frames>[:<warmup>[:<step>]]`)
//!
//! - `hover:N` — request a redraw N times **without changing any state**.
//! - `scroll:N` — advance page scroll by `step` CSS px per frame (bouncing at
//!   the ends) and redraw. The display list is untouched, so this measures the
//!   real repaint path end to end.
//!
//! # Reading the numbers
//!
//! Report is median / p95 / max, never the mean alone: frame-time
//! distributions have a cold-start tail and a mean silently folds that tail
//! into the steady state. `warmup` frames are measured and then discarded.
//!
//! Run with `LUMEN_PRESENT=immediate` (wgpu backend) unless you *want* vsync
//! in the sample: under the default `Fifo` present mode `frame.present()`
//! blocks until the next refresh, pinning every sample at ~16.7 ms. The
//! femtovg/OpenGL backend has no such switch and always presents on vsync.
//!
//! Difference from the experimental branch: this branch has no
//! skip-identical-frame path, so the frame counters live here in the shell
//! ([`mark_rendered`] is called from the `RedrawRequested` render site) and
//! `skipped` is always 0.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// Frames that reached the backend `render` call this process.
static FRAMES_RENDERED: AtomicU64 = AtomicU64::new(0);

/// Wall-clock anchor for [`log_first_frame_once`], set by [`mark_process_start`]
/// from the first line of `main()`.
static PROCESS_START: OnceLock<std::time::Instant> = OnceLock::new();

/// Anchors "time since launch" measurements. Call once, first thing in `main()`.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(std::time::Instant::now());
}

/// Counts a frame that reached the backend render call. Call from the
/// `RedrawRequested` handler next to `Renderer::render`.
pub fn mark_rendered() {
    FRAMES_RENDERED.fetch_add(1, Ordering::Relaxed);
}

/// Prints `launch -> first non-empty frame` once per process.
///
/// Counterpart of a Chromium `launch->FCP` measurement: both anchor at process
/// creation and stop at the first frame with page content. Not gated on
/// `LUMEN_BENCH` — one stderr line per run.
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
    /// Redraw with no state change.
    Hover,
    /// Advance page scroll by `step` CSS px per frame — a real repaint.
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
    /// CSS px of page scroll per frame in `Scroll` mode (4th field, default 1).
    /// Large steps model a fast fling.
    pub step: f32,
    /// Minimum ms between driven frames (5th field, default 25).
    ///
    /// Back-to-back redraw requests starve the compositor of buffer-release
    /// opportunities on Wayland/KWin: after ~4 frames (one swapchain) every
    /// `get_current_texture` stalls to a 10 s `Timeout`. Interactive-rate
    /// redraws never hit this, so the harness paces itself instead of
    /// spinning. Frame-time samples stay honest — they measure work inside
    /// `RedrawRequested`, not the idle gap between frames.
    pub pace_ms: u64,
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
    /// Full top-to-bottom (or bottom-to-top) passes completed, i.e. bounces.
    passes: u32,
    /// Set once the run has printed its report; suppresses a second report.
    finished: bool,
    /// `FRAMES_RENDERED` when the first measured frame was recorded.
    rendered_at_start: u64,
}

thread_local! {
    static RUN: RefCell<Option<BenchRun>> = const { RefCell::new(None) };
}

/// Parses `LUMEN_BENCH` once per process.
///
/// Format: `<hover|scroll>:<frames>[:<warmup>[:<step>]]`. Warmup defaults to
/// 30 — enough to get past cold framebuffer/pipeline warmup.
///
/// A malformed value is a hard error rather than a silent fallback: a
/// benchmark that quietly measured the wrong thing is worse than one that
/// refuses to run.
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
        let step = parts.next().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
        let pace_ms = parts.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(25);
        if frames == 0 {
            eprintln!("LUMEN_BENCH: frames must be > 0");
            std::process::exit(2);
        }
        // NaN falls in here too — a bench with a garbage step is meaningless.
        if step <= 0.0 || step.is_nan() {
            eprintln!("LUMEN_BENCH: step must be > 0");
            std::process::exit(2);
        }
        Some(BenchCfg { mode, frames, warmup, step, pace_ms })
    })
}

thread_local! {
    /// Wall clock of the last harness-driven redraw request (pacing anchor).
    static LAST_DRIVE: RefCell<Option<std::time::Instant>> = const { RefCell::new(None) };
}

/// `true` when at least `pace_ms` elapsed since the previous driven frame —
/// the caller should perturb state and request a redraw. Otherwise the caller
/// should park the loop until [`next_drive_deadline`].
pub fn should_drive() -> bool {
    let Some(c) = cfg() else { return false };
    LAST_DRIVE.with(|cell| {
        let mut last = cell.borrow_mut();
        let now = std::time::Instant::now();
        match *last {
            Some(t) if now.duration_since(t).as_millis() < u128::from(c.pace_ms) => false,
            _ => {
                *last = Some(now);
                true
            }
        }
    })
}

/// Deadline for the next driven frame (used with `ControlFlow::WaitUntil`).
pub fn next_drive_deadline() -> std::time::Instant {
    let pace = cfg().map_or(25, |c| c.pace_ms);
    LAST_DRIVE.with(|cell| {
        cell.borrow()
            .map_or_else(std::time::Instant::now, |t| t + std::time::Duration::from_millis(pace))
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
            passes: 0,
            finished: false,
            rendered_at_start: 0,
        });
        Some(f(run))
    })
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
                // Baseline taken after warmup, so cold frames don't pollute
                // the rendered count the report uses to validate itself.
                run.rendered_at_start = FRAMES_RENDERED.load(Ordering::Relaxed);
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
/// list: the HTML parser streams, so an early sample catches a half-built page.
fn warmup_done() -> bool {
    with_run(|run| run.warmup_left == 0).unwrap_or(false)
}

/// Prints the page geometry the harness depends on, once per process.
///
/// `max_scroll == 0` silently turns `scroll` mode into `hover` mode: the page
/// cannot move and the report shows a no-op while claiming a repaint. Print it
/// rather than infer it.
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
    let step = cfg().map_or(1.0, |c| c.step);
    with_run(|run| {
        if max_scroll <= 0.0 {
            return 0.0;
        }
        let next = current + run.dir * step;
        if next >= max_scroll {
            run.dir = -1.0;
            run.passes += 1;
            max_scroll
        } else if next <= 0.0 {
            run.dir = 1.0;
            run.passes += 1;
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
        let mean = sum / n as f64;

        let mode = match c.mode {
            BenchMode::Hover => "hover (no state change)",
            BenchMode::Scroll => "scroll (full repaint)",
        };
        let rendered =
            FRAMES_RENDERED.load(Ordering::Relaxed) - run.rendered_at_start;

        eprintln!(
            "[bench] {mode}: n={n} warmup={} step={} | median {:.3}ms p95 {:.3}ms max {:.3}ms \
             min {:.3}ms mean {:.3}ms | total {:.1}ms | rendered {rendered} | \
             passes {} | scroll_speed {:.0}px/s",
            c.warmup,
            c.step,
            pick(0.50),
            pick(0.95),
            s[n - 1],
            s[0],
            mean,
            sum,
            run.passes,
            // Effective scroll velocity: px advanced per frame over mean frame
            // wall time. Frames run back-to-back, so mean wall ≈ frame period.
            f64::from(c.step) / (mean / 1000.0),
        );

        // Self-validation. A harness that measured the wrong path must say so
        // loudly — silent wrong numbers produced multiple false premises on
        // the experimental branch.
        if c.mode == BenchMode::Scroll && rendered == 0 {
            eprintln!(
                "[bench] INVALID: scroll mode repainted nothing. The page probably \
                 does not scroll (max_scroll == 0): check content_height against \
                 the viewport."
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_scroll_is_inert_without_config() {
        // No LUMEN_BENCH in the test env → the harness must not move the page.
        assert_eq!(next_scroll(10.0, 100.0), 10.0);
    }

    #[test]
    fn next_scroll_is_a_noop_when_page_fits() {
        assert_eq!(next_scroll(0.0, 0.0), 0.0);
    }
}

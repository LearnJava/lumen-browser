//! Focus mode + Pomodoro timer panel (V4, task #25).
//!
//! Focus mode is a distraction-free reading view: while active the shell hides
//! the tab bar and side panels and overlays a compact Pomodoro countdown widget
//! in the top-right corner.  The widget draws a circular **arc progress ring**
//! that fills as the work session elapses, with the remaining time as `MM:SS` in
//! the centre.
//!
//! Toggled with `Ctrl+Shift+F` (default 25-minute Pomodoro).  While active,
//! `Escape` exits focus mode (instead of quitting the app).  Clicking the ring
//! pauses / resumes the countdown; the small `×` in the card corner exits.
//!
//! All timing math lives in [`PomodoroTimer`] and is driven by a monotonic
//! wall-clock millisecond timestamp fed from the shell's `about_to_wait` loop,
//! so it can be unit-tested without a real clock.

use lumen_core::geom::Rect;
use lumen_layout::{Color, FontStyle, FontWeight};
use lumen_paint::{CornerRadii, DisplayCommand, DisplayList};
use std::f32::consts::PI;

// ── Visual constants ─────────────────────────────────────────────────────────

/// Default Pomodoro work-session length in minutes.
pub const DEFAULT_POMODORO_MIN: u32 = 25;

/// Width of the focus widget card in CSS px.
const CARD_W: f32 = 150.0;

/// Height of the focus widget card in CSS px.
const CARD_H: f32 = 168.0;

/// Margin from the top-right window corner to the card in CSS px.
const CARD_MARGIN: f32 = 14.0;

/// Outer radius of the progress ring in CSS px.
const RING_OUTER: f32 = 46.0;

/// Inner radius of the progress ring in CSS px (ring thickness = outer − inner).
const RING_INNER: f32 = 37.0;

/// Vertical offset of the ring centre from the card top in CSS px.
const RING_CY_OFFSET: f32 = 62.0;

/// Number of triangle segments used to tessellate the full ring (a sweep of
/// `progress` covers `progress * SEGMENTS` of them).
const RING_SEGMENTS: usize = 96;

/// Side length of the square `×` exit hit-zone in the card's top-right corner.
const CLOSE_W: f32 = 22.0;

const CARD_BG: Color = Color { r: 18, g: 18, b: 24, a: 235 };
const CARD_BORDER: Color = Color { r: 46, g: 46, b: 58, a: 255 };
const RING_TRACK: Color = Color { r: 44, g: 44, b: 56, a: 255 };
const RING_FILL: Color = Color { r: 240, g: 96, b: 84, a: 255 };
const RING_FILL_PAUSED: Color = Color { r: 150, g: 150, b: 160, a: 255 };
const RING_FILL_DONE: Color = Color { r: 90, g: 200, b: 110, a: 255 };
const TEXT_TIME: Color = Color { r: 232, g: 232, b: 240, a: 255 };
const TEXT_DIM: Color = Color { r: 150, g: 150, b: 160, a: 255 };
const CLOSE_FG: Color = Color { r: 180, g: 90, b: 90, a: 255 };

const CARD_RADIUS: f32 = 12.0;

// ── Pomodoro timer ────────────────────────────────────────────────────────────

/// Wall-clock-driven countdown timer.
///
/// The shell calls [`tick`](Self::tick) each frame with a monotonic millisecond
/// timestamp; the timer accumulates [`elapsed_ms`](Self::elapsed_ms) only while
/// [`running`](Self::running).  Pausing resets the per-tick baseline so paused
/// spans are excluded from the count.
pub struct PomodoroTimer {
    /// Total work-session duration in milliseconds.
    pub duration_ms: f64,
    /// Accumulated running time in milliseconds (excludes paused spans), capped
    /// at [`duration_ms`](Self::duration_ms).
    pub elapsed_ms: f64,
    /// `true` while counting down; `false` while paused.
    pub running: bool,
    /// Wall-clock timestamp (ms) of the previous tick, or `None` before the
    /// first tick after start / resume.  The first tick only records the
    /// baseline so a long idle gap before the panel opens is not counted.
    last_tick_ms: Option<f64>,
}

impl PomodoroTimer {
    /// Create a running timer of `duration_min` minutes with zero elapsed time.
    pub fn new(duration_min: u32) -> Self {
        Self {
            duration_ms: f64::from(duration_min) * 60_000.0,
            elapsed_ms: 0.0,
            running: true,
            last_tick_ms: None,
        }
    }

    /// Advance the timer to wall-clock `now_ms`.  Adds the delta since the last
    /// tick to [`elapsed_ms`](Self::elapsed_ms) while running; the first tick
    /// after start / resume only records the baseline.
    pub fn tick(&mut self, now_ms: f64) {
        if self.running
            && let Some(prev) = self.last_tick_ms
        {
            let dt = (now_ms - prev).max(0.0);
            self.elapsed_ms = (self.elapsed_ms + dt).min(self.duration_ms);
        }
        self.last_tick_ms = Some(now_ms);
    }

    /// Remaining time in milliseconds, clamped to `>= 0`.
    pub fn remaining_ms(&self) -> f64 {
        (self.duration_ms - self.elapsed_ms).max(0.0)
    }

    /// Elapsed fraction in `[0, 1]`.  Returns `1.0` for a zero-length duration.
    pub fn progress(&self) -> f32 {
        if self.duration_ms <= 0.0 {
            return 1.0;
        }
        (self.elapsed_ms / self.duration_ms).clamp(0.0, 1.0) as f32
    }

    /// `true` once the full duration has elapsed.
    pub fn is_finished(&self) -> bool {
        self.elapsed_ms >= self.duration_ms
    }

    /// Pause counting.  Clears the tick baseline so the paused span is excluded.
    pub fn pause(&mut self) {
        self.running = false;
        self.last_tick_ms = None;
    }

    /// Resume counting.  Clears the tick baseline so the gap before the next
    /// tick is not counted.
    pub fn resume(&mut self) {
        self.running = true;
        self.last_tick_ms = None;
    }

    /// Flip between paused and running.
    pub fn toggle_pause(&mut self) {
        if self.running {
            self.pause();
        } else {
            self.resume();
        }
    }

    /// Remaining time formatted as `MM:SS` (rounded up to whole seconds).
    pub fn label(&self) -> String {
        let total_s = (self.remaining_ms() / 1000.0).ceil() as u64;
        let m = total_s / 60;
        let s = total_s % 60;
        format!("{m:02}:{s:02}")
    }
}

// ── Panel state ───────────────────────────────────────────────────────────────

/// Focus-mode panel state: the active flag plus the embedded [`PomodoroTimer`].
pub struct FocusModePanel {
    /// `true` while focus mode is engaged (chrome hidden, widget shown).
    pub active: bool,
    /// The countdown timer; reset on every [`enter`](Self::enter).
    pub timer: PomodoroTimer,
}

impl FocusModePanel {
    /// Create an inactive panel with a default-length (paused-at-zero) timer.
    pub fn new() -> Self {
        Self {
            active: false,
            timer: PomodoroTimer::new(DEFAULT_POMODORO_MIN),
        }
    }

    /// Enter focus mode with a fresh `duration_min`-minute timer.
    pub fn enter(&mut self, duration_min: u32) {
        self.active = true;
        self.timer = PomodoroTimer::new(duration_min);
    }

    /// Leave focus mode (the timer state is kept but no longer ticked).
    pub fn exit(&mut self) {
        self.active = false;
    }

    /// Toggle focus mode: enter with `duration_min` when off, else exit.
    pub fn toggle(&mut self, duration_min: u32) {
        if self.active {
            self.exit();
        } else {
            self.enter(duration_min);
        }
    }

    /// Advance the embedded timer to `now_ms` when active (no-op otherwise).
    pub fn tick(&mut self, now_ms: f64) {
        if self.active {
            self.timer.tick(now_ms);
        }
    }
}

impl Default for FocusModePanel {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hit-testing ───────────────────────────────────────────────────────────────

/// Result of a click inside the focus widget card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusHit {
    /// Clicked the ring / card body — pause or resume the countdown.
    TogglePause,
    /// Clicked the `×` corner button — exit focus mode.
    Exit,
}

/// Top-left corner `(x, y)` of the focus card for window width `window_w`.
fn card_origin(window_w: f32) -> (f32, f32) {
    (window_w - CARD_MARGIN - CARD_W, CARD_MARGIN)
}

/// Hit-test a click at CSS-px `(x, y)` against the focus widget card.
///
/// Returns `None` when the click is outside the card so it falls through to the
/// page.  `window_w` is the full window width in CSS px.
pub fn hit_test(panel: &FocusModePanel, x: f32, y: f32, window_w: f32) -> Option<FocusHit> {
    if !panel.active {
        return None;
    }
    let (cx0, cy0) = card_origin(window_w);
    if x < cx0 || x >= cx0 + CARD_W || y < cy0 || y >= cy0 + CARD_H {
        return None;
    }
    // `×` exit zone — top-right corner of the card.
    let close_left = cx0 + CARD_W - CLOSE_W;
    if x >= close_left && y < cy0 + CLOSE_W {
        return Some(FocusHit::Exit);
    }
    Some(FocusHit::TogglePause)
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Build the display list for the focus widget overlay.
///
/// Rendered as a top-right floating card containing the arc progress ring and
/// the remaining-time label.  `window_w` is the full window width in CSS px.
pub fn build_panel(panel: &FocusModePanel, window_w: f32) -> DisplayList {
    let mut out = DisplayList::with_capacity(8);
    if !panel.active {
        return out;
    }

    let (cx0, cy0) = card_origin(window_w);
    let radii = CornerRadii {
        tl: CARD_RADIUS, tl_y: CARD_RADIUS,
        tr: CARD_RADIUS, tr_y: CARD_RADIUS,
        br: CARD_RADIUS, br_y: CARD_RADIUS,
        bl: CARD_RADIUS, bl_y: CARD_RADIUS,
    };

    // Card background + 1px border (drawn as a slightly larger filled rect).
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(cx0 - 1.0, cy0 - 1.0, CARD_W + 2.0, CARD_H + 2.0),
        radii,
        color: CARD_BORDER,
    });
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(cx0, cy0, CARD_W, CARD_H),
        radii,
        color: CARD_BG,
    });

    // Progress ring.
    let ring_cx = cx0 + CARD_W * 0.5;
    let ring_cy = cy0 + RING_CY_OFFSET;
    let progress = panel.timer.progress();
    let fill = if panel.timer.is_finished() {
        RING_FILL_DONE
    } else if panel.timer.running {
        RING_FILL
    } else {
        RING_FILL_PAUSED
    };

    // Full track ring (background).
    out.push(DisplayCommand::DrawSvgPath {
        vertices: ring_triangles(ring_cx, ring_cy, RING_OUTER, RING_INNER, 1.0),
        color: RING_TRACK,
    });
    // Filled portion (clockwise from 12 o'clock).
    if progress > 0.0 {
        out.push(DisplayCommand::DrawSvgPath {
            vertices: ring_triangles(ring_cx, ring_cy, RING_OUTER, RING_INNER, progress),
            color: fill,
        });
    }

    // Remaining-time label, centred in the ring.
    let time = panel.timer.label();
    let time_sz = 24.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(ring_cx - RING_OUTER, ring_cy - time_sz * 0.55, RING_OUTER * 2.0, time_sz * 1.2),
        text: time,
        font_size: time_sz,
        color: TEXT_TIME,
        font_family: Vec::new(),
        font_weight: FontWeight::BOLD,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // Status line below the ring: "Focus" / "Paused" / "Done".
    let status = if panel.timer.is_finished() {
        "Done"
    } else if panel.timer.running {
        "Focus"
    } else {
        "Paused"
    };
    let status_sz = 11.0;
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(cx0, cy0 + RING_CY_OFFSET + RING_OUTER + 12.0, CARD_W, status_sz * 1.4),
        text: status.to_owned(),
        font_size: status_sz,
        color: TEXT_DIM,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // Hint line.
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(cx0, cy0 + CARD_H - status_sz * 1.8, CARD_W, status_sz * 1.4),
        text: "click: pause  ·  Esc: exit".to_owned(),
        font_size: 9.0,
        color: TEXT_DIM,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    // `×` exit glyph, top-right corner.
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(cx0 + CARD_W - CLOSE_W, cy0 + 3.0, CLOSE_W, 16.0),
        text: "×".to_owned(),
        font_size: 14.0,
        color: CLOSE_FG,
        font_family: Vec::new(),
        font_weight: FontWeight::NORMAL,
        font_style: FontStyle::Normal,
        font_variation_axes: Vec::new(),
        tab_size: 0.0,
    });

    out
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Tessellate an annulus sector (ring arc) into a flat triangle list.
///
/// The sweep starts at 12 o'clock and goes clockwise covering `fraction` of the
/// full circle (`fraction` clamped to `[0, 1]`).  `r_outer`/`r_inner` are the
/// outer/inner radii in CSS px; each ring segment becomes two triangles so the
/// returned `Vec` length is always a multiple of 3.
fn ring_triangles(cx: f32, cy: f32, r_outer: f32, r_inner: f32, fraction: f32) -> Vec<[f32; 2]> {
    let fraction = fraction.clamp(0.0, 1.0);
    let segments = ((RING_SEGMENTS as f32 * fraction).ceil() as usize).max(1);
    let total_angle = 2.0 * PI * fraction;
    // 12 o'clock = -90° in screen space (y grows downward), clockwise sweep.
    let start = -PI * 0.5;

    let mut verts = Vec::with_capacity(segments * 6);
    for i in 0..segments {
        let a0 = start + total_angle * (i as f32 / segments as f32);
        let a1 = start + total_angle * ((i + 1) as f32 / segments as f32);
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();

        let outer0 = [cx + c0 * r_outer, cy + s0 * r_outer];
        let outer1 = [cx + c1 * r_outer, cy + s1 * r_outer];
        let inner0 = [cx + c0 * r_inner, cy + s0 * r_inner];
        let inner1 = [cx + c1 * r_inner, cy + s1 * r_inner];

        // Two triangles per quad: (outer0, outer1, inner1) and (outer0, inner1, inner0).
        verts.push(outer0);
        verts.push(outer1);
        verts.push(inner1);
        verts.push(outer0);
        verts.push(inner1);
        verts.push(inner0);
    }
    verts
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_W: f32 = 1024.0;

    // ── Timer ──────────────────────────────────────────────────────────────────

    #[test]
    fn new_timer_full_remaining() {
        let t = PomodoroTimer::new(25);
        assert_eq!(t.duration_ms, 25.0 * 60_000.0);
        assert_eq!(t.remaining_ms(), 25.0 * 60_000.0);
        assert_eq!(t.progress(), 0.0);
        assert!(!t.is_finished());
    }

    #[test]
    fn first_tick_does_not_count() {
        let mut t = PomodoroTimer::new(1);
        t.tick(1000.0);
        // Only baseline recorded; no elapsed yet.
        assert_eq!(t.elapsed_ms, 0.0);
    }

    #[test]
    fn tick_accumulates_delta() {
        let mut t = PomodoroTimer::new(1);
        t.tick(1000.0);
        t.tick(1500.0);
        assert!((t.elapsed_ms - 500.0).abs() < 1e-6);
    }

    #[test]
    fn elapsed_capped_at_duration() {
        let mut t = PomodoroTimer::new(1); // 60_000 ms
        t.tick(0.0);
        t.tick(120_000.0);
        assert_eq!(t.elapsed_ms, 60_000.0);
        assert_eq!(t.remaining_ms(), 0.0);
        assert!(t.is_finished());
        assert_eq!(t.progress(), 1.0);
    }

    #[test]
    fn paused_does_not_accumulate() {
        let mut t = PomodoroTimer::new(1);
        t.tick(0.0);
        t.tick(1000.0);
        assert!((t.elapsed_ms - 1000.0).abs() < 1e-6);
        t.pause();
        t.tick(2000.0);
        t.tick(5000.0);
        // No accumulation while paused.
        assert!((t.elapsed_ms - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn resume_skips_paused_gap() {
        let mut t = PomodoroTimer::new(1);
        t.tick(0.0);
        t.tick(1000.0); // elapsed = 1000
        t.pause();
        t.tick(10_000.0); // paused, ignored
        t.resume();
        t.tick(11_000.0); // baseline after resume
        t.tick(12_000.0); // +1000
        assert!((t.elapsed_ms - 2000.0).abs() < 1e-6, "got {}", t.elapsed_ms);
    }

    #[test]
    fn toggle_pause_flips_running() {
        let mut t = PomodoroTimer::new(1);
        assert!(t.running);
        t.toggle_pause();
        assert!(!t.running);
        t.toggle_pause();
        assert!(t.running);
    }

    #[test]
    fn label_formats_mm_ss() {
        let mut t = PomodoroTimer::new(25);
        assert_eq!(t.label(), "25:00");
        t.tick(0.0);
        t.tick(90_000.0); // 1m30s elapsed → 23:30 remaining
        assert_eq!(t.label(), "23:30");
    }

    #[test]
    fn progress_midway() {
        let mut t = PomodoroTimer::new(1);
        t.tick(0.0);
        t.tick(30_000.0);
        assert!((t.progress() - 0.5).abs() < 1e-4);
    }

    // ── Panel state ─────────────────────────────────────────────────────────────

    #[test]
    fn new_panel_inactive() {
        let p = FocusModePanel::new();
        assert!(!p.active);
    }

    #[test]
    fn enter_activates_and_resets() {
        let mut p = FocusModePanel::new();
        p.enter(10);
        assert!(p.active);
        assert_eq!(p.timer.duration_ms, 10.0 * 60_000.0);
        assert_eq!(p.timer.elapsed_ms, 0.0);
    }

    #[test]
    fn exit_deactivates() {
        let mut p = FocusModePanel::new();
        p.enter(10);
        p.exit();
        assert!(!p.active);
    }

    #[test]
    fn toggle_enters_then_exits() {
        let mut p = FocusModePanel::new();
        p.toggle(5);
        assert!(p.active);
        p.toggle(5);
        assert!(!p.active);
    }

    #[test]
    fn tick_noop_when_inactive() {
        let mut p = FocusModePanel::new();
        p.tick(1000.0);
        p.tick(2000.0);
        assert_eq!(p.timer.elapsed_ms, 0.0);
    }

    #[test]
    fn tick_advances_when_active() {
        let mut p = FocusModePanel::new();
        p.enter(1);
        p.tick(0.0);
        p.tick(1000.0);
        assert!((p.timer.elapsed_ms - 1000.0).abs() < 1e-6);
    }

    // ── Hit-testing ──────────────────────────────────────────────────────────────

    #[test]
    fn hit_inactive_returns_none() {
        let p = FocusModePanel::new();
        let (cx0, cy0) = card_origin(WIN_W);
        assert_eq!(hit_test(&p, cx0 + 10.0, cy0 + 60.0, WIN_W), None);
    }

    #[test]
    fn hit_outside_card_none() {
        let mut p = FocusModePanel::new();
        p.enter(25);
        assert_eq!(hit_test(&p, 10.0, 10.0, WIN_W), None);
    }

    #[test]
    fn hit_close_zone_exit() {
        let mut p = FocusModePanel::new();
        p.enter(25);
        let (cx0, cy0) = card_origin(WIN_W);
        let x = cx0 + CARD_W - CLOSE_W * 0.5;
        let y = cy0 + CLOSE_W * 0.5;
        assert_eq!(hit_test(&p, x, y, WIN_W), Some(FocusHit::Exit));
    }

    #[test]
    fn hit_ring_toggle_pause() {
        let mut p = FocusModePanel::new();
        p.enter(25);
        let (cx0, cy0) = card_origin(WIN_W);
        let x = cx0 + CARD_W * 0.5;
        let y = cy0 + RING_CY_OFFSET;
        assert_eq!(hit_test(&p, x, y, WIN_W), Some(FocusHit::TogglePause));
    }

    // ── Rendering ────────────────────────────────────────────────────────────────

    #[test]
    fn build_panel_inactive_empty() {
        let p = FocusModePanel::new();
        let dl = build_panel(&p, WIN_W);
        assert!(dl.is_empty());
    }

    #[test]
    fn build_panel_active_emits_commands() {
        let mut p = FocusModePanel::new();
        p.enter(25);
        let dl = build_panel(&p, WIN_W);
        assert!(!dl.is_empty());
    }

    #[test]
    fn build_panel_draws_time_label() {
        let mut p = FocusModePanel::new();
        p.enter(25);
        let dl = build_panel(&p, WIN_W);
        let has_time = dl.iter().any(|c| {
            matches!(c, DisplayCommand::DrawText { text, .. } if text == "25:00")
        });
        assert!(has_time, "panel must draw the MM:SS label");
    }

    #[test]
    fn build_panel_emits_ring_path() {
        let mut p = FocusModePanel::new();
        p.enter(25);
        let dl = build_panel(&p, WIN_W);
        let ring_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawSvgPath { .. }))
            .count();
        // Track ring is always drawn; the fill ring only when progress > 0.
        assert!(ring_count >= 1, "panel must draw at least the track ring");
    }

    #[test]
    fn build_panel_progressed_emits_two_rings() {
        let mut p = FocusModePanel::new();
        p.enter(1);
        p.tick(0.0);
        p.tick(30_000.0); // 50% progress
        let dl = build_panel(&p, WIN_W);
        let ring_count = dl
            .iter()
            .filter(|c| matches!(c, DisplayCommand::DrawSvgPath { .. }))
            .count();
        assert_eq!(ring_count, 2, "track + fill rings expected once progress > 0");
    }

    // ── Ring tessellation ────────────────────────────────────────────────────────

    #[test]
    fn ring_triangles_multiple_of_three() {
        let v = ring_triangles(100.0, 100.0, 40.0, 30.0, 1.0);
        assert_eq!(v.len() % 3, 0);
        assert!(!v.is_empty());
    }

    #[test]
    fn ring_triangles_zero_fraction_minimal() {
        // Clamped to at least one segment so it never produces an empty list.
        let v = ring_triangles(100.0, 100.0, 40.0, 30.0, 0.0);
        assert_eq!(v.len(), 6);
    }

    #[test]
    fn ring_triangles_partial_fewer_than_full() {
        let full = ring_triangles(100.0, 100.0, 40.0, 30.0, 1.0);
        let half = ring_triangles(100.0, 100.0, 40.0, 30.0, 0.5);
        assert!(half.len() < full.len());
    }
}

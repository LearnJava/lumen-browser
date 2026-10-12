//! Focus mode + Pomodoro timer panel (V4, task #25).
//!
//! Focus mode shows a compact Pomodoro countdown pill in the top-right corner
//! (`.focus-timer` in the engine chrome, `bind_focus`, UX-CHROME-PANELS) with
//! the remaining time as `MM:SS`, a pause/resume button
//! (`toggle-focus-timer`) and an exit button (`toggle-focus`); a green banner
//! (`#focusDone`) appears when the session is complete.
//!
//! Toggled with `Ctrl+Shift+F` (default 25-minute Pomodoro).  While active,
//! `Escape` exits focus mode (never quits the app).
//!
//! All timing math lives in [`PomodoroTimer`] and is driven by a monotonic
//! wall-clock millisecond timestamp fed from the shell's `about_to_wait` loop,
//! so it can be unit-tested without a real clock.

use lumen_chrome::ChromeFocusModel;

/// Default Pomodoro work-session length in minutes.
pub const DEFAULT_POMODORO_MIN: u32 = 25;

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
    /// Label last pushed into the chrome (`#focusTime`); see
    /// [`FocusModePanel::label_changed`].
    bound_label: String,
}

impl FocusModePanel {
    /// Create an inactive panel with a default-length (paused-at-zero) timer.
    pub fn new() -> Self {
        Self {
            active: false,
            timer: PomodoroTimer::new(DEFAULT_POMODORO_MIN),
            bound_label: String::new(),
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

impl FocusModePanel {
    /// `.focus-timer` snapshot (UX-CHROME-PANELS).
    pub fn chrome_model(&self) -> ChromeFocusModel {
        ChromeFocusModel {
            active: self.active,
            running: self.timer.running,
            finished: self.timer.is_finished(),
            label: self.timer.label(),
        }
    }

    /// `true` when the countdown label differs from the last one the chrome was
    /// bound with (once per second while running), so the shell relayouts the
    /// chrome only when the pill's text actually changes.
    pub fn label_changed(&mut self) -> bool {
        let label = self.timer.label();
        if label == self.bound_label {
            return false;
        }
        self.bound_label = label;
        true
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Timer ──────────────────────────────────────────────────────────────────

    #[test]
    fn new_timer_full_remaining() {
        let t = PomodoroTimer::new(25);
        assert_eq!(t.duration_ms, 25.0 * 60_000.0);
        assert_eq!(t.remaining_ms(), 25.0 * 60_000.0);
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

    // ── Chrome model ────────────────────────────────────────────────────────────

    #[test]
    fn chrome_model_reflects_state() {
        let mut p = FocusModePanel::new();
        assert!(!p.chrome_model().active);
        p.enter(1);
        let m = p.chrome_model();
        assert!(m.active && m.running && !m.finished);
        assert_eq!(m.label, "01:00");
        p.timer.toggle_pause();
        assert!(!p.chrome_model().running);
        p.timer.elapsed_ms = p.timer.duration_ms;
        assert!(p.chrome_model().finished);
    }

    #[test]
    fn label_changed_fires_once_per_new_label() {
        let mut p = FocusModePanel::new();
        p.enter(1);
        assert!(p.label_changed());
        assert!(!p.label_changed());
        p.timer.tick(0.0);
        p.timer.tick(2_000.0);
        assert!(p.label_changed());
    }
}

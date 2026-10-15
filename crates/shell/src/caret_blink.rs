//! UX-CARET: the text caret's blink clock — one model for every caret the
//! shell paints (page `<input>`/`<textarea>`, frames, the address bar).
//!
//! The rhythm comes from the OS ([`OsParams::caret_blink_ms`], the length of one
//! phase), the caret goes solid after [`OsParams::caret_timeout_ms`] without
//! input (and stays visible), and any input restarts the visible phase. The
//! clock is pure — `now` is a parameter — so the rules are unit-tested without
//! a window.

use std::time::{Duration, Instant};

use crate::platform::os_params::OsParams;

/// Restartable blink phase. `signature` is the caret's observable state
/// (focused field, position, value): a change of it is an edit or a caret
/// move and restarts the phase, so the caret is visible in the very frame that
/// shows the result.
#[derive(Debug, Clone)]
pub(crate) struct CaretBlink {
    phase_start: Instant,
    signature: u64,
    /// Visibility of the last painted frame; the wake-up asks for a repaint
    /// only when the phase actually flipped.
    painted: bool,
}

impl CaretBlink {
    pub(crate) fn new(now: Instant) -> Self {
        Self { phase_start: now, signature: 0, painted: true }
    }

    /// Restarts the visible phase (key press, click, focus gained).
    pub(crate) fn restart(&mut self, now: Instant) {
        self.phase_start = now;
    }

    /// Feeds the caret's current signature; a different one restarts the phase.
    pub(crate) fn observe(&mut self, signature: u64, now: Instant) {
        if signature != self.signature {
            self.signature = signature;
            self.phase_start = now;
        }
    }

    /// Whether the bar is drawn at `now`. A caret that does not blink, or one
    /// past the stop timeout, is always visible.
    pub(crate) fn visible(&self, now: Instant, os: &OsParams) -> bool {
        let Some(phase) = os.caret_blink_ms else { return true };
        let elapsed = now.saturating_duration_since(self.phase_start);
        if os.caret_timeout_ms.is_some_and(|t| elapsed >= Duration::from_millis(u64::from(t))) {
            return true;
        }
        let phase_ms = u128::from(phase.max(1));
        (elapsed.as_millis() / phase_ms).is_multiple_of(2)
    }

    /// The next instant at which [`Self::visible`] may change; `None` while the
    /// caret is solid for good (no blinking, or already past the timeout).
    pub(crate) fn next_change(&self, now: Instant, os: &OsParams) -> Option<Instant> {
        let phase = u64::from(os.caret_blink_ms?.max(1));
        let elapsed = now.saturating_duration_since(self.phase_start);
        let stop = os.caret_timeout_ms.map(|t| Duration::from_millis(u64::from(t)));
        if stop.is_some_and(|s| elapsed >= s) {
            return None;
        }
        let idx = elapsed.as_millis() as u64 / phase;
        let mut next = Duration::from_millis((idx + 1) * phase);
        if let Some(s) = stop {
            next = next.min(s);
        }
        Some(self.phase_start + next)
    }

    /// Records what the frame being painted shows; `true` iff that differs
    /// from the previous frame (a repaint was due).
    pub(crate) fn note_painted(&mut self, visible: bool) -> bool {
        std::mem::replace(&mut self.painted, visible) != visible
    }

    /// Whether the visibility computed for `now` differs from the last painted one.
    pub(crate) fn repaint_due(&self, now: Instant, os: &OsParams) -> bool {
        self.visible(now, os) != self.painted
    }
}

/// Caret bar width in the layout units of a surface whose CSS px are
/// `device_px_per_css` physical pixels: the OS width scaled by the monitor
/// DPI and rounded to whole physical pixels, never thinner than one — page
/// zoom and `transform: scale()` must not make the caret vanish (CARET-06).
pub(crate) fn caret_width_css(os: &OsParams, dpi_scale: f32, device_px_per_css: f32) -> f32 {
    let physical = (os.caret_width_px * dpi_scale.max(0.1)).round().max(1.0);
    physical / device_px_per_css.max(0.01)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(blink: Option<u32>, timeout: Option<u32>) -> OsParams {
        OsParams { caret_blink_ms: blink, caret_timeout_ms: timeout, ..OsParams::DEFAULT }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn phases_alternate_every_blink_period() {
        let t0 = Instant::now();
        let c = CaretBlink::new(t0);
        let p = os(Some(530), None);
        assert!(c.visible(t0, &p));
        assert!(c.visible(t0 + ms(529), &p));
        assert!(!c.visible(t0 + ms(530), &p));
        assert!(!c.visible(t0 + ms(1059), &p));
        assert!(c.visible(t0 + ms(1060), &p));
    }

    #[test]
    fn no_blinking_is_always_visible_and_never_wakes() {
        let t0 = Instant::now();
        let c = CaretBlink::new(t0);
        let p = os(None, None);
        for t in [0, 600, 5_000, 90_000] {
            assert!(c.visible(t0 + ms(t), &p));
        }
        assert_eq!(c.next_change(t0, &p), None);
    }

    #[test]
    fn stops_visible_after_timeout() {
        let t0 = Instant::now();
        let c = CaretBlink::new(t0);
        let p2 = os(Some(530), Some(5_000));
        // 4_900 / 530 = phase 9: hidden until the timeout makes it solid.
        assert!(!c.visible(t0 + ms(4_900), &p2));
        assert!(c.visible(t0 + ms(5_000), &p2));
        assert!(c.visible(t0 + ms(60_000), &p2));
        assert_eq!(c.next_change(t0 + ms(5_000), &p2), None);
    }

    #[test]
    fn timeout_inside_a_hidden_phase_wakes_at_the_timeout() {
        let t0 = Instant::now();
        let c = CaretBlink::new(t0);
        // phases: [0,700) on, [700,1400) off, [1400,2100) on, [2100,2800) off;
        // the caret goes solid at 2400, inside an "off" phase.
        let p = os(Some(700), Some(2_400));
        assert!(!c.visible(t0 + ms(2_200), &p));
        assert_eq!(c.next_change(t0 + ms(2_200), &p), Some(t0 + ms(2_400)));
        assert!(c.visible(t0 + ms(2_400), &p));
    }

    #[test]
    fn next_change_is_the_next_phase_boundary() {
        let t0 = Instant::now();
        let c = CaretBlink::new(t0);
        let p = os(Some(500), None);
        assert_eq!(c.next_change(t0, &p), Some(t0 + ms(500)));
        assert_eq!(c.next_change(t0 + ms(499), &p), Some(t0 + ms(500)));
        assert_eq!(c.next_change(t0 + ms(500), &p), Some(t0 + ms(1_000)));
    }

    #[test]
    fn input_restarts_the_visible_phase() {
        let t0 = Instant::now();
        let mut c = CaretBlink::new(t0);
        let p = os(Some(500), None);
        c.observe(7, t0);
        assert!(!c.visible(t0 + ms(600), &p));
        // Typing: the signature moves, the caret is visible immediately.
        c.observe(8, t0 + ms(600));
        assert!(c.visible(t0 + ms(600), &p));
        assert!(c.visible(t0 + ms(1_099), &p));
        // The same signature does not restart.
        c.observe(8, t0 + ms(1_099));
        assert!(!c.visible(t0 + ms(1_100), &p));
        // An explicit restart (a click on the same spot) does.
        c.restart(t0 + ms(1_100));
        assert!(c.visible(t0 + ms(1_100), &p));
    }

    #[test]
    fn typing_faster_than_a_phase_never_hides_the_caret() {
        let t0 = Instant::now();
        let mut c = CaretBlink::new(t0);
        let p = os(Some(500), None);
        for i in 1..40u64 {
            let t = t0 + ms(i * 200);
            c.observe(i, t);
            assert!(c.visible(t, &p), "keystroke {i}");
            assert!(c.visible(t + ms(199), &p), "just before keystroke {}", i + 1);
        }
    }

    #[test]
    fn repaint_is_due_only_when_the_phase_flipped() {
        let t0 = Instant::now();
        let mut c = CaretBlink::new(t0);
        let p = os(Some(500), None);
        assert!(!c.repaint_due(t0 + ms(100), &p));
        assert!(c.repaint_due(t0 + ms(500), &p));
        assert!(c.note_painted(false));
        assert!(!c.repaint_due(t0 + ms(700), &p));
        assert!(!c.note_painted(false));
    }

    #[test]
    fn width_follows_the_os_and_dpi_but_never_below_one_physical_pixel() {
        let p = OsParams { caret_width_px: 10.0, ..OsParams::DEFAULT };
        // 10 px at 150% → 15 physical px; page at 100% zoom = 15 / 1.5 css px.
        assert!((caret_width_css(&p, 1.5, 1.5) - 10.0).abs() < 1e-4);
        let thin = OsParams::DEFAULT;
        // 1 px at 100% DPI and 33% zoom: still one physical pixel = 3 css px.
        let w = caret_width_css(&thin, 1.0, 0.33);
        assert!((w * 0.33 - 1.0).abs() < 1e-4, "{w}");
        // Fractional DPI rounds to whole physical pixels: 1 × 1.25 → 1.
        assert!((caret_width_css(&thin, 1.25, 1.25) - 0.8).abs() < 1e-4);
    }
}

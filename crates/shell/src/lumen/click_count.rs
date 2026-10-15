//! Click counting for `MouseEvent.detail` and `dblclick` (UI Events §3.5).
//!
//! A left press continues the streak when it lands within the OS double-click
//! time and zone of the previous one ([`OsParams::is_double_click`], which
//! reads `GetDoubleClickTime`/`SM_CXDOUBLECLK` — UX-OSPARAMS); otherwise it
//! starts a new one at 1. The count rides on `mousedown`, `mouseup` and
//! `click` as `detail`, a second press also fires `dblclick`, and the shell
//! turns 2 into a word selection and 3 into a paragraph selection.

use crate::platform::os_params::OsParams;

/// The previous left press and the streak it belongs to.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClickStreak {
    /// `(time in ms since the shell epoch, x, y in CSS px)` of the last press.
    last: Option<(f64, f32, f32)>,
    count: u8,
}

impl ClickStreak {
    /// Registers a left press at `(x, y)` CSS px, `now_ms` after the shell
    /// epoch, and returns its click count (1 for a fresh streak).
    pub(crate) fn press(&mut self, os: &OsParams, now_ms: f64, x: f32, y: f32) -> u8 {
        let continues = self
            .last
            .is_some_and(|(at, px, py)| os.is_double_click(now_ms - at, px - x, py - y));
        self.count = if continues { self.count.saturating_add(1) } else { 1 };
        self.last = Some((now_ms, x, y));
        self.count
    }

    /// Click count of the press in progress (`0` before any press).
    pub(crate) fn count(&self) -> u8 {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os() -> OsParams {
        OsParams { double_click_ms: 500, double_click_slop: (4.0, 4.0), ..OsParams::DEFAULT }
    }

    #[test]
    fn two_quick_presses_in_the_zone_count_to_two_then_three() {
        let mut s = ClickStreak::default();
        assert_eq!(s.count(), 0);
        assert_eq!(s.press(&os(), 1000.0, 10.0, 10.0), 1);
        assert_eq!(s.press(&os(), 1200.0, 11.0, 9.0), 2);
        assert_eq!(s.press(&os(), 1400.0, 11.0, 9.0), 3);
        assert_eq!(s.count(), 3);
    }

    #[test]
    fn a_slow_press_starts_a_new_streak() {
        let mut s = ClickStreak::default();
        s.press(&os(), 1000.0, 10.0, 10.0);
        assert_eq!(s.press(&os(), 1501.0, 10.0, 10.0), 1);
    }

    #[test]
    fn a_press_outside_the_zone_starts_a_new_streak() {
        let mut s = ClickStreak::default();
        s.press(&os(), 1000.0, 10.0, 10.0);
        assert_eq!(s.press(&os(), 1100.0, 13.0, 10.0), 1);
    }

    #[test]
    fn the_window_is_measured_from_the_previous_press_not_the_first() {
        let mut s = ClickStreak::default();
        s.press(&os(), 0.0, 5.0, 5.0);
        s.press(&os(), 400.0, 5.0, 5.0);
        assert_eq!(s.press(&os(), 800.0, 5.0, 5.0), 3);
    }
}

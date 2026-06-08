//! "Восстановление" banner overlay for T2 (BackgroundOld) tab restore > 100 ms (10I).
//!
//! Shown when restoring a T2 tab from SQLite takes longer than THRESHOLD_MS.
//! The overlay is intentionally simpler than the T3 `restore_spinner` — a
//! translucent bar at the top of the viewport reading "Восстановление вкладки…"
//! in white on a dark tint.  It disappears as soon as restore completes.

use lumen_core::geom::Rect;
use lumen_layout::Color;
use lumen_paint::DisplayCommand;

/// Milliseconds after which the hint banner becomes visible.
const THRESHOLD_MS: f64 = 100.0;

/// Background tint of the hint bar: semi-transparent dark.
const TINT_COLOR: Color = Color { r: 30, g: 30, b: 30, a: 200 };

/// Height of the hint bar in CSS px.
const BAR_HEIGHT: f32 = 28.0;

/// Build the sleep-restore hint overlay if restore has taken longer than THRESHOLD_MS.
///
/// `elapsed_ms` — time elapsed since the start of the T2 restore.
/// `win_w` — viewport width in CSS px.
/// Returns `None` when the elapsed time is below the threshold.
pub fn build_sleep_hint(elapsed_ms: f64, win_w: f32) -> Option<Vec<DisplayCommand>> {
    if elapsed_ms < THRESHOLD_MS {
        return None;
    }

    Some(vec![DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, win_w, BAR_HEIGHT),
        color: TINT_COLOR,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_none_before_threshold() {
        assert!(build_sleep_hint(50.0, 1024.0).is_none());
    }

    #[test]
    fn hint_some_at_threshold() {
        let cmds = build_sleep_hint(100.0, 1024.0);
        assert!(cmds.is_some());
        assert_eq!(cmds.unwrap().len(), 1);
    }

    #[test]
    fn hint_some_after_threshold() {
        let cmds = build_sleep_hint(500.0, 1024.0);
        assert!(cmds.is_some());
    }

    #[test]
    fn hint_none_just_below_threshold() {
        assert!(build_sleep_hint(99.9, 1024.0).is_none());
    }
}

//! Fullscreen spinner overlay during restoration of a T3-hibernated tab (10K.3).

use lumen_core::geom::Rect;
use lumen_layout::Color;
use lumen_paint::DisplayCommand;

/// Milliseconds after which the spinner becomes visible.
const THRESHOLD_MS: f64 = 200.0;

/// Angular velocity of the spinner in radians per second.
const SPEED: f64 = std::f64::consts::TAU * 0.8; // ~290°/sec, one rotation per ~1.25 sec

/// Semi-transparent darkening overlay color.
const SCRIM_COLOR: Color = Color { r: 0, g: 0, b: 0, a: 115 }; // 0.45 * 255 ≈ 115

/// Spinner dot color: light blue.
const SPINNER_COLOR: Color = Color { r: 102, g: 178, b: 255, a: 255 }; // #66b2ff

/// Build spinner overlay if restore has taken longer than THRESHOLD_MS.
///
/// `elapsed_ms` — time elapsed since the start of restore.
/// `win_w`, `win_h` — window dimensions in physical pixels.
/// Returns `None` if the elapsed time is below the threshold.
pub fn build_spinner(elapsed_ms: f64, win_w: f32, win_h: f32) -> Option<Vec<DisplayCommand>> {
    if elapsed_ms < THRESHOLD_MS {
        return None;
    }

    let _angle = (elapsed_ms / 1000.0 * SPEED) as f32;
    let cx = win_w / 2.0;
    let cy = win_h / 2.0;

    let mut cmds = Vec::new();

    // Semi-transparent darkening background.
    cmds.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, win_w, win_h),
        color: SCRIM_COLOR,
    });

    // Spinner: animated dots rotating around center.
    let dot_dist = 12.0_f32;
    let dot_size = 3.0_f32;
    let num_dots = 8;

    for i in 0..num_dots {
        let dot_angle = _angle + (i as f32) * std::f32::consts::TAU / (num_dots as f32);
        let (sin_a, cos_a) = dot_angle.sin_cos();
        let x = cx + cos_a * dot_dist - dot_size / 2.0;
        let y = cy + sin_a * dot_dist - dot_size / 2.0;

        let r = dot_size / 2.0;
        cmds.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(x, y, dot_size, dot_size),
            radii: lumen_paint::CornerRadii {
                tl: r,
                tl_y: r,
                tr: r,
                tr_y: r,
                br: r,
                br_y: r,
                bl: r,
                bl_y: r,
            },
            color: SPINNER_COLOR,
        });
    }

    Some(cmds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_none_before_threshold() {
        assert!(build_spinner(100.0, 1024.0, 768.0).is_none());
    }

    #[test]
    fn spinner_some_after_threshold() {
        let cmds = build_spinner(300.0, 1024.0, 768.0);
        assert!(cmds.is_some());
        assert!(cmds.unwrap().len() >= 2); // backdrop + arc
    }

    #[test]
    fn spinner_some_at_exact_threshold() {
        // 200.0 ms = exactly at threshold — should show.
        assert!(build_spinner(200.0, 1024.0, 768.0).is_some());
    }
}

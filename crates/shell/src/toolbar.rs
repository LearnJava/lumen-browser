//! Toolbar geometry constants shared by the engine-drawn chrome (CC-4) and
//! legacy-forever panels anchored below the tab bar/toolbar strip.
//!
//! CC-15-3: the legacy paint/hit-test code (`build_toolbar`, `hit_test`,
//! `push_btn`, `push_avatar`, `ToolbarHit`, `ToolbarActive`) was removed once
//! the engine-drawn chrome became the default (CC-14) and made it dead code.
//! Only the geometry this module's former callers still read unconditionally
//! survives here.

use crate::tabs::strip::TAB_BAR_HEIGHT;
use crate::theme_tokens::size;

/// Total CSS-px height of the tab bar + toolbar stack. This is the y-origin
/// of the page content region and of every chrome panel anchored "below the
/// bars" — see `docs/tasks/p1-design-v3.md` DS-9 step 2/3.
pub const CHROME_H: f32 = TAB_BAR_HEIGHT + size::TOOLBAR_H;

/// Horizontal padding between the window edge and the outermost cluster.
const CLUSTER_PAD: f32 = 10.0;

/// Left edge x-coordinate of the profile avatar button (DS-14) — the
/// leading element of the left cluster, before the nav buttons.
pub fn avatar_x() -> f32 {
    CLUSTER_PAD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_h_is_tab_bar_plus_toolbar() {
        assert!((CHROME_H - (TAB_BAR_HEIGHT + size::TOOLBAR_H)).abs() < 1e-6);
    }
}

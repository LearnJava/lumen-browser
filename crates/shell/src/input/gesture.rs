//! Mouse gesture recognizer (§7B.3).
//!
//! Tracks right-mouse-button drags, classifies the trajectory into one of six
//! direction codes (L / R / U / D / LD / RD), and maps each code to a
//! [`GestureAction`] via a configurable [`GestureMap`].
//!
//! ## Integration
//!
//! Store a [`GestureRecognizer`] in the `Lumen` shell struct.
//!
//! 1. On `MouseButton::Right` + `ElementState::Pressed` → [`GestureRecognizer::begin`].
//! 2. On `CursorMoved` while right button is held → [`GestureRecognizer::track`].
//! 3. On `MouseButton::Right` + `ElementState::Released` → [`GestureRecognizer::finish`].
//!    Act on the returned [`GestureAction`] (if any).
//! 4. On `CursorLeft` → [`GestureRecognizer::cancel`] (clears the in-progress gesture).
//!
//! ## Classification
//!
//! The drag displacement vector `(dx, dy)` (positive = right/down) is classified:
//!
//! ```text
//! |dx| >= |dy|  AND  dy/|dx| > DIAGONAL_RATIO  AND  dy > 0  →  LD / RD
//! |dx| >= |dy|  (otherwise)                                  →  L / R
//! |dy| > |dx|                                                →  U / D
//! ```
//!
//! A minimum drag distance of [`MIN_DRAG_PX`] (30 px) prevents normal
//! right-clicks from triggering gestures.

use std::collections::HashMap;

// ── Gesture direction ─────────────────────────────────────────────────────────

/// Six-way gesture direction code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GestureDir {
    /// Predominantly leftward.
    Left,
    /// Predominantly rightward.
    Right,
    /// Predominantly upward.
    Up,
    /// Predominantly downward.
    Down,
    /// Diagonal: left and downward.
    LeftDown,
    /// Diagonal: right and downward.
    RightDown,
}

// ── Gesture action ────────────────────────────────────────────────────────────

/// Shell action emitted when a completed gesture matches a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureAction {
    /// Navigate to the previous page in history.
    NavigateBack,
    /// Navigate to the next page in history.
    NavigateForward,
    /// Close the active tab.
    CloseTab,
    /// Open a blank new tab.
    NewTab,
}

// ── Gesture map ───────────────────────────────────────────────────────────────

/// Configurable mapping from [`GestureDir`] to [`GestureAction`].
///
/// **Default bindings:**
///
/// | Direction | Action |
/// |-----------|--------|
/// | Left      | NavigateBack |
/// | Right     | NavigateForward |
/// | LeftDown  | CloseTab |
/// | RightDown | NewTab |
///
/// Up and Down are unbound by default.
#[derive(Debug, Clone)]
pub struct GestureMap(HashMap<GestureDir, GestureAction>);

impl Default for GestureMap {
    fn default() -> Self {
        let mut m = HashMap::new();
        m.insert(GestureDir::Left, GestureAction::NavigateBack);
        m.insert(GestureDir::Right, GestureAction::NavigateForward);
        m.insert(GestureDir::LeftDown, GestureAction::CloseTab);
        m.insert(GestureDir::RightDown, GestureAction::NewTab);
        Self(m)
    }
}

impl GestureMap {
    /// Empty map — no bindings.
    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self(HashMap::new())
    }

    /// Bind `dir` to `action`, replacing any previous binding.
    #[allow(dead_code)]
    pub fn bind(&mut self, dir: GestureDir, action: GestureAction) {
        self.0.insert(dir, action);
    }

    /// Remove the binding for `dir`.
    #[allow(dead_code)]
    pub fn unbind(&mut self, dir: GestureDir) {
        self.0.remove(&dir);
    }

    /// Return the action bound to `dir`, or `None` if unbound.
    pub fn lookup(&self, dir: GestureDir) -> Option<GestureAction> {
        self.0.get(&dir).copied()
    }
}

// ── Constants ─────────────────────────────────────────────────────────────────

/// Minimum Euclidean drag distance (CSS px) required to classify a gesture.
///
/// Normal right-clicks produce zero or sub-pixel movement and never cross
/// this threshold.
pub const MIN_DRAG_PX: f32 = 30.0;

/// Ratio of vertical displacement to horizontal displacement above which
/// a horizontal gesture is re-classified as diagonal (LD or RD).
///
/// Only applied when the drag is predominantly horizontal (`|dx| >= |dy|`)
/// and the vertical component is downward (`dy > 0`).
const DIAGONAL_RATIO: f32 = 0.5;

// ── Recognizer ────────────────────────────────────────────────────────────────

struct ActiveGesture {
    /// Drag start position in CSS px (viewport-relative).
    start: (f32, f32),
    /// Most-recently reported cursor position in CSS px.
    current: (f32, f32),
}

/// State machine for recognizing right-button drag mouse gestures.
///
/// Transitions: idle → active (on [`begin`]) → idle (on [`finish`] / [`cancel`]).
///
/// [`begin`]: GestureRecognizer::begin
/// [`finish`]: GestureRecognizer::finish
/// [`cancel`]: GestureRecognizer::cancel
pub struct GestureRecognizer {
    active: Option<ActiveGesture>,
    map: GestureMap,
}

impl GestureRecognizer {
    /// Create a recognizer with the default gesture map.
    pub fn new() -> Self {
        Self { active: None, map: GestureMap::default() }
    }

    /// Create a recognizer with a custom gesture map.
    #[allow(dead_code)]
    pub fn with_map(map: GestureMap) -> Self {
        Self { active: None, map }
    }

    /// Replace the gesture map at runtime (e.g. from settings).
    #[allow(dead_code)]
    pub fn set_map(&mut self, map: GestureMap) {
        self.map = map;
    }

    /// Shared reference to the current gesture map.
    #[allow(dead_code)]
    pub fn map(&self) -> &GestureMap {
        &self.map
    }

    /// Mutable reference to the current gesture map.
    #[allow(dead_code)]
    pub fn map_mut(&mut self) -> &mut GestureMap {
        &mut self.map
    }

    /// Begin tracking a right-button drag from `(x, y)` in CSS pixels.
    ///
    /// Replaces any previously active gesture (should not happen under normal
    /// OS event ordering, but handles edge cases gracefully).
    pub fn begin(&mut self, x: f32, y: f32) {
        self.active = Some(ActiveGesture { start: (x, y), current: (x, y) });
    }

    /// Update the current drag end-point.
    ///
    /// Call on every `CursorMoved` event while the right button is held.
    /// No-op when no drag is in progress.
    pub fn track(&mut self, x: f32, y: f32) {
        if let Some(ref mut g) = self.active {
            g.current = (x, y);
        }
    }

    /// Finish the drag and return the mapped [`GestureAction`], if any.
    ///
    /// Returns `None` when:
    /// - No drag was in progress.
    /// - Total drag distance is less than [`MIN_DRAG_PX`] (ordinary right-click).
    /// - The classified direction is unbound in the gesture map.
    ///
    /// Always resets the recognizer to idle.
    pub fn finish(&mut self) -> Option<GestureAction> {
        let g = self.active.take()?;
        let dx = g.current.0 - g.start.0;
        let dy = g.current.1 - g.start.1;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < MIN_DRAG_PX {
            return None;
        }
        let dir = classify(dx, dy);
        self.map.lookup(dir)
    }

    /// Cancel the in-progress drag without emitting an action.
    ///
    /// Call on `CursorLeft` or window-focus-lost events.
    pub fn cancel(&mut self) {
        self.active = None;
    }

    /// Returns `true` while a right-button drag is being tracked.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

impl Default for GestureRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Direction classifier ──────────────────────────────────────────────────────

/// Classify displacement `(dx, dy)` into one of six [`GestureDir`]s.
///
/// Screen coordinates: positive dx = right, positive dy = down.
fn classify(dx: f32, dy: f32) -> GestureDir {
    let adx = dx.abs();
    let ady = dy.abs();

    if adx >= ady {
        // Predominantly horizontal motion.
        // Check for a significant downward component → diagonal.
        if dy > 0.0 && ady / adx > DIAGONAL_RATIO {
            if dx >= 0.0 { GestureDir::RightDown } else { GestureDir::LeftDown }
        } else if dx >= 0.0 {
            GestureDir::Right
        } else {
            GestureDir::Left
        }
    } else {
        // Predominantly vertical motion.
        if dy >= 0.0 { GestureDir::Down } else { GestureDir::Up }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify ──────────────────────────────────────────────────────────────

    #[test]
    fn classify_pure_right() {
        assert_eq!(classify(100.0, 0.0), GestureDir::Right);
    }

    #[test]
    fn classify_pure_left() {
        assert_eq!(classify(-100.0, 0.0), GestureDir::Left);
    }

    #[test]
    fn classify_pure_down() {
        assert_eq!(classify(0.0, 100.0), GestureDir::Down);
    }

    #[test]
    fn classify_pure_up() {
        assert_eq!(classify(0.0, -100.0), GestureDir::Up);
    }

    #[test]
    fn classify_right_with_small_down_stays_right() {
        // dy/|dx| = 10/100 = 0.1, below DIAGONAL_RATIO (0.5)
        assert_eq!(classify(100.0, 10.0), GestureDir::Right);
    }

    #[test]
    fn classify_right_with_large_down_becomes_right_down() {
        // dy/|dx| = 60/100 = 0.6 > DIAGONAL_RATIO
        assert_eq!(classify(100.0, 60.0), GestureDir::RightDown);
    }

    #[test]
    fn classify_left_with_large_down_becomes_left_down() {
        assert_eq!(classify(-100.0, 60.0), GestureDir::LeftDown);
    }

    #[test]
    fn classify_left_with_large_up_stays_left() {
        // Diagonal only applies to downward component
        assert_eq!(classify(-100.0, -60.0), GestureDir::Left);
    }

    #[test]
    fn classify_right_with_large_up_stays_right() {
        // Diagonal only applies to downward component
        assert_eq!(classify(100.0, -60.0), GestureDir::Right);
    }

    #[test]
    fn classify_diagonal_boundary_exactly_at_ratio() {
        // dy/|dx| == DIAGONAL_RATIO exactly → NOT diagonal (strict >)
        let dy = 100.0 * DIAGONAL_RATIO;
        assert_eq!(classify(100.0, dy), GestureDir::Right);
    }

    #[test]
    fn classify_equal_dx_dy_down_right() {
        // |dx| == |dy| → horizontal branch; down component 1.0/1.0 = 1.0 > 0.5 → RightDown
        assert_eq!(classify(1.0, 1.0), GestureDir::RightDown);
    }

    // ── GestureMap ────────────────────────────────────────────────────────────

    #[test]
    fn default_map_has_four_bindings() {
        let m = GestureMap::default();
        assert_eq!(m.lookup(GestureDir::Left), Some(GestureAction::NavigateBack));
        assert_eq!(m.lookup(GestureDir::Right), Some(GestureAction::NavigateForward));
        assert_eq!(m.lookup(GestureDir::LeftDown), Some(GestureAction::CloseTab));
        assert_eq!(m.lookup(GestureDir::RightDown), Some(GestureAction::NewTab));
        assert_eq!(m.lookup(GestureDir::Up), None);
        assert_eq!(m.lookup(GestureDir::Down), None);
    }

    #[test]
    fn map_bind_and_unbind() {
        let mut m = GestureMap::empty();
        assert_eq!(m.lookup(GestureDir::Up), None);
        m.bind(GestureDir::Up, GestureAction::NewTab);
        assert_eq!(m.lookup(GestureDir::Up), Some(GestureAction::NewTab));
        m.unbind(GestureDir::Up);
        assert_eq!(m.lookup(GestureDir::Up), None);
    }

    // ── GestureRecognizer ─────────────────────────────────────────────────────

    #[test]
    fn no_drag_finish_returns_none() {
        let mut r = GestureRecognizer::new();
        assert_eq!(r.finish(), None);
    }

    #[test]
    fn short_drag_returns_none() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 0.0);
        r.track(20.0, 0.0); // < MIN_DRAG_PX
        assert_eq!(r.finish(), None);
    }

    #[test]
    fn right_drag_navigate_forward() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 0.0);
        r.track(100.0, 0.0);
        assert_eq!(r.finish(), Some(GestureAction::NavigateForward));
    }

    #[test]
    fn left_drag_navigate_back() {
        let mut r = GestureRecognizer::new();
        r.begin(200.0, 100.0);
        r.track(50.0, 100.0); // dx = -150
        assert_eq!(r.finish(), Some(GestureAction::NavigateBack));
    }

    #[test]
    fn right_down_drag_new_tab() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 0.0);
        r.track(100.0, 80.0); // dy/|dx| = 0.8 > 0.5 → RightDown
        assert_eq!(r.finish(), Some(GestureAction::NewTab));
    }

    #[test]
    fn left_down_drag_close_tab() {
        let mut r = GestureRecognizer::new();
        r.begin(200.0, 0.0);
        r.track(50.0, 100.0); // dx = -150, dy = 100 → |dx|>|dy|, dy/|dx|=0.67 → LeftDown
        assert_eq!(r.finish(), Some(GestureAction::CloseTab));
    }

    #[test]
    fn up_drag_unbound_returns_none() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 200.0);
        r.track(0.0, 50.0); // dy = -150 → Up
        assert_eq!(r.finish(), None);
    }

    #[test]
    fn cancel_clears_active_gesture() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 0.0);
        assert!(r.is_active());
        r.cancel();
        assert!(!r.is_active());
        assert_eq!(r.finish(), None);
    }

    #[test]
    fn finish_resets_to_idle() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 0.0);
        r.track(100.0, 0.0);
        let _ = r.finish();
        assert!(!r.is_active());
        assert_eq!(r.finish(), None); // second finish = no-op
    }

    #[test]
    fn is_active_true_after_begin() {
        let mut r = GestureRecognizer::new();
        assert!(!r.is_active());
        r.begin(10.0, 20.0);
        assert!(r.is_active());
    }

    #[test]
    fn custom_map_overrides_default() {
        let mut map = GestureMap::empty();
        map.bind(GestureDir::Up, GestureAction::NewTab);
        let mut r = GestureRecognizer::with_map(map);
        r.begin(0.0, 200.0);
        r.track(0.0, 50.0); // Up gesture
        assert_eq!(r.finish(), Some(GestureAction::NewTab));
    }

    #[test]
    fn set_map_replaces_at_runtime() {
        let mut r = GestureRecognizer::new();
        // Default: Right → Forward
        r.begin(0.0, 0.0);
        r.track(100.0, 0.0);
        assert_eq!(r.finish(), Some(GestureAction::NavigateForward));

        // Replace map so Right → CloseTab
        let mut new_map = GestureMap::empty();
        new_map.bind(GestureDir::Right, GestureAction::CloseTab);
        r.set_map(new_map);

        r.begin(0.0, 0.0);
        r.track(100.0, 0.0);
        assert_eq!(r.finish(), Some(GestureAction::CloseTab));
    }

    #[test]
    fn track_without_begin_is_noop() {
        let mut r = GestureRecognizer::new();
        r.track(500.0, 500.0); // no active gesture — should not panic
        assert!(!r.is_active());
    }

    #[test]
    fn exactly_min_drag_distance_returns_none() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 0.0);
        // Exactly MIN_DRAG_PX = 30.0 → dist < 30.0 is false, but 30.0 < 30.0 is also false
        // The check is `dist < MIN_DRAG_PX`, so exactly 30 should NOT return None.
        r.track(MIN_DRAG_PX, 0.0);
        assert_eq!(r.finish(), Some(GestureAction::NavigateForward));
    }

    #[test]
    fn just_under_min_drag_distance_returns_none() {
        let mut r = GestureRecognizer::new();
        r.begin(0.0, 0.0);
        r.track(MIN_DRAG_PX - 0.1, 0.0);
        assert_eq!(r.finish(), None);
    }
}

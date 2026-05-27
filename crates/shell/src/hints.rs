//! Click-hint overlay: vimium-style keyboard navigation for clickable elements.
//!
//! `F` (без модификаторов) → hint-бейджи на всех ссылках/кнопках/инпутах.
//! Ввод букв хинта → активация соответствующего DOM-узла (dispatch click).
//! `Escape` → закрыть оверлей без активации.
//!
//! Позиции бейджей пересчитываются каждый кадр из page-space координат
//! ClickableElement-ов с вычетом текущего scroll — бейджи всегда правильно
//! позиционированы даже если скролл изменился с момента открытия режима.

use lumen_core::geom::Rect;
use lumen_dom::NodeId;
use lumen_layout::{ClickableElement, Color, FontStyle, FontWeight};
use lumen_paint::{DisplayCommand, DisplayList};

/// Hint badge for one clickable element.
#[derive(Debug, Clone)]
pub struct HintItem {
    /// Keyboard label to type (1–2 lowercase ASCII chars).
    pub label: String,
    /// Viewport-space rect (element page rect minus current scroll offsets).
    pub rect: Rect,
}

/// Keyboard hint mode state machine.
#[derive(Debug, Default)]
pub struct HintState {
    active: bool,
    /// Characters typed so far toward a hint label.
    typed: String,
    /// Page-space clickable elements captured when the mode was opened.
    elements: Vec<ClickableElement>,
    /// Hint labels assigned in parallel with `elements` (index-stable).
    labels: Vec<String>,
}

/// Result returned by [`HintState::push_char`].
pub enum HintResult {
    /// Unique match found — activate this DOM node.
    Activate(NodeId),
    /// More characters needed — redraw to show narrowed badges.
    Partial,
    /// No hint starts with the typed prefix — mode cancelled.
    NoMatch,
}

impl HintState {
    /// Whether the hint overlay is currently visible.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Open hint mode with a snapshot of the current page's clickable elements.
    pub fn open(&mut self, elements: Vec<ClickableElement>) {
        let labels = make_labels(elements.len());
        self.elements = elements;
        self.labels = labels;
        self.typed.clear();
        self.active = true;
    }

    /// Dismiss the overlay without activating anything.
    pub fn close(&mut self) {
        self.active = false;
        self.typed.clear();
        self.elements.clear();
        self.labels.clear();
    }

    /// Record one typed character and return the resulting state.
    pub fn push_char(&mut self, c: char) -> HintResult {
        self.typed.push(c);
        let needle = self.typed.as_str();
        let mut activate_id: Option<NodeId> = None;
        let mut partial_count = 0usize;
        for (el, label) in self.elements.iter().zip(&self.labels) {
            if label.starts_with(needle) {
                if label == needle {
                    activate_id = Some(el.node_id);
                } else {
                    partial_count += 1;
                }
            }
        }
        match (activate_id, partial_count) {
            (Some(id), 0) => {
                self.close();
                HintResult::Activate(id)
            }
            (None, 0) => {
                self.close();
                HintResult::NoMatch
            }
            _ => HintResult::Partial,
        }
    }

    /// Characters typed so far — used to dim non-matching badges.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// Compute viewport-space hint items for the current scroll offsets.
    ///
    /// Called every frame so badges remain anchored to their elements
    /// even after the user scrolls while hint mode is open.
    pub fn items(&self, scroll_x: f32, scroll_y: f32) -> Vec<HintItem> {
        self.elements
            .iter()
            .zip(&self.labels)
            .map(|(el, label)| HintItem {
                label: label.clone(),
                rect: Rect::new(
                    el.rect.x - scroll_x,
                    el.rect.y - scroll_y,
                    el.rect.width,
                    el.rect.height,
                ),
            })
            .collect()
    }
}

// ── Label generation ──────────────────────────────────────────────────────────

/// Home-row keys first for ergonomics (vimium-style priority ordering).
const HINT_CHARS: &[char] = &[
    'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o',
    'p', 'z', 'x', 'c', 'v', 'b', 'n', 'm',
];

/// Generate `count` unique lowercase labels: single chars, then two-char combos.
fn make_labels(count: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(count);
    for &c in HINT_CHARS {
        if out.len() == count {
            return out;
        }
        out.push(c.to_string());
    }
    'outer: for &c1 in HINT_CHARS {
        for &c2 in HINT_CHARS {
            if out.len() == count {
                break 'outer;
            }
            out.push(format!("{c1}{c2}"));
        }
    }
    out
}

// ── Overlay rendering ─────────────────────────────────────────────────────────

/// Yellow-on-dark badge background (vimium colour palette).
const BADGE_BG: Color = Color { r: 255, g: 220, b: 0, a: 240 };
/// Dark text for contrast on yellow badge.
const BADGE_FG: Color = Color { r: 30, g: 30, b: 30, a: 255 };
/// Greyed-out badge for hints that no longer match the typed prefix.
const BADGE_DIM_BG: Color = Color { r: 180, g: 180, b: 180, a: 100 };

const BADGE_PAD_X: f32 = 3.0;
const BADGE_PAD_Y: f32 = 1.0;
const BADGE_FONT_SIZE: f32 = 12.0;
/// Approximate monospace glyph width at 12 px for badge width estimation.
const BADGE_CHAR_W: f32 = 7.5;

/// Build the viewport-locked overlay display list for all active hint badges.
///
/// `scroll_x` / `scroll_y` are the current page scroll offsets in CSS px.
/// Each active badge becomes: `FillRect` (background) + `DrawText` (label).
/// Badges whose label no longer matches `state.typed()` are dimmed (background only).
pub fn build_hints_overlay(state: &HintState, scroll_x: f32, scroll_y: f32) -> DisplayList {
    let items = state.items(scroll_x, scroll_y);
    let mut out: DisplayList = Vec::with_capacity(items.len() * 2);
    for item in &items {
        let label_w = item.label.len() as f32 * BADGE_CHAR_W + 2.0 * BADGE_PAD_X;
        let badge_h = BADGE_FONT_SIZE + 2.0 * BADGE_PAD_Y;
        // Position badge just above the top-left corner of the element.
        let bx = item.rect.x.max(0.0);
        let by = (item.rect.y - badge_h).max(0.0);
        let dimmed =
            !state.typed().is_empty() && !item.label.starts_with(state.typed());
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(bx, by, label_w, badge_h),
            color: if dimmed { BADGE_DIM_BG } else { BADGE_BG },
        });
        if !dimmed {
            out.push(DisplayCommand::DrawText {
                rect: Rect::new(
                    bx + BADGE_PAD_X,
                    by + BADGE_PAD_Y,
                    label_w - BADGE_PAD_X,
                    BADGE_FONT_SIZE,
                ),
                text: item.label.clone(),
                font_size: BADGE_FONT_SIZE,
                color: BADGE_FG,
                font_family: Vec::new(),
                font_weight: FontWeight(700),
                font_style: FontStyle::Normal,
                font_variation_axes: Vec::new(),
                tab_size: 0.0,
            });
        }
    }
    out
}

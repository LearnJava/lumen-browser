//! Forms UI runtime — click dispatch, constraint validation, picker overlays.
//!
//! Runs entirely in the shell. No layout/paint crate changes are required for
//! the overlay elements (validation tooltip, color picker) because they are
//! viewport-locked overlays like the find-bar — injected into `overlay_buf`
//! just before `Renderer::render`.
//!
//! Form state (current input values, checkbox checked state) is stored in a
//! `FormState` map keyed by `NodeId`. On each checkbox toggle the DOM attribute
//! is mutated in `LayoutSource.document` and `relayout_page` is called so the
//! `:checked` CSS pseudo-class reflects the new state.

use std::collections::HashMap;

use lumen_core::geom::Rect;
use lumen_dom::{Attribute, Document, InputType, NodeData, NodeId, QualName};
use lumen_layout::{BorderStyle, BoxKind, Color, FontStyle, FontWeight, LayoutBox};
use lumen_paint::{DisplayCommand, DisplayList};

// ──────────────────────────────────────────────────────────────────────────────
// Runtime state
// ──────────────────────────────────────────────────────────────────────────────

/// Mutable runtime state for a single form control.
#[derive(Debug, Default, Clone)]
pub struct FormControlState {
    /// Current input value (what the user typed, or the initial `value` attr).
    pub value: String,
    /// Whether the control is checked (checkbox / radio only).
    /// Updated on ToggleCheckbox; read during form reset / serialization.
    #[allow(dead_code)]
    pub checked: bool,
}

/// `NodeId` → mutable state map for all form controls on the current page.
pub type FormState = HashMap<NodeId, FormControlState>;

// ──────────────────────────────────────────────────────────────────────────────
// Click classification
// ──────────────────────────────────────────────────────────────────────────────

/// What the shell should do after a left-click on `node`.
#[derive(Debug)]
pub enum FormClickAction {
    ToggleCheckbox(NodeId),
    ToggleRadio { clicked: NodeId, _group_name: String },
    OpenColorPicker(NodeId),
    SubmitForm(NodeId),
    Nothing,
}

/// Classify a click on `node` given the current DOM tree.
pub fn classify_click(doc: &Document, node: NodeId) -> FormClickAction {
    let n = doc.get(node);
    let tag = n.element_name().map(|q| q.local.as_str()).unwrap_or("");
    match tag {
        "input" => {
            let itype = n.input_type().unwrap_or(InputType::Text);
            match itype {
                InputType::Checkbox => FormClickAction::ToggleCheckbox(node),
                InputType::Radio => {
                    let name = n.get_attr("name").unwrap_or("").to_owned();
                    FormClickAction::ToggleRadio { clicked: node, _group_name: name }
                }
                InputType::Color => FormClickAction::OpenColorPicker(node),
                InputType::Submit => FormClickAction::SubmitForm(node),
                _ => FormClickAction::Nothing,
            }
        }
        "button" => {
            let btype = n.get_attr("type").unwrap_or("submit").to_ascii_lowercase();
            if btype == "submit" {
                FormClickAction::SubmitForm(node)
            } else {
                FormClickAction::Nothing
            }
        }
        _ => FormClickAction::Nothing,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// DOM mutation helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Toggle the `checked` attribute on a checkbox input in the live DOM.
/// After calling this, relayout is needed to update `:checked` pseudo-class.
pub fn toggle_checkbox(doc: &mut Document, id: NodeId) {
    let node = doc.get_mut(id);
    if let NodeData::Element { ref mut attrs, .. } = node.data {
        if attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("checked")) {
            attrs.retain(|a| !a.name.local.eq_ignore_ascii_case("checked"));
        } else {
            attrs.push(Attribute { name: QualName::html("checked"), value: String::new() });
        }
    }
}

/// Set `value` attribute of an input / textarea in the DOM.
pub fn set_value(doc: &mut Document, id: NodeId, value: &str) {
    let node = doc.get_mut(id);
    if let NodeData::Element { ref mut attrs, .. } = node.data {
        if let Some(attr) = attrs.iter_mut().find(|a| a.name.local.eq_ignore_ascii_case("value")) {
            attr.value = value.to_owned();
        } else {
            attrs.push(Attribute { name: QualName::html("value"), value: value.to_owned() });
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Constraint validation
// ──────────────────────────────────────────────────────────────────────────────

/// Depth-first walk: find the first form control that fails HTML5 constraint
/// validation. Returns `(node_id, box_rect, human-readable message)`.
pub fn find_validation_error(
    root: &LayoutBox,
    doc: &Document,
    form_state: &FormState,
) -> Option<(NodeId, Rect, String)> {
    find_error_in(root, doc, form_state)
}

fn find_error_in(
    b: &LayoutBox,
    doc: &Document,
    form_state: &FormState,
) -> Option<(NodeId, Rect, String)> {
    if matches!(b.kind, BoxKind::FormControl { .. }) {
        let node = doc.get(b.node);
        let tag = node.element_name().map(|q| q.local.as_str()).unwrap_or("");
        if matches!(tag, "input" | "textarea" | "select") {
            // required
            if node.get_attr("required").is_some() {
                let value = form_state
                    .get(&b.node)
                    .map(|s| s.value.as_str())
                    .unwrap_or_else(|| node.get_attr("value").unwrap_or(""));
                if value.trim().is_empty() {
                    let label = node
                        .get_attr("placeholder")
                        .unwrap_or("Это поле");
                    return Some((
                        b.node,
                        b.rect,
                        format!("{label} обязательно для заполнения"),
                    ));
                }
            }
        }
    }
    for child in &b.children {
        if let Some(found) = find_error_in(child, doc, form_state) {
            return Some(found);
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────────────
// Layout tree helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Find the bounding rect of the LayoutBox for `node`. Returns `None` if the
/// node has no box (e.g. `display:none` or not in the tree).
pub fn find_box_rect(root: &LayoutBox, node: NodeId) -> Option<Rect> {
    if root.node == node {
        return Some(root.rect);
    }
    for child in &root.children {
        if let Some(r) = find_box_rect(child, node) {
            return Some(r);
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────────────────────
// Validation tooltip overlay
// ──────────────────────────────────────────────────────────────────────────────

const TOOLTIP_PADDING: f32 = 6.0;
const TOOLTIP_FONT_SIZE: f32 = 13.0;

/// Build a validation tooltip anchored below `anchor` (document coordinates).
/// The result is viewport-fixed: caller must pass the current `scroll_y` so
/// the tooltip appears at the correct screen position regardless of scroll.
pub fn build_validation_tooltip(
    anchor: Rect,
    message: &str,
    scroll_y: f32,
    viewport_w: f32,
) -> DisplayList {
    let mut out: DisplayList = Vec::new();

    // Approximate width: 7 px per character.
    let w = (message.len() as f32 * 7.0 + TOOLTIP_PADDING * 2.0).clamp(120.0, 320.0);
    let h = TOOLTIP_FONT_SIZE + TOOLTIP_PADDING * 2.0;

    let vp_y = anchor.y - scroll_y + anchor.height + 4.0;
    let vp_x = anchor.x.min(viewport_w - w).max(0.0);

    let bg = Rect::new(vp_x, vp_y, w, h);

    // Yellow background
    out.push(DisplayCommand::FillRect {
        rect: bg,
        color: Color { r: 255, g: 253, b: 200, a: 245 },
    });
    // 1 px dark border
    out.push(DisplayCommand::DrawBorder {
        rect: bg,
        widths: [1.0; 4],
        colors: [Color { r: 60, g: 60, b: 60, a: 255 }; 4],
        styles: [BorderStyle::Solid; 4],
    });
    // Text
    out.push(DisplayCommand::DrawText {
        rect: Rect::new(vp_x + TOOLTIP_PADDING, vp_y + TOOLTIP_PADDING, w - TOOLTIP_PADDING * 2.0, TOOLTIP_FONT_SIZE),
        text: message.to_owned(),
        font_size: TOOLTIP_FONT_SIZE,
        color: Color { r: 20, g: 20, b: 20, a: 255 },
        font_family: vec![],
        font_weight: FontWeight(400),
        font_style: FontStyle::Normal,
        font_variation_axes: vec![],
    });
    out
}

// ──────────────────────────────────────────────────────────────────────────────
// Color picker overlay
// ──────────────────────────────────────────────────────────────────────────────

/// 36-color palette (6 columns × 6 rows): grayscale + primaries + mixed tones.
const PALETTE: &[[u8; 3]] = &[
    [0,   0,   0  ], [51,  51,  51 ], [102, 102, 102], [153, 153, 153], [204, 204, 204], [255, 255, 255],
    [255, 0,   0  ], [204, 0,   0  ], [153, 0,   0  ], [255, 102, 102], [255, 153, 51 ], [255, 204, 51 ],
    [0,   255, 0  ], [0,   204, 0  ], [0,   153, 0  ], [102, 255, 102], [0,   255, 153], [51,  204, 102],
    [0,   0,   255], [0,   0,   204], [0,   0,   153], [102, 102, 255], [0,   153, 255], [51,  0,   204],
    [255, 255, 0  ], [204, 204, 0  ], [255, 153, 0  ], [255, 204, 0  ], [204, 102, 0  ], [255, 153, 51 ],
    [255, 0,   255], [204, 0,   204], [153, 0,   153], [0,   255, 255], [0,   204, 204], [0,   153, 153],
];

pub const PICKER_COLS: usize = 6;
pub const SWATCH_SIZE: f32 = 20.0;
const SWATCH_GAP: f32 = 2.0;
const PICKER_PAD: f32 = 6.0;

fn picker_size() -> (f32, f32) {
    let rows = PALETTE.len().div_ceil(PICKER_COLS);
    let w = PICKER_COLS as f32 * (SWATCH_SIZE + SWATCH_GAP) - SWATCH_GAP + PICKER_PAD * 2.0;
    let h = rows as f32 * (SWATCH_SIZE + SWATCH_GAP) - SWATCH_GAP + PICKER_PAD * 2.0;
    (w, h)
}

/// Build a color-swatch picker anchored below `anchor` (document coordinates).
pub fn build_color_picker(anchor: Rect, scroll_y: f32, viewport_w: f32) -> DisplayList {
    let mut out: DisplayList = Vec::new();
    let (pw, ph) = picker_size();

    let vp_y = anchor.y - scroll_y + anchor.height + 4.0;
    let vp_x = anchor.x.min(viewport_w - pw).max(0.0);
    let bg = Rect::new(vp_x, vp_y, pw, ph);

    out.push(DisplayCommand::FillRect {
        rect: bg,
        color: Color { r: 245, g: 245, b: 245, a: 252 },
    });
    out.push(DisplayCommand::DrawBorder {
        rect: bg,
        widths: [1.0; 4],
        colors: [Color { r: 80, g: 80, b: 80, a: 255 }; 4],
        styles: [BorderStyle::Solid; 4],
    });

    for (i, &[r, g, b]) in PALETTE.iter().enumerate() {
        let col = (i % PICKER_COLS) as f32;
        let row = (i / PICKER_COLS) as f32;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(
                vp_x + PICKER_PAD + col * (SWATCH_SIZE + SWATCH_GAP),
                vp_y + PICKER_PAD + row * (SWATCH_SIZE + SWATCH_GAP),
                SWATCH_SIZE,
                SWATCH_SIZE,
            ),
            color: Color { r, g, b, a: 255 },
        });
    }
    out
}

/// If viewport-space point `(px, py)` lands on a swatch, return its `[r, g, b]`.
pub fn hit_color_swatch(anchor: Rect, scroll_y: f32, px: f32, py: f32) -> Option<[u8; 3]> {
    let origin_x = anchor.x.min(/* viewport_w */ f32::MAX - picker_size().0).max(0.0);
    let origin_y = anchor.y - scroll_y + anchor.height + 4.0;

    let rel_x = px - origin_x - PICKER_PAD;
    let rel_y = py - origin_y - PICKER_PAD;
    if rel_x < 0.0 || rel_y < 0.0 {
        return None;
    }

    let cell = SWATCH_SIZE + SWATCH_GAP;
    let col = (rel_x / cell) as usize;
    let row = (rel_y / cell) as usize;
    if col >= PICKER_COLS { return None; }
    // Ensure we hit the swatch, not the gap.
    if rel_x % cell > SWATCH_SIZE || rel_y % cell > SWATCH_SIZE { return None; }

    PALETTE.get(row * PICKER_COLS + col).copied()
}

/// Format `[r, g, b]` as CSS `#rrggbb`.
pub fn swatch_to_css_color(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

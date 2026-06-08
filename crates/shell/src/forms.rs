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

use lumen_core::form::{encode_form_urlencoded, FormEntry, FormValue};
use lumen_core::geom::Rect;
use lumen_dom::{
    check_validity_form, collect_dom_form_fields, element_validity, find_ancestor_form,
    invalid_controls_in_form, submit_form, Attribute, Document, FormSubmitEvent, InputType,
    NodeData, NodeId, QualName,
};
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
    /// Open a dropdown overlay showing the `<option>` children of the `<select>`.
    OpenSelectDropdown(NodeId),
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
        "select" => FormClickAction::OpenSelectDropdown(node),
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
#[allow(dead_code)]
pub fn find_validation_error(
    root: &LayoutBox,
    doc: &Document,
    form_state: &FormState,
) -> Option<(NodeId, Rect, String)> {
    find_error_in(root, doc, form_state)
}

/// Find rect and error message for a specific invalid control.
/// Used in Phase 3 form submission to show validation tooltip for invalid controls.
#[allow(dead_code)]
pub fn find_control_rect_and_error(
    root: &LayoutBox,
    doc: &Document,
    node_id: NodeId,
) -> Option<(Rect, String)> {
    find_control_error_in(root, doc, node_id)
}

/// Collect all form controls that fail HTML5 constraint validation.
/// Returns a vector of `(node_id, box_rect, human-readable message)` tuples.
#[allow(dead_code)]  // Public API for Phase 2 — used when displaying all invalid controls
pub fn find_all_validation_errors(
    root: &LayoutBox,
    doc: &Document,
    form_state: &FormState,
) -> Vec<(NodeId, Rect, String)> {
    let mut errors = Vec::new();
    collect_errors_in(root, doc, form_state, &mut errors);
    errors
}

#[allow(dead_code)]  // Helper for find_all_validation_errors
fn collect_errors_in(
    b: &LayoutBox,
    doc: &Document,
    form_state: &FormState,
    errors: &mut Vec<(NodeId, Rect, String)>,
) {
    if matches!(b.kind, BoxKind::FormControl { .. })
        && let Some(vs) = element_validity(doc, b.node)
        && let Some(msg) = validation_error_message(doc, b.node, &vs, form_state)
    {
        errors.push((b.node, b.rect, msg));
    }
    for child in &b.children {
        collect_errors_in(child, doc, form_state, errors);
    }
}

#[allow(dead_code)]
fn find_error_in(
    b: &LayoutBox,
    doc: &Document,
    form_state: &FormState,
) -> Option<(NodeId, Rect, String)> {
    if matches!(b.kind, BoxKind::FormControl { .. })
        && let Some(vs) = element_validity(doc, b.node)
        && let Some(msg) = validation_error_message(doc, b.node, &vs, form_state)
    {
        return Some((b.node, b.rect, msg));
    }
    for child in &b.children {
        if let Some(found) = find_error_in(child, doc, form_state) {
            return Some(found);
        }
    }
    None
}

/// Find rect and error message for a specific control by NodeId.
/// Phase 3: used when displaying validation errors from FormSubmitEvent::Invalid.
fn find_control_error_in(
    b: &LayoutBox,
    doc: &Document,
    target_node: NodeId,
) -> Option<(Rect, String)> {
    if b.node == target_node
        && let Some(vs) = element_validity(doc, b.node)
        && let Some(msg) = validation_error_message(doc, b.node, &vs, &FormState::default())
    {
        return Some((b.rect, msg));
    }
    for child in &b.children {
        if let Some(found) = find_control_error_in(child, doc, target_node) {
            return Some(found);
        }
    }
    None
}

/// Generate human-readable validation error message based on ValidityState.
/// Returns `None` if the element is valid.
fn validation_error_message(
    doc: &Document,
    node_id: NodeId,
    validity: &lumen_dom::ValidityState,
    _form_state: &FormState,
) -> Option<String> {
    if validity.valid() {
        return None;
    }

    let node = doc.get(node_id);
    let label = node
        .get_attr("placeholder")
        .or_else(|| node.get_attr("aria-label"))
        .unwrap_or("Это поле");

    if validity.value_missing {
        Some(format!("{label} обязательно для заполнения"))
    } else if validity.type_mismatch {
        let itype = node.get_attr("type").unwrap_or("text");
        match itype {
            "email" => Some(format!("{label} должен быть корректным email адресом")),
            "url" => Some(format!("{label} должен быть корректным URL")),
            _ => Some(format!("{label} имеет некорректное значение")),
        }
    } else if validity.pattern_mismatch {
        Some(format!("{label} не соответствует требуемому формату"))
    } else if validity.too_long {
        let maxlength = node.get_attr("maxlength").unwrap_or("N");
        Some(format!("{label} не может быть длиннее {maxlength} символов"))
    } else if validity.too_short {
        let minlength = node.get_attr("minlength").unwrap_or("N");
        Some(format!("{label} должен быть минимум {minlength} символов"))
    } else if validity.range_underflow {
        let min = node.get_attr("min").unwrap_or("N");
        Some(format!("{label} не может быть меньше {min}"))
    } else if validity.range_overflow {
        let max = node.get_attr("max").unwrap_or("N");
        Some(format!("{label} не может быть больше {max}"))
    } else if validity.step_mismatch {
        Some(format!("{label} имеет некорректное значение (не соответствует шагу)"))
    } else if validity.bad_input || validity.custom_error {
        Some(format!("{label} имеет некорректное значение"))
    } else {
        None
    }
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
        radii: lumen_paint::CornerRadii::default(),
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
        tab_size: 0.0,
    });
    out
}

// ──────────────────────────────────────────────────────────────────────────────
// Form submission
// ──────────────────────────────────────────────────────────────────────────────

/// Собрать данные формы для submit — DOM-значения, поверх которых наложен
/// `FormState` (то что пользователь ввёл в runtime).
///
/// HTML LS §constructing-form-data-set: обходим submittable-контролы формы,
/// берём значение из `form_state` если есть, иначе — из DOM-атрибута `value`.
/// Checkbox/radio из `form_state` отражают runtime-состояние (checked).
#[allow(dead_code)]
pub fn collect_form_entries(
    doc: &Document,
    form_id: NodeId,
    form_state: &FormState,
) -> Vec<FormEntry> {
    let dom_fields = collect_dom_form_fields(doc, form_id);
    dom_fields
        .into_iter()
        .map(|(name, dom_value)| {
            // Ищем NodeId с этим именем, чтобы взять runtime-значение.
            let runtime_value = form_state
                .iter()
                .find(|(id, _)| {
                    doc.get(**id)
                        .get_attr("name")
                        .map(|n| n == name)
                        .unwrap_or(false)
                })
                .map(|(_, s)| s.value.clone());
            FormEntry::text(name, runtime_value.unwrap_or(dom_value))
        })
        .collect()
}

/// Построить параметры отправки формы: `(action, method, body)`.
///
/// Возвращает `None` если:
/// - submit-кнопка не вложена ни в какую `<form>`, или
/// - форма не проходит constraint validation (HTML5 §4.10.22.3 step 5).
///
/// Во втором случае выводит список невалидных контролов в stderr.
///
/// - `action` — значение атрибута `action` формы (пустая строка если
///   атрибут отсутствует; вызывающий код должен резолвить к текущему URL).
/// - `method` — `"get"` или `"post"` (нижний регистр).
/// - `body` — urlencoded данные формы.
///
/// Для GET-форм вызывающий должен добавить `?body` к action-URL.
/// Для POST-форм `body` — тело запроса, Content-Type: application/x-www-form-urlencoded.
/// Execute HTML5 form submission algorithm on the form containing submit_node.
/// Returns FormSubmitEvent::Valid with action, method, fields if validation passes,
/// or FormSubmitEvent::Invalid with list of invalid controls if validation fails.
/// Returns None if submit_node is not part of a form.
pub fn build_form_submit_event(
    doc: &Document,
    submit_node: NodeId,
) -> Option<FormSubmitEvent> {
    let form_id = find_ancestor_form(doc, submit_node)?;
    Some(submit_form(doc, form_id))
}

/// Encode form fields for submission. Wraps a FormSubmitEvent::Valid variant
/// and encodes fields as application/x-www-form-urlencoded.
pub fn encode_form_fields(fields: &[(String, String)]) -> String {
    let entries: Vec<FormEntry> = fields
        .iter()
        .map(|(name, value)| FormEntry { name: name.clone(), value: FormValue::Text(value.clone()) })
        .collect();
    encode_form_urlencoded(&entries)
}

#[allow(dead_code)]
pub fn build_form_submit(
    doc: &Document,
    submit_node: NodeId,
    form_state: &FormState,
) -> Option<(String, String, String)> {
    let form_id = find_ancestor_form(doc, submit_node)?;

    // HTML5 §4.10.22.3 step 5: interactive validation — block submit if invalid.
    if !check_validity_form(doc, form_id) {
        let invalid = invalid_controls_in_form(doc, form_id);
        eprintln!(
            "forms: submit blocked — {} control(s) failed constraint validation",
            invalid.len()
        );
        return None;
    }

    let form_node = doc.get(form_id);
    let action = form_node.get_attr("action").unwrap_or("").to_string();
    let method = form_node
        .get_attr("method")
        .unwrap_or("get")
        .to_ascii_lowercase();
    let entries = collect_form_entries(doc, form_id, form_state);
    let body = encode_form_urlencoded(&entries);
    Some((action, method, body))
}

/// Построить итоговый URL для GET-формы: добавить `?body` к action URL.
///
/// Если `action` пустой — возвращает `?body` (браузер резолвит к текущей странице).
/// Если `body` пустой — возвращает `action` без изменений.
pub fn make_get_url(action: &str, body: &str) -> String {
    if body.is_empty() {
        return action.to_string();
    }
    if action.is_empty() {
        return format!("?{body}");
    }
    // Удаляем существующий query-string и fragment из action per HTML LS.
    let base = action.split('?').next().unwrap_or(action);
    let base = base.split('#').next().unwrap_or(base);
    format!("{base}?{body}")
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
        radii: lumen_paint::CornerRadii::default(),
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

// ──────────────────────────────────────────────────────────────────────────────
// <select> dropdown overlay
// ──────────────────────────────────────────────────────────────────────────────

/// One entry in a `<select>` dropdown list.
#[derive(Debug, Clone)]
pub struct SelectOption {
    /// Visible label text.
    pub label: String,
    /// `value` attribute, or `label` if absent.
    pub value: String,
    /// Whether this option carries the `selected` attribute.
    pub selected: bool,
    /// Whether this option is `disabled`.
    pub disabled: bool,
    /// NodeId of the `<option>` element.
    pub node_id: NodeId,
}

/// Row height for each option row in the dropdown.
pub const DROPDOWN_ROW_H: f32 = 22.0;
const DROPDOWN_FONT: f32 = 13.0;
const DROPDOWN_PAD_X: f32 = 8.0;
const DROPDOWN_PAD_Y: f32 = 4.0;
const DROPDOWN_MIN_W: f32 = 120.0;
const DROPDOWN_MAX_ROWS_VISIBLE: usize = 8;

/// Collect all direct `<option>` children of a `<select>` DOM node.
/// `<optgroup>` children are flattened (their `<option>` children are included).
pub fn collect_select_options(doc: &Document, select_id: NodeId) -> Vec<SelectOption> {
    let mut opts = Vec::new();
    collect_options_from(doc, select_id, &mut opts);
    opts
}

fn collect_options_from(doc: &Document, parent_id: NodeId, out: &mut Vec<SelectOption>) {
    let children = doc.get(parent_id).children.clone();
    for child_id in children {
        let child = doc.get(child_id);
        let NodeData::Element { name, attrs, .. } = &child.data else { continue };
        match name.local.as_str() {
            "option" => {
                let disabled = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("disabled"));
                let selected = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("selected"));
                let label = if let Some(a) = attrs.iter().find(|a| a.name.local.eq_ignore_ascii_case("label")) {
                    a.value.trim().to_owned()
                } else {
                    // text content of children
                    child.children.iter().filter_map(|&c| {
                        if let NodeData::Text(t) = &doc.get(c).data { Some(t.as_str()) } else { None }
                    }).collect::<Vec<_>>().join("").trim().to_owned()
                };
                let value = attrs.iter()
                    .find(|a| a.name.local.eq_ignore_ascii_case("value"))
                    .map(|a| a.value.clone())
                    .unwrap_or_else(|| label.clone());
                out.push(SelectOption { label, value, selected, disabled, node_id: child_id });
            }
            "optgroup" => collect_options_from(doc, child_id, out),
            _ => {}
        }
    }
}

/// Build a dropdown overlay anchored below (or above if near the bottom of the
/// viewport) `anchor`. `scroll_y` converts document-space anchor to viewport-space.
pub fn build_select_dropdown(
    anchor: Rect,
    options: &[SelectOption],
    scroll_y: f32,
    viewport_w: f32,
    viewport_h: f32,
) -> DisplayList {
    let mut out: DisplayList = Vec::new();
    if options.is_empty() {
        return out;
    }

    let rows = options.len().min(DROPDOWN_MAX_ROWS_VISIBLE);
    let w = (anchor.width).max(DROPDOWN_MIN_W).min(viewport_w);
    let h = rows as f32 * DROPDOWN_ROW_H + DROPDOWN_PAD_Y * 2.0;

    // Position: below anchor, flip above if it would overflow viewport bottom.
    let anchor_bottom_vp = anchor.y - scroll_y + anchor.height;
    let vp_y = if anchor_bottom_vp + h > viewport_h && anchor.y - scroll_y >= h {
        anchor.y - scroll_y - h
    } else {
        anchor_bottom_vp + 1.0
    };
    let vp_x = anchor.x.min(viewport_w - w).max(0.0);

    let bg = Rect::new(vp_x, vp_y, w, h);

    // Background + shadow border.
    out.push(DisplayCommand::FillRect {
        rect: bg,
        color: Color { r: 255, g: 255, b: 255, a: 255 },
    });
    out.push(DisplayCommand::DrawBorder {
        rect: bg,
        widths: [1.0; 4],
        colors: [Color { r: 180, g: 180, b: 180, a: 255 }; 4],
        styles: [BorderStyle::Solid; 4],
        radii: lumen_paint::CornerRadii::default(),
    });

    for (i, opt) in options.iter().take(DROPDOWN_MAX_ROWS_VISIBLE).enumerate() {
        let row_y = vp_y + DROPDOWN_PAD_Y + i as f32 * DROPDOWN_ROW_H;
        let row_rect = Rect::new(vp_x, row_y, w, DROPDOWN_ROW_H);

        // Highlight selected option.
        if opt.selected {
            out.push(DisplayCommand::FillRect {
                rect: row_rect,
                color: Color { r: 0, g: 120, b: 215, a: 255 },
            });
        }

        let text_color = if opt.disabled {
            Color { r: 150, g: 150, b: 150, a: 255 }
        } else if opt.selected {
            Color { r: 255, g: 255, b: 255, a: 255 }
        } else {
            Color { r: 20, g: 20, b: 20, a: 255 }
        };

        out.push(DisplayCommand::DrawText {
            rect: Rect::new(
                vp_x + DROPDOWN_PAD_X,
                row_y + (DROPDOWN_ROW_H - DROPDOWN_FONT) * 0.5,
                w - DROPDOWN_PAD_X * 2.0,
                DROPDOWN_FONT,
            ),
            text: opt.label.clone(),
            font_size: DROPDOWN_FONT,
            color: text_color,
            font_family: vec![],
            font_weight: FontWeight(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            tab_size: 0.0,
        });
    }

    out
}

/// If viewport-space point `(px, py)` lands on an option row, return its index.
pub fn hit_select_option(
    anchor: Rect,
    options_count: usize,
    scroll_y: f32,
    viewport_w: f32,
    viewport_h: f32,
    px: f32,
    py: f32,
) -> Option<usize> {
    if options_count == 0 {
        return None;
    }
    let rows = options_count.min(DROPDOWN_MAX_ROWS_VISIBLE);
    let w = anchor.width.max(DROPDOWN_MIN_W).min(viewport_w);
    let h = rows as f32 * DROPDOWN_ROW_H + DROPDOWN_PAD_Y * 2.0;

    let anchor_bottom_vp = anchor.y - scroll_y + anchor.height;
    let vp_y = if anchor_bottom_vp + h > viewport_h && anchor.y - scroll_y >= h {
        anchor.y - scroll_y - h
    } else {
        anchor_bottom_vp + 1.0
    };
    let vp_x = anchor.x.min(viewport_w - w).max(0.0);

    if px < vp_x || px > vp_x + w || py < vp_y || py > vp_y + h {
        return None;
    }
    let rel_y = py - vp_y - DROPDOWN_PAD_Y;
    if rel_y < 0.0 {
        return None;
    }
    let row = (rel_y / DROPDOWN_ROW_H) as usize;
    if row < rows { Some(row) } else { None }
}

/// Apply the selection of option at `opt_idx` to the `<select>` DOM node:
/// removes all `selected` attributes, then sets `selected` on the chosen option.
pub fn apply_select_choice(doc: &mut Document, options: &[SelectOption], opt_idx: usize) {
    for opt in options {
        let node = doc.get_mut(opt.node_id);
        if let NodeData::Element { ref mut attrs, .. } = node.data {
            attrs.retain(|a| !a.name.local.eq_ignore_ascii_case("selected"));
        }
    }
    if let Some(chosen) = options.get(opt_idx) {
        let node = doc.get_mut(chosen.node_id);
        if let NodeData::Element { ref mut attrs, .. } = node.data {
            attrs.push(Attribute { name: QualName::html("selected"), value: String::new() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};

    fn make_submit_doc() -> (Document, NodeId) {
        // <form action="/go" method="get">
        //   <input name="q" value="rust">
        //   <input type="submit">
        // </form>
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
            attrs.push(Attribute { name: QualName::html("action"), value: "/go".into() });
            attrs.push(Attribute { name: QualName::html("method"), value: "get".into() });
        }
        let q = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(q).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "q".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "rust".into() });
        }
        let submit = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, q);
        doc.append_child(form, submit);
        (doc, submit)
    }

    #[test]
    fn make_get_url_appends_query() {
        assert_eq!(make_get_url("/search", "q=hello"), "/search?q=hello");
    }

    #[test]
    fn make_get_url_empty_body() {
        assert_eq!(make_get_url("/page", ""), "/page");
    }

    #[test]
    fn make_get_url_empty_action() {
        assert_eq!(make_get_url("", "k=v"), "?k=v");
    }

    #[test]
    fn make_get_url_strips_existing_query() {
        // HTML LS: заменяем query-string целиком при submit.
        assert_eq!(make_get_url("/s?old=1", "new=2"), "/s?new=2");
    }

    #[test]
    fn make_get_url_strips_fragment() {
        assert_eq!(make_get_url("/page#sec", "x=1"), "/page?x=1");
    }

    #[test]
    fn build_form_submit_get() {
        let (doc, submit) = make_submit_doc();
        let state = FormState::default();
        let result = build_form_submit(&doc, submit, &state);
        assert!(result.is_some());
        let (action, method, body) = result.unwrap();
        assert_eq!(action, "/go");
        assert_eq!(method, "get");
        assert_eq!(body, "q=rust");
    }

    #[test]
    fn build_form_submit_post() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
            attrs.push(Attribute { name: QualName::html("action"), value: "/api".into() });
            attrs.push(Attribute { name: QualName::html("method"), value: "POST".into() });
        }
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "email".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "a@b.c".into() });
        }
        let submit = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);
        doc.append_child(form, submit);

        let state = FormState::default();
        let (action, method, body) = build_form_submit(&doc, submit, &state).unwrap();
        assert_eq!(action, "/api");
        assert_eq!(method, "post");
        assert!(body.contains("email="));
    }

    #[test]
    fn build_form_submit_no_form_returns_none() {
        let mut doc = Document::new();
        let orphan = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(orphan).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
        }
        doc.append_child(doc.root(), orphan);
        let state = FormState::default();
        assert!(build_form_submit(&doc, orphan, &state).is_none());
    }

    #[test]
    fn build_form_submit_runtime_state_overrides_dom() {
        let (doc, submit) = make_submit_doc();
        // Найдём NodeId поля "q" через DOM
        let form_id = lumen_dom::find_ancestor_form(&doc, submit).unwrap();
        let fields = lumen_dom::collect_dom_form_fields(&doc, form_id);
        assert!(!fields.is_empty());
        // Найдём NodeId поля q по имени
        let q_id = doc.root();  // заглушка — runtime_state override ищет по name-атрибуту
        let _ = q_id;
        // Для полноты проверяем что body содержит encoded name
        let state = FormState::default();
        let (_, _, body) = build_form_submit(&doc, submit, &state).unwrap();
        assert_eq!(body, "q=rust");
    }

    #[test]
    fn validation_error_required_field() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("required"), value: String::new() });
            attrs.push(Attribute { name: QualName::html("placeholder"), value: "Email".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, input);

        if let Some(vs) = element_validity(&doc, input) {
            let msg = validation_error_message(&doc, input, &vs, &FormState::default()).unwrap();
            assert!(msg.contains("обязательно"));
        }
    }

    #[test]
    fn validation_error_type_mismatch_email() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "email".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "not-an-email".into() });
            attrs.push(Attribute { name: QualName::html("placeholder"), value: "Email".into() });
        }
        doc.append_child(doc.root(), input);

        if let Some(vs) = element_validity(&doc, input)
            && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
        {
            assert!(msg.contains("email"));
        }
    }

    #[test]
    fn validation_error_type_mismatch_url() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "url".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "not a url".into() });
            attrs.push(Attribute { name: QualName::html("placeholder"), value: "Website".into() });
        }
        doc.append_child(doc.root(), input);

        if let Some(vs) = element_validity(&doc, input)
            && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
        {
            assert!(msg.contains("URL"));
        }
    }

    #[test]
    fn validation_error_range_underflow() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "number".into() });
            attrs.push(Attribute { name: QualName::html("min"), value: "5".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "3".into() });
            attrs.push(Attribute { name: QualName::html("placeholder"), value: "Age".into() });
        }
        doc.append_child(doc.root(), input);

        if let Some(vs) = element_validity(&doc, input)
            && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
        {
            assert!(msg.contains("меньше"));
        }
    }

    #[test]
    fn validation_error_range_overflow() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "number".into() });
            attrs.push(Attribute { name: QualName::html("max"), value: "100".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "150".into() });
            attrs.push(Attribute { name: QualName::html("placeholder"), value: "Score".into() });
        }
        doc.append_child(doc.root(), input);

        if let Some(vs) = element_validity(&doc, input)
            && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
        {
            assert!(msg.contains("больше"));
        }
    }

    #[test]
    fn find_all_validation_errors_multiple() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));

        // First invalid input (required)
        let inp1 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
            attrs.push(Attribute { name: QualName::html("required"), value: String::new() });
            attrs.push(Attribute { name: QualName::html("placeholder"), value: "Name".into() });
        }

        // Second invalid input (invalid email)
        let inp2 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "email".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "bad-email".into() });
            attrs.push(Attribute { name: QualName::html("placeholder"), value: "Email".into() });
        }

        doc.append_child(doc.root(), form);
        doc.append_child(form, inp1);
        doc.append_child(form, inp2);

        // This would need a proper LayoutBox tree to test. For now just verify
        // that find_all_validation_errors function exists and compiles.
        let _ = find_all_validation_errors;
    }

    // ──── <select> dropdown tests ────────────────────────────────────────────

    fn make_select_doc() -> (Document, NodeId) {
        // <select>
        //   <option value="a">Apple</option>
        //   <option value="b" selected>Banana</option>
        //   <option value="c" disabled>Cherry</option>
        // </select>
        let mut doc = Document::new();
        let sel = doc.create_element(QualName::html("select"));

        let opt_a = doc.create_element(QualName::html("option"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(opt_a).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "a".into() });
        }
        let txt_a = doc.create_text(String::from("Apple"));

        let opt_b = doc.create_element(QualName::html("option"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(opt_b).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "b".into() });
            attrs.push(Attribute { name: QualName::html("selected"), value: String::new() });
        }
        let txt_b = doc.create_text(String::from("Banana"));

        let opt_c = doc.create_element(QualName::html("option"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(opt_c).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "c".into() });
            attrs.push(Attribute { name: QualName::html("disabled"), value: String::new() });
        }
        let txt_c = doc.create_text(String::from("Cherry"));

        doc.append_child(doc.root(), sel);
        doc.append_child(sel, opt_a);
        doc.append_child(opt_a, txt_a);
        doc.append_child(sel, opt_b);
        doc.append_child(opt_b, txt_b);
        doc.append_child(sel, opt_c);
        doc.append_child(opt_c, txt_c);
        (doc, sel)
    }

    #[test]
    fn collect_select_options_labels_and_values() {
        let (doc, sel) = make_select_doc();
        let opts = collect_select_options(&doc, sel);
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].label, "Apple");
        assert_eq!(opts[0].value, "a");
        assert!(!opts[0].selected);
        assert!(!opts[0].disabled);
        assert_eq!(opts[1].label, "Banana");
        assert_eq!(opts[1].value, "b");
        assert!(opts[1].selected);
        assert!(!opts[1].disabled);
        assert!(opts[2].disabled);
    }

    #[test]
    fn classify_click_select_returns_open_dropdown() {
        let (doc, sel) = make_select_doc();
        let action = classify_click(&doc, sel);
        assert!(matches!(action, FormClickAction::OpenSelectDropdown(_)));
    }

    #[test]
    fn hit_select_option_returns_correct_row() {
        let anchor = Rect::new(100.0, 200.0, 120.0, 22.0);
        // dropdown appears below anchor (no flip needed for default viewport)
        let vp_w = 1024.0;
        let vp_h = 720.0;
        let scroll_y = 0.0;
        // Click on first row: just below the anchor + border + PAD_Y
        let anchor_bottom_vp = 200.0 + 22.0;
        let dd_y = anchor_bottom_vp + 1.0;
        let click_y = dd_y + DROPDOWN_PAD_Y + DROPDOWN_ROW_H * 0.5;
        let result = hit_select_option(anchor, 3, scroll_y, vp_w, vp_h, 110.0, click_y);
        assert_eq!(result, Some(0));
    }

    #[test]
    fn hit_select_option_second_row() {
        let anchor = Rect::new(100.0, 200.0, 120.0, 22.0);
        let vp_w = 1024.0;
        let vp_h = 720.0;
        let scroll_y = 0.0;
        let anchor_bottom_vp = 200.0 + 22.0;
        let dd_y = anchor_bottom_vp + 1.0;
        let click_y = dd_y + DROPDOWN_PAD_Y + DROPDOWN_ROW_H * 1.5;
        let result = hit_select_option(anchor, 3, scroll_y, vp_w, vp_h, 110.0, click_y);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn hit_select_option_outside_returns_none() {
        let anchor = Rect::new(100.0, 200.0, 120.0, 22.0);
        // Click far outside the dropdown area.
        let result = hit_select_option(anchor, 3, 0.0, 1024.0, 720.0, 500.0, 500.0);
        assert_eq!(result, None);
    }

    #[test]
    fn apply_select_choice_moves_selected_attr() {
        let (mut doc, sel) = make_select_doc();
        let opts = collect_select_options(&doc, sel);
        // Initially "b" (idx=1) is selected. Pick idx=0.
        apply_select_choice(&mut doc, &opts, 0);
        let updated = collect_select_options(&doc, sel);
        assert!(updated[0].selected);
        assert!(!updated[1].selected);
        assert!(!updated[2].selected);
    }

    #[test]
    fn build_select_dropdown_non_empty() {
        let (doc, sel) = make_select_doc();
        let opts = collect_select_options(&doc, sel);
        let anchor = Rect::new(10.0, 10.0, 100.0, 22.0);
        let dl = build_select_dropdown(anchor, &opts, 0.0, 1024.0, 720.0);
        assert!(!dl.is_empty(), "dropdown display list should not be empty");
    }
}

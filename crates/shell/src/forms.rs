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

use lumen_core::form::{encode_form_multipart, encode_form_urlencoded, FormEntry, FormValue};
use lumen_core::geom::{Rect, Size};
use lumen_dom::{
    check_validity_form, collect_dom_form_fields, element_validity, find_ancestor_form,
    invalid_controls_in_form, set_radio_checked, Attribute, Document, InputType,
    NodeData, NodeId, QualName,
};
use lumen_layout::{
    compute_style, BorderStyle, BoxKind, Color, ComputedStyle, FontStyle, FontWeight, LayoutBox,
    Mat4,
};
use lumen_paint::{build_display_list, DisplayCommand, DisplayList};

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
    /// Char-index text cursor for a typeable `<input>`/`<textarea>` (FRAME-2
    /// п.1). `None` means "not yet touched by cursor movement" — the field
    /// reads as end-of-text, matching the append-only behaviour it had before
    /// cursor tracking existed. Not round-tripped through hibernation/tab
    /// snapshots (always `None` after restore) — only the value persists,
    /// same tradeoff `checked` already made for those paths.
    pub cursor: Option<usize>,
    /// Char-index selection anchor (FRAME-7 remainder 2 — Shift+arrow text
    /// selection). `Some(a)` together with `cursor == Some(c)` and `a != c`
    /// means chars `[min(a,c), max(a,c))` are selected; `None` (the common
    /// case) means no selection is active. Set when a Shift+arrow first
    /// extends past a bare cursor, cleared by any unshifted cursor movement
    /// or by an edit that consumes the selection. Not round-tripped through
    /// hibernation/tab snapshots, same as `cursor`.
    pub selection_anchor: Option<usize>,
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
    /// Open a calendar date-picker overlay for `<input type="date/datetime-local/time/month/week">`.
    OpenDatePicker(NodeId),
    /// Open a dropdown overlay showing the `<option>` children of the `<select>`.
    OpenSelectDropdown(NodeId),
    /// Open the OS native file-picker dialog for `<input type="file">`.
    ///
    /// Shell reads `accept` + `multiple` attributes, calls `platform::file_dialog::open_file_dialog`,
    /// then evals `_lumen_deliver_file_list(nid, json)` to push results into JS.
    OpenFilePicker(NodeId),
    SubmitForm(NodeId),
    /// Toggle the `open` attribute on a `<details>` element (HTML5 §4.11.1).
    /// NodeId is the `<details>` parent of the clicked `<summary>`.
    ToggleDetails(NodeId),
    /// Slide a range input. Shell resolves the new value from click coords + box rect.
    SlideRange(NodeId),
    Nothing,
}

/// Tags this module knows how to activate — the `match` in [`classify_click`]
/// below, kept separate so [`activation_target`] can ask «does this node have a
/// behaviour» without running it.
fn is_activatable(tag: &str) -> bool {
    matches!(tag, "input" | "button" | "select" | "selectlist" | "summary" | "label")
}

/// Interactive content with no activation behaviour of its own. The walk stops
/// here instead of continuing to an ancestor: HTML LS §4.10.20 makes a `<label>`
/// do nothing for events targeted at such a descendant, so a click inside the
/// `<textarea>` of a `<label for=cb>` must not toggle `cb`.
fn is_activation_barrier(tag: &str) -> bool {
    matches!(tag, "textarea" | "iframe" | "embed" | "object")
}

/// DOM Standard §2.9 «activation target»: the nearest inclusive ancestor of the
/// clicked node that has an activation behaviour. A hit test lands on whatever
/// box is under the cursor — the text inside a `<label>`, the `<img>` inside a
/// `<button>` — and the behaviour belongs to the ancestor, not to that node
/// (BUG-837, the native-click half; [`links::find_link_href`] already walks the
/// same chain for `<a href>`).
fn activation_target(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut current = node;
    loop {
        let n = doc.get(current);
        if let Some(tag) = n.element_name().map(|q| q.local.as_str()) {
            if is_activatable(tag) {
                return Some(current);
            }
            if is_activation_barrier(tag) {
                return None;
            }
        }
        current = n.parent?;
    }
}

/// HTML LS §4.10.4 — a `<label>`'s labeled control: the element named by `for`,
/// else its first labelable descendant in tree order. A label whose `for` names
/// a non-labelable element (or nothing) has no control at all, and per spec that
/// is not a fallback to the descendant search.
fn labeled_control(doc: &Document, label: NodeId) -> Option<NodeId> {
    let is_labelable = |id: NodeId| {
        doc.get(id)
            .element_name()
            .map(|q| q.local.as_str())
            .is_some_and(|t| {
                matches!(
                    t,
                    "input" | "select" | "textarea" | "button" | "output" | "progress" | "meter"
                )
            })
    };
    if let Some(target) = doc.get(label).get_attr("for") {
        if target.is_empty() {
            return None;
        }
        return doc.find_by_id(target).filter(|&id| is_labelable(id));
    }
    let mut stack: Vec<NodeId> = doc.get(label).children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        if is_labelable(id) {
            return Some(id);
        }
        stack.extend(doc.get(id).children.iter().rev().copied());
    }
    None
}

/// Classify a click on `node` given the current DOM tree.
pub fn classify_click(doc: &Document, node: NodeId) -> FormClickAction {
    let Some(target) = activation_target(doc, node) else {
        return FormClickAction::Nothing;
    };
    // A `<label>` forwards its activation to the labeled control (HTML LS
    // §4.10.4 «synthetic click activation steps»), so the action is the
    // control's — which is why the resolution happens before the match below
    // rather than as a branch inside it.
    let target = match doc.get(target).element_name().map(|q| q.local.as_str()) {
        Some("label") => match labeled_control(doc, target) {
            Some(control) => control,
            None => return FormClickAction::Nothing,
        },
        _ => target,
    };
    let n = doc.get(target);
    let tag = n.element_name().map(|q| q.local.as_str()).unwrap_or("");
    // A disabled form control has no activation behaviour (HTML LS §4.10.19).
    // The node the hit test landed on used to be the node classified, so this
    // could not come up before the walk above; now a click on the `<span>`
    // inside a `<button disabled>` reaches the button.
    if n.get_attr("disabled").is_some()
        && matches!(
            tag,
            "input" | "select" | "selectlist" | "textarea" | "button" | "option" | "optgroup" | "fieldset"
        )
    {
        return FormClickAction::Nothing;
    }
    let node = target;
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
                InputType::Date
                | InputType::DateTimeLocal
                | InputType::Time
                | InputType::Month
                | InputType::Week => FormClickAction::OpenDatePicker(node),
                InputType::File => FormClickAction::OpenFilePicker(node),
                InputType::Submit => FormClickAction::SubmitForm(node),
                InputType::Range => FormClickAction::SlideRange(node),
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
        // `<selectlist>` (Customizable Select) opens the same option popup.
        "select" | "selectlist" => FormClickAction::OpenSelectDropdown(node),
        // HTML5 §4.11.1 — clicking <summary> toggles its parent <details>.
        "summary" => find_parent_details(doc, node)
            .map_or(FormClickAction::Nothing, FormClickAction::ToggleDetails),
        _ => FormClickAction::Nothing,
    }
}

/// Walk up the DOM tree from `node` to find the nearest `<details>` ancestor.
fn find_parent_details(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut current = doc.get(node).parent?;
    loop {
        let n = doc.get(current);
        if n.element_name().map(|q| q.local.as_str()) == Some("details") {
            return Some(current);
        }
        current = n.parent?;
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// DOM mutation helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Toggle the `open` attribute on a `<details>` element in the live DOM.
///
/// Adds `open` when absent (expand), removes it when present (collapse).
/// After calling this, relayout is needed so hidden children become visible.
pub fn toggle_details_open(doc: &mut Document, id: NodeId) {
    let node = doc.get_mut(id);
    if let NodeData::Element { ref mut attrs, .. } = node.data {
        if attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("open")) {
            attrs.retain(|a| !a.name.local.eq_ignore_ascii_case("open"));
        } else {
            attrs.push(Attribute { name: QualName::html("open"), value: String::new() });
        }
    }
}

/// Toggle a checkbox/radio input's current checkedness in the live DOM.
/// After calling this, relayout is needed to update `:checked` pseudo-class.
///
/// Writes the control's runtime checkedness, not the `checked` content
/// attribute: HTML LS §4.10.5.5 keeps the attribute as the *default* that
/// `defaultChecked` reads and `form.reset()` restores. A native mouse click
/// used to write the attribute directly, which destroyed the default the
/// moment a script hadn't already snapshotted it — the one path BUG-383's
/// `_lumen_default_checked` workaround could not cover (BUG-444).
pub fn toggle_checkbox(doc: &mut Document, id: NodeId) {
    let checked = doc.control_checked(id);
    doc.set_control_checked(id, !checked);
}

/// Apply a native click on an `<input type=radio>` in the live DOM.
///
/// Unlike a checkbox, a radio button's click behaviour is not a toggle
/// (HTML LS §4.10.5.1.14): it checks `clicked` and unchecks every other
/// radio button in its group (same `name`, nearest form owner). See
/// [`lumen_dom::set_radio_checked`] for the group-resolution rules. After
/// calling this, relayout is needed to update `:checked`/`:indeterminate`.
pub fn toggle_radio(doc: &mut Document, id: NodeId) {
    set_radio_checked(doc, id);
}

/// Set the current value of an `<input>` in the live DOM — typing, a picker
/// choice, a spellcheck replacement.
///
/// Writes the control's runtime («dirty») value, not the `value` content
/// attribute: HTML LS §4.10.5.5 keeps the attribute as the *default* value that
/// `defaultValue` reads and `form.reset()` restores. Layout, form submission
/// and constraint validation all read the runtime value via
/// [`Document::control_value`], so this is the single write path (BUG-441).
pub fn set_value(doc: &mut Document, id: NodeId, value: &str) {
    doc.set_control_value(id, value);
}

/// Set a `<textarea>`'s current value (P3-spell slice 4).
///
/// Like [`set_value`], and for the same reason: the textarea's text-node
/// children are its *default* value (HTML LS §4.10.11 — text content model),
/// so editing must not rewrite them. The runtime value shadows the children
/// for everything that renders or submits the field.
pub fn set_textarea_text(doc: &mut Document, id: NodeId, new_text: &str) {
    doc.set_control_value(id, new_text);
}

/// Update a range input's `value` attribute from a click at `click_x` within
/// its bounding rect. Reads `min`/`max` from the DOM and clamps the result.
///
/// `slider_rect` is the content-box rect of the `<input type="range">` element.
/// `click_x` is the page-space x coordinate.
pub fn apply_range_value(doc: &mut Document, id: NodeId, slider_rect: lumen_core::geom::Rect, click_x: f32) {
    let thumb_r = 8.0_f32;
    let track_x = slider_rect.x + thumb_r / 2.0;
    let track_w = (slider_rect.width - thumb_r).max(1.0);

    let fraction = ((click_x - track_x) / track_w).clamp(0.0, 1.0);

    let min: f32 = doc.get(id).get_attr("min")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0.0);
    let max: f32 = doc.get(id).get_attr("max")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(100.0);

    let value = min + fraction * (max - min);
    set_value(doc, id, &format!("{:.6}", value));
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

/// Find the LayoutBox subtree for `node`. Returns `None` if the node has no box.
pub fn find_layout_box(root: &LayoutBox, node: NodeId) -> Option<&LayoutBox> {
    if root.node == node {
        return Some(root);
    }
    for child in &root.children {
        if let Some(lb) = find_layout_box(child, node) {
            return Some(lb);
        }
    }
    None
}

/// FRAME-7 (remainder item 1): caret bar rect for a focused `<textarea>`,
/// computed straight from the layout box tree rather than the painted
/// display list — cursor movement (Left/Right/Home/End) does not force a
/// relayout, but it also does not move any glyph, so re-reading `field_box`'s
/// (cached, still valid) geometry each frame is correct.
///
/// `field_box` is the `<textarea>`'s `FormControl` box. Its value is ordinary
/// inline content (BUG-441), laid out as a child `InlineRun` box only once the
/// control has a runtime ("dirty") value — an untouched, freshly focused
/// textarea still shows its default DOM content and has no such child yet, so
/// this falls back to the box's own content edge (border + padding) for line
/// 0. Multi-line placement walks `value`'s `\n`-split *logical* lines, not the
/// `InlineRun`'s real *visual* lines — a `pre-wrap` line that word-wraps into
/// more than one visual row is not accounted for (the same tolerance for
/// approximation `emit_input_caret` already documents for `<input>`).
pub fn textarea_caret_rect(
    field_box: &LayoutBox,
    value: &str,
    cursor: usize,
    measure: &dyn Fn(&str) -> f32,
) -> Rect {
    let s = &field_box.style;
    let inline_run = field_box
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }));
    let (content_x, content_y) = match inline_run {
        Some(ir) => (ir.rect.x, ir.rect.y),
        None => (
            field_box.rect.x + s.border_left_width + s.padding_left.px(),
            field_box.rect.y + s.border_top_width + s.padding_top.px(),
        ),
    };
    let raw_line_h = field_box.used_line_height;
    let line_h = if s.line_height_step > 0.0 {
        (raw_line_h / s.line_height_step).ceil() * s.line_height_step
    } else {
        raw_line_h
    };

    let mut remaining = cursor;
    let mut line_idx = 0usize;
    let mut prefix = String::new();
    for (i, line) in value.split('\n').enumerate() {
        line_idx = i;
        let n = line.chars().count();
        if remaining <= n {
            prefix = line.chars().take(remaining).collect();
            break;
        }
        remaining -= n + 1; // +1 for the '\n' consumed between logical lines
        prefix.clear();
    }

    Rect::new(content_x + measure(&prefix), content_y + line_idx as f32 * line_h, 1.0, line_h)
}

/// FRAME-7 remainder (1): caret bar rect for a focused `<input>` INSIDE a
/// frame, computed straight from the layout box tree — a shell-side overlay
/// mirroring [`textarea_caret_rect`], not the page `<input>`'s
/// `CompositorOverride` channel (`lumen_paint`'s `emit_input_caret`, whose
/// geometry this duplicates): a frame's `content_dl` is rebuilt only on
/// relayout (`frames::rebuild_frame_display_lists`'s dirty gate), so wiring
/// the compositor-override channel through it is a much larger change than a
/// per-redraw shell overlay — see FRAME-7's ROADMAP "Остаток" note.
///
/// `field_box` is the `<input>`'s `FormControl` box. Same 2px content inset
/// and per-glyph advance approximation as `emit_input_caret` — this crate has
/// no real `TextMeasurer` either.
pub fn input_caret_rect(field_box: &LayoutBox, value: &str, char_index: usize) -> Rect {
    let s = &field_box.style;
    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = field_box.rect.x + bl + inset;
    let content_y = field_box.rect.y + bt;
    let content_h = (field_box.rect.height - bt - bb).max(1.0);
    let font_size = s.font_size;

    let chars_before = char_index.min(value.chars().count());
    let caret_x = content_x + font_size * 0.5 * chars_before as f32;

    Rect::new(caret_x, content_y, 1.0, content_h)
}

/// FRAME-7 remainder 2: fallback text-selection highlight colour for a
/// typeable field's shell-side overlays (frame `<input>`, page/frame
/// `<textarea>`) — same value as `lumen_paint`'s `SELECTION_HIGHLIGHT_DEFAULT`
/// (the page `<input>` path, which rides the `CompositorOverride` channel
/// instead and lives in that crate). Kept as a separate constant rather than
/// exported from `lumen-paint`: this crate already duplicates the caret
/// geometry helpers for the same "different rendering mechanism" reason (see
/// `textarea_caret_rect`'s doc comment).
pub const SELECTION_HIGHLIGHT_DEFAULT: Color = Color { r: 0x30, g: 0x8a, b: 0xff, a: 110 };

/// FRAME-7 remainder 2: selection highlight rect for a focused `<input>`
/// INSIDE a frame, char range `[start, end)` — mirror of [`input_caret_rect`]
/// the same way [`textarea_selection_rects`] mirrors [`textarea_caret_rect`].
/// Returns a zero-width rect when the range is empty; callers should not draw
/// a fill for it (mirroring `lumen_paint::emit_input_selection`'s own guard).
pub fn input_selection_rect(field_box: &LayoutBox, value: &str, start: usize, end: usize) -> Rect {
    let s = &field_box.style;
    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = field_box.rect.x + bl + inset;
    let content_y = field_box.rect.y + bt;
    let content_h = (field_box.rect.height - bt - bb).max(1.0);
    let font_size = s.font_size;

    let len = value.chars().count();
    let start = start.min(len);
    let end = end.min(len);
    let sel_x = content_x + font_size * 0.5 * start as f32;
    let sel_w = font_size * 0.5 * end.saturating_sub(start) as f32;

    Rect::new(sel_x, content_y, sel_w, content_h)
}

/// FRAME-7 remainder 2: selection highlight rects for a focused `<textarea>`,
/// char range `[start, end)` — one rect per logical line the range overlaps,
/// same logical-line/visual-line tolerance [`textarea_caret_rect`] documents.
/// A range spanning N lines yields N rects (start clipped to the range on the
/// first line, end clipped on the last, full line width in between); an empty
/// or out-of-bounds range yields no rects.
pub fn textarea_selection_rects(
    field_box: &LayoutBox,
    value: &str,
    start: usize,
    end: usize,
    measure: &dyn Fn(&str) -> f32,
) -> Vec<Rect> {
    if start >= end {
        return Vec::new();
    }
    let s = &field_box.style;
    let inline_run = field_box
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }));
    let (content_x, content_y) = match inline_run {
        Some(ir) => (ir.rect.x, ir.rect.y),
        None => (
            field_box.rect.x + s.border_left_width + s.padding_left.px(),
            field_box.rect.y + s.border_top_width + s.padding_top.px(),
        ),
    };
    let raw_line_h = field_box.used_line_height;
    let line_h = if s.line_height_step > 0.0 {
        (raw_line_h / s.line_height_step).ceil() * s.line_height_step
    } else {
        raw_line_h
    };

    let mut rects = Vec::new();
    let mut offset = 0usize;
    for (line_idx, line) in value.split('\n').enumerate() {
        let n = line.chars().count();
        let line_start = offset;
        let line_end = line_start + n;
        offset = line_end + 1; // +1 for the '\n' consumed between logical lines

        if start >= line_end || end <= line_start {
            continue;
        }
        let sel_start_in_line = start.max(line_start) - line_start;
        let sel_end_in_line = end.min(line_end) - line_start;
        if sel_start_in_line >= sel_end_in_line {
            continue;
        }
        let prefix: String = line.chars().take(sel_start_in_line).collect();
        let selected: String =
            line.chars().skip(sel_start_in_line).take(sel_end_in_line - sel_start_in_line).collect();
        let x = content_x + measure(&prefix);
        let w = measure(&selected).max(1.0);
        rects.push(Rect::new(x, content_y + line_idx as f32 * line_h, w, line_h));
    }
    rects
}

/// FRAME-7 остаток: char-index under viewport-space `x` for a focused
/// `<input>` — the inverse of [`input_caret_rect`]/[`input_selection_rect`],
/// used to place the cursor/anchor for a mouse press or drag. Same content
/// inset and per-glyph `font_size * 0.5` advance approximation those two
/// share, so a hit test and the caret it later paints always agree.
/// Clamped to `[0, value length]` — a click past either edge of the field
/// lands on the nearest end, matching every other text editor.
pub fn input_char_index_at_x(field_box: &LayoutBox, value: &str, x: f32) -> usize {
    let s = &field_box.style;
    let bl = s.border_left_width;
    let inset = 2.0_f32;
    let content_x = field_box.rect.x + bl + inset;
    let font_size = s.font_size;
    let len = value.chars().count();
    if font_size <= 0.0 {
        return len;
    }
    let raw = (x - content_x) / (font_size * 0.5);
    raw.round().clamp(0.0, len as f32) as usize
}

/// FRAME-7 остаток: char-index under document-space `(x, y)` for a focused
/// `<textarea>` — the inverse of [`textarea_caret_rect`]/[`textarea_selection_rects`],
/// same logical-line tolerance those document (a `pre-wrap` visual wrap is
/// not accounted for). `y` picks the logical line via the same `line_h` those
/// use; `x` within that line is found by the closest char-boundary prefix
/// width, mirroring the O(n) glyph-advance scan `textarea_selection_rects`
/// already does per selected line — textarea values are short shell-side
/// text, not a reason to keep a running-width cache.
pub fn textarea_char_index_at_point(
    field_box: &LayoutBox,
    value: &str,
    x: f32,
    y: f32,
    measure: &dyn Fn(&str) -> f32,
) -> usize {
    let s = &field_box.style;
    let inline_run = field_box
        .children
        .iter()
        .find(|c| matches!(c.kind, BoxKind::InlineRun { .. }));
    let (content_x, content_y) = match inline_run {
        Some(ir) => (ir.rect.x, ir.rect.y),
        None => (
            field_box.rect.x + s.border_left_width + s.padding_left.px(),
            field_box.rect.y + s.border_top_width + s.padding_top.px(),
        ),
    };
    let raw_line_h = field_box.used_line_height;
    let line_h = if s.line_height_step > 0.0 {
        (raw_line_h / s.line_height_step).ceil() * s.line_height_step
    } else {
        raw_line_h
    }
    .max(1.0);

    let lines: Vec<&str> = value.split('\n').collect();
    let rel_line = ((y - content_y) / line_h).floor();
    let line_idx = if rel_line < 0.0 {
        0
    } else {
        (rel_line as usize).min(lines.len().saturating_sub(1))
    };

    let mut offset = 0usize;
    for line in &lines[..line_idx] {
        offset += line.chars().count() + 1; // +1 for the '\n' consumed between logical lines
    }
    let line = lines[line_idx];
    let chars: Vec<char> = line.chars().collect();
    let rel_x = x - content_x;
    let mut best_i = 0usize;
    let mut best_dist = f32::MAX;
    for i in 0..=chars.len() {
        let prefix: String = chars[..i].iter().collect();
        let dist = (measure(&prefix) - rel_x).abs();
        if dist < best_dist {
            best_dist = dist;
            best_i = i;
        }
    }
    offset + best_i
}

/// Walk `doc` and collect all NodeIds with `data-lumen-modal` attribute
/// (set by `dialog.showModal()`, cleared by `close()` and Escape handler).
/// Returns them in DOM order (first opened first).
pub fn collect_modal_dialogs(doc: &Document) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_modal_dialogs_in(doc, doc.root(), &mut out);
    out
}

fn collect_modal_dialogs_in(doc: &Document, node: NodeId, out: &mut Vec<NodeId>) {
    if doc.get(node).get_attr("data-lumen-modal").is_some() {
        out.push(node);
    }
    for &child in &doc.get(node).children.clone() {
        collect_modal_dialogs_in(doc, child, out);
    }
}

/// Build a `::backdrop` + translated dialog overlay for a modal `<dialog>`.
///
/// Renders the `::backdrop` (full-viewport dim) followed by the dialog content
/// shifted to the center of the viewport. The dialog paint commands are produced
/// by re-walking `dialog_lb` via `build_display_list`, then wrapped in a
/// `PushTransform`/`PopTransform` pair that translates document-space coords
/// to viewport-space center. (HTML5 §4.11.7 — top-layer rendering emulation.)
///
/// `scroll_y` is the current page scroll in CSS px (document-space → viewport-space).
pub fn build_dialog_overlay(
    dialog_lb: &LayoutBox,
    scroll_y: f32,
    vp_w: f32,
    vp_h: f32,
) -> DisplayList {
    let r = dialog_lb.rect;
    // Center the dialog in the viewport.
    // dialog center in viewport-space: x stays, y adjusted for scroll.
    let dlg_cx_doc = r.x + r.width * 0.5;
    let dlg_cy_vp = r.y + r.height * 0.5 - scroll_y;
    let dx = vp_w * 0.5 - dlg_cx_doc;
    let dy = vp_h * 0.5 - dlg_cy_vp;

    let mut out: DisplayList = Vec::new();

    // ::backdrop — full viewport, semi-transparent (HTML5 §15.3.9 UA default).
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, vp_w, vp_h),
        color: Color { r: 0, g: 0, b: 0, a: 100 },
    });

    // Dialog content: re-paint from layout subtree, shifted to viewport center.
    let dialog_dl = build_display_list(dialog_lb);
    out.push(DisplayCommand::PushTransform { matrix: Mat4::translation_2d(dx, dy) });
    out.extend(dialog_dl);
    out.push(DisplayCommand::PopTransform);

    out
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
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: Rect::new(vp_x + TOOLTIP_PADDING, vp_y + TOOLTIP_PADDING, w - TOOLTIP_PADDING * 2.0, TOOLTIP_FONT_SIZE),
        text: message.to_owned(),
        font_size: TOOLTIP_FONT_SIZE,
        color: Color { r: 20, g: 20, b: 20, a: 255 },
        font_family: vec![],
        font_weight: FontWeight::new(400),
        font_style: FontStyle::Normal,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    });
    out
}

// ──────────────────────────────────────────────────────────────────────────────
// Password-field indicator (Caps Lock, layout)
// ──────────────────────────────────────────────────────────────────────────────

const INDICATOR_FONT_SIZE: f32 = 12.0;
const INDICATOR_PAD_X: f32 = 5.0;
const INDICATOR_PAD_Y: f32 = 3.0;
const INDICATOR_GAP: f32 = 4.0;

/// Small browser-drawn badges (`⇪` Caps Lock, `RU` layout) hugging the top-right
/// corner of a focused password field, one pill per entry of `labels`.
///
/// The badges sit **outside** the field, above its top edge, so they never cover
/// the field's own text or the site's "show password" button that lives inside
/// its right end; when the field is flush with the top of the viewport they go
/// below it instead. Part of the browser overlay, hence invisible to scripts.
/// `anchor` is in document coordinates, the result is viewport-fixed.
pub fn build_input_indicator(anchor: Rect, labels: &[&str], scroll_y: f32, viewport_w: f32) -> DisplayList {
    let mut out: DisplayList = Vec::new();
    if labels.is_empty() {
        return out;
    }
    let h = INDICATOR_FONT_SIZE + INDICATOR_PAD_Y * 2.0;
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| l.chars().count() as f32 * 8.0 + INDICATOR_PAD_X * 2.0)
        .collect();
    let total = widths.iter().sum::<f32>() + INDICATOR_GAP * (labels.len() - 1) as f32;

    let field_top = anchor.y - scroll_y;
    let y = if field_top >= h + 2.0 { field_top - h - 2.0 } else { field_top + anchor.height + 2.0 };
    let mut x = (anchor.x + anchor.width - total).min(viewport_w - total).max(0.0);

    for (label, w) in labels.iter().zip(widths) {
        let pill = Rect::new(x, y, w, h);
        out.push(DisplayCommand::FillRect { rect: pill, color: Color { r: 45, g: 45, b: 48, a: 235 } });
        out.push(DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: Rect::new(x + INDICATOR_PAD_X, y + INDICATOR_PAD_Y, w - INDICATOR_PAD_X * 2.0, INDICATOR_FONT_SIZE),
            text: (*label).to_owned(),
            font_size: INDICATOR_FONT_SIZE,
            color: Color { r: 255, g: 255, b: 255, a: 255 },
            font_family: vec![],
            font_weight: FontWeight::new(600),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        });
        x += w + INDICATOR_GAP;
    }
    out
}

// ──────────────────────────────────────────────────────────────────────────────
// Form submission
// ──────────────────────────────────────────────────────────────────────────────

/// Собрать данные формы для submit.
///
/// HTML LS §constructing-form-data-set: обходим submittable-контролы формы.
/// Значение `<input>`/`<textarea>` берёт [`Document::control_value`] внутри
/// `collect_dom_form_fields` — это единственный источник истины для текстовых
/// полей (BUG-441), поэтому и набранное пользователем, и присвоенное скриптом
/// `el.value` попадают в набор данных сами собой.
///
/// `FormState` остаётся наложением только для контролов, чьё runtime-значение
/// DOM не хранит (`<select>`, выбранный из shell-овского оверлея): у них нет
/// dirty value, и именно этот случай подхватывает поиск по `name`.
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
                        // Контрол с собственным runtime-значением уже отдал его
                        // через DOM — накладывать поверх снимок из `FormState`
                        // нельзя: он не знает о записях из JS и переживает
                        // `form.reset()`.
                        && doc.dirty_value(**id).is_none()
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
/// Encode form fields for submission. Wraps a FormSubmitEvent::Valid variant
/// and encodes fields as application/x-www-form-urlencoded.
pub fn encode_form_fields(fields: &[(String, String)]) -> String {
    let entries: Vec<FormEntry> = fields
        .iter()
        .map(|(name, value)| FormEntry { name: name.clone(), value: FormValue::Text(value.clone()) })
        .collect();
    encode_form_urlencoded(&entries)
}

/// Encode form fields as `multipart/form-data` (RFC 7578).
///
/// Returns `(content_type, body_bytes)` where `content_type` is
/// `multipart/form-data; boundary=<boundary>`.
/// Boundary is deterministic for Phase 0; callers may supply their own.
pub fn encode_form_fields_multipart(fields: &[(String, String)], boundary: &str) -> (String, Vec<u8>) {
    let entries: Vec<FormEntry> = fields
        .iter()
        .map(|(name, value)| FormEntry { name: name.clone(), value: FormValue::Text(value.clone()) })
        .collect();
    let body = encode_form_multipart(&entries, boundary);
    let ct = format!("multipart/form-data; boundary={boundary}");
    (ct, body)
}

/// Encode form fields with the `text/plain` algorithm (HTML LS §4.10.21.8):
/// one `name=value` line per field, each terminated by CRLF, no escaping.
///
/// Returns `(content_type, body_bytes)`; the content type is
/// `text/plain;charset=UTF-8` as the entity-body submission step requires.
pub fn encode_form_fields_plain(fields: &[(String, String)]) -> (String, Vec<u8>) {
    let mut out = String::new();
    for (name, value) in fields {
        out.push_str(name);
        out.push('=');
        out.push_str(value);
        out.push_str("\r\n");
    }
    ("text/plain;charset=UTF-8".to_owned(), out.into_bytes())
}

/// Return the `enctype` attribute of the `<form>` ancestor of `submit_node`,
/// normalised to lower-case. Default: `"application/x-www-form-urlencoded"`.
///
/// Kept alongside [`enctype_of_form`] (which the live submission path uses since
/// BUG-383) for callers that only hold a control inside the form; exercised by
/// the `get_form_enctype_*` tests below.
#[allow(dead_code)]
pub fn get_form_enctype(doc: &Document, submit_node: NodeId) -> String {
    match find_ancestor_form(doc, submit_node) {
        Some(form_id) => enctype_of_form(doc, form_id),
        None => "application/x-www-form-urlencoded".to_string(),
    }
}

/// Normalised `enctype` of the `<form>` element `form_id` itself.
///
/// Same rules as [`get_form_enctype`], but taking the form directly instead of
/// a control inside it — which is what a script-initiated `form.submit()` has
/// (BUG-383); `find_ancestor_form` is strictly ancestor-walking and would miss
/// the form when handed the form itself.
pub fn enctype_of_form(doc: &Document, form_id: NodeId) -> String {
    let enctype = doc
        .get(form_id)
        .get_attr("enctype")
        .unwrap_or("application/x-www-form-urlencoded")
        .to_ascii_lowercase();
    // HTML LS §4.10.18.6: valid enctype values.
    match enctype.as_str() {
        "multipart/form-data" | "text/plain" => enctype,
        _ => "application/x-www-form-urlencoded".to_string(),
    }
}

/// Submit-button overrides of the form's own submission attributes (HTML LS
/// §4.10.18.6): `formaction`, `formmethod`, `formenctype`, `formtarget` on the
/// activated `submitter` win over `action`/`method`/`enctype`/`target` of the
/// form. Values the spec would reject (`formmethod` outside get/post/dialog,
/// `formenctype` outside the three enctypes) leave the form's own in place.
pub fn apply_submitter_overrides(
    doc: &Document,
    submitter: Option<NodeId>,
    action: &mut String,
    method: &mut String,
    enctype: &mut String,
    target: &mut String,
) {
    let Some(sub) = submitter else { return };
    if let Some(v) = lumen_dom::submitter_override(doc, sub, "formaction") {
        *action = v;
    }
    if let Some(v) = lumen_dom::submitter_override(doc, sub, "formmethod") {
        let v = v.to_ascii_lowercase();
        if matches!(v.as_str(), "get" | "post" | "dialog") {
            *method = v;
        }
    }
    if let Some(v) = lumen_dom::submitter_override(doc, sub, "formenctype") {
        let v = v.to_ascii_lowercase();
        if matches!(
            v.as_str(),
            "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
        ) {
            *enctype = v;
        }
    }
    if let Some(v) = lumen_dom::submitter_override(doc, sub, "formtarget") {
        *target = v;
    }
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
/// `<optgroup>` and `<listbox>` (the `<selectlist>` option container) children are
/// flattened (their `<option>` children are included).
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
            "optgroup" | "listbox" => collect_options_from(doc, child_id, out),
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
            font_stretch: lumen_layout::FontStretch::NORMAL,
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
            font_weight: FontWeight::new(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        });
    }

    out
}

/// Resolved per-`<option>` visual style for a base-select dropdown row.
///
/// Only the properties that do **not** change row geometry are honoured, so the
/// shared [`hit_select_option`] geometry stays valid for both the native and the
/// base-select dropdown.
#[derive(Debug, Clone)]
pub struct OptionRowStyle {
    /// Author `color` (falls back to the native option text colour).
    text: Color,
    /// Author `background-color`, if any (row is left transparent otherwise).
    background: Option<Color>,
    /// Author `font-size` (px).
    font_size: f32,
    /// Author `font-weight`.
    font_weight: FontWeight,
    /// Author `font-style`.
    font_style: FontStyle,
}

/// Resolve the author style of the visible `options` rows, inheriting from
/// `parent` (the `<select>`'s computed style). The only part of a base-select
/// dropdown that reads the document — BUG-1109 caches it under `try_lock`.
pub fn resolve_option_row_styles(
    doc: &Document,
    sheet: &lumen_css_parser::Stylesheet,
    parent: &ComputedStyle,
    options: &[SelectOption],
    viewport: Size,
    dark_mode: bool,
) -> Vec<OptionRowStyle> {
    options.iter().take(DROPDOWN_MAX_ROWS_VISIBLE).map(|opt| {
        let s = compute_style(doc, opt.node_id, sheet, parent, viewport, dark_mode);
        OptionRowStyle {
            text: s.color,
            background: s.background_color.map(|c| c.resolve(s.color)),
            font_size: s.font_size,
            font_weight: s.font_weight,
            font_style: s.font_style,
        }
    }).collect()
}

/// Build the option-list popover for a `<select appearance: base-select>`.
///
/// Unlike [`build_select_dropdown`] (fixed native colours), this honours author
/// CSS on each `<option>`: `color`, `background-color`, `font-size`,
/// `font-weight`, `font-style`. The selected row keeps the native highlight
/// unless the author set an explicit `background-color` on that option.
///
/// Row geometry is intentionally identical to the native dropdown so
/// [`hit_select_option`] stays valid for both paths. Author-driven per-row
/// heights and full `::picker(select)` container styling are deferred
/// (see `docs/tasks/ph3-select-base.md`, slice 4 follow-up). `row_styles` come
/// from [`resolve_option_row_styles`]; a row without one is not drawn.
#[allow(clippy::too_many_arguments)]
pub fn build_base_select_dropdown(
    anchor: Rect,
    select_style: &ComputedStyle,
    options: &[SelectOption],
    row_styles: &[OptionRowStyle],
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

    // Position: below anchor, flip above if it would overflow viewport bottom
    // (identical to `build_select_dropdown` / `hit_select_option`).
    let anchor_bottom_vp = anchor.y - scroll_y + anchor.height;
    let vp_y = if anchor_bottom_vp + h > viewport_h && anchor.y - scroll_y >= h {
        anchor.y - scroll_y - h
    } else {
        anchor_bottom_vp + 1.0
    };
    let vp_x = anchor.x.min(viewport_w - w).max(0.0);

    let bg = Rect::new(vp_x, vp_y, w, h);

    // Container: author `background-color`/`color` on the `<select>` bleed into
    // the picker surface; otherwise the native white/grey chrome.
    let container_bg = select_style
        .background_color
        .map(|c| c.resolve(select_style.color))
        .filter(|c| c.a != 0)
        .unwrap_or(Color { r: 255, g: 255, b: 255, a: 255 });
    out.push(DisplayCommand::FillRect { rect: bg, color: container_bg });
    out.push(DisplayCommand::DrawBorder {
        rect: bg,
        widths: [1.0; 4],
        colors: [Color { r: 180, g: 180, b: 180, a: 255 }; 4],
        styles: [BorderStyle::Solid; 4],
        radii: lumen_paint::CornerRadii::default(),
    });

    for (i, (opt, os)) in options.iter().zip(row_styles).take(DROPDOWN_MAX_ROWS_VISIBLE).enumerate() {
        let row_y = vp_y + DROPDOWN_PAD_Y + i as f32 * DROPDOWN_ROW_H;
        let row_rect = Rect::new(vp_x, row_y, w, DROPDOWN_ROW_H);

        // Row background: author `background-color` wins; else the native blue
        // highlight for the selected row; else transparent.
        if let Some(bg_col) = os.background {
            out.push(DisplayCommand::FillRect { rect: row_rect, color: bg_col });
        } else if opt.selected {
            out.push(DisplayCommand::FillRect {
                rect: row_rect,
                color: Color { r: 0, g: 120, b: 215, a: 255 },
            });
        }

        // Text colour: disabled greys out; else author `color`; the native blue
        // highlight forces white only when the author left the row background at
        // its initial (transparent) value.
        let text_color = if opt.disabled {
            Color { r: 150, g: 150, b: 150, a: 255 }
        } else if opt.selected && os.background.is_none() {
            Color { r: 255, g: 255, b: 255, a: 255 }
        } else {
            os.text
        };

        let font_size = os.font_size.max(1.0);
        out.push(DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: Rect::new(
                vp_x + DROPDOWN_PAD_X,
                row_y + (DROPDOWN_ROW_H - font_size) * 0.5,
                w - DROPDOWN_PAD_X * 2.0,
                font_size,
            ),
            text: opt.label.clone(),
            font_size,
            color: text_color,
            // Author `font-family` on the popover chrome is deferred with the
            // rest of `::picker(select)` styling — use the UA font.
            font_family: Vec::new(),
            font_weight: os.font_weight,
            font_style: os.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
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

// ──────────────────────────────────────────────────────────────────────────────
// Date picker overlay
// ──────────────────────────────────────────────────────────────────────────────

/// What a viewport-space click hit inside an open date picker.
#[derive(Debug, PartialEq, Eq)]
pub enum DatePickerHit {
    /// Click landed on the previous-month arrow.
    Prev,
    /// Click landed on the next-month arrow.
    Next,
    /// Click landed on calendar day `day` (1-based).
    Day(u8),
    /// Click outside the picker (or in an empty cell).
    None,
}

const DATE_CELL_W: f32 = 24.0;
const DATE_CELL_H: f32 = 24.0;
const DATE_CELL_GAP: f32 = 2.0;
const DATE_PICKER_PAD: f32 = 6.0;
const DATE_HEADER_H: f32 = 26.0;
const DATE_DAYNAME_H: f32 = 18.0;
const DATE_ROWS: usize = 6;

fn date_picker_size() -> (f32, f32) {
    let w = 7.0 * (DATE_CELL_W + DATE_CELL_GAP) - DATE_CELL_GAP + DATE_PICKER_PAD * 2.0;
    let h = DATE_PICKER_PAD * 2.0
        + DATE_HEADER_H
        + DATE_DAYNAME_H
        + DATE_ROWS as f32 * (DATE_CELL_H + DATE_CELL_GAP);
    (w, h)
}

/// True if `year` is a leap year.
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Number of days in the given month (1-based month, Gregorian calendar).
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap_year(year) { 29 } else { 28 },
        _ => 30,
    }
}

/// ISO weekday (0=Mon … 6=Sun) of the first day of the given month.
/// Uses Zeller's congruence converted to ISO day-of-week.
pub fn first_weekday_of_month(year: i32, month: u8) -> u8 {
    let (y, m) = if month <= 2 {
        (year - 1, month as i32 + 12)
    } else {
        (year, month as i32)
    };
    let k = y % 100;
    let j = y / 100;
    // Zeller: h=0 Sat, h=1 Sun, h=2 Mon … h=6 Fri
    let h = (1 + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 - 2 * j).rem_euclid(7);
    // Convert to ISO: (h + 5) % 7  → Mon=0 … Sun=6
    ((h + 5) % 7) as u8
}

/// English month name, 1-based.
pub fn month_name(month: u8) -> &'static str {
    match month {
        1 => "January", 2 => "February", 3 => "March",
        4 => "April",   5 => "May",       6 => "June",
        7 => "July",    8 => "August",    9 => "September",
        10 => "October", 11 => "November", 12 => "December",
        _ => "?",
    }
}

/// Parse an ISO 8601 date string `YYYY-MM-DD` → `(year, month, day)`.
/// Returns `None` if the string is malformed.
pub fn parse_date_value(value: &str) -> Option<(i32, u8, u8)> {
    let parts: Vec<&str> = value.splitn(3, '-').collect();
    if parts.len() != 3 { return None; }
    let y: i32 = parts[0].parse().ok()?;
    let m: u8 = parts[1].parse().ok()?;
    let d: u8 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) { return None; }
    Some((y, m, d))
}

/// Format `(year, month, day)` as `YYYY-MM-DD`.
pub fn format_date_value(year: i32, month: u8, day: u8) -> String {
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// Return the current year and month derived from the system clock.
/// Falls back to (2026, 1) if the clock is unavailable.
pub fn today_year_month() -> (i32, u8) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    // Howard Hinnant civil_from_days algorithm.
    let days = (secs / 86400) as i32;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_final = if m <= 2 { y + 1 } else { y };
    (y_final, m as u8)
}

/// Build a calendar date-picker overlay anchored below `anchor` (document coords).
///
/// `year`/`month` define which month to display (1-based month).
/// The overlay is viewport-locked: `scroll_y` converts document-space anchor to
/// viewport-space. Navigation arrows (Prev/Next) are rendered at the header ends.
pub fn build_date_picker(
    anchor: Rect,
    scroll_y: f32,
    viewport_w: f32,
    year: i32,
    month: u8,
) -> DisplayList {
    let mut out: DisplayList = Vec::new();
    let (pw, ph) = date_picker_size();

    let vp_y = anchor.y - scroll_y + anchor.height + 4.0;
    let vp_x = anchor.x.min(viewport_w - pw).max(0.0);
    let bg = Rect::new(vp_x, vp_y, pw, ph);

    // Background
    out.push(DisplayCommand::FillRect {
        rect: bg,
        color: Color { r: 255, g: 255, b: 255, a: 255 },
    });
    out.push(DisplayCommand::DrawBorder {
        rect: bg,
        widths: [1.0; 4],
        colors: [Color { r: 160, g: 160, b: 160, a: 255 }; 4],
        styles: [BorderStyle::Solid; 4],
        radii: lumen_paint::CornerRadii::default(),
    });

    let content_x = vp_x + DATE_PICKER_PAD;
    let content_w = pw - DATE_PICKER_PAD * 2.0;
    let header_y = vp_y + DATE_PICKER_PAD;

    // ── Header: "← June 2026 →" ─────────────────────────────────────
    let arrow_w = DATE_HEADER_H;
    // Prev arrow background on hover — rendered as a grey rect
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(content_x, header_y, arrow_w, DATE_HEADER_H),
        color: Color { r: 235, g: 235, b: 235, a: 255 },
    });
    out.push(DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: Rect::new(content_x, header_y + 4.0, arrow_w, DATE_HEADER_H - 4.0),
        text: "<".to_owned(),
        font_size: 14.0,
        color: Color { r: 40, g: 40, b: 40, a: 255 },
        font_family: vec![],
        font_weight: FontWeight::new(700),
        font_style: FontStyle::Normal,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    });
    // Month/year label centered
    let label = format!("{} {}", month_name(month), year);
    out.push(DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: Rect::new(content_x + arrow_w, header_y + 4.0, content_w - arrow_w * 2.0, DATE_HEADER_H - 4.0),
        text: label,
        font_size: 12.0,
        color: Color { r: 20, g: 20, b: 20, a: 255 },
        font_family: vec![],
        font_weight: FontWeight::new(600),
        font_style: FontStyle::Normal,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    });
    // Next arrow
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(content_x + content_w - arrow_w, header_y, arrow_w, DATE_HEADER_H),
        color: Color { r: 235, g: 235, b: 235, a: 255 },
    });
    out.push(DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: Rect::new(content_x + content_w - arrow_w, header_y + 4.0, arrow_w, DATE_HEADER_H - 4.0),
        text: ">".to_owned(),
        font_size: 14.0,
        color: Color { r: 40, g: 40, b: 40, a: 255 },
        font_family: vec![],
        font_weight: FontWeight::new(700),
        font_style: FontStyle::Normal,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    });

    // ── Day-of-week header ───────────────────────────────────────────
    let daynames = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    let daynames_y = header_y + DATE_HEADER_H;
    for (i, name) in daynames.iter().enumerate() {
        let cell_x = content_x + i as f32 * (DATE_CELL_W + DATE_CELL_GAP);
        out.push(DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: Rect::new(cell_x, daynames_y, DATE_CELL_W, DATE_DAYNAME_H),
            text: (*name).to_owned(),
            font_size: 10.0,
            color: Color { r: 100, g: 100, b: 100, a: 255 },
            font_family: vec![],
            font_weight: FontWeight::new(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        });
    }

    // ── Day cells ────────────────────────────────────────────────────
    let days_start_y = daynames_y + DATE_DAYNAME_H;
    let first_col = first_weekday_of_month(year, month) as usize;
    let num_days = days_in_month(year, month) as usize;

    for day in 1..=num_days {
        let cell_idx = first_col + day - 1;
        let row = cell_idx / 7;
        let col = cell_idx % 7;
        let cx = content_x + col as f32 * (DATE_CELL_W + DATE_CELL_GAP);
        let cy = days_start_y + row as f32 * (DATE_CELL_H + DATE_CELL_GAP);
        let cell_rect = Rect::new(cx, cy, DATE_CELL_W, DATE_CELL_H);

        // Highlight weekend days slightly
        if col >= 5 {
            out.push(DisplayCommand::FillRect {
                rect: cell_rect,
                color: Color { r: 248, g: 245, b: 255, a: 255 },
            });
        }
        out.push(DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: cell_rect,
            text: day.to_string(),
            font_size: 12.0,
            color: Color { r: 20, g: 20, b: 20, a: 255 },
            font_family: vec![],
            font_weight: FontWeight::new(400),
            font_style: FontStyle::Normal,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        });
    }

    out
}

/// Hit-test a viewport-space click `(px, py)` against an open date picker.
///
/// Returns `DatePickerHit::Day(d)` when a day cell is clicked,
/// `DatePickerHit::Prev`/`Next` for navigation arrows, or
/// `DatePickerHit::None` when the click is outside the picker.
pub fn hit_date_picker(
    anchor: Rect,
    scroll_y: f32,
    viewport_w: f32,
    year: i32,
    month: u8,
    px: f32,
    py: f32,
) -> DatePickerHit {
    let (pw, ph) = date_picker_size();
    let vp_y = anchor.y - scroll_y + anchor.height + 4.0;
    let vp_x = anchor.x.min(viewport_w - pw).max(0.0);

    if px < vp_x || px > vp_x + pw || py < vp_y || py > vp_y + ph {
        return DatePickerHit::None;
    }

    let rel_x = px - vp_x - DATE_PICKER_PAD;
    let rel_y = py - vp_y - DATE_PICKER_PAD;
    let content_w = pw - DATE_PICKER_PAD * 2.0;
    let arrow_w = DATE_HEADER_H;

    // Header row
    if rel_y < DATE_HEADER_H {
        if rel_x < arrow_w {
            return DatePickerHit::Prev;
        }
        if rel_x > content_w - arrow_w {
            return DatePickerHit::Next;
        }
        return DatePickerHit::None;
    }

    // Day name row
    if rel_y < DATE_HEADER_H + DATE_DAYNAME_H {
        return DatePickerHit::None;
    }

    // Day cells grid
    let grid_y = rel_y - DATE_HEADER_H - DATE_DAYNAME_H;
    let col = (rel_x / (DATE_CELL_W + DATE_CELL_GAP)) as usize;
    let row = (grid_y / (DATE_CELL_H + DATE_CELL_GAP)) as usize;
    if col >= 7 { return DatePickerHit::None; }

    // Ensure click is within the cell (not in the gap)
    let cell_rel_x = rel_x % (DATE_CELL_W + DATE_CELL_GAP);
    let cell_rel_y = grid_y % (DATE_CELL_H + DATE_CELL_GAP);
    if cell_rel_x > DATE_CELL_W || cell_rel_y > DATE_CELL_H {
        return DatePickerHit::None;
    }

    let first_col = first_weekday_of_month(year, month) as usize;
    let cell_idx = row * 7 + col;
    if cell_idx < first_col { return DatePickerHit::None; }
    let day = cell_idx - first_col + 1;
    let num_days = days_in_month(year, month) as usize;
    if day < 1 || day > num_days { return DatePickerHit::None; }
    DatePickerHit::Day(day as u8)
}

/// Advance display month by `delta` months (positive = forward, negative = backward).
/// Returns the new `(year, month)` pair, clamped to valid Gregorian range.
pub fn advance_month(year: i32, month: u8, delta: i32) -> (i32, u8) {
    let total = (year * 12 + month as i32 - 1) + delta;
    let new_year = total.div_euclid(12);
    let new_month = (total.rem_euclid(12) + 1) as u8;
    (new_year, new_month)
}

#[cfg(test)]
mod tests;

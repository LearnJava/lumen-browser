use super::*;

use crate::style::{Color, GradientStop, SvgGradientDef, SvgGradientUnits, SvgPaint, parse_color};
use lumen_core::ColorSpace;

/// SVG `viewBox="min-x min-y width height"` attribute. Maps SVG user-unit space
/// to the CSS pixel rect of the `<svg>` element. All four values are in SVG user units.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewBox {
    /// Left edge of the SVG viewport in user units.
    pub min_x: f32,
    /// Top edge of the SVG viewport in user units.
    pub min_y: f32,
    /// Width of the SVG viewport in user units (> 0).
    pub width: f32,
    /// Height of the SVG viewport in user units (> 0).
    pub height: f32,
}

/// SVG `preserveAspectRatio` attribute for aspect-ratio preservation.
/// Controls how viewBox scales to fit the SVG's CSS width/height.
/// Default is `xMidYMid` with uniform scaling.
#[derive(Debug, Clone, PartialEq)]
pub struct PreserveAspectRatio {
    /// Horizontal alignment: `xMin` (left), `xMid` (center), `xMax` (right).
    pub align_x: SvgAlignX,
    /// Vertical alignment: `YMin` (top), `YMid` (middle), `YMax` (bottom).
    pub align_y: SvgAlignY,
    /// Uniform scaling (`Uniform`) or stretch to fill (`NonUniform`).
    pub meet_or_slice: SvgMeetOrSlice,
}

/// SVG preserveAspectRatio horizontal alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgAlignX {
    /// `xMin` — align viewBox to left edge.
    Min,
    /// `xMid` — align viewBox to center (default).
    Mid,
    /// `xMax` — align viewBox to right edge.
    Max,
}

/// SVG preserveAspectRatio vertical alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgAlignY {
    /// `YMin` — align viewBox to top edge.
    Min,
    /// `YMid` — align viewBox to center (default).
    Mid,
    /// `YMax` — align viewBox to bottom edge.
    Max,
}

/// SVG preserveAspectRatio meet-or-slice mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgMeetOrSlice {
    /// `meet` (default) — uniform scale to fit inside, may have letterboxing.
    Meet,
    /// `slice` — uniform scale to cover, may clip.
    Slice,
}

/// SVG `text-anchor` attribute for text horizontal alignment.
/// Controls how text is anchored at the specified x position (SVG L1 §10.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvgTextAnchor {
    /// `start` (default) — text starts at the x position.
    #[default]
    Start,
    /// `middle` — text center is at the x position.
    Middle,
    /// `end` — text ends at the x position.
    End,
}

/// SVG `dominant-baseline` attribute for text vertical alignment.
/// Controls how text is anchored at the specified y position (SVG L1 §10.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvgDominantBaseline {
    /// `auto` (default) — dominant baseline is determined by the text.
    #[default]
    Auto,
    /// `baseline` — use the alphabetic baseline of the text.
    Baseline,
    /// `hanging` — use the hanging baseline (e.g., for Devanagari scripts).
    Hanging,
    /// `middle` — use the middle of the em-box.
    Middle,
    /// `central` — use the central baseline (midpoint between ascender and descender).
    Central,
    /// `text-before-edge` — use the top of the em-box.
    TextBeforeEdge,
    /// `text-after-edge` — use the bottom of the em-box.
    TextAfterEdge,
}

/// SVG 1.1 §10.9.2 / CSS Inline Layout L3 §5.2 — `baseline-shift`. Vertical shift
/// of the text baseline relative to the dominant baseline of the parent.
/// NOT inherited; initial `baseline` (no shift). Positive lengths/percentages
/// *raise* the text (shift up, toward smaller `y`); `sub` lowers and `super`
/// raises by an approximate sub/superscript offset.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SvgBaselineShift {
    /// `baseline` (initial) — no shift.
    #[default]
    Baseline,
    /// `sub` — lower the baseline to the subscript position.
    Sub,
    /// `super` — raise the baseline to the superscript position.
    Super,
    /// `<length>` in user units. Positive raises the text (shifts up).
    Length(f32),
    /// `<percentage>` as a fraction of the current font-size. Positive raises.
    Percentage(f32),
}

/// SVG transformation data from the `transform` presentation attribute.
/// Stores parsed transform functions in order of application.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SvgTransform {
    /// Transform matrix components: [a, b, c, d, e, f] representing the 2D transformation matrix.
    /// Default is identity matrix [1, 0, 0, 1, 0, 0].
    pub matrix: [f32; 6],
}

impl SvgTransform {
    /// Creates an identity transform (no transformation).
    pub fn identity() -> Self {
        SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] }
    }

    /// Creates a translation transform.
    pub fn translate(tx: f32, ty: f32) -> Self {
        SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, tx, ty] }
    }

    /// Multiplies this transform by another, composing them.
    pub fn compose(&mut self, other: &SvgTransform) {
        let [a, b, c, d, e, f] = self.matrix;
        let [a2, b2, c2, d2, e2, f2] = other.matrix;
        // Matrix multiplication: self × other
        self.matrix = [
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ];
    }

    /// Applies this transform to a point (x, y).
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.matrix;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

/// Geometric primitive for an SVG shape element in SVG user units (before viewBox scaling).
/// Coordinate origin: top-left of the SVG viewport.
#[derive(Debug, Clone, PartialEq)]
pub enum SvgShapeKind {
    /// `<rect x y width height rx ry>`. Corner radii `rx`/`ry` default to 0 (sharp corners).
    Rect { x: f32, y: f32, width: f32, height: f32, rx: f32, ry: f32 },
    /// `<circle cx cy r>`. Center at (cx, cy), radius r.
    Circle { cx: f32, cy: f32, r: f32 },
    /// `<ellipse cx cy rx ry>`. Center at (cx, cy), horizontal radius rx, vertical ry.
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    /// `<line x1 y1 x2 y2>`. Segment from (x1,y1) to (x2,y2).
    Line { x1: f32, y1: f32, x2: f32, y2: f32 },
    /// `<path d="...">`. SVG path data string; bounding box computed by paint.
    /// CSS: fill, stroke, stroke-width — P4 wires via ComputedStyle svg_fill/svg_stroke.
    Path { d: String },
}

/// Вид form control — используется в `BoxKind::FormControl` для paint-специализаций
/// (фокус-рамка, checkbox/radio indicator, placeholder, стрелка select и т.д.).
#[derive(Debug, Clone, PartialEq)]
pub enum FormControlKind {
    /// `<input>` — carries input type (from `type` attribute) and initial
    /// checked state (from presence of `checked` attribute in DOM). Paint uses
    /// this to draw checkbox/radio indicators without re-querying the DOM.
    /// `value_text` is the `value` attribute content, used by `field-sizing: content`.
    /// `placeholder` is the `placeholder` attribute content, painted in grey by
    /// text-like inputs when `value_text` is empty (HTML rendering §15.5.5).
    /// `placeholder_style` is the computed `::placeholder` override (CSS
    /// Pseudo-Elements L4 §4.10), if any author rule targets
    /// `input::placeholder` — `None` falls back to the UA default grey hint.
    Input {
        input_type: lumen_dom::InputType,
        checked: bool,
        value_text: String,
        placeholder: String,
        placeholder_style: Option<Box<ComputedStyle>>,
    },
    Button,
    /// `<select>` — `selected_text` is the label of the currently selected
    /// `<option>` (first option if none is explicitly selected). Paint uses this
    /// to draw the visible label without re-querying the DOM.
    Select { selected_text: String },
    /// `<textarea>` — `value_text` is the text content of all direct text children,
    /// used by `field-sizing: content` to compute intrinsic dimensions.
    Textarea { value_text: String },
    /// `<input type="range">` — carries current value and bounds so paint can
    /// draw track / fill / thumb without re-querying the DOM.
    Range {
        /// Current slider value clamped to [min, max].
        value: f32,
        /// Minimum bound (HTML `min` attribute; default 0).
        min: f32,
        /// Maximum bound (HTML `max` attribute; default 100).
        max: f32,
    },
    /// `<progress>` — determinate or indeterminate progress bar.
    ///
    /// `value` is `None` when the `value` attribute is absent (indeterminate).
    /// Paint draws a filled bar (blue) proportional to `value / max`, or a
    /// static partial fill for indeterminate.
    Progress {
        /// Current value clamped to [0, max]; `None` = indeterminate.
        value: Option<f32>,
        /// Maximum value (HTML `max` attribute; default 1.0).
        max: f32,
    },
    /// `<meter>` — gauge bar whose fill color reflects optimality (HTML5 §4.10.14).
    ///
    /// Color: green = optimal zone, yellow = sub-optimal, red = bad.
    Meter {
        /// Current value clamped to [min, max].
        value: f32,
        /// Minimum bound (HTML `min` attribute; default 0.0).
        min: f32,
        /// Maximum bound (HTML `max` attribute; default 1.0).
        max: f32,
        /// Low threshold: below `low` is the "low" segment (default = min).
        low: f32,
        /// High threshold: above `high` is the "high" segment (default = max).
        high: f32,
        /// Optimal value — determines which segment is colored green (default = midpoint).
        optimum: f32,
    },
}

/// Collect the text label of the currently selected `<option>` inside a
/// `<select>` element. Returns the text of the first `<option selected>` child,
/// falling back to the first `<option>` child, then an empty string.
pub(crate) fn collect_select_label(doc: &Document, select_id: NodeId) -> String {
    let children = doc.get(select_id).children.clone();
    let mut first_label: Option<String> = None;
    for child_id in children {
        let child = doc.get(child_id);
        let NodeData::Element { name, attrs, .. } = &child.data else { continue };
        if name.local.as_str() != "option" { continue }
        let label = option_text(doc, child_id);
        let is_selected = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("selected"));
        if is_selected {
            return label;
        }
        if first_label.is_none() {
            first_label = Some(label);
        }
    }
    first_label.unwrap_or_default()
}

/// Collect the selected `<option>` label from a `<selectlist>` element.
///
/// `<selectlist>` may contain `<option>` elements directly or nested inside a
/// `<listbox>` child (Customizable Select §3.1). Searches both levels.
/// Returns the first `<option selected>` text, falling back to the first
/// `<option>` text, or an empty string if no options are present.
///
/// Phase 0 layout stub — renders like a native `<select>` widget.
/// `// CSS: appearance: base-select` — P4 wires ::picker(select) styling.
pub fn collect_selectlist_label(doc: &Document, sl_id: NodeId) -> String {
    // Gather direct <option> children and <option> children inside <listbox>.
    let mut option_ids: Vec<NodeId> = Vec::new();
    for &child_id in &doc.get(sl_id).children.clone() {
        let child = doc.get(child_id);
        let NodeData::Element { name, .. } = &child.data else { continue };
        if name.local.as_str() == "option" {
            option_ids.push(child_id);
        } else if name.local.as_str() == "listbox" {
            for &gc_id in &child.children.clone() {
                let gc = doc.get(gc_id);
                let NodeData::Element { name: gcn, .. } = &gc.data else { continue };
                if gcn.local.as_str() == "option" {
                    option_ids.push(gc_id);
                }
            }
        }
    }
    let mut first_label: Option<String> = None;
    for opt_id in option_ids {
        let opt = doc.get(opt_id);
        let NodeData::Element { attrs, .. } = &opt.data else { continue };
        let label = option_text(doc, opt_id);
        let is_selected = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("selected"));
        if is_selected {
            return label;
        }
        if first_label.is_none() {
            first_label = Some(label);
        }
    }
    first_label.unwrap_or_default()
}

/// Returns `true` when `node` is a `<selectlist>` element (Customizable Select).
///
/// Used by layout to render `<selectlist>` as a form control widget (Phase 0
/// fallback — same appearance as `<select>`).
pub fn is_selectlist(doc: &Document, node: NodeId) -> bool {
    matches!(
        &doc.get(node).data,
        NodeData::Element { name, .. } if name.local.as_str() == "selectlist"
    )
}

/// Returns the display text for an `<option>` element: `label` attribute if
/// present, otherwise the concatenated text content of its child text nodes.
fn option_text(doc: &Document, option_id: NodeId) -> String {
    let node = doc.get(option_id);
    if let NodeData::Element { attrs, .. } = &node.data
        && let Some(label) = attrs.iter().find(|a| a.name.local.eq_ignore_ascii_case("label"))
    {
        return label.value.trim().to_owned();
    }
    node.children
        .iter()
        .filter_map(|&c| {
            if let NodeData::Text(t) = &doc.get(c).data { Some(t.as_str()) } else { None }
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_owned()
}

/// Является ли DOM-узел HTML form control-ом.
/// Tag-name хранится lower-case (HTML5 tree-builder).
pub(crate) fn is_form_control_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. }
            if matches!(name.local.as_str(), "input" | "button" | "select" | "selectlist" | "textarea" | "meter" | "progress")
    )
}

/// Финальный URL картинки + author-объявленные intrinsic dimensions.
/// Заполняется `resolve_image_source` ниже — это адаптер `PickedSource`
/// из `lumen-html-parser`, плюс legacy-fallback на голый `src`-атрибут
/// для битых страниц, у которых picker отказал.
pub(crate) struct ImageSource {
    pub(crate) url: String,
    pub(crate) intrinsic_width: Option<u32>,
    pub(crate) intrinsic_height: Option<u32>,
}

// ─── SVG helpers ─────────────────────────────────────────────────────────────

/// Returns `true` when `id` is an `<svg>` element.
/// Note: the HTML5 parser does not yet implement foreign-content mode, so all
/// elements (including SVG ones) are created with `Namespace::Html`. We detect
/// SVG elements by local name until the parser gains full foreign-content support.
pub(crate) fn is_svg_root(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("svg")
    )
}

/// Returns `true` when `id` is an SVG `<defs>` element (invisible container).
#[allow(dead_code)]
pub(crate) fn is_svg_defs(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("defs")
    )
}

/// Returns `true` when `id` is an SVG `<use>` element (reference to another element).
#[allow(dead_code)]
fn is_svg_use(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("use")
    )
}

/// Returns `true` when `id` is a `<details>` element.
pub(crate) fn is_details_element(doc: &Document, id: NodeId) -> bool {
    matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == "details")
}

/// Returns `true` when `id` is a `<summary>` element.
pub(crate) fn is_summary_element(doc: &Document, id: NodeId) -> bool {
    matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == "summary")
}

/// Returns `true` when `id` is a `<details>` element with the `open` attribute set.
///
/// HTML LS §4.11.1: when `open` is absent only `<summary>` is rendered; when present all
/// children are visible. External callers (paint, a11y) use this to query disclosure state.
pub fn is_open_details(doc: &Document, id: NodeId) -> bool {
    is_details_element(doc, id) && doc.get(id).get_attr("open").is_some()
}

/// Returns `true` when `id` has a `popover` attribute but is not open.
///
/// Elements with `popover` are hidden by default (UA: `[popover]{display:none}`);
/// JS calls `showPopover()` which sets `data-lumen-popover-open` to expose the element.
pub(crate) fn is_closed_popover(doc: &Document, id: NodeId) -> bool {
    let node = doc.get(id);
    node.get_attr("popover").is_some() && node.get_attr("data-lumen-popover-open").is_none()
}

/// Parses a float attribute from the given element; returns 0.0 if absent or non-numeric.
fn svg_attr_f32(doc: &Document, id: NodeId, attr: &str) -> f32 {
    doc.get(id)
        .get_attr(attr)
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
}

// ─── SVG paint servers (LIB-5) ─────────────────────────────────────────────

/// Parses an SVG gradient coordinate: a bare number (already in `units`'
/// scale — a 0..1 fraction for `objectBoundingBox`, a user unit for
/// `userSpaceOnUse`) or a `<percentage>` (divided by 100, SVG L1 §4.5 treats
/// `N%` and the bare fraction `N/100` as equivalent for these attributes).
/// Returns `default` if the attribute is absent or unparseable.
fn svg_gradient_coord(doc: &Document, id: NodeId, attr: &str, default: f32) -> f32 {
    let Some(raw) = doc.get(id).get_attr(attr) else { return default };
    let raw = raw.trim();
    match raw.strip_suffix('%') {
        Some(pct) => pct.trim().parse::<f32>().map(|v| v / 100.0).unwrap_or(default),
        None => raw.parse::<f32>().unwrap_or(default),
    }
}

/// Recursively collects `<stop>` element ids under `parent_id`, in document
/// order — see [`collect_gradient_stops`] for why a direct-children scan
/// isn't enough. Any non-`<stop>` descendant (an `<animate>`, a mis-nested
/// non-stop sibling) is skipped but still walked into.
fn collect_stop_ids(doc: &Document, parent_id: NodeId, out: &mut Vec<NodeId>) {
    for &child_id in &doc.get(parent_id).children {
        if doc.get(child_id).element_name().is_some_and(|n| n.local.as_str() == "stop") {
            out.push(child_id);
        }
        collect_stop_ids(doc, child_id, out);
    }
}

/// Collects the `<stop>` descendants of a `<linearGradient>`/`<radialGradient>`
/// element (SVG L1 §13.2.4) into display-list-ready [`GradientStop`]s, in
/// document order.
///
/// Walks the full subtree, not just direct children: `<stop/>` is not an
/// HTML void element, so the HTML5 parser (no SVG foreign-content mode, same
/// gotcha `<rect/>`/`<use/>` already work around elsewhere in this file)
/// treats its trailing `/` as decorative and opens a real element — the
/// *next* `<stop/>` becomes its DOM *child*, not its sibling. A direct-children
/// scan would silently keep only the first stop of every self-closed list.
///
/// Reads `offset`/`stop-color`/`stop-opacity` as DOM **presentation
/// attributes only** — a `style="stop-color:…"` inline style or an author
/// stylesheet rule targeting `stop` is not applied, since `<stop>` elements
/// never become a `LayoutBox` (they live under `<defs>`, skipped by
/// `collect_svg_shapes`) and so never go through `compute_style`. Covers the
/// overwhelmingly common authoring form; a `<stop>` relying on CSS for its
/// color paints black instead (SVG L1 §13.2.4's own fallback for an invalid
/// `stop-color`).
///
/// SVG L1 §13.2.4 requires offsets to be non-decreasing — a `<stop>` whose
/// `offset` is less than the previous one is clamped up to it.
fn collect_gradient_stops(doc: &Document, gradient_id: NodeId) -> Vec<GradientStop> {
    let mut stops = Vec::new();
    let mut floor = 0.0_f32;
    let mut ids = Vec::new();
    collect_stop_ids(doc, gradient_id, &mut ids);
    for child_id in ids {
        let offset = svg_gradient_coord(doc, child_id, "offset", 0.0).clamp(0.0, 1.0).max(floor);
        floor = offset;
        let color_attr = doc.get(child_id).get_attr("stop-color").unwrap_or("black");
        // `currentColor` on a <stop> should resolve to the stop element's own
        // (inherited) `color` — out of scope here (no cascade pass over
        // `<defs>` content); falls back to the SVG default paint (black).
        let base = if color_attr.trim().eq_ignore_ascii_case("currentcolor") {
            None
        } else {
            parse_color(color_attr.trim())
        }
        .unwrap_or(Color::BLACK);
        let opacity = doc.get(child_id).get_attr("stop-opacity")
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let a = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;
        stops.push(GradientStop {
            color: Color { a, ..base },
            color_space: ColorSpace::default(),
            position: Some(Length::Percent(offset * 100.0)),
        });
    }
    stops
}

/// Resolves an SVG paint-server reference (`fill`/`stroke` `url(#id)`) into a
/// concrete gradient definition. `None` when `id` doesn't resolve, isn't a
/// `<linearGradient>`/`<radialGradient>`, or (after one `href` hop) has no
/// `<stop>` children — SVG L1 §13.2.1's fallback for all of these is "not
/// painted" (like `fill: none`), which the caller applies by leaving the
/// paint as `SvgPaint::None` when this returns `None`.
fn resolve_svg_gradient(doc: &Document, id: &str) -> Option<SvgGradientDef> {
    let node_id = doc.find_by_id(id)?;
    // The HTML5 tokenizer ASCII-lowercases every tag name it builds
    // (`html-parser/src/tokenizer.rs`), same as any other tag — `<linearGradient>`
    // is stored as `lineargradient` — so this must compare case-insensitively
    // (`get_attr` already does, via `eq_ignore_ascii_case`; tag names don't).
    let tag = doc.get(node_id).element_name()?.local.as_str().to_owned();
    let is_linear = tag.eq_ignore_ascii_case("linearGradient");
    let is_radial = tag.eq_ignore_ascii_case("radialGradient");
    if !is_linear && !is_radial {
        return None;
    }
    let units = match doc.get(node_id).get_attr("gradientUnits") {
        Some(v) if v.trim().eq_ignore_ascii_case("userSpaceOnUse") => SvgGradientUnits::UserSpaceOnUse,
        _ => SvgGradientUnits::ObjectBoundingBox,
    };
    let mut stops = collect_gradient_stops(doc, node_id);
    if stops.is_empty() {
        // SVG L1 §13.2.1 — `xlink:href`/`href` inherits stops (and, for any
        // geometry attribute this element doesn't specify itself) from
        // another gradient. One hop only: a `<stop>`-less chain longer than
        // one link is rare in authored content, and bounding the hop count
        // avoids needing cycle detection for a reference chain.
        let href = doc.get(node_id).get_attr("href")
            .or_else(|| doc.get(node_id).get_attr("xlink:href"))?;
        let ref_node = doc.find_by_id(href.trim_start_matches('#'))?;
        stops = collect_gradient_stops(doc, ref_node);
        if stops.is_empty() {
            return None;
        }
    }
    Some(if is_linear {
        // SVG L1 §13.2.2 defaults: a horizontal gradient spanning the full box.
        SvgGradientDef::Linear {
            x1: svg_gradient_coord(doc, node_id, "x1", 0.0),
            y1: svg_gradient_coord(doc, node_id, "y1", 0.0),
            x2: svg_gradient_coord(doc, node_id, "x2", 1.0),
            y2: svg_gradient_coord(doc, node_id, "y2", 0.0),
            units,
            stops,
        }
    } else {
        // SVG L1 §13.2.3 defaults: centred, radius half the box. SVG 2's focal
        // point (`fx`/`fy`) is not modelled — see `SvgGradientDef::Radial`.
        SvgGradientDef::Radial {
            cx: svg_gradient_coord(doc, node_id, "cx", 0.5),
            cy: svg_gradient_coord(doc, node_id, "cy", 0.5),
            r: svg_gradient_coord(doc, node_id, "r", 0.5),
            units,
            stops,
        }
    })
}

/// Resolves a `SvgPaint::Url` in place against the DOM, replacing it with
/// `Gradient` (found) or `None` (unresolvable — SVG L1 §13.2.1 fallback).
/// No-op for every other `SvgPaint` variant.
fn resolve_svg_paint_url(doc: &Document, paint: &mut SvgPaint) {
    let SvgPaint::Url(id) = paint else { return };
    *paint = resolve_svg_gradient(doc, id)
        .map(|g| SvgPaint::Gradient(Arc::new(g)))
        .unwrap_or(SvgPaint::None);
}

/// Parses the SVG `viewBox="min-x min-y width height"` attribute.
/// Returns `None` if the attribute is absent or malformed.
pub(crate) fn parse_view_box(doc: &Document, id: NodeId) -> Option<ViewBox> {
    let s = doc.get(id).get_attr("viewBox")?;
    let vals: Vec<f32> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<f32>().ok())
        .collect();
    if vals.len() < 4 || vals[2] <= 0.0 || vals[3] <= 0.0 {
        return None;
    }
    Some(ViewBox { min_x: vals[0], min_y: vals[1], width: vals[2], height: vals[3] })
}

/// Parses an SVG `points="x1,y1 x2,y2 ..."` list (commas and/or whitespace as
/// separators, SVG 1.1 §9.7) into a flat coordinate list, then groups it into
/// `(x, y)` pairs. A trailing lone coordinate is dropped.
pub(crate) fn parse_svg_points(s: &str) -> Vec<(f32, f32)> {
    let nums: Vec<f32> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<f32>().ok())
        .collect();
    nums.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// Builds an SVG path `d` string from a `points` list. `<polygon>` closes the
/// contour with `Z`; `<polyline>` leaves it open. Returns `None` when fewer than
/// two points are present (nothing renderable). Reusing the `<path>` pipeline
/// keeps polygon/polyline fill, stroke and joins consistent with `<path>`.
pub(crate) fn points_to_path_d(points: &[(f32, f32)], close: bool) -> Option<String> {
    if points.len() < 2 {
        return None;
    }
    let mut d = String::with_capacity(points.len() * 12);
    for (i, (x, y)) in points.iter().enumerate() {
        if i == 0 {
            d.push_str(&format!("M {x} {y}"));
        } else {
            d.push_str(&format!(" L {x} {y}"));
        }
    }
    if close {
        d.push_str(" Z");
    }
    Some(d)
}

/// Parses the SVG `preserveAspectRatio` attribute.
/// Format: `[defer] <align> [meet|slice]`
/// Default is `xMidYMid meet` (center, uniform scale, fit inside).
pub(crate) fn parse_preserve_aspect_ratio(doc: &Document, id: NodeId) -> PreserveAspectRatio {
    let s = match doc.get(id).get_attr("preserveAspectRatio") {
        Some(s) => s.trim(),
        None => "xMidYMid meet",
    };

    // Skip optional "defer" keyword at start.
    let s = s.strip_prefix("defer ").unwrap_or(s);

    // Parse align and meet-or-slice.
    let parts: Vec<&str> = s.split_whitespace().collect();
    let align_str = parts.first().copied().unwrap_or("xMidYMid");
    let meet_or_slice_str = parts.get(1).copied().unwrap_or("meet");

    // Parse alignment (e.g. "xMidYMid", "xMinYMin", etc.).
    let (align_x, align_y) = if align_str == "none" {
        // "none" means non-uniform scaling — not implemented yet, fall back to uniform.
        (SvgAlignX::Mid, SvgAlignY::Mid)
    } else {
        // Extract x-align from prefix: xMin|xMid|xMax.
        let align_x = if align_str.starts_with("xMin") {
            SvgAlignX::Min
        } else if align_str.starts_with("xMax") {
            SvgAlignX::Max
        } else {
            SvgAlignX::Mid
        };
        // Extract y-align from suffix: YMin|YMid|YMax.
        let align_y = if align_str.contains("YMin") {
            SvgAlignY::Min
        } else if align_str.contains("YMax") {
            SvgAlignY::Max
        } else {
            SvgAlignY::Mid
        };
        (align_x, align_y)
    };

    let meet_or_slice = if meet_or_slice_str == "slice" {
        SvgMeetOrSlice::Slice
    } else {
        SvgMeetOrSlice::Meet
    };

    PreserveAspectRatio { align_x, align_y, meet_or_slice }
}

/// Parses the SVG `transform` presentation attribute and returns a composed transform matrix.
/// Syntax: `<transform-function> [ <transform-function> ]* | none`
/// Supported functions: translate, scale, rotate, skewX, skewY, matrix.
pub(crate) fn parse_svg_transform(attr: Option<&str>) -> SvgTransform {
    let attr = match attr {
        Some(s) => s.trim(),
        None => return SvgTransform::identity(),
    };

    if attr.eq_ignore_ascii_case("none") {
        return SvgTransform::identity();
    }

    let mut result = SvgTransform::identity();

    // Simple regex-free parser: extract function names and their arguments.
    let mut pos = 0;
    let attr_bytes = attr.as_bytes();

    while pos < attr_bytes.len() {
        // Skip whitespace and commas. BUG-803: `&&` binds tighter than `||`,
        // so an unparenthesized condition reads as
        // `(pos < len && ws) || attr_bytes[pos] == b','` — once `pos == len`
        // the first disjunct is false but the second still indexes past the
        // end of the slice. Both checks must be gated by the length check.
        while pos < attr_bytes.len()
            && ((attr_bytes[pos] as char).is_whitespace() || attr_bytes[pos] == b',')
        {
            pos += 1;
        }

        if pos >= attr_bytes.len() {
            break;
        }

        // Extract function name.
        let start = pos;
        while pos < attr_bytes.len() && (attr_bytes[pos] as char).is_alphabetic() {
            pos += 1;
        }

        let func_name = &attr[start..pos];

        // Skip whitespace and opening paren.
        while pos < attr_bytes.len() && (attr_bytes[pos] as char).is_whitespace() {
            pos += 1;
        }

        if pos >= attr_bytes.len() || attr_bytes[pos] != b'(' {
            // BUG-803: a byte that is neither a letter, whitespace, a comma
            // nor `(` (an underscore, a digit, `;`, `|`, ...) leaves both the
            // name loop above and this branch without moving `pos` — `continue`
            // then re-enters this exact position forever. Force one byte of
            // progress whenever the name loop itself made none, so a name
            // that already advanced (e.g. `translate` before the `3` of
            // `translate3d`) still gets a second chance next iteration
            // instead of being force-skipped mid-token.
            if pos == start {
                pos += 1;
            }
            continue;
        }

        pos += 1; // skip '('

        // Extract arguments until closing paren.
        let args_start = pos;
        let mut depth = 1;
        while pos < attr_bytes.len() && depth > 0 {
            if attr_bytes[pos] == b'(' {
                depth += 1;
            } else if attr_bytes[pos] == b')' {
                depth -= 1;
            }
            if depth > 0 {
                pos += 1;
            }
        }

        let args_str = attr[args_start..pos].trim();
        let args: Vec<f32> = args_str
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();

        // Apply the transform function.
        let fn_transform = match func_name.to_lowercase().as_str() {
            "translate" => {
                let tx = args.first().copied().unwrap_or(0.0);
                let ty = args.get(1).copied().unwrap_or(0.0);
                SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, tx, ty] }
            }
            "scale" => {
                let sx = args.first().copied().unwrap_or(1.0);
                let sy = args.get(1).copied().unwrap_or(sx);
                SvgTransform { matrix: [sx, 0.0, 0.0, sy, 0.0, 0.0] }
            }
            "rotate" => {
                let angle = args.first().copied().unwrap_or(0.0); // in degrees
                let rad = angle.to_radians();
                let cos = rad.cos();
                let sin = rad.sin();
                // Optional cx, cy for rotation center.
                let cx = args.get(1).copied().unwrap_or(0.0);
                let cy = args.get(2).copied().unwrap_or(0.0);
                if cx.abs() < 0.001 && cy.abs() < 0.001 {
                    SvgTransform { matrix: [cos, sin, -sin, cos, 0.0, 0.0] }
                } else {
                    // rotate(a cx cy) = translate(cx cy) · rotate(a) · translate(-cx -cy).
                    // `compose` is `self × other` (other is applied first to a point),
                    // so the list must be accumulated left-to-right starting from the
                    // outermost translate. The previous code started from `R` and
                    // post-composed both translates, which cancel out
                    // (R · T(cx,cy) · T(-cx,-cy) = R) — silently dropping the rotation
                    // centre (BUG-244).
                    let mut m = SvgTransform::translate(cx, cy);
                    m.compose(&SvgTransform { matrix: [cos, sin, -sin, cos, 0.0, 0.0] });
                    m.compose(&SvgTransform::translate(-cx, -cy));
                    m
                }
            }
            "skewx" => {
                let angle = args.first().copied().unwrap_or(0.0);
                let tan = angle.to_radians().tan();
                SvgTransform { matrix: [1.0, 0.0, tan, 1.0, 0.0, 0.0] }
            }
            "skewy" => {
                let angle = args.first().copied().unwrap_or(0.0);
                let tan = angle.to_radians().tan();
                SvgTransform { matrix: [1.0, tan, 0.0, 1.0, 0.0, 0.0] }
            }
            "matrix" => {
                if let [a, b, c, d, e, f, ..] = args.as_slice() {
                    SvgTransform { matrix: [*a, *b, *c, *d, *e, *f] }
                } else {
                    SvgTransform::identity()
                }
            }
            _ => SvgTransform::identity(),
        };

        result.compose(&fn_transform);

        if pos < attr_bytes.len() && attr_bytes[pos] == b')' {
            pos += 1;
        }
    }

    result
}

/// Calculates the intrinsic aspect ratio from SVG viewBox.
/// Returns `Some(width / height)` if viewBox is present and both dimensions > 0.
#[allow(dead_code)]
pub(crate) fn svg_intrinsic_ratio(view_box: &Option<ViewBox>) -> Option<f32> {
    view_box.as_ref().and_then(|vb| {
        if vb.width > 0.0 && vb.height > 0.0 {
            Some(vb.width / vb.height)
        } else {
            None
        }
    })
}

/// Collects text content from an SVG text element and its descendants.
/// Recursively walks the DOM tree, concatenating text nodes and content of nested `<tspan>` elements.
fn collect_text_content(doc: &Document, node_id: NodeId) -> String {
    let mut text = String::new();
    let node = doc.get(node_id);

    // Walk through immediate children and concatenate text.
    for child_id in node.children.iter() {
        let child = doc.get(*child_id);
        match &child.data {
            NodeData::Text(s) => {
                // Text node: add content.
                text.push_str(s);
            }
            NodeData::Element { name, .. }
                if name.local.as_str() == "tspan" || name.local.as_str() == "textPath" =>
            {
                // For element nodes like <tspan>, recursively collect their text.
                text.push_str(&collect_text_content(doc, *child_id));
            }
            _ => {}
        }
    }

    text
}

/// Collects the text content of a `<textarea>` from its direct text-node children.
///
/// Used by `field-sizing: content` to determine the intrinsic size of the control.
/// Newlines are preserved (each `\n` becomes a line for height computation).
pub(crate) fn collect_textarea_content(doc: &Document, node_id: NodeId) -> String {
    // BUG-441: what a textarea *shows* is its current value — the child text is
    // only the default, replaced as soon as the user types or a script assigns
    // `el.value` (HTML LS §4.10.11).
    if let Some(dirty) = doc.dirty_value(node_id) {
        return dirty.to_owned();
    }
    let mut text = String::new();
    let node = doc.get(node_id);
    for child_id in node.children.iter() {
        if let NodeData::Text(s) = &doc.get(*child_id).data {
            text.push_str(s);
        }
    }
    text
}


/// Maps an SVG `viewBox` into the SVG viewport using the `preserveAspectRatio`
/// attribute (SVG 1.1 §7.8). Inline `<svg>` ignores CSS `object-fit`/`object-position`
/// (those govern replaced content only); browsers fit the viewBox per this attribute.
/// Returns `(scale_x, scale_y, origin_dx, origin_dy)` where `origin_d*` is the
/// document-space offset of the viewBox origin from the viewport's top-left corner —
/// same shape as [`compute_object_fit_transform`] so the caller is unchanged. BUG-198.
pub(crate) fn compute_preserve_aspect_ratio_transform(
    view_box: &ViewBox,
    box_w: f32,
    box_h: f32,
    par: &PreserveAspectRatio,
) -> (f32, f32, f32, f32) {
    let vb_w = view_box.width.max(0.001);
    let vb_h = view_box.height.max(0.001);
    let raw_sx = box_w / vb_w;
    let raw_sy = box_h / vb_h;

    // `meet` → uniform scale fitting inside (contain); `slice` → uniform scale
    // covering (cover). Lumen has no `preserveAspectRatio="none"` variant, so
    // non-uniform fill never occurs here.
    let (sx, sy) = match par.meet_or_slice {
        SvgMeetOrSlice::Meet  => { let s = raw_sx.min(raw_sy); (s, s) }
        SvgMeetOrSlice::Slice => { let s = raw_sx.max(raw_sy); (s, s) }
    };

    // Align the scaled viewBox within the free space (may be negative for `slice`).
    let free_x = box_w - vb_w * sx;
    let free_y = box_h - vb_h * sy;
    let ox = match par.align_x {
        SvgAlignX::Min => 0.0,
        SvgAlignX::Mid => free_x * 0.5,
        SvgAlignX::Max => free_x,
    };
    let oy = match par.align_y {
        SvgAlignY::Min => 0.0,
        SvgAlignY::Mid => free_y * 0.5,
        SvgAlignY::Max => free_y,
    };

    (sx, sy, ox - view_box.min_x * sx, oy - view_box.min_y * sy)
}

/// Best-effort CSS-px size of an `<svg>` root's own viewport, computed at box-tree-build
/// time (before layout runs, so percentage width/height cannot resolve against a containing
/// block yet — only the `None` percent-basis case). Mirrors the intrinsic-size fallback chain
/// `lay_out_svg_root` uses later for the box's own rect (CSS width/height → viewBox dims → SVG
/// default 300×150). BUG-334: this is the "current viewport" a descendant `<use>`/`<symbol>`
/// without explicit width/height should size itself against (SVG 2 §5.7/§7.10 — the used value
/// is 100% of the current viewport), not the target's own viewBox dimensions.
pub(crate) fn svg_root_own_size(style: &ComputedStyle, view_box: Option<&ViewBox>, viewport: Size) -> Size {
    let em = style.font_size;
    let width = style.width.as_ref()
        .and_then(|l| l.resolve(em, None, viewport))
        .or_else(|| view_box.map(|vb| vb.width))
        .unwrap_or(300.0)
        .max(0.0);
    let height = style.height.as_ref()
        .and_then(|l| l.resolve(em, None, viewport))
        .or_else(|| view_box.map(|vb| vb.height))
        .unwrap_or(150.0)
        .max(0.0);
    Size { width, height }
}

/// Builds `SvgShape` and `Block` (for `<g>`) layout boxes for the SVG subtree rooted at
/// `parent_id`. Because the HTML5 parser does not implement SVG foreign-content mode, self-
/// closing SVG tags like `<rect/>` are treated as open tags and subsequent siblings become
/// DOM children. This function performs a depth-first recursive scan, collecting SVG shape
/// elements wherever they appear in the subtree.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_svg_children(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    dark_mode: bool,
) -> Vec<LayoutBox> {
    let mut out = Vec::new();
    collect_svg_shapes(doc, sheet, parent_id, inherited, viewport, own_svg_size, flat, &mut out, dark_mode);
    out
}

/// Recursively collects SVG shape and group boxes from the DOM subtree of `parent_id`.
/// `use_stack` tracks NodeIds currently being expanded via `<use>` for cycle detection.
/// Handles the HTML5 parser's incorrect nesting of self-closing SVG tags: when a `<rect/>`
/// is parsed as an open element, its DOM children (intended siblings) are also scanned.
#[allow(clippy::too_many_arguments)]
fn collect_svg_shapes(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    out: &mut Vec<LayoutBox>,
    dark_mode: bool,
) {
    collect_svg_shapes_impl(doc, sheet, parent_id, inherited, viewport, own_svg_size, flat, out, dark_mode, &[]);
}

/// Inner recursive worker for `collect_svg_shapes`. Carries `use_stack` for cycle detection.
#[allow(clippy::too_many_arguments)]
fn collect_svg_shapes_impl(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    out: &mut Vec<LayoutBox>,
    dark_mode: bool,
    use_stack: &[NodeId],
) {
    for child_id in flat.children_of(doc, parent_id) {
        process_svg_node(doc, sheet, *child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
    }
}

/// Processes a single SVG element node, appending layout boxes to `out`.
/// Used by both the main `collect_svg_shapes_impl` loop and `<use>` clone expansion.
#[allow(clippy::too_many_arguments)]
fn process_svg_node(
    doc: &Document,
    sheet: &Stylesheet,
    child_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    own_svg_size: Size,
    flat: &FlatTree,
    out: &mut Vec<LayoutBox>,
    dark_mode: bool,
    use_stack: &[NodeId],
) {
    let Some(name) = doc.get(child_id).element_name() else {
        return; // text node / comment / etc.
    };
    let mut style = Arc::new(crate::style::compute_style(doc, child_id, sheet, inherited, viewport, dark_mode));
    if style.display == crate::style::Display::None {
        return;
    }
    // LIB-5 — `fill`/`stroke: url(#id)` is only a fragment id after cascade
    // (which has no `Document`); resolve it against the DOM here, once per
    // element, so descendants that inherit the paint (no `fill`/`stroke` of
    // their own) inherit the already-resolved `Gradient` like any other
    // inherited value. `Arc::make_mut` is cheap here: `style` was just
    // created above and has no other references yet, so this never clones.
    if matches!(style.svg_fill, SvgPaint::Url(_)) || matches!(style.svg_stroke, SvgPaint::Url(_)) {
        let s = Arc::make_mut(&mut style);
        resolve_svg_paint_url(doc, &mut s.svg_fill);
        resolve_svg_paint_url(doc, &mut s.svg_stroke);
    }
    let svg_transform = parse_svg_transform(doc.get(child_id).get_attr("transform"));

    match name.local.as_str() {
        "rect" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Rect {
                        x: svg_attr_f32(doc, child_id, "x"),
                        y: svg_attr_f32(doc, child_id, "y"),
                        width: svg_attr_f32(doc, child_id, "width"),
                        height: svg_attr_f32(doc, child_id, "height"),
                        rx: svg_attr_f32(doc, child_id, "rx"),
                        ry: svg_attr_f32(doc, child_id, "ry"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            // Recurse: incorrectly-nested siblings (HTML5 parser wraps them inside rect).
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "circle" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Circle {
                        cx: svg_attr_f32(doc, child_id, "cx"),
                        cy: svg_attr_f32(doc, child_id, "cy"),
                        r: svg_attr_f32(doc, child_id, "r"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "ellipse" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Ellipse {
                        cx: svg_attr_f32(doc, child_id, "cx"),
                        cy: svg_attr_f32(doc, child_id, "cy"),
                        rx: svg_attr_f32(doc, child_id, "rx"),
                        ry: svg_attr_f32(doc, child_id, "ry"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "line" => {
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape {
                    shape: SvgShapeKind::Line {
                        x1: svg_attr_f32(doc, child_id, "x1"),
                        y1: svg_attr_f32(doc, child_id, "y1"),
                        x2: svg_attr_f32(doc, child_id, "x2"),
                        y2: svg_attr_f32(doc, child_id, "y2"),
                    },
                    svg_transform: svg_transform.clone(),
                    svg_paint_matrix: SvgTransform::identity(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "path" => {
            let d = doc.get(child_id).get_attr("d").unwrap_or("").to_string();
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgShape { shape: SvgShapeKind::Path { d }, svg_transform: svg_transform.clone(), svg_paint_matrix: SvgTransform::identity() },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "text" | "tspan" | "textPath" => {
            // SVG text element: collect text content from this element and descendants.
            let text = collect_text_content(doc, child_id);
            // SVG 2 §11.6 / §11.10.2 — `text-anchor` / `dominant-baseline` come from
            // the cascade (`apply_svg_presentational_hints` folds the presentation
            // attributes in as lowest-priority declarations, so author CSS overrides
            // them and they inherit from container elements). `None` = the `start` /
            // `auto` initial value.
            let text_anchor = style.text_anchor.unwrap_or_default();
            let dominant_baseline = style.dominant_baseline.unwrap_or_default();
            // SVG 1.1 §10.9.2 — `baseline-shift` is non-inherited; the presentation
            // attribute is folded into the cascade by `apply_svg_presentational_hints`.
            let baseline_shift = style.baseline_shift;
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::SvgText {
                    text,
                    x: svg_attr_f32(doc, child_id, "x"),
                    y: svg_attr_f32(doc, child_id, "y"),
                    dx: svg_attr_f32(doc, child_id, "dx"),
                    dy: svg_attr_f32(doc, child_id, "dy"),
                    text_anchor,
                    dominant_baseline,
                    baseline_shift,
                    svg_transform: svg_transform.clone(),
                },
                children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
            // Recurse for potential nested text/tspan/textPath elements.
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "g" => {
            // Group: collect children shapes, then wrap in a Block box.
            let mut group_children: Vec<LayoutBox> = Vec::new();
            collect_svg_shapes_impl(doc, sheet, child_id, &style, viewport, own_svg_size, flat, &mut group_children, dark_mode, use_stack);
            let group_transform = parse_svg_transform(doc.get(child_id).get_attr("transform"));
            out.push(LayoutBox {
                node: child_id, rect: Rect::ZERO, style,
                kind: BoxKind::Block,
                children: group_children, col_span: 1, row_span: 1, svg_group_transform: Some(group_transform), scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
            });
        }
        "use" => {
            // SVG <use>: clone the referenced element at an optional (x, y) offset.
            // SVG 2 §5.6 — shadow tree clone with cycle detection via `use_stack`.
            let href_val = doc.get(child_id).get_attr("href")
                .or_else(|| doc.get(child_id).get_attr("xlink:href"))
                .unwrap_or_default();
            let target_ref = href_val.trim_start_matches('#');
            if target_ref.is_empty() {
                return;
            }
            let Some(target_id) = doc.find_by_id(target_ref) else { return; };

            // Cycle guard: skip if target is already on the use-expansion stack.
            if use_stack.contains(&target_id) {
                return;
            }

            // Build the combined transform: <use transform="..."> then translate(x, y).
            let use_x = svg_attr_f32(doc, child_id, "x");
            let use_y = svg_attr_f32(doc, child_id, "y");
            let mut combined = svg_transform.clone();
            if use_x != 0.0 || use_y != 0.0 {
                combined.compose(&SvgTransform::translate(use_x, use_y));
            }

            // Build new stack with target pushed for nested <use> detection.
            let mut new_stack: Vec<NodeId> = use_stack.to_vec();
            new_stack.push(target_id);

            // Collect the referenced subtree into a clone group.
            let mut use_children: Vec<LayoutBox> = Vec::new();
            let target_tag = doc.get(target_id).element_name()
                .map(|n| n.local.as_str().to_owned())
                .unwrap_or_default();

            // BUG-246: a `<use>` referencing a `<symbol>` (or `<svg>`) with a
            // `viewBox` establishes a new viewport (SVG 2 §5.7). The instance is
            // sized by the `<use>`'s `width`/`height` (overriding the symbol's),
            // and the symbol's `viewBox` is mapped into that viewport via
            // `preserveAspectRatio`. Without this, every instance renders at the
            // viewBox's intrinsic size regardless of width/height. Compose the
            // viewBox→viewport scale onto `combined` *after* the use's x/y
            // translate, so it operates in the symbol's local coordinate system.
            if matches!(target_tag.as_str(), "symbol" | "svg")
                && let Some(vb) = parse_view_box(doc, target_id)
            {
                // Viewport size: `<use>` width/height win; else the symbol's own
                // width/height; else BUG-334: fall back to the enclosing `<svg>`'s own
                // CSS-resolved viewport (SVG 2 §5.7/§7.10 "100% of current viewport"),
                // not the target's viewBox dims (that was the BUG-246-era identity bug).
                let attr_dim = |id: NodeId, attr: &str| -> Option<f32> {
                    doc.get(id).get_attr(attr)
                        .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok())
                        .filter(|d| *d > 0.0)
                };
                let vp_w = attr_dim(child_id, "width")
                    .or_else(|| attr_dim(target_id, "width"))
                    .unwrap_or(own_svg_size.width);
                let vp_h = attr_dim(child_id, "height")
                    .or_else(|| attr_dim(target_id, "height"))
                    .unwrap_or(own_svg_size.height);
                let par = parse_preserve_aspect_ratio(doc, target_id);
                let (sx, sy, tx, ty) =
                    compute_preserve_aspect_ratio_transform(&vb, vp_w, vp_h, &par);
                combined.compose(&SvgTransform { matrix: [sx, 0.0, 0.0, sy, tx, ty] });
            }

            if matches!(target_tag.as_str(), "g" | "symbol") {
                // Container: recursively collect its children as the clone content.
                collect_svg_shapes_impl(doc, sheet, target_id, &style, viewport, own_svg_size, flat, &mut use_children, dark_mode, &new_stack);
            } else {
                // Single shape or other element: process the node directly.
                process_svg_node(doc, sheet, target_id, &style, viewport, own_svg_size, flat, &mut use_children, dark_mode, &new_stack);
            }

            if !use_children.is_empty() {
                out.push(LayoutBox {
                    node: child_id, rect: Rect::ZERO, style,
                    kind: BoxKind::Block,
                    children: use_children, col_span: 1, row_span: 1,
                    svg_group_transform: Some(combined),
                    scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                    origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
                });
            }

            // The HTML5 parser does not honour `<use/>` self-closing (it is not a
            // void element), so sibling SVG elements written after a `<use>` are
            // mis-nested as its DOM children. Scan them into `out` as siblings —
            // mirror the rect/circle workaround. A `<use>`'s rendered content comes
            // from its target, never from its DOM children, so this is unambiguous.
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "polygon" | "polyline" => {
            // SVG 1.1 §9.6/§9.7: render via the `<path>` pipeline. A polygon
            // auto-closes its contour (`Z`); a polyline stays open.
            let close = name.local.eq_ignore_ascii_case("polygon");
            let points = parse_svg_points(doc.get(child_id).get_attr("points").unwrap_or(""));
            if let Some(d) = points_to_path_d(&points, close) {
                out.push(LayoutBox {
                    node: child_id, rect: Rect::ZERO, style,
                    kind: BoxKind::SvgShape { shape: SvgShapeKind::Path { d }, svg_transform: svg_transform.clone(), svg_paint_matrix: SvgTransform::identity() },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
                origin: BoxOrigin { node: Some(child_id), role: BoxRole::Element },
                });
            }
            // Mis-nested siblings (HTML5 parser wraps them inside the self-closed shape).
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
        "defs" | "symbol" => {
            // `<defs>` (SVG 2 §5.5) and `<symbol>` (§5.7) are never rendered
            // directly when encountered as a direct child — their content is
            // painted only when instantiated through `<use>`. (The `<use>` arm
            // collects a symbol's children explicitly, so referencing still works.)
        }
        _ => {
            // Unknown SVG element: skip self, but scan children for shapes.
            collect_svg_shapes_impl(doc, sheet, child_id, inherited, viewport, own_svg_size, flat, out, dark_mode, use_stack);
        }
    }
}

// ─── SVG layout ──────────────────────────────────────────────────────────────

/// Lays out an `SvgRoot` box: computes its CSS rect, then positions SVG shape children
/// in document coordinates by applying the viewBox-to-CSS-pixel transform.
pub(crate) fn lay_out_svg_root(b: &mut LayoutBox, start_x: f32, start_y: f32, avail_w: f32, avail_h: Option<f32>, viewport: Size) {
    let s = &b.style;
    let em = s.font_size;
    let cb = avail_w;
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_top  = s.margin_top.resolve_or_zero(em, cb, viewport);
    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;

    let (view_box, preserve_aspect_ratio) = if let BoxKind::SvgRoot { view_box, preserve_aspect_ratio, .. } = &b.kind {
        (view_box.clone(), preserve_aspect_ratio.clone())
    } else {
        // SVG default per §7.8: xMidYMid meet (centered, uniform fit-inside).
        (None, PreserveAspectRatio {
            align_x: SvgAlignX::Mid,
            align_y: SvgAlignY::Mid,
            meet_or_slice: SvgMeetOrSlice::Meet,
        })
    };

    // SVG intrinsic size: CSS width/height wins, then viewBox dimensions, then SVG defaults.
    let svg_w = s.width.as_ref()
        .and_then(|l| l.resolve(em, Some(cb), viewport))
        .or_else(|| view_box.as_ref().map(|vb| vb.width))
        .unwrap_or(300.0)
        .max(0.0);
    let svg_h = s.height.as_ref()
        .and_then(|l| l.resolve(em, avail_h, viewport))
        .or_else(|| view_box.as_ref().map(|vb| vb.height))
        .unwrap_or(150.0)
        .max(0.0);
    b.rect.width  = svg_w;
    b.rect.height = svg_h;

    // viewBox → CSS-px transform via the SVG `preserveAspectRatio` attribute
    // (SVG 1.1 §7.8). An inline `<svg>` is NOT a CSS replaced element, so CSS
    // `object-fit`/`object-position` do NOT apply to it — Chrome/Edge fit the
    // viewBox purely by `preserveAspectRatio` (verified pixel-for-pixel against
    // the Edge TEST-70 reference: every box renders as `meet`/contain, the named
    // `object-fit` classes have no effect). The earlier BUG-110 wiring routed the
    // viewBox through object-fit, stretching/cropping the viewBox in ways Edge
    // never does (BUG-198). object-fit still applies to `<img>`-embedded SVG via
    // the DrawImage path.
    let (scale_x, scale_y, origin_x, origin_y) = match &view_box {
        Some(vb) if vb.width > 0.0 && vb.height > 0.0 => {
            let (sx, sy, ox_delta, oy_delta) =
                compute_preserve_aspect_ratio_transform(vb, svg_w, svg_h, &preserve_aspect_ratio);
            (sx, sy, b.rect.x + ox_delta, b.rect.y + oy_delta)
        }
        _ => (1.0, 1.0, b.rect.x, b.rect.y),
    };
    let root_transform = SvgTransform::identity();
    lay_out_svg_children_positions(&mut b.children, origin_x, origin_y, scale_x, scale_y, &root_transform);
}

/// Recursively positions SVG shape boxes (and `<g>` group children) using the
/// viewBox-to-document-coordinate transform `(origin_x, origin_y, scale_x, scale_y)`.
/// Composes element transforms hierarchically via `parent_transform`.
fn lay_out_svg_children_positions(children: &mut [LayoutBox], ox: f32, oy: f32, sx: f32, sy: f32, parent_transform: &SvgTransform) {
    for child in children.iter_mut() {
        lay_out_svg_element_position(child, ox, oy, sx, sy, parent_transform);
    }
}

#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
fn lay_out_svg_element_position(b: &mut LayoutBox, ox: f32, oy: f32, sx: f32, sy: f32, parent_transform: &SvgTransform) {
    // Phase 2: full nested transform composition.
    // Get element's own transform (stored during box creation).
    let element_transform = match &b.kind {
        BoxKind::SvgShape { svg_transform, .. } => svg_transform.clone(),
        BoxKind::Block if b.svg_group_transform.is_some() => b.svg_group_transform.as_ref().unwrap().clone(),
        _ => SvgTransform::identity(),
    };

    // Compose parent transform with element transform.
    let mut composed = parent_transform.clone();
    composed.compose(&element_transform);

    if let BoxKind::SvgShape { ref shape, .. } = b.kind {
        // Compute the shape bbox in user coordinates, apply element/group
        // transforms in user space, THEN map user→document via the viewport
        // (ox, oy, sx, sy). Order matters: a `scale`/`rotate` element transform
        // must operate in the SVG local coordinate system, NOT scale the
        // document-space viewport origin. Baking ox/oy in first (the old order)
        // made `scale(0.75)` on a `<use>` in a low SVG drift the clone upward by
        // 0.75·origin_y (BUG-201 row 3: scaled tiles jumped from y≈347 to y≈260).
        let mut bbox = svg_shape_bbox(shape, 0.0, 0.0, 1.0, 1.0); // User coords
        bbox = apply_transform_to_bbox(&bbox, &composed);
        bbox.x = ox + bbox.x * sx;
        bbox.y = oy + bbox.y * sy;
        bbox.width *= sx;
        bbox.height *= sy;
        b.rect = bbox;
        // BUG-174: `<path>` has a ZERO bbox here (its document-space bounds are
        // computed at paint time from the `d` data). `apply_transform_to_bbox`
        // collapses a zero-size bbox to `Rect::ZERO`, discarding the SVG viewport
        // origin (ox, oy). The painter shifts the raw `d` coordinates by
        // `b.rect.x/y`, so without an origin every in-flow SVG path renders at the
        // page-space raw coords instead of inside its own SVG box. Mirror the
        // SvgText branch: anchor the path box at the document-space mapping of the
        // viewport origin. (Absolute-positioned SVGs already get this via the
        // post-layout `shift_tree`; in-flow inline-block SVGs did not.)
        if matches!(shape, SvgShapeKind::Path { .. }) {
            let (px, py) = composed.transform_point(ox, oy);
            b.rect = Rect::new(px, py, 0.0, 0.0);
        }
    } else if let BoxKind::SvgText { x, y, dx, dy, .. } = b.kind {
        // SVG text element: position at specified coordinates with offsets.
        // x, y are in user units; dx, dy are additional offsets.
        // Apply viewBox scaling to user unit coordinates.
        let text_x = ox + (x + dx) * sx;
        let text_y = oy + (y + dy) * sy;
        // Apply only the translation of the composed transform to the text origin point.
        // Cannot use apply_transform_to_bbox: it returns ZERO for zero-size bboxes.
        // Phase 2: measure text width and compute proper bbox based on text-anchor and dominant-baseline.
        let (tx, ty) = composed.transform_point(text_x, text_y);
        b.rect = Rect::new(tx, ty, 0.0, 0.0);
    } else if matches!(b.kind, BoxKind::Block) {
        // <g> group: position its children with composed transform, then compute union bbox.
        lay_out_svg_children_positions(&mut b.children, ox, oy, sx, sy, &composed);
        b.rect = svg_children_union_bbox(&b.children);
    }

    // BUG-244: store the full document-space transform (viewport V ∘ composed) on
    // the shape so paint can apply rotation/skew as a canvas CTM. `b.rect` above
    // remains the axis-aligned bounds (used for clip/hit-test); the matrix carries
    // the off-diagonal (rotate/skew) components an AABB cannot represent. The
    // viewport maps user→document as `doc = (ox + sx·x, oy + sy·y)`, applied AFTER
    // `composed` — mirroring the `bbox.x = ox + bbox.x * sx` mapping above.
    // Stored in the dedicated `svg_paint_matrix` output field — NOT back into
    // `svg_transform` (BUG-262): an inline-block `<svg>` that wraps gets laid out
    // twice, and the first pass's matrix (carrying the viewport translation) would
    // be misread as the element transform on the second pass, drifting the shape
    // out of its clip. Pure translate/scale (b=c=0) leaves paint on its existing
    // axis-aligned `b.rect` fast path.
    if let BoxKind::SvgShape { svg_paint_matrix, .. } = &mut b.kind {
        let mut m_doc = SvgTransform { matrix: [sx, 0.0, 0.0, sy, ox, oy] };
        m_doc.compose(&composed);
        *svg_paint_matrix = m_doc;
    }
}

/// Applies an SVG transform matrix to a bounding box by transforming all 4 corners
/// and computing the new bounding box. Phase 2: nested transform composition.
fn apply_transform_to_bbox(bbox: &Rect, transform: &SvgTransform) -> Rect {
    if bbox.width == 0.0 && bbox.height == 0.0 {
        return Rect::ZERO;
    }
    let corners = [
        (bbox.x, bbox.y),
        (bbox.x + bbox.width, bbox.y),
        (bbox.x, bbox.y + bbox.height),
        (bbox.x + bbox.width, bbox.y + bbox.height),
    ];
    let transformed: Vec<(f32, f32)> = corners.iter()
        .map(|(x, y)| transform.transform_point(*x, *y))
        .collect();
    let min_x = transformed.iter().map(|(x, _)| *x).fold(f32::INFINITY, f32::min);
    let min_y = transformed.iter().map(|(_, y)| *y).fold(f32::INFINITY, f32::min);
    let max_x = transformed.iter().map(|(x, _)| *x).fold(f32::NEG_INFINITY, f32::max);
    let max_y = transformed.iter().map(|(_, y)| *y).fold(f32::NEG_INFINITY, f32::max);
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Bounding box of an SVG shape in document coordinates.
/// `ox`/`oy` — document-space origin of the SVG viewport (after viewBox min_x/min_y offset).
/// `sx`/`sy` — CSS-px / SVG-user-unit scale factors.
fn svg_shape_bbox(shape: &SvgShapeKind, ox: f32, oy: f32, sx: f32, sy: f32) -> Rect {
    match *shape {
        SvgShapeKind::Rect { x, y, width, height, .. } =>
            Rect::new(ox + x * sx, oy + y * sy, width * sx, height * sy),
        SvgShapeKind::Circle { cx, cy, r } =>
            Rect::new(ox + (cx - r) * sx, oy + (cy - r) * sy, 2.0 * r * sx, 2.0 * r * sy),
        SvgShapeKind::Ellipse { cx, cy, rx, ry } =>
            Rect::new(ox + (cx - rx) * sx, oy + (cy - ry) * sy, 2.0 * rx * sx, 2.0 * ry * sy),
        SvgShapeKind::Line { x1, y1, x2, y2 } => {
            // Bounding rect of the line segment; minimum 1 CSS px on each axis so the
            // painter can clip-test against it.
            let lx = x1.min(x2);
            let ly = y1.min(y2);
            let rw = (x2 - x1).abs().max(1.0 / sx);
            let rh = (y2 - y1).abs().max(1.0 / sy);
            Rect::new(ox + lx * sx, oy + ly * sy, rw * sx, rh * sy)
        }
        SvgShapeKind::Path { .. } =>
            // Path bounding box requires full path-data parsing — deferred to paint.
            // CSS: fill, stroke — P4 wires; P2 renders via GPU path commands.
            Rect::ZERO,
    }
}

/// Union bounding box of a slice of already-positioned layout boxes.
/// Returns `Rect::ZERO` when all children have zero-area rects.
fn svg_children_union_bbox(children: &[LayoutBox]) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for c in children {
        if c.rect.width > 0.0 || c.rect.height > 0.0 {
            min_x = min_x.min(c.rect.x);
            min_y = min_y.min(c.rect.y);
            max_x = max_x.max(c.rect.x + c.rect.width);
            max_y = max_y.max(c.rect.y + c.rect.height);
        }
    }
    if min_x == f32::INFINITY { Rect::ZERO } else { Rect::new(min_x, min_y, max_x - min_x, max_y - min_y) }
}

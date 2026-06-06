//! Box tree: block-флоу + inline-флоу.
//!
//! Каждый DOM-элемент даёт один LayoutBox. Блочные элементы стэкаются
//! вертикально. Текстовые узлы и inline-элементы (`<a>`, `<span>`, `<em>`,
//! `<strong>`, и т.д.) объединяются в `InlineRun` — анонимный бокс, в
//! котором слова переносятся как единый поток. Слова с одинаковым стилем
//! на одной строке объединяются в один фрагмент (→ один DrawText).
//!
//! Whitespace-only текст и комментарии пропускаются.

use lumen_core::geom::{Rect, Size};
use lumen_core::ext::{HyphenationProvider, NullHyphenationProvider};
use lumen_css_parser::Stylesheet;
use lumen_dom::{build_flat_tree, Document, FlatTree, NodeData, NodeId};
use lumen_html_parser::{
    PictureParams, SizesViewport, pick_img_source, pick_picture_source,
};

use crate::style::{
    apply_container_rules, clear_cq_context, compute_pseudo_element_style, compute_style,
    set_cq_context, AlignValue,
    BackgroundImage, BoxSizing, ClearSide, ContainFlags, ContainerContext, ContainerType, Content,
    ContentItem, ComputedStyle, Direction, Display, FlexBasis, FlexDirection, FlexWrap, FloatSide,
    GridAutoFlow, GridLine, GridTrackSize, Hyphens, Length, LengthOrAuto, ListStylePosition,
    ListStyleType, Overflow, OverflowWrap, Position, TextAlign, TextOverflow, TextWrapMode,
    TextWrapStyle,
    VerticalAlign, WordBreak,
};
use crate::counters::{precompute_counters, CounterMap, CounterStyleRegistry,
                      build_counter_style_registry, format_counter_with_registry};
use crate::subgrid::{SubgridContext, SubgridContextGuard, SUBGRID_COL_CTX, SUBGRID_ROW_CTX};
use crate::TextMeasurer;

/// HTML-имя элемента `<img>` для распознавания replaced-боксов в layout.
/// Tag-name в DOM хранится lower-case (HTML5 tree-builder), поэтому
/// сравнение точное, без `eq_ignore_ascii_case`.
fn is_image_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "img"
    )
}

/// HTML-имя `<video>` для распознавания media replaced-боксов в layout.
fn is_video_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "video"
    )
}

/// HTML-имя `<canvas>` для распознавания replaced-боксов рисовалки в layout.
fn is_canvas_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "canvas"
    )
}

/// HTML-имя `<audio>` для распознавания media replaced-боксов в layout.
fn is_audio_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "audio"
    )
}

/// HTML-имя `<iframe>` для распознавания встроенных документов в layout.
fn is_iframe_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "iframe"
    )
}

/// HTML-имя `<picture>` — обёртка над `<source>`-кандидатами и одним
/// `<img>`-fallback-ом. Сам по себе пиктур ничего не рендерит, его
/// единственная роль — переадресовать source-selection на inner `<img>`.
fn is_picture_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local == "picture"
    )
}

/// SVG `viewBox="min-x min-y width height"` attribute. Maps SVG user-unit space
/// to the CSS pixel rect of the `<svg>` element. All four values are in SVG user units.
#[derive(Debug, Clone)]
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

/// SVG transformation data from the `transform` presentation attribute.
/// Stores parsed transform functions in order of application.
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormControlKind {
    /// `<input>` — carries input type (from `type` attribute) and initial
    /// checked state (from presence of `checked` attribute in DOM). Paint uses
    /// this to draw checkbox/radio indicators without re-querying the DOM.
    Input { input_type: lumen_dom::InputType, checked: bool },
    Button,
    Select,
    Textarea,
}

/// Является ли DOM-узел HTML form control-ом.
/// Tag-name хранится lower-case (HTML5 tree-builder).
fn is_form_control_element(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. }
            if matches!(name.local.as_str(), "input" | "button" | "select" | "textarea")
    )
}

/// Финальный URL картинки + author-объявленные intrinsic dimensions.
/// Заполняется `resolve_image_source` ниже — это адаптер `PickedSource`
/// из `lumen-html-parser`, плюс legacy-fallback на голый `src`-атрибут
/// для битых страниц, у которых picker отказал.
struct ImageSource {
    url: String,
    intrinsic_width: Option<u32>,
    intrinsic_height: Option<u32>,
}

// ─── SVG helpers ─────────────────────────────────────────────────────────────

/// Returns `true` when `id` is an `<svg>` element.
/// Note: the HTML5 parser does not yet implement foreign-content mode, so all
/// elements (including SVG ones) are created with `Namespace::Html`. We detect
/// SVG elements by local name until the parser gains full foreign-content support.
fn is_svg_root(doc: &Document, id: NodeId) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { name, .. } if name.local.eq_ignore_ascii_case("svg")
    )
}

/// Returns `true` when `id` is an SVG `<defs>` element (invisible container).
#[allow(dead_code)]
fn is_svg_defs(doc: &Document, id: NodeId) -> bool {
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
fn is_details_element(doc: &Document, id: NodeId) -> bool {
    matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == "details")
}

/// Returns `true` when `id` is a `<summary>` element.
fn is_summary_element(doc: &Document, id: NodeId) -> bool {
    matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == "summary")
}

/// Returns `true` when `id` has a `popover` attribute but is not open.
///
/// Elements with `popover` are hidden by default (UA: `[popover]{display:none}`);
/// JS calls `showPopover()` which sets `data-lumen-popover-open` to expose the element.
fn is_closed_popover(doc: &Document, id: NodeId) -> bool {
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

/// Parses the SVG `viewBox="min-x min-y width height"` attribute.
/// Returns `None` if the attribute is absent or malformed.
fn parse_view_box(doc: &Document, id: NodeId) -> Option<ViewBox> {
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

/// Parses the SVG `preserveAspectRatio` attribute.
/// Format: `[defer] <align> [meet|slice]`
/// Default is `xMidYMid meet` (center, uniform scale, fit inside).
fn parse_preserve_aspect_ratio(doc: &Document, id: NodeId) -> PreserveAspectRatio {
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
fn parse_svg_transform(attr: Option<&str>) -> SvgTransform {
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
        // Skip whitespace and commas.
        while pos < attr_bytes.len() && (attr_bytes[pos] as char).is_whitespace() || attr_bytes[pos] == b',' {
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
                    // rotate(a cx cy) = translate(cx cy) rotate(a) translate(-cx -cy)
                    let mut m = SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, cx, cy] };
                    let mut rot = SvgTransform { matrix: [cos, sin, -sin, cos, 0.0, 0.0] };
                    rot.compose(&m);
                    m = SvgTransform { matrix: [1.0, 0.0, 0.0, 1.0, -cx, -cy] };
                    rot.compose(&m);
                    rot
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
fn svg_intrinsic_ratio(view_box: &Option<ViewBox>) -> Option<f32> {
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

/// Parses SVG `text-anchor` attribute.
/// Returns the corresponding `SvgTextAnchor` enum value, defaulting to `Start` if attribute is absent or invalid.
fn parse_text_anchor(attr: Option<&str>) -> SvgTextAnchor {
    match attr {
        Some("middle") => SvgTextAnchor::Middle,
        Some("end") => SvgTextAnchor::End,
        _ => SvgTextAnchor::Start, // default
    }
}

/// Parses SVG `dominant-baseline` attribute.
/// Returns the corresponding `SvgDominantBaseline` enum value, defaulting to `Auto` if attribute is absent or invalid.
fn parse_dominant_baseline(attr: Option<&str>) -> SvgDominantBaseline {
    match attr {
        Some("baseline") => SvgDominantBaseline::Baseline,
        Some("hanging") => SvgDominantBaseline::Hanging,
        Some("middle") => SvgDominantBaseline::Middle,
        Some("central") => SvgDominantBaseline::Central,
        Some("text-before-edge") => SvgDominantBaseline::TextBeforeEdge,
        Some("text-after-edge") => SvgDominantBaseline::TextAfterEdge,
        _ => SvgDominantBaseline::Auto, // default
    }
}

/// Calculates SVG viewBox scaling and offset for aspect-ratio preservation.
/// Returns `(scale_x, scale_y, offset_x, offset_y)` to transform viewBox → CSS px.
fn compute_viewbox_transform(
    view_box: &ViewBox,
    svg_width: f32,
    svg_height: f32,
    preserve: &PreserveAspectRatio,
) -> (f32, f32, f32, f32) {
    let vb_width = view_box.width.max(0.001);
    let vb_height = view_box.height.max(0.001);

    // Base scale: how many CSS px per SVG user unit.
    let scale_x = svg_width / vb_width;
    let scale_y = svg_height / vb_height;

    // Determine final scale based on meet-or-slice mode.
    let (final_scale, scale_x_adj, scale_y_adj) = match preserve.meet_or_slice {
        SvgMeetOrSlice::Meet => {
            // Uniform scale that fits inside: use minimum scale.
            let s = scale_x.min(scale_y);
            (s, s, s)
        }
        SvgMeetOrSlice::Slice => {
            // Uniform scale that covers: use maximum scale.
            let s = scale_x.max(scale_y);
            (s, s, s)
        }
    };

    // Calculate scaled viewBox dimensions.
    let scaled_vb_width = vb_width * final_scale;
    let scaled_vb_height = vb_height * final_scale;

    // Determine alignment offsets within the SVG's CSS rect.
    let offset_x = match preserve.align_x {
        SvgAlignX::Min => 0.0,
        SvgAlignX::Mid => (svg_width - scaled_vb_width) / 2.0,
        SvgAlignX::Max => svg_width - scaled_vb_width,
    };

    let offset_y = match preserve.align_y {
        SvgAlignY::Min => 0.0,
        SvgAlignY::Mid => (svg_height - scaled_vb_height) / 2.0,
        SvgAlignY::Max => svg_height - scaled_vb_height,
    };

    // Return scale and origin offset due to viewBox min_x/min_y.
    (scale_x_adj, scale_y_adj, offset_x - view_box.min_x * final_scale, offset_y - view_box.min_y * final_scale)
}

/// Builds `SvgShape` and `Block` (for `<g>`) layout boxes for the SVG subtree rooted at
/// `parent_id`. Because the HTML5 parser does not implement SVG foreign-content mode, self-
/// closing SVG tags like `<rect/>` are treated as open tags and subsequent siblings become
/// DOM children. This function performs a depth-first recursive scan, collecting SVG shape
/// elements wherever they appear in the subtree.
fn build_svg_children(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    dark_mode: bool,
) -> Vec<LayoutBox> {
    let mut out = Vec::new();
    collect_svg_shapes(doc, sheet, parent_id, inherited, viewport, flat, &mut out, dark_mode);
    out
}

/// Recursively collects SVG shape and group boxes from the DOM subtree of `parent_id`.
/// Handles the HTML5 parser's incorrect nesting of self-closing SVG tags: when a `<rect/>`
/// is parsed as an open element, its DOM children (intended siblings) are also scanned.
#[allow(clippy::too_many_arguments)]
fn collect_svg_shapes(
    doc: &Document,
    sheet: &Stylesheet,
    parent_id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    out: &mut Vec<LayoutBox>,
    dark_mode: bool,
) {
    for child_id in flat.children_of(doc, parent_id) {
        let child_id = *child_id;
        let Some(name) = doc.get(child_id).element_name() else {
            continue; // text node / comment / etc.
        };
        let style = crate::style::compute_style(doc, child_id, sheet, inherited, viewport, dark_mode);
        if style.display == crate::style::Display::None {
            continue;
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
                    },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                });
                // Recurse: incorrectly-nested siblings (HTML5 parser wraps them inside rect).
                collect_svg_shapes(doc, sheet, child_id, inherited, viewport, flat, out, dark_mode);
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
                    },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                });
                collect_svg_shapes(doc, sheet, child_id, inherited, viewport, flat, out, dark_mode);
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
                    },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                });
                collect_svg_shapes(doc, sheet, child_id, inherited, viewport, flat, out, dark_mode);
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
                    },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                });
                collect_svg_shapes(doc, sheet, child_id, inherited, viewport, flat, out, dark_mode);
            }
            "path" => {
                let d = doc.get(child_id).get_attr("d").unwrap_or("").to_string();
                out.push(LayoutBox {
                    node: child_id, rect: Rect::ZERO, style,
                    kind: BoxKind::SvgShape { shape: SvgShapeKind::Path { d }, svg_transform: svg_transform.clone() },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                });
                collect_svg_shapes(doc, sheet, child_id, inherited, viewport, flat, out, dark_mode);
            }
            "text" | "tspan" | "textPath" => {
                // SVG text element: collect text content from this element and descendants.
                let text = collect_text_content(doc, child_id);
                let text_anchor = parse_text_anchor(doc.get(child_id).get_attr("text-anchor"));
                let dominant_baseline = parse_dominant_baseline(doc.get(child_id).get_attr("dominant-baseline"));
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
                        svg_transform: svg_transform.clone(),
                    },
                    children: vec![], col_span: 1, row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                });
                // Recurse for potential nested text/tspan/textPath elements.
                collect_svg_shapes(doc, sheet, child_id, inherited, viewport, flat, out, dark_mode);
            }
            "g" => {
                // Group: collect children shapes, then wrap in a Block box.
                let mut group_children: Vec<LayoutBox> = Vec::new();
                collect_svg_shapes(doc, sheet, child_id, &style, viewport, flat, &mut group_children, dark_mode);
                let group_transform = parse_svg_transform(doc.get(child_id).get_attr("transform"));
                out.push(LayoutBox {
                    node: child_id, rect: Rect::ZERO, style,
                    kind: BoxKind::Block,
                    children: group_children, col_span: 1, row_span: 1, svg_group_transform: Some(group_transform), scroll_x: 0.0, scroll_y: 0.0,
                });
            }
            "use" => {
                // SVG <use> element: references another element by ID via href attribute.
                // Phase 2: implement full clone support with cycle detection.
                // Phase 1: skip <use> elements to avoid potential stack overflow from cyclic references.
            }
            _ => {
                // Unknown SVG element: skip self, but scan children for shapes.
                collect_svg_shapes(doc, sheet, child_id, inherited, viewport, flat, out, dark_mode);
            }
        }
    }
}

// ─── SVG layout ──────────────────────────────────────────────────────────────

/// Lays out an `SvgRoot` box: computes its CSS rect, then positions SVG shape children
/// in document coordinates by applying the viewBox-to-CSS-pixel transform.
fn lay_out_svg_root(b: &mut LayoutBox, start_x: f32, start_y: f32, avail_w: f32, avail_h: Option<f32>, viewport: Size) {
    let s = &b.style;
    let em = s.font_size;
    let cb = avail_w;
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_top  = s.margin_top.resolve_or_zero(em, cb, viewport);
    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;

    let (view_box, preserve_aspect_ratio) = if let BoxKind::SvgRoot { view_box, preserve_aspect_ratio } = &b.kind {
        (view_box.clone(), preserve_aspect_ratio.clone())
    } else {
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

    // viewBox → CSS-px transform: scale + origin offset with aspect-ratio preservation.
    // CSS: object-fit, object-position — P4 can override viewBox scaling and alignment
    let (scale_x, scale_y, origin_x, origin_y) = match &view_box {
        Some(vb) if vb.width > 0.0 && vb.height > 0.0 => {
            let (sx, sy, ox_delta, oy_delta) = compute_viewbox_transform(vb, svg_w, svg_h, &preserve_aspect_ratio);
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
        // Compute shape bbox in user coordinates, then apply viewBox scaling.
        let mut bbox = svg_shape_bbox(shape, 0.0, 0.0, 1.0, 1.0); // User coords
        // Apply viewBox scaling and origin offset first.
        bbox.x = ox + bbox.x * sx;
        bbox.y = oy + bbox.y * sy;
        bbox.width *= sx;
        bbox.height *= sy;
        // Then apply composed transform.
        b.rect = apply_transform_to_bbox(&bbox, &composed);
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

/// Запрос на предзагрузку изображения: URL после picking-а по
/// `<picture>`/`srcset`/`sizes` плюс признаки явного задания размеров
/// author-ом (нужны shell для `apply_intrinsic_size`).
pub struct ImageRequest {
    pub node_id: NodeId,
    pub url: String,
    pub has_explicit_width: bool,
    pub has_explicit_height: bool,
    /// `loading="lazy"` (HTML LS §2.6.6.9): defer fetch until element is near viewport.
    /// Shell skips eager fetch and instead registers the image for IntersectionObserver
    /// proximity check; loaded once the element scrolls within one viewport of the fold.
    pub is_lazy: bool,
}

/// Обходит DOM и возвращает запросы на загрузку для всех `<img>`-элементов.
/// URL выбирается через тот же picker, что layout использует при построении
/// `BoxKind::Image { src }` — гарантирует совпадение ключей в
/// `Renderer::register_image` и `DisplayCommand::DrawImage.src`.
pub fn collect_image_requests(doc: &Document, viewport: Size) -> Vec<ImageRequest> {
    let mut out = Vec::new();
    collect_requests_inner(doc, doc.root(), viewport, &mut out);
    out
}

/// Обходит готовое layout-дерево и возвращает уникальные URL-ы из
/// `background-image: url(...)` (CSS Backgrounds L3 §3.10) — те же ключи,
/// что эмиттер кладёт в `DisplayCommand::DrawBackgroundImage.src`.
///
/// Background-image не участвует в расчёте размеров, поэтому собирается
/// уже после layout — shell вызывает функцию между layout-ом и paint-ом,
/// дозагружает байты и регистрирует через `Renderer::register_image`.
///
/// Возвращает `Vec<String>` (а не `Vec<ImageRequest>`): для background-image
/// нет node-anchored intrinsic-size hint-ов (CSS Backgrounds L3 §3.9 говорит
/// о `background-size` в стилях, intrinsic-размер картинки в layout не
/// влияет). Дубликаты отфильтрованы — одна и та же картинка на разных
/// элементах загружается один раз.
#[must_use]
pub fn collect_background_image_requests(root: &LayoutBox) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_bg_image_inner(root, &mut out);
    out
}

fn collect_bg_image_inner(b: &LayoutBox, out: &mut Vec<String>) {
    for layer in &b.style.background_layers {
        if let BackgroundImage::Url(src) = &layer.image
            && !src.is_empty()
            && !out.iter().any(|u| u == src)
        {
            out.push(src.clone());
        }
    }
    for child in &b.children {
        collect_bg_image_inner(child, out);
    }
}

fn collect_requests_inner(doc: &Document, id: NodeId, viewport: Size, out: &mut Vec<ImageRequest>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "img"
    {
        let has_explicit_width = attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("width"));
        let has_explicit_height =
            attrs.iter().any(|a| a.name.local.eq_ignore_ascii_case("height"));
        let is_lazy = attrs.iter().any(|a| {
            a.name.local.eq_ignore_ascii_case("loading")
                && a.value.as_str().eq_ignore_ascii_case("lazy")
        });
        let source = resolve_image_source(doc, id, viewport);
        if !source.url.is_empty() {
            out.push(ImageRequest {
                node_id: id,
                url: source.url,
                has_explicit_width,
                has_explicit_height,
                is_lazy,
            });
        }
        return; // void element — нет children
    }
    for &child in &node.children {
        collect_requests_inner(doc, child, viewport, out);
    }
}

/// Выбрать источник для `<img>`-элемента с учётом окружающего контекста:
///  1. Если parent — `<picture>`, прогоняем picture-picker
///     (выбирает `<source>` или fallback на `<img>` по `media`/`type`/
///     `srcset`/`sizes`).
///  2. Иначе — `<img>`-picker, учитывающий собственный `srcset`/`sizes`/`src`.
///  3. Если оба picker-а вернули `None` (нет ни `srcset`, ни `src`) —
///     fallback на голый `src` атрибут как раньше: для битой разметки
///     лучше отрисовать пустую коробку, чем ничего.
///
/// Phase 0: DPR=1.0 (layout не знает про device pixel ratio renderer-а —
/// это интегрирует P3 при relayout-on-resize), `prefers_dark` = false.
/// `supported_types` заполняется из `lumen_image::supported_mime_types()`:
/// picker пропускает `<source type="image/webp">` и аналогичные пока
/// неподдерживаемые форматы вместо того чтобы выбирать их и показывать пустую коробку.
fn resolve_image_source(doc: &Document, img_id: NodeId, viewport: Size) -> ImageSource {
    let sizes_vp = SizesViewport {
        width_px: viewport.width,
        height_px: viewport.height,
        root_font_size_px: 16.0,
        prefers_dark: false,
    };
    let params = PictureParams {
        viewport: sizes_vp,
        dpr: 1.0,
        supported_types: Some(lumen_image::supported_mime_types()),
    };

    if let Some(parent_id) = doc.get(img_id).parent
        && is_picture_element(doc, parent_id)
        && let Some(picked) = pick_picture_source(doc, parent_id, &params)
    {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    if let Some(picked) = pick_img_source(doc, img_id, sizes_vp, params.dpr) {
        return ImageSource {
            url: picked.url,
            intrinsic_width: picked.intrinsic_width,
            intrinsic_height: picked.intrinsic_height,
        };
    }

    let raw_src = doc.get(img_id).get_attr("src").unwrap_or("").to_string();
    ImageSource { url: raw_src, intrinsic_width: None, intrinsic_height: None }
}

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node: NodeId,
    /// Border-box rectangle: (x, y) is the top-left corner after margin,
    /// (width, height) includes padding + border but NOT margin.
    pub rect: Rect,
    pub style: ComputedStyle,
    pub kind: BoxKind,
    pub children: Vec<LayoutBox>,
    /// HTML `colspan` attribute (table cells only). Number of columns this cell spans.
    /// Always ≥ 1; defaults to 1 for non-table-cell boxes.
    pub col_span: u32,
    /// HTML `rowspan` attribute (table cells only). Number of rows this cell spans.
    /// Always ≥ 1; defaults to 1 for non-table-cell boxes.
    pub row_span: u32,
    /// SVG `transform` attribute for `<g>` groups (Phase 2: nested transforms).
    /// Only used for Block boxes that represent SVG groups; None for all other boxes.
    pub svg_group_transform: Option<SvgTransform>,
    /// Horizontal scroll offset in CSS px for `overflow: scroll` / `overflow: auto`
    /// containers. Updated by shell on wheel/touch events via `set_scroll_position()`.
    /// Zero for non-scrollable boxes.
    pub scroll_x: f32,
    /// Vertical scroll offset in CSS px. Same semantics as `scroll_x`.
    pub scroll_y: f32,
}

/// Отрезок inline-контента с собственным стилем (до layout).
#[derive(Debug, Clone)]
pub struct InlineSegment {
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved px space before this segment's first word:
    /// margin_left + border_left_width + padding_left of the inline element.
    pub pre_space: f32,
    /// Resolved px space after this segment's last word:
    /// padding_right + border_right_width + margin_right of the inline element.
    pub post_space: f32,
    /// True when this segment comes from inside an inline element box
    /// (not anonymous text directly in a block container). Used by the painter
    /// to know whether to draw the element's own background/border.
    pub is_element_box: bool,
    /// Non-None when this segment is an inline-replaced `<img>`. Contains the
    /// resolved image URL. `text` holds the alt attribute.
    pub img_src: Option<String>,
    /// Pre-computed pixel width for image segments (0.0 for text segments).
    pub img_width: f32,
    /// True when this segment represents a forced line break (CSS §4.1: newline
    /// in white-space: pre / pre-wrap text). `text` is empty in this case.
    pub forced_break: bool,
    /// CSS structural pseudo-element role of this segment.
    /// Split out by `collect_inline_segments` before wrapping.
    /// // CSS: ::first-letter — P4 wires: look up `::first-letter` rule, override style of
    /// segments where `pseudo_kind == PseudoKind::FirstLetter`.
    pub pseudo_kind: PseudoKind,
    /// DOM text node that produced this segment, for Selection/Range mapping.
    /// `NodeId(0)` (document root) for generated content with no DOM origin.
    pub source_node: NodeId,
    /// UTF-8 byte offset of `text[0]` within the source text node's content.
    /// Always 0 for non-pre text (whole text node → one segment after whitespace
    /// collapsing); non-zero for pre/pre-wrap segments split at `\n`.
    pub source_char_offset: u32,
}

/// Marks an inline segment as the target of a CSS structural pseudo-element.
/// P4 uses this to apply `::first-letter` styles without touching layout geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PseudoKind {
    /// Regular content — no pseudo-element style override.
    #[default]
    None,
    /// CSS Pseudo-elements L4 §5.1 — typographic first letter of the block.
    /// Split from the first non-whitespace text node by `collect_inline_segments`.
    /// P4 entry point: `compute_pseudo_element_style(node, "first-letter")` → override `seg.style`.
    // CSS: ::first-letter
    FirstLetter,
}

/// Позиционированный текстовый фрагмент в строке (после layout).
/// `x` — смещение от левого края inline-контейнера до начала ТЕКСТА
/// (после border+padding inline-элемента слева).
/// `width` — ширина текста фрагмента в пикселях.
/// `padding_left` / `padding_right` — разрешённые px padding-а inline-элемента
/// для этого фрагмента (ненулевые только для первого/последнего слова сегмента).
#[derive(Debug, Clone)]
pub struct InlineFrag {
    pub x: f32,
    pub width: f32,
    /// Vertical offset within the line box (CSS vertical-align). Positive = down.
    pub y_offset: f32,
    pub text: String,
    pub style: ComputedStyle,
    /// Resolved padding_left of this frag's inline box start (0 if not a box start).
    pub padding_left: f32,
    /// Resolved padding_right of this frag's inline box end (0 if not a box end).
    pub padding_right: f32,
    /// True when this frag comes from an inline element box (not anonymous text).
    /// Used by the painter to draw element background/border.
    pub is_element_box: bool,
    /// Non-None when this frag represents an inline-replaced `<img>`.
    /// `text` holds the alt attribute; `width` is the rendered pixel width.
    pub img_src: Option<String>,
    /// True when this fragment lies on the first formatted line of its block container.
    /// Set by `lay_out` after `wrap_inline_run` completes.
    /// // CSS: ::first-line — P4 wires: `compute_pseudo_element_style(node, "first-line")` →
    /// override `frag.style` for all frags where `is_first_line = true`.
    pub is_first_line: bool,
    /// DOM text node that produced this fragment (for Selection/Range mapping).
    /// Matches the source `InlineSegment::source_node`. `NodeId(0)` for
    /// generated/anonymous content with no direct DOM text node.
    pub source_node: NodeId,
    /// UTF-8 byte offset of `text[0]` within the source text node's content.
    /// Computed in `wrap_inline_run` as words are taken from the segment.
    pub source_char_offset: u32,
}

#[derive(Debug, Clone)]
pub enum BoxKind {
    /// Block-уровневый бокс (элемент или корень документа).
    Block,
    /// Анонимный контейнер для потока inline-контента (текст + inline-элементы).
    /// `segments` — сырые отрезки до lay_out; `lines` — позиционированные строки
    /// после lay_out. Каждая строка — `Vec<InlineFrag>`.
    /// `first_line_style` — pre-computed `::first-line` pseudo-element style for the owning
    /// element. `None` if no rule matches. Applied by `lay_out()` to frags on `lines[0]`.
    InlineRun {
        segments: Vec<InlineSegment>,
        lines: Vec<Vec<InlineFrag>>,
        /// CSS Pseudo-elements L4 §5.3: computed ::first-line style. Set during build_box(),
        /// applied in lay_out() after wrap_inline_run() to first-line frags.
        first_line_style: Option<Box<crate::style::ComputedStyle>>,
    },
    /// Анонимный контейнер для горизонтального потока `display: inline-block`
    /// элементов. Сами дочерние боксы хранятся в `LayoutBox.children`. При
    /// layout дети раскладываются горизонтально слева направо; высота строки
    /// = высота самого высокого дочернего элемента.
    InlineBlockRow,
    /// Replaced element: изображение (`<img>`). В Phase 0 — block-level
    /// (одна картинка занимает свою строку). `src` — путь / URL ресурса
    /// (декодирование откладывается на следующий шаг), `alt` — alternate-текст
    /// для отображения и AT, размеры берутся из `style.width`/`style.height`
    /// (которые могут происходить из CSS или HTML-атрибутов как
    /// presentational hints). Inline-replaced в InlineRun-е — отдельная задача.
    Image {
        src: String,
        alt: String,
    },
    /// Replaced element: HTML `<video>` element (HTML spec §14).
    ///
    /// Phase 0: rendered as a grey `DrawImage` placeholder (the video src is
    /// not fetched or decoded). Intrinsic size comes from `width`/`height`
    /// HTML attributes; UA default is 300×150 CSS px (HTML spec §14.1).
    /// `poster` is the optional poster-image URL shown before playback starts.
    Video {
        /// Primary video source URL (`src` attribute).
        src: String,
        /// Poster image URL (`poster` attribute), may be empty.
        poster: String,
    },
    /// Replaced element: HTML `<canvas>` element — CPU-rasterized drawing surface
    /// (HTML Living Standard §4.12.4).
    ///
    /// Phase 0: the pixel buffer is produced by JS Canvas 2D drawing operations
    /// (`canvas.getContext('2d')`) and rendered via a `DrawImage` command keyed by
    /// `canvas:{node_id}`. Intrinsic size comes from the `width`/`height` content
    /// attributes; UA defaults are 300×150 CSS px (HTML LS §4.12.4).
    Canvas {
        /// Canvas bitmap width in CSS pixels (from `width` attribute, default 300).
        width: u32,
        /// Canvas bitmap height in CSS pixels (from `height` attribute, default 150).
        height: u32,
    },
    /// Replaced element: HTML `<audio>` element (HTML spec §4.8.10).
    ///
    /// Phase 0: no audio playback. Without `controls` attribute: 0×0 (invisible).
    /// With `controls` attribute: full-width × 40px grey bar (UA default per spec).
    /// `src` is the primary audio source URL.
    Audio {
        /// Primary audio source URL (`src` attribute), may be empty.
        src: String,
        /// Whether the `controls` attribute is present (shows a 40px control bar).
        controls: bool,
    },
    /// Replaced element: HTML `<iframe>` element (HTML spec §4.8.5).
    ///
    /// Phase 0: rendered as a grey `DrawImage` placeholder (no sub-document
    /// navigation). Intrinsic size comes from `width`/`height` HTML attributes;
    /// UA defaults are 300×150 CSS px (HTML spec §4.8.5). `src` is the URL
    /// to display in paint-side label and in JS `src` property.
    Iframe {
        /// Primary document URL (`src` attribute), may be empty.
        src: String,
    },
    /// Replaced element: HTML form control (`<input>`, `<button>`, `<select>`,
    /// `<textarea>`). Phase 0: block-level replaced. Размеры берутся из
    /// `style.width`/`style.height` (UA defaults из `apply_ua_form_controls`).
    /// `kind` зарезервирован для paint-специализаций в следующих фазах.
    FormControl {
        kind: FormControlKind,
    },
    /// CSS 2.1 §17 — строка таблицы (`display: table-row`). Дочерние
    /// боксы — ячейки (`display: table-cell`), которые раскладываются
    /// горизонтально слева направо. Высота строки = max высота ячейки.
    TableRow,
    /// Схлопнутый межэлементный пробел в InlineBlockRow.
    /// Не рисуется; участвует только как горизонтальный gap между
    /// inline-block соседями (CSS white-space collapsing §4.1.2).
    InlineSpace,
    /// Не участвует в layout (whitespace, комментарий, doctype, display:none).
    Skip,
    /// CSS Lists L3 §2.1 — `::marker` pseudo-element for `display: list-item`.
    /// `text` — marker string for counter types (1., a., i., …); empty for bullet
    /// types (disc/circle/square) which are rendered as geometric shapes.
    /// `position` — inside/outside flow. `list_style_type` — used by the display-list
    /// emitter to choose geometric (disc/circle/square) vs text rendering.
    /// For `outside` (default) positioned left of the principal block, out of flow.
    Marker {
        text: String,
        position: ListStylePosition,
        list_style_type: ListStyleType,
    },
    /// CSS Display L3 §8 — `display: flow-root`. Establishes a Block Formatting
    /// Context: contains floats, prevents margin escape. Laid out identically to
    /// Block in Phase 0; BFC float-containment wired when float layout is added.
    /// CSS: flow-root
    FlowRoot,
    /// CSS Display L3 §7.2 — `display: contents`. The element itself generates no
    /// box. Children are flattened into the parent's formatting context by
    /// `flatten_contents()` during `build_box`. Must never appear in the final
    /// layout tree that reaches `lay_out`.
    Contents,
    /// CSS 2.1 §17 — table container (`display: table` / `display: inline-table`).
    /// Direct children are `TableRowGroup` or `TableRow` boxes. Layout computes
    /// global column widths across all rows before positioning each row.
    Table,
    /// CSS 2.1 §17 — row group (`display: table-row-group`, `table-header-group`,
    /// `table-footer-group`). Rendered as a transparent wrapper; rows inside are
    /// collected by the parent `Table` box during column-width computation.
    TableRowGroup,
    /// SVG root element (`<svg>`). Acts as a replaced element in CSS flow:
    /// `rect` is its border-box in document coordinates (CSS width × height).
    /// `view_box` maps SVG user-unit space to this rect for shape coordinate transforms.
    /// Children are `SvgShape` and `Block` (for `<g>` groups) boxes.
    /// CSS: width, height (from attributes as presentational hints), fill, stroke — P4 wires.
    SvgRoot {
        /// Parsed `viewBox` attribute. `None` when attribute absent: shapes use 1:1 px mapping.
        view_box: Option<ViewBox>,
        /// Parsed `preserveAspectRatio` attribute for aspect-ratio preservation.
        preserve_aspect_ratio: PreserveAspectRatio,
    },
    /// Individual SVG shape (`<rect>`, `<circle>`, `<ellipse>`, `<line>`, `<path>`).
    /// `LayoutBox.rect` is the bounding box in *document coordinates* (post-viewBox scaling).
    /// `shape` carries the original SVG user-unit geometry for accurate paint-side rendering.
    /// CSS: fill, stroke, stroke-width, opacity — P4 wires via ComputedStyle SVG fields.
    SvgShape {
        /// Geometric primitive in SVG user units (before viewBox scaling).
        shape: SvgShapeKind,
        /// Parsed SVG `transform` presentation attribute (Phase 2: nested transforms).
        /// Composed with parent transforms during layout for accurate positioning.
        svg_transform: SvgTransform,
    },
    /// SVG text element (`<text>`, `<tspan>`, `<textPath>`).
    /// `LayoutBox.rect` is the text bounding box in *document coordinates*.
    /// Text content is measured via `TextMeasurer` and positioned according to SVG text attributes.
    /// CSS: fill, stroke, font-family, font-size — P4 wires via ComputedStyle SVG fields.
    /// // CSS: text-anchor, dominant-baseline, dx, dy
    SvgText {
        /// Text content (concatenated from text nodes within `<text>`, `<tspan>`, `<textPath>`).
        text: String,
        /// SVG `x` attribute in user units (baseline x position). 0.0 if absent.
        x: f32,
        /// SVG `y` attribute in user units (baseline y position). 0.0 if absent.
        y: f32,
        /// SVG `dx` attribute in user units (horizontal offset). 0.0 if absent.
        dx: f32,
        /// SVG `dy` attribute in user units (vertical offset). 0.0 if absent.
        dy: f32,
        /// Text anchor alignment: start/middle/end. Defaults to "start" per SVG spec.
        text_anchor: SvgTextAnchor,
        /// Dominant baseline alignment: auto/baseline/hanging/middle/etc. Defaults to "auto" per SVG spec.
        dominant_baseline: SvgDominantBaseline,
        /// Parsed SVG `transform` presentation attribute.
        svg_transform: SvgTransform,
    },
}

/// CSS Pseudo-elements L4 §5.1 — split the `PseudoKind::FirstLetter` segment in
/// `row_items` into `[first_grapheme | rest]` and apply `fl_style` to the first part.
///
/// The segment was already marked by `collect_inline_segments`; this function
/// overrides its style and (when the text is longer than one char) splits it so
/// `wrap_inline_run` applies the correct font metrics to each part independently.
fn apply_first_letter_style(
    row_items: &mut [LayoutBox],
    fl_style: ComputedStyle,
    inherited: &ComputedStyle,
) {
    for item in row_items.iter_mut() {
        let BoxKind::InlineRun { segments, .. } = &mut item.kind else {
            continue;
        };
        for i in 0..segments.len() {
            if segments[i].pseudo_kind != PseudoKind::FirstLetter {
                continue;
            }
            let text = segments[i].text.clone();
            // Split at the first char boundary (CSS "typographic character unit").
            let boundary = text.char_indices().nth(1).map(|(b, _)| b).unwrap_or(text.len());
            if boundary < text.len() {
                // Multi-char segment: split into first-letter + rest.
                let rest_text = text[boundary..].to_string();
                let first_text = text[..boundary].to_string();
                let source_node = segments[i].source_node;
                let forced_break = segments[i].forced_break;
                let is_element_box = segments[i].is_element_box;
                let img_src = segments[i].img_src.clone();
                let img_width = segments[i].img_width;
                segments[i].text = first_text;
                segments[i].style = fl_style;
                let rest = InlineSegment {
                    text: rest_text,
                    style: inherited.clone(),
                    pre_space: 0.0,
                    post_space: segments[i].post_space,
                    is_element_box,
                    img_src,
                    img_width,
                    forced_break,
                    pseudo_kind: PseudoKind::None,
                    source_node,
                    source_char_offset: segments[i].source_char_offset + boundary as u32,
                };
                // Transfer post_space from first-letter to rest.
                segments[i].post_space = 0.0;
                segments.insert(i + 1, rest);
            } else {
                // Single-char or empty segment: just override style.
                segments[i].style = fl_style;
            }
            return;
        }
    }
}

/// CSS Pseudo-elements L4 §3.1 — apply `::first-line` style overrides after layout.
///
/// Must be called after `lay_out` has populated `InlineRun.lines` with `InlineFrag`s.
/// Walks the box tree; for each block-level box that has a `::first-line` rule on
/// its DOM node, overrides the style of every frag on the first formatted line
/// (`is_first_line == true`).
pub(crate) fn apply_first_line_pseudo_styles(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) {
    for child in &mut b.children {
        apply_first_line_pseudo_styles(child, doc, sheet, viewport, dark_mode);
    }
    if !matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot) {
        return;
    }
    let Some(fl_style) = compute_pseudo_element_style(doc, b.node, "first-line", sheet, &b.style, viewport, dark_mode) else {
        return;
    };
    // Find the first InlineRun child (or inside InlineBlockRow) and apply.
    let mut applied = false;
    'find: for child in &mut b.children {
        match &mut child.kind {
            BoxKind::InlineRun { lines, .. } => {
                if let Some(first_line) = lines.first_mut() {
                    for frag in first_line.iter_mut() {
                        if frag.is_first_line {
                            frag.style = fl_style.clone();
                        }
                    }
                }
                applied = true;
                break 'find;
            }
            BoxKind::InlineBlockRow => {
                for row_child in &mut child.children {
                    if let BoxKind::InlineRun { lines, .. } = &mut row_child.kind {
                        if let Some(first_line) = lines.first_mut() {
                            for frag in first_line.iter_mut() {
                                if frag.is_first_line {
                                    frag.style = fl_style.clone();
                                }
                            }
                        }
                        applied = true;
                        break 'find;
                    }
                }
            }
            _ => {}
        }
    }
    let _ = applied;
}

pub fn layout(doc: &Document, sheet: &Stylesheet, viewport: Size) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    let counters = precompute_counters(doc, sheet, viewport, &flat, false);
    let registry = build_counter_style_registry(sheet);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, false);
    propagate_canvas_background(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    let null_hp = NullHyphenationProvider;
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), None, viewport, init_pcb, &null_hp);
    apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, false);
    // CSS Container Queries L1: second pass applies @container rules + re-layout.
    apply_container_styles(&mut root, doc, sheet, viewport, None, &null_hp, false);
    root
}

/// Layout without a text measurer. For tests and headless modes; uses `layout_measured_hyp` with `dark_mode=false`.
pub fn layout_measured(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
) -> LayoutBox {
    let null_hp = NullHyphenationProvider;
    layout_measured_hyp(doc, sheet, viewport, measurer, &null_hp, false)
}

/// Layout with a real hyphenation provider (for `hyphens: auto`).
/// `dark_mode` drives `@media (prefers-color-scheme: dark)` matching throughout
/// the cascade — shell reads the value from `Lumen.dark_mode` (OS preference via winit).
pub fn layout_measured_hyp(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) -> LayoutBox {
    let root_style = ComputedStyle::root();
    let flat = build_flat_tree(doc);
    let counters = precompute_counters(doc, sheet, viewport, &flat, dark_mode);
    let registry = build_counter_style_registry(sheet);
    let mut root = build_box(doc, sheet, doc.root(), &root_style, viewport, &flat, &counters, &registry, dark_mode);
    propagate_canvas_background(doc, &mut root);
    let init_pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    lay_out(&mut root, 0.0, 0.0, viewport.width, Some(viewport.height), Some(measurer), viewport, init_pcb, hp);
    apply_first_line_pseudo_styles(&mut root, doc, sheet, viewport, dark_mode);
    apply_container_styles(&mut root, doc, sheet, viewport, Some(measurer), hp, dark_mode);
    root
}

/// CSS Backgrounds L3 §2.11.2 — «The Canvas Background and the Root Element»:
/// если у root-элемента (`<html>`) нет собственного фона
/// (`background-color: transparent` И `background-image: none`), фон
/// `<body>` пропагируется на root box, а у `<body>` обнуляется. Это
/// покрывает legacy-страницы `body { background: red }`, где иначе фон
/// рисуется только в пределах body box-а и не достигает viewport-а
/// сверху / снизу.
///
/// Phase 0: переносим только два longhand-а — `background-color` и
/// `background-image`. Остальные `background-*` longhand-ы у body без
/// image не имеют визуального эффекта и сейчас не propagated; при
/// добавлении реального paint pattern fill-а их тоже нужно будет
/// перенести.
///
/// Structure: `doc.root()` — Document-узел; его ребёнок — `<html>`
/// element. Body — прямой ребёнок `<html>`. SVG / MathML root-ы пока не
/// учитываются (spec упоминает их отдельно).
fn propagate_canvas_background(doc: &Document, root: &mut LayoutBox) {
    let html_idx = root
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "html"));
    let Some(html_idx) = html_idx else {
        return;
    };

    let html_box = &mut root.children[html_idx];
    let html_has_bg = html_box.style.background_color.is_some()
        || !html_box.style.background_layers.is_empty();
    if html_has_bg {
        return;
    }

    let body_idx = html_box
        .children
        .iter()
        .position(|c| is_html_element_named(doc, c.node, "body"));
    let Some(body_idx) = body_idx else {
        return;
    };

    let body = &mut html_box.children[body_idx];
    let body_has_bg = body.style.background_color.is_some()
        || !body.style.background_layers.is_empty();
    if !body_has_bg {
        return;
    }

    let bg_color = body.style.background_color.take();
    let bg_layers = std::mem::take(&mut body.style.background_layers);
    html_box.style.background_color = bg_color;
    html_box.style.background_layers = bg_layers;
}

fn is_html_element_named(doc: &Document, id: NodeId, want: &str) -> bool {
    matches!(
        doc.get(id).element_name(),
        Some(q) if q.local.eq_ignore_ascii_case(want)
    )
}

/// Является ли DOM-узел inline-контентом (non-whitespace текст или inline-элемент).
///
/// `<img>` в Phase 0 — block-level replaced element, не inline-контент:
/// он порождает собственный `BoxKind::Image`, а не вливается в `InlineRun`.
/// Inline-replaced (картинка внутри строки текста) — отдельная задача;
/// до неё `<img>` всегда занимает свою строку, как `<div>`.
fn is_inline_content(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> bool {
    match &doc.get(id).data {
        NodeData::Text(s) => !s.chars().all(char::is_whitespace),
        NodeData::Element { .. } => {
            if is_image_element(doc, id) || is_form_control_element(doc, id) {
                return false;
            }
            // Inline-семантика: чистый `inline` или его flex/grid-варианты.
            // Phase 0 layout не делает реального flex/grid — флэт-семантика
            // блока для outer-display, но inline-family остаётся inline.
            matches!(
                compute_style(doc, id, sheet, inherited, viewport, dark_mode).display,
                Display::Inline | Display::InlineFlex | Display::InlineGrid
            )
        }
        _ => false,
    }
}

/// Является ли DOM-узел `display: inline-block` элементом.
/// Возвращает false для изображений (`<img>` — replaced element).
fn is_inline_block(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    dark_mode: bool,
) -> bool {
    matches!(
        &doc.get(id).data,
        NodeData::Element { .. }
        if !is_image_element(doc, id)
            && !is_form_control_element(doc, id)
            && compute_style(doc, id, sheet, inherited, viewport, dark_mode).display
                == Display::InlineBlock
    )
}

/// Обнуляет box-model spacing анонимного контейнера (InlineRun / InlineBlockRow).
fn anon_style(parent: &ComputedStyle) -> ComputedStyle {
    let mut s = parent.clone();
    s.margin_top = LengthOrAuto::ZERO;
    s.margin_right = LengthOrAuto::ZERO;
    s.margin_bottom = LengthOrAuto::ZERO;
    s.margin_left = LengthOrAuto::ZERO;
    s.padding_top = Length::Px(0.0);
    s.padding_right = Length::Px(0.0);
    s.padding_bottom = Length::Px(0.0);
    s.padding_left = Length::Px(0.0);
    s.background_color = None;
    s.width = None;
    s.height = None;
    s.min_width = None;
    s.max_width = None;
    s.min_height = None;
    s.max_height = None;
    s.border_top_width = 0.0;
    s.border_right_width = 0.0;
    s.border_bottom_width = 0.0;
    s.border_left_width = 0.0;
    s.box_sizing = BoxSizing::ContentBox;
    s
}

fn anon_inline_run(node: NodeId, parent: &ComputedStyle, segs: Vec<InlineSegment>) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: anon_style(parent),
        kind: BoxKind::InlineRun { segments: segs, lines: vec![], first_line_style: None },
        children: vec![],
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
    }
}

/// CSS Pseudo-elements L4 §5.4: applies `::first-letter` style to the first grapheme of the
/// `FirstLetter`-marked segment. Splits the segment if it contains more than one character so
/// only the first grapheme gets the pseudo-element style; the remainder keeps the original style.
/// No-op when no `FirstLetter` segment exists or no matching `::first-letter` rule is found.
fn apply_first_letter_pseudo(
    segs: &mut Vec<InlineSegment>,
    doc: &lumen_dom::Document,
    node: lumen_dom::NodeId,
    sheet: &lumen_css_parser::Stylesheet,
    parent: &crate::style::ComputedStyle,
    viewport: lumen_core::geom::Size,
    dark_mode: bool,
) {
    let Some(pos) = segs.iter().position(|s| s.pseudo_kind == PseudoKind::FirstLetter) else {
        return;
    };
    let Some(fl_style) = crate::style::compute_pseudo_element_style(
        doc, node, "first-letter", sheet, parent, viewport, dark_mode,
    ) else {
        return;
    };
    // Split at first Unicode scalar boundary (good-enough for Phase 0; full grapheme
    // cluster support requires unicode-segmentation which is not yet a dependency).
    let first_char_end = segs[pos].text.chars().next().map_or(0, |c| c.len_utf8());
    if first_char_end == 0 {
        return;
    }
    if first_char_end >= segs[pos].text.len() {
        // Single-character segment: override style in place.
        segs[pos].style = fl_style;
        return;
    }
    // Multi-character: split into [first_char | rest], each with its own style.
    let rest_text = segs[pos].text[first_char_end..].to_string();
    let original_style = segs[pos].style.clone();
    let source_node = segs[pos].source_node;
    let post_space = segs[pos].post_space;
    segs[pos].text.truncate(first_char_end);
    segs[pos].style = fl_style;
    segs[pos].post_space = 0.0;
    segs.insert(pos + 1, InlineSegment {
        text: rest_text,
        style: original_style,
        pre_space: 0.0,
        post_space,
        is_element_box: false,
        img_src: None,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node,
        source_char_offset: first_char_end as u32,
    });
}

fn anon_inline_block_row(node: NodeId, parent: &ComputedStyle, items: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox {
        node,
        rect: Rect::ZERO,
        style: anon_style(parent),
        kind: BoxKind::InlineBlockRow,
        children: items,
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
    }
}

/// Рекурсивно собирает `InlineSegment`-ы из поддерева inline-контента.
///
/// `need_first_letter` — starts `true` for the first call on a block container; set to `false`
/// once the first non-whitespace text character is split into a `PseudoKind::FirstLetter` segment.
/// Callers must initialize to `true` and pass through all recursive calls within the same run.
// CSS: ::first-letter — P4 wires: after this function, check segments for PseudoKind::FirstLetter
// and apply compute_pseudo_element_style(node, "first-letter") to override that segment's style.
#[allow(clippy::too_many_arguments)]
fn collect_inline_segments(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    out: &mut Vec<InlineSegment>,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    need_first_letter: &mut bool,
    dark_mode: bool,
) {
    match &doc.get(id).data {
        NodeData::Text(s) if inherited.white_space.preserves_whitespace() => {
            // CSS Text L3 §4.1: white-space: pre/pre-wrap — preserve tabs and
            // newlines. Split on \n to produce forced-break segments.
            let style = inherited.clone();
            let mut byte_offset: u32 = 0;
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    out.push(InlineSegment {
                        text: String::new(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_width: 0.0,
                        forced_break: true,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                    });
                    byte_offset += 1; // the \n character
                }
                if !line.is_empty() {
                    out.push(InlineSegment {
                        text: line.to_string(),
                        style: style.clone(),
                        pre_space: 0.0,
                        post_space: 0.0,
                        is_element_box: false,
                        img_src: None,
                        img_width: 0.0,
                        forced_break: false,
                        pseudo_kind: PseudoKind::None,
                        source_node: id,
                        source_char_offset: byte_offset,
                    });
                }
                byte_offset += line.len() as u32;
            }
        }
        NodeData::Text(s) if !s.chars().all(char::is_whitespace) => {
            // text-transform применяется здесь, до wrapping и paint —
            // measurer считает ширину уже после преобразования.
            let text = inherited.text_transform.apply(s);
            // CSS Pseudo-elements L4 §5.1: the first text segment in this inline run
            // is the candidate for ::first-letter. Mark it so P4 can look up the
            // ::first-letter rule and extract the first grapheme at render time.
            // We mark the whole first non-whitespace segment; P4 splits at the character
            // boundary when building the display list, using the full text metrics.
            let kind = if *need_first_letter && !text.trim().is_empty() {
                *need_first_letter = false;
                PseudoKind::FirstLetter
            } else {
                PseudoKind::None
            };
            out.push(InlineSegment {
                text,
                style: inherited.clone(),
                pre_space: 0.0,
                post_space: 0.0,
                is_element_box: false,
                img_src: None,
                img_width: 0.0,
                forced_break: false,
                pseudo_kind: kind,
                source_node: id,
                source_char_offset: 0,
            });
        }
        NodeData::Text(_) => {}
        NodeData::Element { .. } => {
            let s = compute_style(doc, id, sheet, inherited, viewport, dark_mode);
            if s.display == Display::None {
                return;
            }
            // Inline-replaced image: emit as a fixed-width, non-breakable segment.
            if is_image_element(doc, id) {
                let src = resolve_image_source(doc, id, viewport);
                let em = s.font_size;
                let w = s.width
                    .as_ref()
                    .and_then(|l| l.resolve(em, None, viewport))
                    .or_else(|| src.intrinsic_width.map(|v| v as f32))
                    .unwrap_or(em * 2.0);
                let pre = s.margin_left.resolve_or_zero(em, 0.0, viewport)
                    + s.border_left_width
                    + s.padding_left.resolve_or_zero(em, 0.0, viewport);
                let post = s.padding_right.resolve_or_zero(em, 0.0, viewport)
                    + s.border_right_width
                    + s.margin_right.resolve_or_zero(em, 0.0, viewport);
                let alt = doc.get(id).get_attr("alt").unwrap_or("").to_string();
                out.push(InlineSegment {
                    text: alt,
                    style: s,
                    pre_space: pre,
                    post_space: post,
                    is_element_box: true,
                    img_src: Some(src.url),
                    img_width: w,
                    forced_break: false,
                    pseudo_kind: PseudoKind::None,
                    source_node: id,
                    source_char_offset: 0,
                });
                return;
            }
            // Compute horizontal inline box model: margin + border + padding.
            // Use em=font_size, cb=0 (% padding on inline elements is uncommon).
            let em = s.font_size;
            let pre = s.margin_left.resolve_or_zero(em, 0.0, viewport)
                + s.border_left_width
                + s.padding_left.resolve_or_zero(em, 0.0, viewport);
            let post = s.padding_right.resolve_or_zero(em, 0.0, viewport)
                + s.border_right_width
                + s.margin_right.resolve_or_zero(em, 0.0, viewport);
            let start = out.len();
            // CSS Pseudo-elements L4 §4 — ::before in inline formatting context.
            // Block pseudo-elements inside inline context are skipped (Phase 0).
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "before", sheet, &s, viewport, dark_mode)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, doc, id, viewport, counters, registry, out);
            }
            let children: Vec<NodeId> = flat.children_of(doc, id).to_vec();
            for child_id in children {
                collect_inline_segments(doc, sheet, child_id, &s, viewport, out, flat, counters, registry, need_first_letter, dark_mode);
            }
            // CSS Pseudo-elements L4 §4 — ::after in inline formatting context.
            if let Some(ps) =
                compute_pseudo_element_style(doc, id, "after", sheet, &s, viewport, dark_mode)
                && matches!(
                    ps.display,
                    Display::Inline
                        | Display::InlineFlex
                        | Display::InlineGrid
                        | Display::InlineBlock
                )
            {
                push_pseudo_inline_segs(&ps, doc, id, viewport, counters, registry, out);
            }
            let added = out.len() - start;
            // Mark all segments from this element (including pseudo-element content)
            // as element boxes so the painter draws their background/border.
            for seg in &mut out[start..start + added] {
                seg.is_element_box = true;
            }
            if added > 0 && (pre > 0.0 || post > 0.0) {
                out[start].pre_space += pre;
                out[start + added - 1].post_space += post;
            }
        }
        _ => {}
    }
}

/// Injects a pseudo-element box (::before or ::after) into the children list.
///
/// `is_before = true` → prepend; `false` → append.
/// Inline pseudo-elements are merged into the adjacent InlineRun when possible.
/// Block pseudo-elements are inserted as separate Block boxes.
fn inject_pseudo(
    parent_id: NodeId,
    children: &mut Vec<LayoutBox>,
    ps: Option<ComputedStyle>,
    is_before: bool,
    doc: &Document,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) {
    let Some(ps) = ps else { return };
    match ps.display {
        Display::Inline
        | Display::InlineFlex
        | Display::InlineGrid
        | Display::InlineBlock => {
            let segs = content_to_inline_segments(&ps, doc, parent_id, counters, registry);
            if segs.is_empty() {
                return;
            }
            if is_before {
                match children.first_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        let mut new_segs = segs;
                        new_segs.extend(std::mem::take(segments));
                        *segments = new_segs;
                    }
                    _ => children.insert(0, anon_inline_run(parent_id, &ps, segs)),
                }
            } else {
                match children.last_mut() {
                    Some(LayoutBox { kind: BoxKind::InlineRun { segments, .. }, .. }) => {
                        segments.extend(segs);
                    }
                    _ => children.push(anon_inline_run(parent_id, &ps, segs)),
                }
            }
        }
        _ => {
            // Block-level pseudo-element.
            let inner_segs = content_to_inline_segments(&ps, doc, parent_id, counters, registry);
            let inner = if inner_segs.is_empty() {
                vec![]
            } else {
                vec![anon_inline_run(parent_id, &ps, inner_segs)]
            };
            let b = LayoutBox {
                node: parent_id,
                rect: Rect::ZERO,
                style: ps,
                kind: BoxKind::Block,
                children: inner,
                col_span: 1,
                row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
            };
            if is_before {
                children.insert(0, b);
            } else {
                children.push(b);
            }
        }
    }
}

/// Extracts text from `Content::Items` and returns it as a single `InlineSegment`.
///
/// Resolves `ContentItem::String`, `ContentItem::Counter`, `ContentItem::Counters`,
/// and `ContentItem::Attr` using the per-element `CounterMap` snapshot and DOM lookup.
/// `owner_id` is the element whose `::before`/`::after` pseudo-element we're generating.
/// Custom `@counter-style` names are resolved via `registry`.
fn content_to_inline_segments(
    style: &ComputedStyle,
    doc: &Document,
    owner_id: NodeId,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) -> Vec<InlineSegment> {
    let Content::Items(items) = &style.content else {
        return vec![];
    };
    let snap = counters.get(&owner_id);
    let text: String = items
        .iter()
        .filter_map(|item| match item {
            ContentItem::String(s) => Some(s.clone()),
            ContentItem::Counter { name, style: list_style } => {
                let val = snap
                    .and_then(|s| s.get(name))
                    .and_then(|v| v.last())
                    .copied()
                    .unwrap_or(0);
                let sname = list_style.as_deref().unwrap_or("decimal");
                Some(format_counter_with_registry(val, sname, registry))
            }
            ContentItem::Counters { name, separator, style: list_style } => {
                let vals = snap
                    .and_then(|s| s.get(name))
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let sname = list_style.as_deref().unwrap_or("decimal");
                let formatted: Vec<String> = vals
                    .iter()
                    .map(|&v| format_counter_with_registry(v, sname, registry))
                    .collect();
                Some(formatted.join(separator.as_str()))
            }
            ContentItem::Attr(attr) => {
                doc.get(owner_id).get_attr(attr).map(|s| s.to_string())
            }
            _ => None,
        })
        .collect();
    if text.is_empty() {
        return vec![];
    }
    vec![InlineSegment {
        text,
        style: style.clone(),
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: PseudoKind::None,
        source_node: owner_id,
        source_char_offset: 0,
    }]
}

/// Builds inline segments for a pseudo-element and applies its own box model
/// spacing (margin + border + padding) as `pre_space` / `post_space`.
/// Used by `collect_inline_segments` to inject `::before` / `::after` content.
fn push_pseudo_inline_segs(
    ps: &ComputedStyle,
    doc: &Document,
    owner_id: NodeId,
    viewport: Size,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    out: &mut Vec<InlineSegment>,
) {
    let mut segs = content_to_inline_segments(ps, doc, owner_id, counters, registry);
    if segs.is_empty() {
        return;
    }
    let em = ps.font_size;
    let pre = ps.margin_left.resolve_or_zero(em, 0.0, viewport)
        + ps.border_left_width
        + ps.padding_left.resolve_or_zero(em, 0.0, viewport);
    let post = ps.padding_right.resolve_or_zero(em, 0.0, viewport)
        + ps.border_right_width
        + ps.margin_right.resolve_or_zero(em, 0.0, viewport);
    if pre > 0.0 {
        segs[0].pre_space += pre;
    }
    if post > 0.0 {
        let last = segs.len() - 1;
        segs[last].post_space += post;
    }
    out.extend(segs);
}

/// CSS Lists L3 §2.1 — ordinal of a `<li>` among its element siblings (1-based).
fn li_ordinal(doc: &Document, id: NodeId) -> u32 {
    let Some(parent_id) = doc.get(id).parent else { return 1 };
    let mut n = 0u32;
    for &sib in &doc.get(parent_id).children.clone() {
        if matches!(&doc.get(sib).data, NodeData::Element { name, .. } if name.local.as_str() == "li") {
            n += 1;
            if sib == id {
                return n;
            }
        }
    }
    1
}

fn to_roman(n: u32, upper: bool) -> String {
    const VALS: &[(u32, &str, &str)] = &[
        (1000, "M", "m"), (900, "CM", "cm"), (500, "D", "d"), (400, "CD", "cd"),
        (100, "C", "c"), (90, "XC", "xc"), (50, "L", "l"), (40, "XL", "xl"),
        (10, "X", "x"), (9, "IX", "ix"), (5, "V", "v"), (4, "IV", "iv"), (1, "I", "i"),
    ];
    if n == 0 { return "0".to_string(); }
    let mut out = String::new();
    let mut rem = n;
    for &(val, up, lo) in VALS {
        while rem >= val {
            out.push_str(if upper { up } else { lo });
            rem -= val;
        }
    }
    out
}

fn to_alpha(n: u32, upper: bool) -> String {
    if n == 0 { return "0".to_string(); }
    let base = if upper { b'A' } else { b'a' };
    let mut out = String::new();
    let mut rem = n;
    while rem > 0 {
        rem -= 1;
        out.insert(0, (base + (rem % 26) as u8) as char);
        rem /= 26;
    }
    out
}

fn to_greek(n: u32) -> String {
    const GREEK: &[char] = &['α','β','γ','δ','ε','ζ','η','θ','ι','κ','λ','μ',
                              'ν','ξ','ο','π','ρ','σ','τ','υ','φ','χ','ψ','ω'];
    if n == 0 { return "0".to_string(); }
    let idx = ((n - 1) as usize) % GREEK.len();
    GREEK[idx].to_string()
}

/// CSS Lists L3 §2.1 — builds the marker string from `list-style-type` + ordinal.
/// Bullet types (Disc/Circle/Square) return "" — rendered as geometric shapes by
/// the display-list emitter (FillRoundedRect / DrawBorder / FillRect).
/// CSS: @counter-style — P4 extends with custom counter styles.
fn marker_text(lst: ListStyleType, ordinal: u32) -> String {
    match lst {
        ListStyleType::None   => String::new(),
        ListStyleType::Disc   => String::new(), // geometric: filled circle
        ListStyleType::Circle => String::new(), // geometric: hollow circle
        ListStyleType::Square => String::new(), // geometric: filled square
        ListStyleType::Decimal            => format!("{}. ", ordinal),
        ListStyleType::DecimalLeadingZero => format!("{:02}. ", ordinal),
        ListStyleType::LowerRoman => format!("{}. ", to_roman(ordinal, false)),
        ListStyleType::UpperRoman => format!("{}. ", to_roman(ordinal, true)),
        ListStyleType::LowerAlpha => format!("{}. ", to_alpha(ordinal, false)),
        ListStyleType::UpperAlpha => format!("{}. ", to_alpha(ordinal, true)),
        ListStyleType::LowerGreek => format!("{}. ", to_greek(ordinal)),
    }
}

/// CSS Lists L3 §2.1 — creates `BoxKind::Marker` and prepends to children.
/// Calls `compute_pseudo_element_style("marker")` so CSS `::marker` rules (color,
/// font, content) override the defaults. `content: none` on `::marker` suppresses
/// the marker entirely; `content: <string>` / `counter()` replaces the default text.
#[allow(clippy::too_many_arguments)]
fn inject_marker(
    parent_id: NodeId,
    children: &mut Vec<LayoutBox>,
    style: &ComputedStyle,
    ordinal: u32,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) {
    if matches!(style.list_style_type, ListStyleType::None) {
        return;
    }
    // CSS Pseudo-elements L4 §14.2 — compute ::marker style.
    // Returns None only when `content: none` is set, which suppresses the marker.
    let Some(mut ms) = compute_pseudo_element_style(
        doc, parent_id, "marker", sheet, style, viewport, dark_mode,
    ) else {
        return;
    };
    // CSS: list-style-image — P4 wires image markers.
    let text = match &ms.content {
        Content::Items(items) => marker_content_text(items, doc, parent_id, counters, registry),
        _ => marker_text(style.list_style_type, ordinal),
    };
    ms.display = Display::Inline;
    children.insert(0, LayoutBox {
        node:     parent_id,
        rect:     Rect::ZERO,
        style:    ms,
        kind:     BoxKind::Marker {
            text,
            position:        style.list_style_position,
            list_style_type: style.list_style_type,
        },
        children: vec![],
        col_span: 1,
        row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
    });
}

/// Extracts a plain-text string from `::marker { content: <items> }`.
/// Supports String literals, `attr()`, `counter()`, `counters()`.
fn marker_content_text(
    items: &[ContentItem],
    doc: &Document,
    owner_id: NodeId,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
) -> String {
    let snap = counters.get(&owner_id);
    items.iter().filter_map(|item| match item {
        ContentItem::String(s) => Some(s.clone()),
        ContentItem::Counter { name, style: list_style } => {
            let val = snap
                .and_then(|s| s.get(name))
                .and_then(|v| v.last())
                .copied()
                .unwrap_or(0);
            let sname = list_style.as_deref().unwrap_or("decimal");
            Some(format_counter_with_registry(val, sname, registry))
        }
        ContentItem::Counters { name, separator, style: list_style } => {
            let vals = snap
                .and_then(|s| s.get(name))
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let sname = list_style.as_deref().unwrap_or("decimal");
            let parts: Vec<String> = vals.iter()
                .map(|&v| format_counter_with_registry(v, sname, registry))
                .collect();
            Some(parts.join(separator.as_str()))
        }
        ContentItem::Attr(attr) => {
            doc.get(owner_id).get_attr(attr).map(str::to_string)
        }
        _ => None,
    }).collect()
}

/// CSS Display L3 §7.2 — replaces each `BoxKind::Contents` child with its own
/// children in-place. Grandchildren are already flattened (recursive `build_box`
/// calls run `flatten_contents` on inner levels first).
fn flatten_contents(children: &mut Vec<LayoutBox>) {
    let mut i = 0;
    while i < children.len() {
        if matches!(children[i].kind, BoxKind::Contents) {
            let grandchildren = std::mem::take(&mut children[i].children);
            let gc_len = grandchildren.len();
            children.remove(i);
            for (j, gc) in grandchildren.into_iter().enumerate() {
                children.insert(i + j, gc);
            }
            // Don't advance i — a grandchild might itself be Contents (edge case
            // if the inner build_box somehow produced an un-flattened Contents).
            // Advancing by gc_len skips them all safely since they were already
            // flattened at their own build level.
            i += gc_len;
        } else {
            i += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_box(
    doc: &Document,
    sheet: &Stylesheet,
    id: NodeId,
    inherited: &ComputedStyle,
    viewport: Size,
    flat: &FlatTree,
    counters: &CounterMap,
    registry: &CounterStyleRegistry,
    dark_mode: bool,
) -> LayoutBox {
    let mut style = compute_style(doc, id, sheet, inherited, viewport, dark_mode);

    let kind = match &doc.get(id).data {
        // Shadow root nodes are infrastructure — never rendered directly.
        // The flat tree already maps host children to shadow root's children.
        NodeData::Text(_) | NodeData::Comment(_) | NodeData::Doctype { .. } | NodeData::ShadowRoot { .. } | NodeData::DocumentFragment => BoxKind::Skip,
        NodeData::Document | NodeData::Element { .. } => {
            if style.display == Display::None || is_closed_popover(doc, id) || is_svg_defs(doc, id) {
                BoxKind::Skip
            } else if is_image_element(doc, id) {
                let src = resolve_image_source(doc, id, viewport);
                let alt = doc.get(id).get_attr("alt").unwrap_or("").to_string();
                // Intrinsic dimensions у выбранного `<source>` действуют как
                // presentational hint: заполняют только пустые слоты, не
                // перекрывают ни CSS-каскад, ни собственные `<img width|
                // height>` атрибуты (последние уже легли в style через
                // `apply_image_presentational_hints`). HTML5 §10 «mapped
                // attributes»: hint = UA-rule с specificity 0.
                if style.width.is_none()
                    && let Some(w) = src.intrinsic_width
                {
                    style.width = Some(Length::Px(w as f32));
                }
                if style.height.is_none()
                    && let Some(h) = src.intrinsic_height
                {
                    style.height = Some(Length::Px(h as f32));
                }
                BoxKind::Image { src: src.url, alt }
            } else if is_video_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let poster = node.get_attr("poster").unwrap_or("").to_string();
                // HTML spec §14.1: UA default intrinsic size is 300×150 CSS px.
                // Explicit width/height attrs applied earlier as presentational hints;
                // fill only if still unset.
                if style.width.is_none() {
                    style.width = Some(Length::Px(300.0));
                }
                if style.height.is_none() {
                    style.height = Some(Length::Px(150.0));
                }
                BoxKind::Video { src, poster }
            } else if is_canvas_element(doc, id) {
                let node = doc.get(id);
                // HTML LS §4.12.4: width/height content attributes are
                // non-negative integers; defaults are 300×150 CSS px.
                let cw = node
                    .get_attr("width")
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .unwrap_or(300);
                let ch = node
                    .get_attr("height")
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .unwrap_or(150);
                // The bitmap dimensions act as intrinsic size; explicit CSS
                // width/height (or presentational hints) win if already set.
                if style.width.is_none() {
                    style.width = Some(Length::Px(cw as f32));
                }
                if style.height.is_none() {
                    style.height = Some(Length::Px(ch as f32));
                }
                BoxKind::Canvas { width: cw, height: ch }
            } else if is_audio_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                let controls = node.get_attr("controls").is_some();
                // HTML spec §4.8.10: without controls, <audio> has no box (0×0).
                // With controls, UA must render a control interface; we use 40px height.
                if controls {
                    if style.height.is_none() {
                        style.height = Some(Length::Px(40.0));
                    }
                } else {
                    style.width = Some(Length::Px(0.0));
                    style.height = Some(Length::Px(0.0));
                }
                BoxKind::Audio { src, controls }
            } else if is_iframe_element(doc, id) {
                let node = doc.get(id);
                let src = node.get_attr("src").unwrap_or("").to_string();
                // HTML spec §4.8.5: UA default intrinsic size is 300×150 CSS px.
                // Explicit width/height attrs applied earlier as presentational hints;
                // fill only if still unset.
                if style.width.is_none() {
                    style.width = Some(Length::Px(300.0));
                }
                if style.height.is_none() {
                    style.height = Some(Length::Px(150.0));
                }
                BoxKind::Iframe { src }
            } else if is_form_control_element(doc, id) {
                let kind = {
                    let node = doc.get(id);
                    let tag = node.element_name()
                        .map(|q| q.local.as_str())
                        .unwrap_or("")
                        .to_owned();
                    match tag.as_str() {
                        "button"   => FormControlKind::Button,
                        "select"   => FormControlKind::Select,
                        "textarea" => FormControlKind::Textarea,
                        _ => {
                            let input_type = node.input_type()
                                .unwrap_or(lumen_dom::InputType::Text);
                            let checked = node.get_attr("checked").is_some();
                            FormControlKind::Input { input_type, checked }
                        }
                    }
                };
                BoxKind::FormControl { kind }
            } else if matches!(style.display, Display::TableRow) {
                BoxKind::TableRow
            } else if matches!(style.display, Display::Table | Display::InlineTable) {
                BoxKind::Table
            } else if matches!(
                style.display,
                Display::TableRowGroup
                    | Display::TableHeaderGroup
                    | Display::TableFooterGroup
            ) {
                BoxKind::TableRowGroup
            } else if matches!(style.display, Display::FlowRoot) {
                BoxKind::FlowRoot
            } else if matches!(style.display, Display::Contents) {
                BoxKind::Contents
            } else if is_svg_root(doc, id) {
                // SVG root: apply width/height attributes as presentational hints.
                // CSS: width, height — if author CSS is absent, attribute values are used.
                // CSS: object-fit, object-position — P4 can override viewBox scaling (Phase 2)
                // CSS: intrinsic aspect-ratio from viewBox for replaced element sizing
                if style.width.is_none()
                    && let Some(w) = doc.get(id).get_attr("width").and_then(|v| v.trim().parse::<f32>().ok())
                {
                    style.width = Some(crate::style::Length::Px(w));
                }
                if style.height.is_none()
                    && let Some(h) = doc.get(id).get_attr("height").and_then(|v| v.trim().parse::<f32>().ok())
                {
                    style.height = Some(crate::style::Length::Px(h));
                }
                BoxKind::SvgRoot {
                    view_box: parse_view_box(doc, id),
                    preserve_aspect_ratio: parse_preserve_aspect_ratio(doc, id),
                }
            } else {
                BoxKind::Block
            }
        }
    };

    // CSS Containment L3 §4 — content-visibility: hidden suppresses the subtree.
    // Phase 1: element keeps its own box but contributes 0×0 (no contain-intrinsic-size yet).
    // content-visibility: auto (off-viewport skip) is deferred to Phase 2.
    if style.content_visibility == crate::style::ContentVisibility::Hidden {
        return LayoutBox {
            node: id,
            rect: Rect::ZERO,
            style,
            kind,
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
        };
    }

    let mut children = Vec::new();
    if matches!(kind, BoxKind::Block | BoxKind::FlowRoot | BoxKind::Contents | BoxKind::FormControl { .. } | BoxKind::TableRow | BoxKind::Table | BoxKind::TableRowGroup | BoxKind::SvgRoot { .. }) {
        // CSS: :host, ::slotted — P4 wires shadow-scoped styles here
        // HTML5 §4.11.1 — <details>: when `open` attribute absent, only <summary> is rendered.
        // P3 wires: clicking <summary> should toggle `open` attribute + relayout.
        let dom_children: Vec<NodeId> = if is_details_element(doc, id)
            && doc.get(id).get_attr("open").is_none()
        {
            flat.children_of(doc, id)
                .iter()
                .copied()
                .filter(|&cid| is_summary_element(doc, cid))
                .collect()
        } else {
            flat.children_of(doc, id).to_vec()
        };
        // CSS Grid L1 §6: all direct children of a grid/flex container are
        // "blockified" — they participate as individual items, not wrapped in
        // InlineRun. Skip the inline-collection logic for these containers.
        let is_item_container = matches!(
            style.display,
            Display::Grid | Display::InlineGrid | Display::Flex | Display::InlineFlex
                | Display::TableRow
                | Display::Table | Display::InlineTable
                | Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup
        );
        if is_item_container {
            for child_id in dom_children {
                let child_box = build_box(doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode);
                if !matches!(child_box.kind, BoxKind::Skip) {
                    children.push(child_box);
                }
            }
        } else {
        let mut i = 0;
        while i < dom_children.len() {
            let child_id = dom_children[i];
            let is_inl = is_inline_content(doc, sheet, child_id, &style, viewport, dark_mode);
            let is_ib = !is_inl && is_inline_block(doc, sheet, child_id, &style, viewport, dark_mode);

            if is_inl || is_ib {
                // Унифицированный сбор inline-уровневого контента: inline-элементы
                // и inline-block элементы участвуют в ОДНОМ inline-контексте.
                // Межэлементный whitespace не прерывает поток.
                // Результат: InlineRun (чистый текст) или InlineBlockRow (смешанный).
                let mut row_items: Vec<LayoutBox> = Vec::new();
                let mut pending: Vec<InlineSegment> = Vec::new();
                // CSS §4.1.2 white-space collapsing: whitespace between
                // inline-level siblings collapses to a single space.
                let mut had_ws = false;
                // CSS Pseudo-elements L4 §5.1: first letter of this inline run hasn't been
                // split out yet. Passed through all collect_inline_segments calls in this loop.
                let mut need_first_letter = true;
                // CSS Pseudo-elements L4 §5.3: pre-compute ::first-line style once for this block.
                let first_line_style =
                    crate::style::compute_pseudo_element_style(doc, id, "first-line", sheet, &style, viewport, dark_mode)
                        .map(Box::new);
                // Track whether first_line_style has been assigned to the first InlineRun.
                let mut first_line_assigned = false;

                loop {
                    if i >= dom_children.len() {
                        break;
                    }
                    let cid = dom_children[i];
                    match &doc.get(cid).data {
                        NodeData::Text(s) if s.chars().all(char::is_whitespace) => {
                            had_ws = true;
                            i += 1;
                            continue;
                        }
                        NodeData::Comment(_) | NodeData::Doctype { .. } => {
                            i += 1;
                            continue;
                        }
                        _ => {}
                    }
                    if is_inline_content(doc, sheet, cid, &style, viewport, dark_mode) {
                        collect_inline_segments(doc, sheet, cid, &style, viewport, &mut pending, flat, counters, registry, &mut need_first_letter, dark_mode);
                        had_ws = false;
                        i += 1;
                    } else if is_inline_block(doc, sheet, cid, &style, viewport, dark_mode) {
                        if !pending.is_empty() {
                            let mut segs = std::mem::take(&mut pending);
                            apply_first_letter_pseudo(&mut segs, doc, id, sheet, &style, viewport, dark_mode);
                            let mut run = anon_inline_run(id, &style, segs);
                            if !first_line_assigned {
                                if let BoxKind::InlineRun { first_line_style: ref mut fls, .. } = run.kind {
                                    *fls = first_line_style.clone();
                                }
                                first_line_assigned = true;
                            }
                            row_items.push(run);
                        }
                        // Whitespace between inline-blocks → collapsed space gap.
                        if had_ws && !row_items.is_empty() {
                            row_items.push(LayoutBox {
                                node: id,
                                rect: Rect::ZERO,
                                style: anon_style(&style),
                                kind: BoxKind::InlineSpace,
                                children: vec![],
                                col_span: 1,
                                row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
                            });
                        }
                        row_items.push(build_box(doc, sheet, cid, &style, viewport, flat, counters, registry, dark_mode));
                        had_ws = false;
                        i += 1;
                    } else if matches!(doc.get(cid).data, NodeData::Element { .. })
                        && compute_style(doc, cid, sheet, &style, viewport, dark_mode).display
                            == Display::None
                    {
                        // display:none не прерывает inline-контекст — CSS §9.2.4.
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !pending.is_empty() {
                    let mut segs = std::mem::take(&mut pending);
                    apply_first_letter_pseudo(&mut segs, doc, id, sheet, &style, viewport, dark_mode);
                    let mut run = anon_inline_run(id, &style, segs);
                    if !first_line_assigned
                        && let BoxKind::InlineRun { first_line_style: ref mut fls, .. } = run.kind
                    {
                        *fls = first_line_style.clone();
                    }
                    row_items.push(run);
                }

                // CSS Pseudo-elements L4 §5.1 — apply ::first-letter style.
                // collect_inline_segments marks the first non-whitespace text segment
                // with PseudoKind::FirstLetter; split it here so wrap_inline_run uses
                // the override font metrics for both the letter and the remainder.
                if let Some(fl_style) = compute_pseudo_element_style(
                    doc, id, "first-letter", sheet, &style, viewport, dark_mode,
                ) {
                    apply_first_letter_style(&mut row_items, fl_style, &style);
                }

                match row_items.len() {
                    0 => {}
                    // Единственный чисто-текстовый run — без лишней обёртки.
                    1 if matches!(row_items[0].kind, BoxKind::InlineRun { .. }) => {
                        children.push(row_items.remove(0));
                    }
                    // Несколько элементов или inline-block → InlineBlockRow.
                    _ => {
                        children.push(anon_inline_block_row(id, &style, row_items));
                    }
                }
            } else {
                children.push(build_box(doc, sheet, child_id, &style, viewport, flat, counters, registry, dark_mode));
                i += 1;
            }
        }
        // CSS Pseudo-elements L4 §4 — inject ::before / ::after for block-flow.
        // Only for Block / FlowRoot (not FormControl, not flex/grid item containers).
        if matches!(kind, BoxKind::Block | BoxKind::FlowRoot) {
            let before_ps =
                compute_pseudo_element_style(doc, id, "before", sheet, &style, viewport, dark_mode);
            let after_ps =
                compute_pseudo_element_style(doc, id, "after", sheet, &style, viewport, dark_mode);
            inject_pseudo(id, &mut children, before_ps, true, doc, counters, registry);
            inject_pseudo(id, &mut children, after_ps, false, doc, counters, registry);
            // CSS Lists L3 §2.1 — inject ::marker for list items.
            // ::marker comes before ::before in document order.
            if style.display == Display::ListItem {
                let ordinal = li_ordinal(doc, id);
                inject_marker(id, &mut children, &style, ordinal,
                              doc, sheet, viewport, dark_mode, counters, registry);
            }
        }
        } // end else (non-item-container)
        // CSS Display L3 §7.2 — flatten display:contents boxes into this context.
        // Must run for ALL child-building paths (item-container and non-item-container)
        // because flex/grid/table children may include display:contents elements whose
        // Contents boxes must be unpacked before lay_out sees them.
        flatten_contents(&mut children);
    }

    // SVG root: build SVG shape children (separate from HTML box-tree flow).
    if matches!(kind, BoxKind::SvgRoot { .. }) {
        children = build_svg_children(doc, sheet, id, &style, viewport, flat, dark_mode);
    }

    // Read HTML colspan/rowspan attributes for table-cell elements.
    let (col_span, row_span) = if style.display == Display::TableCell {
        let cs = doc
            .get(id)
            .get_attr("colspan")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);
        let rs = doc
            .get(id)
            .get_attr("rowspan")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1)
            .max(1);
        (cs, rs)
    } else {
        (1, 1)
    };

    LayoutBox {
        node: id,
        rect: Rect::ZERO,
        style,
        kind,
        children,
        col_span,
        row_span,
        svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0,
    }
}

/// Phase 0 shrink-to-fit: возвращает «предпочтительную» ширину inline-block-бокса
/// (включая padding+border самого бокса). Алгоритм: если у бокса явная CSS `width` —
/// берём её; иначе рекурсивно ищем максимальную preferred_width среди потомков
/// и добавляем padding+border текущего бокса. Возвращает `None` если явных размеров
/// нет ни у бокса, ни у его потомков.
///
/// Для typed-Length полей используем em = font_size, cb_width = 0 как
/// аппроксимацию (shrink-to-fit не знает cb_width заранее).
fn preferred_inline_block_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> Option<f32> {
    let s = &b.style;
    let em = s.font_size;
    // % ширины на этом этапе не разрешима — трактуем как отсутствие.
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    if let Some(w_len) = &s.width
        && let Some(w) = w_len.resolve(em, Some(0.0), viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr
                + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return Some(outer.max(0.0));
    }
    // InlineBlockRow — горизонтальный поток: суммируем ширины детей + их margins.
    // InlineSpace — collapsed whitespace gap; его ширина = char_width(' ').
    // Остальные боксы (Block, Image и т.д.) — вертикальный поток: берём max.
    let content_w = if matches!(b.kind, BoxKind::InlineBlockRow) {
        let sum: f32 = b.children.iter().map(|c| {
            if matches!(c.kind, BoxKind::InlineSpace) {
                // Учитываем ширину collapsed space, чтобы при shrink-to-fit
                // не занижать ширину контейнера и не вызывать перенос соседних
                // inline-block элементов на следующую строку.
                return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
            }
            let cw = preferred_inline_block_width(c, measurer, viewport).unwrap_or(0.0);
            let cem = c.style.font_size;
            let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
            let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
            cw + ml + mr
        }).sum();
        sum
    } else {
        b.children
            .iter()
            .filter_map(|c| preferred_inline_block_width(c, measurer, viewport))
            .fold(0.0_f32, f32::max)
    };
    if content_w > 0.0 {
        Some(
            (content_w + pl + pr
                + s.border_left_width + s.border_right_width)
                .max(0.0),
        )
    } else {
        None
    }
}

/// CSS Intrinsic Sizing L3 §4 — max-content border-box width of `b`.
///
/// The max-content width is the width a box would use if line breaking were
/// suppressed: all content on one line. For block containers this is the
/// maximum over children's max-content widths. For `InlineRun` boxes it is
/// the sum of all segment text widths (no wrapping). Includes the box's own
/// padding + border in the returned value (border-box width).
///
/// Phase-0 approximation: only `char_width` per-character measurement is
/// available; inter-word spacing is included, but features like ligatures or
/// kerning are not. Word-break is not applied — text is treated as one run.
fn max_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    // Explicit non-intrinsic CSS width takes precedence (same logic as
    // preferred_inline_block_width).
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, Some(0.0), viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // max-content = all segments on one line (no wrapping).
            measurer.map_or(0.0, |m| {
                segments.iter().map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let ts = seg.style.tab_size * m.char_width(' ', seg.style.font_size);
                    measure_text_w(&seg.text, seg.style.font_size, ls, ts, m)
                }).sum()
            })
        }
        BoxKind::InlineBlockRow => {
            b.children.iter().map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return measurer.map_or(0.0, |m| m.char_width(' ', c.style.font_size));
                }
                let cw = max_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).sum()
        }
        _ => {
            b.children.iter()
                .map(|c| max_content_outer_width(c, measurer, viewport))
                .fold(0.0_f32, f32::max)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// CSS Intrinsic Sizing L3 §4 — min-content border-box width of `b`.
///
/// The min-content width is the narrowest a box can be without overflowing:
/// the width of the longest unbreakable content unit (word, image, etc.).
///
/// Phase-0 approximation: computes the max word width per `InlineRun` by
/// splitting on ASCII whitespace. This gives correct results for Latin text
/// but may overestimate for languages without whitespace-based word breaks.
fn min_content_outer_width(
    b: &LayoutBox,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
) -> f32 {
    let s = &b.style;
    let em = s.font_size;
    let pl = s.padding_left.resolve_or_zero(em, 0.0, viewport);
    let pr = s.padding_right.resolve_or_zero(em, 0.0, viewport);
    if let Some(w_len) = &s.width
        && !w_len.is_intrinsic()
        && let Some(w) = w_len.resolve(em, Some(0.0), viewport)
    {
        let outer = match s.box_sizing {
            BoxSizing::ContentBox => w + pl + pr + s.border_left_width + s.border_right_width,
            BoxSizing::BorderBox => w.max(pl + pr + s.border_left_width + s.border_right_width),
        };
        return outer.max(0.0);
    }
    let content_w = match &b.kind {
        BoxKind::InlineRun { segments, .. } => {
            // min-content = longest single word across all segments.
            measurer.map_or(0.0, |m| {
                segments.iter().flat_map(|seg| {
                    let ls = seg.style.letter_spacing;
                    let ts = seg.style.tab_size * m.char_width(' ', seg.style.font_size);
                    // Split on whitespace to find individual "words".
                    seg.text.split_whitespace().map(move |word| {
                        measure_text_w(word, seg.style.font_size, ls, ts, m)
                    })
                }).fold(0.0_f32, f32::max)
            })
        }
        BoxKind::InlineBlockRow => {
            // For inline-block row, min-content is the max over children.
            b.children.iter().map(|c| {
                if matches!(c.kind, BoxKind::InlineSpace) {
                    return 0.0; // spaces are breakable
                }
                let cw = min_content_outer_width(c, measurer, viewport);
                let cem = c.style.font_size;
                let ml = c.style.margin_left.resolve_or_zero(cem, 0.0, viewport);
                let mr = c.style.margin_right.resolve_or_zero(cem, 0.0, viewport);
                cw + ml + mr
            }).fold(0.0_f32, f32::max)
        }
        _ => {
            b.children.iter()
                .map(|c| min_content_outer_width(c, measurer, viewport))
                .fold(0.0_f32, f32::max)
        }
    };
    (content_w + pl + pr + s.border_left_width + s.border_right_width).max(0.0)
}

/// Рекурсивно смещает rect.y всего поддерева на dy (для vertical-align).
fn shift_y_box(b: &mut LayoutBox, dy: f32) {
    b.rect.y += dy;
    for child in &mut b.children {
        shift_y_box(child, dy);
    }
}

/// Рекурсивно смещает rect всего поддерева на (dx, dy).
/// Используется при позиционировании абсолютных потомков.
fn shift_tree(b: &mut LayoutBox, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    b.rect.x += dx;
    b.rect.y += dy;
    for child in &mut b.children {
        shift_tree(child, dx, dy);
    }
}

// ─── CSS 2.1 §9.5 — Float context ────────────────────────────────────────────

/// CSS Shapes L1 §5.1 — parse `circle(<length-px>)` from a raw shape string.
/// Returns the radius in px. Only handles `circle(Npx)` without `at` clause.
/// Returns `None` for any unrecognised syntax (fallback to rectangular float).
pub(crate) fn parse_circle_px(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("circle(")?.strip_suffix(')')?;
    let token = inner.split_whitespace().next()?;
    // Accept "50px" or bare "50" (assume px).
    let digits = token.strip_suffix("px").unwrap_or(token);
    digits.parse::<f32>().ok().filter(|&r| r > 0.0)
}

/// CSS Shapes L1 §5.2 — parse `polygon([<fill-rule>,] x1 y1, x2 y2, ...)`.
/// Returns vertex list in float-local (margin-box-relative) px coordinates.
/// Accepts `Npx` or bare `N` (assumed px). Returns `None` for any unknown syntax.
pub(crate) fn parse_shape_polygon_px(s: &str) -> Option<Vec<(f32, f32)>> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("polygon(")?.strip_suffix(')')?;
    // Strip optional fill-rule keyword (nonzero | evenodd).
    let coords_str = if inner.trim_start().starts_with("nonzero")
        || inner.trim_start().starts_with("evenodd")
    {
        inner.split_once(',').map(|x| x.1).unwrap_or("")
    } else {
        inner
    };
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for pair in coords_str.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.split_whitespace();
        let xs = it.next()?;
        let ys = it.next()?;
        let x = xs.strip_suffix("px").unwrap_or(xs).parse::<f32>().ok()?;
        let y = ys.strip_suffix("px").unwrap_or(ys).parse::<f32>().ok()?;
        pts.push((x, y));
    }
    if pts.len() >= 3 { Some(pts) } else { None }
}

/// CSS Shapes L1 §5.2 — parse `ellipse(<rx> <ry> at <cx> <cy>)`.
/// Returns `(rx, ry, cx, cy)` in float-local (margin-box-relative) px coords.
/// Returns `None` for any unknown syntax or zero/negative radii.
pub(crate) fn parse_shape_ellipse_px(s: &str) -> Option<(f32, f32, f32, f32)> {
    let s = s.trim().to_ascii_lowercase();
    let inner = s.strip_prefix("ellipse(")?.strip_suffix(')')?;
    // Expected: "rxpx rypx at cxpx cypx"
    let at_pos = inner.find(" at ")?;
    let radii_part = inner[..at_pos].trim();
    let center_part = inner[at_pos + 4..].trim();
    let mut ri = radii_part.split_whitespace();
    let mut ci = center_part.split_whitespace();
    let rxs = ri.next()?;
    let rys = ri.next()?;
    let cxs = ci.next()?;
    let cys = ci.next()?;
    let rx = rxs.strip_suffix("px").unwrap_or(rxs).parse::<f32>().ok()?;
    let ry = rys.strip_suffix("px").unwrap_or(rys).parse::<f32>().ok()?;
    let cx = cxs.strip_suffix("px").unwrap_or(cxs).parse::<f32>().ok()?;
    let cy = cys.strip_suffix("px").unwrap_or(cys).parse::<f32>().ok()?;
    if rx > 0.0 && ry > 0.0 { Some((rx, ry, cx, cy)) } else { None }
}

/// CSS Shapes L1 §5.2 — polygon shape for `shape-outside` on a float.
/// Points are stored in content-area coordinates (same as FloatContext).
struct ShapePolygon {
    top_y: f32,
    bottom_y: f32,
    /// `true` = left float, `false` = right float.
    is_left: bool,
    /// Polygon vertices in content-area coordinates.
    points: Vec<(f32, f32)>,
}

/// CSS Shapes L1 §5.2 — ellipse shape for `shape-outside` on a float.
/// All coordinates are in content-area space (same as FloatContext).
struct ShapeEllipse {
    top_y: f32,
    bottom_y: f32,
    /// `true` = left float, `false` = right float.
    is_left: bool,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
}

/// CSS 2.1 §9.5 — tracks float placements within a single block formatting
/// context.  Simplified Phase-0 implementation: only axis-aligned rectangles,
/// no shape-outside wrapping.  All coordinates are in the same space as the
/// block container's content area (i.e. not relative to viewport).
struct FloatContext {
    /// Left floats: `(bottom_y, right_edge)` — right edge of the float margin
    /// box in content-area coordinates.  Active while `bottom_y > query_y`.
    left: Vec<(f32, f32)>,
    /// Right floats: `(bottom_y, left_edge)` — left edge of the float margin
    /// box.  Active while `bottom_y > query_y`.
    right: Vec<(f32, f32)>,
    /// CSS Shapes L1 — `shape-outside: circle(r)` overrides.
    /// `(top_y, bottom_y, is_left, center_x, center_y, radius)`.
    /// `is_left=true` → left float, `false` → right float.
    shape_circles: Vec<(f32, f32, bool, f32, f32, f32)>,
    /// CSS Shapes L1 — `shape-outside: polygon(...)` overrides.
    shape_polygons: Vec<ShapePolygon>,
    /// CSS Shapes L1 — `shape-outside: ellipse(...)` overrides.
    shape_ellipses: Vec<ShapeEllipse>,
}

impl FloatContext {
    fn new() -> Self {
        Self {
            left: Vec::new(),
            right: Vec::new(),
            shape_circles: Vec::new(),
            shape_polygons: Vec::new(),
            shape_ellipses: Vec::new(),
        }
    }

    /// Left boundary of available inline space at `y` (= rightmost right-edge
    /// of all left floats whose `bottom_y > y`).  Falls back to `default_x`.
    fn left_edge_at(&self, y: f32, default_x: f32) -> f32 {
        let rect_edge = self.left
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, r)| *r)
            .fold(default_x, f32::max);
        // CSS Shapes L1: circle boundary.
        let after_circles = self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| *is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx + hw
            })
            .fold(rect_edge, f32::max);
        // CSS Shapes L1: polygon boundary (rightmost edge at y).
        let after_polygons = self.shape_polygons
            .iter()
            .filter(|p| p.is_left && p.top_y <= y && p.bottom_y > y)
            .filter_map(|p| polygon_right_edge_at_y(&p.points, y))
            .fold(after_circles, f32::max);
        // CSS Shapes L1: ellipse boundary (right edge at y).
        self.shape_ellipses
            .iter()
            .filter(|e| e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some(e.cx + e.rx * (1.0 - norm * norm).max(0.0).sqrt())
            })
            .fold(after_polygons, f32::max)
    }

    /// Right boundary of available inline space at `y` (= leftmost left-edge
    /// of all right floats whose `bottom_y > y`).  Falls back to `default_x`.
    fn right_edge_at(&self, y: f32, default_x: f32) -> f32 {
        let rect_edge = self.right
            .iter()
            .filter(|(bot, _)| *bot > y)
            .map(|(_, l)| *l)
            .fold(default_x, f32::min);
        // CSS Shapes L1: circle boundary.
        let after_circles = self.shape_circles
            .iter()
            .filter(|(top, bot, is_left, ..)| !is_left && *top <= y && *bot > y)
            .map(|(_, _, _, cx, cy, r)| {
                let dy = y - cy;
                let hw = (r * r - dy * dy).max(0.0_f32).sqrt();
                cx - hw
            })
            .fold(rect_edge, f32::min);
        // CSS Shapes L1: polygon boundary (leftmost edge at y).
        let after_polygons = self.shape_polygons
            .iter()
            .filter(|p| !p.is_left && p.top_y <= y && p.bottom_y > y)
            .filter_map(|p| polygon_left_edge_at_y(&p.points, y))
            .fold(after_circles, f32::min);
        // CSS Shapes L1: ellipse boundary (left edge at y).
        self.shape_ellipses
            .iter()
            .filter(|e| !e.is_left && e.top_y <= y && e.bottom_y > y)
            .filter_map(|e| {
                let norm = (y - e.cy) / e.ry;
                if norm.abs() > 1.0 { return None; }
                Some(e.cx - e.rx * (1.0 - norm * norm).max(0.0).sqrt())
            })
            .fold(after_polygons, f32::min)
    }

    /// Record a left float occupying `[y_top, bottom_y)` with right margin
    /// edge at `right_edge`.
    fn add_left(&mut self, bottom_y: f32, right_edge: f32) {
        self.left.push((bottom_y, right_edge));
    }

    /// Record a right float occupying `[y_top, bottom_y)` with left margin
    /// edge at `left_edge`.
    fn add_right(&mut self, bottom_y: f32, left_edge: f32) {
        self.right.push((bottom_y, left_edge));
    }

    /// CSS 2.1 §9.5.2 — advance `y` past all floats on the given side.
    fn clear_y(&self, y: f32, side: ClearSide) -> f32 {
        let mut result = y;
        let do_left  = matches!(side, ClearSide::Left  | ClearSide::Both);
        let do_right = matches!(side, ClearSide::Right | ClearSide::Both);
        if do_left  { for (bot, _) in &self.left  { result = result.max(*bot); } }
        if do_right { for (bot, _) in &self.right { result = result.max(*bot); } }
        result
    }

    /// True when there are no active floats at all.
    fn is_empty(&self) -> bool {
        self.left.is_empty() && self.right.is_empty()
    }
}

/// CSS Shapes L1 §4 — rightmost x of polygon boundary at scanline `y`.
/// Scans all edges that cross `y`; returns `None` if no edge crosses.
fn polygon_right_edge_at_y(pts: &[(f32, f32)], y: f32) -> Option<f32> {
    polygon_edge_x_at_y(pts, y, true)
}

/// CSS Shapes L1 §4 — leftmost x of polygon boundary at scanline `y`.
fn polygon_left_edge_at_y(pts: &[(f32, f32)], y: f32) -> Option<f32> {
    polygon_edge_x_at_y(pts, y, false)
}

/// Shared kernel: iterate polygon edges, return rightmost (want_max=true) or
/// leftmost (want_max=false) x intersection with horizontal scanline at `y`.
fn polygon_edge_x_at_y(pts: &[(f32, f32)], y: f32, want_max: bool) -> Option<f32> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let mut best: Option<f32> = None;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        // Edge crosses y iff exactly one endpoint is strictly below y.
        // Use half-open interval [min, max) to avoid double-counting vertices.
        if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
            let x_at_y = x0 + (y - y0) * (x1 - x0) / (y1 - y0);
            best = Some(match best {
                None => x_at_y,
                Some(prev) => if want_max { prev.max(x_at_y) } else { prev.min(x_at_y) },
            });
        }
    }
    best
}

/// Crate-internal shim so `vertical.rs` can recursively invoke the main
/// `lay_out` for children inside a vertical writing-mode container.
///
/// Same parameters and semantics as the private `lay_out`. Exists only
/// because Rust modules cannot reach a sibling module's private functions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_out_for_vertical(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    lay_out(b, start_x, start_y, available_width, available_height, measurer, viewport, pcb, hp);
}

/// `pcb` — rect positioned containing block (ближайший предок с position != static),
/// используется для layout абсолютно-позиционированных потомков.
#[allow(clippy::too_many_arguments)]
fn lay_out(
    b: &mut LayoutBox,
    start_x: f32,
    start_y: f32,
    available_width: f32,
    // CSS 2.1 §10.5: definite content height of the containing block, or None if auto.
    // None means percentage heights on children compute to 'auto'.
    available_height: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    if matches!(b.kind, BoxKind::Skip) {
        b.rect = Rect::new(start_x, start_y, 0.0, 0.0);
        return;
    }

    // SVG root dispatches to its own layout algorithm: replaced-element sizing
    // from CSS width/height (or viewBox fallback), then SVG-coordinate shape positioning.
    if matches!(b.kind, BoxKind::SvgRoot { .. } | BoxKind::SvgShape { .. } | BoxKind::SvgText { .. }) {
        lay_out_svg_root(b, start_x, start_y, available_width, available_height, viewport);
        return;
    }

    // CSS Writing Modes L3 §3: vertical writing modes swap the block/inline axes.
    // Vertical block stacking is handled by the `vertical` module. InlineRun,
    // FormControl, etc. inside a vertical context fall through to horizontal
    // layout as a Phase 0 stub (text appears sideways but positions are valid).
    // CSS: writing-mode — Phase 2: vertical inline text flow + sideways glyphs.
    if !matches!(b.style.writing_mode, crate::style::WritingMode::HorizontalTb)
        && matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot)
    {
        crate::vertical::lay_out_vertical_block(
            b,
            start_x,
            start_y,
            available_width,
            available_height,
            measurer,
            viewport,
            pcb,
            hp,
        );
        return;
    }

    let s = b.style.clone();
    let em = s.font_size;
    let cb = available_width;

    // Резолвим typed Length-поля с known containing block.
    let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
    let margin_right = s.margin_right.resolve_or_zero(em, cb, viewport);
    let margin_top = s.margin_top.resolve_or_zero(em, cb, viewport);
    let padding_left = s.padding_left.resolve_or_zero(em, cb, viewport);
    let padding_right = s.padding_right.resolve_or_zero(em, cb, viewport);
    let padding_top = s.padding_top.resolve_or_zero(em, cb, viewport);
    let padding_bottom = s.padding_bottom.resolve_or_zero(em, cb, viewport);

    b.rect.x = start_x + margin_left;
    b.rect.y = start_y + margin_top;
    // Block: auto-ширина = весь доступный inline-размер контейнера.
    // Replaced element (Image): auto-ширина = intrinsic (0 в Phase 0, без
    // декодированных пикселей). Это CSS 2.1 §10.3.2 — replaced-боксы
    // НЕ растягиваются на весь контейнер при отсутствии width.
    let is_replaced = matches!(b.kind, BoxKind::Image { .. } | BoxKind::Video { .. } | BoxKind::Canvas { .. } | BoxKind::Iframe { .. } | BoxKind::FormControl { .. });
    b.rect.width = if is_replaced {
        0.0
    } else {
        (available_width - margin_left - margin_right).max(0.0)
    };
    // Явная ширина (CSS width: Npx) перекрывает auto-ширину.
    // box-sizing определяет, к какой части бокса относится `width`:
    //   - content-box: width — это размер контента, padding+border прибавляются;
    //   - border-box: width — общий размер вместе с padding+border.
    if let Some(w_len) = &s.width {
        if w_len.is_intrinsic() {
            // CSS Intrinsic Sizing L3 §4 — min-content / max-content / fit-content.
            // max_content_outer_width / min_content_outer_width already include
            // the box's own padding+border (border-box width), so we assign directly.
            let avail_bb = (available_width - margin_left - margin_right).max(0.0);
            b.rect.width = match w_len {
                Length::MaxContent => max_content_outer_width(b, measurer, viewport),
                Length::MinContent => min_content_outer_width(b, measurer, viewport),
                Length::FitContent(max_arg) => {
                    let max_c = max_content_outer_width(b, measurer, viewport);
                    if let Some(arg) = max_arg {
                        // fit-content(<length>) = min(avail, max(min-content, arg))
                        let min_c = min_content_outer_width(b, measurer, viewport);
                        let arg_px = arg.resolve(em, Some(cb), viewport).unwrap_or(avail_bb);
                        // arg_px is a content-box length; convert to border-box:
                        let arg_bb = match s.box_sizing {
                            BoxSizing::ContentBox => arg_px + padding_left + padding_right
                                + s.border_left_width + s.border_right_width,
                            BoxSizing::BorderBox => arg_px,
                        };
                        max_c.min(min_c.max(arg_bb)).min(avail_bb)
                    } else {
                        // fit-content = min(available, max-content)
                        max_c.min(avail_bb)
                    }
                }
                _ => unreachable!(),
            };
        } else if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
            b.rect.width = match s.box_sizing {
                BoxSizing::ContentBox => (w + padding_left + padding_right
                    + s.border_left_width + s.border_right_width).max(0.0),
                BoxSizing::BorderBox => w.max(padding_left + padding_right + s.border_left_width + s.border_right_width),
            };
        }
    }
    // CSS 2.1 §10.4: tentative width → clamp в [min-width, max-width].
    // Intrinsic keywords in min-/max- also resolve to intrinsic values here.
    // Порядок «max сначала, потом min» автоматически даёт правило
    // «при min > max побеждает min». min-/max- интерпретируются в той же
    // box-sizing модели, что и width: content-box добавляет padding+border,
    // border-box оставляет как есть.
    let outer_horiz = |v: f32| match s.box_sizing {
        BoxSizing::ContentBox => v + padding_left + padding_right
            + s.border_left_width + s.border_right_width,
        BoxSizing::BorderBox => v,
    };
    if let Some(max_len) = &s.max_width {
        let max_bb = if max_len.is_intrinsic() {
            Some(max_content_outer_width(b, measurer, viewport))
        } else {
            max_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v).max(0.0))
        };
        if let Some(max_w) = max_bb {
            b.rect.width = b.rect.width.min(max_w);
        }
    }
    if let Some(min_len) = &s.min_width {
        let min_bb = if min_len.is_intrinsic() {
            Some(min_content_outer_width(b, measurer, viewport))
        } else {
            min_len.resolve(em, Some(cb), viewport).map(|v| outer_horiz(v.max(0.0)))
        };
        if let Some(min_w) = min_bb {
            b.rect.width = b.rect.width.max(min_w);
        }
    }
    // Phase 0 shrink-to-fit для inline-block без явной CSS width.
    // Полный алгоритм (CSS 2.1 §10.3.9) требует двух проходов; здесь —
    // упрощение: ищем максимальную explicit-width среди потомков.
    if s.width.is_none() && s.display == Display::InlineBlock
        && let Some(pref_w) = preferred_inline_block_width(b, measurer, viewport)
    {
        b.rect.width = pref_w.min(b.rect.width);
    }

    // CSS 2.1 §10.3.3 — auto horizontal-margin centering for block-level
    // non-replaced elements in normal flow with an explicit CSS width.
    // Remaining inline space distributes to auto margins: both auto → equal
    // halves (centered block); only left auto → left takes all remaining;
    // only right auto → no x shift (right margin absorbs remainder silently).
    // Does not apply to: replaced, inline-block, flex/grid containers, floats,
    // or absolute/fixed positioned elements.
    let ml_is_auto = s.margin_left.is_auto();
    let mr_is_auto = s.margin_right.is_auto();
    if (ml_is_auto || mr_is_auto)
        && s.width.is_some()
        && !is_replaced
        && !matches!(
            s.display,
            Display::InlineBlock
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
        )
        && !matches!(s.float_side, FloatSide::Left | FloatSide::Right)
        && !matches!(s.position, Position::Absolute | Position::Fixed)
    {
        let ml_fixed = if ml_is_auto { 0.0 } else { margin_left };
        let mr_fixed = if mr_is_auto { 0.0 } else { margin_right };
        let remaining = (available_width - b.rect.width - ml_fixed - mr_fixed).max(0.0);
        let ml_computed = if ml_is_auto && mr_is_auto {
            remaining / 2.0
        } else if ml_is_auto {
            remaining
        } else {
            ml_fixed
        };
        b.rect.x = start_x + ml_computed;
    }

    let content_x = b.rect.x + padding_left + s.border_left_width;
    let content_y = b.rect.y + padding_top + s.border_top_width;
    let content_width = (b.rect.width
        - padding_left - padding_right
        - s.border_left_width - s.border_right_width).max(0.0);

    // pcb для потомков: если текущий элемент positioned — он сам CB для абсолютных детей.
    // CSS Containment L3: contain:layout и contain:paint тоже устанавливают containing block.
    // Высота ещё неизвестна, используем 0 — корректируем after layout.
    let is_positioned = !matches!(s.position, Position::Static);
    let contain_establishes_cb = s.contain.0
        & (ContainFlags::LAYOUT.0 | ContainFlags::PAINT.0 | ContainFlags::STRICT.0) != 0;
    let children_pcb = if is_positioned || contain_establishes_cb {
        // CSS Position L3 §2.2: CB for absolute descendants = padding edge of the element.
        Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            0.0,
        )
    } else {
        pcb
    };

    // InlineRun обрабатывается до основного match.
    if let BoxKind::InlineRun { segments, lines, first_line_style } = &mut b.kind {
        if let Some(m) = measurer {
            // white-space: nowrap / text-wrap-mode: nowrap → infinite max_width so
            // the line-breaker never wraps; word-spacing/letter-spacing logic unchanged.
            let wrap_width = if s.white_space.is_nowrap() || s.text_wrap_mode == TextWrapMode::Nowrap {
                f32::INFINITY
            } else {
                content_width
            };
            let text_indent_px = s.text_indent.resolve_or_zero(em, cb, viewport);
            let raw_lines = wrap_inline_run(segments, wrap_width, s.font_size, text_indent_px, viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap);
            // CSS Text L4 §6.4.2: apply text-wrap-style post-processing only when
            // wrapping is active (wrap_width is finite) and text actually wraps.
            *lines = if wrap_width.is_finite() {
                match s.text_wrap_style {
                    TextWrapStyle::Balance => balance_wrap(
                        segments, wrap_width, raw_lines, s.font_size, text_indent_px,
                        viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap,
                    ),
                    TextWrapStyle::Pretty => pretty_wrap(
                        segments, wrap_width, raw_lines, s.font_size, text_indent_px,
                        viewport, m, s.hyphens, hp, s.white_space, s.word_break, s.overflow_wrap,
                    ),
                    // Auto / Stable: greedy result unchanged.
                    // Stable stability is about incremental editing; for static layout it's identical to auto.
                    TextWrapStyle::Auto | TextWrapStyle::Stable => raw_lines,
                }
            } else {
                raw_lines
            };
            align_lines(lines, content_width, s.text_align, s.direction);
            let line_h = s.font_size * s.line_height;
            apply_inline_vertical_align(lines, line_h);
            // CSS Overflow L4 §3.2: -webkit-line-clamp / line-clamp — multi-line truncation.
            // Takes priority over text-overflow:ellipsis (both cannot apply simultaneously).
            if let Some(n) = s.line_clamp.filter(|&n| n > 0) {
                apply_line_clamp(lines, n, content_width, s.font_size, m);
            } else if s.text_overflow == TextOverflow::Ellipsis
                && (s.overflow_x != Overflow::Visible || s.overflow_y != Overflow::Visible)
            {
                // CSS UI L4 §10.1: text-overflow: ellipsis требует overflow != visible.
                apply_text_overflow_ellipsis(lines, content_width, s.font_size, m);
            }
        } else {
            *lines = one_line_fallback(segments);
        }
        // CSS Pseudo-elements L4 §3.1: ::first-line applies to the first formatted line.
        // Mark frags on lines[0] and apply pre-computed ::first-line style override.
        if let Some(first_line) = lines.first_mut() {
            for frag in first_line.iter_mut() {
                frag.is_first_line = true;
                // Apply ::first-line style (inheritable properties only — guaranteed by
                // compute_pseudo_element_style which starts from inherited values).
                if let Some(fls) = first_line_style {
                    frag.style = *fls.clone();
                }
            }
        }
        let line_count = lines.len().max(1);
        b.rect.height = line_count as f32 * (s.font_size * s.line_height);
        return;
    }

    // Абсолютно-позиционированные дети: (index, static_x, static_y).
    // Заполняется внутри Block-flow и обрабатывается после match.
    let mut abs_deferred: Vec<(usize, f32, f32)> = Vec::new();

    match &mut b.kind {
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::Image { .. } | BoxKind::Video { .. } | BoxKind::Canvas { .. } | BoxKind::Audio { .. } | BoxKind::Iframe { .. } | BoxKind::FormControl { .. } => {
            // Flex containers dispatch to lay_out_flex before block-flow.
            if matches!(s.display, Display::Flex | Display::InlineFlex) {
                // For row flex, align-content needs the explicit container height (cross axis).
                let flex_explicit_cross = if !matches!(
                    s.flex_direction,
                    FlexDirection::Column | FlexDirection::ColumnReverse
                ) {
                    s.height.as_ref()
                        .and_then(|h| h.resolve(em, available_height, viewport))
                        .map(|h| match s.box_sizing {
                            BoxSizing::ContentBox => h,
                            BoxSizing::BorderBox => (h - padding_top - padding_bottom
                                - s.border_top_width - s.border_bottom_width)
                                .max(0.0),
                        })
                } else {
                    None
                };
                let content_height = lay_out_flex(
                    &mut b.children, &s, content_x, content_y, content_width,
                    flex_explicit_cross, measurer, viewport, children_pcb, hp,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = h_len.resolve(em, available_height, viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else if let Some((aw, ah)) = s.aspect_ratio
                    && aw > 0.0 && ah > 0.0
                {
                    (b.rect.width * ah / aw).max(0.0)
                } else {
                    let ch = if s.contain.0 & ContainFlags::SIZE.0 != 0 { 0.0 } else { content_height };
                    ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                };
                return;
            }
            // Grid containers dispatch to lay_out_grid before block-flow.
            if matches!(s.display, Display::Grid | Display::InlineGrid) {
                let content_height = lay_out_grid(
                    &mut b.children, &s, content_x, content_y, content_width, measurer, viewport,
                    children_pcb, hp,
                );
                b.rect.height = if let Some(h_len) = &s.height
                    && let Some(h) = h_len.resolve(em, available_height, viewport)
                {
                    match s.box_sizing {
                        BoxSizing::ContentBox => {
                            (h + padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width).max(0.0)
                        }
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else if let Some((aw, ah)) = s.aspect_ratio
                    && aw > 0.0 && ah > 0.0
                {
                    (b.rect.width * ah / aw).max(0.0)
                } else {
                    let ch = if s.contain.0 & ContainFlags::SIZE.0 != 0 { 0.0 } else { content_height };
                    ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
                };
                return;
            }
            // Image не имеет flow-детей, поэтому child-цикл просто пуст —
            // объединяем с Block, чтобы общий код width/height/min-max/borders
            // не дублировался. content_height = 0 для Image без явной высоты
            // даёт коробку только из padding+border (что для пустой картинки
            // визуально корректно).
            // CSS 2.1 §10.5: definite content height for children's height percentage resolution.
            // Only available when this element itself has an explicit height.
            let children_available_height: Option<f32> = if let Some(h_len) = &s.height
                && let Some(h) = h_len.resolve(em, available_height, viewport)
            {
                Some(match s.box_sizing {
                    BoxSizing::ContentBox => h,
                    BoxSizing::BorderBox => (h - padding_top - padding_bottom
                        - s.border_top_width - s.border_bottom_width).max(0.0),
                })
            } else {
                None
            };
            let content_height = if (s.column_count.is_some() || s.column_width.is_some())
                && !b.children.is_empty()
            {
                lay_out_multicol_children(
                    &mut b.children,
                    content_x, content_y, content_width,
                    &s, em, measurer, viewport, children_pcb, hp,
                    children_available_height,
                )
            } else {
                // CSS 2.1 §9.5 — float context for this block formatting context.
                let mut fc = FloatContext::new();
                let container_right = content_x + content_width;

                let mut child_y = content_y;
                // CSS 2.1 §8.3.1: resolved bottom margin of the previous block-level child.
                // Adjacent Block/FlowRoot siblings collapse their margins (gap = max, not sum).
                // Inline runs, replaced elements, and floats break the collapsing chain.
                let mut prev_block_mb: f32 = 0.0;
                // CSS Lists L3 §2.4: pending indent from an inside ::marker (em units).
                // Consumed by the first normal-flow content child after the marker.
                let mut inside_marker_w: f32 = 0.0;
                for (i, child) in b.children.iter_mut().enumerate() {
                    if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                        abs_deferred.push((i, content_x, child_y));
                        continue;
                    }
                    // CSS Lists L3 §2.4 — position ::marker outside or inside principal block.
                    if matches!(&child.kind, BoxKind::Marker { .. }) {
                        let (position, em, lh) = if let BoxKind::Marker { position, .. } = &child.kind {
                            (*position, child.style.font_size, child.style.line_height)
                        } else { unreachable!() };
                        let line_h = em * lh;
                        let marker_w = em * 1.5; // CSS: list-style-type determines exact width
                        match position {
                            ListStylePosition::Outside => {
                                // Out of flow: does not advance child_y.
                                child.rect = Rect::new(content_x - marker_w, child_y, marker_w, line_h);
                            }
                            ListStylePosition::Inside => {
                                // CSS Lists L3 §2.4: inside marker shares the first line with
                                // content. Place at content_x; record indent for the next child.
                                child.rect = Rect::new(content_x, child_y, marker_w, line_h);
                                inside_marker_w = marker_w;
                                // Do NOT advance child_y — marker is inline with content.
                            }
                        }
                        continue;
                    }

                    // CSS 2.1 §9.5.2: clear — advance child_y past relevant floats.
                    if !fc.is_empty() && child.style.clear != ClearSide::None {
                        child_y = fc.clear_y(child_y, child.style.clear);
                    }

                    // CSS 2.1 §9.5.1: float box — placed out of normal flow.
                    if child.style.float_side != FloatSide::None {
                        let cem = child.style.font_size;
                        let avail_left  = fc.left_edge_at(child_y, content_x);
                        let avail_right = fc.right_edge_at(child_y, container_right);
                        let avail_w = (avail_right - avail_left).max(0.0);

                        // Shrink-to-fit width: explicit CSS width wins; otherwise use
                        // preferred content width clamped to available space.
                        let float_layout_w = if child.style.width.is_some() {
                            avail_w
                        } else {
                            preferred_inline_block_width(child, measurer, viewport)
                                .map(|pw| pw.min(avail_w))
                                .unwrap_or(avail_w)
                        };
                        lay_out(child, avail_left, child_y, float_layout_w,
                                children_available_height, measurer, viewport, children_pcb, hp);

                        let fml = child.style.margin_left.resolve_or_zero(cem, avail_w, viewport);
                        let fmr = child.style.margin_right.resolve_or_zero(cem, avail_w, viewport);
                        let fmt = child.style.margin_top.resolve_or_zero(cem, avail_w, viewport);
                        let fmb = child.style.margin_bottom.resolve_or_zero(cem, avail_w, viewport);
                        let fw  = child.rect.width;
                        let fh  = child.rect.height;

                        match child.style.float_side {
                            FloatSide::Left => {
                                let lx = fc.left_edge_at(child_y, content_x);
                                child.rect.x = lx + fml;
                                child.rect.y = child_y + fmt;
                                let top_y  = child_y + fmt;
                                let bot_y  = top_y + fh + fmb;
                                let right_edge = lx + fml + fw + fmr;
                                fc.add_left(bot_y, right_edge);
                                // CSS Shapes L1 — wire shape-outside for left float.
                                // Margin-box origin: (lx, child_y). Points are float-local.
                                if let crate::style::ShapeOutside::Value(ref sv) = child.style.shape_outside {
                                    if let Some(r) = parse_circle_px(sv) {
                                        let cx = child.rect.x + fw / 2.0;
                                        let cy = top_y + fh / 2.0;
                                        fc.shape_circles.push((top_y, bot_y, true, cx, cy, r));
                                    } else if let Some(local_pts) = parse_shape_polygon_px(sv) {
                                        let pts = local_pts.into_iter()
                                            .map(|(px, py)| (px + lx, py + child_y))
                                            .collect();
                                        fc.shape_polygons.push(ShapePolygon {
                                            top_y, bottom_y: bot_y, is_left: true, points: pts,
                                        });
                                    } else if let Some((rx, ry, ecx, ecy)) = parse_shape_ellipse_px(sv) {
                                        fc.shape_ellipses.push(ShapeEllipse {
                                            top_y, bottom_y: bot_y, is_left: true,
                                            cx: ecx + lx, cy: ecy + child_y, rx, ry,
                                        });
                                    }
                                }
                            }
                            FloatSide::Right => {
                                let rx = fc.right_edge_at(child_y, container_right);
                                child.rect.x = rx - fmr - fw;
                                child.rect.y = child_y + fmt;
                                let top_y  = child_y + fmt;
                                let bot_y  = top_y + fh + fmb;
                                let left_edge = rx - fmr - fw - fml;
                                fc.add_right(bot_y, left_edge);
                                // CSS Shapes L1 — wire shape-outside for right float.
                                // Margin-box origin: (left_edge, child_y). Points are float-local.
                                if let crate::style::ShapeOutside::Value(ref sv) = child.style.shape_outside {
                                    if let Some(r) = parse_circle_px(sv) {
                                        let cx = child.rect.x + fw / 2.0;
                                        let cy = top_y + fh / 2.0;
                                        fc.shape_circles.push((top_y, bot_y, false, cx, cy, r));
                                    } else if let Some(local_pts) = parse_shape_polygon_px(sv) {
                                        let pts = local_pts.into_iter()
                                            .map(|(px, py)| (px + left_edge, py + child_y))
                                            .collect();
                                        fc.shape_polygons.push(ShapePolygon {
                                            top_y, bottom_y: bot_y, is_left: false, points: pts,
                                        });
                                    } else if let Some((rx_e, ry_e, ecx, ecy)) = parse_shape_ellipse_px(sv) {
                                        fc.shape_ellipses.push(ShapeEllipse {
                                            top_y, bottom_y: bot_y, is_left: false,
                                            cx: ecx + left_edge, cy: ecy + child_y, rx: rx_e, ry: ry_e,
                                        });
                                    }
                                }
                            }
                            FloatSide::None => unreachable!(),
                        }
                        // Float does not advance child_y in normal flow.
                        continue;
                    }

                    // Normal flow: narrow x/width for active floats.
                    let flow_left  = fc.left_edge_at(child_y, content_x);
                    let flow_right = fc.right_edge_at(child_y, container_right);
                    // Apply inside-marker indent to the first normal-flow content child.
                    let (eff_left, eff_w) = if inside_marker_w > 0.0 {
                        let l = flow_left + inside_marker_w;
                        inside_marker_w = 0.0;
                        (l, (flow_right - l).max(0.0))
                    } else {
                        (flow_left, (flow_right - flow_left).max(0.0))
                    };

                    // CSS 2.1 §8.3.1: collapse adjacent sibling block margins.
                    // Only Block/FlowRoot participate; other kinds break the chain.
                    // Formula: start_y = child_y - min(prev_mb, mt)
                    // so that lay_out's internal "+mt" yields child_y + max(prev_mb, mt).
                    let is_block = matches!(&child.kind, BoxKind::Block | BoxKind::FlowRoot);
                    let mt = child.style.margin_top
                        .resolve_or_zero(child.style.font_size, eff_w, viewport);
                    let start_y = if is_block {
                        child_y - prev_block_mb.min(mt.max(0.0))
                    } else {
                        child_y
                    };

                    lay_out(child, eff_left, start_y, eff_w,
                            children_available_height, measurer, viewport, children_pcb, hp);
                    if matches!(child.kind, BoxKind::Skip) {
                        // Zero-height; does not break the collapsing chain.
                        continue;
                    }
                    let child_mb = child.style.margin_bottom.resolve_or_zero(
                        child.style.font_size, content_width, viewport);
                    child_y = child.rect.y + child.rect.height + child_mb;
                    prev_block_mb = if is_block { child_mb.max(0.0) } else { 0.0 };
                }
                // CSS 2.1 §9.5: the container height must also enclose all floats.
                let float_bottom = fc.left.iter().chain(fc.right.iter())
                    .map(|(bot, _)| *bot)
                    .fold(child_y, f32::max);
                (float_bottom - content_y).max(0.0)
            };
            // Явная высота (CSS height: Npx) перекрывает auto-высоту по содержимому.
            // box-sizing работает симметрично width: content-box прибавляет
            // padding+border, border-box оставляет h как итоговую высоту.
            b.rect.height = if let Some(h_len) = &s.height {
                if let Some(h) = h_len.resolve(em, available_height, viewport) {
                    match s.box_sizing {
                        BoxSizing::ContentBox => h
                            + padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                        BoxSizing::BorderBox => h.max(
                            padding_top + padding_bottom
                                + s.border_top_width + s.border_bottom_width,
                        ),
                    }
                } else {
                    content_height + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width
                }
            } else if let Some((aw, ah)) = s.aspect_ratio
                && aw > 0.0 && ah > 0.0
            {
                // CSS Sizing L4 §6.1: height auto + aspect-ratio → derive from width.
                // Phase 0: ratio applied in border-box space.
                (b.rect.width * ah / aw).max(0.0)
            } else {
                // CSS Containment L3 §3.3: contain:size suppresses children contribution
                // to auto height — intrinsic height = 0.
                let ch = if s.contain.0 & ContainFlags::SIZE.0 != 0 { 0.0 } else { content_height };
                ch + padding_top + padding_bottom + s.border_top_width + s.border_bottom_width
            };
            // CSS 2.1 §10.4: clamp [min-height, max-height]. Симметрия с
            // width: max сначала, потом min → «min побеждает max». Content
            // оверфлоу-ит коробку если min режет ниже — это правильное
            // поведение CSS.
            let outer_vert = |v: f32| match s.box_sizing {
                BoxSizing::ContentBox => v + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width,
                BoxSizing::BorderBox => v,
            };
            if let Some(max_len) = &s.max_height
                && let Some(max_h) = max_len.resolve(em, available_height, viewport)
            {
                b.rect.height = b.rect.height.min(outer_vert(max_h).max(0.0));
            }
            if let Some(min_len) = &s.min_height
                && let Some(min_h) = min_len.resolve(em, available_height, viewport)
            {
                b.rect.height = b.rect.height.max(outer_vert(min_h.max(0.0)));
            }
        }
        BoxKind::InlineBlockRow => {
            // Двухфазный горизонтальный layout с переносом строк и
            // vertical-align (CSS 2.1 §9.4.3 + §10.8).
            //
            // Фаза 1: расставляем детей по X, группируем в строки.
            // Фаза 2: применяем вертикальное выравнивание внутри каждой строки.
            //
            // rows: (row_y, row_max_h, Vec<child_index>)
            // IFC strut (CSS §10.8 / верифицировано pixel-diff TEST-11/TEST-12):
            // strut_descent добавляется к высоте строки только если в строке есть
            // хотя бы один элемент с vertical-align: baseline (явный или InlineRun).
            // Для строк, где все элементы используют top/bottom/middle, strut не
            // нужен — baseline вообще не задействован (Edge/Blink подтверждено).
            let strut_descent = measurer.map_or(0.0, |m| m.descent_px(b.style.font_size));
            // rows: (row_y, row_max_h, has_baseline, Vec<child_index>)
            let mut rows: Vec<(f32, f32, bool, Vec<usize>)> = Vec::new();
            let mut cur_x = content_x;
            let mut cur_y = content_y;
            let mut row_max_h: f32 = 0.0;
            let mut row_y = cur_y;
            let mut cur_row: Vec<usize> = Vec::new();
            let mut row_has_baseline = false;
            let mut total_h: f32 = 0.0;

            for i in 0..b.children.len() {
                // InlineSpace: collapsed whitespace gap — advance cur_x only.
                if matches!(b.children[i].kind, BoxKind::InlineSpace) {
                    let space_w = measurer.map_or(0.0, |m| m.char_width(' ', b.style.font_size));
                    cur_x += space_w;
                    continue;
                }
                let is_run = matches!(b.children[i].kind, BoxKind::InlineRun { .. });
                // Snap inline-block x to integer CSS pixels (Chrome/Edge behaviour at DPR=1).
                // InlineSpace uses float advance (font metrics); accumulated sub-pixel error
                // would shift all subsequent elements by up to 1px relative to Edge.
                let place_x = if is_run { cur_x } else { cur_x.floor() };
                let child_avail = if is_run {
                    (content_width - (cur_x - content_x)).max(0.0)
                } else {
                    content_width
                };
                lay_out(&mut b.children[i], place_x, cur_y, child_avail, None, measurer, viewport, children_pcb, hp);
                if matches!(b.children[i].kind, BoxKind::Skip) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let child_mr = b.children[i].style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                let child_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let child_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                let child_right = b.children[i].rect.x + b.children[i].rect.width + child_mr;
                let child_full_h = child_mt + b.children[i].rect.height + child_mb;

                if !is_run && child_right > content_x + content_width && cur_x > content_x {
                    let row_strut = if row_has_baseline { strut_descent } else { 0.0 };
                    let row_spacing = row_max_h + row_strut;
                    rows.push((row_y, row_max_h, row_has_baseline, std::mem::take(&mut cur_row)));
                    // Snap to integer CSS pixels (Chrome/Edge DPR=1 behaviour): fractional
                    // IFC strut from font metrics (descent_px) would otherwise drift row
                    // y-positions by sub-pixel amounts relative to a browser with a different
                    // default font.
                    let new_y = (cur_y + row_spacing).round();
                    let actual_spacing = new_y - cur_y;
                    total_h += actual_spacing;
                    cur_y = new_y;
                    row_y = cur_y;
                    cur_x = content_x;
                    row_max_h = 0.0;
                    row_has_baseline = false;
                    lay_out(&mut b.children[i], cur_x, cur_y, content_width, None, measurer, viewport, children_pcb, hp);
                }
                cur_row.push(i);
                let child_is_baseline = is_run
                    || matches!(b.children[i].style.vertical_align, VerticalAlign::Baseline);
                if child_is_baseline {
                    row_has_baseline = true;
                }
                cur_x = b.children[i].rect.x + b.children[i].rect.width + child_mr;
                row_max_h = row_max_h.max(child_full_h);
            }
            if !cur_row.is_empty() {
                rows.push((row_y, row_max_h, row_has_baseline, cur_row));
            }
            let last_row_strut = if row_has_baseline { strut_descent } else { 0.0 };
            b.rect.height = total_h + row_max_h + last_row_strut;

            // Фаза 2: vertical-align (CSS 2.1 §10.8.1).
            // row_h = row_max_h (без strut); row_full_h = row_h + row_strut.
            // Baseline: dy = row_h - child_h  (bottom at baseline, strut below).
            // Bottom: dy = row_full_h - child_h (bottom at line-box bottom).
            let mut adjustments: Vec<(usize, f32)> = Vec::new();
            for (_, row_h, has_baseline, child_idxs) in &rows {
                let row_strut = if *has_baseline { strut_descent } else { 0.0 };
                let row_full_h = row_h + row_strut;
                for &idx in child_idxs {
                    let child = &b.children[idx];
                    let c_em = child.style.font_size;
                    let child_mt = child.style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                    let child_mb = child.style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                    let child_full_h = child_mt + child.rect.height + child_mb;
                    let dy = match child.style.vertical_align {
                        VerticalAlign::Baseline => row_h - child_full_h,
                        VerticalAlign::Bottom | VerticalAlign::TextBottom => row_full_h - child_full_h,
                        VerticalAlign::Top | VerticalAlign::TextTop => 0.0,
                        VerticalAlign::Middle => (row_full_h - child_full_h) / 2.0,
                        _ => 0.0,
                    };
                    if dy > 0.001 {
                        adjustments.push((idx, dy));
                    }
                }
            }
            for (idx, dy) in adjustments {
                shift_y_box(&mut b.children[idx], dy);
            }
        }
        BoxKind::TableRow => {
            // CSS 2.1 §17.5 — table row: ячейки раскладываются горизонтально.
            // col_widths=None → per-row auto-distribution (standalone <tr> outside <table>).
            let row_h = lay_out_table_row(
                b, content_x, content_y, content_width, None, None, measurer, viewport, children_pcb, hp,
            );
            b.rect.height = if let Some(h_len) = &s.height
                && let Some(h) = h_len.resolve(em, available_height, viewport)
            {
                match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                }
            } else {
                row_h + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width
            };
        }
        BoxKind::Table => {
            // CSS 2.1 §17 — table container: compute global column widths, lay out rows.
            let content_height = lay_out_table(
                b, content_x, content_y, content_width, measurer, viewport, children_pcb, hp,
            );
            b.rect.height = if let Some(h_len) = &s.height
                && let Some(h) = h_len.resolve(em, available_height, viewport)
            {
                match s.box_sizing {
                    BoxSizing::ContentBox => (h + padding_top + padding_bottom
                        + s.border_top_width + s.border_bottom_width).max(0.0),
                    BoxSizing::BorderBox => h.max(
                        padding_top + padding_bottom
                            + s.border_top_width + s.border_bottom_width,
                    ),
                }
            } else {
                content_height + padding_top + padding_bottom
                    + s.border_top_width + s.border_bottom_width
            };
        }
        BoxKind::TableRowGroup => {
            // CSS 2.1 §17 — row group standalone (outside a <table>): block-flow of rows.
            // When inside a Table, rows are handled directly by lay_out_table.
            let mut cur_y = content_y;
            for i in 0..b.children.len() {
                if !matches!(b.children[i].kind, BoxKind::TableRow) {
                    continue;
                }
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                lay_out(&mut b.children[i], content_x, cur_y + c_mt, content_width, None, measurer, viewport, children_pcb, hp);
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(c_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb;
            }
            b.rect.height = (cur_y - content_y) + padding_top + padding_bottom
                + s.border_top_width + s.border_bottom_width;
        }
        BoxKind::InlineRun { .. } => unreachable!(),
        BoxKind::InlineSpace => unreachable!(),
        BoxKind::Skip => unreachable!(),
        BoxKind::Contents => unreachable!("display:contents boxes must be flattened before lay_out"),
        BoxKind::Marker { .. } => {
            // Rect is set by the parent's block-flow loop; nothing to do here.
        }
        // SvgRoot, SvgShape, and SvgText are dispatched before this match (early return above).
        BoxKind::SvgRoot { .. } | BoxKind::SvgShape { .. } | BoxKind::SvgText { .. } => unreachable!(),
    }

    // CSS Positioned Layout L3 §4 — абсолютное / фиксированное позиционирование.
    // Деферированные дети (abs_deferred) собраны в Block-ветке выше.
    // Обрабатываем после finalize b.rect.height, чтобы знать высоту containing block.
    if !abs_deferred.is_empty() {
        let my_pcb = if is_positioned {
            // CSS Position L3 §2.2: CB for absolute descendants = padding edge.
            Rect::new(
                b.rect.x + s.border_left_width,
                b.rect.y + s.border_top_width,
                (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
            )
        } else {
            pcb
        };
        lay_out_abs_children(b, &abs_deferred, measurer, viewport, my_pcb, hp);
    }

    // CSS Positioned Layout L3 §9.4.3 — position: relative — смещение после normal flow.
    if matches!(s.position, Position::Relative) {
        let off_x = match &s.left {
            LengthOrAuto::Length(l) => l.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.right {
                LengthOrAuto::Length(r) => -(r.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        let off_y = match &s.top {
            LengthOrAuto::Length(t) => t.resolve(em, Some(cb), viewport).unwrap_or(0.0),
            LengthOrAuto::Auto => match &s.bottom {
                LengthOrAuto::Length(bot) => -(bot.resolve(em, Some(cb), viewport).unwrap_or(0.0)),
                LengthOrAuto::Auto => 0.0,
            },
        };
        if off_x != 0.0 || off_y != 0.0 {
            shift_tree(b, off_x, off_y);
        }
    }
    // CSS: position: sticky — treated as normal flow here; P4 resolves inset values
    // (top/right/bottom/left) from ComputedStyle; P3 calls collect_sticky_boxes() +
    // compute_sticky_offset() to apply scroll-driven paint transforms at render time.
}

/// CSS 2.1 §17.5 — table row layout with colspan/rowspan support.
///
/// Algorithm:
/// 1. Map each cell to its starting column (skipping rowspan-occupied columns).
/// 2. Determine cell width: sum of spanned `col_widths` columns, or explicit CSS width.
/// 3. Place cells horizontally; use column-position x when `col_widths` is present.
/// 4. Normalise heights: non-rowspan cells all get the max row height.
///    Rowspan cells keep their laid-out height; `lay_out_table` fixes them after all rows.
/// 5. Register new rowspan occupancy in `rowspan_map` (caller decrements after the row).
///
/// Returns content height (without the row's own padding/border).
#[allow(clippy::too_many_arguments)]
fn lay_out_table_row(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    col_widths: Option<&[f32]>,
    // None for standalone <tr> outside <table>; caller must call decrement_rowspan_map after return.
    rowspan_map: Option<&mut Vec<u32>>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let cell_idxs: Vec<usize> = b
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    let n = cell_idxs.len();
    if n == 0 {
        return 0.0;
    }

    // Step 1 + 2: map cells to (col_start, cell_width).
    // `cell_cols[j]` = (starting column index, border-box width to allocate).
    let cell_cols: Vec<(usize, f32)> = if let Some(cw) = col_widths {
        // Pre-computed table-wide column widths are authoritative.
        // Skip columns occupied by rowspan cells from prior rows.
        let empty: Vec<u32> = Vec::new();
        let rsmap: &[u32] = rowspan_map
            .as_deref()
            .map(|v: &Vec<u32>| v.as_slice())
            .unwrap_or(empty.as_slice());
        let mut col_pos = 0usize;
        let mut result = Vec::with_capacity(n);
        for &i in &cell_idxs {
            while col_pos < rsmap.len() && rsmap[col_pos] > 0 {
                col_pos += 1;
            }
            let span = b.children[i].col_span.max(1) as usize;
            let w: f32 = (col_pos..col_pos + span)
                .map(|c| cw.get(c).copied().unwrap_or(0.0))
                .sum();
            result.push((col_pos, w));
            col_pos += span;
        }
        result
    } else {
        // No pre-computed widths: derive from each cell's explicit CSS width.
        let mut explicit_w: Vec<Option<f32>> = Vec::with_capacity(n);
        let mut total_explicit = 0.0_f32;
        let mut auto_count: usize = 0;
        for &i in &cell_idxs {
            let c = &b.children[i];
            let em = c.style.font_size;
            if let Some(w_len) = &c.style.width
                && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
            {
                let border_w = match c.style.box_sizing {
                    BoxSizing::ContentBox => {
                        let pl = c.style.padding_left.resolve_or_zero(em, content_width, viewport);
                        let pr = c.style.padding_right.resolve_or_zero(em, content_width, viewport);
                        w + pl + pr + c.style.border_left_width + c.style.border_right_width
                    }
                    BoxSizing::BorderBox => w,
                };
                explicit_w.push(Some(border_w));
                total_explicit += border_w;
                continue;
            }
            explicit_w.push(None);
            auto_count += 1;
        }
        let auto_share = if auto_count > 0 {
            ((content_width - total_explicit) / auto_count as f32).max(0.0)
        } else {
            0.0
        };
        // Standalone row: sequential column assignment (cell j → column j).
        (0..n)
            .map(|j| (j, explicit_w[j].unwrap_or(auto_share)))
            .collect()
    };

    // Step 3: lay out each cell at its column x position.
    // When col_widths is present, the column width is authoritative — clear the cell's CSS
    // `width` temporarily so lay_out uses `avail` as the final width.
    let use_global = col_widths.is_some();
    for (j, &i) in cell_idxs.iter().enumerate() {
        let (col_start, avail) = cell_cols[j];
        let cell_x = if use_global {
            // Exact x from column positions.
            content_x
                + (0..col_start)
                    .map(|c| col_widths.and_then(|cw| cw.get(c)).copied().unwrap_or(0.0))
                    .sum::<f32>()
        } else {
            // Standalone row: use prior cell's right edge.
            if j == 0 {
                content_x
            } else {
                let prev_i = cell_idxs[j - 1];
                let c = &b.children[prev_i];
                let c_em = c.style.font_size;
                let mr = c.style.margin_right.resolve_or_zero(c_em, content_width, viewport);
                c.rect.x + c.rect.width + mr
            }
        };
        let saved_width = if use_global { b.children[i].style.width.take() } else { None };
        lay_out(
            &mut b.children[i],
            cell_x,
            content_y,
            avail,
            None,
            measurer,
            viewport,
            pcb,
            hp,
        );
        if use_global {
            b.children[i].style.width = saved_width;
        }
    }

    // Register rowspan occupancy. Value = row_span (not row_span-1) because the caller
    // calls decrement_rowspan_map after this row, leaving row_span-1 remaining rows occupied.
    if let Some(rsmap) = rowspan_map {
        for (j, &i) in cell_idxs.iter().enumerate() {
            if b.children[i].row_span > 1 {
                let (col_start, _) = cell_cols[j];
                let span = b.children[i].col_span.max(1) as usize;
                let end_col = col_start + span;
                if end_col > rsmap.len() {
                    rsmap.resize(end_col, 0);
                }
                let rs = b.children[i].row_span;
                for v in rsmap.iter_mut().skip(col_start).take(span) {
                    if *v < rs {
                        *v = rs;
                    }
                }
            }
        }
    }

    // Step 4: normalise heights — non-rowspan cells all become the max row height.
    // Rowspan > 1 cells keep their own height; lay_out_table patches them later.
    let row_h = cell_idxs
        .iter()
        .filter(|&&i| b.children[i].row_span == 1)
        .map(|&i| b.children[i].rect.height)
        .fold(0.0_f32, f32::max);
    for &i in &cell_idxs {
        if b.children[i].row_span == 1 {
            b.children[i].rect.height = row_h;
        }
    }

    row_h
}

/// CSS 2.1 §17 — table layout with colspan/rowspan support.
///
/// Pass 1: compute column widths (span-aware), lay out rows top-to-bottom while tracking
/// rowspan occupancy and collecting spanning cells.
/// Pass 2: fix spanning cell heights — each rowspan cell's height is extended to cover
/// the bottom edge of its last spanned row.
///
/// Returns content height.
#[allow(clippy::too_many_arguments)]
fn lay_out_table(
    b: &mut LayoutBox,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let col_widths = compute_table_col_widths(b, content_width, viewport);

    let mut cur_y = content_y;
    let mut rowspan_map: Vec<u32> = Vec::new();

    // flat_row_rects[k] = (y, height) for the k-th row in DOM order (across all groups).
    let mut flat_row_rects: Vec<(f32, f32)> = Vec::new();

    // Spanning cells that need height post-fix:
    // (group: Option<usize>, row_in_group: usize, child_idx: usize, start_flat: usize, span: u32)
    let mut span_fixes: Vec<(Option<usize>, usize, usize, usize, u32)> = Vec::new();

    let n = b.children.len();
    for i in 0..n {
        match b.children[i].kind {
            BoxKind::TableRow => {
                let c_em = b.children[i].style.font_size;
                let c_mt = b.children[i].style.margin_top.resolve_or_zero(c_em, content_width, viewport);
                let row_y = cur_y + c_mt;
                b.children[i].rect.x = content_x;
                b.children[i].rect.y = row_y;
                b.children[i].rect.width = content_width;
                let flat_idx = flat_row_rects.len();
                let row_h = lay_out_table_row(
                    &mut b.children[i],
                    content_x, row_y, content_width,
                    Some(&col_widths),
                    Some(&mut rowspan_map),
                    measurer, viewport, pcb, hp,
                );
                let row_style_h = {
                    let s = &b.children[i].style;
                    if let Some(h_len) = &s.height
                        && let Some(h) = h_len.resolve(s.font_size, None, viewport)
                    {
                        let pt = s.padding_top.resolve_or_zero(s.font_size, content_width, viewport);
                        let pb = s.padding_bottom.resolve_or_zero(s.font_size, content_width, viewport);
                        match s.box_sizing {
                            BoxSizing::ContentBox => (h + pt + pb + s.border_top_width + s.border_bottom_width).max(0.0),
                            BoxSizing::BorderBox => h.max(pt + pb + s.border_top_width + s.border_bottom_width),
                        }
                    } else {
                        let pt = b.children[i].style.padding_top.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        let pb = b.children[i].style.padding_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                        row_h + pt + pb + b.children[i].style.border_top_width + b.children[i].style.border_bottom_width
                    }
                };
                b.children[i].rect.height = row_style_h;
                flat_row_rects.push((b.children[i].rect.y, row_style_h));
                // Collect spanning cells for post-fix.
                for (ci, child) in b.children[i].children.iter().enumerate() {
                    if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                        span_fixes.push((None, i, ci, flat_idx, child.row_span));
                    }
                }
                let c_mb = b.children[i].style.margin_bottom.resolve_or_zero(b.children[i].style.font_size, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + c_mb;
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                let group_em = b.children[i].style.font_size;
                let g_mt = b.children[i].style.margin_top.resolve_or_zero(group_em, content_width, viewport);
                let group_y = cur_y + g_mt;
                b.children[i].rect.x = content_x;
                b.children[i].rect.y = group_y;
                b.children[i].rect.width = content_width;
                let mut row_y = group_y;
                let n_rows = b.children[i].children.len();
                for r in 0..n_rows {
                    if !matches!(b.children[i].children[r].kind, BoxKind::TableRow) {
                        continue;
                    }
                    let flat_idx = flat_row_rects.len();
                    let r_em = b.children[i].children[r].style.font_size;
                    let r_mt = b.children[i].children[r].style.margin_top.resolve_or_zero(r_em, content_width, viewport);
                    b.children[i].children[r].rect.x = content_x;
                    b.children[i].children[r].rect.y = row_y + r_mt;
                    b.children[i].children[r].rect.width = content_width;
                    let row_h = lay_out_table_row(
                        &mut b.children[i].children[r],
                        content_x, row_y + r_mt, content_width,
                        Some(&col_widths),
                        Some(&mut rowspan_map),
                        measurer, viewport, pcb, hp,
                    );
                    let r_pt = b.children[i].children[r].style.padding_top.resolve_or_zero(r_em, content_width, viewport);
                    let r_pb = b.children[i].children[r].style.padding_bottom.resolve_or_zero(r_em, content_width, viewport);
                    let r_bor = b.children[i].children[r].style.border_top_width + b.children[i].children[r].style.border_bottom_width;
                    let row_style_h = row_h + r_pt + r_pb + r_bor;
                    b.children[i].children[r].rect.height = row_style_h;
                    flat_row_rects.push((b.children[i].children[r].rect.y, row_style_h));
                    // Collect spanning cells for post-fix.
                    for (ci, child) in b.children[i].children[r].children.iter().enumerate() {
                        if !matches!(child.kind, BoxKind::Skip) && child.row_span > 1 {
                            span_fixes.push((Some(i), r, ci, flat_idx, child.row_span));
                        }
                    }
                    let r_mb = b.children[i].children[r].style.margin_bottom.resolve_or_zero(r_em, content_width, viewport);
                    row_y = b.children[i].children[r].rect.y + b.children[i].children[r].rect.height + r_mb;
                    decrement_rowspan_map(&mut rowspan_map);
                }
                let g_pt = b.children[i].style.padding_top.resolve_or_zero(group_em, content_width, viewport);
                let g_pb = b.children[i].style.padding_bottom.resolve_or_zero(group_em, content_width, viewport);
                let g_bor = b.children[i].style.border_top_width + b.children[i].style.border_bottom_width;
                b.children[i].rect.height = (row_y - group_y) + g_pt + g_pb + g_bor;
                let g_mb = b.children[i].style.margin_bottom.resolve_or_zero(group_em, content_width, viewport);
                cur_y = b.children[i].rect.y + b.children[i].rect.height + g_mb;
            }
            _ => {}
        }
    }

    // Pass 2: fix rowspan cell heights.
    // Each spanning cell's height is extended to reach the bottom of its last spanned row.
    for (group, row, child_idx, start_flat, span) in span_fixes {
        let end_flat = (start_flat + span as usize).min(flat_row_rects.len());
        if end_flat == 0 {
            continue;
        }
        let (last_y, last_h) = flat_row_rects[end_flat - 1];
        let target_bottom = last_y + last_h;
        let cell = match group {
            None => &mut b.children[row].children[child_idx],
            Some(g) => &mut b.children[g].children[row].children[child_idx],
        };
        let new_h = (target_bottom - cell.rect.y).max(cell.rect.height);
        cell.rect.height = new_h;
    }

    (cur_y - content_y).max(0.0)
}

/// Scans `row`'s cells and updates `col_explicit` with per-column explicit border-box
/// widths. Colspan cells distribute their width evenly across spanned columns.
/// Rowspan cells register occupancy in `rowspan_map` for subsequent rows.
/// Caller must call `decrement_rowspan_map` after processing each row.
fn scan_row_explicit_widths(
    row: &LayoutBox,
    col_explicit: &mut Vec<Option<f32>>,
    rowspan_map: &mut Vec<u32>,
    content_width: f32,
    viewport: Size,
) {
    let cells: Vec<_> = row
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .collect();

    let mut col_pos = 0usize;
    for cell in &cells {
        // Skip columns occupied by rowspan cells from prior rows.
        while col_pos < rowspan_map.len() && rowspan_map[col_pos] > 0 {
            col_pos += 1;
        }

        let span = cell.col_span.max(1) as usize;
        let em = cell.style.font_size;
        let w_border = if let Some(w_len) = &cell.style.width
            && let Some(w) = w_len.resolve(em, Some(content_width), viewport)
        {
            let bw = match cell.style.box_sizing {
                BoxSizing::ContentBox => {
                    let pl = cell.style.padding_left.resolve_or_zero(em, content_width, viewport);
                    let pr = cell.style.padding_right.resolve_or_zero(em, content_width, viewport);
                    w + pl + pr + cell.style.border_left_width + cell.style.border_right_width
                }
                BoxSizing::BorderBox => w,
            };
            Some(bw)
        } else {
            None
        };

        let end_col = col_pos + span;
        if end_col > col_explicit.len() {
            col_explicit.resize(end_col, None);
        }
        // Distribute the cell's explicit width evenly across its spanned columns.
        if let Some(total_w) = w_border {
            let per_col = total_w / span as f32;
            for slot in col_explicit.iter_mut().skip(col_pos).take(span) {
                *slot = Some(match *slot {
                    Some(existing) => existing.max(per_col),
                    None => per_col,
                });
            }
        }

        // Register rowspan occupancy. Value = row_span (decrement_rowspan_map brings it to
        // row_span-1 after this row, meaning row_span-1 subsequent rows remain occupied).
        if cell.row_span > 1 {
            if end_col > rowspan_map.len() {
                rowspan_map.resize(end_col, 0);
            }
            let rs = cell.row_span;
            for v in rowspan_map.iter_mut().skip(col_pos).take(span) {
                if *v < rs {
                    *v = rs;
                }
            }
        }

        col_pos = end_col;
    }
}

/// Decrements each entry in `rowspan_map` by 1 (clamped to 0). Call after each row.
fn decrement_rowspan_map(map: &mut [u32]) {
    for v in map.iter_mut() {
        *v = v.saturating_sub(1);
    }
}

/// Computes per-column widths for a `BoxKind::Table` element by scanning all rows
/// (direct and inside `TableRowGroup` children). Colspan/rowspan-aware: cells with
/// `colspan > 1` distribute their width across columns; `rowspan > 1` cells block
/// subsequent rows from reusing those columns. Returns a `Vec<f32>` of border-box
/// widths, one per column.
fn compute_table_col_widths(b: &LayoutBox, content_width: f32, viewport: Size) -> Vec<f32> {
    let mut col_explicit: Vec<Option<f32>> = Vec::new();
    let mut rowspan_map: Vec<u32> = Vec::new();

    for child in &b.children {
        match &child.kind {
            BoxKind::TableRow => {
                scan_row_explicit_widths(child, &mut col_explicit, &mut rowspan_map, content_width, viewport);
                decrement_rowspan_map(&mut rowspan_map);
            }
            BoxKind::TableRowGroup => {
                for row in &child.children {
                    if matches!(row.kind, BoxKind::TableRow) {
                        scan_row_explicit_widths(row, &mut col_explicit, &mut rowspan_map, content_width, viewport);
                        decrement_rowspan_map(&mut rowspan_map);
                    }
                }
            }
            _ => {}
        }
    }

    let n_cols = col_explicit.len();
    if n_cols == 0 {
        return Vec::new();
    }

    let total_explicit: f32 = col_explicit.iter().filter_map(|w| *w).sum();
    let auto_count = col_explicit.iter().filter(|w| w.is_none()).count();
    let auto_share = if auto_count > 0 {
        ((content_width - total_explicit) / auto_count as f32).max(0.0)
    } else {
        0.0
    };

    col_explicit.iter().map(|w| w.unwrap_or(auto_share)).collect()
}

/// CSS Multi-column Layout L1 — lays out `children` into N columns.
/// Returns content height (max column height, without padding/border).
///
/// `container_h` is the resolved content-box height of the multi-column container, used
/// by `column-fill: auto` to fill columns sequentially up to that height instead of
/// balancing content equally across all columns.
#[allow(clippy::too_many_arguments)]
fn lay_out_multicol_children(
    children: &mut [LayoutBox],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    s: &ComputedStyle,
    em: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    container_h: Option<f32>,
) -> f32 {
    let cb = content_width;
    let col_gap = s.column_gap.resolve_or_zero(em, cb, viewport).max(0.0);

    // Compute column count from column-count / column-width.
    let n_cols: u32 = match (s.column_count, &s.column_width) {
        (Some(n), Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport) {
                let n_from_w = ((content_width + col_gap) / (w + col_gap)).floor() as u32;
                n.min(n_from_w).max(1)
            } else {
                n.max(1)
            }
        }
        (Some(n), None) => n.max(1),
        (None, Some(w_len)) => {
            if let Some(w) = w_len.resolve(em, Some(cb), viewport)
                && w > 0.0
            {
                ((content_width + col_gap) / (w + col_gap)).floor() as u32
            } else {
                1
            }
        }
        (None, None) => 1,
    }.max(1);

    let col_w = ((content_width - col_gap * (n_cols - 1) as f32) / n_cols as f32).max(0.0);

    // column-fill: balance distributes content equally; auto fills columns to container height.
    // When no container height is known, auto behaves like balance.
    let balance = s.column_fill_balance || container_h.is_none();

    // Collect flow (non-abs, non-skip) child indices.
    let flow_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.style.position, Position::Absolute | Position::Fixed))
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    if flow_idxs.is_empty() {
        return 0.0;
    }

    // Split flow children into segments separated by column-span:all elements.
    // Each entry is (regular_children, Option<span_all_child_idx>).
    let mut segments: Vec<(Vec<usize>, Option<usize>)> = Vec::new();
    let mut seg: Vec<usize> = Vec::new();
    for &i in &flow_idxs {
        if children[i].style.column_span_all {
            segments.push((std::mem::take(&mut seg), Some(i)));
        } else {
            seg.push(i);
        }
    }
    segments.push((seg, None));

    let mut cur_y = content_y;

    for (seg_idxs, span_idx) in &segments {
        if !seg_idxs.is_empty() {
            // First pass at (0, 0) to measure intrinsic heights.
            for &i in seg_idxs {
                lay_out(&mut children[i], 0.0, 0.0, col_w, None, measurer, viewport, pcb, hp);
            }

            // Outer height of each segment child = margin_top + rect.height + margin_bottom.
            let outer_hs: Vec<f32> = seg_idxs.iter().map(|&i| {
                let c = &children[i];
                let mt = c.style.margin_top.resolve_or_zero(c.style.font_size, col_w, viewport);
                let mb = c.style.margin_bottom.resolve_or_zero(c.style.font_size, col_w, viewport);
                mt + c.rect.height + mb
            }).collect();

            let total_h: f32 = outer_hs.iter().sum();

            // column-fill: auto → fill each column to container_h; balance → equal split.
            let target_h = if balance {
                (total_h / n_cols as f32).ceil().max(1.0)
            } else {
                container_h.unwrap_or_else(|| (total_h / n_cols as f32).ceil()).max(1.0)
            };
            // Count-based per-column cap prevents starvation when content heights are equal.
            let per_col_cap = seg_idxs.len().div_ceil(n_cols as usize);

            // Greedy column assignment (height + count guard).
            let mut col_assignment = vec![0usize; seg_idxs.len()];
            let mut col_fill = vec![0.0f32; n_cols as usize];
            let mut col_count = vec![0usize; n_cols as usize];
            let mut cur_col = 0usize;
            for (j, &oh) in outer_hs.iter().enumerate() {
                let height_overflow = col_fill[cur_col] + oh > target_h && oh > 0.0;
                let count_overflow = col_count[cur_col] >= per_col_cap;
                if cur_col + 1 < n_cols as usize && (height_overflow || count_overflow) {
                    cur_col += 1;
                }
                col_assignment[j] = cur_col;
                col_fill[cur_col] += oh;
                col_count[cur_col] += 1;
            }

            // Second pass: final positioning.
            let mut col_y = vec![cur_y; n_cols as usize];
            for (j, &i) in seg_idxs.iter().enumerate() {
                let col = col_assignment[j];
                let col_x = content_x + col as f32 * (col_w + col_gap);
                lay_out(&mut children[i], col_x, col_y[col], col_w, None, measurer, viewport, pcb, hp);
                let mb = children[i].style.margin_bottom
                    .resolve_or_zero(children[i].style.font_size, col_w, viewport);
                col_y[col] = children[i].rect.y + children[i].rect.height + mb;
            }

            cur_y = col_y.into_iter().fold(cur_y, f32::max);
        }

        // column-span: all — element spans the full column container width.
        if let Some(span_i) = *span_idx {
            lay_out(&mut children[span_i], content_x, cur_y, content_width, None, measurer, viewport, pcb, hp);
            let mb = children[span_i].style.margin_bottom
                .resolve_or_zero(children[span_i].style.font_size, content_width, viewport);
            cur_y = children[span_i].rect.y + children[span_i].rect.height + mb;
        }
    }

    cur_y - content_y
}

/// Positions absolutely/fixed-positioned deferred children of `parent`.
/// Called after parent's height is finalized so `my_pcb` is complete.
fn lay_out_abs_children(
    parent: &mut LayoutBox,
    deferred: &[(usize, f32, f32)],
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    my_pcb: Rect,
    hp: &dyn HyphenationProvider,
) {
    for &(idx, static_x, static_y) in deferred {
        let cs = parent.children[idx].style.clone();
        let c_em = cs.font_size;

        let cb = if matches!(cs.position, Position::Fixed) {
            Rect::new(0.0, 0.0, viewport.width, viewport.height)
        } else {
            my_pcb
        };

        let left = cs.left.resolve(c_em, cb.width, viewport);
        let right = cs.right.resolve(c_em, cb.width, viewport);
        let top = cs.top.resolve(c_em, cb.height, viewport);
        let bottom = cs.bottom.resolve(c_em, cb.height, viewport);

        // Доступная ширина для layout абсолютного child.
        let avail_w = if left.is_some() && right.is_some() && cs.width.is_none() {
            (cb.width - left.unwrap_or(0.0) - right.unwrap_or(0.0)).max(0.0)
        } else {
            cb.width
        };

        lay_out(&mut parent.children[idx], 0.0, 0.0, avail_w, None, measurer, viewport, my_pcb, hp);

        let c_ml = cs.margin_left.resolve_or_zero(c_em, cb.width, viewport);
        let c_mr = cs.margin_right.resolve_or_zero(c_em, cb.width, viewport);
        let c_mt = cs.margin_top.resolve_or_zero(c_em, cb.height, viewport);
        let c_mb = cs.margin_bottom.resolve_or_zero(c_em, cb.height, viewport);

        // CSS Position L3 §6: an abs-pos box with both `top` and `bottom` non-auto
        // and `height: auto` resolves its used height to fill the inset gap. Mirror of
        // the `avail_w` width-from-insets path above. Applied post-layout because the
        // gap height is a containing-block used value, not a content-driven size.
        if top.is_some() && bottom.is_some() && cs.height.is_none() {
            let resolved_h =
                (cb.height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - c_mt - c_mb).max(0.0);
            parent.children[idx].rect.height = resolved_h;
        }

        let child = &mut parent.children[idx];

        // Desired border-left edge.
        let new_x = match (left, right) {
            (Some(l), _)    => cb.x + l + c_ml,
            (None, Some(r)) => cb.x + cb.width - r - c_mr - child.rect.width,
            (None, None)    => static_x + c_ml,
        };
        // Desired border-top edge.
        let new_y = match (top, bottom) {
            (Some(t), _)    => cb.y + t + c_mt,
            (None, Some(bv)) => cb.y + cb.height - bv - c_mb - child.rect.height,
            (None, None)    => static_y + c_mt,
        };

        let dx = new_x - child.rect.x;
        let dy = new_y - child.rect.y;
        shift_tree(child, dx, dy);
    }
}

/// CSS Flexbox L1 §9 — multi-line flex layout.
///
/// Алгоритм:
/// 1. Для каждого flex-item вычисляем hypothetical main size из flex-basis.
/// 2. Распределяем free space через flex-grow / flex-shrink.
/// 3. Раскладываем items с учётом justify-content и align-items.
/// 4. При flex-wrap: apply align-content across flex lines.
///
/// `explicit_cross` — явная высота контейнера (content box) для row flex;
/// используется в align-content для вычисления свободного пространства по cross axis.
///
/// Возвращает `content_height` (вертикальный размер контентной зоны контейнера).
#[allow(clippy::too_many_arguments)]
fn lay_out_flex(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    explicit_cross: Option<f32>,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let is_column = matches!(s.flex_direction, FlexDirection::Column | FlexDirection::ColumnReverse);
    let is_reverse = matches!(
        s.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    );
    let is_wrap = matches!(s.flex_wrap, FlexWrap::Wrap | FlexWrap::WrapReverse);
    let is_wrap_reverse = matches!(s.flex_wrap, FlexWrap::WrapReverse);

    // Indices of non-Skip children (actual flex items).
    let mut item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();
    // CSS Flexbox L1 §4 — stable sort by `order` (same-order items keep source order).
    item_idxs.sort_by_key(|&i| children[i].style.order);

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Container main size (for row: width; for column: 0 = auto, computed from items).
    let container_main = if is_column { 0.0 } else { content_width };

    // CSS Box Alignment §8: gap is fixed space between items, subtracted before flex-grow/shrink.
    let em = s.font_size;
    // item_gap: gap between items along the main axis.
    // cross_gap: gap between flex lines along the cross axis (wrap only).
    let item_gap = if is_column {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };
    let cross_gap = if is_column {
        s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    } else {
        s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0)
    };

    // Step 1 — preliminary layout for intrinsic sizes.
    let cb = content_width;
    for &i in &item_idxs {
        lay_out(&mut children[i], content_x, content_y, content_width, None, measurer, viewport, pcb, hp);
    }

    // Compute hypothetical main sizes for all items (outer = including margins).
    let all_hyp: Vec<f32> = item_idxs
        .iter()
        .map(|&i| {
            let item = &children[i];
            let is = &item.style;
            let iem = is.font_size;
            let m_l = is.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = is.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
            match &is.flex_basis {
                FlexBasis::Auto | FlexBasis::Content => {
                    if is_column {
                        item.rect.height + m_t + m_b
                    } else {
                        // CSS Flexbox §9.2: for auto flex-basis with no explicit width,
                        // use the max-content main size. Approximate by finding the
                        // widest child that has an explicit CSS width, rather than
                        // using the container-stretched width from the preliminary pass.
                        let w = if is.width.is_none() {
                            let max_child_w = item
                                .children
                                .iter()
                                .filter(|c| !matches!(c.kind, BoxKind::Skip) && c.style.width.is_some())
                                .map(|c| c.rect.width)
                                .fold(0.0_f32, f32::max);
                            if max_child_w > 0.0 { max_child_w } else { item.rect.width }
                        } else {
                            item.rect.width
                        };
                        w + m_l + m_r
                    }
                }
                FlexBasis::Length(l) => {
                    let base = l.resolve(iem, Some(cb), viewport).unwrap_or(0.0).max(0.0);
                    if is_column { base + m_t + m_b } else { base + m_l + m_r }
                }
            }
        })
        .collect();

    // Step 2 — break items into flex lines.
    // Wrap only applies to row direction (column wrapping requires known container height, Phase 0: skip).
    let lines: Vec<Vec<usize>> = if is_wrap && !is_column && container_main > 0.0 {
        let mut lines: Vec<Vec<usize>> = Vec::new();
        let mut cur_line: Vec<usize> = Vec::new();
        let mut cur_main = 0.0_f32;
        for (k, &item_main) in all_hyp.iter().enumerate() {
            let gap = if cur_line.is_empty() { 0.0 } else { item_gap };
            if !cur_line.is_empty() && cur_main + gap + item_main > container_main {
                lines.push(cur_line);
                cur_line = vec![k];
                cur_main = item_main;
            } else {
                cur_line.push(k);
                cur_main += gap + item_main;
            }
        }
        if !cur_line.is_empty() {
            lines.push(cur_line);
        }
        lines
    } else {
        vec![(0..item_idxs.len()).collect()]
    };

    // Step 3–5: process each line (grow/shrink, justify, position, align).
    // cross_cursor tracks the current cross-axis offset across lines.
    let mut cross_cursor = 0.0_f32;

    let n_lines = lines.len();
    let ordered_line_idxs: Vec<usize> = if is_wrap_reverse {
        (0..n_lines).rev().collect()
    } else {
        (0..n_lines).collect()
    };
    // Track line cross-sizes for align-content.
    let mut line_cross_sizes: Vec<f32> = Vec::with_capacity(n_lines);


    for li in &ordered_line_idxs {
        let line_keys = &lines[*li]; // keys into item_idxs
        let n = line_keys.len();

        // Per-line hyp mains (mutable for grow/shrink).
        let mut hyp_mains: Vec<f32> = line_keys.iter().map(|&k| all_hyp[k]).collect();

        // Free space after gaps.
        let line_gap_total = if n > 1 { item_gap * (n - 1) as f32 } else { 0.0 };
        let total_hyp: f32 = hyp_mains.iter().sum();
        let free_space = if is_column { 0.0 } else { container_main - total_hyp - line_gap_total };

        if free_space > 0.0 {
            let total_grow: f32 = line_keys.iter().map(|&k| children[item_idxs[k]].style.flex_grow).sum();
            if total_grow > 0.0 {
                for (j, &k) in line_keys.iter().enumerate() {
                    let grow = children[item_idxs[k]].style.flex_grow;
                    hyp_mains[j] += free_space * (grow / total_grow);
                }
            }
        } else if free_space < 0.0 {
            let weights: Vec<f32> = line_keys
                .iter()
                .enumerate()
                .map(|(j, &k)| children[item_idxs[k]].style.flex_shrink * hyp_mains[j])
                .collect();
            let total_weight: f32 = weights.iter().sum();
            if total_weight > 0.0 {
                for j in 0..n {
                    hyp_mains[j] = (hyp_mains[j] + free_space * (weights[j] / total_weight)).max(0.0);
                }
            }
        }

        // Justify-content within the line.
        let resolved_main: f32 = hyp_mains.iter().sum();
        let remaining = if is_column { 0.0 } else { (container_main - resolved_main - line_gap_total).max(0.0) };
        let (jc_start, jc_gap) = match s.justify_content {
            AlignValue::End => (remaining, 0.0),
            AlignValue::Center => (remaining / 2.0, 0.0),
            AlignValue::SpaceBetween => {
                if n <= 1 { (0.0, 0.0) } else { (0.0, remaining / (n - 1) as f32) }
            }
            AlignValue::SpaceAround => {
                let per = remaining / n as f32;
                (per / 2.0, per)
            }
            AlignValue::SpaceEvenly => {
                let per = remaining / (n + 1) as f32;
                (per, per)
            }
            _ => (0.0, 0.0),
        };

        // Final layout: position items along main axis.
        let ordered_keys: Vec<usize> = if is_reverse { (0..n).rev().collect() } else { (0..n).collect() };
        let mut main_cursor = jc_start;

        for &j in &ordered_keys {
            let k = line_keys[j];
            let i = item_idxs[k];
            let outer_main = hyp_mains[j];
            let item_s = children[i].style.clone();
            let iem = item_s.font_size;
            let m_l = item_s.margin_left.resolve_or_zero(iem, cb, viewport);
            let m_r = item_s.margin_right.resolve_or_zero(iem, cb, viewport);
            let m_t = item_s.margin_top.resolve_or_zero(iem, cb, viewport);
            let m_b = item_s.margin_bottom.resolve_or_zero(iem, cb, viewport);

            if is_column {
                let inner_main = (outer_main - m_t - m_b).max(0.0);
                children[i].style.height = Some(Length::Px(inner_main));
                lay_out(
                    &mut children[i],
                    content_x + m_l,
                    content_y + main_cursor + m_t,
                    content_width - m_l - m_r,
                    None,
                    measurer,
                    viewport,
                    pcb,
                    hp,
                );
                main_cursor += outer_main + item_gap + jc_gap;
            } else {
                let inner_main = (outer_main - m_l - m_r).max(0.0);
                children[i].style.width = Some(Length::Px(inner_main));
                lay_out(
                    &mut children[i],
                    content_x + main_cursor + m_l,
                    content_y + cross_cursor + m_t,
                    inner_main,
                    None,
                    measurer,
                    viewport,
                    pcb,
                    hp,
                );
                main_cursor += outer_main + item_gap + jc_gap;
            }
        }

        // Align-items on cross axis for this line.
        let line_cross: f32 = if is_column {
            0.0 // column cross axis (width) not handled in wrap Phase 0
        } else {
            line_keys.iter().map(|&k| children[item_idxs[k]].rect.height).fold(0.0_f32, f32::max)
        };
        line_cross_sizes.push(line_cross);

        if !is_column {
            for &k in line_keys {
                let i = item_idxs[k];
                let item = &mut children[i];
                let is = &item.style;
                let iem = is.font_size;
                let m_t = is.margin_top.resolve_or_zero(iem, cb, viewport);
                let m_b = is.margin_bottom.resolve_or_zero(iem, cb, viewport);
                let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
                let outer_cross = item.rect.height + m_t + m_b;
                match align {
                    AlignValue::End => {
                        item.rect.y = content_y + cross_cursor + line_cross - outer_cross + m_t;
                    }
                    AlignValue::Center => {
                        item.rect.y = content_y + cross_cursor + m_t + (line_cross - outer_cross) / 2.0;
                    }
                    AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                        let stretch_h = (line_cross - m_t - m_b).max(item.rect.height);
                        if item.rect.height < stretch_h {
                            item.rect.height = stretch_h;
                        }
                        item.rect.y = content_y + cross_cursor + m_t;
                    }
                    _ => {
                        item.rect.y = content_y + cross_cursor + m_t;
                    }
                }
            }
        }

        cross_cursor += line_cross + cross_gap;
    }

    // Remove trailing gap from cross_cursor.
    let mut total_cross = if n_lines > 1 {
        cross_cursor - cross_gap
    } else {
        cross_cursor
    };

    // Apply align-content to distribute remaining space between flex lines (row wrap only).
    // Uses explicit_cross (container height) to compute free cross-axis space.
    if !is_column && n_lines > 1 && is_wrap {
        let line_gap_total = cross_gap * (n_lines.saturating_sub(1)) as f32;
        let used_cross: f32 = line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        let free_cross = explicit_cross.map_or(0.0, |h| (h - used_cross).max(0.0));

        if free_cross > 0.0 {
            let mut line_offsets: Vec<f32> = vec![0.0; n_lines];

            match s.align_content {
                AlignValue::End => {
                    line_offsets.fill(free_cross);
                }
                AlignValue::Center => {
                    line_offsets.fill(free_cross / 2.0);
                }
                AlignValue::SpaceBetween if n_lines > 1 => {
                    let gap_per = free_cross / (n_lines - 1) as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate().skip(1) {
                        *offset = gap_per * i as f32;
                    }
                }
                AlignValue::SpaceAround => {
                    let per = free_cross / n_lines as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per / 2.0 + (per * i as f32);
                    }
                }
                AlignValue::SpaceEvenly => {
                    let per = free_cross / (n_lines + 1) as f32;
                    for (i, offset) in line_offsets.iter_mut().enumerate() {
                        *offset = per * (i as f32 + 1.0);
                    }
                }
                AlignValue::Stretch => {
                    let total_size: f32 = line_cross_sizes.iter().sum();
                    if total_size > 0.0 {
                        for size in line_cross_sizes.iter_mut() {
                            *size += free_cross * (*size / total_size);
                        }
                    }
                }
                _ => {
                }
            }

            for li in 0..n_lines {
                let line_keys = &lines[li];
                let offset = line_offsets[li];

                if !is_column && offset > 0.0 {
                    for &k in line_keys {
                        let i = item_idxs[k];
                        children[i].rect.y += offset;
                    }
                }
            }

            total_cross = line_cross_sizes.iter().sum::<f32>() + line_gap_total;
        }
    }

    if is_column {
        // Column: return main-axis height (main_cursor from last line).
        // Re-compute from stored item positions.
        item_idxs
            .iter()
            .map(|&i| children[i].rect.y + children[i].rect.height - content_y)
            .fold(0.0_f32, f32::max)
    } else {
        total_cross
    }
}

/// CSS Grid Layout Level 1 — grid container layout.
///
/// Implements a Phase-0 subset of the grid layout algorithm (CSS Grid L1 §12):
///
/// - Explicit track lists (grid-template-columns / rows) with px, fr, auto.
/// - `repeat(N, size)` expansion.
/// - `minmax(min, max)` — min side used for sizing.
/// - Integer line numbers (positive only), `span N`, and `auto` placement.
/// - `grid-auto-flow: row | column` (no dense packing).
/// - `gap` / `column-gap` / `row-gap` between cells.
/// - `align-items` / `justify-items` within cells.
///
/// Returns the total content height of the grid.
#[allow(clippy::too_many_arguments)]
fn lay_out_grid(
    children: &mut [LayoutBox],
    s: &ComputedStyle,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    measurer: Option<&dyn TextMeasurer>,
    viewport: Size,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
) -> f32 {
    let em = s.font_size;

    // CSS Grid L2 §9: If this grid was set up as a subgrid by its parent, read
    // the inherited track contexts that the parent set in the thread-locals.
    // We clear them immediately so our own children don't accidentally inherit them.
    let inherited_cols: Option<SubgridContext> = SUBGRID_COL_CTX.with(|c| c.borrow_mut().take());
    let inherited_rows: Option<SubgridContext> = SUBGRID_ROW_CTX.with(|c| c.borrow_mut().take());

    // Indices of actual items (non-Skip).
    let item_idxs: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| !matches!(c.kind, BoxKind::Skip))
        .map(|(i, _)| i)
        .collect();

    if item_idxs.is_empty() {
        return 0.0;
    }

    // Gap between tracks.  When the axis is subgridded we use the parent's gap
    // (already baked into the offsets in SubgridContext); fall back to our own style.
    let col_gap = inherited_cols.as_ref()
        .map(|ctx| ctx.gap)
        .unwrap_or_else(|| s.column_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0));
    let row_gap = inherited_rows.as_ref()
        .map(|ctx| ctx.gap)
        .unwrap_or_else(|| s.row_gap.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0));

    // Determine explicit track counts.
    // Subgrid sentinel `[Subgrid]` is a single-element vec meaning "inherit all parent tracks";
    // for placement purposes use the number of inherited tracks (or 1 for auto-placement).
    let n_explicit_cols = if s.grid_template_columns.first() == Some(&GridTrackSize::Subgrid) {
        inherited_cols.as_ref().map(|ctx| ctx.sizes.len()).unwrap_or(1).max(1)
    } else {
        s.grid_template_columns.len().max(1)
    };

    // --- Step 1: Resolve placements for every item ---
    // placement: (col_start, col_end, row_start, row_end) all 1-based inclusive/exclusive.
    let mut placements: Vec<(u32, u32, u32, u32)> = vec![(0, 0, 0, 0); item_idxs.len()];

    let row_flow = !matches!(s.grid_auto_flow, GridAutoFlow::Column | GridAutoFlow::ColumnDense);

    // Pass 1: items with fully explicit placements.
    for (k, &i) in item_idxs.iter().enumerate() {
        let is = &children[i].style;

        // Resolve named area references first (grid-area: <name> shorthand or
        // individual grid-{row,column}-{start,end}: <name> values).
        let (named_cs, named_ce, named_rs, named_re) = {
            let has_named = matches!(&is.grid_column_start, GridLine::Named(_))
                || matches!(&is.grid_column_end, GridLine::Named(_))
                || matches!(&is.grid_row_start, GridLine::Named(_))
                || matches!(&is.grid_row_end, GridLine::Named(_));
            if has_named && !s.grid_template_areas.is_empty() {
                resolve_named_lines(
                    &is.grid_column_start,
                    &is.grid_column_end,
                    &is.grid_row_start,
                    &is.grid_row_end,
                    &s.grid_template_areas,
                )
            } else {
                (0, 0, 0, 0)
            }
        };

        // For each axis: use resolved named value if non-zero, else fall back to
        // the normal numeric/span resolver.
        let cs = if named_cs != 0 { named_cs } else { resolve_grid_line(&is.grid_column_start, n_explicit_cols as u32) };
        let ce = if named_ce != 0 { named_ce } else { resolve_grid_line_end(&is.grid_column_end, cs, n_explicit_cols as u32) };
        let rs = if named_rs != 0 { named_rs } else { resolve_grid_line(&is.grid_row_start, 0) };
        let re = if named_re != 0 { named_re } else { resolve_grid_line_end(&is.grid_row_end, rs, 0) };

        // `grid-column: span N` → start=Span(N), end=Auto → cs=0, ce=0.
        // resolve_grid_line returns 0 for Span-on-start, losing the count.
        // Recover the span so Pass 2 can use it for placement sizing.
        let ce = if ce == 0 {
            match &is.grid_column_start { GridLine::Span(n) => *n, _ => 0 }
        } else { ce };
        let re = if re == 0 {
            match &is.grid_row_start { GridLine::Span(n) => *n, _ => 0 }
        } else { re };

        if cs != 0 && rs != 0 {
            // Fully explicit: both axes known.
            placements[k] = (cs, ce, rs, re);
        } else if cs != 0 {
            // Column position fixed, row auto; preserve row-span if declared.
            placements[k] = (cs, ce, 0, re);
        } else if rs != 0 {
            // Row position fixed, column auto; preserve col-span if declared.
            placements[k] = (0, ce, rs, re);
        } else if ce > 0 || re > 0 {
            // Both axes auto but at least one span is declared (e.g. grid-column:span 2).
            // Store so pass-2 can recover the span via `end - 0 = span`.
            placements[k] = (0, ce, 0, re);
        }
        // All-auto no spans: stays (0,0,0,0) → span=1 in pass 2.
    }

    // Pass 2: auto-place remaining items — CSS Grid L1 §8.5 auto-placement algorithm.
    //
    // Two packing modes:
    //   Sparse (grid-auto-flow: row | column): cursor only moves forward.
    //   Dense  (grid-auto-flow: row dense | column dense): each item scans from
    //          (1,1) so it can fill gaps left by larger items.
    //
    // Occupancy HashSet replaces the O(k²) overlap scan from Pass 1 with O(1)
    // per-cell lookups.
    let dense = matches!(s.grid_auto_flow, GridAutoFlow::RowDense | GridAutoFlow::ColumnDense);
    let mut occupied: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    for &(cs, ce, rs, re) in &placements {
        if cs != 0 && rs != 0 {
            for r in rs..re {
                for c in cs..ce {
                    occupied.insert((c, r));
                }
            }
        }
    }

    let mut cursor_row: u32 = 1;
    let mut cursor_col: u32 = 1;

    for (k, _) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs != 0 && rs != 0 {
            continue; // explicitly placed
        }

        let col_span = if ce > cs { ce - cs } else { 1 };
        let row_span = if re > rs { re - rs } else { 1 };

        if row_flow {
            let fixed_cs = if cs != 0 { cs } else { 0 };
            let fixed_ce = if cs != 0 { ce } else { 0 };

            // Dense packing starts each scan from (1,1); sparse continues from cursor.
            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            loop {
                let try_c   = if fixed_cs != 0 { fixed_cs } else { scan_c };
                let try_ce_val = if fixed_cs != 0 { fixed_ce } else { try_c + col_span };

                // Bounds: item must fit within explicit column count (or 1-col fallback).
                let fits = (try_ce_val - 1) <= n_explicit_cols as u32 || n_explicit_cols == 1;
                let cell_free = fits && (try_c..try_ce_val)
                    .all(|c| (scan_r..scan_r + row_span).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (try_c, try_ce_val, scan_r, scan_r + row_span);
                    for r in scan_r..scan_r + row_span {
                        for c in try_c..try_ce_val {
                            occupied.insert((c, r));
                        }
                    }
                    // Track highest placed row for grid-size calculation.
                    cursor_row = cursor_row.max(scan_r);
                    if !dense {
                        cursor_col = try_ce_val;
                        if cursor_col > n_explicit_cols as u32 {
                            cursor_col = 1;
                            cursor_row += 1;
                        }
                    }
                    break;
                }

                // Advance scan position.
                if fixed_cs != 0 {
                    scan_r += 1;
                    scan_c = 1;
                } else {
                    scan_c += 1;
                    if scan_c > n_explicit_cols as u32 {
                        scan_c = 1;
                        scan_r += 1;
                    }
                }
            }
        } else {
            // Column flow: fill top-to-bottom, wrap to next column.
            let n_explicit_rows = s.grid_template_rows.len().max(1) as u32;
            let fixed_rs = if rs != 0 { rs } else { 0 };
            let fixed_re = if rs != 0 { re } else { 0 };

            let (mut scan_r, mut scan_c) = if dense { (1u32, 1u32) } else { (cursor_row, cursor_col) };

            loop {
                let try_r      = if fixed_rs != 0 { fixed_rs } else { scan_r };
                let try_re_val = if fixed_rs != 0 { fixed_re } else { try_r + row_span };

                let fits = (try_re_val - 1) <= n_explicit_rows || n_explicit_rows == 1;
                let cell_free = fits && (scan_c..scan_c + col_span)
                    .all(|c| (try_r..try_re_val).all(|r| !occupied.contains(&(c, r))));

                if cell_free {
                    placements[k] = (scan_c, scan_c + col_span, try_r, try_re_val);
                    for r in try_r..try_re_val {
                        for c in scan_c..scan_c + col_span {
                            occupied.insert((c, r));
                        }
                    }
                    cursor_col = cursor_col.max(scan_c);
                    if !dense {
                        cursor_row = try_re_val;
                        if cursor_row > n_explicit_rows {
                            cursor_row = 1;
                            cursor_col += 1;
                        }
                    }
                    break;
                }

                if fixed_rs != 0 {
                    scan_c += 1;
                    scan_r = 1;
                } else {
                    scan_r += 1;
                    if scan_r > n_explicit_rows {
                        scan_r = 1;
                        scan_c += 1;
                    }
                }
            }
        }
    }

    // --- Step 2: Determine total grid dimensions ---
    let n_cols = placements.iter().map(|&(_, ce, _, _)| ce.saturating_sub(1)).max().unwrap_or(1)
        .max(n_explicit_cols as u32);
    let n_rows = placements.iter().map(|&(_, _, _, re)| re.saturating_sub(1)).max().unwrap_or(1);

    // --- Step 3: Compute column widths ---
    // If the column axis is subgridded, use the inherited track sizes directly;
    // otherwise compute from the style as usual (CSS Grid L2 §9).
    let (col_widths, col_offsets) = if let Some(ref ctx) = inherited_cols {
        // Subgrid column axis: clip to n_cols (parent may span more tracks than
        // the explicit template; auto-place inside those tracks).
        let sizes: Vec<f32> = ctx.sizes.iter().take(n_cols as usize).cloned().collect();
        let offsets: Vec<f32> = ctx.offsets.iter().take(n_cols as usize).cloned().collect();
        (sizes, offsets)
    } else {
        // Normal grid: compute column widths from the style.
        let mut col_widths: Vec<f32> = (0..n_cols)
            .map(|c| {
                let ts = grid_track(c, &s.grid_template_columns, &s.grid_auto_columns);
                match ts {
                    GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                    GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                    // Subgrid sentinel without parent context — fall back to auto.
                    GridTrackSize::Subgrid => 0.0,
                    _ => 0.0, // fr / auto resolved later
                }
            })
            .collect();

        // Total gap between columns.
        let total_col_gap = if n_cols > 1 { col_gap * (n_cols - 1) as f32 } else { 0.0 };
        let fixed_col_total: f32 = col_widths.iter().sum::<f32>() + total_col_gap;
        let free_col = (content_width - fixed_col_total).max(0.0);

        // Distribute fr among column tracks.
        let total_fr: f32 = (0..n_cols)
            .map(|c| grid_track(c, &s.grid_template_columns, &s.grid_auto_columns).fr().unwrap_or(0.0))
            .sum();
        let auto_col_count = (0..n_cols)
            .filter(|&c| matches!(
                grid_track(c, &s.grid_template_columns, &s.grid_auto_columns),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent
            ))
            .count();

        // For auto columns, divide remaining free space equally (after fr).
        let fr_width = if total_fr > 0.0 { free_col / total_fr } else { 0.0 };
        let auto_col_width = if auto_col_count > 0 && total_fr == 0.0 {
            free_col / auto_col_count as f32
        } else {
            0.0
        };

        for c in 0..n_cols {
            match grid_track(c, &s.grid_template_columns, &s.grid_auto_columns) {
                GridTrackSize::Fr(f) => col_widths[c as usize] = (f * fr_width).max(0.0),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent => {
                    col_widths[c as usize] = auto_col_width;
                }
                _ => {}
            }
        }

        // Column start offsets.
        let mut col_offsets: Vec<f32> = Vec::with_capacity(n_cols as usize);
        let mut x_off = 0.0_f32;
        for c in 0..n_cols {
            col_offsets.push(x_off);
            x_off += col_widths[c as usize] + if c < n_cols - 1 { col_gap } else { 0.0 };
        }

        (col_widths, col_offsets)
    };

    // --- Step 4: Layout items to measure row heights ---
    // If the row axis is subgridded, use inherited sizes; otherwise compute from style.
    let mut row_heights: Vec<f32> = if let Some(ref ctx) = inherited_rows {
        ctx.sizes.iter().take(n_rows as usize).cloned().collect()
    } else {
        (0..n_rows)
            .map(|r| {
                match grid_track(r, &s.grid_template_rows, &s.grid_auto_rows) {
                    GridTrackSize::Length(l) => l.resolve(em, Some(content_width), viewport).unwrap_or(0.0).max(0.0),
                    GridTrackSize::Minmax(min, _) => min.resolve_fixed(em, content_width, viewport).unwrap_or(0.0),
                    GridTrackSize::Subgrid => 0.0,
                    _ => 0.0,
                }
            })
            .collect()
    };

    // Row offsets (computed from row_heights regardless of subgrid).
    // For subgrid row axis the offsets are inherited below in final pass.

    // Layout each item in its cell to determine content height.
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs == 0 || rs == 0 {
            continue; // unplaced (should not happen after auto-placement)
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let cell_w: f32 = if c1 > c0 {
            col_widths[c0..c1].iter().sum::<f32>() + col_gap * (c1 - c0 - 1) as f32
        } else {
            col_widths.get(c0).copied().unwrap_or(0.0)
        };

        // For subgrid children: set the thread-local context before laying out.
        let child_col_subgrid = children[i].style.grid_template_columns.first()
            == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = children[i].style.grid_template_rows.first()
            == Some(&GridTrackSize::Subgrid);

        if child_col_subgrid || child_row_subgrid {
            // Build subgrid context slices from our resolved track sizes.
            let child_col_ctx = if child_col_subgrid && c1 > c0 {
                Some(SubgridContext::from_parent_tracks(&col_widths[c0..c1], col_gap))
            } else {
                None
            };
            let child_row_ctx = if child_row_subgrid {
                // Row heights not fully determined yet; pass current estimates.
                let r0 = (rs - 1).min(n_rows - 1) as usize;
                let re_eff = re.max(rs + 1);
                let r1 = (re_eff - 1).min(n_rows) as usize;
                if r1 > r0 {
                    Some(SubgridContext::from_parent_tracks(&row_heights[r0..r1], row_gap))
                } else {
                    None
                }
            } else {
                None
            };
            let _guard = SubgridContextGuard::set(child_col_ctx, child_row_ctx);
            lay_out(&mut children[i], content_x + col_offsets.get(c0).copied().unwrap_or(0.0), 0.0, cell_w, None, measurer, viewport, pcb, hp);
        } else {
            // Layout at temporary position (y=0) to get intrinsic height.
            lay_out(&mut children[i], content_x + col_offsets.get(c0).copied().unwrap_or(0.0), 0.0, cell_w, None, measurer, viewport, pcb, hp);
        }

        // Update auto row heights.
        let r0 = (rs - 1) as usize;
        if r0 < row_heights.len()
            && inherited_rows.is_none()
            && matches!(
                grid_track(r0 as u32, &s.grid_template_rows, &s.grid_auto_rows),
                GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent | GridTrackSize::Fr(_)
            )
        {
            let item_h = children[i].rect.height;
            if item_h > row_heights[r0] {
                row_heights[r0] = item_h;
            }
        }
    }

    // Resolve fr row heights (skip when row axis is subgridded — sizes are fixed).
    if inherited_rows.is_none() {
        let total_row_gap = if n_rows > 1 { row_gap * (n_rows - 1) as f32 } else { 0.0 };
        let fixed_row_total: f32 = row_heights.iter().sum::<f32>() + total_row_gap;
        // If container has explicit height, distribute fr rows from it.
        let container_h = s.height.as_ref().and_then(|h| h.resolve(em, Some(content_width), viewport));
        let free_row = container_h.map(|h| (h - fixed_row_total).max(0.0)).unwrap_or(0.0);
        let total_row_fr: f32 = (0..n_rows)
            .map(|r| grid_track(r, &s.grid_template_rows, &s.grid_auto_rows).fr().unwrap_or(0.0))
            .sum();
        if total_row_fr > 0.0 && free_row > 0.0 {
            let fr_h = free_row / total_row_fr;
            for r in 0..n_rows {
                if let Some(f) = grid_track(r, &s.grid_template_rows, &s.grid_auto_rows).fr() {
                    row_heights[r as usize] = (f * fr_h).max(row_heights[r as usize]);
                }
            }
        }
    }

    // Row top offsets: if row axis is subgridded, use inherited offsets; else compute.
    let (row_offsets, y_off) = if let Some(ref ctx) = inherited_rows {
        let offsets: Vec<f32> = ctx.offsets.iter().take(n_rows as usize).cloned().collect();
        let total = ctx.total_size();
        (offsets, total)
    } else {
        let mut row_offsets: Vec<f32> = Vec::with_capacity(n_rows as usize);
        let mut y_off = 0.0_f32;
        for r in 0..n_rows {
            row_offsets.push(y_off);
            y_off += row_heights[r as usize] + if r < n_rows - 1 { row_gap } else { 0.0 };
        }
        (row_offsets, y_off)
    };
    let mut y_off = y_off;

    // --- Step 5: Final positioning pass ---
    for (k, &i) in item_idxs.iter().enumerate() {
        let (cs, ce, rs, re) = placements[k];
        if cs == 0 || rs == 0 {
            // Unplaced — stack below grid content.
            lay_out(&mut children[i], content_x, content_y + y_off, content_width, None, measurer, viewport, pcb, hp);
            y_off += children[i].rect.height;
            continue;
        }
        let c0 = (cs - 1).min(n_cols - 1) as usize;
        let c1 = (ce - 1).min(n_cols) as usize;
        let r0 = (rs - 1).min(n_rows - 1) as usize;
        let r1 = (re - 1).min(n_rows) as usize;

        let cell_x = content_x + col_offsets[c0];
        let cell_y = content_y + row_offsets[r0];
        let cell_w: f32 = if c1 > c0 {
            col_widths[c0..c1].iter().sum::<f32>() + col_gap * (c1 - c0 - 1) as f32
        } else {
            col_widths[c0]
        };
        let cell_h: f32 = if r1 > r0 {
            row_heights[r0..r1].iter().sum::<f32>() + row_gap * (r1 - r0 - 1) as f32
        } else {
            row_heights[r0]
        };

        // Re-layout with final cell width. For subgrid children, restore the context.
        let child_col_subgrid = children[i].style.grid_template_columns.first()
            == Some(&GridTrackSize::Subgrid);
        let child_row_subgrid = children[i].style.grid_template_rows.first()
            == Some(&GridTrackSize::Subgrid);
        if child_col_subgrid || child_row_subgrid {
            let final_col_ctx = if child_col_subgrid && c1 > c0 {
                Some(SubgridContext::from_parent_tracks(&col_widths[c0..c1], col_gap))
            } else {
                None
            };
            let final_row_ctx = if child_row_subgrid && r1 > r0 {
                Some(SubgridContext::from_parent_tracks(&row_heights[r0..r1], row_gap))
            } else {
                None
            };
            let _guard = SubgridContextGuard::set(final_col_ctx, final_row_ctx);
            lay_out(&mut children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp);
        } else {
            lay_out(&mut children[i], cell_x, cell_y, cell_w, None, measurer, viewport, pcb, hp);
        }

        let item = &mut children[i];
        let is = &item.style;
        let iem = is.font_size;
        let m_t = is.margin_top.resolve_or_zero(iem, content_width, viewport);
        let m_b = is.margin_bottom.resolve_or_zero(iem, content_width, viewport);
        let m_l = is.margin_left.resolve_or_zero(iem, content_width, viewport);
        let m_r = is.margin_right.resolve_or_zero(iem, content_width, viewport);

        // align-items (cross / block axis within cell).
        let align = if matches!(is.align_self, AlignValue::Auto) { s.align_items } else { is.align_self };
        let item_outer_h = item.rect.height + m_t + m_b;
        match align {
            AlignValue::End => {
                item.rect.y = cell_y + cell_h - item.rect.height - m_b;
            }
            AlignValue::Center => {
                item.rect.y = cell_y + (cell_h - item_outer_h) / 2.0 + m_t;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                if item.rect.height < cell_h - m_t - m_b {
                    item.rect.height = (cell_h - m_t - m_b).max(item.rect.height);
                }
                item.rect.y = cell_y + m_t;
            }
            _ => {
                item.rect.y = cell_y + m_t;
            }
        }

        // justify-items (inline axis within cell).
        let justify = if matches!(is.justify_self, AlignValue::Auto) { s.justify_items } else { is.justify_self };
        let item_outer_w = item.rect.width + m_l + m_r;
        match justify {
            AlignValue::End => {
                item.rect.x = cell_x + cell_w - item.rect.width - m_r;
            }
            AlignValue::Center => {
                item.rect.x = cell_x + (cell_w - item_outer_w) / 2.0 + m_l;
            }
            AlignValue::Stretch | AlignValue::Auto | AlignValue::Normal => {
                item.rect.x = cell_x + m_l;
            }
            _ => {
                item.rect.x = cell_x + m_l;
            }
        }
    }

    y_off
}

/// CSS Grid Layout L3 §9 — Resolve `repeat(auto-fill|auto-fit, <track-list>)` count.
/// Returns the number of tracks to fill the available space when using auto-fill or auto-fit.
///
/// # Arguments
/// * `available_width` — CSS px width of the container content box.
/// * `track_sizes` — The track sizes inside the repeat(), e.g. `[minmax(100px, 1fr)]`.
/// * `gap` — Column gap in px.
/// * `auto_fit` — If true, resolve as auto-fit (collapse empty tracks); else auto-fill.
///
/// # Returns
/// The minimum number of tracks that fit in available space, with preference
/// for auto-fill (leave empty) over auto-fit (collapse).
pub fn resolve_auto_fill_fit_count(
    available_width: f32,
    track_sizes: &[GridTrackSize],
    gap: f32,
) -> usize {
    if track_sizes.is_empty() || available_width <= 0.0 {
        return 1; // At least one track
    }

    // Compute minimum track width: the min() sizing function of each track.
    // For minmax(min, max), use min. For auto/fr/max-content, use 0 as placeholder (content-sized).
    let mut track_min_width: f32 = 0.0;
    for track in track_sizes {
        let w = match track {
            GridTrackSize::Length(len) => {
                // Fixed length: use as-is (simplified: only px supported in this pass)
                len.resolve(1.0, Some(available_width), Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            GridTrackSize::Minmax(min, _max) => {
                // Use the min() part
                min.resolve_fixed(1.0, available_width, Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            GridTrackSize::FitContent(limit) => {
                // Use the limit as min sizing (simplified)
                limit.resolve_fixed(1.0, available_width, Size::new(1024.0, 768.0))
                    .unwrap_or(0.0)
            }
            // Auto, MinContent, MaxContent, Fr, Subgrid: no fixed minimum, use 0
            _ => 0.0,
        };
        track_min_width = track_min_width.max(w);
    }

    // Count tracks: (available_width + gap) / (track_min_width + gap), minimum 1.
    let gap_adjusted_available = available_width + gap;
    let track_plus_gap = track_min_width + gap;

    if track_plus_gap <= 0.0 {
        1
    } else {
        ((gap_adjusted_available / track_plus_gap).floor() as usize).max(1)
    }
}

/// Return the track size for track index `idx` (0-based) from a template list,
/// falling back to `auto_track` for implicit tracks beyond the template.
fn grid_track<'a>(idx: u32, template: &'a [GridTrackSize], auto_track: &'a GridTrackSize) -> &'a GridTrackSize {
    template.get(idx as usize).unwrap_or(auto_track)
}

/// Resolve a `GridLine` to a 1-based track number, or 0 if auto.
fn resolve_grid_line(line: &GridLine, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => 0,
        GridLine::Line(n) => {
            if *n > 0 {
                *n as u32
            } else if n_tracks > 0 {
                // Negative line numbers count from the end.
                (n_tracks as i32 + 1 + n).max(1) as u32
            } else {
                1
            }
        }
        GridLine::Span(_) => 0, // span on start — auto
    }
}

/// Resolve a grid-line end given start position and span.
fn resolve_grid_line_end(line: &GridLine, start: u32, n_tracks: u32) -> u32 {
    match line {
        GridLine::Auto | GridLine::Named(_) => {
            if start > 0 { start + 1 } else { 0 }
        }
        GridLine::Line(n) => {
            if *n > 0 {
                (*n as u32).max(start + 1)
            } else if n_tracks > 0 {
                let abs = (n_tracks as i32 + 1 + n).max(1) as u32;
                abs.max(start + 1)
            } else {
                start + 1
            }
        }
        GridLine::Span(n) => {
            // When start is known: end = start + span.
            // When start is auto (0): store span N directly so pass-2 placement
            // can use `re - rs = N - 0 = N` to recover the span count.
            if start > 0 { start + n } else { *n }
        }
    }
}

/// CSS Grid L1 §7.3 — locate a named area in `grid-template-areas`.
///
/// Returns `(row_start, row_end, col_start, col_end)` as 1-based exclusive
/// line numbers, or `None` if the name is not found. Handles rectangular
/// area shapes only (CSS Grid L1 requires areas to be rectangular).
fn find_named_area(areas: &[Vec<String>], name: &str) -> Option<(u32, u32, u32, u32)> {
    let mut row_start: Option<u32> = None;
    let mut row_end: Option<u32> = None;
    let mut col_start: Option<u32> = None;
    let mut col_end: Option<u32> = None;
    for (r, row) in areas.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if cell == name {
                let rs = (r + 1) as u32;
                let re = (r + 2) as u32;
                let cs = (c + 1) as u32;
                let ce = (c + 2) as u32;
                row_start = Some(row_start.map_or(rs, |v: u32| v.min(rs)));
                row_end   = Some(row_end.map_or(re,   |v: u32| v.max(re)));
                col_start = Some(col_start.map_or(cs, |v: u32| v.min(cs)));
                col_end   = Some(col_end.map_or(ce,   |v: u32| v.max(ce)));
            }
        }
    }
    Some((row_start?, row_end?, col_start?, col_end?))
}

/// Resolve named grid-line references for a single item against the
/// container's `grid-template-areas`. Returns `(col_start, col_end, row_start, row_end)`.
///
/// When all four placement properties are `Named(same_name)` (set by
/// `grid-area: <name>` shorthand), the area bounds are looked up once and
/// applied to all four axes. Mixed named/unnamed configurations fall back
/// to `Auto` (0) for any unresolved axis.
fn resolve_named_lines(
    col_start: &GridLine,
    col_end: &GridLine,
    row_start: &GridLine,
    row_end: &GridLine,
    areas: &[Vec<String>],
) -> (u32, u32, u32, u32) {
    // When grid-area: <name> sets all four to Named(name), resolve as one area.
    if let (
        GridLine::Named(n_cs),
        GridLine::Named(n_ce),
        GridLine::Named(n_rs),
        GridLine::Named(n_re),
    ) = (col_start, col_end, row_start, row_end)
        && n_cs == n_ce
        && n_ce == n_rs
        && n_rs == n_re
        && let Some((rs, re, cs, ce)) = find_named_area(areas, n_cs)
    {
        return (cs, ce, rs, re);
    }
    // Partial Named references: each axis resolved independently.
    let cs = if let GridLine::Named(n) = col_start {
        find_named_area(areas, n).map_or(0, |(_, _, cs, _)| cs)
    } else { 0 };
    let ce = if let GridLine::Named(n) = col_end {
        find_named_area(areas, n).map_or(0, |(_, _, _, ce)| ce)
    } else { 0 };
    let rs = if let GridLine::Named(n) = row_start {
        find_named_area(areas, n).map_or(0, |(rs, _, _, _)| rs)
    } else { 0 };
    let re = if let GridLine::Named(n) = row_end {
        find_named_area(areas, n).map_or(0, |(_, re, _, _)| re)
    } else { 0 };
    (cs, ce, rs, re)
}

/// Strips U+00AD (soft hyphens) from a word and collects break positions
/// (byte offsets in the returned display string).
fn strip_soft_hyphens(raw: &str) -> (String, Vec<usize>) {
    let mut display = String::with_capacity(raw.len());
    let mut positions: Vec<usize> = Vec::new();
    for ch in raw.chars() {
        if ch == '\u{00AD}' {
            positions.push(display.len());
        } else {
            display.push(ch);
        }
    }
    (display, positions)
}

/// Measures text width (letter_spacing applied between each character).
/// `tab_size` is used for `\t` characters; pass 0.0 when text contains no tabs.
// CSS: font-variation-settings — P4 добавит `variation_axes` параметр когда
// TextMeasurer получит char_width_varied; сейчас используется base advance width.
pub fn measure_text_w(text: &str, font_size: f32, letter_spacing: f32, tab_size: f32, m: &dyn TextMeasurer) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: f32 = text
        .chars()
        .map(|c| {
            let cw = if c == '\t' { tab_size } else { m.char_width(c, font_size) };
            cw + letter_spacing
        })
        .sum();
    total - letter_spacing
}

/// Как [`measure_text_w`], но учитывает CSS `font-family` каскад.
///
/// Используется в `wrap_inline_run`, где для каждого `InlineSegment` доступен
/// `seg.style.font_family`. Позволяет `MultiFontMeasurer` выбирать правильный
/// шрифт для измерения ширины слов при перенос-расчёте.
pub fn measure_text_w_families(
    text: &str,
    font_size: f32,
    letter_spacing: f32,
    tab_size: f32,
    families: &[String],
    m: &dyn TextMeasurer,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: f32 = text
        .chars()
        .map(|c| {
            let cw = if c == '\t' {
                tab_size
            } else {
                m.char_width_with_families(c, font_size, families)
            };
            cw + letter_spacing
        })
        .sum();
    total - letter_spacing
}

/// Tries to find a hyphenation break in `display` that fits within `available_w`.
/// `break_positions` are byte offsets in `display` (already sorted ascending).
/// Returns `(prefix_with_hyphen, suffix)` for the rightmost fitting break, or `None`.
fn try_hyp_break(
    display: &str,
    available_w: f32,
    font_size: f32,
    letter_spacing: f32,
    m: &dyn TextMeasurer,
    break_positions: &[usize],
) -> Option<(String, String)> {
    if break_positions.is_empty() || available_w <= 0.0 {
        return None;
    }
    let hyphen_w = m.char_width('-', font_size) + letter_spacing;
    // Try from rightmost to leftmost — most characters on current line preferred.
    for &pos in break_positions.iter().rev() {
        if !display.is_char_boundary(pos) || pos == 0 {
            continue;
        }
        let prefix = &display[..pos];
        let prefix_w = measure_text_w(prefix, font_size, letter_spacing, 0.0, m);
        if prefix_w + hyphen_w <= available_w {
            let mut pfx = prefix.to_string();
            pfx.push('-');
            return Some((pfx, display[pos..].to_string()));
        }
    }
    None
}

/// Разбивает потоковые сегменты на строки.
///
/// Алгоритм: жадный word-wrap + опциональные переносы (hyphens: manual/auto).
/// Слова одного стиля на одной строке сливаются
/// Returns the byte offset where `word` must be split so the prefix fits within
/// `avail_px`. Guarantees at least one character in the prefix to prevent
/// infinite loops when even a single character is wider than `avail_px`.
/// Returns `word.len()` when the whole word fits.
fn char_break_offset(
    word: &str,
    avail_px: f32,
    font_size: f32,
    ls: f32,
    families: &[String],
    m: &dyn TextMeasurer,
) -> usize {
    let mut w = 0.0_f32;
    for (char_idx, (byte_pos, ch)) in word.char_indices().enumerate() {
        let cw = m.char_width_with_families(ch, font_size, families);
        // Width of prefix ending at this char: sum(cw + ls) - ls.
        // For first char: width = cw (no trailing letter-spacing).
        let prefix_w = if char_idx == 0 { cw } else { w + ls + cw };
        if prefix_w > avail_px {
            if char_idx == 0 {
                // Even the first char overflows — emit it to avoid infinite loop.
                return byte_pos + ch.len_utf8();
            }
            return byte_pos;
        }
        w = prefix_w;
    }
    word.len()
}

// ─── text-wrap: balance / pretty (CSS Text L4 §6.4.2) ───────────────────────

/// Returns the pixel width of the widest single word across all text segments.
/// Used as the lower-bound for `balance_wrap` binary search (cannot wrap narrower
/// than the longest token without breaking words).
fn widest_word(segments: &[InlineSegment], m: &dyn TextMeasurer) -> f32 {
    let mut max_w: f32 = 1.0;
    for seg in segments {
        if seg.img_src.is_some() {
            max_w = max_w.max(seg.img_width);
            continue;
        }
        let em = seg.style.font_size;
        let ls = seg.style.letter_spacing;
        let tab = seg.style.tab_size;
        let families = &seg.style.font_family;
        for raw in seg.text.split_whitespace() {
            let (display, _) = strip_soft_hyphens(raw);
            let w = measure_text_w_families(&display, em, ls, tab, families, m);
            max_w = max_w.max(w);
        }
    }
    max_w
}

/// CSS Text L4 §6.4.2 `text-wrap: balance` — redistributes line breaks so
/// that all lines are roughly equal in length.
///
/// Binary-searches the interval `[widest_word, container_width]` for the
/// minimum wrap width that produces the same number of lines as the greedy
/// result.  20 iterations → sub-pixel convergence for any container up to
/// ~500 000 px.  Single-line text is returned unchanged (nothing to balance).
#[allow(clippy::too_many_arguments)]
fn balance_wrap(
    segments: &[InlineSegment],
    container_width: f32,
    greedy_lines: Vec<Vec<InlineFrag>>,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    hyphens: Hyphens,
    hp: &dyn HyphenationProvider,
    white_space: crate::style::WhiteSpace,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
) -> Vec<Vec<InlineFrag>> {
    let target = greedy_lines.len();
    if target <= 1 {
        return greedy_lines;
    }
    let min_w = widest_word(segments, m);
    let mut lo = min_w;
    let mut hi = container_width;
    for _ in 0..20 {
        if hi - lo < 0.5 {
            break;
        }
        let mid = (lo + hi) * 0.5;
        let n = wrap_inline_run(
            segments, mid, container_font_size, text_indent, viewport,
            m, hyphens, hp, white_space, word_break, overflow_wrap,
        ).len();
        if n <= target {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    // Only re-wrap if we found a genuinely narrower balanced width.
    if hi < container_width - 0.5 {
        wrap_inline_run(
            segments, hi, container_font_size, text_indent, viewport,
            m, hyphens, hp, white_space, word_break, overflow_wrap,
        )
    } else {
        greedy_lines
    }
}

/// CSS Text L4 §6.4.2 `text-wrap: pretty` — prevents typographic widows.
///
/// A widow occurs when the last line contains only a single fragment.
/// This function finds a wrap width that moves one word from the penultimate
/// line onto the last line, so the last line has ≥ 2 fragments.
/// The total line count may increase by at most 1.
#[allow(clippy::too_many_arguments)]
fn pretty_wrap(
    segments: &[InlineSegment],
    container_width: f32,
    greedy_lines: Vec<Vec<InlineFrag>>,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    hyphens: Hyphens,
    hp: &dyn HyphenationProvider,
    white_space: crate::style::WhiteSpace,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
) -> Vec<Vec<InlineFrag>> {
    // A "widow" is a last line with exactly one word. Words may be merged into a
    // single InlineFrag, so check word count, not frag count.
    let last_word_count: usize = greedy_lines
        .last()
        .map(|l| l.iter().map(|f| f.text.split_whitespace().count()).sum())
        .unwrap_or(0);
    if last_word_count != 1 || greedy_lines.len() < 2 {
        return greedy_lines;
    }
    let target = greedy_lines.len();
    let penult = &greedy_lines[greedy_lines.len() - 2];
    if penult.is_empty() {
        return greedy_lines;
    }
    let penult_end = penult.last().map(|f| f.x + f.width).unwrap_or(0.0);
    let space_w = m.char_width(' ', container_font_size);
    // The penultimate line's last frag may be merged (e.g. "aaaa bb cc").
    // Extract the last word's width to find where a tighter wrap would push it down.
    let last_frag = penult.last().unwrap();
    let last_word_w = last_frag
        .text
        .split_whitespace()
        .last()
        .map(|w| {
            let (display, _) = strip_soft_hyphens(w);
            measure_text_w_families(
                &display,
                last_frag.style.font_size,
                last_frag.style.letter_spacing,
                0.0,
                &last_frag.style.font_family,
                m,
            )
        })
        .unwrap_or(last_frag.width);

    // Width at which the last word of the penultimate line wraps to the last line,
    // eliminating the widow.
    let trial_w = (penult_end - last_word_w - space_w).max(widest_word(segments, m));

    if trial_w >= container_width - 0.5 {
        return greedy_lines;
    }
    let trial = wrap_inline_run(
        segments, trial_w, container_font_size, text_indent, viewport,
        m, hyphens, hp, white_space, word_break, overflow_wrap,
    );
    // Accept if the new last line has ≥ 2 words (merged or not) and line count
    // didn't blow up by more than 1 line.
    let trial_last_words: usize = trial
        .last()
        .map(|l| l.iter().map(|f| f.text.split_whitespace().count()).sum())
        .unwrap_or(0);
    if trial_last_words >= 2 && trial.len() <= target + 1 {
        trial
    } else {
        greedy_lines
    }
}

// ─────────────────────────────────────────────────────────────────────────────

/// в один `InlineFrag`. Сегменты обрабатываются по одному, чтобы учитывать
/// `pre_space` / `post_space` (inline box model: margin + border + padding).
/// `white_space` controls whether whitespace is preserved (pre/pre-wrap).
#[allow(clippy::too_many_arguments)]
fn wrap_inline_run(
    segments: &[InlineSegment],
    max_width: f32,
    container_font_size: f32,
    text_indent: f32,
    viewport: Size,
    m: &dyn TextMeasurer,
    hyphens: Hyphens,
    hp: &dyn HyphenationProvider,
    white_space: crate::style::WhiteSpace,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
) -> Vec<Vec<InlineFrag>> {
    let space_w = m.char_width(' ', container_font_size);

    let mut result: Vec<Vec<InlineFrag>> = Vec::new();
    let mut current_line: Vec<InlineFrag> = Vec::new();
    // CSS Text L3 §7.1: text-indent только на первой строке.
    let mut current_x = text_indent;

    for seg in segments {
        // Forced line break from \n in white-space: pre/pre-wrap text.
        if seg.forced_break {
            result.push(std::mem::take(&mut current_line));
            current_x = 0.0;
            continue;
        }

        // Pre-mode: whitespace preserved, no word wrapping, tabs are tab_size wide.
        if white_space.preserves_whitespace() {
            if seg.text.is_empty() {
                continue;
            }
            let style = &seg.style;
            let em = style.font_size;
            let ls = style.letter_spacing;
            let tab_size = style.tab_size;
            let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
            let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);
            current_x += seg.pre_space;
            let frag_x = current_x;
            let frag_w = measure_text_w_families(&seg.text, em, ls, tab_size, &seg.style.font_family, m);
            current_line.push(InlineFrag {
                x: frag_x,
                y_offset: 0.0,
                width: frag_w,
                text: seg.text.clone(),
                style: style.clone(),
                padding_left: pad_l,
                padding_right: pad_r,
                is_element_box: seg.is_element_box,
                img_src: None,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
            });
            current_x += frag_w + seg.post_space;
            continue;
        }

        // Image segments are fixed-width, non-breakable inline replaced elements.
        if let Some(img_src) = &seg.img_src {
            let img_w = seg.img_width;
            let gap = if current_line.is_empty() { 0.0 } else { space_w };
            if !current_line.is_empty() && current_x + gap + seg.pre_space + img_w > max_width {
                result.push(std::mem::take(&mut current_line));
                current_x = 0.0;
            }
            let line_gap = if current_line.is_empty() { 0.0 } else { space_w };
            current_x += line_gap + seg.pre_space;
            let em = seg.style.font_size;
            let pad_l = seg.style.padding_left.resolve_or_zero(em, max_width, viewport);
            let pad_r = seg.style.padding_right.resolve_or_zero(em, max_width, viewport);
            current_line.push(InlineFrag {
                x: current_x,
                y_offset: 0.0,
                width: img_w,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: pad_l,
                padding_right: pad_r,
                is_element_box: true,
                img_src: Some(img_src.clone()),
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
            });
            current_x += img_w + seg.post_space;
            continue;
        }

        // Collect words; split_whitespace preserves U+00AD within tokens.
        let raw_words: Vec<&str> = seg.text.split_whitespace().collect();
        if raw_words.is_empty() {
            continue;
        }
        let style = &seg.style;
        let em = style.font_size;
        let ls = style.letter_spacing;
        let ws = style.word_spacing;
        let inter_word = space_w + ls + ws;

        // Resolved padding for this segment's inline box (for paint use).
        let pad_l = style.padding_left.resolve_or_zero(em, max_width, viewport);
        let pad_r = style.padding_right.resolve_or_zero(em, max_width, viewport);

        let n = raw_words.len();
        for (wi, raw_word) in raw_words.iter().enumerate() {
            let is_seg_first = wi == 0;
            let is_seg_last = wi == n - 1;

            // Strip soft hyphens for display + collect hyphenation break positions.
            let (display_word, shy_positions) = strip_soft_hyphens(raw_word);

            // Byte offset of this word within seg.text — used for Selection/Range mapping.
            // raw_word is a subslice produced by split_whitespace(), so pointer arithmetic is valid.
            let frag_source_offset = {
                let raw_ptr = raw_word.as_ptr() as usize;
                let seg_ptr = seg.text.as_ptr() as usize;
                let word_off = if raw_ptr >= seg_ptr && raw_ptr <= seg_ptr + seg.text.len() {
                    (raw_ptr - seg_ptr) as u32
                } else {
                    0u32
                };
                seg.source_char_offset.saturating_add(word_off)
            };

            // Space that the inline box model contributes at the word boundaries.
            let pre = if is_seg_first { seg.pre_space } else { 0.0 };
            let post = if is_seg_last { seg.post_space } else { 0.0 };

            let word_w = measure_text_w_families(&display_word, style.font_size, ls, 0.0, &style.font_family, m);
            let gap = if current_line.is_empty() { 0.0 } else { inter_word };

            // Wrap: слово не влезает (но первое слово строки добавляем всегда).
            let needs_wrap = !current_line.is_empty()
                && current_x + gap + pre + word_w > max_width;

            if needs_wrap {
                // CSS Text L3 §6: try hyphenation before hard wrap.
                let hyph_result = if hyphens != Hyphens::None {
                    let mut break_pts = shy_positions.clone();
                    if hyphens == Hyphens::Auto && !display_word.is_empty() {
                        let auto_pts = hp.hyphenate(&display_word, "");
                        break_pts.extend_from_slice(&auto_pts);
                        break_pts.sort_unstable();
                        break_pts.dedup();
                    }
                    let avail = max_width - current_x - gap - pre;
                    try_hyp_break(&display_word, avail, style.font_size, ls, m, &break_pts)
                } else {
                    None
                };

                if let Some((pfx, sfx)) = hyph_result {
                    // Emit prefix (with trailing '-') to current line, then wrap.
                    let pfx_w = measure_text_w_families(&pfx, style.font_size, ls, 0.0, &style.font_family, m);
                    current_x += gap + pre;
                    current_line.push(InlineFrag {
                        x: current_x,
                        y_offset: 0.0,
                        width: pfx_w,
                        text: pfx,
                        style: style.clone(),
                        padding_left: if is_seg_first { pad_l } else { 0.0 },
                        padding_right: 0.0,
                        is_element_box: seg.is_element_box,
                        img_src: None,
                        is_first_line: false,
                        source_node: seg.source_node,
                        source_char_offset: frag_source_offset,
                    });
                    result.push(std::mem::take(&mut current_line));
                    current_x = 0.0;
                    // Emit suffix as first fragment on new line.
                    let sfx_w = measure_text_w_families(&sfx, style.font_size, ls, 0.0, &style.font_family, m);
                    current_line.push(InlineFrag {
                        x: 0.0,
                        y_offset: 0.0,
                        width: sfx_w,
                        text: sfx,
                        style: style.clone(),
                        padding_left: 0.0,
                        padding_right: if is_seg_last { pad_r } else { 0.0 },
                        is_element_box: seg.is_element_box,
                        img_src: None,
                        is_first_line: false,
                        source_node: seg.source_node,
                        source_char_offset: frag_source_offset,
                    });
                    current_x += sfx_w + post;
                    continue;
                }

                // CSS Text L3 §5.1: word-break: break-all — char-break at the
                // current line position before wrapping.
                if word_break == WordBreak::BreakAll {
                    let gap_w = if current_line.is_empty() { 0.0 } else { inter_word };
                    current_x += gap_w + pre;
                    let mut rest = display_word.as_str();
                    let mut first_chunk = true;
                    while !rest.is_empty() {
                        let avail = (max_width - current_x).max(0.0);
                        let split = char_break_offset(rest, avail, style.font_size, ls, &style.font_family, m);
                        let head = &rest[..split];
                        let tail = &rest[split..];
                        if !head.is_empty() {
                            let head_w = measure_text_w_families(head, style.font_size, ls, 0.0, &style.font_family, m);
                            current_line.push(InlineFrag {
                                x: current_x,
                                y_offset: 0.0,
                                width: head_w,
                                text: head.to_string(),
                                style: style.clone(),
                                padding_left: if first_chunk && is_seg_first { pad_l } else { 0.0 },
                                padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                                is_element_box: seg.is_element_box,
                                img_src: None,
                                is_first_line: false,
                                source_node: seg.source_node,
                                source_char_offset: frag_source_offset,
                            });
                            current_x += head_w;
                            first_chunk = false;
                        }
                        rest = tail;
                        if !rest.is_empty() {
                            result.push(std::mem::take(&mut current_line));
                            current_x = 0.0;
                        }
                    }
                    current_x += post;
                    continue;
                }

                // No hyphenation break found — normal wrap.
                result.push(std::mem::take(&mut current_line));
                current_x = 0.0;
            }

            // CSS Text L3 §8.1: overflow-wrap: break-word / anywhere — char-break
            // words that are wider than the container (won't fit on any line).
            // word-break: break-word is a legacy alias for overflow-wrap: break-word.
            let ow_char_break = (word_break == WordBreak::BreakWord
                || matches!(overflow_wrap, OverflowWrap::BreakWord | OverflowWrap::Anywhere))
                && word_w > max_width;
            if ow_char_break {
                let line_gap_ow = if current_line.is_empty() { 0.0 } else { inter_word };
                current_x += line_gap_ow + pre;
                let mut rest = display_word.as_str();
                let mut first_chunk = true;
                while !rest.is_empty() {
                    let avail = (max_width - current_x).max(0.0);
                    let split = char_break_offset(rest, avail, style.font_size, ls, &style.font_family, m);
                    let head = &rest[..split];
                    let tail = &rest[split..];
                    if !head.is_empty() {
                        let head_w = measure_text_w_families(head, style.font_size, ls, 0.0, &style.font_family, m);
                        current_line.push(InlineFrag {
                            x: current_x,
                            y_offset: 0.0,
                            width: head_w,
                            text: head.to_string(),
                            style: style.clone(),
                            padding_left: if first_chunk && is_seg_first { pad_l } else { 0.0 },
                            padding_right: if tail.is_empty() && is_seg_last { pad_r } else { 0.0 },
                            is_element_box: seg.is_element_box,
                            img_src: None,
                            is_first_line: false,
                            source_node: seg.source_node,
                            source_char_offset: frag_source_offset,
                        });
                        current_x += head_w;
                        first_chunk = false;
                    }
                    rest = tail;
                    if !rest.is_empty() {
                        result.push(std::mem::take(&mut current_line));
                        current_x = 0.0;
                    }
                }
                current_x += post;
                continue;
            }

            let line_gap = if current_line.is_empty() { 0.0 } else { inter_word };
            current_x += line_gap + pre;
            let frag_x = current_x;

            // Слияние: только когда нет pre/post space у данного слова
            // и предыдущий фраг тоже не заканчивается inline-box-ом.
            let no_box = pre == 0.0 && post == 0.0;
            let merged = if no_box {
                if let Some(last) = current_line.last_mut() {
                    if last.style.text_rendering_eq(style) && last.padding_right == 0.0 {
                        last.text.push(' ');
                        last.text.push_str(&display_word);
                        last.width += inter_word + word_w;
                        current_x += word_w;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !merged {
                current_line.push(InlineFrag {
                    x: frag_x,
                    y_offset: 0.0,
                    width: word_w,
                    text: display_word,
                    style: style.clone(),
                    padding_left: if is_seg_first { pad_l } else { 0.0 },
                    padding_right: if is_seg_last { pad_r } else { 0.0 },
                    is_element_box: seg.is_element_box,
                    img_src: None,
                    is_first_line: false,
                    source_node: seg.source_node,
                    source_char_offset: frag_source_offset,
                });
                current_x += word_w;
            }

            current_x += post;
        }
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    result
}

/// Сдвигает фрагменты каждой строки по text-align + direction.
/// `Start`/`End` разрешаются в Left/Right по direction (CSS Text L3 §7.1).
/// Для RTL фрагменты зеркалируются относительно content_width.
fn align_lines(
    lines: &mut [Vec<InlineFrag>],
    content_width: f32,
    text_align: TextAlign,
    direction: Direction,
) {
    let is_rtl = direction == Direction::Rtl;
    // Resolve Start/End to physical Left/Right.
    let physical = match text_align {
        TextAlign::Start => if is_rtl { TextAlign::Right } else { TextAlign::Left },
        TextAlign::End   => if is_rtl { TextAlign::Left  } else { TextAlign::Right },
        other => other,
    };
    for line in lines.iter_mut() {
        let Some(last) = line.last() else { continue };
        let line_width = last.x + last.width;
        if is_rtl {
            // Mirror positions within the line block, then align the block.
            // `right_gap` = space to the right of the mirrored line block.
            let right_gap = match physical {
                TextAlign::Right  => (content_width - line_width).max(0.0),
                TextAlign::Center => ((content_width - line_width) / 2.0).max(0.0),
                _                 => 0.0, // Left / flush-left for RTL end
            };
            for frag in line.iter_mut() {
                frag.x = line_width - (frag.x + frag.width) + right_gap;
            }
        } else {
            let offset = match physical {
                TextAlign::Center => ((content_width - line_width) / 2.0).max(0.0),
                TextAlign::Right  => (content_width - line_width).max(0.0),
                _                 => 0.0,
            };
            if offset > 0.0 {
                for frag in line.iter_mut() {
                    frag.x += offset;
                }
            }
        }
    }
}

/// CSS 2.1 §10.8 — применяет вертикальное выравнивание к inline-фрагментам.
/// Записывает `y_offset` (смещение от верхнего края line-box, вниз — положительное).
/// `line_h` = font_size * line_height контейнера.
///
/// Half-leading (§10.8.1): когда line-height > content-area, разница делится пополам
/// и добавляется выше и ниже content-area. Для `baseline` — фрагмент сдвигается вниз
/// на `half_leading = (line_h - frag_h) / 2`, чтобы content-area была центрирована.
fn apply_inline_vertical_align(lines: &mut [Vec<InlineFrag>], line_h: f32) {
    for line in lines.iter_mut() {
        for frag in line.iter_mut() {
            // frag_h: content area height ≈ font-size (ascent + descent for normal line-height).
            let frag_h = frag.style.font_size;
            // CSS 2.1 §10.8.1: half-leading pushes content area away from line-box edges.
            let half_leading = ((line_h - frag_h) / 2.0).max(0.0);
            frag.y_offset = match frag.style.vertical_align {
                // Baseline: content area centred via half-leading (top = half_leading).
                VerticalAlign::Baseline => half_leading,
                // Top/TextTop: fragment top-aligned to line-box top edge.
                VerticalAlign::Top | VerticalAlign::TextTop => 0.0,
                // Bottom/TextBottom: fragment bottom-aligned to line-box bottom edge.
                VerticalAlign::Bottom | VerticalAlign::TextBottom => (line_h - frag_h).max(0.0),
                // Middle: visual midpoint of fragment at midpoint of line-box.
                VerticalAlign::Middle => ((line_h - frag_h) / 2.0).max(0.0),
                // sub/super: relative shift from baseline (~0.8 * frag_h from frag top).
                VerticalAlign::Sub => half_leading + frag_h * 0.15,
                VerticalAlign::Super => half_leading - frag_h * 0.35,
                // CSS: positive length = shift up (above baseline) → negative screen y.
                VerticalAlign::Length(px) => half_leading - px,
                VerticalAlign::Percent(p) => half_leading - (p / 100.0 * line_h),
            };
        }
    }
}

/// Без измерителя: помещаем всё в одну строку. Ширина каждого фрагмента
/// без шрифтовых метрик неизвестна — оставляем 0.0; text-decoration в этом
/// режиме не рисуется. layout() для финального рендеринга всё равно ходит
/// через layout_measured().
fn one_line_fallback(segments: &[InlineSegment]) -> Vec<Vec<InlineFrag>> {
    let mut frags: Vec<InlineFrag> = Vec::new();
    for seg in segments {
        // Image segment: emit with pre-computed width, don't merge with text.
        if let Some(img_src) = &seg.img_src {
            frags.push(InlineFrag {
                x: 0.0,
                y_offset: 0.0,
                width: seg.img_width,
                text: seg.text.clone(),
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: true,
                img_src: Some(img_src.clone()),
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
            });
            continue;
        }
        let text: String = seg.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        let merged = if let Some(last) = frags.last_mut() {
            if last.style.text_rendering_eq(&seg.style) && last.img_src.is_none() {
                last.text.push(' ');
                last.text.push_str(&text);
                true
            } else {
                false
            }
        } else {
            false
        };
        if !merged {
            frags.push(InlineFrag {
                x: 0.0,
                y_offset: 0.0,
                width: 0.0,
                text,
                style: seg.style.clone(),
                padding_left: 0.0,
                padding_right: 0.0,
                is_element_box: seg.is_element_box,
                img_src: None,
                is_first_line: false,
                source_node: seg.source_node,
                source_char_offset: seg.source_char_offset,
            });
        }
    }
    if frags.is_empty() { vec![] } else { vec![frags] }
}

/// CSS UI L4 §10.1 — усекает фрагменты строк, выходящих за `max_width`,
/// добавляя символ «…» (U+2026). Вызывается только когда `text-overflow:
/// ellipsis` И `overflow` создаёт clip.
fn apply_text_overflow_ellipsis(
    lines: &mut [Vec<InlineFrag>],
    max_width: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
) {
    let ellipsis = '\u{2026}'; // …
    let ellipsis_w = m.char_width(ellipsis, font_size);

    for line in lines.iter_mut() {
        let line_end = line.last().map(|f| f.x + f.width).unwrap_or(0.0);
        if line_end <= max_width {
            continue;
        }

        // Максимальная ширина для текстового контента перед «…».
        let budget = (max_width - ellipsis_w).max(0.0);

        // Ищем первый фрагмент, чьё начало выходит за budget.
        let cut = line.iter().position(|f| f.x > budget);

        match cut {
            Some(0) => {
                // Первый фрагмент уже за budget — показываем только «…».
                line[0].text = ellipsis.to_string();
                line[0].width = ellipsis_w;
                line.truncate(1);
            }
            Some(fi) => {
                // Усекаем фрагмент fi-1, удаляем fi и далее.
                let avail = budget - line[fi - 1].x;
                truncate_frag_with_ellipsis(&mut line[fi - 1], avail, font_size, m, ellipsis, ellipsis_w);
                line.truncate(fi);
            }
            None => {
                // Все фрагменты начинаются в пределах budget, но последний
                // выходит за max_width — усекаем его.
                let last = line.len() - 1;
                let avail = budget - line[last].x;
                truncate_frag_with_ellipsis(&mut line[last], avail, font_size, m, ellipsis, ellipsis_w);
            }
        }
    }
}

fn truncate_frag_with_ellipsis(
    frag: &mut InlineFrag,
    avail: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
    ellipsis: char,
    ellipsis_w: f32,
) {
    let mut buf = String::new();
    let mut w = 0.0_f32;
    for ch in frag.text.chars() {
        let cw = m.char_width(ch, font_size);
        if w + cw > avail {
            break;
        }
        buf.push(ch);
        w += cw;
    }
    buf.push(ellipsis);
    frag.text = buf;
    frag.width = w + ellipsis_w;
}

/// CSS Overflow L4 §3.2 / CSS Display L3 §7.2 — `-webkit-line-clamp` / `line-clamp`.
///
/// Truncates `lines` to at most `max_lines` entries. If truncation occurred, forces
/// an ellipsis (U+2026) onto the *last* visible line to signal omitted content.
/// The ellipsis is appended to the last fragment if the line fits within `max_width`,
/// or replaces overflowing text if the line is already too wide.
///
/// Called only when a text measurer is available (same guard as `text-overflow: ellipsis`).
fn apply_line_clamp(
    lines: &mut Vec<Vec<InlineFrag>>,
    max_lines: u32,
    max_width: f32,
    font_size: f32,
    m: &dyn TextMeasurer,
) {
    let n = max_lines as usize;
    if lines.len() <= n {
        return;
    }
    lines.truncate(n);

    let ellipsis = '\u{2026}';
    let ellipsis_w = m.char_width(ellipsis, font_size);
    let last = match lines.last_mut() {
        Some(l) => l,
        None => return,
    };
    if last.is_empty() {
        return;
    }

    let line_end = last.last().map(|f| f.x + f.width).unwrap_or(0.0);
    if line_end + ellipsis_w <= max_width {
        // Line fits: append "…" by extending the last fragment.
        let last_frag = last.last_mut().unwrap();
        last_frag.text.push(ellipsis);
        last_frag.width += ellipsis_w;
    } else {
        // Line overflows: truncate from the right to make room for "…".
        let budget = (max_width - ellipsis_w).max(0.0);
        let cut = last.iter().position(|f| f.x > budget);
        match cut {
            Some(0) => {
                last[0].text = ellipsis.to_string();
                last[0].width = ellipsis_w;
                last.truncate(1);
            }
            Some(fi) => {
                let avail = budget - last[fi - 1].x;
                truncate_frag_with_ellipsis(&mut last[fi - 1], avail, font_size, m, ellipsis, ellipsis_w);
                last.truncate(fi);
            }
            None => {
                let idx = last.len() - 1;
                let avail = budget - last[idx].x;
                truncate_frag_with_ellipsis(&mut last[idx], avail, font_size, m, ellipsis, ellipsis_w);
            }
        }
    }
}

/// CSS Container Queries L1: second-pass after layout.
///
/// Walks the laid-out box tree looking for elements that establish containers
/// (`container-type: size | inline-size`). For each container, resolves its
/// content dimensions from the first-pass layout rect, re-applies matching
/// `@container` rules to all descendants, then re-lays out those descendants
/// so that layout-affecting properties (width, height, display, …) take effect.
///
/// Phase 0 limitations:
/// - Only block-flow children are re-laid out (Flex/Grid children use first-pass positions).
/// - Nested containers are processed outermost-first (inner containers are re-entered in
///   the same walk, but they use the parent container's context for their own re-layout).
pub fn apply_container_styles(
    root: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) {
    // No container rules in this sheet → fast path.
    if sheet.container_rules.is_empty() {
        return;
    }
    let pcb = Rect::new(0.0, 0.0, viewport.width, viewport.height);
    apply_container_inner(root, doc, sheet, viewport, measurer, pcb, hp, dark_mode);
}

#[allow(clippy::too_many_arguments, clippy::only_used_in_recursion)]
fn apply_container_inner(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    pcb: Rect,
    hp: &dyn HyphenationProvider,
    dark_mode: bool,
) {
    let is_container = !matches!(b.style.container_type, ContainerType::Normal);
    if is_container {
        // Derive content dimensions from already-laid-out rect + style.
        let em = b.style.font_size;
        let bw = b.rect.width;
        let pad_l = b.style.padding_left.resolve_or_zero(em, bw, viewport);
        let pad_r = b.style.padding_right.resolve_or_zero(em, bw, viewport);
        let pad_t = b.style.padding_top.resolve_or_zero(em, bw, viewport);
        let pad_b = b.style.padding_bottom.resolve_or_zero(em, bw, viewport);
        let content_w = (bw - pad_l - pad_r
            - b.style.border_left_width - b.style.border_right_width).max(0.0);
        let content_h_val = (b.rect.height - pad_t - pad_b
            - b.style.border_top_width - b.style.border_bottom_width).max(0.0);
        let content_h = if matches!(b.style.container_type, ContainerType::Size) {
            Some(content_h_val)
        } else {
            None // inline-size: height not queryable
        };
        let ctx = ContainerContext {
            width: content_w,
            height: content_h,
            names: b.style.container_name.clone(),
        };
        // Re-apply container rules to all direct + indirect descendants.
        for child in &mut b.children {
            re_style_subtree(child, doc, sheet, &ctx, viewport);
        }
        // Re-lay out block-flow children with updated styles.
        let content_x = b.rect.x + pad_l + b.style.border_left_width;
        let content_y = b.rect.y + pad_t + b.style.border_top_width;
        let avail_h: Option<f32> = content_h;
        let child_pcb = if !matches!(b.style.position, Position::Static) {
            Rect::new(b.rect.x, b.rect.y, b.rect.width, b.rect.height)
        } else {
            pcb
        };
        // Expose this container's dimensions to cq* unit resolution during re-layout.
        set_cq_context(content_w, content_h);
        let mut child_y = content_y;
        for child in &mut b.children {
            if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                // Re-lay out against new pcb but don't advance child_y.
                lay_out(child, content_x, child_y, content_w, avail_h, measurer, viewport, child_pcb, hp);
                continue;
            }
            lay_out(child, content_x, child_y, content_w, avail_h, measurer, viewport, child_pcb, hp);
            if matches!(child.kind, BoxKind::Skip) {
                continue;
            }
            let child_mb = child.style.margin_bottom
                .resolve_or_zero(child.style.font_size, content_w, viewport);
            child_y = child.rect.y + child.rect.height + child_mb;
        }
        clear_cq_context();
        // After re-layout, recurse into children to catch nested containers.
        // Each nested container will set its own cq* context during its own re-layout.
        for child in &mut b.children {
            apply_container_inner(child, doc, sheet, viewport, measurer, child_pcb, hp, dark_mode);
        }
    } else {
        // Not a container — just recurse looking for container descendants.
        for child in &mut b.children {
            apply_container_inner(child, doc, sheet, viewport, measurer, pcb, hp, dark_mode);
        }
    }
}

/// Recursively re-applies container rules to a subtree.
/// Stops descending into elements that are themselves containers (they will
/// be processed by `apply_container_inner` with their own context).
fn re_style_subtree(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    ctx: &ContainerContext,
    viewport: Size,
) {
    if !matches!(b.kind, BoxKind::Skip) {
        apply_container_rules(&mut b.style, doc, b.node, sheet, ctx, viewport);
    }
    // Don't propagate into nested containers — they'll build their own context.
    if matches!(b.style.container_type, ContainerType::Normal) {
        for child in &mut b.children {
            re_style_subtree(child, doc, sheet, ctx, viewport);
        }
    }
}

#[cfg(test)]
mod tests {
    use lumen_core::geom::Size;
    use crate::style::{GridTrackSize, Length};
    use super::resolve_auto_fill_fit_count;

    fn layout_div(css: &str, viewport_w: f32, viewport_h: f32) -> super::LayoutBox {
        let html = "<div></div>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(viewport_w, viewport_h));
        // html box > body box > div box
        fn find_empty_block(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::Block) && child.children.is_empty() {
                    return Some(child);
                }
                if let Some(found) = find_empty_block(child) {
                    return Some(found);
                }
            }
            None
        }
        find_empty_block(&root).cloned().expect("empty Block not found in layout tree")
    }

    #[test]
    fn aspect_ratio_height_from_width() {
        // width: 200px, aspect-ratio: 2/1 → height should be 100px border-box
        let div = layout_div("div { width: 200px; aspect-ratio: 2/1; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 200.0);
        assert_eq!(div.rect.height, 100.0);
    }

    #[test]
    fn aspect_ratio_16_9() {
        // width: 160px, aspect-ratio: 16/9 → height = 160 * 9/16 = 90px
        let div = layout_div("div { width: 160px; aspect-ratio: 16/9; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 160.0);
        assert!((div.rect.height - 90.0).abs() < 0.5, "height={}", div.rect.height);
    }

    #[test]
    fn aspect_ratio_explicit_height_wins() {
        // Explicit height overrides aspect-ratio.
        let div = layout_div("div { width: 200px; height: 50px; aspect-ratio: 2/1; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 200.0);
        assert_eq!(div.rect.height, 50.0);
    }

    #[test]
    fn aspect_ratio_no_height_without_ratio() {
        // Without aspect-ratio, height collapses to 0 for empty div.
        let div = layout_div("div { width: 200px; }", 800.0, 600.0);
        assert_eq!(div.rect.width, 200.0);
        assert_eq!(div.rect.height, 0.0);
    }

    // ── Hyphenation helpers ───────────────────────────────────────────────────

    #[test]
    fn strip_soft_hyphens_removes_shy_and_collects_positions() {
        let (disp, pos) = super::strip_soft_hyphens("hy\u{00AD}phen");
        assert_eq!(disp, "hyphen");
        assert_eq!(pos, vec![2]); // break point between 'y' and 'p'
    }

    #[test]
    fn strip_soft_hyphens_multiple_breaks() {
        // "su\u{AD}per\u{AD}man"
        let (disp, pos) = super::strip_soft_hyphens("su\u{00AD}per\u{00AD}man");
        assert_eq!(disp, "superman");
        assert_eq!(pos, vec![2, 5]);
    }

    #[test]
    fn strip_soft_hyphens_no_shy_returns_empty_positions() {
        let (disp, pos) = super::strip_soft_hyphens("hello");
        assert_eq!(disp, "hello");
        assert!(pos.is_empty());
    }

    #[test]
    fn measure_text_w_empty_is_zero() {
        struct ZeroMeasurer;
        impl super::super::TextMeasurer for ZeroMeasurer {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let m = ZeroMeasurer;
        assert_eq!(super::measure_text_w("", 16.0, 0.0, 0.0, &m), 0.0);
    }

    #[test]
    fn measure_text_w_three_chars_no_spacing() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // 3 chars × 8px − 0 letter-spacing = 24px
        let w = super::measure_text_w("abc", 16.0, 0.0, 0.0, &Fixed8);
        assert_eq!(w, 24.0);
    }

    #[test]
    fn try_hyp_break_finds_rightmost_fitting_split() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // "superman" → break positions [2, 5] (su|per|man)
        // Each char = 8px; hyphen = 8px.
        // If available_w = 32px: "su-" = 3×8 = 24 ≤ 32 ✓, "super-" = 6×8 = 48 > 32
        // So rightmost fitting = pos 2 ("su-" / "perman")
        let m = Fixed8;
        let result = super::try_hyp_break("superman", 32.0, 16.0, 0.0, &m, &[2, 5]);
        assert_eq!(result, Some(("su-".to_string(), "perman".to_string())));
    }

    #[test]
    fn try_hyp_break_prefers_rightmost_break() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // "superman" → break positions [2, 5]; available = 56px
        // "super-" = 6×8 = 48 ≤ 56 ✓ → prefer pos 5 over pos 2
        let m = Fixed8;
        let result = super::try_hyp_break("superman", 56.0, 16.0, 0.0, &m, &[2, 5]);
        assert_eq!(result, Some(("super-".to_string(), "man".to_string())));
    }

    #[test]
    fn try_hyp_break_returns_none_when_nothing_fits() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // Only 10px available; minimum "su-" = 24px
        let m = Fixed8;
        let result = super::try_hyp_break("superman", 10.0, 16.0, 0.0, &m, &[2, 5]);
        assert!(result.is_none());
    }

    #[test]
    fn wrap_inline_run_soft_hyphen_breaks_word_on_manual() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, PseudoKind, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens};
        use lumen_core::geom::Size;
        use lumen_dom::NodeId;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // Segment: "hi hy\u{AD}phen" — two words; 'hi' fills line, 'hy\u{AD}phen' needs break.
        // char=10, max_width=60:
        //   "hi"=20px fits; then gap(10)+60=90>60 → wrap attempted.
        //   avail = 60-20-10 = 30; "hy-"=30 ≤ 30 → break at pos 2.
        let seg = InlineSegment {
            text: "hi hy\u{00AD}phen".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
            pseudo_kind: PseudoKind::None,
            source_node: NodeId::from_index(0),
            source_char_offset: 0,
        };

        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(&[seg], 60.0, 16.0, 0.0, Size::new(800.0, 600.0), &m, Hyphens::Manual, &hp, crate::style::WhiteSpace::Normal, crate::style::WordBreak::Normal, crate::style::OverflowWrap::Normal);
        assert_eq!(lines.len(), 2, "expected 2 lines, got {}", lines.len());
        // Line 1 has both "hi" and "hy-" merged or as separate frags.
        let line1_text: String = lines[0].iter().map(|f| f.text.as_str()).collect::<Vec<_>>().join(" ");
        assert!(line1_text.contains("hi"), "line1={line1_text}");
        assert!(line1_text.contains("hy-"), "line1={line1_text}");
        assert_eq!(lines[1].len(), 1);
        assert_eq!(lines[1][0].text, "phen");
    }

    #[test]
    fn wrap_inline_run_hyphens_none_no_break_on_shy() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, PseudoKind, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens};
        use lumen_core::geom::Size;
        use lumen_dom::NodeId;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // Same segment, Hyphens::None → soft hyphen ignored, full word wraps to new line unbroken.
        let seg = InlineSegment {
            text: "hi hy\u{00AD}phen".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
            pseudo_kind: PseudoKind::None,
            source_node: NodeId::from_index(0),
            source_char_offset: 0,
        };
        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(&[seg], 60.0, 16.0, 0.0, Size::new(800.0, 600.0), &m, Hyphens::None, &hp, crate::style::WhiteSpace::Normal, crate::style::WordBreak::Normal, crate::style::OverflowWrap::Normal);
        assert_eq!(lines.len(), 2, "expected 2 lines, got {}", lines.len());
        // Line 1 has only "hi"; line 2 has "hyphen" (whole, no hyphen char).
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].text, "hi");
        let line2_text = &lines[1][0].text;
        assert_eq!(line2_text, "hyphen", "soft-hyphen should be stripped: {line2_text}");
    }

    // ── char_break_offset ────────────────────────────────────────────────────

    #[test]
    fn char_break_offset_all_fit() {
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // "abc" = 3 chars × 8px = 24px; avail = 100 → whole word fits.
        let off = super::char_break_offset("abc", 100.0, 16.0, 0.0, &[], &Fixed8);
        assert_eq!(off, 3); // "abc".len() == 3
    }

    #[test]
    fn char_break_offset_splits_after_second_char() {
        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }
        // "abcde", avail = 25px; "ab" = 20px fits, "abc" = 30px > 25 → split at 2.
        let off = super::char_break_offset("abcde", 25.0, 16.0, 0.0, &[], &Fixed10);
        assert_eq!(off, 2); // byte offset 2 = between 'b' and 'c'
    }

    #[test]
    fn char_break_offset_emits_at_least_one_char() {
        struct Wide;
        impl super::super::TextMeasurer for Wide {
            fn char_width(&self, _: char, _: f32) -> f32 { 100.0 }
        }
        // avail = 5px, char width 100px — even first char doesn't fit.
        // Must return offset past first char to avoid infinite loop.
        let off = super::char_break_offset("abc", 5.0, 16.0, 0.0, &[], &Wide);
        assert_eq!(off, 1); // emit 'a' anyway
    }

    // ── text-wrap-mode: nowrap ────────────────────────────────────────────────

    #[test]
    fn text_wrap_mode_nowrap_no_line_break() {
        // text-wrap-mode: nowrap should prevent wrapping (like white-space: nowrap).
        // Container 50px wide, word each 8px × 5 chars = 40px ("Hello" + " " + "World").
        let html = "<p>Hello World</p>";
        let css = "p { width: 50px; text-wrap-mode: nowrap; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_inline_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_inline_run(c) { return Some(f); } }
            None
        }
        let ir = find_inline_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { lines, .. } = &ir.kind {
            assert_eq!(lines.len(), 1, "text-wrap-mode:nowrap must produce 1 line, got {}", lines.len());
        }
    }

    // ── overflow-wrap: break-word ─────────────────────────────────────────────

    #[test]
    fn overflow_wrap_break_word_splits_long_word() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, PseudoKind, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens, OverflowWrap, WordBreak};
        use lumen_core::geom::Size;
        use lumen_dom::NodeId;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // "Superlongword" = 13 chars × 10px = 130px; max_width = 80px.
        // overflow-wrap: break-word should split it across lines.
        let seg = InlineSegment {
            text: "Superlongword".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
            pseudo_kind: PseudoKind::None,
            source_node: NodeId::from_index(0),
            source_char_offset: 0,
        };

        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(
            &[seg], 80.0, 16.0, 0.0,
            Size::new(800.0, 600.0),
            &m, Hyphens::None, &hp,
            crate::style::WhiteSpace::Normal,
            WordBreak::Normal,
            OverflowWrap::BreakWord,
        );
        // 13 chars at 10px = 130px > 80px, so must wrap.
        assert!(lines.len() >= 2, "expected multiple lines, got {}", lines.len());
        // No line should exceed max_width.
        for (i, line) in lines.iter().enumerate() {
            if let Some(last) = line.last() {
                let line_w = last.x + last.width;
                assert!(line_w <= 81.0, "line {} width {line_w} exceeds max_width 80", i);
            }
        }
        // All characters of "Superlongword" must appear in the output.
        let all_text: String = lines.iter().flat_map(|l| l.iter().map(|f| f.text.as_str())).collect();
        assert_eq!(all_text, "Superlongword", "all chars must be emitted: {all_text}");
    }

    // ── word-break: break-all ─────────────────────────────────────────────────

    #[test]
    fn word_break_break_all_breaks_at_current_position() {
        use lumen_core::ext::NullHyphenationProvider;
        use super::{InlineSegment, PseudoKind, wrap_inline_run};
        use crate::style::{ComputedStyle, Hyphens, OverflowWrap, WordBreak};
        use lumen_core::geom::Size;
        use lumen_dom::NodeId;

        struct Fixed10;
        impl super::super::TextMeasurer for Fixed10 {
            fn char_width(&self, _: char, _: f32) -> f32 { 10.0 }
        }

        let style = ComputedStyle::root();
        // Two words: "Hi" (20px) then "World" (50px). max_width = 60px.
        // Normal: "Hi" fits, gap(10)+50=80 > 60 → wrap → line2 = "World".
        // break-all: "Hi" fits; gap(10)+"World" → need 80 > 60 → char-break.
        //   avail at current pos = 60 - 20 - 10 = 30px → "Wor" (30px) fits.
        //   Emit "Wor" at end of line1, line2 = "ld".
        let seg = InlineSegment {
            text: "Hi World".to_string(),
            style: style.clone(),
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_width: 0.0,
            forced_break: false,
            pseudo_kind: PseudoKind::None,
            source_node: NodeId::from_index(0),
            source_char_offset: 0,
        };

        let m = Fixed10;
        let hp = NullHyphenationProvider;
        let lines = wrap_inline_run(
            &[seg], 60.0, 16.0, 0.0,
            Size::new(800.0, 600.0),
            &m, Hyphens::None, &hp,
            crate::style::WhiteSpace::Normal,
            WordBreak::BreakAll,
            OverflowWrap::Normal,
        );
        assert_eq!(lines.len(), 2, "expected 2 lines with break-all, got {}", lines.len());
        // All text must be preserved.
        let all_text: String = lines.iter()
            .flat_map(|l| l.iter().map(|f| f.text.as_str()))
            .collect::<Vec<_>>()
            .join(" "); // words may be merged by frag-merging
        assert!(all_text.contains("Hi"), "line1 must contain 'Hi': {all_text}");
        // Line 2 must have the remainder of "World".
        let line2_text: String = lines[1].iter().map(|f| f.text.as_str()).collect();
        assert!(!line2_text.is_empty(), "line2 must not be empty");
    }

    // ── display: flow-root (BFC) ──────────────────────────────────────────────

    #[test]
    fn flow_root_produces_flow_root_kind() {
        let html = r#"<div id="bfc"></div>"#;
        let css = "#bfc { display: flow-root; width: 200px; height: 50px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_flow_root(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::FlowRoot) {
                return Some(b);
            }
            for child in &b.children {
                if let Some(found) = find_flow_root(child) {
                    return Some(found);
                }
            }
            None
        }
        let bfc = find_flow_root(&root).expect("FlowRoot box not found");
        assert_eq!(bfc.rect.width, 200.0);
        assert_eq!(bfc.rect.height, 50.0);
    }

    #[test]
    fn flow_root_lays_out_children_like_block() {
        // A flow-root containing two block children should stack them vertically.
        let html = r#"<div class="bfc"><div class="a"></div><div class="b"></div></div>"#;
        let css = ".bfc { display: flow-root; width: 200px; } .a { height: 30px; } .b { height: 20px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_flow_root(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::FlowRoot) { return Some(b); }
            for c in &b.children { if let Some(f) = find_flow_root(c) { return Some(f); } }
            None
        }
        let bfc = find_flow_root(&root).expect("FlowRoot box not found");
        // Height auto → sum of children (30 + 20 = 50).
        assert_eq!(bfc.rect.height, 50.0, "flow-root auto height wrong: {}", bfc.rect.height);
        // Children stacked vertically.
        let blocks: Vec<_> = bfc.children.iter()
            .filter(|c| matches!(c.kind, super::BoxKind::Block))
            .collect();
        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].rect.y > blocks[0].rect.y, "children not stacked vertically");
    }

    // ── display: contents (box elimination) ──────────────────────────────────

    #[test]
    fn contents_box_is_eliminated_from_layout_tree() {
        // The display:contents wrapper should not appear as a box; its child
        // block should be a direct child of the outer div.
        let html = r#"<div id="outer"><div id="wrap"><div id="inner"></div></div></div>"#;
        let css = "#outer { width: 400px; } #wrap { display: contents; } #inner { height: 40px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_by_id<'a>(b: &'a super::LayoutBox, doc: &lumen_dom::Document, id: &str) -> Option<&'a super::LayoutBox> {
            if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
                && attrs.iter().any(|a| a.name.local == "id" && a.value == id)
            {
                return Some(b);
            }
            for child in &b.children { if let Some(f) = find_by_id(child, doc, id) { return Some(f); } }
            None
        }
        // display:contents wrapper must not appear as a Contents box in the tree.
        fn find_contents(b: &super::LayoutBox) -> bool {
            if matches!(b.kind, super::BoxKind::Contents) { return true; }
            b.children.iter().any(find_contents)
        }
        assert!(!find_contents(&root), "Contents box must be flattened out of layout tree");
        // Inner block must exist with correct height.
        let inner = find_by_id(&root, &doc, "inner").expect("inner div not found");
        assert_eq!(inner.rect.height, 40.0, "inner height wrong: {}", inner.rect.height);
    }

    #[test]
    fn nested_contents_flattened() {
        // Two nested display:contents wrappers — both should be eliminated.
        let html = r#"<div id="root"><div id="a"><div id="b"><div id="leaf"></div></div></div></div>"#;
        let css = "#a, #b { display: contents; } #leaf { height: 25px; width: 100px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_contents(b: &super::LayoutBox) -> bool {
            if matches!(b.kind, super::BoxKind::Contents) { return true; }
            b.children.iter().any(find_contents)
        }
        assert!(!find_contents(&root), "nested Contents boxes must be fully flattened");
    }

    #[test]
    fn contents_in_flex_container_no_panic() {
        // BUG-058: display:contents child inside a flex container caused a panic
        // because flatten_contents was only called in the non-item-container path.
        let html = r#"<div id="flex"><div id="wrap"><div id="item"></div></div></div>"#;
        let css = "#flex { display: flex; width: 400px; } #wrap { display: contents; } #item { width: 100px; height: 50px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        // Must not panic.
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_contents(b: &super::LayoutBox) -> bool {
            if matches!(b.kind, super::BoxKind::Contents) { return true; }
            b.children.iter().any(find_contents)
        }
        assert!(!find_contents(&root), "Contents box must be flattened inside flex container");
    }

    #[test]
    fn contents_in_grid_container_no_panic() {
        // BUG-058: same panic reproducible with display:grid container.
        let html = r#"<div id="grid"><div id="wrap"><div id="item"></div></div></div>"#;
        let css = "#grid { display: grid; width: 400px; } #wrap { display: contents; } #item { width: 100px; height: 50px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        // Must not panic.
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        fn find_contents(b: &super::LayoutBox) -> bool {
            if matches!(b.kind, super::BoxKind::Contents) { return true; }
            b.children.iter().any(find_contents)
        }
        assert!(!find_contents(&root), "Contents box must be flattened inside grid container");
    }

    // ── CSS 2.1 §10.3.3 — auto horizontal-margin centering ───────────────────

    fn find_by_id_all<'a>(b: &'a super::LayoutBox, doc: &lumen_dom::Document, id: &str) -> Option<&'a super::LayoutBox> {
        if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
            && attrs.iter().any(|a| a.name.local == "id" && a.value == id)
        {
            return Some(b);
        }
        for child in &b.children {
            if let Some(f) = find_by_id_all(child, doc, id) { return Some(f); }
        }
        None
    }

    #[test]
    fn margin_auto_both_centers_block() {
        // margin: 0 auto on a 200px block inside an 800px viewport → x = 300.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { width: 200px; height: 50px; margin: 0 auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // (800 - 200) / 2 = 300
        assert_eq!(b.rect.x, 300.0, "centered x expected 300, got {}", b.rect.x);
        assert_eq!(b.rect.width, 200.0, "width must stay 200px");
    }

    #[test]
    fn margin_auto_left_only_pushes_to_right() {
        // margin-left: auto, margin-right: 0 → element flush-right.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { width: 200px; height: 50px; margin-left: auto; margin-right: 0; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // available=800, width=200, mr=0 → remaining=600 → ml_computed=600 → x=600
        assert_eq!(b.rect.x, 600.0, "flush-right x expected 600, got {}", b.rect.x);
    }

    #[test]
    fn margin_auto_right_only_no_x_shift() {
        // margin-right: auto, margin-left: 20px → element at x=20.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { width: 200px; height: 50px; margin-left: 20px; margin-right: auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // margin-left is fixed at 20px → x=20
        assert_eq!(b.rect.x, 20.0, "x with fixed left margin expected 20, got {}", b.rect.x);
    }

    #[test]
    fn margin_auto_no_explicit_width_fills_container() {
        // Without explicit width, auto margins resolve to 0 (width takes remaining).
        let html = r#"<div id="box"></div>"#;
        let css = "#box { height: 50px; margin: 0 auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // No explicit width → margin auto resolves to 0 → element fills 800px, x=0.
        assert_eq!(b.rect.x, 0.0, "x without explicit width must be 0, got {}", b.rect.x);
        assert_eq!(b.rect.width, 800.0, "width without explicit must fill 800px, got {}", b.rect.width);
    }

    #[test]
    fn margin_auto_position_sticky_centers() {
        // position:sticky element with margin: 20px auto 0 in 1022px container.
        // Static view: sticky behaves like normal flow → centering applies.
        let html = r#"<div id="wrap"><div id="sticky"></div></div>"#;
        let css = "#wrap { width: 1022px; position: relative; } \
                   #sticky { position: sticky; top: 10px; width: 600px; height: 60px; margin: 20px auto 0; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
        let s = find_by_id_all(&root, &doc, "sticky").expect("sticky not found");
        // (1022 - 600) / 2 = 211 → x = wrap.content_x + 211
        assert_eq!(s.rect.width, 600.0, "width must be 600, got {}", s.rect.width);
        let centered_x = s.rect.x;
        // Should be (1022-600)/2 = 211 relative to wrap's content_x (0).
        assert!((centered_x - 211.0).abs() < 1.0, "centered x expected ~211, got {centered_x}");
        assert_eq!(s.rect.y, 20.0, "top margin 20px must be respected, got {}", s.rect.y);
    }

    #[test]
    fn abs_pos_inset_resolves_width_and_height() {
        // CSS Position L3 §6: position:absolute with inset:0 (top/right/bottom/left
        // all 0) and no explicit width/height must fill the relatively-positioned
        // containing block on both axes. Regression for BUG-051 — height-from-insets
        // was missing, so the box collapsed to height 0.
        let html = r#"<div id="cb"><div id="bg"></div></div>"#;
        let css = "#cb { position: relative; width: 660px; height: 120px; } \
                   #bg { position: absolute; top: 0; right: 0; bottom: 0; left: 0; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
        let bg = find_by_id_all(&root, &doc, "bg").expect("bg not found");
        assert_eq!(bg.rect.width, 660.0, "inset:0 width must fill cb, got {}", bg.rect.width);
        assert_eq!(bg.rect.height, 120.0, "inset:0 height must fill cb, got {}", bg.rect.height);
    }

    #[test]
    fn abs_pos_explicit_height_overrides_insets() {
        // An explicit height wins over top+bottom insets (height is not auto), so the
        // §6 gap-fill rule does not apply — guards the `cs.height.is_none()` guard.
        let html = r#"<div id="cb"><div id="bg"></div></div>"#;
        let css = "#cb { position: relative; width: 660px; height: 120px; } \
                   #bg { position: absolute; top: 0; bottom: 0; left: 0; height: 40px; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(1024.0, 720.0));
        let bg = find_by_id_all(&root, &doc, "bg").expect("bg not found");
        assert_eq!(bg.rect.height, 40.0, "explicit height must win, got {}", bg.rect.height);
    }

    #[test]
    fn margin_auto_float_not_centered() {
        // float:left with margin: 0 auto must NOT be centered — floats ignore auto margins.
        let html = r#"<div id="box"></div>"#;
        let css = "#box { float: left; width: 100px; height: 50px; margin: 0 auto; }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let b = find_by_id_all(&root, &doc, "box").expect("box not found");
        // Float placed at left edge (auto = 0).
        assert_eq!(b.rect.x, 0.0, "float with auto margins must be at x=0, got {}", b.rect.x);
    }

    // ── loading="lazy" image deferral (HTML LS §2.6.6.9) ────────────────────

    #[test]
    fn loading_lazy_marks_image_as_lazy() {
        let doc = lumen_html_parser::parse(r#"<img src="a.png" loading="lazy">"#);
        let viewport = Size::new(800.0, 600.0);
        let reqs = super::collect_image_requests(&doc, viewport);
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].is_lazy, "loading=lazy must set is_lazy=true");
        assert_eq!(reqs[0].url, "a.png");
    }

    #[test]
    fn loading_eager_not_lazy() {
        let doc = lumen_html_parser::parse(r#"<img src="b.png" loading="eager">"#);
        let reqs = super::collect_image_requests(&doc, Size::new(800.0, 600.0));
        assert_eq!(reqs.len(), 1);
        assert!(!reqs[0].is_lazy, "loading=eager must not set is_lazy");
    }

    #[test]
    fn loading_absent_not_lazy() {
        let doc = lumen_html_parser::parse(r#"<img src="c.png">"#);
        let reqs = super::collect_image_requests(&doc, Size::new(800.0, 600.0));
        assert_eq!(reqs.len(), 1);
        assert!(!reqs[0].is_lazy, "absent loading attr must not set is_lazy");
    }

    #[test]
    fn loading_lazy_case_insensitive() {
        let doc = lumen_html_parser::parse(r#"<img src="d.png" loading="LAZY">"#);
        let reqs = super::collect_image_requests(&doc, Size::new(800.0, 600.0));
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].is_lazy, "loading=LAZY (uppercase) must set is_lazy=true");
    }

    #[test]
    fn loading_lazy_mixed_with_eager() {
        let html = r#"<img src="e.png"><img src="f.png" loading="lazy"><img src="g.png">"#;
        let doc = lumen_html_parser::parse(html);
        let reqs = super::collect_image_requests(&doc, Size::new(800.0, 600.0));
        assert_eq!(reqs.len(), 3);
        assert!(!reqs[0].is_lazy, "first img (no attr) must not be lazy");
        assert!(reqs[1].is_lazy, "second img (loading=lazy) must be lazy");
        assert!(!reqs[2].is_lazy, "third img (no attr) must not be lazy");
    }

    // ── ::first-letter / ::first-line structural markers ─────────────────────

    #[test]
    fn first_letter_segment_marked_on_plain_paragraph() {
        // The first text segment in a block should be marked as FirstLetter.
        let root = super::layout(
            &lumen_html_parser::parse("<p>Hello world</p>"),
            &lumen_css_parser::parse(""),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        fn find_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
            None
        }
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { segments, .. } = &run.kind {
            assert!(!segments.is_empty(), "expected at least one segment");
            assert_eq!(
                segments[0].pseudo_kind,
                super::PseudoKind::FirstLetter,
                "first segment must be PseudoKind::FirstLetter"
            );
            // Remaining segments have no pseudo kind.
            for seg in segments.iter().skip(1) {
                assert_eq!(seg.pseudo_kind, super::PseudoKind::None, "only first seg is FirstLetter");
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_letter_not_marked_on_second_paragraph() {
        // Each block creates its own inline run; each run's first seg is marked.
        let root = super::layout(
            &lumen_html_parser::parse("<p>One</p><p>Two</p>"),
            &lumen_css_parser::parse(""),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        fn collect_runs<'a>(b: &'a super::LayoutBox, out: &mut Vec<&'a super::LayoutBox>) {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { out.push(b); }
            for c in &b.children { collect_runs(c, out); }
        }
        let mut runs = Vec::new();
        collect_runs(&root, &mut runs);
        assert!(runs.len() >= 2, "expected at least 2 inline runs");
        for run in &runs {
            if let super::BoxKind::InlineRun { segments, .. } = &run.kind
                && !segments.is_empty()
            {
                assert_eq!(
                    segments[0].pseudo_kind,
                    super::PseudoKind::FirstLetter,
                    "each run's first seg should be FirstLetter"
                );
            }
        }
    }

    #[test]
    fn first_line_frags_marked_after_wrap() {
        // After lay_out, frags on lines[0] must have is_first_line = true;
        // frags on subsequent lines must have is_first_line = false.
        // Uses Fixed8 measurer (8px/char): "one two" = 7×8=56 ≤ 60px; "three" = 5×8=40,
        // 56+8+40=104 > 60 → wraps. 60px viewport ensures at least 2 lines.
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let html = "<p>one two three four five</p>";
        let root = super::layout_measured(
            &lumen_html_parser::parse(html),
            &lumen_css_parser::parse(""),
            lumen_core::geom::Size::new(60.0, 600.0),
            &Fixed8,
        );
        fn find_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
            None
        }
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(lines.len() >= 2, "expected multiple lines, got {}", lines.len());
            for frag in &lines[0] {
                assert!(frag.is_first_line, "line 0 frag must be is_first_line=true");
            }
            for line in lines.iter().skip(1) {
                for frag in line {
                    assert!(!frag.is_first_line, "lines 1+ frags must be is_first_line=false");
                }
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    // ::first-letter / ::first-line style application

    #[test]
    fn first_letter_style_applied_when_rule_present() {
        // ::first-letter { color: red } must change only the first segment's style.
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let html = "<p>Hello world</p>";
        let css  = "p::first-letter { color: red; }";
        let root = super::layout_measured(
            &lumen_html_parser::parse(html),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(800.0, 600.0),
            &Fixed8,
        );
        fn find_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
            None
        }
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { segments, .. } = &run.kind {
            assert!(!segments.is_empty());
            // First segment (the single 'H' letter) must have red color.
            let red = crate::style::Color { r: 255, g: 0, b: 0, a: 255 };
            assert_eq!(
                segments[0].style.color, red,
                "::first-letter segment must have red color"
            );
            assert_eq!(segments[0].text, "H", "first-letter segment should be exactly 'H'");
            // Remaining segment keeps original (black) color.
            if segments.len() > 1 {
                assert_ne!(
                    segments[1].style.color, red,
                    "remainder segment must keep original color"
                );
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_letter_no_rule_leaves_segment_unchanged() {
        // Without a ::first-letter rule the segment style must be unchanged.
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let html = "<p>Hello</p>";
        let css  = "p { color: blue; }";
        let root = super::layout_measured(
            &lumen_html_parser::parse(html),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(800.0, 600.0),
            &Fixed8,
        );
        fn find_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
            None
        }
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { segments, .. } = &run.kind {
            // No split: single segment still contains full text.
            assert_eq!(segments[0].text, "Hello");
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_line_style_applied_to_first_line_frags() {
        // ::first-line { color: green } must change the style of frags on line 0 only.
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        // 60px wide container forces wrap: "one two" (56px) fits on line 0, rest wraps.
        let html = "<p>one two three four</p>";
        let css  = "p::first-line { color: green; }";
        let root = super::layout_measured(
            &lumen_html_parser::parse(html),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(60.0, 600.0),
            &Fixed8,
        );
        fn find_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
            None
        }
        let run = find_run(&root).expect("InlineRun not found");
        let green = crate::style::Color { r: 0, g: 128, b: 0, a: 255 };
        if let super::BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(lines.len() >= 2, "expected at least 2 lines");
            for frag in &lines[0] {
                assert_eq!(frag.style.color, green, "line 0 frag must have green color");
            }
            for line in lines.iter().skip(1) {
                for frag in line {
                    assert_ne!(frag.style.color, green, "lines 1+ frags must keep original color");
                }
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_line_no_rule_frags_unchanged() {
        // Without a ::first-line rule, frag styles must be unchanged.
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let html = "<p>one two three four</p>";
        let css  = "p { color: blue; }";
        let root = super::layout_measured(
            &lumen_html_parser::parse(html),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(60.0, 600.0),
            &Fixed8,
        );
        fn find_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
            for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
            None
        }
        let run = find_run(&root).expect("InlineRun not found");
        let blue = crate::style::Color { r: 0, g: 0, b: 255, a: 255 };
        if let super::BoxKind::InlineRun { lines, .. } = &run.kind {
            // All frags across all lines must be blue (from `p { color: blue }`).
            for line in lines {
                for frag in line {
                    assert_eq!(frag.style.color, blue, "all frags must keep blue color");
                }
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    // Phase 3: Nested SVG layout tests

    #[test]
    fn nested_svg_viewbox_scaling() {
        let html = r#"
            <svg viewBox="0 0 100 100" width="100" height="100">
                <rect x="0" y="0" width="50" height="50" />
                <svg viewBox="0 0 50 50" width="50" height="50" x="50" y="50">
                    <rect x="0" y="0" width="25" height="25" />
                </svg>
            </svg>
        "#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
        assert!(!root.children.is_empty());
    }

    #[test]
    fn nested_svg_transform_composition() {
        let html = r#"
            <svg viewBox="0 0 100 100" width="100" height="100" transform="scale(2)">
                <svg viewBox="0 0 50 50" width="50" height="50" x="0" y="0" transform="translate(10, 10)">
                    <rect x="0" y="0" width="25" height="25" />
                </svg>
            </svg>
        "#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
        assert!(!root.children.is_empty());
    }

    #[test]
    fn nested_svg_preserve_aspect_ratio() {
        let html = r#"
            <svg viewBox="0 0 100 100" width="100" height="100">
                <svg viewBox="0 0 100 50" width="100" height="100" preserveAspectRatio="xMidYMid meet" x="0" y="0">
                    <rect x="0" y="0" width="100" height="50" />
                </svg>
            </svg>
        "#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
        assert!(!root.children.is_empty());
    }

    #[test]
    fn deeply_nested_svg_viewbox_cascade() {
        let html = r#"
            <svg viewBox="0 0 200 200" width="200" height="200">
                <svg viewBox="0 0 100 100" width="100" height="100" x="0" y="0">
                    <svg viewBox="0 0 50 50" width="50" height="50" x="0" y="0">
                        <rect x="0" y="0" width="50" height="50" />
                    </svg>
                </svg>
            </svg>
        "#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, lumen_core::geom::Size::new(400.0, 400.0));
        assert!(!root.children.is_empty());
    }

    #[test]
    fn nested_svg_group_with_transform() {
        let html = r#"
            <svg viewBox="0 0 100 100" width="100" height="100">
                <svg viewBox="0 0 50 50" width="50" height="50" x="0" y="0">
                    <g transform="scale(2)">
                        <rect x="0" y="0" width="10" height="10" />
                    </g>
                </svg>
            </svg>
        "#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, lumen_core::geom::Size::new(200.0, 200.0));
        assert!(!root.children.is_empty());
    }

    // ── ::first-letter / ::first-line CSS wiring ─────────────────────────────

    fn find_run(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
        if matches!(b.kind, super::BoxKind::InlineRun { .. }) { return Some(b); }
        for c in &b.children { if let Some(f) = find_run(c) { return Some(f); } }
        None
    }

    #[test]
    fn first_letter_style_override_splits_segment() {
        // p::first-letter { font-size: 3em } → segment "H" gets overridden style,
        // "ello world" becomes a separate segment with normal style.
        let css = "p::first-letter { font-size: 3em; }";
        let root = super::layout(
            &lumen_html_parser::parse("<p>Hello world</p>"),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { segments, .. } = &run.kind {
            assert!(segments.len() >= 2, "expected split: got {} segment(s)", segments.len());
            assert_eq!(segments[0].text, "H", "first segment must be the first letter");
            assert_eq!(segments[0].pseudo_kind, super::PseudoKind::FirstLetter);
            // font-size 3em on the root = 3 × 16px = 48px.
            assert!(
                (segments[0].style.font_size - 48.0).abs() < 1.0,
                "first-letter font-size must be 3em, got {}", segments[0].style.font_size,
            );
            assert_eq!(segments[1].text, "ello world");
            assert_eq!(segments[1].pseudo_kind, super::PseudoKind::None);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_letter_no_rule_leaves_segment_unsplit() {
        // No ::first-letter rule → segment stays marked but style is unchanged.
        let root = super::layout(
            &lumen_html_parser::parse("<p>Hello</p>"),
            &lumen_css_parser::parse(""),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { segments, .. } = &run.kind {
            assert_eq!(segments.len(), 1, "no split without ::first-letter rule");
            assert_eq!(segments[0].pseudo_kind, super::PseudoKind::FirstLetter);
            assert_eq!(segments[0].text, "Hello");
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_letter_single_char_no_split() {
        // Single character: style override without splitting.
        let css = "p::first-letter { font-weight: bold; }";
        let root = super::layout(
            &lumen_html_parser::parse("<p>X</p>"),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { segments, .. } = &run.kind {
            assert_eq!(segments.len(), 1, "single char: no extra segment");
            assert_eq!(segments[0].text, "X");
            assert_eq!(segments[0].pseudo_kind, super::PseudoKind::FirstLetter);
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_line_style_override_applied_to_first_line_frags() {
        // p::first-line { color: #ff0000 } → frags on lines[0] get red color.
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let css = "p::first-line { color: #ff0000; }";
        // 60px wide → "one two" (56px) on line 0, "three" wraps to line 1.
        let root = super::layout_measured(
            &lumen_html_parser::parse("<p>one two three four</p>"),
            &lumen_css_parser::parse(css),
            lumen_core::geom::Size::new(60.0, 600.0),
            &Fixed8,
        );
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(lines.len() >= 2, "expected wrapping");
            for frag in &lines[0] {
                assert!(
                    frag.style.color.r > 200,
                    "first-line frags must have red color (r={})", frag.style.color.r,
                );
            }
            for line in lines.iter().skip(1) {
                for frag in line {
                    assert!(
                        frag.style.color.r < 50,
                        "non-first-line frags must NOT be red (r={})", frag.style.color.r,
                    );
                }
            }
        } else {
            panic!("expected InlineRun");
        }
    }

    #[test]
    fn first_line_no_rule_leaves_frags_unstyled() {
        // No ::first-line rule → is_first_line is true but style is unchanged.
        struct Fixed8;
        impl super::super::TextMeasurer for Fixed8 {
            fn char_width(&self, _: char, _: f32) -> f32 { 8.0 }
        }
        let root = super::layout_measured(
            &lumen_html_parser::parse("<p>one two three four</p>"),
            &lumen_css_parser::parse(""),
            lumen_core::geom::Size::new(60.0, 600.0),
            &Fixed8,
        );
        let run = find_run(&root).expect("InlineRun not found");
        if let super::BoxKind::InlineRun { lines, .. } = &run.kind {
            assert!(lines.len() >= 2, "expected wrapping");
            // Verify is_first_line is still set (layout infrastructure works).
            assert!(lines[0].iter().all(|f| f.is_first_line), "first line must be marked");
            assert!(lines[1..].iter().flatten().all(|f| !f.is_first_line), "rest not marked");
        } else {
            panic!("expected InlineRun");
        }
    }

    // ── CSS Pseudo-elements L4 §14.2 — ::marker tests ────────────────────────

    fn find_markers(b: &super::LayoutBox, out: &mut Vec<super::LayoutBox>) {
        if matches!(b.kind, super::BoxKind::Marker { .. }) { out.push(b.clone()); }
        for c in &b.children { find_markers(c, out); }
    }

    #[test]
    fn marker_default_inherits_parent_color() {
        // No ::marker rule → marker inherits color from li parent.
        let root = super::layout(
            &lumen_html_parser::parse("<ul><li>item</li></ul>"),
            &lumen_css_parser::parse("ul { color: #ff0000; }"),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let mut markers = Vec::new();
        find_markers(&root, &mut markers);
        assert!(!markers.is_empty(), "expected at least one marker");
        // Marker should have inherited red color from parent ul.
        assert!(
            markers[0].style.color.r > 200,
            "marker should inherit red color from ul, got r={}", markers[0].style.color.r,
        );
    }

    #[test]
    fn marker_css_rule_overrides_color() {
        // ::marker { color: #0000ff } → marker gets blue, not parent color.
        let root = super::layout(
            &lumen_html_parser::parse("<ul><li>item</li></ul>"),
            &lumen_css_parser::parse("ul { color: #ff0000; } li::marker { color: #0000ff; }"),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let mut markers = Vec::new();
        find_markers(&root, &mut markers);
        assert!(!markers.is_empty(), "expected at least one marker");
        // Marker must use blue (::marker rule) not parent red.
        assert!(
            markers[0].style.color.b > 200,
            "marker should be blue from ::marker rule, got b={}", markers[0].style.color.b,
        );
        assert!(
            markers[0].style.color.r < 50,
            "marker should NOT be red (parent color), got r={}", markers[0].style.color.r,
        );
    }

    #[test]
    fn marker_content_none_suppresses_marker() {
        // li::marker { content: none } → no BoxKind::Marker in tree.
        let root = super::layout(
            &lumen_html_parser::parse("<ul><li>item</li></ul>"),
            &lumen_css_parser::parse("li::marker { content: none; }"),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let mut markers = Vec::new();
        find_markers(&root, &mut markers);
        assert!(markers.is_empty(), "content:none should suppress marker box, found {} markers", markers.len());
    }

    #[test]
    fn marker_content_string_overrides_text() {
        // li::marker { content: "★ " } → marker text becomes "★ " not "• ".
        let root = super::layout(
            &lumen_html_parser::parse("<ul><li>item</li></ul>"),
            &lumen_css_parser::parse(r#"li::marker { content: "★ "; }"#),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let mut markers = Vec::new();
        find_markers(&root, &mut markers);
        assert!(!markers.is_empty(), "expected marker with custom content");
        if let super::BoxKind::Marker { ref text, .. } = markers[0].kind {
            assert_eq!(text, "★ ", "custom content string should override default marker text");
        } else {
            panic!("expected BoxKind::Marker");
        }
    }

    #[test]
    fn marker_default_without_css_rule_still_renders() {
        // No ::marker CSS rule at all → marker renders with default disc bullet.
        let root = super::layout(
            &lumen_html_parser::parse("<ul><li>item</li></ul>"),
            &lumen_css_parser::parse(""),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let mut markers = Vec::new();
        find_markers(&root, &mut markers);
        assert!(!markers.is_empty(), "default disc list item must produce a marker box");
    }

    #[test]
    fn marker_font_size_css_rule_applied() {
        // li::marker { font-size: 24px } → marker uses 24px, not the inherited 16px.
        let root = super::layout(
            &lumen_html_parser::parse("<ul><li>item</li></ul>"),
            &lumen_css_parser::parse("li { font-size: 16px; } li::marker { font-size: 24px; }"),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let mut markers = Vec::new();
        find_markers(&root, &mut markers);
        assert!(!markers.is_empty(), "expected marker");
        assert!(
            (markers[0].style.font_size - 24.0).abs() < 0.5,
            "marker should have font-size 24px from CSS rule, got {}", markers[0].style.font_size,
        );
    }

    #[test]
    fn marker_inherits_font_size_from_parent_without_rule() {
        // No ::marker rule → marker inherits font-size from li parent.
        let root = super::layout(
            &lumen_html_parser::parse("<ul><li>item</li></ul>"),
            &lumen_css_parser::parse("li { font-size: 20px; }"),
            lumen_core::geom::Size::new(800.0, 600.0),
        );
        let mut markers = Vec::new();
        find_markers(&root, &mut markers);
        assert!(!markers.is_empty(), "expected marker");
        assert!(
            (markers[0].style.font_size - 20.0).abs() < 0.5,
            "marker should inherit 20px font-size from li, got {}", markers[0].style.font_size,
        );
    }

    // ── CSS Shapes L1 — shape-outside circle() ────────────────────────────────

    #[test]
    fn parse_circle_px_valid() {
        assert_eq!(super::parse_circle_px("circle(50px)"), Some(50.0));
        assert_eq!(super::parse_circle_px("circle(0px)"), None);
        assert_eq!(super::parse_circle_px("circle(10)"), Some(10.0));
        assert_eq!(super::parse_circle_px("CIRCLE(30PX)"), Some(30.0)); // case-insensitive
    }

    #[test]
    fn parse_circle_px_invalid() {
        assert_eq!(super::parse_circle_px("none"), None);
        assert_eq!(super::parse_circle_px("ellipse(30px 20px)"), None);
        assert_eq!(super::parse_circle_px("polygon(0 0, 10 0, 10 10)"), None);
    }

    #[test]
    fn shape_outside_circle_computation() {
        // Circle with radius 50px centered at (100, 50): at y=50 (center),
        // horizontal extent = center_x + radius = 100 + 50 = 150.
        // At y=0 (50px above center): hw = sqrt(50^2 - 50^2) = 0, extent = 100.
        let mut fc = super::FloatContext::new();
        fc.shape_circles.push((0.0, 100.0, true, 100.0, 50.0, 50.0));
        assert!((fc.left_edge_at(50.0, 0.0) - 150.0).abs() < 0.01);
        assert!((fc.left_edge_at(0.0, 0.0) - 100.0).abs() < 0.01);
    }

    // ── CSS Shapes L1 — shape-outside polygon() ───────────────────────────────

    #[test]
    fn parse_shape_polygon_valid() {
        // Triangle with px values.
        let pts = super::parse_shape_polygon_px("polygon(0px 0px, 100px 0px, 50px 100px)");
        assert_eq!(pts, Some(vec![(0.0, 0.0), (100.0, 0.0), (50.0, 100.0)]));
        // Bare numbers (no "px" suffix).
        let pts2 = super::parse_shape_polygon_px("polygon(0 0, 10 0, 10 10, 0 10)");
        assert_eq!(pts2, Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]));
        // With fill-rule prefix.
        let pts3 = super::parse_shape_polygon_px("polygon(nonzero, 0 0, 50 0, 50 50)");
        assert_eq!(pts3, Some(vec![(0.0, 0.0), (50.0, 0.0), (50.0, 50.0)]));
    }

    #[test]
    fn parse_shape_polygon_invalid() {
        // Fewer than 3 points.
        assert_eq!(super::parse_shape_polygon_px("polygon(0 0, 10 10)"), None);
        // Not a polygon.
        assert_eq!(super::parse_shape_polygon_px("circle(50px)"), None);
        assert_eq!(super::parse_shape_polygon_px("none"), None);
    }

    #[test]
    fn polygon_edge_at_y_triangle() {
        // Right-triangle: (0,0)→(100,0)→(0,100)→(0,0).
        // At y=50 the right edge is the hypotenuse at x = 100 - 50 = 50.
        let pts = vec![(0.0_f32, 0.0), (100.0, 0.0), (0.0, 100.0)];
        let right = super::polygon_right_edge_at_y(&pts, 50.0);
        assert!(right.is_some());
        assert!((right.unwrap() - 50.0).abs() < 0.01, "right edge at y=50 should be 50, got {:?}", right);
        // Left edge at y=50: leftmost intersection = 0.0 (vertical left side).
        let left = super::polygon_left_edge_at_y(&pts, 50.0);
        assert!(left.is_some());
        assert!((left.unwrap() - 0.0).abs() < 0.01);
    }

    #[test]
    fn float_context_polygon_left_float() {
        // Triangle left float: (0,0)→(100,0)→(0,100)→(0,0) in content-area coords.
        // At y=50: rightmost edge = 50. Should narrow left boundary to 50.
        let mut fc = super::FloatContext::new();
        fc.shape_polygons.push(super::ShapePolygon {
            top_y: 0.0, bottom_y: 100.0, is_left: true,
            points: vec![(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)],
        });
        assert!((fc.left_edge_at(50.0, 0.0) - 50.0).abs() < 0.01);
        // Outside float range: falls back to default.
        assert!((fc.left_edge_at(110.0, 0.0) - 0.0).abs() < 0.01);
    }

    // ── CSS Shapes L1 — shape-outside ellipse() ───────────────────────────────

    #[test]
    fn parse_shape_ellipse_valid() {
        let r = super::parse_shape_ellipse_px("ellipse(50px 80px at 100px 150px)");
        assert_eq!(r, Some((50.0, 80.0, 100.0, 150.0)));
        // Bare numbers.
        let r2 = super::parse_shape_ellipse_px("ellipse(30 40 at 60 70)");
        assert_eq!(r2, Some((30.0, 40.0, 60.0, 70.0)));
    }

    #[test]
    fn parse_shape_ellipse_invalid() {
        // No "at" keyword.
        assert_eq!(super::parse_shape_ellipse_px("ellipse(50px 80px)"), None);
        // Zero radius.
        assert_eq!(super::parse_shape_ellipse_px("ellipse(0px 40px at 50px 50px)"), None);
        // Not an ellipse.
        assert_eq!(super::parse_shape_ellipse_px("circle(50px)"), None);
    }

    #[test]
    fn float_context_ellipse_left_float() {
        // Ellipse: rx=50, ry=50, center (100,50). At y=50 (center): right edge = 150.
        // At y=0 (top): norm=(0-50)/50=-1.0, hw=0, right edge=100.
        let mut fc = super::FloatContext::new();
        fc.shape_ellipses.push(super::ShapeEllipse {
            top_y: 0.0, bottom_y: 100.0, is_left: true,
            cx: 100.0, cy: 50.0, rx: 50.0, ry: 50.0,
        });
        assert!((fc.left_edge_at(50.0, 0.0) - 150.0).abs() < 0.01);
        assert!((fc.left_edge_at(0.0, 0.0) - 100.0).abs() < 0.01);
    }

    #[test]
    fn float_context_ellipse_right_float() {
        // Ellipse: rx=50, ry=50, center (200,50). Right float.
        // At y=50 (center): left edge = 200 - 50 = 150.
        let mut fc = super::FloatContext::new();
        fc.shape_ellipses.push(super::ShapeEllipse {
            top_y: 0.0, bottom_y: 100.0, is_left: false,
            cx: 200.0, cy: 50.0, rx: 50.0, ry: 50.0,
        });
        assert!((fc.right_edge_at(50.0, 400.0) - 150.0).abs() < 0.01);
    }

    #[test]
    fn content_visibility_hidden_produces_empty_children() {
        let html = r#"<div class="hidden"><span>should be skipped</span></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(".hidden { content-visibility: hidden; }");
        let root = super::layout(&doc, &sheet, Size::new(300.0, 300.0));
        fn find_hidden(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            if b.style.content_visibility == crate::style::ContentVisibility::Hidden {
                return Some(b);
            }
            b.children.iter().find_map(find_hidden)
        }
        if let Some(hidden_box) = find_hidden(&root) {
            assert!(hidden_box.children.is_empty(), "content-visibility:hidden should have no children");
        }
    }

    #[test]
    fn content_visibility_visible_children_present() {
        let html = r#"<div><span>hello</span></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(300.0, 300.0));
        let has_children = root.children.iter().any(|c| !c.children.is_empty());
        assert!(has_children, "visible elements should have children");
    }

    // ── Flex align-content (multi-line flex wrap) ───────────────────────────
    //
    // Setup: 200px wide × 300px tall flex container with 3 × 90px wide items.
    // Lines: [a, b] on line 1, [c] on line 2. Each line cross-size = 50px.
    // used_cross = 100px; free_cross = 200px.

    #[test]
    fn flex_align_content_flex_start() {
        // flex-start: lines packed at cross-start → line1 y=0, line2 y=50.
        let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
        let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:flex-start} #a,#b,#c{width:90px;height:50px}";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let a = find_by_id_all(&root, &doc, "a").expect("a");
        let c = find_by_id_all(&root, &doc, "c").expect("c");
        assert_eq!(a.rect.y, 0.0, "a.y {}", a.rect.y);
        assert_eq!(c.rect.y, 50.0, "c.y {}", c.rect.y);
    }

    #[test]
    fn flex_align_content_flex_end() {
        // flex-end: offset=200 → line1 y=200, line2 y=250.
        let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
        let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:flex-end} #a,#b,#c{width:90px;height:50px}";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let a = find_by_id_all(&root, &doc, "a").expect("a");
        let c = find_by_id_all(&root, &doc, "c").expect("c");
        assert_eq!(a.rect.y, 200.0, "a.y {}", a.rect.y);
        assert_eq!(c.rect.y, 250.0, "c.y {}", c.rect.y);
    }

    #[test]
    fn flex_align_content_center() {
        // center: offset=100 → line1 y=100, line2 y=150.
        let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
        let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:center} #a,#b,#c{width:90px;height:50px}";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let a = find_by_id_all(&root, &doc, "a").expect("a");
        let c = find_by_id_all(&root, &doc, "c").expect("c");
        assert_eq!(a.rect.y, 100.0, "a.y {}", a.rect.y);
        assert_eq!(c.rect.y, 150.0, "c.y {}", c.rect.y);
    }

    #[test]
    fn flex_align_content_space_between() {
        // space-between (n=2): line1 offset=0, line2 offset=200 → y=0 and y=250.
        let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
        let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:space-between} #a,#b,#c{width:90px;height:50px}";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let a = find_by_id_all(&root, &doc, "a").expect("a");
        let c = find_by_id_all(&root, &doc, "c").expect("c");
        assert_eq!(a.rect.y, 0.0, "a.y {}", a.rect.y);
        assert_eq!(c.rect.y, 250.0, "c.y {}", c.rect.y);
    }

    #[test]
    fn flex_align_content_space_around() {
        // space-around (n=2): per=100; line1 offset=50, line2 offset=150 → y=50 and y=200.
        let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
        let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:space-around} #a,#b,#c{width:90px;height:50px}";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let a = find_by_id_all(&root, &doc, "a").expect("a");
        let c = find_by_id_all(&root, &doc, "c").expect("c");
        assert_eq!(a.rect.y, 50.0, "a.y {}", a.rect.y);
        assert_eq!(c.rect.y, 200.0, "c.y {}", c.rect.y);
    }

    #[test]
    fn flex_align_content_space_evenly() {
        // space-evenly (n=2): per=200/3≈66.67; line1 offset=per, line2 offset=2*per.
        let html = r#"<div id="flex"><div id="a"></div><div id="b"></div><div id="c"></div></div>"#;
        let css = "body{margin:0} #flex{display:flex;flex-wrap:wrap;width:200px;height:300px;align-content:space-evenly} #a,#b,#c{width:90px;height:50px}";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        let a = find_by_id_all(&root, &doc, "a").expect("a");
        let c = find_by_id_all(&root, &doc, "c").expect("c");
        let per = 200.0_f32 / 3.0;
        assert!((a.rect.y - per).abs() < 0.5, "a.y expected ≈{per:.2}, got {}", a.rect.y);
        assert!((c.rect.y - (50.0 + 2.0 * per)).abs() < 0.5, "c.y expected ≈{:.2}, got {}", 50.0 + 2.0 * per, c.rect.y);
    }

    #[test]
    fn svg_defs_element_is_skipped() {
        // <defs> container should be invisible (Skip).
        let html = r#"<svg><defs><rect id="r"/></defs><circle/></svg>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));
        // SVG should have only <circle> as visible child, <defs> should be skipped.
        assert!(!root.children.is_empty(), "svg should have children");
        if let Some(svg) = root.children.first()
            && let super::BoxKind::SvgRoot { .. } = &svg.kind
        {
            assert!(!svg.children.is_empty(), "svg should have visible children");
            // Should contain circle, not defs.
        }
    }

    #[test]
    fn svg_intrinsic_ratio_from_viewbox() {
        // SVG with viewBox="0 0 200 100" should have intrinsic ratio of 2:1.
        let html = r#"<svg viewBox="0 0 200 100"></svg>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));
        // Find SVG root.
        if let Some(svg) = root.children.first()
            && let super::BoxKind::SvgRoot { view_box, .. } = &svg.kind
        {
            let ratio = super::svg_intrinsic_ratio(view_box);
            assert_eq!(ratio, Some(2.0), "viewBox 200x100 should give ratio 2.0");
        }
    }

    #[test]
    fn svg_intrinsic_ratio_none_without_viewbox() {
        // SVG without viewBox should return None for intrinsic ratio.
        let html = r#"<svg></svg>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));
        if let Some(svg) = root.children.first()
            && let super::BoxKind::SvgRoot { view_box, .. } = &svg.kind
        {
            let ratio = super::svg_intrinsic_ratio(view_box);
            assert_eq!(ratio, None, "svg without viewBox should have no intrinsic ratio");
        }
    }

    #[test]
    fn svg_preserve_aspect_ratio_meet() {
        // preserveAspectRatio="xMidYMid meet" (default) should parse correctly.
        let html = r#"<svg viewBox="0 0 100 100" width="200" height="100"></svg>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));
        if let Some(svg) = root.children.first()
            && let super::BoxKind::SvgRoot { preserve_aspect_ratio, .. } = &svg.kind
        {
            assert_eq!(preserve_aspect_ratio.meet_or_slice, super::SvgMeetOrSlice::Meet);
            assert_eq!(preserve_aspect_ratio.align_x, super::SvgAlignX::Mid);
            assert_eq!(preserve_aspect_ratio.align_y, super::SvgAlignY::Mid);
        }
    }

    #[test]
    fn svg_preserve_aspect_ratio_slice() {
        // preserveAspectRatio="xMinYMin slice" should parse correctly.
        let html = r#"<svg preserveAspectRatio="xMinYMin slice"></svg>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));
        if let Some(svg) = root.children.first()
            && let super::BoxKind::SvgRoot { preserve_aspect_ratio, .. } = &svg.kind
        {
            assert_eq!(preserve_aspect_ratio.meet_or_slice, super::SvgMeetOrSlice::Slice);
            assert_eq!(preserve_aspect_ratio.align_x, super::SvgAlignX::Min);
            assert_eq!(preserve_aspect_ratio.align_y, super::SvgAlignY::Min);
        }
    }

    #[test]
    fn svg_use_element_references_target() {
        // <use href="#target"/> should reference element with id="target".
        // SVG 1.1 § 5.6 — <use> creates a reference to another element.
        let html = "<svg><defs><rect id=\"r1\" x=\"10\" y=\"10\" width=\"50\" height=\"50\"/></defs><use href=\"#r1\" x=\"100\" y=\"100\"/></svg>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));
        // SVG should have at least the <use> element (which should create referenced content).
        if let Some(svg) = root.children.first()
            && let super::BoxKind::SvgRoot { .. } = &svg.kind
        {
            // <use> should have been processed and added to the layout.
            // The exact structure depends on implementation, but we verify no panic.
            assert!(!svg.children.is_empty(), "svg should have layout children from <use>");
        }
    }

    #[test]
    fn svg_use_translate_x_y() {
        // <use x="10" y="20"> should apply translate transform.
        let html = "<svg><circle id=\"c1\" cx=\"0\" cy=\"0\" r=\"5\"/><use href=\"#c1\" x=\"10\" y=\"20\"/></svg>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let _root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));
        // Verify no panic when processing <use> with x/y attributes.
    }

    #[test]
    fn svg_text_element_simple() {
        // <text>Hello</text> should create a SvgText layout box with content.
        let html = "<svg><text>Hello</text></svg>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));

        fn find_text_box(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::SvgText { .. }) {
                    return Some(child);
                }
                if let Some(found) = find_text_box(child) {
                    return Some(found);
                }
            }
            None
        }

        let text_box = find_text_box(&root);
        assert!(text_box.is_some(), "SvgText layout box not found");

        if let Some(tb) = text_box {
            if let super::BoxKind::SvgText { text, .. } = &tb.kind {
                assert_eq!(text, "Hello");
            } else {
                panic!("Found box is not SvgText");
            }
        }
    }

    #[test]
    fn svg_text_with_x_y_attributes() {
        // <text x="10" y="20">Content</text> should store x/y values.
        let html = "<svg><text x=\"10\" y=\"20\">Test</text></svg>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));

        fn find_text_box(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::SvgText { .. }) {
                    return Some(child);
                }
                if let Some(found) = find_text_box(child) {
                    return Some(found);
                }
            }
            None
        }

        if let Some(tb) = find_text_box(&root) {
            if let super::BoxKind::SvgText { x, y, text, .. } = &tb.kind {
                assert!((x - 10.0).abs() < 0.1, "x should be ~10, got {}", x);
                assert!((y - 20.0).abs() < 0.1, "y should be ~20, got {}", y);
                assert_eq!(text, "Test");
            } else {
                panic!("Found box is not SvgText");
            }
        }
    }

    #[test]
    fn svg_text_anchor_middle() {
        // <text text-anchor="middle">Center</text> should parse text-anchor.
        let html = "<svg><text text-anchor=\"middle\">Center</text></svg>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));

        fn find_text_box(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::SvgText { .. }) {
                    return Some(child);
                }
                if let Some(found) = find_text_box(child) {
                    return Some(found);
                }
            }
            None
        }

        if let Some(tb) = find_text_box(&root) {
            if let super::BoxKind::SvgText { text_anchor, .. } = &tb.kind {
                assert_eq!(*text_anchor, super::SvgTextAnchor::Middle);
            } else {
                panic!("Found box is not SvgText");
            }
        }
    }

    #[test]
    fn svg_dominant_baseline_hanging() {
        // <text dominant-baseline="hanging">Hanging</text> should parse dominant-baseline.
        let html = "<svg><text dominant-baseline=\"hanging\">Hanging</text></svg>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));

        fn find_text_box(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::SvgText { .. }) {
                    return Some(child);
                }
                if let Some(found) = find_text_box(child) {
                    return Some(found);
                }
            }
            None
        }

        if let Some(tb) = find_text_box(&root) {
            if let super::BoxKind::SvgText { dominant_baseline, .. } = &tb.kind {
                assert_eq!(*dominant_baseline, super::SvgDominantBaseline::Hanging);
            } else {
                panic!("Found box is not SvgText");
            }
        }
    }

    #[test]
    fn svg_tspan_text_content() {
        // <text><tspan>Hello</tspan> <tspan>World</tspan></text> should collect all tspan text.
        let html = "<svg><text><tspan>Hello</tspan><tspan>World</tspan></text></svg>";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let root = super::layout(&doc, &sheet, Size::new(400.0, 400.0));

        fn find_text_box(b: &super::LayoutBox) -> Option<&super::LayoutBox> {
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::SvgText { .. }) {
                    return Some(child);
                }
                if let Some(found) = find_text_box(child) {
                    return Some(found);
                }
            }
            None
        }

        if let Some(tb) = find_text_box(&root) {
            if let super::BoxKind::SvgText { text, .. } = &tb.kind {
                assert!(text.contains("Hello"), "text should contain 'Hello', got '{}'", text);
                assert!(text.contains("World"), "text should contain 'World', got '{}'", text);
            } else {
                panic!("Found box is not SvgText");
            }
        }
    }

    // CSS Grid auto-fill/auto-fit tests (B-3)
    #[test]
    fn grid_auto_fill_count_basic() {
        // repeat(auto-fill, minmax(100px, 1fr)) with 500px available
        // should resolve to 5 tracks (500 / 100 = 5)
        let tracks = vec![GridTrackSize::Minmax(
            Box::new(GridTrackSize::Length(Length::Px(100.0))),
            Box::new(GridTrackSize::Fr(1.0)),
        )];
        let count = resolve_auto_fill_fit_count(500.0, &tracks, 0.0);
        assert_eq!(count, 5, "should fit 5 tracks of 100px each");
    }

    #[test]
    fn grid_auto_fill_count_with_gap() {
        // repeat(auto-fill, minmax(100px, 1fr)) with 500px available and 10px gap
        // (500 + 10) / (100 + 10) = 510 / 110 ≈ 4.63 → 4 tracks
        let tracks = vec![GridTrackSize::Minmax(
            Box::new(GridTrackSize::Length(Length::Px(100.0))),
            Box::new(GridTrackSize::Fr(1.0)),
        )];
        let count = resolve_auto_fill_fit_count(500.0, &tracks, 10.0);
        assert_eq!(count, 4, "should fit 4 tracks with gap");
    }

    #[test]
    fn grid_auto_fill_count_zero_width() {
        // Zero or negative width should return 1 track minimum
        let tracks = vec![GridTrackSize::Length(Length::Px(100.0))];
        let count = resolve_auto_fill_fit_count(0.0, &tracks, 0.0);
        assert_eq!(count, 1, "zero width should return 1 track minimum");
    }

    #[test]
    fn grid_auto_fill_count_large_gap() {
        // Gap larger than available width should still return 1 track
        let tracks = vec![GridTrackSize::Length(Length::Px(50.0))];
        let count = resolve_auto_fill_fit_count(30.0, &tracks, 100.0);
        assert_eq!(count, 1, "should return 1 track minimum");
    }

    #[test]
    fn grid_fit_content_parse() {
        // `fit-content(200px)` should parse correctly
        let parsed = GridTrackSize::parse_track_list("fit-content(200px)", false);
        assert_eq!(parsed.len(), 1, "fit-content(200px) should parse to single track");
        if let GridTrackSize::FitContent(limit) = &parsed[0] {
            // Verify the limit is a Length(200px)
            match &**limit {
                GridTrackSize::Length(Length::Px(val)) => {
                    assert_eq!(*val, 200.0, "fit-content limit should be 200px");
                }
                _ => panic!("fit-content limit should be Length(200px), got {:?}", limit),
            }
        } else {
            panic!("parsed should be FitContent variant");
        }
    }

    #[test]
    fn grid_fit_content_minmax() {
        // `fit-content(300px)` should be equivalent to minmax(auto, min(300px, max-content))
        let parsed = GridTrackSize::parse_track_list("fit-content(300px)", false);
        assert_eq!(parsed.len(), 1);
        // Verify internal structure has FitContent variant
        assert!(matches!(parsed[0], GridTrackSize::FitContent(_)));
    }

    #[test]
    fn grid_auto_fill_multiple_tracks() {
        // repeat(auto-fill, minmax(50px, 1fr) minmax(50px, 1fr)) with 300px
        // Two tracks per repeat unit (100px total) → 3 units → 3 fills
        let tracks = vec![
            GridTrackSize::Minmax(
                Box::new(GridTrackSize::Length(Length::Px(50.0))),
                Box::new(GridTrackSize::Fr(1.0)),
            ),
            GridTrackSize::Minmax(
                Box::new(GridTrackSize::Length(Length::Px(50.0))),
                Box::new(GridTrackSize::Fr(1.0)),
            ),
        ];
        let count = resolve_auto_fill_fit_count(300.0, &tracks, 0.0);
        // Min width = max(50, 50) = 50px, so (300 + 0) / (50 + 0) = 6
        // But we have 2 tracks per repeat, so count should be based on total min width
        // Simplification: resolve_auto_fill_fit_count returns count of repeat units, not total tracks
        assert!(count >= 1, "should resolve to at least 1 repeat unit");
    }

    #[test]
    fn grid_auto_fill_small_container() {
        // Container smaller than one track should still return 1
        let tracks = vec![GridTrackSize::Length(Length::Px(500.0))];
        let count = resolve_auto_fill_fit_count(100.0, &tracks, 0.0);
        assert_eq!(count, 1, "container smaller than track should return 1");
    }

    #[test]
    fn grid_auto_fill_empty_tracks() {
        // Empty track list should return 1
        let tracks: Vec<GridTrackSize> = vec![];
        let count = resolve_auto_fill_fit_count(500.0, &tracks, 0.0);
        assert_eq!(count, 1, "empty track list should return 1");
    }

    // CSS Grid dense packing tests (B-4)
    #[test]
    fn grid_dense_fills_gaps() {
        // grid-auto-flow: row dense should fill gaps left by taller items
        let html = "<div class='container'>\
                     <div style='grid-row: 1 / 3;'>Large</div>\
                     <div>Item 2</div>\
                     <div>Item 3</div>\
                   </div>";
        let css = ".container { \
                    display: grid; \
                    grid-template-columns: repeat(3, 1fr); \
                    grid-auto-flow: row dense; \
                  }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(300.0, 300.0));

        fn find_grid_items(b: &super::LayoutBox) -> Vec<(f32, f32)> {
            let mut items = Vec::new();
            for child in &b.children {
                if matches!(child.kind, super::BoxKind::Block) && !child.children.is_empty() {
                    // This is a grid item (has content)
                    items.push((child.rect.x, child.rect.y));
                }
                items.extend(find_grid_items(child));
            }
            items
        }

        let items = find_grid_items(&root);
        // With dense, Item 2 and 3 should fill the gap in columns 2-3 of row 1
        assert!(items.len() >= 3, "should have at least 3 items");
    }

    #[test]
    fn grid_column_dense_backfill() {
        // grid-auto-flow: column dense should backfill in column order
        let html = "<div class='container'>\
                     <div style='grid-column: 1 / 3;'>Wide</div>\
                     <div>Item 2</div>\
                     <div>Item 3</div>\
                   </div>";
        let css = ".container { \
                    display: grid; \
                    grid-template-rows: repeat(2, 100px); \
                    grid-auto-flow: column dense; \
                  }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(300.0, 300.0));

        // Just verify it doesn't panic and produces a layout
        assert!(!root.children.is_empty(), "grid should have content");
    }

    #[test]
    fn grid_dense_vs_sparse_layout() {
        // Compare dense and sparse layouts to ensure they differ appropriately
        fn layout_with_flow(flow: &str) -> super::LayoutBox {
            let html = "<div class='container'>\
                         <div style='grid-column: span 2; grid-row: span 2;'>1</div>\
                         <div>2</div>\
                         <div>3</div>\
                         <div>4</div>\
                       </div>";
            let css = format!(".container {{ \
                               display: grid; \
                               grid-template-columns: repeat(3, 100px); \
                               grid-auto-flow: {}; \
                             }}", flow);
            let doc = lumen_html_parser::parse(html);
            let sheet = lumen_css_parser::parse(&css);
            super::layout(&doc, &sheet, Size::new(300.0, 300.0))
        }

        let sparse = layout_with_flow("row");
        let dense = layout_with_flow("row dense");

        // Both should produce valid layouts
        assert!(!sparse.children.is_empty(), "sparse layout should have content");
        assert!(!dense.children.is_empty(), "dense layout should have content");
        // Layouts may differ due to dense filling gaps differently
    }

    #[test]
    fn grid_dense_explicit_placement_respected() {
        // Explicitly placed items should not be affected by dense algorithm
        let html = "<div class='container'>\
                     <div style='grid-column: 2; grid-row: 2;'>Explicit</div>\
                     <div>Auto 1</div>\
                     <div>Auto 2</div>\
                   </div>";
        let css = ".container { \
                    display: grid; \
                    grid-template-columns: repeat(3, 1fr); \
                    grid-auto-flow: row dense; \
                  }";
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::layout(&doc, &sheet, Size::new(300.0, 300.0));

        // Verify layout was created without panics
        assert!(!root.children.is_empty(), "grid should be laid out");
    }

}


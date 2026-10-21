//! SVG `<filter>` → граф примитивов (Filter Effects L1 §«The `<filter>` element»).
//!
//! `filter: url(#id)` хранится после каскада как [`FilterFn::Url`] с одним id;
//! здесь, где есть `Document`, ссылка разворачивается в [`SvgFilter`]: дети
//! `<filter>` (`feFlood`, `feOffset`, `feGaussianBlur`, `feColorMatrix`,
//! `feComposite`, `feBlend`, `feMerge`, `feComponentTransfer`, `feDropShadow`)
//! сводятся к вариантам [`FePrimitive`] с уже разобранными числами и входами
//! (`in`/`in2`/`result` → индексы). Остальные `fe*` дают
//! [`FePrimitive::Unsupported`] (прозрачный чёрный), чтобы граф не терял позиции.
//!
//! Свойства `flood-color`/`flood-opacity`/`color-interpolation-filters` читаются
//! из presentation-атрибута и inline-`style` примитива: правила таблицы стилей
//! не учитываются (в `ComputedStyle` этих свойств пока нет).

use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeId};

use super::svg::SvgTransform;
use super::svg_clip::user_bbox_of;
use super::LayoutBox;
use crate::style::{FilterFn, SvgClipPlacement, 
    Color, ComputedStyle, FeBlendMode, FeColorMatrix, FeCompositeOp, FeInput,
    FePrimitive, FeTransfer, SvgFilter, SvgFilterPrimitive, compute_style, parse_color,
};

/// Разворачивает `url(#id)` в граф. `None` — элемента нет или это не `<filter>`.
pub(crate) fn resolve_filter_url(
    doc: &Document,
    sheet: &Stylesheet,
    id: &str,
    viewport: Size,
    dark_mode: bool,
) -> Option<Arc<SvgFilter>> {
    let f = doc.find_by_id(id)?;
    if !is_tag(doc, f, "filter") {
        return None;
    }
    let obb = |attr: &str, default_obb: bool| -> bool {
        match doc.get(f).get_attr(attr).map(str::trim) {
            Some("objectBoundingBox") => true,
            Some("userSpaceOnUse") => false,
            _ => default_obb,
        }
    };
    let filter_units_obb = obb("filterUnits", true);
    let primitive_units_obb = obb("primitiveUnits", false);

    let region_len = |attr: &str, default_frac: f32, basis: f32| -> f32 {
        let parsed = doc
            .get(f)
            .get_attr(attr)
            .and_then(|v| parse_len(v, filter_units_obb, basis));
        parsed.unwrap_or(if filter_units_obb { default_frac } else { default_frac * basis })
    };
    let region = (
        region_len("x", -0.1, viewport.width),
        region_len("y", -0.1, viewport.height),
        region_len("width", 1.2, viewport.width),
        region_len("height", 1.2, viewport.height),
    );

    let filter_style = compute_style(doc, f, sheet, &ComputedStyle::root(), viewport, dark_mode);
    let filter_linear = interpolation_is_linear(doc, f, None);

    let mut primitives: Vec<SvgFilterPrimitive> = Vec::new();
    let mut results: Vec<(String, usize)> = Vec::new();
    for &child in &doc.get(f).children {
        let Some(name) = doc.get(child).element_name() else { continue };
        let tag = name.local.as_str().to_ascii_lowercase();
        if !tag.starts_with("fe") {
            continue;
        }
        let idx = primitives.len();
        let ctx = Ctx { doc, node: child, results: &results, viewport, sheet, dark_mode, filter_style: &filter_style };
        let kind = build_primitive(&ctx, &tag);
        let sub = |attr: &str, basis: f32| {
            doc.get(child)
                .get_attr(attr)
                .and_then(|v| parse_len(v, primitive_units_obb, basis))
        };
        primitives.push(SvgFilterPrimitive {
            kind,
            x: sub("x", viewport.width),
            y: sub("y", viewport.height),
            width: sub("width", viewport.width),
            height: sub("height", viewport.height),
            linear_rgb: interpolation_is_linear(doc, child, Some(filter_linear)),
        });
        if let Some(r) = doc.get(child).get_attr("result").map(str::trim)
            && !r.is_empty()
        {
            results.push((r.to_string(), idx));
        }
    }
    Some(Arc::new(SvgFilter { filter_units_obb, primitive_units_obb, region, primitives }))
}

#[derive(Clone, Copy)]
struct Ctx<'a> {
    doc: &'a Document,
    node: NodeId,
    results: &'a [(String, usize)],
    viewport: Size,
    sheet: &'a Stylesheet,
    dark_mode: bool,
    filter_style: &'a ComputedStyle,
}

impl Ctx<'_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.doc.get(self.node).get_attr(name)
    }

    fn num(&self, name: &str, default: f32) -> f32 {
        self.attr(name).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(default)
    }

    fn input(&self, name: &str) -> FeInput {
        match self.attr(name).map(str::trim) {
            None | Some("") => FeInput::Previous,
            Some("SourceGraphic") => FeInput::SourceGraphic,
            Some("SourceAlpha") => FeInput::SourceAlpha,
            Some("BackgroundImage" | "BackgroundAlpha" | "FillPaint" | "StrokePaint") => {
                FeInput::Empty
            }
            Some(n) => self
                .results
                .iter()
                .rev()
                .find(|(r, _)| r == n)
                .map_or(FeInput::Previous, |&(_, i)| FeInput::Result(i)),
        }
    }

    /// `flood-color` + `flood-opacity` (presentation-атрибут / inline `style`).
    fn flood(&self) -> (Color, f32) {
        let raw = svg_prop(self.doc, self.node, "flood-color");
        let color = match raw.as_deref().map(str::trim) {
            None => Color::BLACK,
            Some(v) if v.eq_ignore_ascii_case("currentcolor") => {
                let st = compute_style(
                    self.doc,
                    self.node,
                    self.sheet,
                    self.filter_style,
                    self.viewport,
                    self.dark_mode,
                );
                st.color
            }
            Some(v) => parse_color(v).unwrap_or(Color::BLACK),
        };
        let opacity = svg_prop(self.doc, self.node, "flood-opacity")
            .and_then(|v| parse_number_or_percent(&v))
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        (color, opacity)
    }

    fn num_pair(&self, name: &str, default: f32) -> (f32, f32) {
        let nums = self.attr(name).map(parse_number_list).unwrap_or_default();
        match nums.as_slice() {
            [] => (default, default),
            [a] => (*a, *a),
            [a, b] => (*a, *b),
            _ => (default, default),
        }
    }
}

fn build_primitive(c: &Ctx<'_>, tag: &str) -> FePrimitive {
    match tag {
        "feflood" => {
            let (color, opacity) = c.flood();
            FePrimitive::Flood { color, opacity }
        }
        "feoffset" => FePrimitive::Offset {
            input: c.input("in"),
            dx: c.num("dx", 0.0),
            dy: c.num("dy", 0.0),
        },
        "fegaussianblur" => {
            let (sx, sy) = c.num_pair("stdDeviation", 0.0);
            FePrimitive::GaussianBlur { input: c.input("in"), sx, sy }
        }
        "fecolormatrix" => FePrimitive::ColorMatrix { input: c.input("in"), matrix: color_matrix(c) },
        "fecomposite" => {
            let op = match c.attr("operator").map(str::trim) {
                Some("in") => FeCompositeOp::In,
                Some("out") => FeCompositeOp::Out,
                Some("atop") => FeCompositeOp::Atop,
                Some("xor") => FeCompositeOp::Xor,
                Some("arithmetic") => FeCompositeOp::Arithmetic([
                    c.num("k1", 0.0),
                    c.num("k2", 0.0),
                    c.num("k3", 0.0),
                    c.num("k4", 0.0),
                ]),
                _ => FeCompositeOp::Over,
            };
            FePrimitive::Composite { a: c.input("in"), b: c.input("in2"), op }
        }
        "feblend" => {
            let mode = match c.attr("mode").map(str::trim) {
                Some("multiply") => FeBlendMode::Multiply,
                Some("screen") => FeBlendMode::Screen,
                Some("darken") => FeBlendMode::Darken,
                Some("lighten") => FeBlendMode::Lighten,
                Some("overlay") => FeBlendMode::Overlay,
                Some("color-dodge") => FeBlendMode::ColorDodge,
                Some("color-burn") => FeBlendMode::ColorBurn,
                Some("hard-light") => FeBlendMode::HardLight,
                Some("soft-light") => FeBlendMode::SoftLight,
                Some("difference") => FeBlendMode::Difference,
                Some("exclusion") => FeBlendMode::Exclusion,
                _ => FeBlendMode::Normal,
            };
            FePrimitive::Blend { a: c.input("in"), b: c.input("in2"), mode }
        }
        "femerge" => {
            let mut layers = Vec::new();
            for &n in &c.doc.get(c.node).children {
                if is_tag(c.doc, n, "feMergeNode") {
                    let sub = Ctx { node: n, ..*c };
                    layers.push(sub.input("in"));
                }
            }
            FePrimitive::Merge(layers)
        }
        "fecomponenttransfer" => {
            let mut funcs: [FeTransfer; 4] = [
                FeTransfer::Identity,
                FeTransfer::Identity,
                FeTransfer::Identity,
                FeTransfer::Identity,
            ];
            for &n in &c.doc.get(c.node).children {
                let Some(name) = c.doc.get(n).element_name() else { continue };
                let slot = match name.local.as_str().to_ascii_lowercase().as_str() {
                    "fefuncr" => 0,
                    "fefuncg" => 1,
                    "fefuncb" => 2,
                    "fefunca" => 3,
                    _ => continue,
                };
                funcs[slot] = transfer_fn(c.doc, n);
            }
            FePrimitive::ComponentTransfer { input: c.input("in"), funcs: Box::new(funcs) }
        }
        "fedropshadow" => {
            let (sx, sy) = c.num_pair("stdDeviation", 2.0);
            let (color, opacity) = c.flood();
            FePrimitive::DropShadow {
                input: c.input("in"),
                dx: c.num("dx", 2.0),
                dy: c.num("dy", 2.0),
                sx,
                sy,
                color,
                opacity,
            }
        }
        "fedisplacementmap" | "femorphology" | "feconvolvematrix" | "fetile" => {
            FePrimitive::Unsupported { input: c.input("in") }
        }
        _ => FePrimitive::Unsupported { input: FeInput::Empty },
    }
}

fn color_matrix(c: &Ctx<'_>) -> FeColorMatrix {
    let values = c.attr("values").map(parse_number_list);
    match c.attr("type").map(str::trim) {
        Some("saturate") => {
            FeColorMatrix::Saturate(values.and_then(|v| v.first().copied()).unwrap_or(1.0))
        }
        Some("hueRotate") => {
            FeColorMatrix::HueRotate(values.and_then(|v| v.first().copied()).unwrap_or(0.0))
        }
        Some("luminanceToAlpha") => FeColorMatrix::LuminanceToAlpha,
        _ => match values {
            Some(v) if v.len() == 20 => {
                let mut m = [0.0; 20];
                m.copy_from_slice(&v);
                FeColorMatrix::Matrix(m)
            }
            _ => FeColorMatrix::Matrix(IDENTITY_MATRIX),
        },
    }
}

const IDENTITY_MATRIX: [f32; 20] = [
    1.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, 0.0,
];

fn transfer_fn(doc: &Document, n: NodeId) -> FeTransfer {
    let node = doc.get(n);
    let num = |name: &str, default: f32| {
        node.get_attr(name).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(default)
    };
    let table = || node.get_attr("tableValues").map(parse_number_list).unwrap_or_default();
    match node.get_attr("type").map(str::trim) {
        Some("table") => {
            let t = table();
            if t.is_empty() { FeTransfer::Identity } else { FeTransfer::Table(t) }
        }
        Some("discrete") => {
            let t = table();
            if t.is_empty() { FeTransfer::Identity } else { FeTransfer::Discrete(t) }
        }
        Some("linear") => FeTransfer::Linear { slope: num("slope", 1.0), intercept: num("intercept", 0.0) },
        Some("gamma") => FeTransfer::Gamma {
            amplitude: num("amplitude", 1.0),
            exponent: num("exponent", 1.0),
            offset: num("offset", 0.0),
        },
        _ => FeTransfer::Identity,
    }
}

fn is_tag(doc: &Document, n: NodeId, tag: &str) -> bool {
    doc.get(n).element_name().is_some_and(|q| q.local.as_str().eq_ignore_ascii_case(tag))
}

/// `color-interpolation-filters`: `linearRGB` (по умолчанию) или `sRGB`/`auto`.
/// У примитива без значения наследуется значение `<filter>` (`inherited`).
fn interpolation_is_linear(doc: &Document, n: NodeId, inherited: Option<bool>) -> bool {
    match svg_prop(doc, n, "color-interpolation-filters").as_deref().map(str::trim) {
        Some("linearRGB") => true,
        Some("sRGB" | "auto") => false,
        _ => inherited.unwrap_or(true),
    }
}

/// Значение SVG-свойства узла: inline-`style` приоритетнее presentation-атрибута.
fn svg_prop(doc: &Document, n: NodeId, name: &str) -> Option<String> {
    if let Some(style) = doc.get(n).get_attr("style") {
        for decl in style.split(';').rev() {
            if let Some((k, v)) = decl.split_once(':')
                && k.trim().eq_ignore_ascii_case(name)
            {
                return Some(v.trim().trim_end_matches("!important").trim().to_string());
            }
        }
    }
    doc.get(n).get_attr(name).map(str::to_string)
}

fn parse_number_list(s: &str) -> Vec<f32> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map_while(|t| t.parse::<f32>().ok())
        .collect()
}

fn parse_number_or_percent(s: &str) -> Option<f32> {
    let s = s.trim();
    match s.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok().map(|v| v / 100.0),
        None => s.parse::<f32>().ok(),
    }
}

/// Длина области/подобласти: число (`px`) или `%`. В единицах `objectBoundingBox`
/// и число, и процент — доли bbox (`%` / 100); в `userSpaceOnUse` процент
/// берётся от `basis` (размер viewport).
fn parse_len(raw: &str, obb: bool, basis: f32) -> Option<f32> {
    let raw = raw.trim();
    if let Some(p) = raw.strip_suffix('%') {
        let v = p.trim().parse::<f32>().ok()? / 100.0;
        return Some(if obb { v } else { v * basis });
    }
    raw.strip_suffix("px").unwrap_or(raw).trim().parse::<f32>().ok()
}

/// Записывает в `FilterFn::Url` бокса положение его user space в документе
/// (`m_doc`: user space → документ); смещение матрицы — относительно
/// `rect.x/y`, как у [`super::svg_clip::place_clip_path`].
pub(super) fn place_filters(b: &mut LayoutBox, m_doc: &SvgTransform) {
    let user_bbox = user_bbox_of(b, m_doc);
    let mut matrix = m_doc.matrix;
    matrix[4] -= b.rect.x;
    matrix[5] -= b.rect.y;
    let placement = SvgClipPlacement { matrix, user_bbox };
    for f in &mut std::sync::Arc::make_mut(&mut b.style).filter {
        if let FilterFn::Url(u) = f {
            u.placement = Some(placement);
        }
    }
}

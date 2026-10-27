//! SVG `<filter>` → граф примитивов (Filter Effects L1 §«The `<filter>` element»).
//!
//! `filter: url(#id)` хранится после каскада как [`FilterFn::Url`] с одним id;
//! здесь, где есть `Document`, ссылка разворачивается в [`SvgFilter`]: дети
//! `<filter>` (`feFlood`, `feOffset`, `feGaussianBlur`, `feColorMatrix`,
//! `feComposite`, `feBlend`, `feMerge`, `feComponentTransfer`, `feDropShadow`,
//! `feTile`, `feMorphology`, `feConvolveMatrix`, `feDisplacementMap`,
//! `feTurbulence`, `feDiffuseLighting`, `feSpecularLighting`, `feImage`) сводятся к
//! вариантам [`FePrimitive`] с уже разобранными числами и входами
//! (`in`/`in2`/`result` → индексы). Остальные `fe*` дают
//! [`FePrimitive::Unsupported`] (прозрачный чёрный), чтобы граф не терял позиции;
//! `feImage href="#id"` раскрывается в картинку-элемент ([`FePrimitive::Element`]).
//!
//! Свойства `flood-color`/`flood-opacity`/`lighting-color`/
//! `color-interpolation-filters` читаются из presentation-атрибута и
//! inline-`style` примитива: правила таблицы стилей не учитываются (в
//! `ComputedStyle` этих свойств пока нет).

use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeId};

use super::svg::SvgTransform;
use super::svg_clip::user_bbox_of;
use super::LayoutBox;
use crate::style::{FilterFn, SvgClipPlacement, 
    Color, ComputedStyle, FeBlendMode, FeColorMatrix, FeCompositeOp, FeEdgeMode, FeImageFit, FeInput, FeLight,
    FeElementPicture, FeLighting, FePrimitive, FeTransfer, SvgFilter, SvgFilterPrimitive, compute_style, parse_color,
};

/// Разворачивает `url(#id)` в граф. `None` — элемента нет или это не `<filter>`.
pub(crate) fn resolve_filter_url(
    doc: &Document,
    sheet: &Stylesheet,
    id: &str,
    node: NodeId,
    viewport: Size,
    dark_mode: bool,
) -> Option<Arc<SvgFilter>> {
    let f = doc.find_by_id(id)?;
    if !is_tag(doc, f, "filter") {
        return None;
    }
    let filter_style = compute_style(doc, f, sheet, &ComputedStyle::root(), viewport, dark_mode);
    // `<filter>` не входит в список SVG-элементов с presentation-атрибутами, поэтому
    // `font-size="10px"` на нём каскад не видит: `em` в `width="1em"` читаем сами.
    let em = svg_prop(doc, f, "font-size")
        .and_then(|v| crate::style::parse_length(&v))
        .and_then(|l| l.resolve(filter_style.font_size, Some(filter_style.font_size), viewport))
        .unwrap_or(filter_style.font_size);
    let lc = LenCtx { em, viewport };
    let obb = |attr: &str, default_obb: bool| -> bool {
        match doc.get(f).get_attr(attr).map(str::trim) {
            Some("objectBoundingBox") => true,
            Some("userSpaceOnUse") => false,
            _ => default_obb,
        }
    };
    let filter_units_obb = obb("filterUnits", true);
    let primitive_units_obb = obb("primitiveUnits", false);

    // Проценты `userSpaceOnUse` считаются от ближайшего SVG viewport ссылающегося
    // элемента; у HTML-бокса (`None`) база — его border box, известный только paint.
    let svg_basis = nearest_svg_viewport(doc, node, viewport);
    let region_len = |attr: &str, default_frac: f32, axis: usize| -> (f32, bool) {
        let parsed = doc
            .get(f)
            .get_attr(attr)
            .and_then(|v| parse_len(v, filter_units_obb, svg_basis, axis, lc));
        parsed.unwrap_or(if filter_units_obb {
            (default_frac, false)
        } else {
            resolve_pct(default_frac, svg_basis, axis)
        })
    };
    let (rx, ry, rw, rh) = (region_len("x", -0.1, 0), region_len("y", -0.1, 1), region_len("width", 1.2, 0), region_len("height", 1.2, 1));
    let region = (rx.0, ry.0, rw.0, rh.0);
    let region_pct = [rx.1, ry.1, rw.1, rh.1];

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
        let ctx = Ctx {
            doc,
            node: child,
            filter: f,
            results: &results,
            viewport,
            svg_size: svg_basis.unwrap_or(viewport),
            sheet,
            dark_mode,
            filter_style: &filter_style,
        };
        let kind = build_primitive(&ctx, &tag);
        let sub = |attr: &str, axis: usize| {
            doc.get(child)
                .get_attr(attr)
                .and_then(|v| parse_len(v, primitive_units_obb, svg_basis, axis, lc))
        };
        let (sx, sy, sw, sh) = (sub("x", 0), sub("y", 1), sub("width", 0), sub("height", 1));
        primitives.push(SvgFilterPrimitive {
            kind,
            x: sx.map(|v| v.0),
            y: sy.map(|v| v.0),
            width: sw.map(|v| v.0),
            height: sh.map(|v| v.0),
            pct: [sx, sy, sw, sh].map(|v| v.is_some_and(|v| v.1)),
            linear_rgb: interpolation_is_linear(doc, child, Some(filter_linear)),
            tainted: match tag.as_str() {
                "feflood" | "fedropshadow" => uses_current_color(doc, child, "flood-color"),
                "fediffuselighting" | "fespecularlighting" => uses_current_color(doc, child, "lighting-color"),
                _ => false,
            },
        });
        if let Some(r) = doc.get(child).get_attr("result").map(str::trim)
            && !r.is_empty()
        {
            results.push((r.to_string(), idx));
        }
    }
    Some(Arc::new(SvgFilter { filter_units_obb, primitive_units_obb, region, region_pct, primitives }))
}

#[derive(Clone, Copy)]
struct Ctx<'a> {
    doc: &'a Document,
    node: NodeId,
    /// Элемент `<filter>`, которому принадлежит примитив.
    filter: NodeId,
    results: &'a [(String, usize)],
    viewport: Size,
    /// Размер ближайшего `<svg>` ссылающегося элемента (база процентов картинки-элемента).
    svg_size: Size,
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
        let color = self.color_prop("flood-color", Color::BLACK);
        let opacity = svg_prop(self.doc, self.node, "flood-opacity")
            .and_then(|v| parse_number_or_percent(&v))
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        (color, opacity)
    }

    /// Цветовое свойство (`flood-color`, `lighting-color`) с `currentColor`.
    fn color_prop(&self, name: &str, default: Color) -> Color {
        let raw = svg_prop(self.doc, self.node, name);
        match raw.as_deref().map(str::trim) {
            None => default,
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
            Some(v) => parse_color(v).unwrap_or(default),
        }
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
                Some("hue") => FeBlendMode::Hue,
                Some("saturation") => FeBlendMode::Saturation,
                Some("color") => FeBlendMode::Color,
                Some("luminosity") => FeBlendMode::Luminosity,
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
        "fetile" => FePrimitive::Tile { input: c.input("in") },
        "femorphology" => {
            let (rx, ry) = c.num_pair("radius", 0.0);
            FePrimitive::Morphology {
                input: c.input("in"),
                dilate: c.attr("operator").map(str::trim) == Some("dilate"),
                rx,
                ry,
            }
        }
        "feconvolvematrix" => convolve_matrix(c),
        "fedisplacementmap" => FePrimitive::DisplacementMap {
            input: c.input("in"),
            map: c.input("in2"),
            scale: c.num("scale", 0.0),
            x_channel: channel(c.attr("xChannelSelector")),
            y_channel: channel(c.attr("yChannelSelector")),
        },
        "feturbulence" => {
            let (fx, fy) = c.num_pair("baseFrequency", 0.0);
            let octaves = c.attr("numOctaves").and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(1);
            FePrimitive::Turbulence {
                fx,
                fy,
                octaves: octaves.max(0) as u32,
                seed: c.num("seed", 0.0).round() as i32,
                stitch: c.attr("stitchTiles").map(str::trim) == Some("stitch"),
                fractal: c.attr("type").map(str::trim) == Some("fractalNoise"),
            }
        }
        "fediffuselighting" | "fespecularlighting" => lighting(c, tag == "fespecularlighting"),
        "feimage" => fe_image(c),
        _ => FePrimitive::Unsupported { input: FeInput::Empty },
    }
}

/// `feImage`: ссылка на картинку (`href`/`xlink:href`, не `#id`) — [`FePrimitive::Image`];
/// `#id` на элемент документа — [`FePrimitive::Element`] (отрисовка как у `<use>`);
/// цель без картинки (`<tspan>`, `<defs>`, ссылка в никуда или цикл) даёт
/// [`FePrimitive::Unsupported`] (прозрачный чёрный).
fn fe_image(c: &Ctx<'_>) -> FePrimitive {
    let empty = || FePrimitive::Unsupported { input: FeInput::Empty };
    let href = c.attr("href").or_else(|| c.attr("xlink:href")).map(str::trim).unwrap_or_default();
    if href.is_empty() {
        return empty();
    }
    let Some(id) = href.strip_prefix('#') else {
        return FePrimitive::Image { href: href.to_string(), fit: parse_image_fit(c.attr("preserveAspectRatio")) };
    };
    let Some(target) = c.doc.find_by_id(id) else { return empty() };
    // Цель, чей `filter` снова ведёт в этот `<filter>`, не раскрываем: каскад цели
    // вызвал бы `resolve_filter_url` рекурсивно.
    let Some(_guard) = ElementGuard::enter(c.filter) else { return empty() };
    let content = super::svg::element_picture(c.doc, c.sheet, target, c.viewport, c.svg_size, c.dark_mode);
    if content.is_empty() {
        return empty();
    }
    FePrimitive::Element { picture: FeElementPicture::new(content) }
}

thread_local! {
    /// `<filter>`, чьи `feImage href="#id"` сейчас раскрываются (защита от цикла).
    static EXPANDING: std::cell::RefCell<Vec<NodeId>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Охранник рекурсии раскрытия `feImage` на элемент.
struct ElementGuard;

impl ElementGuard {
    fn enter(filter: NodeId) -> Option<Self> {
        EXPANDING.with(|s| {
            let mut s = s.borrow_mut();
            if s.contains(&filter) || s.len() >= 8 {
                return None;
            }
            s.push(filter);
            Some(Self)
        })
    }
}

impl Drop for ElementGuard {
    fn drop(&mut self) {
        EXPANDING.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// `preserveAspectRatio="[defer] <align> [meet|slice]"`.
fn parse_image_fit(raw: Option<&str>) -> FeImageFit {
    let mut fit = FeImageFit::default();
    let Some(raw) = raw else { return fit };
    let mut words = raw.split_whitespace().peekable();
    if words.peek() == Some(&"defer") {
        words.next();
    }
    match words.next() {
        Some("none") => fit.stretch = true,
        Some(a) if a.len() == 8 && a.starts_with('x') => {
            let axis = |m: &str| match m {
                "Min" => Some(0.0),
                "Mid" => Some(0.5),
                "Max" => Some(1.0),
                _ => None,
            };
            if let (Some(ax), Some(ay)) = (axis(&a[1..4]), a[4..].strip_prefix('Y').and_then(axis)) {
                (fit.align_x, fit.align_y) = (ax, ay);
            }
        }
        _ => {}
    }
    fit.slice = words.next() == Some("slice");
    fit
}

/// `xChannelSelector`/`yChannelSelector`: `R`/`G`/`B`/`A` → 0..3 (по умолчанию `A`).
fn channel(v: Option<&str>) -> usize {
    match v.map(str::trim) {
        Some("R") => 0,
        Some("G") => 1,
        Some("B") => 2,
        _ => 3,
    }
}

/// `feConvolveMatrix`; любой недопустимый атрибут (Filter Effects L1
/// §«feConvolveMatrix») оставляет `kernel` пустым — примитив даёт прозрачный чёрный.
fn convolve_matrix(c: &Ctx<'_>) -> FePrimitive {
    let input = c.input("in");
    let invalid = || FePrimitive::ConvolveMatrix {
        input,
        order: (3, 3),
        kernel: Vec::new(),
        divisor: 1.0,
        bias: 0.0,
        target: (0, 0),
        edge: FeEdgeMode::Duplicate,
        preserve_alpha: false,
    };
    let order = match c.attr("order").map(parse_number_list).as_deref() {
        None => (3.0, 3.0),
        Some([a]) => (*a, *a),
        Some([a, b]) => (*a, *b),
        Some(_) => return invalid(),
    };
    if order.0 < 1.0 || order.1 < 1.0 || order.0.fract() != 0.0 || order.1.fract() != 0.0 {
        return invalid();
    }
    let (ox, oy) = (order.0 as usize, order.1 as usize);
    let kernel = c.attr("kernelMatrix").map(parse_number_list).unwrap_or_default();
    if kernel.len() != ox * oy {
        return invalid();
    }
    let sum: f32 = kernel.iter().sum();
    // Нулевой или отсутствующий `divisor` заменяется суммой ядра (0 → 1).
    let divisor = match c.attr("divisor").and_then(|v| v.trim().parse::<f32>().ok()) {
        Some(d) if d != 0.0 => d,
        _ if sum != 0.0 => sum,
        _ => 1.0,
    };
    let target = |name: &str, len: usize| match c.attr(name).and_then(|v| v.trim().parse::<i32>().ok()) {
        Some(t) => (0..len as i32).contains(&t).then_some(t),
        None => Some((len / 2) as i32),
    };
    let (Some(tx), Some(ty)) = (target("targetX", ox), target("targetY", oy)) else {
        return invalid();
    };
    FePrimitive::ConvolveMatrix {
        input,
        order: (ox, oy),
        kernel,
        divisor,
        bias: c.num("bias", 0.0),
        target: (tx, ty),
        edge: match c.attr("edgeMode").map(str::trim) {
            Some("wrap") => FeEdgeMode::Wrap,
            Some("none") => FeEdgeMode::None,
            _ => FeEdgeMode::Duplicate,
        },
        preserve_alpha: c.attr("preserveAlpha").map(str::trim) == Some("true"),
    }
}

/// `feDiffuseLighting` / `feSpecularLighting` с первым источником света среди детей.
fn lighting(c: &Ctx<'_>, specular: bool) -> FePrimitive {
    let kind = if specular {
        FeLighting::Specular { ks: c.num("specularConstant", 1.0), exponent: c.num("specularExponent", 1.0) }
    } else {
        FeLighting::Diffuse { kd: c.num("diffuseConstant", 1.0) }
    };
    let valid = match kind {
        FeLighting::Diffuse { kd } => kd >= 0.0,
        FeLighting::Specular { ks, exponent } => ks >= 0.0 && (1.0..=128.0).contains(&exponent),
    };
    let mut light = None;
    for &n in &c.doc.get(c.node).children {
        let Some(name) = c.doc.get(n).element_name() else { continue };
        let sub = Ctx { node: n, ..*c };
        let triple = |x: &str, y: &str, z: &str| [sub.num(x, 0.0), sub.num(y, 0.0), sub.num(z, 0.0)];
        light = Some(match name.local.as_str().to_ascii_lowercase().as_str() {
            "fedistantlight" => FeLight::Distant {
                azimuth: sub.num("azimuth", 0.0),
                elevation: sub.num("elevation", 0.0),
            },
            "fepointlight" => FeLight::Point { pos: triple("x", "y", "z") },
            "fespotlight" => FeLight::Spot {
                pos: triple("x", "y", "z"),
                points_at: triple("pointsAtX", "pointsAtY", "pointsAtZ"),
                exponent: sub.num("specularExponent", 1.0),
                cone: sub.attr("limitingConeAngle").and_then(|v| v.trim().parse::<f32>().ok()),
            },
            _ => continue,
        });
        break;
    }
    FePrimitive::Lighting {
        input: c.input("in"),
        kind,
        surface_scale: c.num("surfaceScale", 1.0),
        color: c.color_prop("lighting-color", Color::WHITE),
        light: if valid { light } else { None },
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

/// Значение свойства упоминает `currentcolor` (в том числе внутри `color-mix()`,
/// `contrast-color()`, `color(from …)`).
fn uses_current_color(doc: &Document, n: NodeId, prop: &str) -> bool {
    svg_prop(doc, n, prop).is_some_and(|v| v.to_ascii_lowercase().contains("currentcolor"))
}

fn is_tag(doc: &Document, n: NodeId, tag: &str) -> bool {
    doc.get(n).element_name().is_some_and(|q| q.local.as_str().eq_ignore_ascii_case(tag))
}

/// `color-interpolation-filters`: `linearRGB` (по умолчанию) или `sRGB`/`auto`.
/// У примитива без значения наследуется значение `<filter>` (`inherited`); у самого
/// `<filter>` (`inherited == None`) — значение ближайшего предка (свойство наследуемое).
fn interpolation_is_linear(doc: &Document, n: NodeId, inherited: Option<bool>) -> bool {
    match svg_prop(doc, n, "color-interpolation-filters").as_deref().map(str::trim) {
        Some("linearRGB") => true,
        Some("sRGB" | "auto") => false,
        _ => match (inherited, doc.get(n).parent) {
            (Some(v), _) => v,
            (None, Some(parent)) if doc.get(parent).element_name().is_some() => interpolation_is_linear(doc, parent, None),
            (None, _) => true,
        },
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
/// раскрывается от SVG viewport `basis` по оси `axis` (0 — x/width, 1 — y/height).
/// Второй элемент — `true`, если процент остался долей (`basis == None`, HTML-бокс).
fn parse_len(raw: &str, obb: bool, basis: Option<Size>, axis: usize, lc: LenCtx) -> Option<(f32, bool)> {
    let raw = raw.trim();
    if let Some(p) = raw.strip_suffix('%')
        && let Ok(v) = p.trim().parse::<f32>()
    {
        let v = v / 100.0;
        return Some(if obb { (v, false) } else { resolve_pct(v, basis, axis) });
    }
    if let Ok(v) = raw.strip_suffix("px").unwrap_or(raw).trim().parse::<f32>() {
        return Some((v, false));
    }
    // `1em`, `calc(25% + 0.25px)` и прочие CSS `<length>`: в `objectBoundingBox` число
    // px — доля bbox, `%` внутри выражения считается от 1.
    let len = crate::style::parse_length(raw)?;
    let pct_basis = if obb {
        1.0
    } else {
        let b = basis.unwrap_or(lc.viewport);
        if axis == 0 { b.width } else { b.height }
    };
    len.resolve(lc.em, Some(pct_basis), lc.viewport).map(|v| (v, false))
}

/// Что нужно CSS-длине атрибута `<filter>`: `em` и размер окна для `vw`/`vh`.
#[derive(Clone, Copy)]
struct LenCtx {
    em: f32,
    viewport: Size,
}

/// Доля viewport → user units, если база известна; иначе доля остаётся с флагом.
fn resolve_pct(frac: f32, basis: Option<Size>, axis: usize) -> (f32, bool) {
    match basis {
        Some(b) => (frac * if axis == 0 { b.width } else { b.height }, false),
        None => (frac, true),
    }
}

/// Размер ближайшего SVG viewport для `node`, лежащего внутри `<svg>`: `viewBox`
/// или числовые `width`/`height` ближайшего предка `<svg>` (нет — окно документа).
/// `None` — `node` не потомок `<svg>` (HTML-бокс или внешний `<svg>`).
fn nearest_svg_viewport(doc: &Document, node: NodeId, viewport: Size) -> Option<Size> {
    let svg = {
        let mut cur = doc.get(node).parent;
        loop {
            let n = cur?;
            if is_tag(doc, n, "svg") {
                break n;
            }
            cur = doc.get(n).parent;
        }
    };
    if let Some(vb) = super::svg::parse_view_box(doc, svg) {
        return Some(Size { width: vb.width, height: vb.height });
    }
    let dim = |attr: &str, fallback: f32| {
        doc.get(svg)
            .get_attr(attr)
            .and_then(|v| v.trim().strip_suffix("px").unwrap_or(v.trim()).trim().parse::<f32>().ok())
            .unwrap_or(fallback)
    };
    Some(Size { width: dim("width", viewport.width), height: dim("height", viewport.height) })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(html: &str, referrer: &str) -> Arc<SvgFilter> {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse("");
        let node = doc.find_by_id(referrer).expect("referrer");
        resolve_filter_url(&doc, &sheet, "f", node, Size::new(800.0, 600.0), false).expect("filter")
    }

    const FILTER: &str = r#"<filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="100%" height="50%">
        <feFlood width="50%" height="1em"/></filter>"#;

    /// Проценты `userSpaceOnUse` у потомка `<svg width height>` берутся от размера этого `<svg>`.
    #[test]
    fn user_space_percent_uses_nearest_svg_size() {
        let g = graph(&format!(r#"<svg width="150" height="120">{FILTER}<rect id="r"/></svg>"#), "r");
        assert_eq!(g.region, (0.0, 0.0, 150.0, 60.0));
        assert_eq!(g.region_pct, [false; 4]);
        assert_eq!(g.primitives[0].width, Some(75.0));
        assert_eq!(g.primitives[0].pct, [false; 4]);
    }

    /// `viewBox` перекрывает `width`/`height`.
    #[test]
    fn user_space_percent_prefers_view_box() {
        let g = graph(&format!(r#"<svg width="150" height="120" viewBox="0 0 40 20">{FILTER}<rect id="r"/></svg>"#), "r");
        assert_eq!(g.region, (0.0, 0.0, 40.0, 10.0));
    }

    /// У HTML-бокса база — его border box, известный только paint: остаётся доля с флагом.
    #[test]
    fn user_space_percent_on_html_box_stays_fraction() {
        let g = graph(&format!(r#"<svg width="0" height="0">{FILTER}</svg><div id="d"></div>"#), "d");
        assert_eq!(g.region, (0.0, 0.0, 1.0, 0.5));
        assert_eq!(g.region_pct, [false, false, true, true]);
        assert_eq!(g.primitives[0].width, Some(0.5));
        assert_eq!(g.primitives[0].pct, [false, false, true, false]);
    }

    /// `em` и `calc()` в атрибутах области: в `objectBoundingBox` число px — доля bbox,
    /// `%` внутри `calc()` считается от 1; `font-size` читается с самого `<filter>`.
    #[test]
    fn region_accepts_em_and_calc_in_object_bounding_box() {
        let g = graph(
            r#"<svg width="0" height="0"><filter id="f" x="0" y="0" width="1em" height="calc(25% + 0.25px)" font-size="10px"/></svg><div id="d"></div>"#,
            "d",
        );
        assert_eq!(g.region, (0.0, 0.0, 10.0, 0.5));
    }

    /// `color-interpolation-filters` наследуется: `<svg color-interpolation-filters="sRGB">`
    /// действует на `<filter>` и его примитивы, собственное значение перекрывает.
    #[test]
    fn color_interpolation_filters_inherits_from_ancestors() {
        let g = graph(
            r#"<svg width="0" height="0" color-interpolation-filters="sRGB"><filter id="f"><feFlood/>
            <feOffset color-interpolation-filters="linearRGB"/></filter></svg><div id="d"></div>"#,
            "d",
        );
        assert!(!g.primitives[0].linear_rgb);
        assert!(g.primitives[1].linear_rgb);
        let g = graph(r#"<svg width="0" height="0"><filter id="f"><feFlood/></filter></svg><div id="d"></div>"#, "d");
        assert!(g.primitives[0].linear_rgb, "умолчание — linearRGB");
    }
}

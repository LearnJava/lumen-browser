//! SVG `<clipPath>` → полигоны (SVG 1.1 §14.3, CSS Masking L1 §3.1).
//!
//! `clip-path: url(#id)` / `clip-path="url(#id)"` хранится после каскада как
//! [`ClipPath::Url`]; здесь, где есть `Document`, ссылка разворачивается в
//! [`ClipPath::Svg`]: дети `<clipPath>` (`rect`, `circle`, `ellipse`, `polygon`,
//! `polyline`, `path`, `use` на фигуру) сводятся к полигонам в системе
//! координат `<clipPath>`, объединение которых и есть видимая область.
//! `<text>` в клипе и собственный `clip-path` детей не поддержаны.

use std::cell::RefCell;
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::Stylesheet;
use lumen_dom::{Document, NodeId};

use super::svg::{SvgAxis, SvgLenCtx, SvgTransform, parse_svg_points, parse_svg_transform, svg_shape_bbox};
use super::{BoxKind, LayoutBox, SvgShapeKind};
use crate::style::{
    ClipPath, ComputedStyle, Display, FillRule, SvgClipPath, SvgClipPlacement, SvgClipSubpath,
    Visibility, compute_style,
};

/// Сегментов на окружность/эллипс: ошибка хорды `r·(1 − cos(π/N))` < 0.0014·r.
/// Не больше `PATH_CLIP_MAX_VERTS` (64) шейдера клипа wgpu — иначе окно
/// откатится на клип по bbox.
const ARC_SEGMENTS: usize = 60;
/// Сегментов на скруглённый угол `<rect rx ry>` (4·(N+1) ≤ 64).
const CORNER_SEGMENTS: usize = 12;

thread_local! {
    /// `<clipPath>`-ы, раскрываемые сейчас: ссылка на уже раскрываемый
    /// элемент — цикл (`clip-path-recursion-*`).
    static ACTIVE: RefCell<Vec<NodeId>> = const { RefCell::new(Vec::new()) };
}

/// Разворачивает `url(#id)` в [`ClipPath::Svg`]. `None` — элемента нет или это
/// не `<clipPath>`: свойство считается не заданным (CSS Masking L1 §3.1).
/// Циклическая ссылка даёт клип с пустой областью (элемент не виден).
pub(crate) fn resolve_clip_path_url(
    doc: &Document,
    sheet: &Stylesheet,
    id: &str,
    viewport: Size,
    dark_mode: bool,
) -> Option<ClipPath> {
    let cp = doc.find_by_id(id)?;
    if !doc.get(cp).element_name()?.local.as_str().eq_ignore_ascii_case("clipPath") {
        return None;
    }
    if ACTIVE.with(|a| a.borrow().contains(&cp)) {
        return Some(ClipPath::Svg(Arc::new(SvgClipPath {
            object_bbox_units: false,
            subpaths: Vec::new(),
            nested: None,
            placement: None,
        })));
    }
    ACTIVE.with(|a| a.borrow_mut().push(cp));
    let built = build_clip_path(doc, sheet, cp, viewport, dark_mode);
    ACTIVE.with(|a| a.borrow_mut().pop());
    Some(ClipPath::Svg(Arc::new(built)))
}

fn build_clip_path(
    doc: &Document,
    sheet: &Stylesheet,
    cp: NodeId,
    viewport: Size,
    dark_mode: bool,
) -> SvgClipPath {
    let cp_style = compute_style(doc, cp, sheet, &ComputedStyle::root(), viewport, dark_mode);
    let cp_xf = parse_svg_transform(doc.get(cp).get_attr("transform"));
    let mut subpaths = Vec::new();
    for &child in &doc.get(cp).children {
        collect_child(doc, sheet, child, &cp_style, &cp_xf, viewport, dark_mode, &mut subpaths);
    }
    SvgClipPath {
        object_bbox_units: doc
            .get(cp)
            .get_attr("clipPathUnits")
            .is_some_and(|v| v.trim() == "objectBoundingBox"),
        subpaths,
        nested: cp_style.clip_path.clone().map(Box::new),
        placement: None,
    }
}

/// Добавляет полигоны одного ребёнка `<clipPath>` (или цели его `<use>`).
/// `xf` — преобразование из системы координат родителя в систему `<clipPath>`.
#[allow(clippy::too_many_arguments)]
fn collect_child(
    doc: &Document,
    sheet: &Stylesheet,
    child: NodeId,
    parent_style: &ComputedStyle,
    xf: &SvgTransform,
    viewport: Size,
    dark_mode: bool,
    out: &mut Vec<SvgClipSubpath>,
) {
    let Some(name) = doc.get(child).element_name() else { return };
    let tag = name.local.as_str();
    let style = compute_style(doc, child, sheet, parent_style, viewport, dark_mode);
    // Скрытые дети в клип не входят (SVG 1.1 §14.3.5).
    if style.display == Display::None || style.visibility != Visibility::Visible {
        return;
    }
    let lc = SvgLenCtx { doc, id: child, em: style.font_size, viewport, own_svg_size: viewport };
    let node = doc.get(child);
    let mut local = xf.clone();
    local.compose(&parse_svg_transform(node.get_attr("transform")));
    let ring: Vec<(f32, f32)> = match tag {
        "rect" => rect_ring(
            [
                lc.len("x", SvgAxis::X),
                lc.len("y", SvgAxis::Y),
                lc.len("width", SvgAxis::X),
                lc.len("height", SvgAxis::Y),
            ],
            [lc.len("rx", SvgAxis::X), lc.len("ry", SvgAxis::Y)],
            [node.get_attr("rx").is_some(), node.get_attr("ry").is_some()],
        ),
        "circle" => {
            let r = lc.len("r", SvgAxis::Diagonal);
            ellipse_ring(lc.len("cx", SvgAxis::X), lc.len("cy", SvgAxis::Y), r, r)
        }
        "ellipse" => ellipse_ring(
            lc.len("cx", SvgAxis::X),
            lc.len("cy", SvgAxis::Y),
            lc.len("rx", SvgAxis::X),
            lc.len("ry", SvgAxis::Y),
        ),
        "polygon" | "polyline" => parse_svg_points(node.get_attr("points").unwrap_or("")),
        "path" => join_contours(crate::motion_path::flatten_path_to_subpaths(
            node.get_attr("d").unwrap_or(""),
        )),
        "use" => {
            let href = node
                .get_attr("href")
                .or_else(|| node.get_attr("xlink:href"))
                .unwrap_or("")
                .trim()
                .trim_start_matches('#');
            let Some(target) = doc.find_by_id(href) else { return };
            // Только фигура: `<use>` на `<g>`/другой `<use>` в клип не входит.
            let target_is_shape = doc.get(target).element_name().is_some_and(|n| {
                matches!(
                    n.local.as_str(),
                    "rect" | "circle" | "ellipse" | "polygon" | "polyline" | "path"
                )
            });
            if !target_is_shape {
                return;
            }
            let mut at_use = local.clone();
            at_use.compose(&SvgTransform::translate(
                lc.len("x", SvgAxis::X),
                lc.len("y", SvgAxis::Y),
            ));
            collect_child(doc, sheet, target, &style, &at_use, viewport, dark_mode, out);
            return;
        }
        _ => return,
    };
    if ring.len() < 3 {
        return;
    }
    out.push(SvgClipSubpath {
        pts: ring.iter().map(|&(x, y)| local.transform_point(x, y)).collect(),
        even_odd: matches!(style.svg_clip_rule, FillRule::EvenOdd),
    });
}

/// Склеивает контуры одной фигуры (подпути `<path>`) в один замкнутый список
/// вершин «мостиками»: от начала первого контура к началу очередного и обратно.
/// Ребро туда и обратно взаимно гасится, поэтому и nonzero, и even-odd
/// заливка видят исходные контуры (дырки сохраняются).
fn join_contours(contours: Vec<Vec<(f32, f32)>>) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::new();
    for c in contours.into_iter().filter(|c| c.len() >= 3) {
        let first = c[0];
        if let Some(&anchor) = out.first() {
            out.push(anchor);
        }
        out.extend(c);
        out.push(first);
    }
    out
}

/// Контур `<rect>`: `[x, y, w, h]`, `[rx, ry]` и признак «атрибут задан».
/// Если задан только один радиус, второй равен ему; каждый ограничен
/// половиной стороны (SVG 1.1 §9.2).
fn rect_ring(geom: [f32; 4], radii: [f32; 2], given: [bool; 2]) -> Vec<(f32, f32)> {
    let [x, y, w, h] = geom;
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    let (mut rx, mut ry) = (radii[0].max(0.0), radii[1].max(0.0));
    match given {
        [true, false] => ry = rx,
        [false, true] => rx = ry,
        _ => {}
    }
    rx = rx.min(w / 2.0);
    ry = ry.min(h / 2.0);
    if rx <= 0.0 || ry <= 0.0 {
        return vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    }
    let mut pts = Vec::with_capacity(4 * (CORNER_SEGMENTS + 1));
    // Углы по часовой стрелке, начиная с правого верхнего.
    let corners = [
        (x + w - rx, y + ry, -std::f32::consts::FRAC_PI_2),
        (x + w - rx, y + h - ry, 0.0),
        (x + rx, y + h - ry, std::f32::consts::FRAC_PI_2),
        (x + rx, y + ry, std::f32::consts::PI),
    ];
    for (cx, cy, start) in corners {
        for i in 0..=CORNER_SEGMENTS {
            let a = start + std::f32::consts::FRAC_PI_2 * i as f32 / CORNER_SEGMENTS as f32;
            pts.push((cx + rx * a.cos(), cy + ry * a.sin()));
        }
    }
    pts
}

/// Контур эллипса; нулевой радиус — пустая фигура.
fn ellipse_ring(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<(f32, f32)> {
    if rx <= 0.0 || ry <= 0.0 {
        return Vec::new();
    }
    (0..ARC_SEGMENTS)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / ARC_SEGMENTS as f32;
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// Записывает в `ClipPath::Svg` бокса положение его user space в документе
/// (`m_doc`: user space → документ). Вызывается после того, как `b.rect` уже
/// выставлен: смещение матрицы хранится относительно `rect.x/y`, чтобы сдвиг
/// дерева (`shift_tree`) не рассинхронизировал клип с боксом.
pub(super) fn place_clip_path(b: &mut LayoutBox, m_doc: &SvgTransform) {
    let Some(ClipPath::Svg(def)) = &b.style.clip_path else { return };
    let user_bbox = user_bbox_of(b, m_doc);
    let mut matrix = m_doc.matrix;
    matrix[4] -= b.rect.x;
    matrix[5] -= b.rect.y;
    let placement = SvgClipPlacement { matrix, user_bbox };
    let placed = with_placement(def, placement);
    Arc::make_mut(&mut b.style).clip_path = Some(ClipPath::Svg(Arc::new(placed)));
}

/// Копия клипа (и вложенных `clip-path` самого `<clipPath>`) с положением.
fn with_placement(def: &SvgClipPath, placement: SvgClipPlacement) -> SvgClipPath {
    let mut out = def.clone();
    out.placement = Some(placement);
    if let Some(nested) = &def.nested
        && let ClipPath::Svg(inner) = nested.as_ref()
    {
        out.nested = Some(Box::new(ClipPath::Svg(Arc::new(with_placement(inner, placement)))));
    }
    out
}

/// bounding box элемента в его user space (`clipPathUnits="objectBoundingBox"`).
pub(super) fn user_bbox_of(b: &LayoutBox, m_doc: &SvgTransform) -> (f32, f32, f32, f32) {
    if let BoxKind::SvgShape { shape, .. } = &b.kind {
        match shape {
            SvgShapeKind::Path { d } => {
                let ring = crate::motion_path::flatten_path_to_polygon(d);
                if let Some(bb) = points_bbox(ring.iter().copied()) {
                    return bb;
                }
            }
            other => {
                let r = svg_shape_bbox(other, 0.0, 0.0, 1.0, 1.0);
                return (r.x, r.y, r.width, r.height);
            }
        }
    }
    // Контейнеры (`<g>`) и остальное: документный bbox, пересчитанный обратно.
    let [a, bb, c, d, e, f] = m_doc.matrix;
    let det = a * d - bb * c;
    if det.abs() < f32::EPSILON {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let inv = |x: f32, y: f32| {
        let (x, y) = (x - e, y - f);
        ((d * x - c * y) / det, (-bb * x + a * y) / det)
    };
    let r = b.rect;
    let corners = [(r.x, r.y), (r.x + r.width, r.y), (r.x, r.y + r.height), (r.x + r.width, r.y + r.height)];
    points_bbox(corners.iter().map(|&(x, y)| inv(x, y))).unwrap_or((0.0, 0.0, 0.0, 0.0))
}

fn points_bbox(pts: impl Iterator<Item = (f32, f32)>) -> Option<(f32, f32, f32, f32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any = false;
    for (x, y) in pts {
        any = true;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    any.then_some((x0, y0, x1 - x0, y1 - y0))
}

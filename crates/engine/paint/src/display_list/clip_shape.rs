//! `clip-path: <basic-shape> || <geometry-box>` → точная форма в page-координатах.
//!
//! Форма считается от reference box (CSS Masking L1 §4.5 `<geometry-box>`): по
//! умолчанию border-box, иначе padding/content/margin. Для CSS-боксов `fill-box`
//! совпадает с content-box, `stroke-box`/`view-box` — с border-box (как `mask-clip`).

use super::*;

use lumen_layout::GeometryBox;

/// Reference box-ы элемента для `clip-path`.
#[derive(Clone, Copy)]
pub(super) struct ClipRefs {
    border: Rect,
    padding: Rect,
    content: Rect,
    margin: Rect,
}

impl From<Rect> for ClipRefs {
    /// Все боксы совпадают с `r` (SVG-фигуры, вложенный `clip-path` у `<clipPath>`, тесты).
    fn from(r: Rect) -> Self {
        Self { border: r, padding: r, content: r, margin: r }
    }
}

impl ClipRefs {
    pub(super) fn of_box(b: &LayoutBox) -> Self {
        let s = &b.style;
        let m = |v: &lumen_layout::LengthOrAuto| v.to_px_opt().unwrap_or(0.0);
        let (mt, mr, mb, ml) = (m(&s.margin_top), m(&s.margin_right), m(&s.margin_bottom), m(&s.margin_left));
        Self {
            border: b.rect,
            padding: background_clip_rect(b, BackgroundClip::PaddingBox),
            content: background_clip_rect(b, BackgroundClip::ContentBox),
            margin: Rect::new(
                b.rect.x - ml,
                b.rect.y - mt,
                (b.rect.width + ml + mr).max(0.0),
                (b.rect.height + mt + mb).max(0.0),
            ),
        }
    }

    fn pick(&self, g: GeometryBox) -> Rect {
        match g {
            GeometryBox::BorderBox | GeometryBox::StrokeBox | GeometryBox::ViewBox => self.border,
            GeometryBox::PaddingBox => self.padding,
            GeometryBox::ContentBox | GeometryBox::FillBox => self.content,
            GeometryBox::MarginBox => self.margin,
        }
    }

    pub(super) fn border(&self) -> Rect {
        self.border
    }
}

/// BUG-140: резолвит `clip-path` в точную форму в page-координатах
/// (пространство до transform элемента). `None` для фигур, точно
/// представимых прямоугольником (`inset(...)` без `round`, голый `<geometry-box>`):
/// они эмитятся как `PushClipRect` (см. `clip_path_to_rect`). Базисы процентов —
/// CSS Shapes L1 §5: x/width, y/height, радиус circle — `sqrt(w²+h²)/√2`.
pub(super) fn clip_path_to_shape(clip: &ClipPath, refs: impl Into<ClipRefs>) -> Option<ResolvedClipShape> {
    let refs = refs.into();
    let r = refs.border;
    match clip {
        ClipPath::Inset(_) | ClipPath::Url(_) | ClipPath::Svg(_) => None,
        ClipPath::Boxed { shape: None, .. } => None,
        ClipPath::Boxed { shape: Some(inner), geometry } => clip_path_to_shape(inner, refs.pick(*geometry)),
        ClipPath::InsetRound { sides, rx, ry } => inset_round_polygon(sides, rx, ry, r),
        ClipPath::Circle { radius, center } => {
            let (cx, cy) = resolve_shape_center(*center, r);
            let diag = ((r.width * r.width + r.height * r.height) * 0.5).sqrt();
            let (dl, dr, dt, db) = edge_distances(cx, cy, r);
            let radius = match radius {
                ShapeValue::ClosestSide => dl.min(dr).min(dt).min(db),
                ShapeValue::FarthestSide => dl.max(dr).max(dt).max(db),
                v => v.resolve(diag),
            };
            Some(ResolvedClipShape::Circle { cx, cy, r: radius })
        }
        ClipPath::Ellipse { rx, ry, center } => {
            let (cx, cy) = resolve_shape_center(*center, r);
            let (dl, dr, dt, db) = edge_distances(cx, cy, r);
            let rx = match rx {
                ShapeValue::ClosestSide => dl.min(dr),
                ShapeValue::FarthestSide => dl.max(dr),
                v => v.resolve(r.width),
            };
            let ry = match ry {
                ShapeValue::ClosestSide => dt.min(db),
                ShapeValue::FarthestSide => dt.max(db),
                v => v.resolve(r.height),
            };
            Some(ResolvedClipShape::Ellipse { cx, cy, rx, ry })
        }
        ClipPath::Polygon(vertices, fill_rule) => {
            if vertices.is_empty() {
                return None;
            }
            Some(ResolvedClipShape::Polygon {
                verts: vertices
                    .iter()
                    .map(|(x, y)| (r.x + x.resolve(r.width), r.y + y.resolve(r.height)))
                    .collect(),
                even_odd: matches!(fill_rule, FillRule::EvenOdd),
            })
        }
        // CSS Shapes L1 §4 — `path()`: точки уже флэттены в px системы пути
        // (origin = верхний левый угол reference box). Смещаем на позицию box.
        ClipPath::Path(points, fill_rule) => {
            if points.len() < 3 {
                return None;
            }
            Some(ResolvedClipShape::Polygon {
                verts: points.iter().map(|(x, y)| (r.x + x, r.y + y)).collect(),
                even_odd: matches!(fill_rule, FillRule::EvenOdd),
            })
        }
    }
}

/// Расстояния от центра до левой/правой/верхней/нижней стороны `r` (модули).
fn edge_distances(cx: f32, cy: f32, r: Rect) -> (f32, f32, f32, f32) {
    ((cx - r.x).abs(), (r.x + r.width - cx).abs(), (cy - r.y).abs(), (r.y + r.height - cy).abs())
}

/// Прямоугольник `inset(top right bottom left)` в reference box `r`.
pub(super) fn inset_rect(sides: &[ShapeValue], r: Rect) -> Rect {
    let rs = |v: &ShapeValue, basis: f32| v.resolve(basis);
    let (top, right, bottom, left) = match sides {
        [a] => (rs(a, r.height), rs(a, r.width), rs(a, r.height), rs(a, r.width)),
        [tb, rl] => (rs(tb, r.height), rs(rl, r.width), rs(tb, r.height), rs(rl, r.width)),
        [t, rl, b] => (rs(t, r.height), rs(rl, r.width), rs(b, r.height), rs(rl, r.width)),
        [t, ri, b, l] => (rs(t, r.height), rs(ri, r.width), rs(b, r.height), rs(l, r.width)),
        _ => (0.0, 0.0, 0.0, 0.0),
    };
    Rect::new(r.x + left, r.y + top, (r.width - left - right).max(0.0), (r.height - top - bottom).max(0.0))
}

/// Bounding rect фигуры из [`clip_path_to_shape`] либо её точное прямоугольное
/// представление (`inset`, голый `<geometry-box>`). Для `Url`/`Svg` — border box.
pub(super) fn clip_path_to_rect(clip: &ClipPath, refs: impl Into<ClipRefs>) -> Rect {
    let refs = refs.into();
    if let Some(shape) = clip_path_to_shape(clip, refs) {
        return shape.bounding_rect();
    }
    match clip {
        ClipPath::Inset(sides) => inset_rect(sides, refs.border),
        ClipPath::Boxed { shape: None, geometry } => refs.pick(*geometry),
        ClipPath::Boxed { shape: Some(inner), geometry } => clip_path_to_rect(inner, refs.pick(*geometry)),
        _ => refs.border,
    }
}

/// Раскрывает 1..=4 значений `border-radius`-стиля в углы `[tl, tr, br, bl]`.
fn expand_corners(v: &[ShapeValue]) -> Option<[ShapeValue; 4]> {
    match v {
        [a] => Some([*a; 4]),
        [a, b] => Some([*a, *b, *a, *b]),
        [a, b, c] => Some([*a, *b, *c, *b]),
        [a, b, c, d] => Some([*a, *b, *c, *d]),
        _ => None,
    }
}

/// `inset(… round …)` как многоугольник с дугами в углах. `None` — радиусы нулевые
/// (форма — прямоугольник).
fn inset_round_polygon(
    sides: &[ShapeValue],
    rx: &[ShapeValue],
    ry: &[ShapeValue],
    r: Rect,
) -> Option<ResolvedClipShape> {
    const SEGMENTS: usize = 16;
    let b = inset_rect(sides, r);
    let hx = expand_corners(rx)?;
    let vy = if ry.is_empty() { hx } else { expand_corners(ry)? };
    // Проценты радиусов — от ширины/высоты reference box (CSS Shapes L1 §3.3).
    let mut rxs = hx.map(|v| v.resolve(r.width).max(0.0));
    let mut rys = vy.map(|v| v.resolve(r.height).max(0.0));
    // CSS Backgrounds L3 §5.5: суммы радиусов на стороне не больше стороны.
    let mut f = 1.0f32;
    for (sum, len) in [
        (rxs[0] + rxs[1], b.width),
        (rxs[3] + rxs[2], b.width),
        (rys[0] + rys[3], b.height),
        (rys[1] + rys[2], b.height),
    ] {
        if sum > len && sum > 0.0 {
            f = f.min(len / sum);
        }
    }
    rxs = rxs.map(|v| v * f);
    rys = rys.map(|v| v * f);
    if rxs.iter().chain(rys.iter()).all(|v| *v <= 0.0) {
        return None;
    }
    let (x0, y0, x1, y1) = (b.x, b.y, b.x + b.width, b.y + b.height);
    // Центры дуг и начальный угол (экранные координаты, y вниз): tl, tr, br, bl.
    let arcs = [
        (x0 + rxs[0], y0 + rys[0], 180.0f32),
        (x1 - rxs[1], y0 + rys[1], 270.0),
        (x1 - rxs[2], y1 - rys[2], 0.0),
        (x0 + rxs[3], y1 - rys[3], 90.0),
    ];
    let mut verts = Vec::with_capacity(4 * (SEGMENTS + 1));
    for (i, (cx, cy, start)) in arcs.into_iter().enumerate() {
        for k in 0..=SEGMENTS {
            let a = (start + 90.0 * k as f32 / SEGMENTS as f32).to_radians();
            verts.push((cx + rxs[i] * a.cos(), cy + rys[i] * a.sin()));
        }
    }
    Some(ResolvedClipShape::Polygon { verts, even_odd: false })
}

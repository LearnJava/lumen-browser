//! SVG-CLIPPATH: `clip-path: url(#id)` → команды клипа дисплей-листа.
//! `ClipPath::Svg` (layout уже свёл `<clipPath>` к полигонам в его системе
//! координат) превращается в `PushClipPath { Polygon }` в page-координатах;
//! вложенный `clip-path` самого `<clipPath>` даёт ещё один слой (пересечение).

use super::*;

use lumen_layout::SvgClipPath;

/// Команды `Push*`, открывающие `clip-path` элемента с border-box `r`.
/// Каждой команде соответствует свой `PopClip`; число слоёв — `len()`.
/// Ссылка `url(#id)`, не разрешённая layout-ом, ничего не клипует.
pub(super) fn clip_path_commands(clip: &ClipPath, refs: impl Into<ClipRefs>) -> Vec<DisplayCommand> {
    let refs = refs.into();
    match clip {
        ClipPath::Url(_) => Vec::new(),
        ClipPath::Svg(def) => svg_clip_layers(def, refs.border()),
        other => match clip_path_to_shape(other, refs) {
            Some(shape) => vec![DisplayCommand::PushClipPath { shape }],
            None => vec![DisplayCommand::PushClipRect { rect: clip_path_to_rect(other, refs) }],
        },
    }
}

/// Попадает ли точка `p` (система элемента, до его transform) в область `clip-path`
/// бокса `b` (BUG-1412). Пересечение всех слоёв клипа; без `clip-path` и для
/// неразрешённой `url(#id)` — `true`, как и в отрисовке (`clip_path_commands`).
pub(crate) fn clip_path_contains(b: &LayoutBox, p: lumen_core::geom::Point) -> bool {
    let Some(clip) = &b.style.clip_path else { return true };
    clip_path_commands(clip, ClipRefs::of_box(b)).iter().all(|cmd| match cmd {
        DisplayCommand::PushClipPath { shape } => shape.contains(p.x, p.y),
        DisplayCommand::PushClipRect { rect } => {
            p.x >= rect.x && p.x < rect.x + rect.width && p.y >= rect.y && p.y < rect.y + rect.height
        }
        _ => true,
    })
}

/// Открывает `clip-path` SVG-фигуры (у неё нет блочного пути `walk`, где клип
/// эмитится для боксов): координаты фигуры уже документные, `PushTransform` нет.
/// Возвращает число слоёв — столько же `PopClip` даст [`pop_clip_layers`].
pub(super) fn push_shape_clip_path(b: &LayoutBox, out: &mut DisplayList) -> usize {
    let Some(clip) = &b.style.clip_path else { return 0 };
    let cmds = clip_path_commands(clip, ClipRefs::of_box(b));
    let n = cmds.len();
    out.extend(cmds);
    n
}

/// SVG-фигура/текст не идут через `box_layer` (у них нет слоёв Push*/Pop* в блочном
/// пути), поэтому `filter` оборачивает их отрисовку здесь — внутри клипа
/// (Filter Effects L1 §4: клип режет уже отфильтрованный вывод).
pub(super) fn push_shape_filter(b: &LayoutBox, out: &mut DisplayList) -> bool {
    if b.style.filter.is_empty() {
        return false;
    }
    out.push(DisplayCommand::PushFilter { filters: b.style.filter.clone(), bounds: Some(b.rect) });
    true
}

/// Закрывает фильтр, открытый [`push_shape_filter`].
pub(super) fn pop_shape_filter(opened: bool, out: &mut DisplayList) {
    if opened {
        out.push(DisplayCommand::PopFilter);
    }
}

/// Закрывает `n` слоёв клипа, открытых [`push_shape_clip_path`].
pub(super) fn pop_clip_layers(n: usize, out: &mut DisplayList) {
    for _ in 0..n {
        out.push(DisplayCommand::PopClip);
    }
}

/// Слои клипа `<clipPath>`: сам клип, затем его вложенный `clip-path`.
fn svg_clip_layers(def: &SvgClipPath, r: Rect) -> Vec<DisplayCommand> {
    let mut out = Vec::new();
    match svg_clip_shape(def, r) {
        Some(shape) => out.push(DisplayCommand::PushClipPath { shape }),
        // Нет видимых фигур: клип отсекает всё.
        None => out.push(DisplayCommand::PushClipRect { rect: Rect::new(r.x, r.y, 0.0, 0.0) }),
    }
    if let Some(nested) = &def.nested {
        out.extend(clip_path_commands(nested, r));
    }
    out
}

/// Объединение фигур `<clipPath>` в page-координатах (до transform элемента).
/// `None` — область пуста.
///
/// Одна фигура с `clip-rule: evenodd` остаётся even-odd. Несколько фигур
/// склеиваются в один контур «мостиками» (ребро туда и обратно взаимно
/// гасится), все — с одной ориентацией, поэтому правило nonzero даёт их
/// объединение; `evenodd`-дырки при нескольких фигурах не передаются.
fn svg_clip_shape(def: &SvgClipPath, r: Rect) -> Option<ResolvedClipShape> {
    let (m, bbox) = match &def.placement {
        Some(p) => {
            let mut m = p.matrix;
            m[4] += r.x;
            m[5] += r.y;
            (m, p.user_bbox)
        }
        None => ([1.0, 0.0, 0.0, 1.0, r.x, r.y], (0.0, 0.0, r.width, r.height)),
    };
    let map = |&(x, y): &(f32, f32)| {
        let (x, y) = if def.object_bbox_units { (bbox.0 + x * bbox.2, bbox.1 + y * bbox.3) } else { (x, y) };
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    };
    let mut rings: Vec<(Vec<(f32, f32)>, bool)> = def
        .subpaths
        .iter()
        .filter(|s| s.pts.len() >= 3)
        .map(|s| (s.pts.iter().map(map).collect(), s.even_odd))
        .collect();
    match rings.len() {
        0 => None,
        1 => {
            let (verts, even_odd) = rings.remove(0);
            Some(ResolvedClipShape::Polygon { verts, even_odd })
        }
        _ => {
            let mut verts: Vec<(f32, f32)> = Vec::new();
            for (mut ring, _) in rings {
                if signed_area(&ring) < 0.0 {
                    ring.reverse();
                }
                let first = ring[0];
                if let Some(&anchor) = verts.first() {
                    // Возврат к началу предыдущего контура и мостик к новому.
                    verts.push(anchor);
                }
                verts.extend(ring);
                verts.push(first);
            }
            Some(ResolvedClipShape::Polygon { verts, even_odd: false })
        }
    }
}

/// Удвоенная знаковая площадь контура (формула шнурков).
fn signed_area(ring: &[(f32, f32)]) -> f32 {
    let mut a = 0.0;
    for (i, &(x0, y0)) in ring.iter().enumerate() {
        let (x1, y1) = ring[(i + 1) % ring.len()];
        a += x0 * y1 - x1 * y0;
    }
    a
}

use super::*;

/// CSS Positioning L3 §6.3 — computes the effective `dy` for a sticky-positioned
/// element given its normal-flow Y position (`flow_rect.y`), `scroll_y`, and
/// sticky insets. The element sticks when scrolling would push it past `top` or
/// before the `bottom` limit from `bound`'s bottom edge — `bound` is the
/// nearest scrolling ancestor's on-screen scrollport (or the full viewport when
/// the sticky element isn't nested in one), already mapped back into this
/// layer's pre-transform page-space by [`sticky_bound`].
///
/// Returns the `dy` to apply instead of `-scroll_y` for this layer's content.
pub(crate) fn sticky_offset_dy(
    flow_rect: &lumen_core::geom::Rect,
    top: Option<f32>,
    bottom: Option<f32>,
    scroll_y: f32,
    bound: Rect,
) -> f32 {
    let mut dy = -scroll_y;
    // top: clamp screen_y to be at least `top` px from the scrollport's top edge.
    if let Some(t) = top {
        let screen_y = flow_rect.y + dy;
        let min_y = bound.y + t;
        if screen_y < min_y {
            dy += min_y - screen_y;
        }
    }
    // bottom: clamp so the element's bottom edge is at most `bottom` px from
    // the scrollport's bottom edge.
    if let Some(b) = bottom {
        let max_screen_y = bound.y + bound.height - b - flow_rect.height;
        let actual_screen_y = flow_rect.y + dy;
        if actual_screen_y > max_screen_y {
            dy -= actual_screen_y - max_screen_y;
        }
    }
    dy
}

/// CSS Positioning L3 §6.3 — same as `sticky_offset_dy` but for the X axis.
pub(crate) fn sticky_offset_dx(
    flow_rect: &lumen_core::geom::Rect,
    left: Option<f32>,
    right: Option<f32>,
    scroll_x: f32,
    bound: Rect,
) -> f32 {
    let mut dx = -scroll_x;
    if let Some(l) = left {
        let screen_x = flow_rect.x + dx;
        let min_x = bound.x + l;
        if screen_x < min_x {
            dx += min_x - screen_x;
        }
    }
    if let Some(r) = right {
        let max_screen_x = bound.x + bound.width - r - flow_rect.width;
        let actual_screen_x = flow_rect.x + dx;
        if actual_screen_x > max_screen_x {
            dx -= actual_screen_x - max_screen_x;
        }
    }
    dx
}

/// CSS Positioning L3 §6.3 — the scrollport a `position:sticky` element is
/// clamped against: the innermost active clip (nearest ancestor
/// `overflow:auto|hidden|scroll` container, whether or not it's the one
/// currently scrolling) if any, else the full viewport.
///
/// `clip_stack`/`transform_stack` entries are screen-space (post all ambient
/// transforms), same convention as `PushClipRect`/`PushScrollLayer` clip
/// intersection. `sticky_offset_dy`/`dx`, like every other draw command,
/// receive their `dx`/`dy` pre-transform (applied via `translate_rect` before
/// `transform_stack.last()` runs) — so the bound must be mapped back into that
/// same pre-transform page-space via the *inverse* of the ambient transform.
/// Falls back to returning the screen-space bound unchanged when there's no
/// ambient transform, or it isn't (invertibly) affine — same conservative
/// policy as `apply_transform_to_clip` (BUG-140).
pub(crate) fn sticky_bound(
    clip_stack: &[Rect],
    transform_stack: &[Mat4],
    viewport_w: f32,
    viewport_h: f32,
) -> Rect {
    let screen_bound = clip_stack
        .last()
        .copied()
        .unwrap_or_else(|| Rect::new(0.0, 0.0, viewport_w, viewport_h));
    match transform_stack
        .last()
        .filter(|m| m.is_2d_affine())
        .and_then(|m| m.invert_2d_affine())
    {
        Some(inv) => apply_transform_to_clip(screen_bound, Some(&inv)),
        None => screen_bound,
    }
}

/// Сдвиг rect-а по Y (CSS px). Используется в `render` для применения
/// scroll-offset-а к page-полосе display list-а; overlay-полоса получает
/// `dy = 0`. Без mutation — Rect: Copy.
pub(crate) fn translate_rect(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(rect.x + dx, rect.y + dy, rect.width, rect.height)
}

/// Применяет аккумулированный 2D-аффинный трансформ к clip-rect-у и
/// возвращает AABB трансформированных углов в screen-координатах.
///
/// Нужно для `PushClipRect*`: рект из display-list-а — в page-пространстве,
/// а clip_stack должен хранить координаты в screen-пространстве (с учётом
/// PushTransform-ов, в т.ч. shell-овского сдвига страницы под tab bar).
/// При не-аффинном или отсутствующем трансформе — возвращает rect без
/// изменений (conservative, BUG-140 policy).
pub(crate) fn apply_transform_to_clip(rect: Rect, m: Option<&Mat4>) -> Rect {
    let Some(m) = m.filter(|m| m.is_2d_affine()) else {
        return rect;
    };
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    let corners = [
        m.transform_point_2d(x0, y0),
        m.transform_point_2d(x1, y0),
        m.transform_point_2d(x0, y1),
        m.transform_point_2d(x1, y1),
    ];
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (sx, sy) in corners {
        min_x = min_x.min(sx);
        min_y = min_y.min(sy);
        max_x = max_x.max(sx);
        max_y = max_y.max(sy);
    }
    Rect::new(min_x, min_y, (max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
}

/// ADR-016 M0.2: extra margin (CSS px) around the viewport before culling in
/// the wgpu renderer. Mirrors the femtovg backend's `CULL_SLOP_CSS_PX` —
/// absorbs anti-alias fringe / rounding and keeps a small off-screen band
/// live so a fast scroll step never exposes an un-drawn edge.
const WGPU_CULL_SLOP_CSS_PX: f32 = 256.0;

/// ADR-016 M0.2 viewport culling. Returns `true` when `screen_rect` (a leaf
/// command's box from [`DisplayCommand::cull_rect`], already shifted by the
/// scroll / sticky offset), after the current accumulated transform `m`,
/// lands fully outside the viewport (`vw`×`vh` CSS px) expanded by
/// [`WGPU_CULL_SLOP_CSS_PX`]. Because it tests the AABB of the four
/// transformed corners, the result is a conservative superset under
/// rotation/scale — a command is only culled when its entire footprint is
/// off-screen. A missing or non-affine (3D/perspective) transform disables
/// culling (`false`), so no visible pixel is ever dropped.
/// Верхняя граница отсева (`cull_y0`) обычно 0, но пасс кромки кольцевой
/// полосы (BUG-405 срез 32) видит только свои строки: команда выше кромки для
/// него так же невидима, как команда ниже цели. Без этого кромка платила бы
/// полный обход списка — а срез 29 мерил цену доли ИМЕННО механизмом отсева.
pub(crate) fn leaf_is_offscreen(
    screen_rect: Rect,
    m: Option<&Mat4>,
    vw: f32,
    cull_y0: f32,
    vh: f32,
) -> bool {
    if screen_rect.width <= 0.0 || screen_rect.height <= 0.0 {
        return false;
    }
    let (x0, y0) = (screen_rect.x, screen_rect.y);
    let (x1, y1) = (screen_rect.x + screen_rect.width, screen_rect.y + screen_rect.height);
    let corners = match m {
        None => [(x0, y0), (x1, y0), (x0, y1), (x1, y1)],
        Some(mat) if mat.is_2d_affine() => [
            mat.transform_point_2d(x0, y0),
            mat.transform_point_2d(x1, y0),
            mat.transform_point_2d(x0, y1),
            mat.transform_point_2d(x1, y1),
        ],
        // 3D / perspective transform in effect — do not cull (conservative).
        Some(_) => return false,
    };
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for (sx, sy) in corners {
        min_x = min_x.min(sx);
        min_y = min_y.min(sy);
        max_x = max_x.max(sx);
        max_y = max_y.max(sy);
    }
    let slop = WGPU_CULL_SLOP_CSS_PX;
    max_x < -slop || max_y < cull_y0 - slop || min_x > vw + slop || min_y > vh + slop
}

/// Применяет 2D-аффинную матрицу к `pos` вершинам в диапазоне `verts`.
/// CSS Transforms L1 §13 forward-применение: каждая вершина (x,y) переходит
/// в (a·x+c·y+e, b·x+d·y+f), где a..f — 6 компонент 2D affine части Mat4.
/// Z/W колонки игнорируются (Phase 0 — только 2D трансформы).
///
/// Каждый из FillVertex / TextVertex / ImageVertex имеет одинаковый layout
/// в начале (`pos: [f32; 2]`); функция параметризована типом V и читает
/// только `pos`-смещение через trait `VertexPos`.
pub(crate) trait VertexPos {
    fn pos_mut(&mut self) -> &mut [f32; 2];
    /// Set CSS depth in pixels (positive = closer to viewer). Default no-op for vertex
    /// types without a depth field; FillVertex overrides to enable GPU depth testing.
    fn set_depth(&mut self, _z: f32) {}
}

impl VertexPos for FillVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

impl VertexPos for TextVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

impl VertexPos for ImageVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

impl VertexPos for CircleVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
}

impl VertexPos for GradVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
}

impl VertexPos for MaskVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
}

impl VertexPos for CrossFadeVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
}

impl VertexPos for RRectVertex {
    fn pos_mut(&mut self) -> &mut [f32; 2] { &mut self.pos }
    fn set_depth(&mut self, z: f32) { self.z = z; }
}

pub(crate) fn apply_affine_to_grad_verts(verts: &mut [GradVertex], m: &Mat4) {
    apply_affine_to_verts(verts, m);
}

/// BUG-247 / BUG-277 срезы 12–13: заменяет треугольный суп, уже уложенный в
/// `fill_vertices[v_start..]`, на pixel-aligned квады с точным покрытием —
/// сглаженная кромка вместо бинарной.
///
/// Вызывается для фигур SVG (`DrawSvgFill`/`DrawSvgStroke`, срез 12) и для
/// повёрнутого/скошенного `FillRect` (срез 13) — оба кладут в общий
/// `fill_pipeline` суп, чьи кромки не совпадают с растровой сеткой, а
/// `sample_count: 1` на всех пассах делает такой пиксель либо полностью
/// залитым, либо чистым фоном.
///
/// Растеризация обязана идти в **device px**: суп приходит в CSS px (матрица
/// `PushTransform` уже применена), поэтому здесь он умножается на `dpr`,
/// считается покрытие относительно настоящей растровой сетки, и результат
/// делится обратно — вершинный шейдер сам отобразит CSS px в NDC через
/// viewport-uniform. `color` — цвет фигуры со straight-alpha (пайплайн
/// заливки блендит `ALPHA_BLENDING`), поэтому покрытие домножается в альфу.
///
/// Ничего не делает при вырожденном `dpr`; выключается флагами вызывающей
/// стороны (`LUMEN_NO_SVG_AA` / `LUMEN_NO_ROT_AA`).
/// Сколько вершин (суп + покрытие) [`CoverageCache`] держит, прежде чем
/// сбросить себя целиком. ≈16 МБ при полном заполнении; страница со статичными
/// иконками занимает единицы килобайт, а сброс нужен только патологии — потоку
/// НОВЫХ супов каждый кадр (анимированный SVG под трансформом), где кэш всё
/// равно не попадает и держать его нечем.
pub(crate) const COVERAGE_CACHE_MAX_VERTS: usize = 1 << 20;


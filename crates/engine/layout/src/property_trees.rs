//! Property trees (Chromium-style) — структура, на которую compositor
//! фиксирует mutable properties layout-дерева для off-main-thread reuse.
//!
//! Sprint 0 — контракты: 4 параллельных дерева (Transform / Scroll / Effect /
//! Clip), вектор узлов + parent-индексы. Реальное построение из style + layout —
//! в P1 п.2B; commit в compositor — P2 п.1B.
//!
//! Идея: вместо того чтобы compositor шёл по layout-дереву и заново
//! комбинировал свойства, layout публикует **4 отдельных дерева**, каждое
//! отвечает за свой "канал":
//!
//! - **TransformTree** — accumulated transform matrix вдоль chain of ancestors;
//!   compositor применяет ровно одну матрицу на каждый layer.
//! - **ScrollTree** — scrollable areas + their offsets; compositor может
//!   двигать subtree без main-thread thread.
//! - **EffectTree** — opacity / blur / filter / blend-mode / isolation.
//! - **ClipTree** — clip rect-ы для overflow / clip-path / `<iframe>`.
//!
//! Между деревьями нет shared topology — у каждого свой parent-граф,
//! который не обязан совпадать с layout-родителями. На этапе Phase 0 мы
//! строим parent-граф каждого дерева отдельно: parent узла — *ближайший
//! ancestor*, который сам внёс узел в это дерево (если такого нет — root).
//! Это совпадает с тем, что Blink называет «property tree fragmentation»
//! на простом сайте: position:fixed-узел всё ещё связан с предком в
//! TransformTree (через identity-цепочку), но в ScrollTree фактически
//! сидит под root, потому что ни один ancestor не сделал scroll node.
//!
//! Phase 0: ни одно из этих деревьев пока не подгружается в compositor —
//! commit реализует P2 в п.1B (compositor-scaffolding).

use lumen_core::geom::Rect;

use crate::box_tree::{BoxKind, LayoutBox};
use crate::style::{ComputedStyle, FilterFn, Isolation, MixBlendMode, Overflow, TransformFn};

/// Идентификатор узла в любом из четырёх деревьев. Уникален в пределах своего
/// дерева (в TransformTree свой набор id, в ScrollTree свой, и т.д.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PropertyTreeNodeId(pub u32);

impl PropertyTreeNodeId {
    /// Корневой узел любого дерева (identity-преобразование).
    pub const ROOT: Self = Self(0);

    pub fn raw(self) -> u32 {
        self.0
    }
}

/// 4×4 матрица в column-major порядке (как принято в OpenGL / WebGPU).
/// Для Sprint 0 хранится как 16 `f32`-х; на этапе compositor offload P2
/// положит её в GPU buffer напрямую.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [f32; 16]);

impl Mat4 {
    /// Identity-матрица.
    pub const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ]);

    pub fn is_identity(&self) -> bool {
        self.0 == Self::IDENTITY.0
    }

    /// 2D translation. Z и W колонки остаются identity.
    pub fn translation_2d(tx: f32, ty: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.0[12] = tx;
        m.0[13] = ty;
        m
    }

    /// 2D scale. CSS Transforms L1 §13.4.
    pub fn scale_2d(sx: f32, sy: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.0[0] = sx;
        m.0[5] = sy;
        m
    }

    /// 2D rotation вокруг Z (положительный угол — против часовой стрелки в
    /// математической системе координат, что соответствует CSS-кoнвенции
    /// «по часовой» при Y-вниз). `theta` в радианах.
    pub fn rotate_2d(theta: f32) -> Self {
        let c = theta.cos();
        let s = theta.sin();
        Self([
            c, s, 0.0, 0.0, //
            -s, c, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// `skewX(angle)` — сдвигает X пропорционально Y. CSS Transforms L1 §13.7.
    pub fn skew_x(angle: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.0[4] = angle.tan();
        m
    }

    /// `skewY(angle)` — сдвигает Y пропорционально X.
    pub fn skew_y(angle: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.0[1] = angle.tan();
        m
    }

    /// 2D affine `matrix(a, b, c, d, e, f)` (CSS Transforms L1 §13.10) →
    /// 4×4 column-major. Семантика: x' = a·x + c·y + e, y' = b·x + d·y + f.
    pub fn from_2d_affine(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Self {
        Self([
            a, b, 0.0, 0.0, //
            c, d, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            e, f, 0.0, 1.0,
        ])
    }

    /// Композиция матриц: `lhs * rhs`. Для column-major OpenGL-конвенции
    /// `(lhs * rhs)[col, row] = Σ_k lhs[k, row] · rhs[col, k]`. Применение
    /// результата к точке: `M·p` сначала применяет rhs, затем lhs — это
    /// нужно для CSS-цепочки `transform: A B C` (= A после B после C).
    pub fn multiply(&self, rhs: &Self) -> Self {
        let mut out = [0.0f32; 16];
        for col in 0..4 {
            for row in 0..4 {
                let mut acc = 0.0;
                for k in 0..4 {
                    // lhs[k, row] = lhs.0[k * 4 + row]
                    // rhs[col, k] = rhs.0[col * 4 + k]
                    acc += self.0[k * 4 + row] * rhs.0[col * 4 + k];
                }
                out[col * 4 + row] = acc;
            }
        }
        Self(out)
    }

    /// Инверсия 2D affine-матрицы. Возвращает `None`, если матрица
    /// сингулярна (`det == 0`). Используется hit testing-ом для
    /// преобразования viewport-точки в локальные координаты бокса
    /// (forward transform применяется к точкам бокса при рисовании →
    /// обратный — при hit-тесте).
    ///
    /// Phase 0 ограничение: предполагает, что Z/W колонки — identity
    /// (что верно для всех текущих `TransformFn`: translate / rotate /
    /// scale / skew / matrix2d). Полный 4×4 invert понадобится только
    /// при появлении 3D-трансформов.
    pub fn invert_2d_affine(&self) -> Option<Self> {
        let a = self.0[0];
        let b = self.0[1];
        let c = self.0[4];
        let d = self.0[5];
        let e = self.0[12];
        let f = self.0[13];
        let det = a * d - b * c;
        if det.abs() < f32::EPSILON {
            return None;
        }
        let inv_det = 1.0 / det;
        Some(Self::from_2d_affine(
            d * inv_det,
            -b * inv_det,
            -c * inv_det,
            a * inv_det,
            (c * f - d * e) * inv_det,
            (b * e - a * f) * inv_det,
        ))
    }

    /// Применяет 2D affine часть матрицы к точке `(x, y)`. Z/W колонки
    /// игнорируются: считаются identity (см. `invert_2d_affine`).
    /// Возвращает `(x', y')` в той же системе координат, что и входная
    /// точка после применения этой матрицы.
    pub fn transform_point_2d(&self, x: f32, y: f32) -> (f32, f32) {
        let a = self.0[0];
        let b = self.0[1];
        let c = self.0[4];
        let d = self.0[5];
        let e = self.0[12];
        let f = self.0[13];
        (a * x + c * y + e, b * x + d * y + f)
    }
}

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Узел TransformTree. Хранит локальный transform; accumulated transform
/// вычисляется compositor-ом обходом до root.
#[derive(Debug, Clone, Default)]
pub struct TransformNode {
    pub id: PropertyTreeNodeId,
    /// `None` для root; иначе индекс родителя в `TransformTree::nodes`.
    pub parent: Option<PropertyTreeNodeId>,
    /// Локальная матрица (только этот узел, без accumulation).
    pub local: Mat4,
}

/// Дерево transform-преобразований. Корень — identity.
#[derive(Debug, Clone, Default)]
pub struct TransformTree {
    pub nodes: Vec<TransformNode>,
}

impl TransformTree {
    /// Sprint 0 stub: только root с identity.
    pub fn empty() -> Self {
        Self {
            nodes: vec![TransformNode {
                id: PropertyTreeNodeId::ROOT,
                parent: None,
                local: Mat4::IDENTITY,
            }],
        }
    }

    pub fn root(&self) -> &TransformNode {
        &self.nodes[0]
    }
}

/// Узел ScrollTree. Хранит scrollable rect и текущий scroll offset.
#[derive(Debug, Clone, Default)]
pub struct ScrollNode {
    pub id: PropertyTreeNodeId,
    pub parent: Option<PropertyTreeNodeId>,
    /// Размер contents — может быть больше container_size, что делает
    /// его scrollable.
    pub scroll_container: Rect,
    /// Текущее смещение содержимого в пикселях (x — горизонталь, y — вертикаль).
    /// Положительное y = прокручено вниз (стандартная CSS-семантика).
    pub offset_x: f32,
    pub offset_y: f32,
}

#[derive(Debug, Clone, Default)]
pub struct ScrollTree {
    pub nodes: Vec<ScrollNode>,
}

impl ScrollTree {
    pub fn empty() -> Self {
        Self {
            nodes: vec![ScrollNode {
                id: PropertyTreeNodeId::ROOT,
                parent: None,
                scroll_container: Rect::ZERO,
                offset_x: 0.0,
                offset_y: 0.0,
            }],
        }
    }

    pub fn root(&self) -> &ScrollNode {
        &self.nodes[0]
    }
}

/// Узел EffectTree. Хранит opacity / filter / blend-mode — всё, что
/// требует отдельного off-screen pass или дополнительной alpha-операции.
#[derive(Debug, Clone)]
pub struct EffectNode {
    pub id: PropertyTreeNodeId,
    pub parent: Option<PropertyTreeNodeId>,
    /// 0.0..=1.0 (1.0 — полностью непрозрачно).
    pub opacity: f32,
    /// CSS Filter Effects L1. Sprint 0: bool «есть ли filter». Реальный
    /// список — в P1 п.2B (вынесем сюда `Vec<FilterFn>` или построим bridge
    /// с `ComputedStyle::filter`).
    pub has_filter: bool,
    /// CSS Compositing L1 — isolation. `true` если контекст изолирован
    /// (новая backdrop-группа).
    pub isolate: bool,
}

impl Default for EffectNode {
    fn default() -> Self {
        Self {
            id: PropertyTreeNodeId::ROOT,
            parent: None,
            opacity: 1.0,
            has_filter: false,
            isolate: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EffectTree {
    pub nodes: Vec<EffectNode>,
}

impl EffectTree {
    pub fn empty() -> Self {
        Self {
            nodes: vec![EffectNode::default()],
        }
    }

    pub fn root(&self) -> &EffectNode {
        &self.nodes[0]
    }
}

/// Узел ClipTree. Хранит clip rectangle в локальных координатах (т.е.
/// родительского scroll / transform space).
#[derive(Debug, Clone, Default)]
pub struct ClipNode {
    pub id: PropertyTreeNodeId,
    pub parent: Option<PropertyTreeNodeId>,
    /// `None` = no clip (бесконечная область). `Some(rect)` = ограничить
    /// видимую область прямоугольником.
    pub clip: Option<Rect>,
}

#[derive(Debug, Clone, Default)]
pub struct ClipTree {
    pub nodes: Vec<ClipNode>,
}

impl ClipTree {
    pub fn empty() -> Self {
        Self {
            nodes: vec![ClipNode {
                id: PropertyTreeNodeId::ROOT,
                parent: None,
                clip: None,
            }],
        }
    }

    pub fn root(&self) -> &ClipNode {
        &self.nodes[0]
    }
}

/// 4-deep property trees — единая поверхность, которую layout
/// commits в compositor (P2 п.1B).
///
/// Sprint 0 stub: все 4 — `*::empty()` с одним root-узлом.
#[derive(Debug, Clone, Default)]
pub struct PropertyTrees {
    pub transform: TransformTree,
    pub scroll: ScrollTree,
    pub effect: EffectTree,
    pub clip: ClipTree,
}

impl PropertyTrees {
    /// Sprint 0 stub: все 4 дерева — empty roots.
    pub fn empty() -> Self {
        Self {
            transform: TransformTree::empty(),
            scroll: ScrollTree::empty(),
            effect: EffectTree::empty(),
            clip: ClipTree::empty(),
        }
    }

    /// Совместимость с Sprint 0: пустые root-only деревья. Используется
    /// тестами compositor-stub-а у P2 до перехода на реальный `build`.
    pub fn build_stub() -> Self {
        Self::empty()
    }

    /// Построение property trees из layout-дерева (P1 п.2B).
    ///
    /// Алгоритм: pre-order обход `LayoutBox`, для каждого box-а четыре
    /// независимые проверки (по одной на дерево). Если style триггерит
    /// узел — добавляем его, и его id становится текущим parent для
    /// потомков в этом конкретном дереве. Иначе потомки используют тот
    /// же parent, что и сам box.
    ///
    /// Триггеры (одинаковые между Phase 0 и Phase 1; уточняются в Phase 2+):
    /// - **TransformNode** — `style.transform` непустой;
    /// - **ScrollNode** — overflow-x или overflow-y создаёт scroll
    ///   container, т.е. != `Visible` (CSS Overflow L3 §3.2);
    /// - **EffectNode** — `opacity < 1`, или `filter` непустой, или
    ///   `mix-blend-mode != normal`, или `isolation: isolate`;
    /// - **ClipNode** — `clip-path != none`, или overflow-x/y
    ///   ограничивает рисунок (`Hidden`/`Clip`/`Scroll`/`Auto`).
    ///
    /// Анонимные / inline-run boxes пропускаются: их стиль клонирован от
    /// родителя, и каждый InlineRun под `opacity:0.5` иначе порождал бы
    /// фантомный effect-узел. Проверка совпадает с
    /// `box_can_own_stacking_context` в [`crate::stacking`] — обе подсистемы
    /// договорились, что только Block / Image боксы «несут» property tree
    /// узлы.
    pub fn build(root: &LayoutBox) -> Self {
        let mut trees = Self::empty();
        for child in &root.children {
            walk(
                child,
                PropertyTreeNodeId::ROOT,
                PropertyTreeNodeId::ROOT,
                PropertyTreeNodeId::ROOT,
                PropertyTreeNodeId::ROOT,
                &mut trees,
            );
        }
        trees
    }
}

/// Те же правила, что в `stacking::box_can_own_stacking_context`: только
/// «настоящие» layout-боксы (Block / Image), привязанные к DOM-элементу,
/// могут владеть property-tree-узлами. Анонимные InlineRun-ы пропускаются.
fn box_can_own_property_node(b: &LayoutBox) -> bool {
    matches!(b.kind, BoxKind::Block | BoxKind::Image { .. } | BoxKind::FormControl { .. })
}

/// Вычислить локальную transform-матрицу элемента. CSS Transforms L1 §13:
/// итог = T(origin) · M · T(-origin), где `M` — произведение функций
/// `transform` слева направо. `origin` — локальные координаты pivot-а в
/// CSS px относительно начала бокса.
///
/// Используется property tree builder-ом (`walk`), а также paint-стороной
/// для эмиссии `DisplayCommand::PushTransform` и hit-тестом (косвенно через
/// `forward_box_transform`).
pub fn compute_local_transform(fns: &[TransformFn], origin: (f32, f32, f32)) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    for f in fns {
        let step = match *f {
            TransformFn::Translate(x, y) => Mat4::translation_2d(x, y),
            TransformFn::TranslateX(x) => Mat4::translation_2d(x, 0.0),
            TransformFn::TranslateY(y) => Mat4::translation_2d(0.0, y),
            TransformFn::Rotate(theta) => Mat4::rotate_2d(theta),
            TransformFn::Scale(sx, sy) => Mat4::scale_2d(sx, sy),
            TransformFn::ScaleX(sx) => Mat4::scale_2d(sx, 1.0),
            TransformFn::ScaleY(sy) => Mat4::scale_2d(1.0, sy),
            TransformFn::SkewX(a) => Mat4::skew_x(a),
            TransformFn::SkewY(a) => Mat4::skew_y(a),
            TransformFn::Matrix([a, b, c, d, e, f]) => Mat4::from_2d_affine(a, b, c, d, e, f),
        };
        m = m.multiply(&step);
    }
    let (ox, oy, _oz) = origin;
    if ox == 0.0 && oy == 0.0 {
        return m;
    }
    // T(origin) · M · T(-origin) — origin задан в локальных px коробки.
    Mat4::translation_2d(ox, oy)
        .multiply(&m)
        .multiply(&Mat4::translation_2d(-ox, -oy))
}

/// Forward-матрица бокса в viewport-координатах. CSS Transforms L1 §13:
/// pivot задан в локальных px бокса, а трансформация применяется в той же
/// системе координат, в которой лежит `b.rect` (обычно viewport). Поэтому
/// итоговая матрица — `T(pivot_viewport) · M · T(-pivot_viewport)`, где
/// `pivot_viewport = b.rect.{x,y} + transform_origin`.
///
/// Возвращает `None`, если transform-список пуст (бокс не трансформирован —
/// caller должен трактовать как identity без эмиссии Push/Pop).
///
/// Симметрично `hit_test::invert_box_transform`, но возвращает прямую
/// матрицу (forward), а не обратную.
#[must_use]
pub fn forward_box_transform(b: &LayoutBox) -> Option<Mat4> {
    if b.style.transform.is_empty() {
        return None;
    }
    let (ox, oy, _) = b.style.transform_origin;
    let pivot_x = b.rect.x + ox.resolve(b.rect.width);
    let pivot_y = b.rect.y + oy.resolve(b.rect.height);
    let mut m = Mat4::IDENTITY;
    for f in &b.style.transform {
        let step = match *f {
            TransformFn::Translate(x, y) => Mat4::translation_2d(x, y),
            TransformFn::TranslateX(x) => Mat4::translation_2d(x, 0.0),
            TransformFn::TranslateY(y) => Mat4::translation_2d(0.0, y),
            TransformFn::Rotate(theta) => Mat4::rotate_2d(theta),
            TransformFn::Scale(sx, sy) => Mat4::scale_2d(sx, sy),
            TransformFn::ScaleX(sx) => Mat4::scale_2d(sx, 1.0),
            TransformFn::ScaleY(sy) => Mat4::scale_2d(1.0, sy),
            TransformFn::SkewX(a) => Mat4::skew_x(a),
            TransformFn::SkewY(a) => Mat4::skew_y(a),
            TransformFn::Matrix([a, b_, c, d, e, f]) => Mat4::from_2d_affine(a, b_, c, d, e, f),
        };
        m = m.multiply(&step);
    }
    if pivot_x == 0.0 && pivot_y == 0.0 {
        Some(m)
    } else {
        Some(
            Mat4::translation_2d(pivot_x, pivot_y)
                .multiply(&m)
                .multiply(&Mat4::translation_2d(-pivot_x, -pivot_y)),
        )
    }
}

/// Build the forward transform matrix from a list of TransformFn with a pivot point.
///
/// Used by the animation compositor path to convert an animated `Vec<TransformFn>`
/// into a `Mat4` with the same pivot semantics as `forward_box_transform`.
/// Returns `None` for an empty list (no transform needed).
#[must_use]
pub fn transform_fns_to_matrix(fns: &[TransformFn], pivot_x: f32, pivot_y: f32) -> Option<Mat4> {
    if fns.is_empty() {
        return None;
    }
    let mut m = Mat4::IDENTITY;
    for f in fns {
        let step = match *f {
            TransformFn::Translate(x, y) => Mat4::translation_2d(x, y),
            TransformFn::TranslateX(x) => Mat4::translation_2d(x, 0.0),
            TransformFn::TranslateY(y) => Mat4::translation_2d(0.0, y),
            TransformFn::Rotate(theta) => Mat4::rotate_2d(theta),
            TransformFn::Scale(sx, sy) => Mat4::scale_2d(sx, sy),
            TransformFn::ScaleX(sx) => Mat4::scale_2d(sx, 1.0),
            TransformFn::ScaleY(sy) => Mat4::scale_2d(1.0, sy),
            TransformFn::SkewX(a) => Mat4::skew_x(a),
            TransformFn::SkewY(a) => Mat4::skew_y(a),
            TransformFn::Matrix([a, b, c, d, e, f_]) => Mat4::from_2d_affine(a, b, c, d, e, f_),
        };
        m = m.multiply(&step);
    }
    if pivot_x == 0.0 && pivot_y == 0.0 {
        Some(m)
    } else {
        Some(
            Mat4::translation_2d(pivot_x, pivot_y)
                .multiply(&m)
                .multiply(&Mat4::translation_2d(-pivot_x, -pivot_y)),
        )
    }
}

fn overflow_creates_scroll(o: Overflow) -> bool {
    !matches!(o, Overflow::Visible)
}

fn overflow_creates_clip(o: Overflow) -> bool {
    matches!(o, Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto)
}

fn creates_transform(style: &ComputedStyle) -> bool {
    !style.transform.is_empty()
}

fn creates_scroll(style: &ComputedStyle) -> bool {
    overflow_creates_scroll(style.overflow_x) || overflow_creates_scroll(style.overflow_y)
}

fn creates_effect(style: &ComputedStyle) -> bool {
    style.opacity < 1.0
        || !style.filter.is_empty()
        || style.mix_blend_mode != MixBlendMode::Normal
        || style.isolation == Isolation::Isolate
}

fn creates_clip(style: &ComputedStyle) -> bool {
    style.clip_path.is_some()
        || overflow_creates_clip(style.overflow_x)
        || overflow_creates_clip(style.overflow_y)
}

/// Filter-стек считается «активным», если есть хоть одна функция, которая
/// реально меняет пиксели. Сейчас все варианты `FilterFn` это делают,
/// но `opacity(1.0)` / `brightness(1.0)` / `contrast(1.0)` / `saturate(1.0)`
/// технически no-op. Phase 0 — упрощаем: «любой filter = есть эффект».
fn has_active_filter(filters: &[FilterFn]) -> bool {
    !filters.is_empty()
}

fn walk(
    b: &LayoutBox,
    transform_parent: PropertyTreeNodeId,
    scroll_parent: PropertyTreeNodeId,
    effect_parent: PropertyTreeNodeId,
    clip_parent: PropertyTreeNodeId,
    trees: &mut PropertyTrees,
) {
    let style = &b.style;
    let mut t_parent = transform_parent;
    let mut s_parent = scroll_parent;
    let mut e_parent = effect_parent;
    let mut c_parent = clip_parent;

    if box_can_own_property_node(b) {
        if creates_transform(style) {
            let id = PropertyTreeNodeId(trees.transform.nodes.len() as u32);
            let (raw_ox, raw_oy, oz) = style.transform_origin;
            let resolved_origin = (raw_ox.resolve(b.rect.width), raw_oy.resolve(b.rect.height), oz);
            let local = compute_local_transform(&style.transform, resolved_origin);
            trees.transform.nodes.push(TransformNode {
                id,
                parent: Some(transform_parent),
                local,
            });
            t_parent = id;
        }

        if creates_scroll(style) {
            let id = PropertyTreeNodeId(trees.scroll.nodes.len() as u32);
            trees.scroll.nodes.push(ScrollNode {
                id,
                parent: Some(scroll_parent),
                scroll_container: b.rect,
                offset_x: 0.0,
                offset_y: 0.0,
            });
            s_parent = id;
        }

        if creates_effect(style) {
            let id = PropertyTreeNodeId(trees.effect.nodes.len() as u32);
            trees.effect.nodes.push(EffectNode {
                id,
                parent: Some(effect_parent),
                opacity: style.opacity,
                has_filter: has_active_filter(&style.filter),
                isolate: style.isolation == Isolation::Isolate
                    || style.mix_blend_mode != MixBlendMode::Normal,
            });
            e_parent = id;
        }

        if creates_clip(style) {
            let id = PropertyTreeNodeId(trees.clip.nodes.len() as u32);
            trees.clip.nodes.push(ClipNode {
                id,
                parent: Some(clip_parent),
                clip: Some(b.rect),
            });
            c_parent = id;
        }
    }

    for child in &b.children {
        walk(child, t_parent, s_parent, e_parent, c_parent, trees);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::box_tree::layout;
    use lumen_core::geom::Size;

    #[test]
    fn identity_matrix_is_identity() {
        let m = Mat4::IDENTITY;
        assert!(m.is_identity());
        assert_eq!(m.0[0], 1.0);
        assert_eq!(m.0[5], 1.0);
        assert_eq!(m.0[10], 1.0);
        assert_eq!(m.0[15], 1.0);
    }

    #[test]
    fn empty_trees_have_root_only() {
        let trees = PropertyTrees::empty();
        assert_eq!(trees.transform.nodes.len(), 1);
        assert_eq!(trees.scroll.nodes.len(), 1);
        assert_eq!(trees.effect.nodes.len(), 1);
        assert_eq!(trees.clip.nodes.len(), 1);
        assert_eq!(trees.transform.root().id, PropertyTreeNodeId::ROOT);
        assert!(trees.transform.root().local.is_identity());
    }

    #[test]
    fn effect_root_is_fully_opaque() {
        let t = EffectTree::empty();
        assert_eq!(t.root().opacity, 1.0);
        assert!(!t.root().has_filter);
        assert!(!t.root().isolate);
    }

    #[test]
    fn scroll_root_has_zero_offset() {
        let t = ScrollTree::empty();
        assert_eq!(t.root().offset_x, 0.0);
        assert_eq!(t.root().offset_y, 0.0);
    }

    #[test]
    fn clip_root_has_no_clip() {
        let t = ClipTree::empty();
        assert!(t.root().clip.is_none());
    }

    // ----- Mat4 builders -----

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn translation_matrix_sets_last_column() {
        let m = Mat4::translation_2d(7.0, -3.0);
        assert_eq!(m.0[12], 7.0);
        assert_eq!(m.0[13], -3.0);
        // Остальное — identity.
        assert_eq!(m.0[0], 1.0);
        assert_eq!(m.0[5], 1.0);
        assert_eq!(m.0[10], 1.0);
        assert_eq!(m.0[15], 1.0);
    }

    #[test]
    fn scale_matrix_sets_diagonal() {
        let m = Mat4::scale_2d(2.0, 3.0);
        assert_eq!(m.0[0], 2.0);
        assert_eq!(m.0[5], 3.0);
        assert_eq!(m.0[10], 1.0);
        assert_eq!(m.0[15], 1.0);
    }

    #[test]
    fn rotate_zero_is_identity() {
        let m = Mat4::rotate_2d(0.0);
        assert!(m.is_identity());
    }

    #[test]
    fn rotate_quarter_turn_swaps_axes() {
        let m = Mat4::rotate_2d(std::f32::consts::FRAC_PI_2);
        // [cos, sin] на col 0, [-sin, cos] на col 1.
        assert!(approx(m.0[0], 0.0));
        assert!(approx(m.0[1], 1.0));
        assert!(approx(m.0[4], -1.0));
        assert!(approx(m.0[5], 0.0));
    }

    #[test]
    fn matrix_multiply_with_identity_is_noop() {
        let m = Mat4::translation_2d(5.0, 10.0);
        let id = Mat4::IDENTITY;
        let out = m.multiply(&id);
        assert_eq!(out.0, m.0);
        let out2 = id.multiply(&m);
        assert_eq!(out2.0, m.0);
    }

    #[test]
    fn matrix_multiply_compose_left_to_right() {
        // CSS: transform: translate(10, 0) scale(2, 1).
        // Цепочка применяется right-to-left к точке: scale потом translate.
        // Точка (1, 0) → scale(2,1) → (2, 0) → translate(10, 0) → (12, 0).
        let t = Mat4::translation_2d(10.0, 0.0);
        let s = Mat4::scale_2d(2.0, 1.0);
        let m = t.multiply(&s);
        // Transform-вектор [1, 0, 0, 1]: x' = m[0]·1 + m[4]·0 + m[8]·0 + m[12]·1.
        let x_prime = m.0[0] * 1.0 + m.0[12];
        assert!(approx(x_prime, 12.0));
    }

    // ----- transform_point_2d / invert_2d_affine -----

    #[test]
    fn transform_point_2d_identity_is_noop() {
        let (x, y) = Mat4::IDENTITY.transform_point_2d(3.0, 7.0);
        assert!(approx(x, 3.0));
        assert!(approx(y, 7.0));
    }

    #[test]
    fn transform_point_2d_translate_shifts() {
        let m = Mat4::translation_2d(10.0, -5.0);
        let (x, y) = m.transform_point_2d(0.0, 0.0);
        assert!(approx(x, 10.0));
        assert!(approx(y, -5.0));
    }

    #[test]
    fn transform_point_2d_scale_multiplies() {
        let m = Mat4::scale_2d(2.0, 3.0);
        let (x, y) = m.transform_point_2d(4.0, 5.0);
        assert!(approx(x, 8.0));
        assert!(approx(y, 15.0));
    }

    #[test]
    fn invert_identity_is_identity() {
        let inv = Mat4::IDENTITY.invert_2d_affine().unwrap();
        assert!(inv.is_identity());
    }

    #[test]
    fn invert_translate_negates_offsets() {
        let m = Mat4::translation_2d(10.0, -5.0);
        let inv = m.invert_2d_affine().unwrap();
        let (x, y) = inv.transform_point_2d(10.0, -5.0);
        assert!(approx(x, 0.0));
        assert!(approx(y, 0.0));
    }

    #[test]
    fn invert_scale_reciprocates() {
        let m = Mat4::scale_2d(2.0, 4.0);
        let inv = m.invert_2d_affine().unwrap();
        let (x, y) = inv.transform_point_2d(10.0, 20.0);
        assert!(approx(x, 5.0));
        assert!(approx(y, 5.0));
    }

    #[test]
    fn invert_round_trip_translate_scale() {
        let m = Mat4::translation_2d(50.0, 30.0).multiply(&Mat4::scale_2d(2.0, 3.0));
        let inv = m.invert_2d_affine().unwrap();
        let (vx, vy) = m.transform_point_2d(7.0, 11.0);
        let (px, py) = inv.transform_point_2d(vx, vy);
        assert!(approx(px, 7.0));
        assert!(approx(py, 11.0));
    }

    #[test]
    fn invert_rotation_round_trips() {
        let m = Mat4::rotate_2d(std::f32::consts::FRAC_PI_3);
        let inv = m.invert_2d_affine().unwrap();
        let (vx, vy) = m.transform_point_2d(42.0, -7.5);
        let (px, py) = inv.transform_point_2d(vx, vy);
        assert!(approx(px, 42.0));
        assert!(approx(py, -7.5));
    }

    #[test]
    fn invert_singular_matrix_returns_none() {
        // scale(0, 0) — det == 0, инверсия невозможна.
        let singular = Mat4::scale_2d(0.0, 0.0);
        assert!(singular.invert_2d_affine().is_none());
    }

    // ----- Build PropertyTrees из layout-дерева -----

    fn build(html: &str, css: &str) -> PropertyTrees {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
        PropertyTrees::build(&root)
    }

    #[test]
    fn build_from_empty_document_keeps_only_roots() {
        let trees = build("<p>a</p><p>b</p>", "");
        assert_eq!(trees.transform.nodes.len(), 1);
        assert_eq!(trees.scroll.nodes.len(), 1);
        assert_eq!(trees.effect.nodes.len(), 1);
        assert_eq!(trees.clip.nodes.len(), 1);
    }

    #[test]
    fn transform_property_creates_transform_node() {
        let trees = build("<div>x</div>", "div { transform: translate(10px, 20px); }");
        assert_eq!(trees.transform.nodes.len(), 2);
        let n = &trees.transform.nodes[1];
        assert_eq!(n.parent, Some(PropertyTreeNodeId::ROOT));
        assert!(!n.local.is_identity());
    }

    #[test]
    fn no_transform_keeps_one_transform_node() {
        let trees = build("<div>x</div>", "");
        assert_eq!(trees.transform.nodes.len(), 1);
    }

    #[test]
    fn opacity_lt_one_creates_effect_node() {
        let trees = build("<div>x</div>", "div { opacity: 0.5; }");
        assert_eq!(trees.effect.nodes.len(), 2);
        let n = &trees.effect.nodes[1];
        assert!(approx(n.opacity, 0.5));
        assert!(!n.has_filter);
        assert!(!n.isolate);
    }

    #[test]
    fn opacity_one_does_not_create_effect_node() {
        let trees = build("<div>x</div>", "div { opacity: 1; }");
        assert_eq!(trees.effect.nodes.len(), 1);
    }

    #[test]
    fn filter_creates_effect_node_with_has_filter_set() {
        let trees = build("<div>x</div>", "div { filter: blur(2px); }");
        assert_eq!(trees.effect.nodes.len(), 2);
        assert!(trees.effect.nodes[1].has_filter);
    }

    #[test]
    fn isolation_isolate_creates_isolated_effect_node() {
        let trees = build("<div>x</div>", "div { isolation: isolate; }");
        assert_eq!(trees.effect.nodes.len(), 2);
        assert!(trees.effect.nodes[1].isolate);
    }

    #[test]
    fn mix_blend_mode_marks_effect_isolated() {
        let trees = build("<div>x</div>", "div { mix-blend-mode: multiply; }");
        assert_eq!(trees.effect.nodes.len(), 2);
        assert!(trees.effect.nodes[1].isolate);
    }

    #[test]
    fn overflow_hidden_creates_scroll_and_clip_nodes() {
        let trees = build("<div>x</div>", "div { overflow: hidden; width: 100px; height: 50px; }");
        assert_eq!(trees.scroll.nodes.len(), 2);
        assert_eq!(trees.clip.nodes.len(), 2);
        let scroll = &trees.scroll.nodes[1];
        assert_eq!(scroll.offset_x, 0.0);
        assert!(scroll.scroll_container.size().width > 0.0);
        let clip = &trees.clip.nodes[1];
        assert!(clip.clip.is_some());
    }

    #[test]
    fn overflow_visible_creates_no_scroll_or_clip() {
        let trees = build("<div>x</div>", "div { overflow: visible; }");
        assert_eq!(trees.scroll.nodes.len(), 1);
        assert_eq!(trees.clip.nodes.len(), 1);
    }

    #[test]
    fn clip_path_alone_creates_clip_node_without_scroll() {
        let trees = build(
            "<div>x</div>",
            "div { clip-path: circle(50px at 50px 50px); }",
        );
        assert_eq!(trees.clip.nodes.len(), 2);
        assert_eq!(trees.scroll.nodes.len(), 1);
    }

    #[test]
    fn nested_transforms_form_chain() {
        // .outer transform → .inner transform: parent inner = outer-узел.
        let trees = build(
            r#"<div class="outer"><div class="inner">x</div></div>"#,
            ".outer { transform: scale(2); } .inner { transform: rotate(45deg); }",
        );
        assert_eq!(trees.transform.nodes.len(), 3);
        let outer_id = trees.transform.nodes[1].id;
        assert_eq!(trees.transform.nodes[1].parent, Some(PropertyTreeNodeId::ROOT));
        assert_eq!(trees.transform.nodes[2].parent, Some(outer_id));
    }

    #[test]
    fn descendant_skips_parent_node_when_no_intermediate() {
        // .outer effect (opacity), .skip пустой стиль, .inner effect (filter).
        // inner.effect.parent = outer.effect, не root.
        let trees = build(
            r#"<div class="outer"><div class="skip"><div class="inner">x</div></div></div>"#,
            ".outer { opacity: 0.5; } .inner { filter: blur(1px); }",
        );
        assert_eq!(trees.effect.nodes.len(), 3);
        let outer_id = trees.effect.nodes[1].id;
        assert_eq!(trees.effect.nodes[2].parent, Some(outer_id));
    }

    #[test]
    fn unrelated_trees_do_not_share_parent_chain() {
        // .a — только transform; .b (потомок .a) — только opacity.
        // В TransformTree: root → A. В EffectTree: root → B (parent root,
        // потому что A не вкладывал effect-узел).
        let trees = build(
            r#"<div class="a"><div class="b">x</div></div>"#,
            ".a { transform: scale(2); } .b { opacity: 0.5; }",
        );
        assert_eq!(trees.transform.nodes.len(), 2);
        assert_eq!(trees.effect.nodes.len(), 2);
        assert_eq!(trees.transform.nodes[1].parent, Some(PropertyTreeNodeId::ROOT));
        assert_eq!(trees.effect.nodes[1].parent, Some(PropertyTreeNodeId::ROOT));
    }

    #[test]
    fn anonymous_inline_run_does_not_emit_property_node() {
        // <p>текст</p> — параграф = Block, без триггеров. Inline-run внутри
        // — анонимный, не должен порождать узлы (даже если бы как-то
        // унаследовал opacity).
        let trees = build("<p>just text</p>", "p { opacity: 0.5; }");
        // Один effect-узел от <p>, не больше.
        assert_eq!(trees.effect.nodes.len(), 2);
    }

    #[test]
    fn build_walks_whole_subtree() {
        // Пять div-ов на одном уровне — каждый со своим триггером, должны
        // дать пять transform-узлов плюс root.
        let html = "<div></div>".repeat(5);
        let css = "div { transform: scale(1.5); }";
        let trees = build(&html, css);
        assert_eq!(trees.transform.nodes.len(), 6);
    }
}

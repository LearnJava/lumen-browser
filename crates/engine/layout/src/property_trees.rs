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
//! который не обязан совпадать с layout-родителями (например, `position: fixed`
//! отсоединён от scroll-родителя, но связан с effect-родителем).
//!
//! Phase 0: ни одно из этих деревьев не сейлас не подгружается в compositor —
//! заполнение происходит в P1 п.2B, потребление в P2 п.1B.

use lumen_core::geom::Rect;

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

    /// Sprint 0 stub. Реальное построение из layout-дерева + style — P1 п.2B.
    pub fn build_stub() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

//! Состояния `scroll-state()` контейнера по раскладке (CSS Conditional 5
//! §«scroll-state container queries»).
//!
//! Считается один раз за проход контейнерных запросов
//! (`apply_container_styles`): `scrollable` — из геометрии и текущего `scroll_x`/
//! `scroll_y` бокса, логические стороны — из `writing-mode`/`direction`.
//! `scrolled`, `stuck`, `snapped` пока всегда «нет» (нужны направление
//! пользовательской прокрутки, липкий сдвиг и снап-позиция на момент прохода).
//!
//! Свежее дерево раскладки начинается с `scroll_x == scroll_y == 0`, а прежние
//! смещения возвращает `restore_scroll_positions` уже после прохода запросов.
//! Чтобы запрос видел текущую прокрутку, вызывающий ставит [`install_scroll_hints`]
//! на время раскладки: смещения из подсказок заменяют `scroll_x/y` бокса.

use crate::box_tree::LayoutBox;
use crate::style::{Direction, Edge, EdgeFlags, Overflow, ScrollStateInfo, WritingMode};
use crate::{padding_box, scrollable_extent_x, scrollable_extent_y};
use lumen_core::geom::Size;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Текущая прокрутка для запросов `scroll-state()` на время раскладки.
#[derive(Debug, Clone, Default)]
pub struct ScrollHints {
    /// `NodeId::raw()` → `[scroll_x, scroll_y, ..]` прокручиваемых контейнеров.
    pub offsets: HashMap<u32, [f32; 4]>,
    /// Прокрутка страницы (вьюпорта): её контейнер — корневой элемент.
    pub page: (f32, f32),
    /// `NodeId::raw()` → стороны, к которым контейнер ещё можно прокрутить —
    /// по ним `::scroll-button():enabled`/`:disabled` отвечает на свежей
    /// раскладке (CSS Overflow L5 §3.2). Считается по ПРЕЖНЕМУ дереву до
    /// пересборки: геометрии нового ещё нет.
    pub edges: HashMap<u32, EdgeFlags>,
}

thread_local! {
    /// Подсказки текущей раскладки этого потока (см. [`install_scroll_hints`]).
    static HINTS: RefCell<Option<Rc<ScrollHints>>> = const { RefCell::new(None) };
}

/// Снимает подсказки прокрутки при выходе из области видимости.
#[must_use = "подсказки действуют, пока жив возвращённый охранник"]
pub struct ScrollHintsGuard(Option<Rc<ScrollHints>>);

impl Drop for ScrollHintsGuard {
    fn drop(&mut self) {
        HINTS.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// Текущая прокрутка для `@container scroll-state(…)` во время раскладки этого потока.
/// Без подсказок контейнер считается непрокрученным (свежее дерево). Прежние подсказки
/// возвращаются при сбросе охранника.
pub fn install_scroll_hints(hints: ScrollHints) -> ScrollHintsGuard {
    let prev = HINTS.with(|h| h.borrow_mut().replace(Rc::new(hints)));
    ScrollHintsGuard(prev)
}

/// `(NodeId::raw(), физическое направление)` → можно ли ещё прокрутить
/// контейнер в эту сторону — таблица [`BUTTON_ENABLED`].
type ButtonEnabledMap = HashMap<(u32, u8), bool>;

thread_local! {
    /// Активность `::scroll-button()` на время сборки псевдо-боксов одного
    /// ребёнка. Читается матчером (`style::pseudo`) для
    /// `:enabled`/`:disabled`; без записи кнопка считается активной.
    static BUTTON_ENABLED: RefCell<Option<Rc<ButtonEnabledMap>>> = const { RefCell::new(None) };
}

/// Снимает таблицу активности кнопок при выходе из области видимости.
#[must_use = "таблица действует, пока жив возвращённый охранник"]
pub struct ButtonStatesGuard(Option<Rc<ButtonEnabledMap>>);

impl Drop for ButtonStatesGuard {
    fn drop(&mut self) {
        BUTTON_ENABLED.with(|b| *b.borrow_mut() = self.0.take());
    }
}

/// Коды физических направлений `::scroll-button()` — порядок общий для
/// [`install_scroll_button_states`] и [`scroll_button_enabled`].
pub(crate) const DIR_UP: u8 = 0;
pub(crate) const DIR_DOWN: u8 = 1;
pub(crate) const DIR_LEFT: u8 = 2;
pub(crate) const DIR_RIGHT: u8 = 3;

/// Код физического направления кнопки (`"up"`/`"down"`/`"left"`/`"right"`).
pub(crate) fn scroll_dir_code(dir: &str) -> Option<u8> {
    Some(match dir {
        "up" => DIR_UP,
        "down" => DIR_DOWN,
        "left" => DIR_LEFT,
        "right" => DIR_RIGHT,
        _ => return None,
    })
}

/// Ставит таблицу «можно ли прокрутить в сторону кнопки» на время вычисления
/// стилей её псевдо-боксов. Прежняя таблица возвращается при сбросе охранника.
pub fn install_scroll_button_states(
    states: HashMap<(u32, u8), bool>,
) -> ButtonStatesGuard {
    let prev = BUTTON_ENABLED.with(|b| b.borrow_mut().replace(Rc::new(states)));
    ButtonStatesGuard(prev)
}

/// Активна ли `::scroll-button()` направления `dir` у узла `node` — `None`,
/// когда таблица не установлена или направления в ней нет (матчер тогда
/// считает кнопку активной: без данных о прокрутке `:disabled` не доказан).
pub(crate) fn scroll_button_enabled(node: lumen_dom::NodeId, dir: &str) -> Option<bool> {
    let code = scroll_dir_code(dir)?;
    BUTTON_ENABLED.with(|b| {
        b.borrow().as_ref().and_then(|m| m.get(&(node.raw(), code)).copied())
    })
}

/// Роль бокса относительно вьюпорта (CSS Overflow 3 §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewportRole {
    /// Обычный бокс.
    None,
    /// Корневой элемент: его прокручиваемая область — вьюпорт.
    Root,
    /// `body`, чей `overflow` ушёл во вьюпорт: контейнером прокрутки не остаётся.
    Propagated,
}

/// Таблица стилей спрашивает `scroll-state(…)`: после прокрутки контейнера запросы надо
/// пересчитать, то есть заново пройти раскладку (стили потомков строятся один раз).
pub fn sheet_has_scroll_state_queries(sheet: &lumen_css_parser::Stylesheet) -> bool {
    sheet.container_rules.iter().any(|r| r.preludes().any(|(_, c)| c.to_ascii_lowercase().contains("scroll-state(")))
}

/// Корневой элемент (`html`) — контейнер `scroll-state`: прокрутка страницы меняет ответы.
pub fn root_is_scroll_state_container(root: &LayoutBox) -> bool {
    root.children
        .iter()
        .find(|c| !matches!(c.kind, crate::box_tree::BoxKind::Skip))
        .is_some_and(|html| html.style.container_scroll_state)
}

/// Ненулевые смещения прокрутки дерева (`NodeId::raw()` → `[scroll_x, scroll_y, 0, 0]`) —
/// то, что [`install_scroll_hints`] и `restore_scroll_positions` возвращают в свежее дерево.
pub fn scroll_offsets(root: &LayoutBox) -> HashMap<u32, [f32; 4]> {
    let mut out = HashMap::new();
    let mut stack = vec![root];
    while let Some(b) = stack.pop() {
        if b.scroll_x != 0.0 || b.scroll_y != 0.0 {
            // Первый бокс узла в порядке обхода — главный.
            out.entry(b.node.raw()).or_insert([b.scroll_x, b.scroll_y, 0.0, 0.0]);
        }
        stack.extend(b.children.iter().rev());
    }
    out
}

/// Смещение контейнера: из подсказок, иначе из самого бокса.
fn current_offset(b: &LayoutBox, role: ViewportRole) -> (f32, f32) {
    HINTS
        .with(|h| {
            h.borrow().as_ref().and_then(|h| match role {
                ViewportRole::Root => Some(h.page),
                _ => h.offsets.get(&b.node.raw()).map(|s| (s[0], s[1])),
            })
        })
        .unwrap_or((b.scroll_x, b.scroll_y))
}

/// Допуск сравнения смещений прокрутки (CSS px).
const EPS: f32 = 0.5;

/// Физические стороны `block-start` и `inline-start` (CSS Writing Modes 4 §6.2).
fn logical_starts(mode: WritingMode, dir: Direction) -> (Edge, Edge) {
    let rtl = dir == Direction::Rtl;
    match mode {
        WritingMode::HorizontalTb => (Edge::Top, if rtl { Edge::Right } else { Edge::Left }),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => (Edge::Right, if rtl { Edge::Bottom } else { Edge::Top }),
        WritingMode::VerticalLr => (Edge::Left, if rtl { Edge::Bottom } else { Edge::Top }),
        WritingMode::SidewaysLr => (Edge::Left, if rtl { Edge::Top } else { Edge::Bottom }),
    }
}

/// Стороны, к которым бокс ещё можно прокрутить, по физическим краям
/// `[top, right, bottom, left]`. `user_scrollable` решает, считается ли ось
/// прокручиваемой вообще (для `scroll-state()` — только `scroll`/`auto`,
/// для `::scroll-button()` — ещё и `hidden`: кнопки листают и её,
/// CSS Overflow L5 §3.2, WPT `scroll-buttons-disabled-snapping`).
fn scrollable_edges(
    b: &LayoutBox,
    role: ViewportRole,
    viewport: Size,
    user_scrollable: impl Fn(Overflow) -> bool,
) -> EdgeFlags {
    let s = &b.style;
    let (scroll_x, scroll_y) = current_offset(b, role);
    // Размер области прокрутки: padding box контейнера или вьюпорт.
    let (view_w, view_h) = match role {
        ViewportRole::Root => (viewport.width, viewport.height),
        _ => {
            let pb = padding_box(b);
            (pb.width, pb.height)
        }
    };
    let (mut top, mut right, mut bottom, mut left) = (false, false, false, false);
    if user_scrollable(s.overflow_x) {
        let (min_x, max_x) = scrollable_extent_x(b);
        let x = scroll_x.clamp(min_x, (max_x - view_w).max(min_x));
        left = x > min_x + EPS;
        right = x < max_x - view_w - EPS;
    }
    if user_scrollable(s.overflow_y) {
        let (min_y, max_y) = scrollable_extent_y(b);
        let y = scroll_y.clamp(min_y, (max_y - view_h).max(min_y));
        top = y > min_y + EPS;
        bottom = y < max_y - view_h - EPS;
    }
    EdgeFlags([top, right, bottom, left])
}

/// Состояния `scroll-state()` для бокса с `container-type: scroll-state`.
/// `role` — отношение бокса ко вьюпорту: у корневого элемента прокручивается вьюпорт
/// (смещение страницы, размер вьюпорта), а `body`, чей `overflow` ушёл во вьюпорт
/// (CSS Overflow 3 §3.5), контейнером прокрутки не остаётся.
pub(crate) fn scroll_state_info(b: &LayoutBox, role: ViewportRole, viewport: Size) -> ScrollStateInfo {
    let s = &b.style;
    let (block_start, inline_start) = logical_starts(s.writing_mode, s.direction);
    if role == ViewportRole::Propagated {
        return ScrollStateInfo { block_start, inline_start, ..ScrollStateInfo::default() };
    }
    // Пользователь прокручивает только `scroll`/`auto`; `hidden`/`clip`/`visible` — нет.
    // Вьюпорт прокручивается, пока его `overflow` не `hidden`/`clip`.
    let user_scrollable = |o: Overflow| match role {
        ViewportRole::Root => !matches!(o, Overflow::Hidden | Overflow::Clip),
        _ => matches!(o, Overflow::Scroll | Overflow::Auto),
    };
    ScrollStateInfo {
        scrollable: scrollable_edges(b, role, viewport, user_scrollable),
        block_start,
        inline_start,
        ..ScrollStateInfo::default()
    }
}

/// `overflow`, при котором `::scroll-button()` может сдвинуть бокс: кнопки
/// работают и на `hidden` (программная прокрутка — WPT `scroll-buttons-*`
/// гоняют `scrollLeft`/`scrollTop` именно на нём); `clip` и `visible`
/// прокрутки не имеют вовсе.
fn button_scrollable(o: Overflow) -> bool {
    matches!(o, Overflow::Scroll | Overflow::Auto | Overflow::Hidden)
}

/// Стороны, к которым каждый потенциальный scroll-контейнер ещё можно
/// прокрутить — `NodeId::raw()` → [`EdgeFlags`], для `ScrollHints::edges`.
/// Считается по уже свёрстанному дереву (геометрия готова); зовётся шеллом
/// ДО пересборки, чтобы новая раскладка увидела состояние прокрутки в
/// [`install_scroll_hints`]. Боксы без `overflow`-клиппинга и не-элементы
/// не записываются — у них кнопки всё равно `:disabled`.
pub fn scroll_state_edges(root: &LayoutBox, viewport: Size) -> HashMap<u32, EdgeFlags> {
    fn first_box(b: &LayoutBox) -> Option<&LayoutBox> {
        b.children.iter().find(|c| !matches!(c.kind, crate::box_tree::BoxKind::Skip))
    }
    let mut out = HashMap::new();
    // Роли вьюпорта — как у `viewport_propagating_nodes`: `html` всегда корневой
    // контеймент, `body` — только пока `overflow` корня `visible` (Overflow 3 §3.5).
    let (html_node, body_node) = match first_box(root) {
        Some(html) => {
            let body = if matches!(html.style.overflow_x, Overflow::Visible)
                && matches!(html.style.overflow_y, Overflow::Visible)
            {
                first_box(html).map(|b| b.node)
            } else {
                None
            };
            (html.node, body)
        }
        None => return out,
    };
    let mut stack: Vec<(&LayoutBox, ViewportRole)> = vec![(root, ViewportRole::None)];
    while let Some((b, role)) = stack.pop() {
        let role = if b.node == html_node {
            ViewportRole::Root
        } else if Some(b.node) == body_node {
            ViewportRole::Propagated
        } else {
            role
        };
        let s = &b.style;
        let clips = matches!(s.overflow_x, Overflow::Scroll | Overflow::Auto | Overflow::Hidden | Overflow::Clip)
            || matches!(s.overflow_y, Overflow::Scroll | Overflow::Auto | Overflow::Hidden | Overflow::Clip);
        if b.origin.role == crate::box_tree::BoxRole::Element && (clips || role == ViewportRole::Root) {
            let pred = |o: Overflow| match role {
                ViewportRole::Root => !matches!(o, Overflow::Hidden | Overflow::Clip),
                ViewportRole::Propagated => false,
                ViewportRole::None => button_scrollable(o),
            };
            out.insert(b.node.raw(), scrollable_edges(b, role, viewport, pred));
        }
        stack.extend(b.children.iter().map(|c| (c, ViewportRole::None)));
    }
    out
}

/// Подсказанные [`scroll_state_edges`] для узла на время текущей раскладки —
/// `None`, когда подсказок нет (первая раскладка, движок без keeping-scroll).
pub(crate) fn hinted_scroll_edges(node: lumen_dom::NodeId) -> Option<EdgeFlags> {
    HINTS.with(|h| {
        h.borrow().as_ref().and_then(|h| h.edges.get(&node.raw()).copied())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_starts_follow_writing_mode_and_direction() {
        use Direction::{Ltr, Rtl};
        assert_eq!(logical_starts(WritingMode::HorizontalTb, Ltr), (Edge::Top, Edge::Left));
        assert_eq!(logical_starts(WritingMode::HorizontalTb, Rtl), (Edge::Top, Edge::Right));
        assert_eq!(logical_starts(WritingMode::VerticalRl, Ltr), (Edge::Right, Edge::Top));
        assert_eq!(logical_starts(WritingMode::VerticalLr, Rtl), (Edge::Left, Edge::Bottom));
        assert_eq!(logical_starts(WritingMode::SidewaysLr, Ltr), (Edge::Left, Edge::Bottom));
    }
}

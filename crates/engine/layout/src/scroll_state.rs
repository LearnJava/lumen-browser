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
    let (scroll_x, scroll_y) = current_offset(b, role);
    // Пользователь прокручивает только `scroll`/`auto`; `hidden`/`clip`/`visible` — нет.
    // Вьюпорт прокручивается, пока его `overflow` не `hidden`/`clip`.
    let user_scrollable = |o: Overflow| match role {
        ViewportRole::Root => !matches!(o, Overflow::Hidden | Overflow::Clip),
        _ => matches!(o, Overflow::Scroll | Overflow::Auto),
    };
    // Размер области прокрутки: padding box контейнера или вьюпорт.
    let (view_w, view_h) = match role {
        ViewportRole::Root => (viewport.width, viewport.height),
        _ => {
            let pb = padding_box(b);
            (pb.width, pb.height)
        }
    };
    let (mut top, mut bottom, mut left, mut right) = (false, false, false, false);
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
    ScrollStateInfo {
        scrollable: EdgeFlags([top, right, bottom, left]),
        block_start,
        inline_start,
        ..ScrollStateInfo::default()
    }
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

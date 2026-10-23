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
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    /// `NodeId::raw()` → `[scroll_x, scroll_y, ..]` текущей прокрутки контейнеров, пока
    /// идёт раскладка (см. [`install_scroll_hints`]).
    static HINTS: RefCell<Option<Rc<HashMap<u32, [f32; 4]>>>> = const { RefCell::new(None) };
}

/// Снимает подсказки прокрутки при выходе из области видимости.
#[must_use = "подсказки действуют, пока жив возвращённый охранник"]
pub struct ScrollHintsGuard(Option<Rc<HashMap<u32, [f32; 4]>>>);

impl Drop for ScrollHintsGuard {
    fn drop(&mut self) {
        HINTS.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// Текущие смещения прокрутки контейнеров (`NodeId::raw()` → `[scroll_x, scroll_y, ..]`)
/// для `@container scroll-state(…)` во время раскладки этого потока. Без подсказки
/// контейнер считается непрокрученным (свежее дерево). Прежние подсказки возвращаются
/// при сбросе охранника.
pub fn install_scroll_hints(states: HashMap<u32, [f32; 4]>) -> ScrollHintsGuard {
    let prev = HINTS.with(|h| h.borrow_mut().replace(Rc::new(states)));
    ScrollHintsGuard(prev)
}

/// Таблица стилей спрашивает `scroll-state(…)`: после прокрутки контейнера запросы надо
/// пересчитать, то есть заново пройти раскладку (стили потомков строятся один раз).
pub fn sheet_has_scroll_state_queries(sheet: &lumen_css_parser::Stylesheet) -> bool {
    sheet.container_rules.iter().any(|r| r.preludes().any(|(_, c)| c.to_ascii_lowercase().contains("scroll-state(")))
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
fn current_offset(b: &LayoutBox) -> (f32, f32) {
    HINTS
        .with(|h| h.borrow().as_ref().and_then(|m| m.get(&b.node.raw()).map(|s| (s[0], s[1]))))
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
/// `viewport_scroller` — `overflow` бокса ушёл во вьюпорт (CSS Overflow 3 §3.5: корень,
/// а при `visible` у корня — `body`): сам бокс контейнером прокрутки не остаётся.
pub(crate) fn scroll_state_info(b: &LayoutBox, viewport_scroller: bool) -> ScrollStateInfo {
    let s = &b.style;
    let (block_start, inline_start) = logical_starts(s.writing_mode, s.direction);
    if viewport_scroller {
        return ScrollStateInfo { block_start, inline_start, ..ScrollStateInfo::default() };
    }
    let pb = padding_box(b);
    let (scroll_x, scroll_y) = current_offset(b);
    // Пользователь прокручивает только `scroll`/`auto`; `hidden`/`clip`/`visible` — нет.
    let user_scrollable = |o: Overflow| matches!(o, Overflow::Scroll | Overflow::Auto);
    let (mut top, mut bottom, mut left, mut right) = (false, false, false, false);
    if user_scrollable(s.overflow_x) {
        let (min_x, max_x) = scrollable_extent_x(b);
        let x = scroll_x.clamp(min_x, (max_x - pb.width).max(min_x));
        left = x > min_x + EPS;
        right = x < max_x - pb.width - EPS;
    }
    if user_scrollable(s.overflow_y) {
        let (min_y, max_y) = scrollable_extent_y(b);
        let y = scroll_y.clamp(min_y, (max_y - pb.height).max(min_y));
        top = y > min_y + EPS;
        bottom = y < max_y - pb.height - EPS;
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

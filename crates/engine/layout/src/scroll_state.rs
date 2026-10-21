//! Состояния `scroll-state()` контейнера по раскладке (CSS Conditional 5
//! §«scroll-state container queries»).
//!
//! Считается один раз за проход контейнерных запросов
//! (`apply_container_styles`): `scrollable` — из геометрии и текущего `scroll_x`/
//! `scroll_y` бокса, логические стороны — из `writing-mode`/`direction`.
//! `scrolled`, `stuck`, `snapped` пока всегда «нет» (нужны направление
//! пользовательской прокрутки, липкий сдвиг и снап-позиция на момент прохода).

use crate::box_tree::LayoutBox;
use crate::style::{Direction, Edge, EdgeFlags, Overflow, ScrollStateInfo, WritingMode};
use crate::{padding_box, scrollable_extent_x, scrollable_extent_y};

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
pub(crate) fn scroll_state_info(b: &LayoutBox) -> ScrollStateInfo {
    let s = &b.style;
    let (block_start, inline_start) = logical_starts(s.writing_mode, s.direction);
    let pb = padding_box(b);
    // Пользователь прокручивает только `scroll`/`auto`; `hidden`/`clip`/`visible` — нет.
    let user_scrollable = |o: Overflow| matches!(o, Overflow::Scroll | Overflow::Auto);
    let (mut top, mut bottom, mut left, mut right) = (false, false, false, false);
    if user_scrollable(s.overflow_x) {
        let (min_x, max_x) = scrollable_extent_x(b);
        left = b.scroll_x > min_x + EPS;
        right = b.scroll_x < max_x - pb.width - EPS;
    }
    if user_scrollable(s.overflow_y) {
        let (min_y, max_y) = scrollable_extent_y(b);
        top = b.scroll_y > min_y + EPS;
        bottom = b.scroll_y < max_y - pb.height - EPS;
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

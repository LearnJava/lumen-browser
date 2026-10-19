//! `display: grid-lanes` (CSS Grid Layout L3 §2, бывший `masonry`): тип значений
//! `flow-tolerance`, `grid-lanes-direction`, `grid-lanes-pack` и сводка `GridLanes`,
//! которую `ComputedStyle` хранит одним полем.
//!
//! Сам `display: grid-lanes` — не отдельный вариант `Display`: контейнер остаётся
//! `Display::Grid`/`InlineGrid` (так его обрабатывают все предикаты «это сетка» — blockification,
//! baseline, intrinsic, abspos), а `GridLanes::enabled` переключает раскладку на
//! `box_tree::grid_lanes`. Старый `display: masonry` и `grid-template-*: masonry` остаются
//! невалидными, как в Edge (BUG-105/143, тесты 27/63/75).

use crate::style::values::length::{parse_length_q, Length};

/// `flow-tolerance: normal | <length-percentage> | infinite` (Grid L3 §3.4).
/// Непересчитанное значение: `em`/`%` разрешаются при раскладке.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum FlowTolerance {
    /// `normal` (initial) — `1em`.
    #[default]
    Normal,
    /// Явная длина или процент.
    Length(Length),
    /// `infinite` — допуск не ограничен.
    Infinite,
}

impl FlowTolerance {
    /// Разбор значения свойства `flow-tolerance`; `None` — невалидно.
    pub fn parse(s: &str, is_quirks: bool) -> Option<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("normal") {
            return Some(Self::Normal);
        }
        if s.eq_ignore_ascii_case("infinite") {
            return Some(Self::Infinite);
        }
        let len = parse_length_q(s, is_quirks)?;
        if matches!(&len, Length::Px(v) if *v < 0.0) {
            return None;
        }
        Some(Self::Length(len))
    }
}

/// Ось, вдоль которой идут полосы (lanes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LanesAxis {
    /// `normal` — по шаблону: `grid-template-columns` задан → полосы-столбцы,
    /// иначе `grid-template-rows` задан → полосы-строки, иначе столбцы.
    #[default]
    Normal,
    /// `column` — полосы-столбцы (элементы текут сверху вниз).
    Column,
    /// `row` — полосы-строки (элементы текут слева направо).
    Row,
}

/// `grid-lanes-direction: normal | [ row | column ] [ fill-reverse || track-reverse ]?`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LanesDirection {
    pub axis: LanesAxis,
    /// `fill-reverse` — элементы набираются с конца оси потока.
    pub fill_reverse: bool,
    /// `track-reverse` — порядок полос зеркален.
    pub track_reverse: bool,
}

impl LanesDirection {
    /// Разбор значения `grid-lanes-direction`; `None` — невалидно.
    pub fn parse(s: &str) -> Option<Self> {
        let lower = s.trim().to_ascii_lowercase();
        let mut it = lower.split_whitespace();
        let first = it.next()?;
        if first == "normal" {
            return it.next().is_none().then(Self::default);
        }
        let axis = match first {
            "row" => LanesAxis::Row,
            "column" => LanesAxis::Column,
            _ => return None,
        };
        let mut out = Self { axis, fill_reverse: false, track_reverse: false };
        for tok in it {
            match tok {
                "fill-reverse" if !out.fill_reverse => out.fill_reverse = true,
                "track-reverse" if !out.track_reverse => out.track_reverse = true,
                _ => return None,
            }
        }
        Some(out)
    }
}

/// Сводка свойств `grid-lanes`-контейнера. Все поля не наследуются.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLanes {
    /// `display` — `grid-lanes` / `inline-grid-lanes`.
    pub enabled: bool,
    pub tolerance: FlowTolerance,
    pub direction: LanesDirection,
    /// `grid-lanes-pack: dense`.
    pub dense: bool,
}

/// `grid-lanes-pack: normal | dense`.
pub fn parse_lanes_pack(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(false),
        "dense" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance_keywords_and_lengths() {
        assert_eq!(FlowTolerance::parse("normal", false), Some(FlowTolerance::Normal));
        assert_eq!(FlowTolerance::parse("infinite", false), Some(FlowTolerance::Infinite));
        assert_eq!(FlowTolerance::parse("0", false), Some(FlowTolerance::Length(Length::Px(0.0))));
        assert_eq!(FlowTolerance::parse("51px", false), Some(FlowTolerance::Length(Length::Px(51.0))));
        assert_eq!(FlowTolerance::parse("-1px", false), None);
        assert_eq!(FlowTolerance::parse("auto", false), None);
    }

    #[test]
    fn direction_forms() {
        assert_eq!(LanesDirection::parse("normal"), Some(LanesDirection::default()));
        let d = LanesDirection::parse("row track-reverse fill-reverse").unwrap();
        assert_eq!((d.axis, d.fill_reverse, d.track_reverse), (LanesAxis::Row, true, true));
        assert_eq!(LanesDirection::parse("column fill-reverse").unwrap().axis, LanesAxis::Column);
        assert_eq!(LanesDirection::parse("fill-reverse"), None);
        assert_eq!(LanesDirection::parse("row row"), None);
        assert_eq!(LanesDirection::parse("normal row"), None);
    }

    #[test]
    fn pack_values() {
        assert_eq!(parse_lanes_pack("dense"), Some(true));
        assert_eq!(parse_lanes_pack("normal"), Some(false));
        assert_eq!(parse_lanes_pack("sparse"), None);
    }
}

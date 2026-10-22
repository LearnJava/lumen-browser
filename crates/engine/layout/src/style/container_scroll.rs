//! `@container scroll-state(…)` — CSS Conditional 5 §«scroll-state()».
//!
//! [`ScrollStateInfo`] — состояния контейнера с `container-type: scroll-state`
//! (`scrollable`, `scrolled`, `stuck`, `snapped`), посчитанные по раскладке;
//! [`evaluate_scroll_state_query`] — трёхзначная оценка тела `scroll-state(…)`
//! (`None` = «unknown»: неизвестное имя признака, значение вне грамматики).

use super::container::{eval_tri, Tri};

/// Физическая сторона контейнера; порядок индексов `[top, right, bottom, left]`
/// совпадает с `box_padding`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edge {
    #[default]
    Top,
    Right,
    Bottom,
    Left,
}

impl Edge {
    fn idx(self) -> usize {
        match self {
            Edge::Top => 0,
            Edge::Right => 1,
            Edge::Bottom => 2,
            Edge::Left => 3,
        }
    }
}

/// Флаги по физическим сторонам `[top, right, bottom, left]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EdgeFlags(pub [bool; 4]);

impl EdgeFlags {
    fn get(self, e: Edge) -> bool {
        self.0[e.idx()]
    }
    fn any(self) -> bool {
        self.0.iter().any(|&b| b)
    }
}

/// Состояния прокрутки контейнера для `scroll-state()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollStateInfo {
    /// `scrollable`: контейнер можно прокрутить к этой стороне.
    pub scrollable: EdgeFlags,
    /// `scrolled`: последняя пользовательская прокрутка шла к этой стороне.
    pub scrolled: EdgeFlags,
    /// `stuck`: липкий бокс прижат к этой стороне своего скроллпорта.
    pub stuck: EdgeFlags,
    /// `snapped`: бокс выровнен по снап-позиции по горизонтали / вертикали.
    pub snapped_x: bool,
    pub snapped_y: bool,
    /// Физическая сторона, на которую приходится `block-start`.
    pub block_start: Edge,
    /// Физическая сторона, на которую приходится `inline-start`.
    pub inline_start: Edge,
}

fn opposite(e: Edge) -> Edge {
    match e {
        Edge::Top => Edge::Bottom,
        Edge::Bottom => Edge::Top,
        Edge::Left => Edge::Right,
        Edge::Right => Edge::Left,
    }
}

impl ScrollStateInfo {
    /// Блоковая ось горизонтальна (вертикальные режимы письма).
    fn block_is_horizontal(&self) -> bool {
        matches!(self.block_start, Edge::Left | Edge::Right)
    }

    /// Физические стороны, на которые указывает значение направления; `None` —
    /// значение вне грамматики (`axes` — допустимы ли `x|y|block|inline`).
    fn edges_for(&self, value: &str, axes: bool) -> Option<Vec<Edge>> {
        let block = [self.block_start, opposite(self.block_start)];
        let inline = [self.inline_start, opposite(self.inline_start)];
        Some(match value {
            "top" => vec![Edge::Top],
            "right" => vec![Edge::Right],
            "bottom" => vec![Edge::Bottom],
            "left" => vec![Edge::Left],
            "block-start" => vec![self.block_start],
            "block-end" => vec![block[1]],
            "inline-start" => vec![self.inline_start],
            "inline-end" => vec![inline[1]],
            "x" if axes => vec![Edge::Left, Edge::Right],
            "y" if axes => vec![Edge::Top, Edge::Bottom],
            "block" if axes => block.to_vec(),
            "inline" if axes => inline.to_vec(),
            _ => return None,
        })
    }

    fn directional(&self, flags: EdgeFlags, value: Option<&str>, axes: bool) -> Tri {
        match value {
            None => Some(flags.any()),
            Some("none") => Some(!flags.any()),
            Some(v) => Some(self.edges_for(v, axes)?.into_iter().any(|e| flags.get(e))),
        }
    }

    fn snapped(&self, value: Option<&str>) -> Tri {
        let (block, inline) = if self.block_is_horizontal() {
            (self.snapped_x, self.snapped_y)
        } else {
            (self.snapped_y, self.snapped_x)
        };
        Some(match value {
            None => self.snapped_x || self.snapped_y,
            Some("none") => !self.snapped_x && !self.snapped_y,
            Some("x") => self.snapped_x,
            Some("y") => self.snapped_y,
            Some("block") => block,
            Some("inline") => inline,
            Some("both") => self.snapped_x && self.snapped_y,
            Some(_) => return None,
        })
    }
}

/// Тело `scroll-state(…)`: `<scroll-state-query>` с `not`/`and`/`or` и скобками.
pub(super) fn evaluate_scroll_state_query(body: &str, info: &ScrollStateInfo) -> Tri {
    eval_tri(body, &|feature| evaluate_scroll_state_feature(feature, info))
}

/// Один `<scroll-state-feature>`: `name` или `name: value`.
fn evaluate_scroll_state_feature(feature: &str, info: &ScrollStateInfo) -> Tri {
    let feature = feature.trim();
    let (name, value) = match feature.split_once(':') {
        Some((n, v)) => {
            let v = v.trim();
            if v.is_empty() {
                return None;
            }
            (n.trim(), Some(v.to_ascii_lowercase()))
        }
        None => (feature, None),
    };
    let value = value.as_deref();
    match name.to_ascii_lowercase().as_str() {
        "scrollable" => info.directional(info.scrollable, value, true),
        "scrolled" => info.directional(info.scrolled, value, true),
        "stuck" => info.directional(info.stuck, value, false),
        "snapped" => info.snapped(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ScrollStateInfo {
        ScrollStateInfo {
            scrollable: EdgeFlags([false, true, true, false]),
            block_start: Edge::Top,
            inline_start: Edge::Left,
            ..ScrollStateInfo::default()
        }
    }

    #[test]
    fn scrollable_directions() {
        let i = info();
        let q = |s: &str| evaluate_scroll_state_query(s, &i);
        assert_eq!(q("scrollable"), Some(true));
        assert_eq!(q("scrollable: none"), Some(false));
        assert_eq!(q("scrollable: bottom"), Some(true));
        assert_eq!(q("scrollable: top"), Some(false));
        assert_eq!(q("scrollable: block-end"), Some(true));
        assert_eq!(q("scrollable: inline-start"), Some(false));
        assert_eq!(q("scrollable: x"), Some(true));
        assert_eq!(q("scrollable: inline"), Some(true));
    }

    #[test]
    fn invalid_is_unknown() {
        let i = info();
        let q = |s: &str| evaluate_scroll_state_query(s, &i);
        assert_eq!(q("scrollable: auto"), None);
        assert_eq!(q("scrollable:"), None);
        assert_eq!(q("--foo"), None);
        assert_eq!(q("style(scrollable: left)"), None);
        assert_eq!(q("stuck: x"), None);
        assert_eq!(q("snapped: top"), None);
    }

    #[test]
    fn logical_combinators() {
        let i = info();
        let q = |s: &str| evaluate_scroll_state_query(s, &i);
        assert_eq!(q("not ((scrollable: bottom) and (scrollable: right))"), Some(false));
        assert_eq!(q("(scrollable: left) or (scrollable: top)"), Some(false));
        assert_eq!(q("(scrollable: left) or (scrollable: auto)"), None);
        assert_eq!(q("(scrollable: right) or (scrollable: auto)"), Some(true));
        assert_eq!(q("not (scrollable: auto)"), None);
    }

    #[test]
    fn snapped_axes_follow_writing_mode() {
        let mut i = info();
        i.snapped_x = true;
        assert_eq!(evaluate_scroll_state_query("snapped: inline", &i), Some(true));
        assert_eq!(evaluate_scroll_state_query("snapped: block", &i), Some(false));
        i.block_start = Edge::Right;
        i.inline_start = Edge::Top;
        assert_eq!(evaluate_scroll_state_query("snapped: block", &i), Some(true));
    }
}

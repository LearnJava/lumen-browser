//! `display: grid-lanes` (CSS Grid Layout L3 §2–3): выбор полосы для каждого элемента.
//!
//! Раскладка идёт через обычный проход замера `grid_trampoline` — ширины полос (`col_widths`,
//! `col_offsets`) считает `build_grid_init`, как для сетки. Отличие в строках: каждому элементу
//! достаётся своя строка `k`, а её смещение — позиция элемента в выбранной полосе. Полосу
//! выбирает [`LanesState::choose`] прямо перед замером элемента (она зависит от высот уже
//! уложенных), [`LanesState::settle`] после замера записывает позицию и конец элемента.

use crate::style::{ComputedStyle, LanesAxis};

/// Ось, вдоль которой идут полосы (`grid-lanes-direction: normal` берёт её из шаблона).
pub(super) fn lanes_axis(s: &ComputedStyle) -> LanesAxis {
    match s.grid_lanes.direction.axis {
        LanesAxis::Normal => {
            if !s.grid_template_columns.is_empty() || s.grid_template_col_auto_repeat.is_some() {
                LanesAxis::Column
            } else if !s.grid_template_rows.is_empty() || s.grid_template_row_auto_repeat.is_some() {
                LanesAxis::Row
            } else {
                LanesAxis::Column
            }
        }
        explicit => explicit,
    }
}

/// Сколько полос и где занимает элемент: `def` — определённая первая полоса (0-based).
#[derive(Clone, Copy, Debug)]
pub(super) struct LaneItem {
    pub def: Option<usize>,
    pub span: usize,
}

pub(super) struct LanesState {
    items: Vec<LaneItem>,
    /// Конец последнего элемента в каждой полосе (`None` — полоса пуста).
    ends: Vec<Option<f32>>,
    /// `flow-tolerance` в пикселях (`INFINITY` — `infinite`).
    tolerance: f32,
    /// Промежуток между элементами вдоль потока.
    gap: f32,
    /// `grid-lanes-direction: … track-reverse` — при равенстве выигрывает последняя полоса.
    track_reverse: bool,
    /// Первая полоса и ширина окна выбранного для элемента `k` (записывает `choose`).
    window: Vec<(usize, usize)>,
    /// Позиция начала элемента вдоль потока (от начала content box).
    pub pos: Vec<f32>,
    /// Конец самого длинного потока — высота содержимого.
    pub extent: f32,
}

impl LanesState {
    pub(super) fn new(items: Vec<LaneItem>, n_lanes: usize, tolerance: f32, gap: f32, track_reverse: bool) -> Self {
        let n_items = items.len();
        Self {
            items,
            ends: vec![None; n_lanes.max(1)],
            tolerance,
            gap,
            track_reverse,
            window: vec![(0, 1); n_items],
            pos: vec![0.0; n_items],
            extent: 0.0,
        }
    }

    /// Окно полос `(первая полоса, число полос)` элемента `k`.
    pub(super) fn choose(&mut self, k: usize) -> (usize, usize) {
        let n = self.ends.len();
        let item = self.items[k];
        let span = item.span.clamp(1, n);
        let first = match item.def {
            Some(d) => d.min(n - span),
            None => pick_window(&self.ends, self.gap, span, self.tolerance, self.track_reverse),
        };
        self.window[k] = (first, span);
        (first, span)
    }

    /// Записывает позицию элемента `k` высотой `h` (внешней) и продвигает концы его полос.
    pub(super) fn settle(&mut self, k: usize, h: f32) {
        let (first, span) = self.window[k];
        let start = window_start(&self.ends, self.gap, first, span);
        self.pos[k] = start;
        let end = start + h.max(0.0);
        for lane in &mut self.ends[first..first + span] {
            *lane = Some(end);
        }
        self.extent = self.extent.max(end);
    }
}

/// Позиция, с которой элемент встанет в окно `first..first+span`: после самого длинного потока
/// окна, через промежуток, если полоса не пуста.
fn window_start(ends: &[Option<f32>], gap: f32, first: usize, span: usize) -> f32 {
    ends[first..first + span]
        .iter()
        .map(|e| e.map_or(0.0, |end| end + gap))
        .fold(0.0, f32::max)
}

/// Grid L3 §3.1: окно с наименьшей позицией; окна в пределах `tolerance` от неё равноправны,
/// из них берётся первое (при `track-reverse` — последнее).
fn pick_window(ends: &[Option<f32>], gap: f32, span: usize, tolerance: f32, reverse: bool) -> usize {
    let n = ends.len();
    let starts: Vec<usize> = (0..=n - span).collect();
    let pos = |w: usize| window_start(ends, gap, w, span);
    let min = starts.iter().map(|&w| pos(w)).fold(f32::INFINITY, f32::min);
    let ok = |w: &usize| pos(*w) <= min + tolerance;
    let found = if reverse { starts.iter().rev().copied().find(ok) } else { starts.iter().copied().find(ok) };
    found.unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(span: usize) -> LaneItem {
        LaneItem { def: None, span }
    }

    #[test]
    fn shortest_lane_wins_and_ties_go_first() {
        let mut st = LanesState::new(vec![auto(1); 4], 2, 0.0, 10.0, false);
        // 50 / 30 / 20 / 20 по двум полосам, зазор 10.
        let heights = [50.0, 30.0, 20.0, 20.0];
        let mut lanes = Vec::new();
        for (k, h) in heights.iter().enumerate() {
            lanes.push(st.choose(k).0);
            st.settle(k, *h);
        }
        assert_eq!(lanes, vec![0, 1, 1, 0]);
        assert_eq!(st.pos, vec![0.0, 0.0, 40.0, 60.0]);
        assert_eq!(st.extent, 80.0);
    }

    #[test]
    fn tolerance_keeps_first_lane() {
        let mut st = LanesState::new(vec![auto(1); 3], 2, 100.0, 0.0, false);
        st.choose(0);
        st.settle(0, 50.0);
        // вторая полоса пуста (0), первая на 50, но допуск 100 — снова первая.
        assert_eq!(st.choose(1).0, 0);
    }

    #[test]
    fn spanning_item_starts_after_longest_lane() {
        let mut st = LanesState::new(vec![auto(1), auto(2)], 2, 0.0, 5.0, false);
        st.choose(0);
        st.settle(0, 30.0);
        assert_eq!(st.choose(1), (0, 2));
        st.settle(1, 10.0);
        assert_eq!(st.pos[1], 35.0);
    }

    #[test]
    fn definite_lane_and_track_reverse() {
        let mut st = LanesState::new(vec![LaneItem { def: Some(1), span: 1 }, auto(1)], 3, 0.0, 0.0, true);
        assert_eq!(st.choose(0).0, 1);
        st.settle(0, 10.0);
        // свободны полосы 0 и 2; при track-reverse первой считается последняя.
        assert_eq!(st.choose(1).0, 2);
    }
}

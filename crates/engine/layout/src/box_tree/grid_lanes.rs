//! `display: grid-lanes` (CSS Grid Layout L3 §2–3): выбор полосы для каждого элемента.
//!
//! Раскладка идёт через обычный проход замера `grid_trampoline` — ширины полос (`col_widths`,
//! `col_offsets`) считает `build_grid_init`, как для сетки. Отличие в строках: каждому элементу
//! достаётся своя строка `k`, а её смещение — позиция элемента в выбранной полосе. Полосу
//! выбирает [`LanesState::choose`] прямо перед замером элемента (она зависит от высот уже
//! уложенных), [`LanesState::settle`] после замера записывает позицию и конец элемента.

use super::grid::{resolve_grid_axis, GridAxis};
use super::LayoutBox;
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
    /// Курсор автоматического размещения (Grid L3 §4.4): номер линии (в порядке потока) после
    /// последнего автоматически размещённого элемента; при равенстве берётся первое окно не раньше него.
    cursor: usize,
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
            cursor: 0,
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
            None => {
                let first =
                    pick_window(&self.ends, self.gap, span, self.tolerance, self.track_reverse, self.cursor);
                self.cursor = if self.track_reverse { n - first } else { first + span };
                first
            }
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

/// Полосы элементов `item_idxs` по их `grid-column-*` (`resolve_grid_axis`).
pub(super) fn lane_items(children: &[LayoutBox], item_idxs: &[usize], col_axis: &GridAxis) -> Vec<LaneItem> {
    item_idxs
        .iter()
        .map(|&i| {
            let is = &children[i].style;
            let (cs, ce) = resolve_grid_axis(&is.grid_column_start, &is.grid_column_end, col_axis);
            if cs != 0 {
                LaneItem { def: Some(cs as usize - 1), span: (ce.saturating_sub(cs)).max(1) as usize }
            } else {
                LaneItem { def: None, span: ce.max(1) as usize }
            }
        })
        .collect()
}

/// Grid L3 §3.4: элемент с автоматическим положением вносит вклад в каждую полосу, которую мог бы
/// занять, — не только в ту, куда в итоге попал. Возвращает элементы и их размещения для подсчёта
/// ширин полос: определённый элемент остаётся как есть, автоматический — по копии на каждое
/// начало окна из `n_cols` полос.
pub(super) fn sizing_items(
    item_idxs: &[usize],
    placements: &[(u32, u32, u32, u32)],
    lanes: &[LaneItem],
    n_cols: usize,
) -> (Vec<usize>, Vec<(u32, u32, u32, u32)>) {
    let mut idxs = Vec::with_capacity(item_idxs.len());
    let mut places = Vec::with_capacity(item_idxs.len());
    for ((&i, &p), item) in item_idxs.iter().zip(placements).zip(lanes) {
        if item.def.is_some() {
            idxs.push(i);
            places.push(p);
            continue;
        }
        let span = item.span.clamp(1, n_cols.max(1));
        for first in 0..=n_cols.max(span) - span {
            idxs.push(i);
            places.push((first as u32 + 1, (first + span) as u32 + 1, p.2, p.3));
        }
    }
    (idxs, places)
}

/// Позиция, с которой элемент встанет в окно `first..first+span`: после самого длинного потока
/// окна, через промежуток, если полоса не пуста.
fn window_start(ends: &[Option<f32>], gap: f32, first: usize, span: usize) -> f32 {
    ends[first..first + span]
        .iter()
        .map(|e| e.map_or(0.0, |end| end + gap))
        .fold(0.0, f32::max)
}

/// Grid L3 §4.4: окно с наименьшей позицией; окна в пределах `tolerance` от неё равноправны, из них
/// берётся первое не раньше курсора (иначе первое вообще). `reverse` — порядок полос обратный.
fn pick_window(ends: &[Option<f32>], gap: f32, span: usize, tolerance: f32, reverse: bool, cursor: usize) -> usize {
    let n = ends.len();
    let starts: Vec<usize> = if reverse { (0..=n - span).rev().collect() } else { (0..=n - span).collect() };
    let pos = |w: usize| window_start(ends, gap, w, span);
    let min = starts.iter().map(|&w| pos(w)).fold(f32::INFINITY, f32::min);
    let tied = |w: &usize| pos(*w) <= min + tolerance;
    // Номер линии, с которой окно начинается в порядке потока.
    let line = |w: usize| if reverse { n - (w + span) } else { w };
    let found = starts.iter().copied().find(|w| tied(w) && line(*w) >= cursor);
    found.or_else(|| starts.iter().copied().find(tied)).unwrap_or(0)
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
    fn tolerance_ties_follow_the_cursor() {
        let mut st = LanesState::new(vec![auto(1); 3], 2, 100.0, 0.0, false);
        st.choose(0);
        st.settle(0, 50.0);
        // полосы 0 (50) и 1 (0) в пределах допуска 100: после полосы 0 — полоса 1.
        assert_eq!(st.choose(1).0, 1);
        st.settle(1, 10.0);
        // снова равны, но после полосы 1 окон нет — первое.
        assert_eq!(st.choose(2).0, 0);
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
    fn auto_item_contributes_to_every_window() {
        let lanes = [auto(1), LaneItem { def: Some(2), span: 1 }, auto(2)];
        let places = [(1, 2, 1, 2), (3, 4, 2, 3), (1, 3, 3, 4)];
        let (idxs, p) = sizing_items(&[7, 8, 9], &places, &lanes, 3);
        assert_eq!(idxs, vec![7, 7, 7, 8, 9, 9]);
        let cols: Vec<(u32, u32)> = p.iter().map(|&(a, b, _, _)| (a, b)).collect();
        assert_eq!(cols, vec![(1, 2), (2, 3), (3, 4), (3, 4), (1, 3), (2, 4)]);
    }

    #[test]
    fn tie_goes_after_the_last_placed_item() {
        // Полосы 0 и 1 в равенстве, последний автоэлемент стоит в полосе 0 — следующий в полосу 1.
        let mut st = LanesState::new(vec![auto(1); 4], 3, 0.0, 0.0, false);
        let lanes: Vec<usize> = [30.0, 30.0, 30.0, 10.0]
            .iter()
            .enumerate()
            .map(|(k, h)| {
                let l = st.choose(k).0;
                st.settle(k, *h);
                l
            })
            .collect();
        // 0, 1, 2, затем все три равны (30): курсор после полосы 2 — окон нет, берётся первое.
        assert_eq!(lanes, vec![0, 1, 2, 0]);
        // Теперь полоса 0 выше (40), 1 и 2 равны (30), курсор после полосы 0: берётся 1, не 2.
        assert_eq!(st.choose(0).0, 1);
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

//! `display: grid-lanes` (CSS Grid Layout L3 §2–3): выбор полосы для каждого элемента.
//!
//! Раскладка идёт через обычный проход замера `grid_trampoline` — ширины полос (`col_widths`,
//! `col_offsets`) считает `build_grid_init`, как для сетки. Отличие в строках: каждому элементу
//! достаётся своя строка `k`, а её смещение — позиция элемента в выбранной полосе. Полосу
//! выбирает [`LanesState::choose`] прямо перед замером элемента (она зависит от высот уже
//! уложенных), [`LanesState::settle`] после замера записывает позицию и конец элемента.

use super::grid::{resolve_grid_axis, GridAxis};
use super::{LayoutBox, Size, TextMeasurer};
use crate::style::{AlignValue, ComputedStyle, GridAutoFlow, GridRepeat, GridTrackSize, LanesAxis};
use std::sync::Arc;

/// `grid-lanes-pack: dense` или устаревшее `grid-auto-flow: dense`.
pub(super) fn is_dense(s: &ComputedStyle) -> bool {
    s.grid_lanes.dense || matches!(s.grid_auto_flow, GridAutoFlow::RowDense | GridAutoFlow::ColumnDense)
}

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

/// Стиль для сетки `grid-lanes`: у полос-столбцов шаблон строк игнорируется (строки здесь по одной на
/// элемент, высоту даёт содержимое), у полос-строк — шаблон столбцов (столбец на элемент, ширина
/// и позиция — из потока). `None` — стиль не меняется. Вынесено из `dispatch_box`: копия
/// `ComputedStyle` в его кадре раздувает стек на каждом уровне вложенности.
#[inline(never)]
pub(super) fn lanes_style(s: &Arc<ComputedStyle>) -> Option<Arc<ComputedStyle>> {
    if !s.grid_lanes.enabled {
        return None;
    }
    let mut ls = (**s).clone();
    match lanes_axis(s) {
        LanesAxis::Column => {
            ls.grid_template_rows.clear();
            ls.grid_template_row_auto_repeat = None;
            ls.grid_auto_rows = Vec::new();
        }
        LanesAxis::Row => {
            ls.grid_template_columns.clear();
            ls.grid_template_col_auto_repeat = None;
            ls.grid_auto_columns = Vec::new();
        }
        LanesAxis::Normal => return None,
    }
    // Повтор с дорожками по содержимому допускает рядом только дорожки фиксированного размера:
    // иначе объявление недействительно и ось остаётся без шаблона (`none`).
    let invalid = |rep: &Option<GridRepeat>| {
        rep.as_ref().is_some_and(|r| {
            r.tracks.iter().any(track_is_intrinsic) && r.before.iter().chain(&r.after).any(|t| !is_fixed_size(t))
        })
    };
    if lanes_axis(s) == LanesAxis::Column && invalid(&ls.grid_template_col_auto_repeat) {
        ls.grid_template_columns.clear();
        ls.grid_template_col_auto_repeat = None;
    } else if lanes_axis(s) == LanesAxis::Row && invalid(&ls.grid_template_row_auto_repeat) {
        ls.grid_template_rows.clear();
        ls.grid_template_row_auto_repeat = None;
    }
    Some(Arc::new(ls))
}

/// `<fixed-size>` (Grid L1 §7.2.3): длина или `minmax()` с длиной, без `fr` и содержимого с обеих сторон.
fn is_fixed_size(t: &GridTrackSize) -> bool {
    match t {
        GridTrackSize::Length(_) => true,
        GridTrackSize::Minmax(lo, hi) => {
            (matches!(**lo, GridTrackSize::Length(_)) || matches!(**hi, GridTrackSize::Length(_)))
                && !lo.is_fr()
                && !hi.is_fr()
        }
        _ => false,
    }
}

/// Размещение элемента в сетке: `(начало, конец столбца, начало, конец строки)`, номера линий с 1.
pub(super) type Placement = (u32, u32, u32, u32);

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
    /// `grid-lanes-pack: dense`: занятые отрезки `(начало, конец)` каждой полосы — элемент встаёт в
    /// самую раннюю щель, куда помещается (Grid L3 §4.5). `None` — обычная упаковка по концам полос.
    busy: Option<Vec<Vec<(f32, f32)>>>,
    /// Размеры полос и промежуток между ними: щель годится только окну того же суммарного размера.
    lane_sizes: Vec<f32>,
    lane_gap: f32,
    /// Элемент `k` выровнен по оси потока явно (`stretch`/`center`/`end`): последний в полосе получает
    /// всё место до конца контейнера (Grid L3 §6).
    pub wants: Vec<bool>,
}

impl LanesState {
    pub(super) fn new(items: Vec<LaneItem>, n_lanes: usize, tolerance: f32, gap: f32, track_reverse: bool) -> Self {
        let n_items = items.len();
        Self {
            busy: None,
            wants: vec![false; n_items],
            lane_sizes: Vec::new(),
            lane_gap: 0.0,
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

    /// Включает плотную упаковку: окно выбирает [`LanesState::settle`] по высоте элемента.
    pub(super) fn dense(mut self, mut lane_sizes: Vec<f32>, lane_gap: f32) -> Self {
        self.busy = Some(vec![Vec::new(); self.ends.len()]);
        lane_sizes.resize(self.ends.len(), 0.0);
        self.lane_sizes = lane_sizes;
        self.lane_gap = lane_gap;
        self
    }

    /// Окно полос `(первая полоса, число полос)` элемента `k`. При плотной упаковке окно уточняет
    /// `settle` (нужна высота элемента), здесь — предварительное: определённое или первое.
    pub(super) fn choose(&mut self, k: usize) -> (usize, usize) {
        let n = self.ends.len();
        let item = self.items[k];
        let span = item.span.clamp(1, n);
        if self.busy.is_some() {
            let first = item.def.map_or(0, |d| d.min(n - span));
            self.window[k] = (first, span);
            return (first, span);
        }
        let first = match item.def {
            Some(d) => d.min(n - span),
            None => pick_window(&self.ends, self.gap, span, self.tolerance, self.track_reverse, self.cursor),
        };
        // Курсор сдвигается и от элементов с определённой полосой (item-placement-004: после
        // `grid-column: 2 / span 2` равные пустые полосы берутся с четвёртой, не с первой).
        self.cursor = if self.track_reverse { n - first } else { first + span };
        self.window[k] = (first, span);
        (first, span)
    }

    /// Записывает позицию элемента `k` высотой `h` (внешней) и продвигает концы его полос.
    /// Возвращает окно полос — при плотной упаковке оно выбирается здесь.
    pub(super) fn settle(&mut self, k: usize, h: f32) -> (usize, usize) {
        if self.busy.is_some() {
            return self.settle_dense(k, h);
        }
        let (first, span) = self.window[k];
        let start = window_start(&self.ends, self.gap, first, span);
        self.pos[k] = start;
        let end = start + h.max(0.0);
        for lane in &mut self.ends[first..first + span] {
            *lane = Some(end);
        }
        self.extent = self.extent.max(end);
        (first, span)
    }

    /// Растягивает области элементов (`wants`) до следующего элемента полосы, у последнего — до `total`.
    pub(super) fn extend_last(&self, sizes: &mut [f32], total: f32) {
        extend_last_items(&self.window, &self.pos, sizes, &self.wants, self.gap, total);
    }

    fn settle_dense(&mut self, k: usize, h: f32) -> (usize, usize) {
        let n = self.ends.len();
        let item = self.items[k];
        let span = item.span.clamp(1, n);
        let h = h.max(0.0);
        // Занятость полос на время выбора окна вынута из `self`, чтобы читать её рядом с остальным состоянием.
        let mut busy = self.busy.take().unwrap_or_default();
        let pos = |w: usize| earliest_fit(&busy, self.gap, w, span, h);
        let first = match item.def {
            Some(d) => d.min(n - span),
            None => {
                // Grid L3 §4.5: сначала окно, куда элемент встал бы без плотной упаковки; щель
                // берётся лишь в окнах с тем же числом и суммарным размером полос.
                let w0 = pick_window(&self.ends, self.gap, span, self.tolerance, self.track_reverse, self.cursor);
                let total = |w: usize| self.lane_sizes[w..w + span].iter().sum::<f32>() + self.lane_gap * (span - 1) as f32;
                let s0 = total(w0);
                let order: Vec<usize> =
                    if self.track_reverse { (0..=n - span).rev().collect() } else { (0..=n - span).collect() };
                let cand: Vec<usize> = order.into_iter().filter(|&w| (total(w) - s0).abs() < 0.01).collect();
                let min = cand.iter().map(|&w| pos(w)).fold(f32::INFINITY, f32::min);
                cand.iter().copied().find(|&w| pos(w) <= min + self.tolerance).unwrap_or(w0)
            }
        };
        self.cursor = if self.track_reverse { n - first } else { first + span };
        let start = earliest_fit(&busy, self.gap, first, span, h);
        self.window[k] = (first, span);
        self.pos[k] = start;
        let end = start + h;
        for lane in &mut busy[first..first + span] {
            lane.push((start, end));
        }
        self.busy = Some(busy);
        self.extent = self.extent.max(end);
        (first, span)
    }
}

/// Полосы элементов `item_idxs` по их `grid-column-*` (полосы-столбцы) или `grid-row-*` (полосы-строки)
/// — `axis` — ось полос (`resolve_grid_axis`).
pub(super) fn lane_items(children: &[LayoutBox], item_idxs: &[usize], axis: &GridAxis) -> Vec<LaneItem> {
    item_idxs
        .iter()
        .map(|&i| {
            let is = &children[i].style;
            let (cs, ce) = if axis.is_col {
                resolve_grid_axis(&is.grid_column_start, &is.grid_column_end, axis)
            } else {
                resolve_grid_axis(&is.grid_row_start, &is.grid_row_end, axis)
            };
            if cs != 0 {
                LaneItem { def: Some(cs as usize - 1), span: (ce.saturating_sub(cs)).max(1) as usize }
            } else {
                LaneItem { def: None, span: ce.max(1) as usize }
            }
        })
        .collect()
}

/// Раскладка `grid-lanes` с полосами-строками: поток идёт по inline-оси, и ширина каждого
/// элемента (fit-content с полями) известна до замера, поэтому полосы выбираются сразу. Каждому
/// элементу достаётся свой столбец `k`: его ширина и смещение — размер и позиция в потоке.
pub(super) struct RowLanes {
    /// Размещение элементов: столбец `k`, окно полос выбранное потоком.
    pub placements: Vec<Placement>,
    pub col_widths: Vec<f32>,
    pub col_offsets: Vec<f32>,
    /// Конец самого длинного потока.
    pub extent: f32,
}

/// Параметры потока полос-строк, не зависящие от замера.
pub(super) struct RowFlow<'a> {
    /// Плотная упаковка: размеры полос (0 — неизвестны до замера).
    pub dense: Option<Vec<f32>>,
    /// `fill-reverse`: поток идёт от конца оси — позиции отражаются в `total`.
    pub fill_reverse: bool,
    /// Контейнер, чьё выравнивание по оси потока читают элементы (`None` — ширины считаются без него).
    pub align: Option<&'a ComputedStyle>,
    /// Размер контейнера вдоль потока: от него отражается `fill-reverse`.
    pub total: f32,
    pub n_lanes: usize,
    pub tolerance: f32,
    pub gap: f32,
    pub track_reverse: bool,
}

/// Поток полос-строк: `outer_width(item)` — ширина элемента вдоль потока вместе с полями.
pub(super) fn row_lanes(
    children: &[LayoutBox],
    item_idxs: &[usize],
    items: Vec<LaneItem>,
    flow: &RowFlow<'_>,
    outer_width: &dyn Fn(&LayoutBox) -> f32,
) -> RowLanes {
    // Определённая полоса за явной сеткой создаёт неявную (как строка в обычной сетке).
    let n_lanes = items.iter().map(|i| i.def.map_or(i.span, |d| d + i.span)).fold(flow.n_lanes, usize::max);
    let mut st = LanesState::new(items, n_lanes, flow.tolerance, flow.gap, flow.track_reverse);
    if let Some(sizes) = &flow.dense {
        st = st.dense(sizes.clone(), 0.0);
    }
    let mut placements = Vec::with_capacity(item_idxs.len());
    let mut col_widths = Vec::with_capacity(item_idxs.len());
    for (k, &i) in item_idxs.iter().enumerate() {
        let w = outer_width(&children[i]);
        st.choose(k);
        let (first, span) = st.settle(k, w);
        st.wants[k] = flow.align.is_some_and(|c| wants_flow_alignment(&children[i].style, c, true));
        placements.push((k as u32 + 1, k as u32 + 2, first as u32 + 1, (first + span) as u32 + 1));
        col_widths.push(w);
    }
    if flow.align.is_some() {
        st.extend_last(&mut col_widths, flow.total);
    }
    let mut col_offsets = st.pos;
    if flow.fill_reverse {
        mirror(&mut col_offsets, &col_widths, flow.total);
    }
    RowLanes { placements, col_widths, col_offsets, extent: st.extent }
}

/// Вклады элементов в размеры полос-строк (Grid L3 §3.4): автоматический элемент вносит свою
/// высоту в каждую полосу (окно полос), которую мог бы занять, определённый — в свою.
pub(super) struct RowLaneSizing {
    items: Vec<LaneItem>,
    /// `(первая полоса, за последней, высота)`.
    contribs: Vec<(usize, usize, f32)>,
}

impl RowLaneSizing {
    pub(super) fn new(items: Vec<LaneItem>) -> Self {
        Self { items, contribs: Vec::new() }
    }

    /// Вклад элемента `k` внешней высотой `h` при `n_rows` полосах.
    pub(super) fn record(&mut self, k: usize, n_rows: usize, h: f32) {
        let item = self.items[k];
        let span = item.span.clamp(1, n_rows.max(1));
        match item.def {
            Some(d) => {
                let first = d.min(n_rows.saturating_sub(span));
                self.contribs.push((first, first + span, h));
            }
            None => {
                for first in 0..=n_rows.max(span) - span {
                    self.contribs.push((first, first + span, h));
                }
            }
        }
    }

    /// Grid L1 §11.5: сначала элементы в одну полосу, затем охватывающие полосы по возрастанию
    /// числа полос; недостающее делится поровну между растущими полосами охвата.
    pub(super) fn apply(mut self, heights: &mut [f32], gap: f32, growable: impl Fn(usize) -> bool) {
        self.contribs.sort_by_key(|&(a, b, _)| b - a);
        for (a, b, h) in self.contribs {
            let b = b.min(heights.len());
            if a >= b {
                continue;
            }
            let targets: Vec<usize> = (a..b).filter(|&r| growable(r)).collect();
            if targets.is_empty() {
                continue;
            }
            let have: f32 = heights[a..b].iter().sum::<f32>() + gap * (b - a - 1) as f32;
            let extra = h - have;
            if extra > 0.0 {
                let share = extra / targets.len() as f32;
                for r in targets {
                    heights[r] += share;
                }
            }
        }
    }
}

/// Область элемента с `wants` тянется до начала следующего элемента в его полосах (минус промежуток),
/// а у последнего — до `total` от его позиции: выравнивание действует внутри такой области.
pub(super) fn extend_last_items(window: &[(usize, usize)], pos: &[f32], sizes: &mut [f32], wants: &[bool], gap: f32, total: f32) {
    let ends: Vec<f32> = pos.iter().zip(sizes.iter()).map(|(p, s)| p + s).collect();
    for k in 0..window.len() {
        if !wants[k] {
            continue;
        }
        let (f, s) = window[k];
        // Ближайший после `k` элемент, делящий с ним хотя бы одну полосу.
        let next = (0..window.len())
            .filter(|&j| j != k && pos[j] >= ends[k] - 1e-3 && window[j].0 < f + s && f < window[j].0 + window[j].1)
            .map(|j| pos[j] - gap)
            .fold(f32::INFINITY, f32::min);
        let limit = if next.is_finite() { next } else { total };
        sizes[k] = sizes[k].max(limit - pos[k]);
    }
}

/// Выровнен ли элемент по оси потока явно: `justify-*` (полосы-строки, поток по inline) или
/// `align-*` (полосы-столбцы). `normal` и `start` места сверх своего размера не просят.
pub(super) fn wants_flow_alignment(item: &ComputedStyle, container: &ComputedStyle, inline_flow: bool) -> bool {
    let v = if inline_flow {
        if matches!(item.justify_self, AlignValue::Auto) { container.justify_items } else { item.justify_self }
    } else if matches!(item.align_self, AlignValue::Auto) {
        container.align_items
    } else {
        item.align_self
    };
    matches!(v, AlignValue::Stretch | AlignValue::Center | AlignValue::End)
}

/// `fill-reverse`: позиции `pos` элементов размером `size` отражаются в отрезке `[0, total]`.
pub(super) fn mirror(pos: &mut [f32], size: &[f32], total: f32) {
    for (p, s) in pos.iter_mut().zip(size) {
        *p = total - (*p + s);
    }
}

/// Ширина элемента вдоль потока полос-строк: fit-content от ширины контейнера, с полями.
pub(super) fn fit_outer_width(
    item: &LayoutBox,
    content_width: f32,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
) -> f32 {
    let em = item.style.font_size;
    let ml = item.style.margin_left.resolve_or_zero(em, 0.0, viewport);
    let mr = item.style.margin_right.resolve_or_zero(em, 0.0, viewport);
    // Процент вдоль потока считается от ширины контейнера (Grid L3 §3.6), не от области элемента.
    if let Some(len) = item.style.width.as_ref().filter(|l| l.contains_percent() && !l.is_intrinsic())
        && let Some(w) = len.resolve(em, Some(content_width), viewport)
    {
        let st = &item.style;
        let edges = st.padding_left.resolve_or_zero(em, content_width, viewport)
            + st.padding_right.resolve_or_zero(em, content_width, viewport)
            + st.border_left_width
            + st.border_right_width;
        let border_box = match st.box_sizing {
            crate::style::BoxSizing::ContentBox => w.max(0.0) + edges,
            crate::style::BoxSizing::BorderBox => w.max(edges),
        };
        return border_box + ml + mr;
    }
    let (mn, mx) = super::grid_auto_cols::item_contribution(measurer, viewport)(item);
    mx.min((content_width - ml - mr).max(mn)) + ml + mr
}

/// `flow-tolerance` в пикселях; `%` считается от `basis`.
pub(super) fn tolerance_px(s: &ComputedStyle, em: f32, basis: Option<f32>, viewport: Size) -> f32 {
    match &s.grid_lanes.tolerance {
        crate::style::FlowTolerance::Normal => em,
        crate::style::FlowTolerance::Infinite => f32::INFINITY,
        crate::style::FlowTolerance::Length(l) => l.resolve(em, basis, viewport).unwrap_or(em).max(0.0),
    }
}

/// Внутренний размер контейнера с полосами-строками вдоль потока (Grid L3 §3.6): конец самого
/// длинного потока при размере элемента `per_item` (min- или max-content) и неограниченной ширине.
pub(super) fn row_lanes_intrinsic(b: &LayoutBox, viewport: Size, per_item: &dyn Fn(&LayoutBox) -> f32) -> f32 {
    let s = &b.style;
    let item_idxs = super::grid::grid_item_indices(&b.children, super::fieldset::rendered_legend_idx_of(b));
    let row_axis = GridAxis {
        n_tracks: s.grid_template_rows.len().max(s.grid_template_areas.len()) as u32,
        names: &s.grid_template_row_line_names,
        areas: &s.grid_template_areas,
        is_col: false,
        clamp: false,
    };
    let items = lane_items(&b.children, &item_idxs, &row_axis);
    let flow = RowFlow {
        dense: is_dense(s).then(Vec::new),
        fill_reverse: false,
        align: None,
        total: 0.0,
        n_lanes: row_axis.n_tracks.max(1) as usize,
        tolerance: tolerance_px(s, s.font_size, Some(0.0), viewport),
        gap: s.column_gap.resolve(s.font_size, Some(0.0), viewport).unwrap_or(0.0).max(0.0),
        track_reverse: s.grid_lanes.direction.track_reverse,
    };
    let outer = |c: &LayoutBox| {
        let em = c.style.font_size;
        per_item(c)
            + c.style.margin_left.resolve_or_zero(em, 0.0, viewport)
            + c.style.margin_right.resolve_or_zero(em, 0.0, viewport)
    };
    row_lanes(&b.children, &item_idxs, items, &flow, &outer).extent
}

/// Дорожка с размером по содержимому (в `repeat(auto-fill, …)` она не имеет заранее известной длины).
fn track_is_intrinsic(t: &GridTrackSize) -> bool {
    match t {
        GridTrackSize::Auto | GridTrackSize::MinContent | GridTrackSize::MaxContent | GridTrackSize::FitContent(_) => true,
        GridTrackSize::Minmax(min, _) => track_is_intrinsic(min),
        _ => false,
    }
}

/// Размер дорожки повтора по самому большому вкладу элемента на полосу: `min`/`max` — min- и
/// max-content вклады (Grid L3 §7.2.3.2).
fn repeat_track_size(t: &GridTrackSize, min: f32, max: f32, em: f32, avail: f32, viewport: Size) -> f32 {
    match t {
        GridTrackSize::MinContent => min,
        GridTrackSize::FitContent(limit) => max.min(limit.resolve_fixed(em, avail, viewport).unwrap_or(max)),
        // Определённый максимум задаёт размер, иначе — функция максимума (`auto` растёт до max-content).
        GridTrackSize::Minmax(lo, hi) => match hi.resolve_fixed(em, avail, viewport) {
            Some(px) => px.max(lo.resolve_fixed(em, avail, viewport).unwrap_or(0.0)).max(0.0),
            None => repeat_track_size(hi, min, max, em, avail, viewport),
        },
        t if track_is_intrinsic(t) => max,
        other => other.resolve_fixed(em, avail, viewport).unwrap_or(0.0).max(0.0),
    }
}

/// Grid L3 §7.2.3.2: число повторов `repeat(auto-fill | auto-fit, …)` с дорожками по содержимому
/// и сам повтор: дорожки с размером по содержимому в нём равны между собой (каждая — самый
/// большой вклад элемента на одну полосу: вклад / число полос, которое он занимает). `None` —
/// повтор из обычных длин, считается как у сетки.
#[allow(clippy::too_many_arguments)]
pub(super) fn intrinsic_repeat(
    rep: &GridRepeat,
    is_col: bool,
    children: &[LayoutBox],
    item_idxs: &[usize],
    s: &ComputedStyle,
    avail: Option<f32>,
    gap: f32,
    em: f32,
    viewport: Size,
    measurer: Option<&dyn TextMeasurer>,
    content_width: f32,
    pcb: lumen_core::geom::Rect,
) -> Option<(usize, GridRepeat)> {
    if !rep.tracks.iter().any(track_is_intrinsic) {
        return None;
    }
    let avail = avail.filter(|a| *a > 0.0)?;
    // Число полос элемента — только из `span`/линий, длина шаблона для него не важна.
    let (starts, names): (usize, &[Vec<String>]) =
        if is_col { (s.grid_template_columns.len(), &s.grid_template_col_line_names) } else { (s.grid_template_rows.len(), &s.grid_template_row_line_names) };
    let axis = GridAxis { n_tracks: starts as u32, names, areas: &s.grid_template_areas, is_col, clamp: false };
    let spans = lane_items(children, item_idxs, &axis);
    let contribution = super::grid_auto_cols::item_contribution(measurer, viewport);
    let (mut min, mut max) = (0.0_f32, 0.0_f32);
    for (&i, it) in item_idxs.iter().zip(&spans) {
        let c = &children[i];
        let ce = c.style.font_size;
        let share = it.span.max(1) as f32;
        let (lo, hi) = if is_col {
            let (lo, hi) = contribution(c);
            let m = c.style.margin_left.resolve_or_zero(ce, 0.0, viewport) + c.style.margin_right.resolve_or_zero(ce, 0.0, viewport);
            (lo + m, hi + m)
        } else {
            let m = c.style.margin_top.resolve_or_zero(ce, 0.0, viewport) + c.style.margin_bottom.resolve_or_zero(ce, 0.0, viewport);
            // Полосы-строки: элемент уже имеет ширину (вдоль потока), высоту даёт его раскладка.
            let w = fit_outer_width(c, content_width, viewport, measurer);
            let mut probe = c.clone();
            super::lay_out(&mut probe, 0.0, 0.0, w, None, measurer, viewport, pcb, &lumen_core::ext::NullHyphenationProvider, false);
            let h = probe.rect.height + m;
            (h, h)
        };
        min = min.max(lo / share);
        max = max.max(hi / share);
    }
    let count = super::grid::auto_repeat_count_by(rep, avail, gap, &|t| repeat_track_size(t, min, max, em, avail, viewport));
    let equal = rep.clone();
    Some((count, equal))
}

/// Grid L3 §3.4: элемент с автоматическим положением вносит вклад в каждую полосу, которую мог бы
/// занять, — не только в ту, куда в итоге попал. Возвращает элементы и их размещения для подсчёта
/// ширин полос: определённый элемент остаётся как есть, автоматический — по копии на каждое
/// начало окна из `n_cols` полос.
pub(super) fn sizing_items(
    item_idxs: &[usize],
    placements: &[Placement],
    lanes: &[LaneItem],
    n_cols: usize,
) -> (Vec<usize>, Vec<Placement>) {
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
    pick_by(|w| window_start(ends, gap, w, span), ends.len(), span, tolerance, reverse, cursor)
}

/// Самая ранняя позиция, где элемент высотой `h` помещается во все полосы окна `first..first+span`
/// (плотная упаковка): с нуля или сразу за концом какого-либо занятого отрезка, с промежутком.
fn earliest_fit(busy: &[Vec<(f32, f32)>], gap: f32, first: usize, span: usize, h: f32) -> f32 {
    let lanes = &busy[first..first + span];
    let mut cands: Vec<f32> = vec![0.0];
    cands.extend(lanes.iter().flatten().map(|&(_, e)| e + gap));
    cands.sort_by(f32::total_cmp);
    let fits = |t: f32| lanes.iter().flatten().all(|&(a, b)| !(t < b + gap && a < t + h + gap));
    cands.into_iter().find(|&t| fits(t)).unwrap_or(0.0)
}

/// [`pick_window`] по произвольной позиции окна `pos`.
fn pick_by(pos: impl Fn(usize) -> f32, n: usize, span: usize, tolerance: f32, reverse: bool, cursor: usize) -> usize {
    let starts: Vec<usize> = if reverse { (0..=n - span).rev().collect() } else { (0..=n - span).collect() };
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
    fn definite_item_moves_the_cursor() {
        // item-placement-004: `grid-column: 2 / span 2` из четырёх пустых полос — следующий
        // автоэлемент берёт из равных полос 0 и 3 ту, что после курсора (3), затем снова 0.
        let items = vec![LaneItem { def: Some(1), span: 2 }, auto(1), auto(1)];
        let mut st = LanesState::new(items, 4, 0.0, 0.0, false);
        let lanes: Vec<usize> = (0..3)
            .map(|k| {
                let l = st.choose(k).0;
                st.settle(k, 10.0);
                l
            })
            .collect();
        assert_eq!(lanes, vec![1, 3, 0]);
    }

    #[test]
    fn definite_lane_and_track_reverse() {
        let mut st = LanesState::new(vec![LaneItem { def: Some(1), span: 1 }, auto(1)], 3, 0.0, 0.0, true);
        assert_eq!(st.choose(0).0, 1);
        st.settle(0, 10.0);
        // свободны полосы 0 и 2; при track-reverse порядок 2, 1, 0, курсор после полосы 1 — это полоса 0.
        assert_eq!(st.choose(1).0, 0);
    }
}

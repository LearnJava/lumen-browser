//! CSS Animations L1 timeline scheduler — P3 п.3A.
//!
//! `AnimationScheduler` отслеживает запущенные анимации и на каждом vsync
//! вычисляет интерполированные значения для всех анимированных элементов.
//! Выходной тип — `AnimationFrame` — используется оболочкой для
//! `request_redraw` и P2 compositor-ом для compositor offload (task 3B).
//!
//! Алгоритм одного тика:
//! 1. Обход layout-дерева.
//! 2. Для каждого элемента с `animation_names` — вычислить `t ∈ [0,1]`.
//! 3. Найти `@keyframes` по имени в Stylesheet.
//! 4. Интерполировать свойства между ближайшими keyframe-ами.
//! 5. Записать в `AnimationFrame.overrides[node_id]`.

use std::collections::HashMap;

use lumen_css_parser::{Keyframe, KeyframesRule, Stylesheet};
use lumen_dom::NodeId;
use lumen_layout::{
    animation::{
        AnimatedStyle, AnimationFrame, AnimationInterpolator, AnimValue, KeyframeStyle,
        LinearInterpolator, parse_keyframe_style,
    },
    style::{
        AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationTimeline,
        IterationCount, TimingFunction,
    },
    collect_named_scroll_timelines, collect_named_view_timelines, resolve_scroll_progress,
    resolve_view_progress, LayoutBox, NamedScrollTimeline, NamedViewTimeline, ScrollTimeline,
    ViewTimeline, Viewport,
};

/// Ключ одного экземпляра анимации: (элемент, индекс в списке animation-name).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AnimKey {
    node: NodeId,
    index: usize,
}

/// Состояние одного запущенного экземпляра анимации.
#[derive(Debug)]
struct RunState {
    /// DOMHighResTimeStamp старта (после применения задержки).
    start_ms: f64,
    /// Если `Some(ts)` — анимация стоит на паузе с момента `ts`.
    paused_at: Option<f64>,
}

/// Scroll-context для одного тика: всё, что нужно резолверам прогресса
/// scroll-driven анимаций (CSS Scroll-Driven Animations L1).
///
/// Собирается один раз в начале `tick` из корня layout-дерева и текущих
/// scroll-офсетов, затем протягивается в `process_node`.
struct ScrollCtx<'a> {
    /// Корень layout-дерева — резолверы прогресса ищут от него subject/container.
    root: &'a LayoutBox,
    /// Текущий горизонтальный scroll-офсет корневого вьюпорта (CSS px).
    scroll_x: f32,
    /// Текущий вертикальный scroll-офсет корневого вьюпорта (CSS px).
    scroll_y: f32,
    /// Размеры вьюпорта (CSS px) для view()-прогресса.
    viewport: Viewport,
    /// Именованные `scroll-timeline` из дерева (для `animation-timeline: --name`).
    named_scroll: Vec<NamedScrollTimeline>,
    /// Именованные `view-timeline` из дерева (для `animation-timeline: --name`).
    named_view: Vec<NamedViewTimeline>,
}

impl ScrollCtx<'_> {
    /// Прогресс `[0,1]` для timeline узла `node`, либо `None` если timeline =
    /// `auto` (тогда анимация управляется обычными часами `@keyframes`).
    ///
    /// * `scroll()` — прогресс корневого вьюпорта по нужной оси. `nearest`/`self`
    ///   аппроксимируются корневым вьюпортом (полный резолвинг ближайшего
    ///   scroll-контейнера — задача L2).
    /// * `view()` — view-прогресс самого узла как subject (cover-диапазон).
    /// * `<custom-ident>` — матч против именованных scroll/view timeline-ов;
    ///   неизвестное имя → inactive timeline, удерживаем прогресс 0 (from-state).
    fn progress_for(&self, timeline: &AnimationTimeline, node: NodeId) -> Option<f32> {
        match timeline {
            AnimationTimeline::Auto => None,
            AnimationTimeline::Scroll { axis, .. } => {
                let tl = ScrollTimeline { element: None, axis: *axis };
                Some(resolve_scroll_progress(
                    &tl, self.root, self.scroll_x, self.scroll_y, self.viewport,
                ))
            }
            AnimationTimeline::View { axis } => {
                let tl = ViewTimeline { element: node, axis: *axis };
                Some(resolve_view_progress(
                    &tl, self.root, self.scroll_y, self.scroll_x, self.viewport,
                ))
            }
            AnimationTimeline::Named(name) => {
                if let Some(t) = self.named_scroll.iter().find(|t| t.name == *name) {
                    let tl = ScrollTimeline { element: Some(t.container), axis: t.axis };
                    Some(resolve_scroll_progress(
                        &tl, self.root, self.scroll_x, self.scroll_y, self.viewport,
                    ))
                } else if let Some(t) = self.named_view.iter().find(|t| t.name == *name) {
                    let tl = ViewTimeline { element: t.subject, axis: t.axis };
                    Some(resolve_view_progress(
                        &tl, self.root, self.scroll_y, self.scroll_x, self.viewport,
                    ))
                } else {
                    Some(0.0)
                }
            }
        }
    }
}

/// Планировщик CSS-анимаций. Хранит timing-состояние между кадрами.
/// Single-threaded; создаётся один раз в `Lumen` и тикается на каждом
/// `RedrawRequested` после `run_rendering_step`.
pub struct AnimationScheduler {
    running: HashMap<AnimKey, RunState>,
}

impl AnimationScheduler {
    pub fn new() -> Self {
        Self {
            running: HashMap::new(),
        }
    }

    /// Тик планировщика: обходит layout-дерево, для каждой активной анимации
    /// вычисляет интерполированный стиль и записывает в `AnimationFrame`.
    ///
    /// `scroll_x`/`scroll_y`/`viewport` дают контекст для scroll-driven анимаций
    /// (`animation-timeline: scroll()|view()|<custom-ident>`): их прогресс берётся
    /// из положения скролла/вьюпорта, а не из часов `@keyframes`.
    pub fn tick(
        &mut self,
        timestamp_ms: f64,
        layout: &LayoutBox,
        stylesheet: &Stylesheet,
        scroll_x: f32,
        scroll_y: f32,
        viewport: Viewport,
    ) -> AnimationFrame {
        let mut frame = AnimationFrame::default();
        let ctx = ScrollCtx {
            root: layout,
            scroll_x,
            scroll_y,
            viewport,
            named_scroll: collect_named_scroll_timelines(layout),
            named_view: collect_named_view_timelines(layout),
        };
        self.tick_box(timestamp_ms, layout, stylesheet, &ctx, &mut frame);
        frame
    }

    /// Удалить все записи для элементов, которых больше нет в дереве.
    /// Вызывать после load/reload, чтобы не накапливать мёртвые записи.
    pub fn clear(&mut self) {
        self.running.clear();
    }

    fn tick_box(
        &mut self,
        ts: f64,
        lb: &LayoutBox,
        ss: &Stylesheet,
        ctx: &ScrollCtx,
        frame: &mut AnimationFrame,
    ) {
        self.process_node(ts, lb, ss, ctx, frame);
        for child in &lb.children {
            self.tick_box(ts, child, ss, ctx, frame);
        }
    }

    fn process_node(
        &mut self,
        ts: f64,
        lb: &LayoutBox,
        ss: &Stylesheet,
        ctx: &ScrollCtx,
        frame: &mut AnimationFrame,
    ) {
        let style = &lb.style;
        if style.animation_names.is_empty() {
            return;
        }

        let n = style.animation_names.len();
        for i in 0..n {
            let name = &style.animation_names[i];
            if name.eq_ignore_ascii_case("none") || name.is_empty() {
                continue;
            }

            // Параметры, общие для time-based и scroll-driven путей.
            let direction = get_cyclic(&style.animation_directions, i)
                .copied()
                .unwrap_or(AnimationDirection::Normal);
            let timing_fn = get_cyclic(&style.animation_timing_functions, i)
                .cloned()
                .unwrap_or_default();
            // animation-timeline для этого индекса (cyclic, default `auto`).
            let timeline = get_cyclic(&style.animation_timelines, i)
                .cloned()
                .unwrap_or_default();

            // Найти @keyframes по имени.
            let Some(kf_rule) = ss.keyframes.iter().find(|k| k.name == *name) else {
                continue;
            };

            let t = match ctx.progress_for(&timeline, lb.node) {
                // CSS Scroll-Driven Animations L1 — прогресс задаёт scroll/view
                // timeline, а не часы. animation-duration игнорируется; has_active
                // НЕ взводим — кадр перевычисляется на следующем скролле/redraw,
                // непрерывная перерисовка не нужна.
                Some(progress) => {
                    let t_raw = apply_direction(progress.clamp(0.0, 1.0), 0, direction);
                    timing_fn.progress(t_raw)
                }
                // animation-timeline: auto — обычная анимация по часам.
                None => {
                    let duration =
                        get_cyclic(&style.animation_durations, i).copied().unwrap_or(0.0);
                    if duration <= 0.0 {
                        continue;
                    }
                    let delay = get_cyclic(&style.animation_delays, i).copied().unwrap_or(0.0);
                    let play_state = get_cyclic(&style.animation_play_states, i)
                        .copied()
                        .unwrap_or(AnimationPlayState::Running);
                    let iter_count = get_cyclic(&style.animation_iteration_counts, i)
                        .cloned()
                        .unwrap_or(IterationCount::Finite(1.0));
                    let fill_mode = get_cyclic(&style.animation_fill_modes, i)
                        .copied()
                        .unwrap_or(AnimationFillMode::None);

                    let key = AnimKey {
                        node: lb.node,
                        index: i,
                    };

                    // Регистрируем новую анимацию — начало отсчёта = сейчас.
                    self.running.entry(key.clone()).or_insert(RunState {
                        start_ms: ts,
                        paused_at: None,
                    });

                    let state = self.running.get_mut(&key).unwrap();

                    // Учёт play-state: пауза/возобновление.
                    match play_state {
                        AnimationPlayState::Paused => {
                            if state.paused_at.is_none() {
                                state.paused_at = Some(ts);
                            }
                        }
                        AnimationPlayState::Running => {
                            if let Some(paused_at) = state.paused_at.take() {
                                // Сдвигаем start_ms на время паузы.
                                state.start_ms += ts - paused_at;
                            }
                        }
                    }

                    // Локальное время (в секундах) с учётом задержки.
                    let elapsed_ms = match state.paused_at {
                        Some(paused_at) => paused_at - state.start_ms,
                        None => ts - state.start_ms,
                    };
                    let local_time_s = elapsed_ms / 1000.0 - delay as f64;

                    // Вычислить t ∈ [0,1] для текущего момента.
                    let Some(t) = compute_t(
                        local_time_s,
                        duration as f64,
                        &iter_count,
                        direction,
                        &timing_fn,
                        fill_mode,
                    ) else {
                        continue;
                    };

                    frame.has_active = true;
                    t
                }
            };

            // Интерполировать keyframe-значения.
            let animated = interpolate_at(kf_rule, t);

            // Слить в overrides для этого узла.
            let entry = frame.overrides.entry(lb.node).or_default();
            if let Some(v) = animated.opacity {
                entry.opacity = Some(v);
            }
            if let Some(v) = animated.transform {
                entry.transform = Some(v);
            }
            if let Some(v) = animated.color {
                entry.color = Some(v);
            }
            if let Some(v) = animated.background_color {
                entry.background_color = Some(v);
            }
        }
    }
}

// ─── Вычисление t ──────────────────────────────────────────────────────────

/// CSS Animations L1 §4.2 — вычислить progress `t ∈ [0,1]` на момент
/// `local_time_s` (уже с вычтенной задержкой).
///
/// `None` означает «за пределами активного периода и fill-mode не требует
/// удержания значения» — caller пропускает этот узел для данной анимации.
fn compute_t(
    local_time_s: f64,
    duration_s: f64,
    iter_count: &IterationCount,
    direction: AnimationDirection,
    timing_fn: &TimingFunction,
    fill_mode: AnimationFillMode,
) -> Option<f32> {
    // До начала активного периода.
    if local_time_s < 0.0 {
        return match fill_mode {
            AnimationFillMode::Backwards | AnimationFillMode::Both => Some(0.0),
            _ => None,
        };
    }

    let max_iters: f64 = match iter_count {
        IterationCount::Infinite => f64::INFINITY,
        IterationCount::Finite(n) => *n as f64,
    };
    let total_s = duration_s * max_iters;

    // После конца активного периода.
    if !total_s.is_infinite() && local_time_s >= total_s {
        return match fill_mode {
            AnimationFillMode::Forwards | AnimationFillMode::Both => Some(1.0),
            _ => None,
        };
    }

    // Внутри активного периода.
    let current_iter = (local_time_s / duration_s).floor();
    // Прогресс внутри текущей итерации [0, 1).
    let iter_progress = (local_time_s % duration_s) / duration_s;

    let t_raw = apply_direction(iter_progress as f32, current_iter as u64, direction);
    Some(timing_fn.progress(t_raw))
}

fn apply_direction(progress: f32, iteration: u64, direction: AnimationDirection) -> f32 {
    match direction {
        AnimationDirection::Normal => progress,
        AnimationDirection::Reverse => 1.0 - progress,
        AnimationDirection::Alternate => {
            if iteration.is_multiple_of(2) { progress } else { 1.0 - progress }
        }
        AnimationDirection::AlternateReverse => {
            if iteration.is_multiple_of(2) { 1.0 - progress } else { progress }
        }
    }
}

// ─── Интерполяция keyframe-ов ──────────────────────────────────────────────

/// Интерполировать свойства keyframe-правила в точке `t ∈ [0,1]`.
fn interpolate_at(rule: &KeyframesRule, t: f32) -> AnimatedStyle {
    let frames = &rule.frames;
    if frames.is_empty() {
        return AnimatedStyle::default();
    }

    // Сортируем по offset (источник может быть неупорядочен).
    let mut sorted: Vec<&Keyframe> = frames.iter().collect();
    sorted.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Найти пару (from, to) так что from.offset ≤ t ≤ to.offset.
    let to_idx = sorted
        .iter()
        .position(|f| f.offset >= t)
        .unwrap_or(sorted.len() - 1);
    let from_idx = if to_idx == 0 { 0 } else { to_idx - 1 };

    let from_kf = sorted[from_idx];
    let to_kf = sorted[to_idx];

    // Нормализованный t внутри интервала.
    let interval = to_kf.offset - from_kf.offset;
    let local_t = if interval < f32::EPSILON {
        1.0f32
    } else {
        ((t - from_kf.offset) / interval).clamp(0.0, 1.0)
    };

    let from_ks = parse_keyframe_style(&from_kf.declarations);
    let to_ks = parse_keyframe_style(&to_kf.declarations);

    interpolate_keyframe_styles(&from_ks, &to_ks, local_t)
}

/// Попарно интерполировать все поля KeyframeStyle.
/// Поле включается в результат только если оба кадра его объявляют.
fn interpolate_keyframe_styles(from: &KeyframeStyle, to: &KeyframeStyle, t: f32) -> AnimatedStyle {
    let interp = LinearInterpolator;
    let mut result = AnimatedStyle::default();

    if let (Some(a), Some(b)) = (from.opacity, to.opacity)
        && let Some(AnimValue::Number(v)) =
            interp.interpolate(&AnimValue::Number(a), &AnimValue::Number(b), t)
    {
        result.opacity = Some(v.clamp(0.0, 1.0));
    }

    if let (Some(a), Some(b)) = (&from.transform, &to.transform)
        && let Some(AnimValue::TransformList(v)) = interp.interpolate(
            &AnimValue::TransformList(a.clone()),
            &AnimValue::TransformList(b.clone()),
            t,
        )
    {
        result.transform = Some(v);
    }

    if let (Some(a), Some(b)) = (from.color, to.color)
        && let Some(AnimValue::Color(v)) =
            interp.interpolate(&AnimValue::Color(a), &AnimValue::Color(b), t)
    {
        result.color = Some(v);
    }

    if let (Some(a), Some(b)) = (from.background_color, to.background_color)
        && let Some(AnimValue::Color(v)) =
            interp.interpolate(&AnimValue::Color(a), &AnimValue::Color(b), t)
    {
        result.background_color = Some(v);
    }

    result
}

// ─── Вспомогательные функции ───────────────────────────────────────────────

/// Cyclic-индексирование параллельных списков CSS Animations L1 §4.2.
/// Если список пуст — `None`.
fn get_cyclic<T>(list: &[T], i: usize) -> Option<&T> {
    if list.is_empty() {
        None
    } else {
        Some(&list[i % list.len()])
    }
}

// ─── Тесты ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_layout::style::{AnimationFillMode, IterationCount};

    fn make_timing() -> TimingFunction {
        TimingFunction::Linear
    }

    // compute_t: до начала без fill → None
    #[test]
    fn compute_t_before_active_no_fill_is_none() {
        let r = compute_t(
            -0.5,
            1.0,
            &IterationCount::Finite(1.0),
            AnimationDirection::Normal,
            &make_timing(),
            AnimationFillMode::None,
        );
        assert_eq!(r, None);
    }

    // compute_t: до начала + backwards → 0.0
    #[test]
    fn compute_t_backwards_fill_before_start() {
        let r = compute_t(
            -0.5,
            1.0,
            &IterationCount::Finite(1.0),
            AnimationDirection::Normal,
            &make_timing(),
            AnimationFillMode::Backwards,
        );
        assert_eq!(r, Some(0.0));
    }

    // compute_t: после конца без fill → None
    #[test]
    fn compute_t_after_end_no_fill_is_none() {
        let r = compute_t(
            2.0,
            1.0,
            &IterationCount::Finite(1.0),
            AnimationDirection::Normal,
            &make_timing(),
            AnimationFillMode::None,
        );
        assert_eq!(r, None);
    }

    // compute_t: после конца + forwards → 1.0
    #[test]
    fn compute_t_forwards_fill_after_end() {
        let r = compute_t(
            2.0,
            1.0,
            &IterationCount::Finite(1.0),
            AnimationDirection::Normal,
            &make_timing(),
            AnimationFillMode::Forwards,
        );
        assert_eq!(r, Some(1.0));
    }

    // compute_t: середина одной итерации
    #[test]
    fn compute_t_midpoint_linear() {
        let r = compute_t(
            0.5,
            1.0,
            &IterationCount::Finite(1.0),
            AnimationDirection::Normal,
            &make_timing(),
            AnimationFillMode::None,
        );
        let t = r.expect("should be active");
        assert!((t - 0.5).abs() < 1e-4, "expected 0.5, got {t}");
    }

    // compute_t: reverse direction инвертирует прогресс
    #[test]
    fn compute_t_reverse_direction() {
        let r = compute_t(
            0.25,
            1.0,
            &IterationCount::Finite(1.0),
            AnimationDirection::Reverse,
            &make_timing(),
            AnimationFillMode::None,
        );
        let t = r.expect("active");
        assert!((t - 0.75).abs() < 1e-4, "expected 0.75, got {t}");
    }

    // compute_t: infinite iteration — всегда active
    #[test]
    fn compute_t_infinite_iter_always_active() {
        let r = compute_t(
            9999.0,
            1.0,
            &IterationCount::Infinite,
            AnimationDirection::Normal,
            &make_timing(),
            AnimationFillMode::None,
        );
        assert!(r.is_some(), "infinite animation should be active");
    }

    // compute_t: alternate — чётная итерация forward, нечётная reverse
    #[test]
    fn compute_t_alternate_direction() {
        let even = compute_t(
            0.25,
            1.0,
            &IterationCount::Finite(4.0),
            AnimationDirection::Alternate,
            &make_timing(),
            AnimationFillMode::None,
        );
        let odd = compute_t(
            1.25,
            1.0,
            &IterationCount::Finite(4.0),
            AnimationDirection::Alternate,
            &make_timing(),
            AnimationFillMode::None,
        );
        let t_even = even.expect("even iter active");
        let t_odd = odd.expect("odd iter active");
        assert!((t_even - 0.25).abs() < 1e-4, "even iter: expected 0.25, got {t_even}");
        assert!((t_odd - 0.75).abs() < 1e-4, "odd iter: expected 0.75, got {t_odd}");
    }

    // get_cyclic: обычный индекс
    #[test]
    fn get_cyclic_in_bounds() {
        let list = vec![1.0f32, 2.0, 3.0];
        assert_eq!(get_cyclic(&list, 1), Some(&2.0));
    }

    // get_cyclic: выход за границу — wrap
    #[test]
    fn get_cyclic_wraps() {
        let list = vec![1.0f32, 2.0];
        assert_eq!(get_cyclic(&list, 3), Some(&2.0));
    }

    // get_cyclic: пустой список — None
    #[test]
    fn get_cyclic_empty_returns_none() {
        let list: Vec<f32> = Vec::new();
        assert_eq!(get_cyclic(&list, 0), None);
    }

    // ── ScrollCtx::progress_for — scroll-driven timeline resolution ──────────

    use lumen_core::geom::Rect;
    use lumen_layout::style::ComputedStyle;
    use lumen_layout::{BoxKind, ScrollAxis};
    use lumen_dom::NodeId;

    fn node(id: u32) -> NodeId {
        NodeId::from_index(id as usize)
    }

    fn make_box(id: u32, x: f32, y: f32, w: f32, h: f32) -> LayoutBox {
        LayoutBox {
            node: node(id),
            rect: Rect { x, y, width: w, height: h },
            style: ComputedStyle::root(),
            kind: BoxKind::Block,
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: Default::default(),
        }
    }

    fn ctx_for(root: &LayoutBox, scroll_y: f32) -> ScrollCtx<'_> {
        ScrollCtx {
            root,
            scroll_x: 0.0,
            scroll_y,
            viewport: Viewport { width: 1024.0, height: 720.0 },
            named_scroll: collect_named_scroll_timelines(root),
            named_view: collect_named_view_timelines(root),
        }
    }

    // animation-timeline: auto → None (управляется часами).
    #[test]
    fn progress_for_auto_is_none() {
        let root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        let ctx = ctx_for(&root, 0.0);
        assert_eq!(ctx.progress_for(&AnimationTimeline::Auto, node(1)), None);
    }

    // scroll() корневого вьюпорта: scroll 0 → 0, половина → ~0.5.
    #[test]
    fn progress_for_scroll_root() {
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        root.children.push(make_box(2, 0.0, 0.0, 1024.0, 2000.0));
        let tl = AnimationTimeline::Scroll { axis: ScrollAxis::Block, nearest: true };

        let at0 = ctx_for(&root, 0.0).progress_for(&tl, node(1)).unwrap();
        assert!(at0.abs() < 1e-6, "scroll 0 → progress 0, got {at0}");

        // content 2000, vp 720 → max 1280; scroll 640 → 0.5.
        let half = ctx_for(&root, 640.0).progress_for(&tl, node(1)).unwrap();
        assert!((half - 0.5).abs() < 0.01, "expected ~0.5, got {half}");
    }

    // Named scroll-timeline резолвится по своему контейнеру, не по корню.
    #[test]
    fn progress_for_named_scroll_container() {
        let mut root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        let mut container = make_box(2, 0.0, 0.0, 400.0, 160.0);
        container.style.scroll_timeline_name = Some("--page".to_string());
        container.style.scroll_timeline_axis = ScrollAxis::Block;
        container.scroll_y = 60.0;
        // content 400 tall inside 160 container → max_scroll 240; 60/240 = 0.25.
        container.children.push(make_box(3, 0.0, 0.0, 400.0, 400.0));
        root.children.push(container);

        let ctx = ctx_for(&root, 0.0);
        let p = ctx
            .progress_for(&AnimationTimeline::Named("--page".into()), node(9))
            .unwrap();
        assert!((p - 0.25).abs() < 0.01, "expected ~0.25, got {p}");
    }

    // Неизвестное имя timeline → inactive, удерживаем from-state (progress 0).
    #[test]
    fn progress_for_named_unknown_is_zero() {
        let root = make_box(1, 0.0, 0.0, 1024.0, 720.0);
        let ctx = ctx_for(&root, 0.0);
        let p = ctx
            .progress_for(&AnimationTimeline::Named("--missing".into()), node(1))
            .unwrap();
        assert_eq!(p, 0.0);
    }
}

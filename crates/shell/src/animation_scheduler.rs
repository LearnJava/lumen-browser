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
    /// `animation-name` в момент регистрации — переживает удаление узла из
    /// текущего кадра ровно настолько, чтобы `animationcancel` мог назвать,
    /// какая анимация отменена (GAP-CSSANIM срез 2).
    name: String,
    /// Взводится, когда `animationstart` уже отправлен — не даёт послать его
    /// повторно на каждом следующем кадре активного периода.
    started_fired: bool,
    /// Число уже пройденных и отражённых `animationiteration` итераций.
    /// Последняя итерация не считается — её завершение даёт `animationend`,
    /// а не `animationiteration` (CSS Animations L1 §4.5.1).
    iterations_completed: u64,
    /// Взводится, когда `animationend` уже отправлен — активный период
    /// длится один раз, повторный проход того же кадра (fill-mode
    /// forwards/both держит t=1) не должен слать событие снова.
    completed: bool,
    /// Последнее вычисленное локальное время (сек, с учётом задержки) —
    /// используется как `elapsedTime`, если экземпляр окажется отменён
    /// (узел/анимация исчезли из следующего кадра).
    last_local_time_s: f64,
}

/// CSS Animations L1 §4.5.1 — один из четырёх событий жизненного цикла
/// CSS-анимации, которые планировщик должен отправить этим кадром.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationEventKind {
    /// Анимация вошла в активный период (после задержки).
    Start,
    /// Завершилась очередная итерация, кроме последней.
    Iteration,
    /// Активный период завершился (последняя итерация доиграна).
    End,
    /// Активный экземпляр анимации исчез до завершения — сменилось
    /// `animation-name`, элемент убран из дерева, или `@keyframes`/
    /// `animation-duration` перестали описывать активную анимацию.
    Cancel,
}

/// Одно событие, которое `AnimationScheduler::tick` вернул этим кадром —
/// оболочка превращает его в настоящий DOM `AnimationEvent`.
#[derive(Debug, Clone)]
pub struct AnimationEventInfo {
    pub node: NodeId,
    /// `AnimationEvent.animationName`.
    pub animation_name: String,
    pub kind: AnimationEventKind,
    /// `AnimationEvent.elapsedTime` — секунды в активном периоде на момент
    /// события (0 для `Start`).
    pub elapsed_time: f32,
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
    ///
    /// Возвращает вместе с кадром список событий жизненного цикла
    /// (`animationstart`/`animationiteration`/`animationend`/`animationcancel`,
    /// GAP-CSSANIM срез 2) — только для time-based (`animation-timeline: auto`)
    /// анимаций; scroll-driven таймлайны вне охвата этого среза.
    pub fn tick(
        &mut self,
        timestamp_ms: f64,
        layout: &LayoutBox,
        stylesheet: &Stylesheet,
        scroll_x: f32,
        scroll_y: f32,
        viewport: Viewport,
    ) -> (AnimationFrame, Vec<AnimationEventInfo>) {
        let mut frame = AnimationFrame::default();
        let mut events = Vec::new();
        let mut visited: std::collections::HashSet<AnimKey> = std::collections::HashSet::new();
        let ctx = ScrollCtx {
            root: layout,
            scroll_x,
            scroll_y,
            viewport,
            named_scroll: collect_named_scroll_timelines(layout),
            named_view: collect_named_view_timelines(layout),
        };
        self.tick_box(timestamp_ms, layout, stylesheet, &ctx, &mut frame, &mut events, &mut visited);
        // Экземпляры, не встреченные этим обходом дерева, больше не описаны
        // текущим стилем (animation-name сменился/исчез, узел убран) — те из
        // них, что ещё не успели доиграть, отменены.
        let stale: Vec<AnimKey> = self
            .running
            .keys()
            .filter(|k| !visited.contains(k))
            .cloned()
            .collect();
        for key in stale {
            if let Some(state) = self.running.remove(&key)
                && !state.completed
            {
                events.push(AnimationEventInfo {
                    node: key.node,
                    animation_name: state.name,
                    kind: AnimationEventKind::Cancel,
                    elapsed_time: state.last_local_time_s.max(0.0) as f32,
                });
            }
        }
        (frame, events)
    }

    /// Удалить все записи для элементов, которых больше нет в дереве.
    /// Вызывать после load/reload, чтобы не накапливать мёртвые записи.
    pub fn clear(&mut self) {
        self.running.clear();
    }

    #[allow(clippy::too_many_arguments)] // внутренний рекурсивный обход, не публичный API
    fn tick_box(
        &mut self,
        ts: f64,
        lb: &LayoutBox,
        ss: &Stylesheet,
        ctx: &ScrollCtx,
        frame: &mut AnimationFrame,
        events: &mut Vec<AnimationEventInfo>,
        visited: &mut std::collections::HashSet<AnimKey>,
    ) {
        self.process_node(ts, lb, ss, ctx, frame, events, visited);
        for child in &lb.children {
            self.tick_box(ts, child, ss, ctx, frame, events, visited);
        }
    }

    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    #[allow(clippy::too_many_arguments)] // внутренний рекурсивный обход, не публичный API
    fn process_node(
        &mut self,
        ts: f64,
        lb: &LayoutBox,
        ss: &Stylesheet,
        ctx: &ScrollCtx,
        frame: &mut AnimationFrame,
        events: &mut Vec<AnimationEventInfo>,
        visited: &mut std::collections::HashSet<AnimKey>,
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
                    visited.insert(key.clone());

                    // Регистрируем новую анимацию — начало отсчёта = сейчас.
                    self.running.entry(key.clone()).or_insert(RunState {
                        start_ms: ts,
                        paused_at: None,
                        name: name.clone(),
                        started_fired: false,
                        iterations_completed: 0,
                        completed: false,
                        last_local_time_s: 0.0,
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
                    state.last_local_time_s = local_time_s;

                    // CSS Animations L1 §4.5.1 — события жизненного цикла.
                    // `animationstart` фиксирует момент входа в активный
                    // период (задержка прошла), один раз.
                    if local_time_s >= 0.0 && !state.started_fired {
                        state.started_fired = true;
                        events.push(AnimationEventInfo {
                            node: lb.node,
                            animation_name: name.clone(),
                            kind: AnimationEventKind::Start,
                            elapsed_time: 0.0,
                        });
                    }

                    let max_iters_f64: f64 = match &iter_count {
                        IterationCount::Infinite => f64::INFINITY,
                        IterationCount::Finite(n) => *n as f64,
                    };
                    let total_s = duration as f64 * max_iters_f64;
                    let finished = !total_s.is_infinite() && local_time_s >= total_s;

                    if !finished && local_time_s >= 0.0 && duration > 0.0 {
                        // `animationiteration` — за каждую пройденную итерацию,
                        // кроме последней (её завершение — уже `animationend`).
                        let current_iter = (local_time_s / duration as f64).floor() as u64;
                        if current_iter > state.iterations_completed {
                            for k in (state.iterations_completed + 1)..=current_iter {
                                events.push(AnimationEventInfo {
                                    node: lb.node,
                                    animation_name: name.clone(),
                                    kind: AnimationEventKind::Iteration,
                                    elapsed_time: k as f32 * duration,
                                });
                            }
                            state.iterations_completed = current_iter;
                        }
                    }

                    if finished && !state.completed {
                        state.completed = true;
                        events.push(AnimationEventInfo {
                            node: lb.node,
                            animation_name: name.clone(),
                            kind: AnimationEventKind::End,
                            elapsed_time: total_s as f32,
                        });
                    }

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
            if let Some(v) = animated.height {
                entry.height = Some(v);
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

    // GAP-CSSANIM срез 8: `height` was parsed by `parse_keyframe_style` (BUG-536
    // срез 5 added that) but never interpolated here — the срез 5 writeup
    // conflated this live scheduler with the unused twin in
    // `lumen_layout::animation::AnimationScheduler`, which does interpolate it
    // but is never instantiated (only `crate::animation_scheduler::AnimationScheduler`
    // is wired into `RedrawRequested`). `@keyframes height` therefore never
    // reached `getComputedStyle()` in practice.
    if let (Some(a), Some(b)) = (from.height.clone(), to.height.clone())
        && let Some(AnimValue::Length(v)) =
            interp.interpolate(&AnimValue::Length(a), &AnimValue::Length(b), t)
    {
        result.height = Some(v);
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
            used_line_height: 16.0 * 1.2,
            style: std::sync::Arc::new(ComputedStyle::root()),
            kind: BoxKind::Block,
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: Default::default(),
            origin: lumen_layout::BoxOrigin::default(),
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
        std::sync::Arc::make_mut(&mut container.style).scroll_timeline_name = Some("--page".to_string());
        std::sync::Arc::make_mut(&mut container.style).scroll_timeline_axis = ScrollAxis::Block;
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

    // ── AnimationScheduler::tick — GAP-CSSANIM срез 2 lifecycle events ───────

    fn make_animated_box(id: u32, name: &str, duration_s: f32, iterations: IterationCount) -> LayoutBox {
        let mut lb = make_box(id, 0.0, 0.0, 50.0, 50.0);
        let style = std::sync::Arc::make_mut(&mut lb.style);
        style.animation_names = vec![name.to_string()];
        style.animation_durations = vec![duration_s];
        style.animation_iteration_counts = vec![iterations];
        lb
    }

    fn fade_keyframes_sheet(name: &str) -> Stylesheet {
        lumen_css_parser::parse(&format!(
            "@keyframes {name} {{ from {{ opacity: 1; }} to {{ opacity: 0; }} }}"
        ))
    }

    fn tick_at(
        sched: &mut AnimationScheduler,
        ts: f64,
        root: &LayoutBox,
        sheet: &Stylesheet,
    ) -> Vec<AnimationEventInfo> {
        sched
            .tick(ts, root, sheet, 0.0, 0.0, Viewport { width: 1024.0, height: 720.0 })
            .1
    }

    #[test]
    fn tick_fires_start_on_first_active_frame() {
        let mut sched = AnimationScheduler::new();
        let root = make_animated_box(1, "fade", 1.0, IterationCount::Finite(1.0));
        let sheet = fade_keyframes_sheet("fade");
        let events = tick_at(&mut sched, 0.0, &root, &sheet);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AnimationEventKind::Start);
        assert_eq!(events[0].animation_name, "fade");

        // A second tick still inside the active period must not refire it.
        let events = tick_at(&mut sched, 100.0, &root, &sheet);
        assert!(events.is_empty(), "start must fire once, got {events:?}");
    }

    #[test]
    fn tick_fires_iteration_between_completed_loops() {
        let mut sched = AnimationScheduler::new();
        let root = make_animated_box(1, "fade", 1.0, IterationCount::Finite(3.0));
        let sheet = fade_keyframes_sheet("fade");
        tick_at(&mut sched, 0.0, &root, &sheet); // Start.
        // 1.5s in: one full iteration (0..1s) completed, second in progress.
        let events = tick_at(&mut sched, 1500.0, &root, &sheet);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AnimationEventKind::Iteration);
        assert!((events[0].elapsed_time - 1.0).abs() < 1e-4);
    }

    #[test]
    fn tick_does_not_fire_iteration_for_the_last_loop() {
        let mut sched = AnimationScheduler::new();
        let root = make_animated_box(1, "fade", 1.0, IterationCount::Finite(2.0));
        let sheet = fade_keyframes_sheet("fade");
        tick_at(&mut sched, 0.0, &root, &sheet); // Start.
        // 2.5s in: total duration is 2s — animation already finished by then,
        // so the boundary at 1s must have produced Iteration, not the End.
        let events = tick_at(&mut sched, 2500.0, &root, &sheet);
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![AnimationEventKind::End]);
    }

    #[test]
    fn tick_fires_end_once_on_completion() {
        let mut sched = AnimationScheduler::new();
        let root = make_animated_box(1, "fade", 1.0, IterationCount::Finite(1.0));
        let sheet = fade_keyframes_sheet("fade");
        tick_at(&mut sched, 0.0, &root, &sheet); // Start.
        let events = tick_at(&mut sched, 1500.0, &root, &sheet);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AnimationEventKind::End);
        assert!((events[0].elapsed_time - 1.0).abs() < 1e-4);

        // Re-ticking a finished (no fill-mode) animation must not refire End.
        let events = tick_at(&mut sched, 2000.0, &root, &sheet);
        assert!(events.is_empty(), "end must fire once, got {events:?}");
    }

    #[test]
    fn tick_fires_cancel_when_animation_disappears_before_completion() {
        let mut sched = AnimationScheduler::new();
        let root = make_animated_box(1, "fade", 2.0, IterationCount::Finite(1.0));
        let sheet = fade_keyframes_sheet("fade");
        tick_at(&mut sched, 0.0, &root, &sheet); // Start, still mid-animation.

        // Next frame the element no longer declares the animation (e.g.
        // `animation-name` changed or the node left the tree).
        let plain_root = make_box(1, 0.0, 0.0, 50.0, 50.0);
        let events = tick_at(&mut sched, 500.0, &plain_root, &sheet);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AnimationEventKind::Cancel);
        assert_eq!(events[0].animation_name, "fade");
    }

    #[test]
    fn tick_does_not_cancel_an_already_completed_animation() {
        let mut sched = AnimationScheduler::new();
        let root = make_animated_box(1, "fade", 1.0, IterationCount::Finite(1.0));
        let sheet = fade_keyframes_sheet("fade");
        tick_at(&mut sched, 0.0, &root, &sheet); // Start.
        let events = tick_at(&mut sched, 1500.0, &root, &sheet); // End.
        assert_eq!(events[0].kind, AnimationEventKind::End);

        // Element removed only after the animation had already finished —
        // no spurious Cancel for an instance that already ran to completion.
        let plain_root = make_box(1, 0.0, 0.0, 50.0, 50.0);
        let events = tick_at(&mut sched, 2000.0, &plain_root, &sheet);
        assert!(events.is_empty(), "no cancel after completion, got {events:?}");
    }

    // GAP-CSSANIM срез 8: `@keyframes height` must reach `AnimationFrame.overrides`
    // (and from there `getComputedStyle()`, via `to_computed_style_patches`) — the
    // срез 5 writeup claimed this already worked, but tested the wrong (unused)
    // `AnimationScheduler` twin; the one actually wired into `RedrawRequested`
    // never interpolated `height` until this slice.
    #[test]
    fn tick_interpolates_height_midpoint() {
        let mut sched = AnimationScheduler::new();
        let mut root = make_animated_box(1, "grow", 1.0, IterationCount::Finite(1.0));
        std::sync::Arc::make_mut(&mut root.style).animation_timing_functions =
            vec![TimingFunction::Linear];
        let sheet = lumen_css_parser::parse(
            "@keyframes grow { from { height: 0px; } to { height: 100px; } }",
        );
        sched.tick(0.0, &root, &sheet, 0.0, 0.0, Viewport { width: 1024.0, height: 720.0 }); // Start.
        let (frame, _) = sched.tick(
            500.0,
            &root,
            &sheet,
            0.0,
            0.0,
            Viewport { width: 1024.0, height: 720.0 },
        );
        let h = frame
            .overrides
            .get(&node(1))
            .and_then(|s| s.height.as_ref())
            .expect("height override at midpoint");
        match h {
            lumen_layout::style::Length::Px(px) => {
                assert!((px - 50.0).abs() < 0.1, "expected ~50px, got {px}");
            }
            other => panic!("expected Length::Px, got {other:?}"),
        }
    }
}

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
        AnimationDirection, AnimationFillMode, AnimationPlayState, IterationCount, TimingFunction,
    },
    LayoutBox,
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
    pub fn tick(
        &mut self,
        timestamp_ms: f64,
        layout: &LayoutBox,
        stylesheet: &Stylesheet,
    ) -> AnimationFrame {
        let mut frame = AnimationFrame::default();
        self.tick_box(timestamp_ms, layout, stylesheet, &mut frame);
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
        frame: &mut AnimationFrame,
    ) {
        self.process_node(ts, lb, ss, frame);
        for child in &lb.children {
            self.tick_box(ts, child, ss, frame);
        }
    }

    fn process_node(
        &mut self,
        ts: f64,
        lb: &LayoutBox,
        ss: &Stylesheet,
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

            // Параметры анимации из параллельных списков ComputedStyle (cyclic).
            let duration = get_cyclic(&style.animation_durations, i).copied().unwrap_or(0.0);
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
            let direction = get_cyclic(&style.animation_directions, i)
                .copied()
                .unwrap_or(AnimationDirection::Normal);
            let timing_fn = get_cyclic(&style.animation_timing_functions, i)
                .cloned()
                .unwrap_or_default();
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

            // Найти @keyframes по имени.
            let Some(kf_rule) = ss.keyframes.iter().find(|k| k.name == *name) else {
                continue;
            };

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
}

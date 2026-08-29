//! Типы значений CSS для тайминга и анимации: `TimingFunction` (включая
//! cubic-bezier и linear() разбор/прогресс), `step-position`,
//! `iteration-count`, направление/fill-mode/play-state анимации,
//! `animation-timeline`, CSS-wide keywords и `CustomProps` — общие custom
//! properties узла под `Arc` (copy-on-write).
//!
//! Перенесено батчем SPLIT-ST16 из `crates/engine/layout/src/style.rs`
//! (анкер `enum TimingFunction` до конца `impl FromIterator for CustomProps`)
//! без правок тел.

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use crate::scroll_timeline::ScrollAxis;
use crate::style::split_top_level_commas;

/// CSS Easing L1 §2 — easing function для CSS Transitions и CSS Animations.
/// Не наследуется (используется как per-list-entry значение в
/// transition/animation longhand-ах). Default по spec — `ease`, что
/// эквивалентно `cubic-bezier(0.25, 0.1, 0.25, 1.0)`.
///
/// P2 п.3B compositor offload и P1 п.3A Web Animations interpolation —
/// потребители этого AST: оба применяют функцию `progress(t) → [0, 1]`
/// к линейному времени `t ∈ [0, 1]` для получения eased progress.
#[derive(Debug, Clone, PartialEq)]
pub enum TimingFunction {
    /// `linear` ≡ `cubic-bezier(0, 0, 1, 1)`. progress(t) = t.
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`. Также покрывает keyword-shortcuts:
    /// `ease` ≡ (0.25, 0.1, 0.25, 1.0);
    /// `ease-in` ≡ (0.42, 0, 1, 1);
    /// `ease-out` ≡ (0, 0, 0.58, 1);
    /// `ease-in-out` ≡ (0.42, 0, 0.58, 1).
    /// x1, x2 ∈ [0, 1] (spec); y1, y2 — unbounded.
    CubicBezier(f32, f32, f32, f32),
    /// `steps(n, <step-position>)`. `step-start` ≡ `steps(1, jump-start)`,
    /// `step-end` ≡ `steps(1, jump-end)`. `n` — положительное целое;
    /// для `jump-none` ещё и ≥ 2.
    Steps(u32, StepPosition),
    /// `linear(<linear-stop-list>)` (CSS Easing L2 §2.4) — кусочно-линейная
    /// функция easing-а, задаваемая 2+ control-точками. Каждая точка:
    /// output (unitless number, может выходить за `[0, 1]`) и input
    /// (∈ `[0, 1]`, монотонно неубывает). Inputs нормализованы по правилам
    /// §2.5.1: пропущенные значения распределяются между соседними
    /// заданными; первая точка получает `0`, последняя — `1`.
    ///
    /// Discontinuity-кейсы (две точки с одинаковым input → вертикальный
    /// прыжок) допустимы и формируются из stop-а с двумя percentage-ами:
    /// `linear(0 0% 50%, 1 50% 100%)` ≡ step-функция со скачком на 0.5.
    ///
    /// `linear(0, 1)` поведенчески эквивалентно `Linear`; парсер хранит
    /// этот случай как `LinearStops`, без коллапса в `Linear`, чтобы
    /// сохранять round-trip.
    LinearStops(Vec<LinearEasingPoint>),
}

/// CSS Easing L2 §2.4 — одна control-точка функции `linear(...)`.
///
/// `output` — значение easing-а в этой точке (unitless, может выходить за
/// `[0, 1]` — overshoot допустим). `input` — соответствующая позиция на
/// time-axis в `[0, 1]`. После канонизации (§2.5.1) inputs всех точек
/// одного `LinearStops` монотонно неубывают.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearEasingPoint {
    /// Output progress в этой точке. Unitless. May exceed `[0, 1]`.
    pub output: f32,
    /// Input progress ∈ `[0, 1]` (доля времени анимации).
    pub input: f32,
}

impl Default for TimingFunction {
    fn default() -> Self {
        // CSS Transitions/Animations L1 — initial value = `ease`.
        TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
    }
}

impl TimingFunction {
    /// Парсит keyword (`linear` / `ease` / `ease-in` / `ease-out` /
    /// `ease-in-out` / `step-start` / `step-end`) или функцию
    /// (`cubic-bezier(...)` / `steps(...)`). Возвращает `None` для
    /// невалидного значения (out-of-range x, n=0, неизвестный keyword).
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        match t.as_str() {
            "linear" => return Some(Self::Linear),
            "ease" => return Some(Self::CubicBezier(0.25, 0.1, 0.25, 1.0)),
            "ease-in" => return Some(Self::CubicBezier(0.42, 0.0, 1.0, 1.0)),
            "ease-out" => return Some(Self::CubicBezier(0.0, 0.0, 0.58, 1.0)),
            "ease-in-out" => return Some(Self::CubicBezier(0.42, 0.0, 0.58, 1.0)),
            "step-start" => return Some(Self::Steps(1, StepPosition::JumpStart)),
            "step-end" => return Some(Self::Steps(1, StepPosition::JumpEnd)),
            _ => {}
        }
        if let Some(args) = t
            .strip_prefix("cubic-bezier(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            if parts.len() != 4 {
                return None;
            }
            let x1 = parts[0].parse::<f32>().ok()?;
            let y1 = parts[1].parse::<f32>().ok()?;
            let x2 = parts[2].parse::<f32>().ok()?;
            let y2 = parts[3].parse::<f32>().ok()?;
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                return None;
            }
            return Some(Self::CubicBezier(x1, y1, x2, y2));
        }
        if let Some(args) = t
            .strip_prefix("steps(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let parts: Vec<&str> = args.split(',').map(str::trim).collect();
            if parts.is_empty() || parts.len() > 2 {
                return None;
            }
            let n = parts[0].parse::<u32>().ok()?;
            if n == 0 {
                return None;
            }
            let pos = match parts.get(1).copied() {
                None => StepPosition::JumpEnd,
                Some("start") | Some("jump-start") => StepPosition::JumpStart,
                Some("end") | Some("jump-end") => StepPosition::JumpEnd,
                Some("jump-none") => {
                    if n < 2 {
                        return None;
                    }
                    StepPosition::JumpNone
                }
                Some("jump-both") => StepPosition::JumpBoth,
                _ => return None,
            };
            return Some(Self::Steps(n, pos));
        }
        if let Some(args) = t
            .strip_prefix("linear(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            return parse_linear_easing_stops(args).map(Self::LinearStops);
        }
        None
    }

    /// CSS Transitions/Animations L1 — comma-list of timing functions.
    /// Пустые / невалидные entry — пропускаются (best-effort lenient).
    pub fn parse_list(s: &str) -> Vec<TimingFunction> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(TimingFunction::parse)
            .collect()
    }

    /// CSS Easing L1 §2 — компьютация eased progress.
    ///
    /// Принимает линейный input ratio `t ∈ [0, 1]` (input progress по spec)
    /// и возвращает output progress в [0, 1] для `Linear` и `Steps`. Для
    /// `CubicBezier` выход может выходить за `[0, 1]` (overshoot — клиент
    /// либо clamp-ает при применении к Length/Color, либо использует напрямую
    /// — например для `transform`).
    ///
    /// Вне `[0, 1]` входное `t` clamp-ается, как требует §2: «If input
    /// progress is less than 0, return 0. If input progress is greater
    /// than 1, return 1.» (реальные `fill-mode` / `direction` обрабатываются
    /// в animation engine ДО вызова progress().)
    pub fn progress(&self, t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        match self {
            TimingFunction::Linear => x,
            TimingFunction::CubicBezier(x1, y1, x2, y2) => cubic_bezier_progress(*x1, *y1, *x2, *y2, x),
            TimingFunction::Steps(n, position) => steps_progress(*n, *position, x),
            TimingFunction::LinearStops(points) => linear_stops_progress(points, x),
        }
    }
}

/// CSS Easing L1 §2.3 — cubic bezier easing. Кривая определена двумя
/// контрольными точками `(x1, y1)`, `(x2, y2)` с эндпоинтами `(0, 0)`,
/// `(1, 1)`. По заданному `x` (== input progress) находим параметр `u`,
/// такой что `bezier_axis(u, x1, x2) = x`, и возвращаем
/// `bezier_axis(u, y1, y2)` — eased output.
///
/// Алгоритм: Newton-Raphson (быстрая сходимость в большинстве кейсов) с
/// bisection fallback на случай, когда производная около нуля или Newton
/// расходится. Стандартный подход в Blink/WebKit/Gecko.
fn cubic_bezier_progress(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let u = solve_bezier_x(x1, x2, x);
    bezier_axis(u, y1, y2)
}

/// `B(u) = 3(1-u)²·u·c1 + 3(1-u)·u²·c2 + u³` для P0=(0,0), P3=(1,1).
fn bezier_axis(u: f32, c1: f32, c2: f32) -> f32 {
    let omu = 1.0 - u;
    3.0 * omu * omu * u * c1 + 3.0 * omu * u * u * c2 + u * u * u
}

/// `B'(u) = 3(1-u)²·c1 + 6(1-u)·u·(c2-c1) + 3u²·(1-c2)`.
fn bezier_axis_derivative(u: f32, c1: f32, c2: f32) -> f32 {
    let omu = 1.0 - u;
    3.0 * omu * omu * c1 + 6.0 * omu * u * (c2 - c1) + 3.0 * u * u * (1.0 - c2)
}

/// Solve `bezier_axis(u, x1, x2) = x` for `u ∈ [0, 1]`.
fn solve_bezier_x(x1: f32, x2: f32, x: f32) -> f32 {
    const EPS: f32 = 1e-6;
    let mut u = x;
    for _ in 0..8 {
        let xu = bezier_axis(u, x1, x2);
        let err = xu - x;
        if err.abs() < EPS {
            return u.clamp(0.0, 1.0);
        }
        let d = bezier_axis_derivative(u, x1, x2);
        if d.abs() < EPS {
            break;
        }
        u -= err / d;
        if !u.is_finite() {
            break;
        }
    }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..64 {
        let mid = (lo + hi) * 0.5;
        let xu = bezier_axis(mid, x1, x2);
        if (xu - x).abs() < EPS || (hi - lo) < EPS {
            return mid;
        }
        if xu < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

/// CSS Easing L1 §3.2 — `steps(n, <step-position>)` easing.
///
/// step-position определяет, сколько output-уровней и где «прыжки»:
/// - `jump-start` / `start`: n уровней `1/n, 2/n, ..., n/n`. Прыжок при t=0.
/// - `jump-end` / `end` (default): n+1 уровень `0/n, 1/n, ..., n/n`. Прыжок при t=1.
/// - `jump-none`: n уровней `0/(n-1), ..., (n-1)/(n-1) = 1`. Прыжков на границах нет.
/// - `jump-both`: n+2 уровня `1/(n+1), 2/(n+1), ..., (n+1)/(n+1) = 1`. Прыжки на обеих границах.
///
/// Для `t = 0` и `t = 1` корректно clamp-ается до границы output-диапазона.
fn steps_progress(n: u32, position: StepPosition, t: f32) -> f32 {
    let n_f = n as f32;
    let (raw_index, divisor, max_step) = match position {
        StepPosition::JumpStart => ((t * n_f).floor() + 1.0, n_f, n_f),
        StepPosition::JumpEnd => ((t * n_f).floor(), n_f, n_f),
        StepPosition::JumpNone => ((t * n_f).floor(), n_f - 1.0, n_f - 1.0),
        StepPosition::JumpBoth => ((t * n_f).floor() + 1.0, n_f + 1.0, n_f + 1.0),
    };
    let step = raw_index.max(0.0).min(max_step);
    (step / divisor).clamp(0.0, 1.0)
}

/// CSS Easing L2 §2.5.1 — канонизация stop-листа `linear(...)`.
///
/// Принимает содержимое скобок (без `linear(` / `)`); ожидает 2+ stop-а,
/// разделённых запятыми. Каждый stop = `<number>` + 0..2 `<percentage>`.
/// Возвращает `None` при синтаксической ошибке или < 2 stop-ов.
///
/// Алгоритм (§2.5.1):
/// 1. Парсим raw stops, преобразуем percentages → доли в `[0, 1]`.
/// 2. Расширяем stops с двумя lengths в две точки с одинаковым output.
/// 3. Первый stop без length получает input = 0, последний — max(1, largest).
/// 4. Каждый явный input clamp-ается до текущего `largest_input` (монотонность).
/// 5. Пропуски (точки без input) распределяются равномерно между соседними
///    известными inputs.
fn parse_linear_easing_stops(args: &str) -> Option<Vec<LinearEasingPoint>> {
    let parts = split_top_level_commas(args);
    if parts.len() < 2 {
        return None;
    }

    // Raw: (output, optional percentages already normalised to [0, 1]).
    let mut raw: Vec<(f32, Vec<f32>)> = Vec::with_capacity(parts.len());
    for stop in &parts {
        let stop = stop.trim();
        if stop.is_empty() {
            return None;
        }
        let tokens: Vec<&str> = stop.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > 3 {
            return None;
        }
        let output = tokens[0].parse::<f32>().ok()?;
        if !output.is_finite() {
            return None;
        }
        let mut lengths: Vec<f32> = Vec::new();
        for tok in &tokens[1..] {
            let stripped = tok.strip_suffix('%')?;
            let pct = stripped.parse::<f32>().ok()?;
            if !pct.is_finite() {
                return None;
            }
            lengths.push(pct / 100.0);
        }
        raw.push((output, lengths));
    }

    // Step 1 + 3 + 4: build points list with optional inputs and clamp by
    // largest_input для монотонности (spec: «whichever is greater»).
    let last_idx = raw.len() - 1;
    let mut points: Vec<(f32, Option<f32>)> = Vec::new();
    let mut largest = f32::NEG_INFINITY;
    for (i, (output, lengths)) in raw.iter().enumerate() {
        if lengths.is_empty() {
            if i == 0 {
                points.push((*output, Some(0.0)));
                largest = 0.0;
            } else if i == last_idx {
                let v = 1.0_f32.max(largest);
                points.push((*output, Some(v)));
                largest = v;
            } else {
                points.push((*output, None));
            }
        } else {
            let first_len = lengths[0].max(largest);
            points.push((*output, Some(first_len)));
            largest = first_len;
            if lengths.len() == 2 {
                let second_len = lengths[1].max(largest);
                points.push((*output, Some(second_len)));
                largest = second_len;
            }
        }
    }

    // Step 5: distribute `None` runs evenly between surrounding known inputs.
    // По §2.5.1 первая и последняя точки гарантированно получают input
    // в шагах 3-4, поэтому None-run всегда окружён двумя Some-границами.
    let mut i = 0;
    while i < points.len() {
        if points[i].1.is_some() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < points.len() && points[j].1.is_none() {
            j += 1;
        }
        // i..j — диапазон None; prev и next — соседние Some.
        let prev = points[i - 1].1?;
        let next = points[j].1?;
        let span = next - prev;
        let count = (j - i) as f32 + 1.0;
        for (k, idx) in (i..j).enumerate() {
            let frac = (k as f32 + 1.0) / count;
            points[idx].1 = Some(prev + frac * span);
        }
        i = j;
    }

    Some(
        points
            .into_iter()
            .map(|(output, input)| LinearEasingPoint {
                output,
                input: input.unwrap_or(0.0),
            })
            .collect(),
    )
}

/// CSS Easing L2 §2.5.2 — вычисление output функции `linear(...)`.
///
/// `points` — канонизованный список из `parse_linear_easing_stops` (inputs
/// монотонно неубывают). `t ∈ [0, 1]` — input progress (уже clamp-нутый
/// вызывающим `progress()`). Алгоритм:
///
/// - Меньше первого input — возвращаем output первой точки.
/// - Больше-или-равно последнему input — output последней (включая
///   `t == 1.0` ровно).
/// - Иначе ищем первую пару соседних точек `[A, B]` такую, что
///   `A.input ≤ t < B.input`, и линейно интерполируем. Discontinuity
///   (одинаковые inputs у соседних точек) обрабатывается возвратом
///   output левой точки — пара выбирается по first-match, поэтому
///   при `t == A.input` мы попадём на левую сторону скачка.
fn linear_stops_progress(points: &[LinearEasingPoint], t: f32) -> f32 {
    match points.len() {
        0 => t,
        1 => points[0].output,
        _ => {
            let first = points[0];
            let last = points[points.len() - 1];
            if t < first.input {
                return first.output;
            }
            if t >= last.input {
                return last.output;
            }
            for w in points.windows(2) {
                let a = w[0];
                let b = w[1];
                if a.input <= t && t < b.input {
                    let span = b.input - a.input;
                    if span <= f32::EPSILON {
                        return a.output;
                    }
                    let local = (t - a.input) / span;
                    return a.output + local * (b.output - a.output);
                }
            }
            last.output
        }
    }
}

/// CSS Easing L1 §3 — позиция шага в `steps()`. Default по spec — `jump-end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepPosition {
    /// `jump-start` (alias `start`) — первый прыжок на t=0,
    /// последний шаг достигает 1 - 1/n.
    JumpStart,
    /// `jump-end` (alias `end`) — первый шаг на t > 0, последний прыжок
    /// на t=1. Default.
    #[default]
    JumpEnd,
    /// `jump-none` — `n` шагов, ни один на границе. Требует n ≥ 2.
    JumpNone,
    /// `jump-both` — n+1 шагов, оба на границах t=0 и t=1.
    JumpBoth,
}

/// CSS Animations L1 §3.5 — `animation-iteration-count`. Либо число
/// (может быть дробным; отрицательные значения трактуются как невалидные),
/// либо ключевое слово `infinite`. Default = `Finite(1.0)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IterationCount {
    Finite(f32),
    Infinite,
}

impl Default for IterationCount {
    fn default() -> Self {
        IterationCount::Finite(1.0)
    }
}

impl IterationCount {
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("infinite") {
            return Some(Self::Infinite);
        }
        let n = t.parse::<f32>().ok()?;
        if n.is_finite() && n >= 0.0 {
            Some(Self::Finite(n))
        } else {
            None
        }
    }

    pub fn parse_list(s: &str) -> Vec<IterationCount> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(IterationCount::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.6 — `animation-direction`. Default = `Normal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationDirection {
    /// Прямое воспроизведение каждой итерации (0 → 100%).
    #[default]
    Normal,
    /// Обратное воспроизведение (100% → 0).
    Reverse,
    /// Чётные итерации normal, нечётные reverse.
    Alternate,
    /// Чётные reverse, нечётные normal.
    AlternateReverse,
}

impl AnimationDirection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "reverse" => Some(Self::Reverse),
            "alternate" => Some(Self::Alternate),
            "alternate-reverse" => Some(Self::AlternateReverse),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationDirection> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationDirection::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.7 — `animation-fill-mode`. Default = `None`.
/// Определяет, применяются ли значения keyframes до начала и/или после
/// окончания анимации.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationFillMode {
    /// До начала и после конца — используется computed-style без keyframes.
    #[default]
    None,
    /// После окончания — последняя keyframe сохраняется.
    Forwards,
    /// До начала — первая keyframe применяется.
    Backwards,
    /// Both `forwards` и `backwards` одновременно.
    Both,
}

impl AnimationFillMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "forwards" => Some(Self::Forwards),
            "backwards" => Some(Self::Backwards),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationFillMode> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationFillMode::parse)
            .collect()
    }
}

/// CSS Animations L1 §3.8 — `animation-play-state`. Default = `Running`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnimationPlayState {
    /// Анимация идёт. Default.
    #[default]
    Running,
    /// Пауза — текущее значение фиксируется.
    Paused,
}

impl AnimationPlayState {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            _ => None,
        }
    }

    pub fn parse_list(s: &str) -> Vec<AnimationPlayState> {
        split_top_level_commas(s)
            .into_iter()
            .filter_map(AnimationPlayState::parse)
            .collect()
    }
}

/// CSS Scroll-Driven Animations L1 §3.3 — `animation-timeline` CSS value.
///
/// Parsed from `animation-timeline: auto | scroll([axis] [scroller]) | view([axis]) | <custom-ident>`.
/// Stored per-animation parallel to `animation_names`. Resolution to a concrete
/// `ScrollTimeline` / `ViewTimeline` happens at runtime in the animation scheduler.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum AnimationTimeline {
    /// Default: time-driven animation (normal `@keyframes` clock).
    #[default]
    Auto,
    /// `scroll([<axis>] [nearest | root | self])` — scroll container progress.
    /// `nearest: true` = nearest scroll ancestor (default); `false` = root viewport.
    Scroll { axis: ScrollAxis, nearest: bool },
    /// `view([<axis>])` — element visibility in scroll container (cover range).
    View { axis: ScrollAxis },
    /// `<custom-ident>` — matched against `scroll-timeline-name` / `view-timeline-name`
    /// at runtime.
    Named(String),
}

/// CSS-wide keywords (CSS Cascade L4 §7) — применимы к любому свойству.
/// - `Inherit` — взять computed value родителя.
/// - `Initial` — взять initial value свойства из спецификации.
/// - `Unset` — для inherited-свойств = `Inherit`, для non-inherited = `Initial`.
/// - `Revert` — откатиться к значению, которое было бы у свойства без
///   author/user-правил, то есть к UA-стилю для этого элемента (User origin
///   в Lumen не выделен отдельно от UA). Источник — снэпшот `ComputedStyle`,
///   снятый в `compute_style` сразу после `ua_*`/`apply_ua_*`/presentational-hint
///   пассов и до применения matched-деклараций (`ua_baseline`). Если у
///   свойства нет UA-хинта, снэпшот совпадает с обычным inherited/initial —
///   тогда `Revert` ведёт себя как `Unset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssWideKeyword {
    Inherit,
    Initial,
    Unset,
    Revert,
}

/// ASCII case-insensitive проверка значения декларации на CSS-wide keyword.
/// Любое из четырёх ключевых слов в любом регистре, с trim-ом whitespace,
/// возвращает соответствующий `Some(...)`. Иначе — `None`.
pub fn parse_css_wide_keyword(value: &str) -> Option<CssWideKeyword> {
    let t = value.trim();
    if t.eq_ignore_ascii_case("inherit") {
        Some(CssWideKeyword::Inherit)
    } else if t.eq_ignore_ascii_case("initial") {
        Some(CssWideKeyword::Initial)
    } else if t.eq_ignore_ascii_case("unset") {
        Some(CssWideKeyword::Unset)
    } else if t.eq_ignore_ascii_case("revert") {
        Some(CssWideKeyword::Revert)
    } else {
        None
    }
}

/// Copy-on-write map of a node's CSS custom properties (`--name` → raw source
/// text), as carried by [`ComputedStyle::custom_props`].
///
/// **Why a dedicated type instead of a plain `HashMap`** (BUG-341 S9): CSS
/// Variables L1 makes *every* custom property inherited, so `compute_style`
/// copies the parent's whole map into each child. With the 30 custom properties
/// `assets/chrome/chrome.html` declares, that copy alone measured 3.7–4.7 µs per
/// node against 0.31–0.46 µs for a node with an empty map — i.e. the map, not
/// the 302 other `ComputedStyle` fields, dominated the cascade. Behind an
/// [`Arc`] the inherit step is a refcount bump, and only the handful of nodes
/// that actually declare a `--name` pay a real copy, through
/// [`make_mut`](Self::make_mut).
///
/// The same sharing makes [`PartialEq`] cheap: two styles that inherited their
/// properties from a common ancestor compare in one pointer comparison, which is
/// what `graft_geometry`'s per-box style comparison relies on. The fast path is
/// spelled out here rather than left to `Arc`'s own (unspecified) pointer
/// short-circuit, so the cost is a property of this type and not of a standard
/// library implementation detail.
///
/// Reads go through [`Deref`] to `HashMap`, so `.get`/`.contains_key`/`.values`
/// and `&props` where a `&HashMap` is expected all work unchanged.
#[derive(Debug, Clone)]
pub struct CustomProps(Arc<HashMap<String, String>>);

impl CustomProps {
    /// Returns a mutable reference to the underlying map, cloning it first if
    /// (and only if) another `ComputedStyle` still shares it — the copy-on-write
    /// half of this type. Call sites that only read must not use this: an
    /// unconditional `make_mut` on every node would reintroduce exactly the
    /// per-node clone this type exists to remove.
    pub fn make_mut(&mut self) -> &mut HashMap<String, String> {
        Arc::make_mut(&mut self.0)
    }

    /// True when both sides are the very same allocation, i.e. one was cloned
    /// from the other with no intervening write. Equal-but-unshared maps return
    /// `false` — this is an identity check, not equality.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Address of the shared map, for callers that memoise per unique
    /// allocation rather than per node — see `collect_custom_properties`,
    /// which resolves each distinct map's `var()` chains exactly once and
    /// hands every node inheriting it the same `Arc`. Never dereferenced:
    /// the pointer is a map-identity key, nothing more.
    pub fn as_ptr(&self) -> *const HashMap<String, String> {
        Arc::as_ptr(&self.0)
    }

    /// The shared map itself, cloned as an `Arc` (a refcount bump, not a copy).
    /// Lets an embedder publish one allocation for every node that inherits it.
    pub fn shared(&self) -> Arc<HashMap<String, String>> {
        Arc::clone(&self.0)
    }
}

impl Default for CustomProps {
    /// The empty map is a process-wide singleton, so every node in a document
    /// that declares no custom property at all shares one allocation and
    /// compares by pointer.
    fn default() -> Self {
        static EMPTY: OnceLock<Arc<HashMap<String, String>>> = OnceLock::new();
        Self(Arc::clone(EMPTY.get_or_init(|| Arc::new(HashMap::new()))))
    }
}

impl Deref for CustomProps {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &HashMap<String, String> {
        &self.0
    }
}

impl PartialEq for CustomProps {
    /// Pointer identity first (the overwhelmingly common case for inherited
    /// maps), full map comparison only for independently built maps.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Eq for CustomProps {}

impl From<HashMap<String, String>> for CustomProps {
    fn from(map: HashMap<String, String>) -> Self {
        Self(Arc::new(map))
    }
}

impl FromIterator<(String, String)> for CustomProps {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self(Arc::new(iter.into_iter().collect()))
    }
}


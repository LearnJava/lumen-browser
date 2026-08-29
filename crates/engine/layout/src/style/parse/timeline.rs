//! Разбор таймлайнов и шортхендов анимации: `scroll()`/`view()` и оси
//! (CSS Scroll Animations L1 §2–§4), шортхенды `animation` и `transition`
//! (CSS Animations L1 §4.9, CSS Transitions L1 §2.5) с их лексером слоёв.
//!
//! Перенесено батчем SPLIT-ST5 из `crates/engine/layout/src/style.rs`
//! (анкеры `fn parse_scroll_axis` … `fn parse_time_seconds`) без правок тел: изменены только пути модулей и
//! видимость тех items, которые продолжают звать `style.rs`, его тест-модули
//! и соседние модули `style::parse`.

use crate::scroll_timeline::ScrollAxis;
use crate::style::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, AnimationTimeline, ComputedStyle,
    IterationCount, TimingFunction, split_top_level_commas,
};

/// Parse `scroll-timeline-axis` / `view-timeline-axis` keyword.
pub(in crate::style) fn parse_scroll_axis(s: &str) -> Option<ScrollAxis> {
    match s.to_ascii_lowercase().as_str() {
        "block" => Some(ScrollAxis::Block),
        "inline" => Some(ScrollAxis::Inline),
        "x" => Some(ScrollAxis::X),
        "y" => Some(ScrollAxis::Y),
        _ => None,
    }
}

/// Parse `scroll()` function: `scroll([<axis>] [nearest | root | self])`.
/// Returns `AnimationTimeline::Scroll { axis, nearest }`.
fn parse_scroll_fn(s: &str) -> AnimationTimeline {
    let inner = s
        .trim_start_matches("scroll(")
        .trim_end_matches(')')
        .trim();
    let mut axis = ScrollAxis::Block;
    let mut nearest = true;
    for token in inner.split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "block" => axis = ScrollAxis::Block,
            "inline" => axis = ScrollAxis::Inline,
            "x" => axis = ScrollAxis::X,
            "y" => axis = ScrollAxis::Y,
            "root" => nearest = false,
            "nearest" | "self" => nearest = true,
            _ => {}
        }
    }
    AnimationTimeline::Scroll { axis, nearest }
}

/// Parse `view()` function: `view([<axis>])`.
fn parse_view_fn(s: &str) -> AnimationTimeline {
    let inner = s
        .trim_start_matches("view(")
        .trim_end_matches(')')
        .trim();
    let axis = parse_scroll_axis(inner).unwrap_or(ScrollAxis::Block);
    AnimationTimeline::View { axis }
}

/// Parse comma-separated `animation-timeline` list.
pub(in crate::style) fn parse_animation_timeline_list(val: &str) -> Vec<AnimationTimeline> {
    split_top_level_commas(val)
        .into_iter()
        .map(|item| {
            let t = item.trim();
            let lower = t.to_ascii_lowercase();
            if lower == "auto" || lower == "none" {
                AnimationTimeline::Auto
            } else if lower.starts_with("scroll(") || lower == "scroll()" {
                parse_scroll_fn(t)
            } else if lower.starts_with("view(") || lower == "view()" {
                parse_view_fn(t)
            } else {
                AnimationTimeline::Named(t.to_string())
            }
        })
        .collect()
}

/// CSS Scroll-Driven Animations — `scroll-timeline` shorthand.
/// Syntax: `<custom-ident> [<axis>]` (resets both name and axis).
pub(in crate::style) fn apply_scroll_timeline_shorthand(style: &mut ComputedStyle, val: &str) {
    let trimmed = val.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        style.scroll_timeline_name = None;
        style.scroll_timeline_axis = ScrollAxis::Block;
        return;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("").trim();
    let axis_str = parts.next().unwrap_or("").trim();
    style.scroll_timeline_name = if name.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(name.to_string())
    };
    style.scroll_timeline_axis =
        parse_scroll_axis(axis_str).unwrap_or(ScrollAxis::Block);
}

/// CSS Scroll-Driven Animations — `view-timeline` shorthand.
/// Syntax: `<custom-ident> [<axis>]` (resets both name and axis).
pub(in crate::style) fn apply_view_timeline_shorthand(style: &mut ComputedStyle, val: &str) {
    let trimmed = val.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        style.view_timeline_name = None;
        style.view_timeline_axis = ScrollAxis::Block;
        return;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("").trim();
    let axis_str = parts.next().unwrap_or("").trim();
    style.view_timeline_name = if name.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(name.to_string())
    };
    style.view_timeline_axis =
        parse_scroll_axis(axis_str).unwrap_or(ScrollAxis::Block);
}

/// CSS Animations L1 §4 — `animation` shorthand.
///
/// Синтаксис: `animation = <single-animation>#`, где
///
/// ```text
/// <single-animation> = <time> || <easing-function> || <time>
///                   || <single-animation-iteration-count>
///                   || <single-animation-direction>
///                   || <single-animation-fill-mode>
///                   || <single-animation-play-state>
///                   || [ none | <keyframes-name> ]
/// ```
///
/// Оператор `||` (CSS Values §1.3.4) разрешает любому subset-у этих 8
/// «слотов» появляться в любом порядке. Первое подходящее `<time>` —
/// duration, второе — delay. Любой identifier-токен, не подходящий ни
/// под один keyword-slot, считается keyframes-name.
///
/// Поведение по spec semantics:
/// - Shorthand сбрасывает ВСЕ 8 longhand Vec-ов: каждый layer (= одна
///   позиция в comma-list) даёт строго одну запись в каждый из 8 Vec-ов;
///   un-set значения — initial-value (`""` для name, `0.0s` для time-ов,
///   `Default::default()` для остальных).
/// - Один токен в позиции, где slot уже занят, — fall-through к
///   следующему slot-у; если ни один не подошёл, токен трактуется как
///   keyframes-name.
/// - `none` без других именных кандидатов → `animation-fill-mode: none`
///   (он валиден без других конфликтов). Это компромисс per Blink/WebKit:
///   результат `animation: none` — пустое имя у этого layer-а →
///   эффективно отсутствие анимации.
pub(in crate::style) fn apply_animation_shorthand(style: &mut ComputedStyle, val: &str) {
    let mut names: Vec<String> = Vec::new();
    let mut durations: Vec<f32> = Vec::new();
    let mut timings: Vec<TimingFunction> = Vec::new();
    let mut delays: Vec<f32> = Vec::new();
    let mut iters: Vec<IterationCount> = Vec::new();
    let mut dirs: Vec<AnimationDirection> = Vec::new();
    let mut fills: Vec<AnimationFillMode> = Vec::new();
    let mut plays: Vec<AnimationPlayState> = Vec::new();

    for layer in split_top_level_commas(val) {
        let layer = layer.trim();
        if layer.is_empty() {
            continue;
        }
        let parsed = parse_single_animation(layer);
        names.push(parsed.name);
        durations.push(parsed.duration);
        timings.push(parsed.timing);
        delays.push(parsed.delay);
        iters.push(parsed.iter);
        dirs.push(parsed.direction);
        fills.push(parsed.fill);
        plays.push(parsed.play_state);
    }

    style.animation_names = names;
    style.animation_durations = durations;
    style.animation_timing_functions = timings;
    style.animation_delays = delays;
    style.animation_iteration_counts = iters;
    style.animation_directions = dirs;
    style.animation_fill_modes = fills;
    style.animation_play_states = plays;
}

/// Результат парсинга одного `<single-animation>` для shorthand. Все
/// поля заполнены: либо явное значение из CSS, либо initial-value.
/// Это обеспечивает совпадение длин всех 8 longhand Vec-ов после
/// shorthand-разворота (см. [`apply_animation_shorthand`]).
struct SingleAnimation {
    name: String,
    duration: f32,
    timing: TimingFunction,
    delay: f32,
    iter: IterationCount,
    direction: AnimationDirection,
    fill: AnimationFillMode,
    play_state: AnimationPlayState,
}

impl Default for SingleAnimation {
    fn default() -> Self {
        Self {
            name: String::new(),
            duration: 0.0,
            timing: TimingFunction::default(),
            delay: 0.0,
            iter: IterationCount::default(),
            direction: AnimationDirection::default(),
            fill: AnimationFillMode::default(),
            play_state: AnimationPlayState::default(),
        }
    }
}

/// Парсит одну `<single-animation>`-секцию: токенизация с учётом круглых
/// скобок (cubic-bezier / steps содержат запятые и пробелы), classify по
/// первому подходящему slot-у, fall-through к следующему при коллизии,
/// последний кандидат — keyframes-name.
fn parse_single_animation(s: &str) -> SingleAnimation {
    let mut out = SingleAnimation::default();
    let mut duration_set = false;
    let mut delay_set = false;
    let mut timing_set = false;
    let mut iter_set = false;
    let mut direction_set = false;
    let mut fill_set = false;
    let mut play_set = false;
    let mut name_set = false;

    for tok in tokenize_with_parens(s) {
        // 1) <time>: первое → duration, второе → delay. Per spec ordering.
        if let Some(t) = parse_time_seconds(&tok) {
            if !duration_set {
                out.duration = t;
                duration_set = true;
                continue;
            }
            if !delay_set {
                out.delay = t;
                delay_set = true;
                continue;
            }
            // Третье «<time>» некуда положить — игнорируем (spec: invalid).
            continue;
        }
        // 2) <easing-function>: keyword / cubic-bezier(...) / steps(...).
        if !timing_set
            && let Some(tf) = TimingFunction::parse(&tok)
        {
            out.timing = tf;
            timing_set = true;
            continue;
        }
        // 3) <iteration-count>: `infinite` или unitless f32 ≥ 0.
        if !iter_set
            && let Some(ic) = IterationCount::parse(&tok)
        {
            out.iter = ic;
            iter_set = true;
            continue;
        }
        // 4) <direction>.
        if !direction_set
            && let Some(d) = AnimationDirection::parse(&tok)
        {
            out.direction = d;
            direction_set = true;
            continue;
        }
        // 5) <fill-mode>. `none` совпадает здесь и используется ДО name —
        // совпадает с поведением Blink/WebKit/Gecko.
        if !fill_set
            && let Some(fm) = AnimationFillMode::parse(&tok)
        {
            out.fill = fm;
            fill_set = true;
            continue;
        }
        // 6) <play-state>.
        if !play_set
            && let Some(ps) = AnimationPlayState::parse(&tok)
        {
            out.play_state = ps;
            play_set = true;
            continue;
        }
        // 7) keyframes-name: любой токен, не подошедший выше. Только
        // первый кандидат остаётся; последующие игнорируются (spec:
        // дубликат недопустим, два keyframes-name делают объявление
        // invalid; lenient — пропускаем).
        if !name_set && !tok.is_empty() {
            out.name = tok;
            name_set = true;
        }
    }
    out
}

/// CSS Transitions L1 §3 — `transition` shorthand.
///
/// Синтаксис: `transition = <single-transition>#`, где
///
/// ```text
/// <single-transition> = [ none | <single-transition-property> ]
///                    || <time> || <easing-function> || <time>
/// ```
///
/// Слоты per layer (порядок в `||` произвольный):
/// - 2 × `<time>`: первый — duration, второй — delay.
/// - `<easing-function>`: timing function (linear / ease / cubic-bezier(…)
///   / steps(…) / step-start / step-end).
/// - property: `none` или CSS-ident (любое property name, плюс keyword
///   `all`). Default = `all`.
///
/// Shorthand сбрасывает все 4 longhand Vec-а; каждый layer (одна позиция
/// в comma-list) кладёт строго одну запись в каждый Vec. Un-set значения
/// → initial-value (duration/delay = 0s, timing = ease, property = "all").
///
/// `none` в позиции property сохраняется как литеральная строка `"none"`
/// — consumer (transition scheduler) skip-нет такие layers. Это отличается
/// от longhand-парсинга `transition-property: none` (там → пустой Vec),
/// что даёт parallel-length-инвариант после shorthand-развёртки.
pub(in crate::style) fn apply_transition_shorthand(style: &mut ComputedStyle, val: &str) {
    let mut props: Vec<String> = Vec::new();
    let mut durations: Vec<f32> = Vec::new();
    let mut timings: Vec<TimingFunction> = Vec::new();
    let mut delays: Vec<f32> = Vec::new();

    for layer in split_top_level_commas(val) {
        let layer = layer.trim();
        if layer.is_empty() {
            continue;
        }
        let parsed = parse_single_transition(layer);
        props.push(parsed.property);
        durations.push(parsed.duration);
        timings.push(parsed.timing);
        delays.push(parsed.delay);
    }

    style.transition_properties = props;
    style.transition_durations = durations;
    style.transition_timing_functions = timings;
    style.transition_delays = delays;
}

/// Результат парсинга одного `<single-transition>` слоя. Все 4 поля
/// заполнены: либо явное значение из CSS, либо initial-value.
struct SingleTransition {
    property: String,
    duration: f32,
    timing: TimingFunction,
    delay: f32,
}

impl Default for SingleTransition {
    fn default() -> Self {
        Self {
            property: "all".to_string(),
            duration: 0.0,
            timing: TimingFunction::default(),
            delay: 0.0,
        }
    }
}

/// Парсит одну `<single-transition>`-секцию. Tokenize-with-parens →
/// classify по первому подходящему slot-у. Property — последний
/// fallback (любой ident, не подошедший под time / easing).
fn parse_single_transition(s: &str) -> SingleTransition {
    let mut out = SingleTransition::default();
    let mut duration_set = false;
    let mut delay_set = false;
    let mut timing_set = false;
    let mut property_set = false;

    for tok in tokenize_with_parens(s) {
        if let Some(t) = parse_time_seconds(&tok) {
            if !duration_set {
                out.duration = t;
                duration_set = true;
                continue;
            }
            if !delay_set {
                out.delay = t;
                delay_set = true;
                continue;
            }
            continue;
        }
        if !timing_set
            && let Some(tf) = TimingFunction::parse(&tok)
        {
            out.timing = tf;
            timing_set = true;
            continue;
        }
        if !property_set && !tok.is_empty() {
            out.property = tok;
            property_set = true;
        }
    }
    out
}

/// Whitespace-разделение `<single-animation>`-слоя с уважением к
/// круглым скобкам (`cubic-bezier(0.42, 0, 0.58, 1)` — один токен,
/// несмотря на запятые и пробелы внутри).
pub(in crate::style) fn tokenize_with_parens(s: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                buf.push(c);
            }
            ')' => {
                depth -= 1;
                buf.push(c);
            }
            ws if ws.is_whitespace() && depth == 0 => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        tokens.push(buf);
    }
    tokens
}

/// CSS Values L4 §8 — список `<time>` значений через запятую.
/// Возвращает Vec секунд (ms → /1000, s → as-is).
pub(in crate::style) fn parse_time_list(s: &str) -> Vec<f32> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter_map(parse_time_seconds)
        .collect()
}

fn parse_time_seconds(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("ms") {
        return num.trim().parse::<f32>().ok().map(|v| v / 1000.0);
    }
    if let Some(num) = s.strip_suffix('s') {
        return num.trim().parse::<f32>().ok();
    }
    None
}

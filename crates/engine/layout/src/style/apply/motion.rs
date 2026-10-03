//! Трансформации, анимации и прокрутка — ветки `match prop` функции `apply_declaration`.
//!
//! `transform` и его окружение, `transition-*`/`animation-*` и таймлайны,
//! motion path (`offset-*`), scroll-snap, `scroll-margin`/`scroll-padding`,
//! `overscroll-behavior`, `touch-action`.
//!
//! Перенесено батчем SPLIT-ST8 из `crates/engine/layout/src/style.rs`: тела
//! веток скопированы побайтово, изменены только пути импортов и форма выхода
//! (`return` → `return true`, см. шапку `style/apply.rs`). Метка ветки в
//! группу не входит по алфавиту, а по смыслу — порядок веток внутри `match`
//! семантики не несёт, потому что все метки уникальны.

use crate::style::{
    AnimationDirection,
    AnimationFillMode, TransitionBehavior,
    AnimationPlayState,
    BackfaceVisibility,
    ComputedStyle,
    IterationCount,
    Length,
    ObjectPosition,
    OffsetRotate,
    PositionComponent,
    ScrollBehavior,
    ScrollInitialTarget,
    ScrollSnapStop,
    TimingFunction,
    TouchAction,
    TransformStyle,
    expand_grouped_transition_property,
    parse_length_q,
    parse_position_component,
    split_top_level_commas,
};
use crate::style::parse::box_sides::{
    expand_4_sides,
    parse_overscroll_behavior,
    parse_scroll_snap_align,
    parse_scroll_snap_type,
    resolve_box_length,
};
use crate::style::parse::timeline::{
    apply_animation_shorthand,
    apply_scroll_timeline_shorthand,
    apply_transition_shorthand,
    apply_view_timeline_shorthand,
    parse_animation_timeline_list,
    parse_scroll_axis,
    parse_interest_delay,
    parse_time_list,
};
use crate::style::parse::transform::{parse_angle_to_radians, parse_length_px, parse_transform_list};
use lumen_core::geom::Size;

/// Разбор `offset-rotate`: `auto | reverse | <angle> | [auto | reverse] <angle>`
/// (в любом порядке токенов). Угол — в градусах; `reverse <angle>` ≡ `auto`
/// с добавкой `angle + 180deg`. Невалидное значение → `None` (декларация
/// игнорируется).
fn parse_offset_rotate(val: &str) -> Option<OffsetRotate> {
    let mut mode: Option<bool> = None; // Some(false) = auto, Some(true) = reverse
    let mut angle: Option<f32> = None;
    let mut tokens = 0;
    for t in val.split_whitespace() {
        tokens += 1;
        if t.eq_ignore_ascii_case("auto") && mode.is_none() {
            mode = Some(false);
        } else if t.eq_ignore_ascii_case("reverse") && mode.is_none() {
            mode = Some(true);
        } else if angle.is_none() && let Some(rad) = parse_angle_to_radians(t) {
            angle = Some(rad.to_degrees());
        } else {
            return None;
        }
    }
    match (tokens, mode, angle) {
        (1..=2, Some(false), None) => Some(OffsetRotate::Auto),
        (1..=2, Some(true), None) => Some(OffsetRotate::Reverse),
        (1..=2, Some(false), Some(a)) => Some(OffsetRotate::AutoAngle(a)),
        (1..=2, Some(true), Some(a)) => Some(OffsetRotate::AutoAngle(a + 180.0)),
        (1, None, Some(a)) => Some(OffsetRotate::Angle(a)),
        _ => None,
    }
}

/// Разбивает значение `offset` на токены верхнего уровня: пробелы разделяют,
/// `/` — самостоятельный токен, содержимое `(...)` не режется.
fn split_offset_tokens(val: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (i, c) in val.char_indices() {
        match c {
            '(' => {
                depth += 1;
                start.get_or_insert(i);
            }
            ')' => depth -= 1,
            '/' if depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&val[st..i]);
                }
                out.push("/");
            }
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&val[st..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(st) = start {
        out.push(&val[st..]);
    }
    out
}

/// `offset-path`-токен: `none` или функция (`path()`, `ray()`, `url()`, фигура).
fn is_offset_path_token(t: &str) -> bool {
    t.eq_ignore_ascii_case("none") || (t.contains('(') && t.ends_with(')') && !t.starts_with("calc(")
        && !t.starts_with("min(") && !t.starts_with("max(") && !t.starts_with("clamp("))
}

/// Шортхенд `offset` (CSS Motion Path L1 §4):
/// `[<offset-position>? [<offset-path> [<offset-distance> || <offset-rotate>]?]?]! [/ <offset-anchor>]?`.
///
/// Сбрасывает `offset-path`/`-distance`/`-rotate`/`-anchor` к initial и
/// присваивает заданные. `offset-position` в движке не хранится (роль
/// начальной точки играет нормальная позиция бокса), поэтому ведущая позиция
/// разбирается, но отбрасывается. Невалидное значение — декларация
/// игнорируется целиком.
fn apply_offset_shorthand(
    style: &mut ComputedStyle,
    val: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) {
    let tokens = split_offset_tokens(val);
    if tokens.is_empty() {
        return;
    }
    let slash = tokens.iter().position(|t| *t == "/");
    let (main, anchor_toks) = match slash {
        Some(i) => (&tokens[..i], Some(&tokens[i + 1..])),
        None => (&tokens[..], None),
    };
    let anchor = match anchor_toks {
        None => None,
        Some([]) => return,
        Some(a) => {
            let joined = a.join(" ");
            if joined.eq_ignore_ascii_case("auto") {
                Some(None)
            } else {
                match ObjectPosition::parse(&joined, em_basis, viewport) {
                    Some(p) => Some(Some(p)),
                    None => return,
                }
            }
        }
    };
    // Ведущая `<offset-position>`: `normal | auto | <position>` до path-токена.
    let path_idx = main.iter().position(|t| is_offset_path_token(t));
    let (pos_toks, rest) = match path_idx {
        Some(i) => (&main[..i], &main[i..]),
        None => (main, &main[main.len()..]),
    };
    if !pos_toks.is_empty() {
        let joined = pos_toks.join(" ");
        let ok = joined.eq_ignore_ascii_case("normal")
            || joined.eq_ignore_ascii_case("auto")
            || ObjectPosition::parse(&joined, em_basis, viewport).is_some();
        if !ok {
            return;
        }
    }
    if rest.is_empty() && pos_toks.is_empty() && anchor.is_none() {
        return;
    }
    let mut path: Option<String> = None;
    let mut distance = None;
    let mut rotate_toks: Vec<&str> = Vec::new();
    if let Some((first, tail)) = rest.split_first() {
        path = if first.eq_ignore_ascii_case("none") { None } else { Some((*first).to_string()) };
        for t in tail {
            let lower = t.to_ascii_lowercase();
            let is_angle = ["deg", "grad", "rad", "turn"].iter().any(|u| lower.ends_with(u))
                && parse_angle_to_radians(t).is_some();
            if is_angle || lower == "auto" || lower == "reverse" {
                rotate_toks.push(t);
            } else if distance.is_none() && let Some(len) = parse_length_q(t, is_quirks) {
                distance = Some(len);
            } else {
                return;
            }
        }
    }
    let rotate = if rotate_toks.is_empty() {
        OffsetRotate::Auto
    } else {
        match parse_offset_rotate(&rotate_toks.join(" ")) {
            Some(r) => r,
            None => return,
        }
    };
    style.offset_path = path;
    style.offset_distance = distance.unwrap_or(Length::Px(0.0));
    style.offset_rotate = rotate;
    style.offset_anchor = anchor.unwrap_or(None);
}

/// Применить одну декларацию из группы «трансформации, анимации и прокрутка».
///
/// Возвращает `true`, если свойство принадлежит этой группе и было обработано;
/// `false` — если метка не наша и декларацию нужно предложить следующему
/// помощнику в цепочке `apply_declaration`.
#[allow(clippy::too_many_arguments)]
pub(in crate::style) fn apply_decl_motion(
    style: &mut ComputedStyle,
    prop: &str,
    val: &str,
    em_basis: f32,
    viewport: Size,
    is_quirks: bool,
) -> bool {
    match prop {
        "transform" => {
            // CSS Transforms L1 §2 — `none | <transform-list>`.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.transform = Vec::new();
            } else {
                style.transform = parse_transform_list(trimmed);
            }
        }
        "translate" => {
            // CSS Transforms L2 §2 — `none | <tx> [<ty>]`. px values; % deferred.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.translate = None;
            } else {
                let mut it = trimmed.split_whitespace();
                if let Some(tx) = it.next().and_then(parse_length_px) {
                    let ty = it.next().and_then(parse_length_px).unwrap_or(0.0);
                    style.translate = Some((tx, ty));
                }
            }
        }
        "rotate" => {
            // CSS Transforms L2 §2 — `none | <angle>`. Axis-angle form deferred.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.rotate = None;
            } else {
                style.rotate = parse_angle_to_radians(trimmed);
            }
        }
        "scale" => {
            // CSS Transforms L2 §2 — `none | <sx> [<sy>]`.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.scale = None;
            } else {
                let mut it = trimmed.split_whitespace();
                if let Some(sx) = it.next().and_then(|s| s.parse::<f32>().ok()) {
                    let sy = it.next().and_then(|s| s.parse::<f32>().ok()).unwrap_or(sx);
                    style.scale = Some((sx, sy));
                }
            }
        }
        "offset" => apply_offset_shorthand(style, val, em_basis, viewport, is_quirks),
        "offset-path" => {
            style.offset_path = match val.trim() {
                "none" => None,
                v => Some(v.to_string()),
            };
        }
        "offset-distance" => {
            if let Some(len) = parse_length_q(val, is_quirks) {
                style.offset_distance = len;
            }
        }
        "offset-rotate" => {
            // CSS Motion Path L1 §3.2 — `[ auto | reverse ] || <angle>`. Угол
            // хранится в ГРАДУСАХ (как ждёт `motion_path::resolve_motion_transform`),
            // а не в радианах, в которых его отдаёт `parse_angle_to_radians`.
            if let Some(r) = parse_offset_rotate(val) {
                style.offset_rotate = r;
            }
        }
        "offset-anchor" => {
            let v = val.trim();
            if v.eq_ignore_ascii_case("auto") {
                style.offset_anchor = None;
            } else if let Some(pos) = ObjectPosition::parse(v, em_basis, viewport) {
                style.offset_anchor = Some(pos);
            }
        }
        // CSS View Transitions L1 §10 — view-transition-name: none | <custom-ident>.
        // Names this element as a capture target during document.startViewTransition().
        // `none` (default) opts out; any other ident opts in. Per-page names must be unique,
        // but that constraint is not enforced here (shell deduplicates at capture time).
        "view-transition-name" => {
            let v = val.trim();
            style.view_transition_name = if v.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(v.into())
            };
        }
        "touch-action" => {
            let v = val.trim();
            style.touch_action = if v.contains("manipulation") {
                TouchAction::Manipulation
            } else if v == "none" {
                TouchAction::None
            } else if v == "auto" {
                TouchAction::Auto
            } else if v.contains("pan-left") {
                TouchAction::PanLeft
            } else if v.contains("pan-right") {
                TouchAction::PanRight
            } else if v.contains("pan-x") {
                TouchAction::PanX
            } else if v.contains("pan-up") {
                TouchAction::PanUp
            } else if v.contains("pan-down") {
                TouchAction::PanDown
            } else if v.contains("pan-y") {
                TouchAction::PanY
            } else if v.contains("pinch-zoom") {
                TouchAction::PinchZoom
            } else {
                style.touch_action
            };
        }
        "scroll-behavior" => {
            if let Some(v) = ScrollBehavior::parse(val) {
                style.scroll_behavior = v;
            }
        }
        "scroll-snap-type" => {
            if let Some(v) = parse_scroll_snap_type(val) {
                style.scroll_snap_type = v;
            }
        }
        "scroll-snap-align" => {
            if let Some(v) = parse_scroll_snap_align(val) {
                style.scroll_snap_align = v;
            }
        }
        "scroll-snap-stop" => {
            match val.trim().to_ascii_lowercase().as_str() {
                "normal" => style.scroll_snap_stop = ScrollSnapStop::Normal,
                "always" => style.scroll_snap_stop = ScrollSnapStop::Always,
                _ => {}
            }
        }
        "scroll-initial-target" => {
            match val.trim().to_ascii_lowercase().as_str() {
                "none" => style.scroll_initial_target = ScrollInitialTarget::None,
                "nearest" => style.scroll_initial_target = ScrollInitialTarget::Nearest,
                _ => {}
            }
        }
        "scroll-margin-top" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_top = px;
            }
        }
        "scroll-margin-right" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_right = px;
            }
        }
        "scroll-margin-bottom" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_bottom = px;
            }
        }
        "scroll-margin-left" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_margin_left = px;
            }
        }
        "scroll-margin" => {
            let parts: Vec<f32> = val
                .split_whitespace()
                .filter_map(|p| resolve_box_length(p, em_basis, viewport, is_quirks))
                .collect();
            let (t, r, b, l) = expand_4_sides(&parts);
            style.scroll_margin_top = t;
            style.scroll_margin_right = r;
            style.scroll_margin_bottom = b;
            style.scroll_margin_left = l;
        }
        "scroll-padding-top" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_top = px;
            }
        }
        "scroll-padding-right" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_right = px;
            }
        }
        "scroll-padding-bottom" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_bottom = px;
            }
        }
        "scroll-padding-left" => {
            if let Some(px) = resolve_box_length(val, em_basis, viewport, is_quirks) {
                style.scroll_padding_left = px;
            }
        }
        "scroll-padding" => {
            let parts: Vec<f32> = val
                .split_whitespace()
                .filter_map(|p| resolve_box_length(p, em_basis, viewport, is_quirks))
                .collect();
            let (t, r, b, l) = expand_4_sides(&parts);
            style.scroll_padding_top = t;
            style.scroll_padding_right = r;
            style.scroll_padding_bottom = b;
            style.scroll_padding_left = l;
        }
        "overscroll-behavior-x" => {
            if let Some(v) = parse_overscroll_behavior(val) {
                style.overscroll_behavior_x = v;
            }
        }
        "overscroll-behavior-y" => {
            if let Some(v) = parse_overscroll_behavior(val) {
                style.overscroll_behavior_y = v;
            }
        }
        "overscroll-behavior-block" => {
            if let Some(v) = parse_overscroll_behavior(val) {
                style.overscroll_behavior_block = v;
            }
        }
        "overscroll-behavior-inline" => {
            if let Some(v) = parse_overscroll_behavior(val) {
                style.overscroll_behavior_inline = v;
            }
        }
        "overscroll-behavior" => {
            // Shorthand: 1 значение — оба, 2 значения — x и y.
            let parts: Vec<&str> = val.split_whitespace().collect();
            if let Some(x) = parts.first().and_then(|p| parse_overscroll_behavior(p)) {
                style.overscroll_behavior_x = x;
                let y = parts.get(1).and_then(|p| parse_overscroll_behavior(p)).unwrap_or(x);
                style.overscroll_behavior_y = y;
            }
        }
        "transform-origin" => {
            // CSS Transforms L1 §6: <position> [<length>]?
            // Supports px, %, and keywords (center/left/right/top/bottom).
            // Percentages are stored raw and resolved at display-list time against
            // the element's border-box dimensions (size only known after layout).
            let parts: Vec<&str> = val.split_whitespace().collect();
            let x = parts.first()
                .and_then(|s| parse_position_component(s, em_basis, viewport, false))
                .unwrap_or(PositionComponent::Percent(0.5));
            let y = parts.get(1)
                .and_then(|s| parse_position_component(s, em_basis, viewport, true))
                .unwrap_or(PositionComponent::Percent(0.5));
            let z = parts.get(2).and_then(|s| resolve_box_length(s, em_basis, viewport, is_quirks)).unwrap_or(0.0);
            style.transform_origin = (x, y, z);
        }
        "perspective" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.perspective = None;
            } else if let Some(px) = resolve_box_length(trimmed, em_basis, viewport, is_quirks) {
                style.perspective = if px > 0.0 { Some(px) } else { None };
            }
        }
        "perspective-origin" => {
            // CSS Transforms L2 §4 — `perspective-origin: <x> <y>`.
            let parts: Vec<&str> = val.split_whitespace().collect();
            let x = parts.first()
                .and_then(|s| parse_position_component(s, em_basis, viewport, false))
                .unwrap_or(PositionComponent::Percent(0.5));
            let y = parts.get(1)
                .and_then(|s| parse_position_component(s, em_basis, viewport, true))
                .unwrap_or(PositionComponent::Percent(0.5));
            style.perspective_origin = (x, y);
        }
        "transform-style" => {
            // CSS Transforms L2 §6 — `transform-style: flat | preserve-3d`.
            let trimmed = val.trim();
            style.transform_style = if trimmed.eq_ignore_ascii_case("preserve-3d") {
                TransformStyle::Preserve3d
            } else {
                TransformStyle::Flat
            };
        }
        "backface-visibility" => {
            // CSS Transforms L2 §5.1 — `backface-visibility: visible | hidden`.
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("hidden") {
                style.backface_visibility = BackfaceVisibility::Hidden;
            } else if trimmed.eq_ignore_ascii_case("visible") {
                style.backface_visibility = BackfaceVisibility::Visible;
            }
        }
        "transition-property" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") {
                style.transition_properties = Vec::new();
            } else if trimmed.eq_ignore_ascii_case("all") {
                style.transition_properties = vec!["all".to_string()];
            } else {
                let mut props = Vec::new();
                for prop in trimmed.split(',') {
                    let prop = prop.trim().to_string();
                    if prop.is_empty() {
                        continue;
                    }
                    for expanded in expand_grouped_transition_property(&prop) {
                        props.push(expanded);
                    }
                }
                style.transition_properties = props;
            }
        }
        "transition-duration" => {
            style.transition_durations = parse_time_list(val);
        }
        "transition-delay" => {
            style.transition_delays = parse_time_list(val);
        }
        // Interest Invokers (GAP-INTERESTINVOKER): задержки показа/снятия
        // интереса. Раскладку не двигают — их читает JS-шим через
        // getComputedStyle, когда взводит таймеры `interest`/`loseinterest`.
        "interest-delay-start" => {
            if let Some(v) = parse_interest_delay(val) {
                style.interest_delay_start = v;
            }
        }
        "interest-delay-end" => {
            if let Some(v) = parse_interest_delay(val) {
                style.interest_delay_end = v;
            }
        }
        "interest-delay" => {
            // `<start> <end>?` — одно значение задаёт обе половины.
            let parts: Vec<&str> = val.split_whitespace().collect();
            let parsed: Option<Vec<Option<f32>>> =
                parts.iter().map(|p| parse_interest_delay(p)).collect();
            if let Some(v) = parsed.filter(|v| matches!(v.len(), 1 | 2)) {
                style.interest_delay_start = v[0];
                style.interest_delay_end = *v.last().unwrap_or(&v[0]);
            }
        }
        "transition-timing-function" => {
            style.transition_timing_functions = TimingFunction::parse_list(val);
        }
        "transition-behavior" => {
            // CSS Transitions L2 §3.1: невалидный список отбрасывает декларацию.
            if let Some(v) = TransitionBehavior::parse_list(val) {
                style.transition_behaviors = v;
            }
        }
        "transition-fill-mode" => {
            style.transition_fill_modes = AnimationFillMode::parse_list(val);
        }
        "animation" => {
            apply_animation_shorthand(style, val);
        }
        "transition" => {
            apply_transition_shorthand(style, val);
        }
        "animation-name" => {
            let trimmed = val.trim();
            if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
                style.animation_names = Vec::new();
            } else {
                style.animation_names = split_top_level_commas(trimmed)
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
                    .collect();
            }
        }
        "animation-duration" => {
            style.animation_durations = parse_time_list(val);
        }
        "animation-delay" => {
            style.animation_delays = parse_time_list(val);
        }
        "animation-timing-function" => {
            style.animation_timing_functions = TimingFunction::parse_list(val);
        }
        "animation-iteration-count" => {
            style.animation_iteration_counts = IterationCount::parse_list(val);
        }
        "animation-direction" => {
            style.animation_directions = AnimationDirection::parse_list(val);
        }
        "animation-fill-mode" => {
            style.animation_fill_modes = AnimationFillMode::parse_list(val);
        }
        "animation-play-state" => {
            style.animation_play_states = AnimationPlayState::parse_list(val);
        }
        "animation-timeline" => {
            style.animation_timelines = parse_animation_timeline_list(val);
        }
        "scroll-timeline-name" => {
            let t = val.trim();
            style.scroll_timeline_name = if t.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(t.to_string())
            };
        }
        "scroll-timeline-axis" => {
            if let Some(axis) = parse_scroll_axis(val.trim()) {
                style.scroll_timeline_axis = axis;
            }
        }
        "scroll-timeline" => {
            apply_scroll_timeline_shorthand(style, val);
        }
        "view-timeline-name" => {
            let t = val.trim();
            style.view_timeline_name = if t.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(t.to_string())
            };
        }
        "view-timeline-axis" => {
            if let Some(axis) = parse_scroll_axis(val.trim()) {
                style.view_timeline_axis = axis;
            }
        }
        "view-timeline" => {
            apply_view_timeline_shorthand(style, val);
        }
        _ => return false,
    }
    true
}

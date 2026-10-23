use super::*;

// ──────── CSS Easing L1 — TimingFunction parser ────────

#[test]
fn timing_function_linear_keyword() {
    assert_eq!(TimingFunction::parse("linear"), Some(TimingFunction::Linear));
}

#[test]
fn timing_function_ease_keywords() {
    assert_eq!(
        TimingFunction::parse("ease"),
        Some(TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0))
    );
    assert_eq!(
        TimingFunction::parse("ease-in"),
        Some(TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0))
    );
    assert_eq!(
        TimingFunction::parse("ease-out"),
        Some(TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0))
    );
    assert_eq!(
        TimingFunction::parse("ease-in-out"),
        Some(TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0))
    );
}

#[test]
fn timing_function_cubic_bezier_explicit() {
    assert_eq!(
        TimingFunction::parse("cubic-bezier(0.1, 0.7, 0.9, 0.3)"),
        Some(TimingFunction::CubicBezier(0.1, 0.7, 0.9, 0.3))
    );
}

#[test]
fn timing_function_cubic_bezier_x_out_of_range_rejected() {
    // x1 / x2 ∈ [0, 1] by spec; out-of-range — invalid.
    assert_eq!(TimingFunction::parse("cubic-bezier(1.5, 0, 0.5, 1)"), None);
    assert_eq!(TimingFunction::parse("cubic-bezier(0, 0, -0.1, 1)"), None);
}

#[test]
fn timing_function_cubic_bezier_y_unbounded() {
    // y координаты могут быть вне [0, 1] (overshoot easings).
    assert_eq!(
        TimingFunction::parse("cubic-bezier(0.5, -0.5, 0.5, 1.5)"),
        Some(TimingFunction::CubicBezier(0.5, -0.5, 0.5, 1.5))
    );
}

#[test]
fn timing_function_step_keywords() {
    assert_eq!(
        TimingFunction::parse("step-start"),
        Some(TimingFunction::Steps(1, StepPosition::JumpStart))
    );
    assert_eq!(
        TimingFunction::parse("step-end"),
        Some(TimingFunction::Steps(1, StepPosition::JumpEnd))
    );
}

#[test]
fn timing_function_steps_with_position() {
    assert_eq!(
        TimingFunction::parse("steps(4, jump-start)"),
        Some(TimingFunction::Steps(4, StepPosition::JumpStart))
    );
    assert_eq!(
        TimingFunction::parse("steps(3, end)"),
        Some(TimingFunction::Steps(3, StepPosition::JumpEnd))
    );
    assert_eq!(
        TimingFunction::parse("steps(5, jump-both)"),
        Some(TimingFunction::Steps(5, StepPosition::JumpBoth))
    );
}

#[test]
fn timing_function_steps_default_position_is_jump_end() {
    // steps(n) без position ≡ steps(n, jump-end).
    assert_eq!(
        TimingFunction::parse("steps(7)"),
        Some(TimingFunction::Steps(7, StepPosition::JumpEnd))
    );
}

#[test]
fn timing_function_steps_jump_none_requires_n_ge_2() {
    // jump-none с n=1 — невалидно (никаких шагов между границами).
    assert_eq!(TimingFunction::parse("steps(1, jump-none)"), None);
    assert_eq!(
        TimingFunction::parse("steps(2, jump-none)"),
        Some(TimingFunction::Steps(2, StepPosition::JumpNone))
    );
}

#[test]
fn timing_function_steps_zero_invalid() {
    assert_eq!(TimingFunction::parse("steps(0)"), None);
    assert_eq!(TimingFunction::parse("steps(0, end)"), None);
}

#[test]
fn timing_function_case_insensitive() {
    assert_eq!(
        TimingFunction::parse("LINEAR"),
        Some(TimingFunction::Linear)
    );
    assert_eq!(
        TimingFunction::parse("Cubic-Bezier(0.25, 0.1, 0.25, 1.0)"),
        Some(TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0))
    );
}

#[test]
fn timing_function_default_is_ease() {
    assert_eq!(
        TimingFunction::default(),
        TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
    );
}

#[test]
fn timing_function_list_with_nested_commas() {
    // split_top_level_commas должен корректно сохранять argument commas
    // внутри cubic-bezier(...) и steps(...).
    let list = TimingFunction::parse_list(
        "linear, cubic-bezier(0.1, 0.2, 0.3, 0.4), steps(3, end)",
    );
    assert_eq!(list.len(), 3);
    assert_eq!(list[0], TimingFunction::Linear);
    assert_eq!(list[1], TimingFunction::CubicBezier(0.1, 0.2, 0.3, 0.4));
    assert_eq!(list[2], TimingFunction::Steps(3, StepPosition::JumpEnd));
}

// ──────── CSS Transitions L1 — transition-timing-function ────────

#[test]
fn transition_timing_function_single() {
    let root = lay("<p>x</p>", "p { transition-timing-function: ease-in-out; }");
    let s = first_p_style(&root);
    assert_eq!(s.transition_timing_functions.len(), 1);
    assert_eq!(
        s.transition_timing_functions[0],
        TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0)
    );
}

#[test]
fn transition_timing_function_list_of_three() {
    let root = lay(
        "<p>x</p>",
        "p { transition-timing-function: linear, cubic-bezier(0.5, 0, 0.5, 1), steps(4); }",
    );
    let s = first_p_style(&root);
    assert_eq!(s.transition_timing_functions.len(), 3);
    assert_eq!(s.transition_timing_functions[0], TimingFunction::Linear);
    assert_eq!(
        s.transition_timing_functions[2],
        TimingFunction::Steps(4, StepPosition::JumpEnd)
    );
}

#[test]
fn transition_timing_function_default_empty() {
    // Без декларации — пустой Vec (consumer применяет default `ease`
    // через cyclically-reuse правило).
    let root = lay("<p>x</p>", "p { color: red; }");
    assert!(first_p_style(&root).transition_timing_functions.is_empty());
}

// ──────── CSS Animations L1 — animation-name ────────

#[test]
fn animation_name_single() {
    let root = lay("<p>x</p>", "p { animation-name: spin; }");
    let s = first_p_style(&root);
    assert_eq!(s.animation_names, vec!["spin".to_string()]);
}

#[test]
fn animation_name_comma_list() {
    let root = lay("<p>x</p>", "p { animation-name: fade, slide, bounce; }");
    let s = first_p_style(&root);
    assert_eq!(s.animation_names.len(), 3);
    assert_eq!(s.animation_names[1], "slide");
}

#[test]
fn animation_name_none_clears() {
    let root = lay(
        "<p>x</p>",
        "p { animation-name: spin; animation-name: none; }",
    );
    assert!(first_p_style(&root).animation_names.is_empty());
}

#[test]
fn animation_name_default_empty() {
    let root = lay("<p>x</p>", "p { color: red; }");
    assert!(first_p_style(&root).animation_names.is_empty());
}

// ──────── CSS Animations L1 — animation-duration / -delay ────────

#[test]
fn animation_duration_seconds_and_ms() {
    let root = lay(
        "<p>x</p>",
        "p { animation-duration: 1s, 200ms, 0.5s; }",
    );
    let durations = &first_p_style(&root).animation_durations;
    assert_eq!(durations.len(), 3);
    assert!((durations[0] - 1.0).abs() < 1e-5);
    assert!((durations[1] - 0.2).abs() < 1e-5);
    assert!((durations[2] - 0.5).abs() < 1e-5);
}

#[test]
fn animation_delay_negative_allowed() {
    // Отрицательный animation-delay допустим (phase offset).
    let root = lay("<p>x</p>", "p { animation-delay: -200ms; }");
    let s = first_p_style(&root);
    assert_eq!(s.animation_delays.len(), 1);
    assert!((s.animation_delays[0] - (-0.2)).abs() < 1e-5);
}

// ──────── CSS Animations L1 — animation-timing-function ────────

#[test]
fn animation_timing_function_keyword_and_function_mixed() {
    let root = lay(
        "<p>x</p>",
        "p { animation-timing-function: ease, steps(4, jump-start); }",
    );
    let s = first_p_style(&root);
    assert_eq!(s.animation_timing_functions.len(), 2);
    assert_eq!(
        s.animation_timing_functions[0],
        TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
    );
    assert_eq!(
        s.animation_timing_functions[1],
        TimingFunction::Steps(4, StepPosition::JumpStart)
    );
}

// ──────── CSS Animations L1 — animation-iteration-count ────────

#[test]
fn animation_iteration_count_finite() {
    let root = lay("<p>x</p>", "p { animation-iteration-count: 3; }");
    let s = first_p_style(&root);
    assert_eq!(s.animation_iteration_counts.len(), 1);
    assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(3.0));
}

#[test]
fn animation_iteration_count_fractional() {
    // Spec L1 §3.5 — count может быть дробным (`2.5` ≡ две полных
    // итерации + половина третьей).
    let root = lay("<p>x</p>", "p { animation-iteration-count: 2.5; }");
    let s = first_p_style(&root);
    assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(2.5));
}

#[test]
fn animation_iteration_count_infinite_keyword() {
    let root = lay("<p>x</p>", "p { animation-iteration-count: infinite; }");
    let s = first_p_style(&root);
    assert_eq!(s.animation_iteration_counts[0], IterationCount::Infinite);
}

#[test]
fn animation_iteration_count_list() {
    let root = lay(
        "<p>x</p>",
        "p { animation-iteration-count: 1, infinite, 5; }",
    );
    let s = first_p_style(&root);
    assert_eq!(s.animation_iteration_counts.len(), 3);
    assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(1.0));
    assert_eq!(s.animation_iteration_counts[1], IterationCount::Infinite);
    assert_eq!(s.animation_iteration_counts[2], IterationCount::Finite(5.0));
}

#[test]
fn animation_iteration_count_negative_invalid() {
    // Отрицательный count — invalid declaration, не записывается.
    let root = lay("<p>x</p>", "p { animation-iteration-count: -1; }");
    let s = first_p_style(&root);
    assert!(s.animation_iteration_counts.is_empty());
}

// ──────── CSS Animations L1 — animation-direction ────────

#[test]
fn animation_direction_all_keywords() {
    let cases = [
        ("normal", AnimationDirection::Normal),
        ("reverse", AnimationDirection::Reverse),
        ("alternate", AnimationDirection::Alternate),
        ("alternate-reverse", AnimationDirection::AlternateReverse),
    ];
    for (kw, expected) in cases {
        let css = format!("p {{ animation-direction: {kw}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).animation_directions[0], expected);
    }
}

#[test]
fn animation_direction_list() {
    let root = lay(
        "<p>x</p>",
        "p { animation-direction: normal, alternate-reverse; }",
    );
    let s = first_p_style(&root);
    assert_eq!(s.animation_directions.len(), 2);
    assert_eq!(s.animation_directions[1], AnimationDirection::AlternateReverse);
}

// ──────── CSS Animations L1 — animation-fill-mode ────────

#[test]
fn animation_fill_mode_all_keywords() {
    let cases = [
        ("none", AnimationFillMode::None),
        ("forwards", AnimationFillMode::Forwards),
        ("backwards", AnimationFillMode::Backwards),
        ("both", AnimationFillMode::Both),
    ];
    for (kw, expected) in cases {
        let css = format!("p {{ animation-fill-mode: {kw}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).animation_fill_modes[0], expected);
    }
}

// ──────── CSS Animations L1 — animation-play-state ────────

#[test]
fn animation_play_state_running_paused() {
    let root = lay("<p>x</p>", "p { animation-play-state: paused; }");
    let s = first_p_style(&root);
    assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
}

#[test]
fn animation_play_state_list() {
    let root = lay(
        "<p>x</p>",
        "p { animation-play-state: running, paused, running; }",
    );
    let s = first_p_style(&root);
    assert_eq!(s.animation_play_states.len(), 3);
    assert_eq!(s.animation_play_states[1], AnimationPlayState::Paused);
}

// ──────── CSS Animations defaults — все списки пусты по initial value ────────

#[test]
fn animation_longhands_default_all_empty() {
    let root = lay("<p>x</p>", "p { color: red; }");
    let s = first_p_style(&root);
    assert!(s.animation_names.is_empty());
    assert!(s.animation_durations.is_empty());
    assert!(s.animation_delays.is_empty());
    assert!(s.animation_iteration_counts.is_empty());
    assert!(s.animation_timing_functions.is_empty());
    assert!(s.animation_directions.is_empty());
    assert!(s.animation_fill_modes.is_empty());
    assert!(s.animation_play_states.is_empty());
}

// ──────── CSS Text typography (tab-size, caret-color, overflow-wrap, word-break, hyphens) ────────

#[test]
fn tab_size_integer_in_spaces() {
    let root = lay("<p>x</p>", "p { tab-size: 4; }");
    // integer 4 → 32px (8px-per-space).
    assert!((first_p_style(&root).tab_size - 32.0).abs() < 0.01);
}

#[test]
fn tab_size_length() {
    let root = lay("<p>x</p>", "p { tab-size: 40px; }");
    assert!((first_p_style(&root).tab_size - 40.0).abs() < 0.01);
}

#[test]
fn tab_size_default_64() {
    let root = lay("<p>x</p>", "p { color: red; }");
    assert!((first_p_style(&root).tab_size - 64.0).abs() < 0.01);
}

#[test]
fn tab_size_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { tab-size: 100px; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert!((p.style.tab_size - 100.0).abs() < 0.01);
}

#[test]
fn white_space_pre_parsed() {
    let root = lay("<p>x</p>", "p { white-space: pre; }");
    assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::Pre);
}

#[test]
fn white_space_pre_wrap_parsed() {
    let root = lay("<p>x</p>", "p { white-space: pre-wrap; }");
    assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::PreWrap);
}

#[test]
fn white_space_pre_line_parsed() {
    let root = lay("<p>x</p>", "p { white-space: pre-line; }");
    assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::PreLine);
}

#[test]
fn pre_element_ua_white_space_pre() {
    let root = lay("<pre>hello</pre>", "");
    let pre_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    assert_eq!(pre_box.style.white_space, crate::style::WhiteSpace::Pre,
        "UA: <pre> should default to white-space: pre");
}

#[test]
fn pre_element_newline_creates_two_lines() {
    let root = lay_measured("<pre>line1\nline2</pre>", "", 800.0);
    let pre_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let run = pre_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    if let BoxKind::InlineRun { lines, .. } = &run.kind {
        assert_eq!(lines.len(), 2, "expected 2 lines for \\n in <pre>, got {}", lines.len());
        assert_eq!(lines[0][0].text, "line1");
        assert_eq!(lines[1][0].text, "line2");
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn pre_element_tab_renders_with_tab_size() {
    // tab-size: 4 → stops every 4 spaces = 32px (space = 8px); char width=8px each.
    // "a	b": 'a'=8, the tab runs to the stop at 32 (24px), 'b'=8.
    let root = lay_measured("<pre>a	b</pre>", "pre { tab-size: 4; }", 800.0);
    let pre_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let run = pre_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    if let BoxKind::InlineRun { lines, .. } = &run.kind {
        assert_eq!(lines.len(), 1);
        let frags = &lines[0];
        assert_eq!(frags.len(), 3, "text / tab / text: {frags:?}");
        assert_eq!(frags[1].text, "	", "tab must be preserved in text");
        assert!((frags[1].width - 24.0).abs() < 0.01, "tab width {}", frags[1].width);
        assert!((frags[2].x - 32.0).abs() < 0.01, "b at {}", frags[2].x);
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn white_space_break_spaces_parsed() {
    let root = lay("<p>x</p>", "p { white-space: break-spaces; }");
    let s = first_p_style(&root);
    assert_eq!(s.white_space, crate::style::WhiteSpace::BreakSpaces);
    assert_eq!(s.white_space_collapse, crate::style::WhiteSpaceCollapse::BreakSpaces);
    assert_eq!(s.text_wrap_mode, crate::style::TextWrapMode::Wrap);
}

#[test]
fn white_space_collapse_default_collapse() {
    let root = lay("<p>x</p>", "");
    let s = first_p_style(&root);
    assert_eq!(s.white_space_collapse, crate::style::WhiteSpaceCollapse::Collapse);
    assert_eq!(s.white_space, crate::style::WhiteSpace::Normal);
}

#[test]
fn white_space_collapse_preserve_gives_pre_wrap() {
    // CSS Text L4 §2.1: collapse=preserve + wrap-mode=wrap (initial) → pre-wrap.
    let root = lay("<p>x</p>", "p { white-space-collapse: preserve; }");
    let s = first_p_style(&root);
    assert_eq!(s.white_space_collapse, crate::style::WhiteSpaceCollapse::Preserve);
    assert_eq!(s.white_space, crate::style::WhiteSpace::PreWrap);
}

#[test]
fn white_space_collapse_preserve_plus_nowrap_gives_pre() {
    let root = lay(
        "<p>x</p>",
        "p { white-space-collapse: preserve; text-wrap-mode: nowrap; }",
    );
    assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::Pre);
}

#[test]
fn text_wrap_mode_before_collapse_order_independent() {
    // Longhand-ы применяются в порядке каскада; результат не должен
    // зависеть от порядка объявлений.
    let root = lay(
        "<p>x</p>",
        "p { text-wrap-mode: nowrap; white-space-collapse: preserve; }",
    );
    assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::Pre);
}

#[test]
fn white_space_collapse_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { white-space-collapse: preserve-breaks; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.white_space_collapse, crate::style::WhiteSpaceCollapse::PreserveBreaks);
    assert_eq!(p.style.white_space, crate::style::WhiteSpace::PreLine);
}

#[test]
fn white_space_collapse_invalid_ignored() {
    let root = lay("<p>x</p>", "p { white-space-collapse: bogus; }");
    let s = first_p_style(&root);
    assert_eq!(s.white_space_collapse, crate::style::WhiteSpaceCollapse::Collapse);
    assert_eq!(s.white_space, crate::style::WhiteSpace::Normal);
}

#[test]
fn text_wrap_mode_nowrap_updates_effective_white_space() {
    // Integration (CSS Text L4 §6.4.1): text-wrap-mode теперь влияет на
    // эффективный white_space, которым пользуется layout.
    let root = lay("<p>x</p>", "p { text-wrap-mode: nowrap; }");
    assert_eq!(first_p_style(&root).white_space, crate::style::WhiteSpace::Nowrap);
}

#[test]
fn white_space_shorthand_resets_collapse_component() {
    // white-space — shorthand: значение normal сбрасывает preserve,
    // унаследованный от родителя.
    let root = lay(
        "<div><p>x</p></div>",
        "div { white-space: pre; } p { white-space: normal; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.white_space_collapse, crate::style::WhiteSpaceCollapse::Collapse);
    assert_eq!(p.style.white_space, crate::style::WhiteSpace::Normal);
}

#[test]
fn pre_line_newline_creates_two_lines() {
    // CSS Text L4 §3.1 preserve-breaks: \n — forced break, пробелы
    // вокруг него схлопываются.
    let root = lay_measured("<p>line1   \n   line2</p>", "p { white-space: pre-line; }", 800.0);
    let p_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let run = p_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    if let BoxKind::InlineRun { lines, .. } = &run.kind {
        assert_eq!(lines.len(), 2, "expected 2 lines for \\n in pre-line, got {}", lines.len());
        assert_eq!(lines[0][0].text, "line1");
        assert_eq!(lines[1][0].text, "line2");
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn white_space_collapse_preserve_breaks_newline_layout() {
    // End-to-end через longhand: preserve-breaks сохраняет \n.
    let root = lay_measured(
        "<p>a\nb</p>",
        "p { white-space-collapse: preserve-breaks; }",
        800.0,
    );
    let p_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let run = p_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    if let BoxKind::InlineRun { lines, .. } = &run.kind {
        assert_eq!(lines.len(), 2, "preserve-breaks must keep \\n, got {} lines", lines.len());
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn break_spaces_preserves_space_runs() {
    // break-spaces сохраняет последовательности пробелов (Phase 0 —
    // как pre-wrap).
    let root = lay_measured("<p>a  b</p>", "p { white-space: break-spaces; }", 800.0);
    let p_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let run = p_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    if let BoxKind::InlineRun { lines, .. } = &run.kind {
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0][0].text, "a  b", "double space must be preserved");
    } else {
        panic!("expected InlineRun");
    }
}

#[test]
fn caret_color_named() {
    let root = lay("<p>x</p>", "p { caret-color: red; }");
    assert_eq!(
        first_p_style(&root).caret_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }))
    );
}

#[test]
fn caret_color_auto() {
    let root = lay("<p>x</p>", "p { caret-color: red; caret-color: auto; }");
    assert_eq!(first_p_style(&root).caret_color, None);
}

#[test]
fn caret_color_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { caret-color: blue; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.caret_color, Some(CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 })));
}

#[test]
fn caret_color_currentcolor_follows_own_color() {
    // CSS UI L4 §6.3: `<color>` включает `currentcolor`; used value — `color`
    // самого элемента, а не унаследованный `caret-color`.
    let root = lay(
        "<div><p>x</p></div>",
        "div { caret-color: currentcolor; color: red; } p { color: blue; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(div.style.used_caret_color(), Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(p.style.caret_color, Some(CssColor::CurrentColor));
    assert_eq!(p.style.used_caret_color(), Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn caret_color_system_color_resolved() {
    let root = lay("<p>x</p>", "p { caret-color: Highlight; }");
    let st = first_p_style(&root);
    assert!(
        matches!(st.caret_color, Some(CssColor::Rgba(_))),
        "системный цвет должен разрешиться постпассом: {:?}",
        st.caret_color
    );
}

#[test]
fn caret_color_wide_gamut_color_function() {
    let root = lay("<p>x</p>", "p { caret-color: color(display-p3 1 0 0); }");
    let st = first_p_style(&root);
    assert!(matches!(st.caret_color, Some(CssColor::Wide(_))));
    assert_eq!(st.used_caret_color().r, 255);
}

#[test]
fn caret_color_css_wide_keywords() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { caret-color: red; } p { caret-color: initial; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert!(div.style.caret_color.is_some());
    assert_eq!(p.style.caret_color, None, "initial = auto");
    let root = lay(
        "<div><p>x</p></div>",
        "div { caret-color: red; } p { caret-color: auto; } span { } p { caret-color: inherit; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.caret_color, div.style.caret_color);
}

#[test]
fn caret_color_invalid_ignored() {
    let root = lay("<p>x</p>", "p { caret-color: red; caret-color: 12px; }");
    assert_eq!(
        first_p_style(&root).caret_color,
        Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }))
    );
}

#[test]
fn caret_shape_and_animation_parse_and_invalid_ignored() {
    use crate::style::{CaretAnimation, CaretShape};
    let root = lay("<p>x</p>", "p { caret-shape: block; caret-animation: manual; }");
    let st = first_p_style(&root);
    assert_eq!((st.caret_shape, st.caret_animation), (CaretShape::Block, CaretAnimation::Manual));
    let root = lay("<p>x</p>", "p { caret-shape: underscore; caret-shape: none; caret-animation: manual; caret-animation: blink; }");
    let st = first_p_style(&root);
    assert_eq!((st.caret_shape, st.caret_animation), (CaretShape::Underscore, CaretAnimation::Manual));
    let root = lay("<p>x</p>", "");
    let st = first_p_style(&root);
    assert_eq!((st.caret_shape, st.caret_animation), (CaretShape::Auto, CaretAnimation::Auto));
}

#[test]
fn caret_shape_and_animation_are_inherited_and_css_wide() {
    use crate::style::{CaretAnimation, CaretShape};
    let root = lay(
        "<div><p>x</p></div>",
        "div { caret-shape: bar; caret-animation: manual; } p { caret-animation: initial; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.caret_shape, CaretShape::Bar, "inherited");
    assert_eq!(div.style.caret_animation, CaretAnimation::Manual);
    assert_eq!(p.style.caret_animation, CaretAnimation::Auto, "initial");
}

#[test]
fn input_security_parses_inherits_and_css_wide() {
    use crate::style::InputSecurity;
    let root = lay("<p>x</p>", "");
    assert_eq!(first_p_style(&root).input_security, InputSecurity::Auto);
    let root = lay("<p>x</p>", "p { input-security: none; input-security: disc; }");
    assert_eq!(first_p_style(&root).input_security, InputSecurity::None, "invalid value ignored");
    let root = lay("<div><p>x</p></div>", "div { input-security: none; } p { input-security: initial; }");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(div.style.input_security, InputSecurity::None);
    assert_eq!(p.style.input_security, InputSecurity::Auto, "initial");
    let root = lay("<div><p>x</p></div>", "div { input-security: none; }");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.input_security, InputSecurity::None, "inherited");
}

#[test]
fn caret_shorthand_sets_all_three_and_resets_omitted() {
    use crate::style::{CaretAnimation, CaretShape};
    let root = lay("<p>x</p>", "p { caret-shape: block; caret: manual red underscore; }");
    let st = first_p_style(&root);
    assert_eq!(st.caret_color, Some(CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 })));
    assert_eq!((st.caret_animation, st.caret_shape), (CaretAnimation::Manual, CaretShape::Underscore));
    // Omitted components go back to initial.
    let root = lay("<p>x</p>", "p { caret-color: red; caret-shape: block; caret: manual; }");
    let st = first_p_style(&root);
    assert_eq!((st.caret_color, st.caret_shape, st.caret_animation), (None, CaretShape::Auto, CaretAnimation::Manual));
    // Invalid / duplicated component: the whole declaration is dropped.
    let root = lay("<p>x</p>", "p { caret-shape: block; caret: block bar; } ");
    assert_eq!(first_p_style(&root).caret_shape, CaretShape::Block);
    let root = lay("<p>x</p>", "p { caret-shape: block; caret: 12px; } ");
    assert_eq!(first_p_style(&root).caret_shape, CaretShape::Block);
}

#[test]
fn overflow_wrap_break_word() {
    let root = lay("<p>x</p>", "p { overflow-wrap: break-word; }");
    assert_eq!(first_p_style(&root).overflow_wrap, OverflowWrap::BreakWord);
}

#[test]
fn word_wrap_alias_overflow_wrap() {
    // `word-wrap` legacy alias.
    let root = lay("<p>x</p>", "p { word-wrap: anywhere; }");
    assert_eq!(first_p_style(&root).overflow_wrap, OverflowWrap::Anywhere);
}

#[test]
fn word_break_keep_all() {
    let root = lay("<p>x</p>", "p { word-break: keep-all; }");
    assert_eq!(first_p_style(&root).word_break, WordBreak::KeepAll);
}

#[test]
fn word_break_break_all() {
    let root = lay("<p>x</p>", "p { word-break: break-all; }");
    assert_eq!(first_p_style(&root).word_break, WordBreak::BreakAll);
}

#[test]
fn hyphens_auto() {
    let root = lay("<p>x</p>", "p { hyphens: auto; }");
    assert_eq!(first_p_style(&root).hyphens, Hyphens::Auto);
}

#[test]
fn hyphens_none() {
    let root = lay("<p>x</p>", "p { hyphens: none; }");
    assert_eq!(first_p_style(&root).hyphens, Hyphens::None);
}

#[test]
fn hyphens_default_manual() {
    let root = lay("<p>x</p>", "p { color: red; }");
    assert_eq!(first_p_style(&root).hyphens, Hyphens::Manual);
}

#[test]
fn text_typography_all_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { tab-size: 50px; overflow-wrap: break-word; word-break: keep-all; hyphens: auto; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert!((p.style.tab_size - 50.0).abs() < 0.01);
    assert_eq!(p.style.overflow_wrap, OverflowWrap::BreakWord);
    assert_eq!(p.style.word_break, WordBreak::KeepAll);
    assert_eq!(p.style.hyphens, Hyphens::Auto);
    // А значения у div те же.
    assert!((div.style.tab_size - 50.0).abs() < 0.01);
}

// ──────── will-change / pointer-events / user-select / scroll-behavior ────────

#[test]
fn will_change_auto_is_empty_list() {
    let root = lay("<p>x</p>", "p { will-change: auto; }");
    assert!(first_p_style(&root).will_change.is_empty());
}

#[test]
fn will_change_property_list() {
    let root = lay("<p>x</p>", "p { will-change: transform, opacity; }");
    let s = first_p_style(&root);
    assert_eq!(
        s.will_change,
        vec!["transform".to_string(), "opacity".to_string()]
    );
}

#[test]
fn will_change_invalid_ident_skipped() {
    let root = lay("<p>x</p>", "p { will-change: 1invalid, transform; }");
    let s = first_p_style(&root);
    assert_eq!(s.will_change, vec!["transform".to_string()]);
}

#[test]
fn pointer_events_none() {
    let root = lay("<p>x</p>", "p { pointer-events: none; }");
    assert_eq!(first_p_style(&root).pointer_events, PointerEvents::None);
}

#[test]
fn pointer_events_all() {
    let root = lay("<p>x</p>", "p { pointer-events: all; }");
    assert_eq!(first_p_style(&root).pointer_events, PointerEvents::All);
}

#[test]
fn user_select_none() {
    let root = lay("<p>x</p>", "p { user-select: none; }");
    assert_eq!(first_p_style(&root).user_select, UserSelect::None);
}

#[test]
fn user_select_webkit_alias() {
    let root = lay("<p>x</p>", "p { -webkit-user-select: none; }");
    assert_eq!(first_p_style(&root).user_select, UserSelect::None);
}

#[test]
fn user_select_none_text_does_not_merge_with_neighbours() {
    // `bb` is `user-select: none`; it must stay its own fragment so selection
    // and copy can treat it separately from "aa " / " cc".
    let root = lay("<p>aa <span>bb</span> cc</p>", "span { user-select: none; }");
    fn frags(b: &crate::LayoutBox, out: &mut Vec<String>) {
        if let crate::BoxKind::InlineRun { lines, .. } = &b.kind {
            out.extend(lines.iter().flatten().map(|f| f.text.clone()));
        }
        for c in &b.children {
            frags(c, out);
        }
    }
    let mut out = Vec::new();
    frags(&root, &mut out);
    assert_eq!(out.len(), 3, "{out:?}");
}

#[test]
fn user_select_text() {
    let root = lay("<p>x</p>", "p { user-select: text; }");
    assert_eq!(first_p_style(&root).user_select, UserSelect::Text);
}

#[test]
fn user_select_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { user-select: none; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // Inherited.
    assert_eq!(p.style.user_select, UserSelect::None);
}

#[test]
fn scroll_behavior_smooth() {
    let root = lay("<p>x</p>", "p { scroll-behavior: smooth; }");
    assert_eq!(first_p_style(&root).scroll_behavior, ScrollBehavior::Smooth);
}

#[test]
fn scroll_behavior_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { scroll-behavior: smooth; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // CSSOM View §13: `scroll-behavior` is not inherited.
    assert_eq!(p.style.scroll_behavior, ScrollBehavior::Auto);
}

#[test]
fn pointer_events_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { pointer-events: none; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // Наследуется (CSS UI L4 §6.1: Inherited: yes) — p видит none от div.
    assert_eq!(p.style.pointer_events, PointerEvents::None);
    assert_eq!(div.style.pointer_events, PointerEvents::None);
}

#[test]
fn pointer_events_child_can_override_inherited_none() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { pointer-events: none; } p { pointer-events: auto; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.pointer_events, PointerEvents::Auto);
}

#[test]
fn pointer_events_css_wide_keywords() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { pointer-events: none; } p { pointer-events: auto; }          p { pointer-events: inherit; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.pointer_events, PointerEvents::None);
    let root = lay(
        "<div><p>x</p></div>",
        "div { pointer-events: none; } p { pointer-events: initial; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.pointer_events, PointerEvents::Auto);
    let root = lay(
        "<div><p>x</p></div>",
        "div { pointer-events: none; } p { pointer-events: unset; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.pointer_events, PointerEvents::None, "unset на inherited = inherit");
}

#[test]
fn inert_attribute_maps_to_interactivity_inert() {
    // UA rule `[inert] { interactivity: inert !important }` (HTML Rendering §15.4.2).
    let root = lay("<p inert>x</p>", "");
    let s = first_p_style(&root);
    assert_eq!(s.interactivity, Interactivity::Inert);
    // `interactivity` is the carrier — `pointer-events` itself is left alone.
    assert_eq!(s.pointer_events, PointerEvents::Auto);
    assert_eq!(s.effective_pointer_events(), PointerEvents::None);
}

#[test]
fn inert_is_inherited_and_a_descendant_can_reset_it() {
    let root = lay("<div inert><p>x</p><p class=a>y</p></div>", ".a { interactivity: auto; }");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let ps: Vec<_> = div.children.iter().filter(|c| matches!(&c.kind, BoxKind::Block)).collect();
    assert_eq!(div.style.interactivity, Interactivity::Inert);
    assert_eq!(ps[0].style.interactivity, Interactivity::Inert);
    assert_eq!(ps[1].style.interactivity, Interactivity::Auto);
}

#[test]
fn inert_attribute_beats_author_on_the_element_itself() {
    // UA `!important`: author `auto`, `all: initial` and a later rule all lose.
    let root = lay("<p inert>x</p>", "p { interactivity: auto; all: initial; display: block; }");
    assert_eq!(first_p_style(&root).interactivity, Interactivity::Inert);
}

#[test]
fn modal_dialog_and_fullscreen_reset_inherited_inert() {
    let root = lay(
        "<div inert><dialog open data-lumen-modal><span id=a inert></span></dialog><p data-lumen-fullscreen>f</p></div>",
        "",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let styles: Vec<_> = div.children.iter().map(|c| c.style.interactivity).collect();
    assert!(!styles.is_empty() && styles.iter().all(|s| *s == Interactivity::Auto), "{styles:?}");
}

#[test]
fn interactivity_css_wide_keywords_and_invalid_value() {
    let root = lay("<div inert><p>x</p></div>", "p { interactivity: initial; }");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.interactivity, Interactivity::Auto);
    let root = lay("<div inert><p>x</p></div>", "p { interactivity: inherit; }");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.interactivity, Interactivity::Inert);
    // Not a keyword of the grammar: the declaration is dropped, the value stays `auto`.
    let root = lay("<p>x</p>", "p { interactivity: none; }");
    assert_eq!(first_p_style(&root).interactivity, Interactivity::Auto);
}

#[test]
fn non_inert_keeps_pointer_events_auto() {
    let root = lay("<p>x</p>", "");
    assert_eq!(first_p_style(&root).pointer_events, PointerEvents::Auto);
}

#[test]
fn unknown_keyword_keeps_default() {
    let root = lay("<p>x</p>", "p { pointer-events: garbage; user-select: weird; }");
    let s = first_p_style(&root);
    assert_eq!(s.pointer_events, PointerEvents::Auto);
    assert_eq!(s.user_select, UserSelect::Auto);
}


// Продолжение файла (DEVX-22, потолок размера): background-*, place-items/align-*, quirks.
#[path = "background_alignment_quirks.rs"]
mod background_alignment_quirks;

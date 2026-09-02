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
    // tab-size: 4 → 4*8=32px; char width=8px each.
    // "a\tb" → 'a'=8 + '\t'=32 + 'b'=8 = 48px width frag.
    let root = lay_measured("<pre>a\tb</pre>", "pre { tab-size: 4; }", 800.0);
    let pre_box = root.children.iter().find(|c| matches!(c.kind, BoxKind::Block)).unwrap();
    let run = pre_box.children.iter().find(|c| matches!(c.kind, BoxKind::InlineRun { .. })).unwrap();
    if let BoxKind::InlineRun { lines, .. } = &run.kind {
        assert_eq!(lines.len(), 1);
        let frag = &lines[0][0];
        // text should be preserved verbatim including \t
        assert!(frag.text.contains('\t'), "tab should be preserved in text: {:?}", frag.text);
        // width: 'a'(8) + '\t'(32) + 'b'(8) = 48
        assert!((frag.width - 48.0).abs() < 0.01, "expected width=48, got {}", frag.width);
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
        Some(Color { r: 255, g: 0, b: 0, a: 255 })
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
    assert_eq!(p.style.caret_color, Some(Color { r: 0, g: 0, b: 255, a: 255 }));
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
fn scroll_behavior_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { scroll-behavior: smooth; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.scroll_behavior, ScrollBehavior::Smooth);
}

#[test]
fn pointer_events_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { pointer-events: none; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // НЕ наследуется — у p default Auto.
    assert_eq!(p.style.pointer_events, PointerEvents::Auto);
    assert_eq!(div.style.pointer_events, PointerEvents::None);
}

#[test]
fn inert_sets_pointer_events_none() {
    // UA rule `[inert] { pointer-events: none; }` (HTML Rendering §15.4.2).
    let root = lay("<p inert>x</p>", "");
    assert_eq!(first_p_style(&root).pointer_events, PointerEvents::None);
}

#[test]
fn inert_inherited_to_descendant_pointer_events() {
    // Inertness is inherited down the DOM tree: a descendant of an inert
    // element is inert too, so its pointer-events is forced to none even
    // though it carries no `inert` attribute of its own.
    let root = lay("<div inert><p>x</p></div>", "");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(div.style.pointer_events, PointerEvents::None);
    assert_eq!(p.style.pointer_events, PointerEvents::None);
}

#[test]
fn inert_author_pointer_events_wins() {
    // The inert rule lives in the UA origin, so an author `pointer-events`
    // declaration overrides it.
    let root = lay("<p inert>x</p>", "p { pointer-events: auto; }");
    assert_eq!(first_p_style(&root).pointer_events, PointerEvents::Auto);
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

// ──────── background-* (CSS Backgrounds L3) ────────

#[test]
fn background_image_url_parses() {
    let root = lay("<p>x</p>", "p { background-image: url(\"bg.png\"); }");
    let s = first_p_style(&root);
    assert_eq!(s.background_layers[0].image, BackgroundImage::Url("bg.png".into()));
}

#[test]
fn background_image_url_unquoted() {
    let root = lay("<p>x</p>", "p { background-image: url(bg.png); }");
    assert_eq!(
        first_p_style(&root).background_layers[0].image,
        BackgroundImage::Url("bg.png".into())
    );
}

#[test]
fn background_image_none() {
    // Setting "none" after a URL replaces all layers with one None-image layer.
    let root = lay(
        "<p>x</p>",
        "p { background-image: url(\"x.png\"); background-image: none; }",
    );
    assert_eq!(first_p_style(&root).background_layers[0].image, BackgroundImage::None);
}

#[test]
fn background_image_gradient_parsed_linear() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: linear-gradient(to right, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, stops, .. }) => {
            assert!((angle_deg - 90.0).abs() < 0.1, "expected 90° for 'to right'");
            assert_eq!(stops.len(), 2);
        }
        other => panic!("expected ParsedGradient::Linear, got {other:?}"),
    }
}

// ── parse_gradient_stops ──────────────────────────────────────────────────

#[test]
fn gradient_stops_empty_string_returns_empty() {
    assert_eq!(parse_gradient_stops(""), vec![]);
}

#[test]
fn gradient_stops_no_parens_returns_empty() {
    assert_eq!(parse_gradient_stops("linear-gradient"), vec![]);
}

#[test]
fn gradient_stops_two_named_colors_no_position() {
    let stops = parse_gradient_stops("linear-gradient(red, blue)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[0].position, None);
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
    assert_eq!(stops[1].position, None);
}

#[test]
fn gradient_stops_to_right_direction_skipped() {
    let stops = parse_gradient_stops("linear-gradient(to right, red, blue)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_angle_direction_skipped() {
    let stops = parse_gradient_stops("linear-gradient(45deg, red 0%, blue 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(100.0)));
}

#[test]
fn gradient_stops_percent_positions_parsed() {
    let stops = parse_gradient_stops("linear-gradient(red 0%, green 50%, blue 100%)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(50.0)));
    assert_eq!(stops[2].position, Some(Length::Percent(100.0)));
}

#[test]
fn gradient_stops_px_positions_parsed() {
    let stops = parse_gradient_stops("linear-gradient(red 0px, blue 200px)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].position, Some(Length::Px(0.0)));
    assert_eq!(stops[1].position, Some(Length::Px(200.0)));
}

#[test]
fn gradient_stops_hex_color_with_percent() {
    let stops = parse_gradient_stops("linear-gradient(#ff0000 20%, #0000ff 80%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[0].position, Some(Length::Percent(20.0)));
}

#[test]
fn gradient_stops_rgba_function_color() {
    let stops = parse_gradient_stops("linear-gradient(rgba(255,0,0,1) 0%, rgba(0,0,255,1) 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_two_position_stop_expands() {
    // `red 20% 60%` → two stops: red@20% and red@60%
    let stops = parse_gradient_stops("linear-gradient(red 20% 60%, blue)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(20.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(60.0)));
    assert_eq!(stops[1].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[2].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_color_hint_skipped() {
    // `50%` between stops is a color hint — no color → skipped
    let stops = parse_gradient_stops("linear-gradient(red 0%, 50%, blue 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn gradient_stops_radial_shape_skipped() {
    let stops =
        parse_gradient_stops("radial-gradient(circle at 50% 50%, white, black)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 255, b: 255, a: 255 });
    assert_eq!(stops[1].color, Color { r: 0, g: 0, b: 0, a: 255 });
}

#[test]
fn gradient_stops_repeating_linear() {
    let stops =
        parse_gradient_stops("repeating-linear-gradient(red 0px, blue 10px)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(stops[0].position, Some(Length::Px(0.0)));
    assert_eq!(stops[1].position, Some(Length::Px(10.0)));
}

#[test]
fn gradient_stops_zero_unitless_is_px_zero() {
    let stops = parse_gradient_stops("linear-gradient(red 0, blue 100%)");
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].position, Some(Length::Px(0.0)));
}

// ── conic-gradient parsing ───────────────────────────────────────────────

#[test]
fn background_image_gradient_parsed_conic_default() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, stops, repeating,
        }) => {
            assert!((center_x_pct - 0.5).abs() < 1e-4);
            assert!((center_y_pct - 0.5).abs() < 1e-4);
            assert!(from_angle_deg.abs() < 1e-4, "default from-angle = 0°");
            assert_eq!(stops.len(), 2);
            assert!(!repeating);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_image_gradient_parsed_conic_from_and_at() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(from 90deg at 25% 75%, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, ..
        }) => {
            assert!((center_x_pct - 0.25).abs() < 1e-4);
            assert!((center_y_pct - 0.75).abs() < 1e-4);
            assert!((from_angle_deg - 90.0).abs() < 1e-3);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_image_gradient_parsed_repeating_conic() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: repeating-conic-gradient(red 0deg, blue 90deg); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic { repeating, stops, .. }) => {
            assert!(repeating);
            assert_eq!(stops.len(), 2);
            // 0deg → 0%, 90deg → 25%.
            assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
            if let Some(Length::Percent(p)) = stops[1].position {
                assert!((p - 25.0).abs() < 1e-3, "90deg should map to 25%, got {p}");
            } else {
                panic!("expected Percent position, got {:?}", stops[1].position);
            }
        }
        other => panic!("expected repeating Conic, got {other:?}"),
    }
}

#[test]
fn conic_stops_angles_converted_to_percent() {
    let stops = parse_gradient_stops("conic-gradient(red 0deg, green 180deg, blue 360deg)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(50.0)));
    if let Some(Length::Percent(p)) = stops[2].position {
        assert!((p - 100.0).abs() < 1e-3);
    } else {
        panic!("expected Percent");
    }
}

#[test]
fn conic_stops_turn_unit() {
    let stops = parse_gradient_stops("conic-gradient(red 0turn, blue 0.5turn)");
    assert_eq!(stops.len(), 2);
    if let Some(Length::Percent(p)) = stops[1].position {
        assert!((p - 50.0).abs() < 1e-3, "0.5turn should map to 50%, got {p}");
    } else {
        panic!("expected Percent");
    }
}

#[test]
fn conic_stops_percent_passthrough() {
    let stops = parse_gradient_stops("conic-gradient(red 0%, blue 25%, green 100%)");
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));
    assert_eq!(stops[1].position, Some(Length::Percent(25.0)));
    assert_eq!(stops[2].position, Some(Length::Percent(100.0)));
}

#[test]
fn conic_stops_named_colors_no_position() {
    // No explicit positions: auto-distributed by renderer; parser keeps None.
    let stops = parse_gradient_stops("conic-gradient(red, green, blue)");
    assert_eq!(stops.len(), 3);
    for s in &stops {
        assert!(s.position.is_none());
    }
}

#[test]
fn conic_from_and_at_parsed_independently() {
    use crate::style::ParsedGradient;
    // Only `at` clause, no `from`.
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(at 10% 20%, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, from_angle_deg, ..
        }) => {
            assert!((center_x_pct - 0.1).abs() < 1e-4);
            assert!((center_y_pct - 0.2).abs() < 1e-4);
            assert!(from_angle_deg.abs() < 1e-4);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn conic_from_turn_unit() {
    use crate::style::ParsedGradient;
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(from 0.25turn, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic { from_angle_deg, .. }) => {
            // 0.25turn = 90deg.
            assert!((from_angle_deg - 90.0).abs() < 1e-3, "got {from_angle_deg}");
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_image_gradient_parsed_conic_keyword_position() {
    use crate::style::ParsedGradient;
    // `at top left` → (0, 0).
    let root = lay(
        "<p>x</p>",
        "p { background-image: conic-gradient(at left top, red, blue); }",
    );
    match &first_p_style(&root).background_layers[0].image {
        BackgroundImage::Gradient(ParsedGradient::Conic {
            center_x_pct, center_y_pct, ..
        }) => {
            assert!(center_x_pct.abs() < 1e-4);
            assert!(center_y_pct.abs() < 1e-4);
        }
        other => panic!("expected Conic, got {other:?}"),
    }
}

#[test]
fn background_repeat_values() {
    for (s, expected) in [
        ("repeat", BackgroundRepeat::Repeat),
        ("no-repeat", BackgroundRepeat::NoRepeat),
        ("repeat-x", BackgroundRepeat::RepeatX),
        ("repeat-y", BackgroundRepeat::RepeatY),
        ("round", BackgroundRepeat::Round),
        ("space", BackgroundRepeat::Space),
    ] {
        let css = format!("p {{ background-repeat: {s}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).background_layers[0].repeat, expected);
    }
}

#[test]
fn background_size_keywords() {
    for (s, expected) in [
        ("auto", BackgroundSize::Auto),
        ("cover", BackgroundSize::Cover),
        ("contain", BackgroundSize::Contain),
    ] {
        let css = format!("p {{ background-size: {s}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).background_layers[0].size, expected);
    }
}

#[test]
fn background_size_length_single() {
    let root = lay("<p>x</p>", "p { background-size: 200px; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Px(200.0));
            assert_eq!(h, BgSizeAxis::Auto);
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_size_length_pair() {
    let root = lay("<p>x</p>", "p { background-size: 200px 100px; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Px(200.0));
            assert_eq!(h, BgSizeAxis::Px(100.0));
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_size_percent_pair() {
    // BUG-115: percent background-size must be preserved as Percent fractions.
    let root = lay("<p>x</p>", "p { background-size: 40% 60%; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Percent(0.4));
            assert_eq!(h, BgSizeAxis::Percent(0.6));
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_size_mixed_px_percent() {
    // BUG-115: `20px 100%` — one fixed axis, one percent axis.
    let root = lay("<p>x</p>", "p { background-size: 20px 100%; }");
    match first_p_style(&root).background_layers[0].size {
        BackgroundSize::Length(w, h) => {
            assert_eq!(w, BgSizeAxis::Px(20.0));
            assert_eq!(h, BgSizeAxis::Percent(1.0));
        }
        _ => panic!("expected Length"),
    }
}

#[test]
fn background_shorthand_percent_size() {
    // BUG-115: `background: <grad> left center / 40% 60% no-repeat` — the
    // percent size in the shorthand must reach the layer as Percent axes.
    let css = "p { background: linear-gradient(to right, #e74c3c, #c0392b) left center / 40% 60% no-repeat, #2c3e50; }";
    let root = lay("<p>x</p>", css);
    let layers = &first_p_style(&root).background_layers;
    assert_eq!(
        layers[0].size,
        BackgroundSize::Length(BgSizeAxis::Percent(0.4), BgSizeAxis::Percent(0.6))
    );
}

#[test]
fn background_attachment_values() {
    for (s, expected) in [
        ("scroll", BackgroundAttachment::Scroll),
        ("fixed", BackgroundAttachment::Fixed),
        ("local", BackgroundAttachment::Local),
    ] {
        let css = format!("p {{ background-attachment: {s}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).background_layers[0].attachment, expected);
    }
}

#[test]
fn background_properties_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { background-image: url(x.png); background-repeat: no-repeat; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // Child element has no background declarations → empty layers (initial state).
    assert!(p.style.background_layers.is_empty());
}

// ──────── place-items / align-* / justify-* (CSS Box Alignment L3) ────────

#[test]
fn align_items_center() {
    let root = lay("<p>x</p>", "p { align-items: center; }");
    assert_eq!(first_p_style(&root).align_items, AlignValue::Center);
}

#[test]
fn justify_content_space_between() {
    let root = lay("<p>x</p>", "p { justify-content: space-between; }");
    assert_eq!(first_p_style(&root).justify_content, AlignValue::SpaceBetween);
}

#[test]
fn flex_start_alias() {
    // CSS spec: flex-start alias для start (вне flex-контекста).
    let root = lay("<p>x</p>", "p { align-items: flex-start; }");
    assert_eq!(first_p_style(&root).align_items, AlignValue::Start);
}

#[test]
fn place_items_single_value() {
    let root = lay("<p>x</p>", "p { place-items: center; }");
    let s = first_p_style(&root);
    // Single value применяется к обоим осям.
    assert_eq!(s.align_items, AlignValue::Center);
    assert_eq!(s.justify_items, AlignValue::Center);
}

#[test]
fn place_items_two_values() {
    let root = lay("<p>x</p>", "p { place-items: start end; }");
    let s = first_p_style(&root);
    assert_eq!(s.align_items, AlignValue::Start);
    assert_eq!(s.justify_items, AlignValue::End);
}

#[test]
fn place_self_shorthand() {
    let root = lay("<p>x</p>", "p { place-self: center stretch; }");
    let s = first_p_style(&root);
    assert_eq!(s.align_self, AlignValue::Center);
    assert_eq!(s.justify_self, AlignValue::Stretch);
}

#[test]
fn place_content_shorthand() {
    let root = lay("<p>x</p>", "p { place-content: space-around; }");
    let s = first_p_style(&root);
    assert_eq!(s.align_content, AlignValue::SpaceAround);
    assert_eq!(s.justify_content, AlignValue::SpaceAround);
}

#[test]
fn align_unknown_value_ignored() {
    let root = lay("<p>x</p>", "p { align-items: garbage; }");
    // default (Auto) сохраняется.
    assert_eq!(first_p_style(&root).align_items, AlignValue::Auto);
}

#[test]
fn alignment_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { align-items: center; justify-content: space-between; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // У p должны быть defaults.
    assert_eq!(p.style.align_items, AlignValue::Auto);
    assert_eq!(p.style.justify_content, AlignValue::Auto);
    // У div — заданные.
    assert_eq!(div.style.align_items, AlignValue::Center);
    assert_eq!(div.style.justify_content, AlignValue::SpaceBetween);
}

#[test]
fn align_value_parse_all_keywords() {
    for (s, expected) in [
        ("auto", AlignValue::Auto),
        ("normal", AlignValue::Normal),
        ("stretch", AlignValue::Stretch),
        ("start", AlignValue::Start),
        ("end", AlignValue::End),
        ("center", AlignValue::Center),
        ("baseline", AlignValue::Baseline),
        ("space-between", AlignValue::SpaceBetween),
        ("space-around", AlignValue::SpaceAround),
        ("space-evenly", AlignValue::SpaceEvenly),
        ("flex-start", AlignValue::Start),
        ("flex-end", AlignValue::End),
        ("self-start", AlignValue::Start),
        ("CENTER", AlignValue::Center),  // case-insensitive
    ] {
        assert_eq!(AlignValue::parse(s), Some(expected), "input: {s}");
    }
}

#[test]
fn align_value_parse_unknown_returns_none() {
    assert_eq!(AlignValue::parse("garbage"), None);
    assert_eq!(AlignValue::parse(""), None);
}

#[test]
fn gap_and_aspect_ratio_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { gap: 20px; aspect-ratio: 2; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.row_gap, Length::Px(0.0));
    assert_eq!(p.style.aspect_ratio, None);
    assert_eq!(div.style.row_gap, Length::Px(20.0));
    assert!(div.style.aspect_ratio.is_some());
}

#[test]
fn media_prefers_color_scheme_light_default() {
    // Phase 0: prefers_dark=false → 'light' matches.
    let root = lay_with_viewport(
        "<p>x</p>",
        "@media (prefers-color-scheme: light) { p { color: red; } }",
        800.0,
        600.0,
    );
    let p = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

// ── CSS Quirks Mode — UA-rule для <table> ──────────────────────────────

/// В Quirks-mode (нет DOCTYPE) `<table>` сбрасывает font-size к
/// initial-значению, не наследует от родителя.
#[test]
fn quirks_table_font_size_resets_to_initial() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (body.style.font_size - 30.0).abs() < 0.01,
        "body должен наследовать заявленные 30px"
    );
    assert!(
        (table.style.font_size - 16.0).abs() < 0.01,
        "table в Quirks должен сбросить font-size к initial 16, получено {}",
        table.style.font_size
    );
}

/// В Standards mode (`<!DOCTYPE html>`) `<table>` наследует font-size
/// от родителя как обычный элемент.
#[test]
fn standards_table_font_size_inherits() {
    let root = lay(
        "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (table.style.font_size - 30.0).abs() < 0.01,
        "table в Standards должен наследовать 30px, получено {}",
        table.style.font_size
    );
}

/// В Quirks color у `<table>` сбрасывается к BLACK, не наследуется.
#[test]
fn quirks_table_color_resets_to_black() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { color: red; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(table.style.color, Color::BLACK);
}

/// В Standards color наследуется.
#[test]
fn standards_table_color_inherits() {
    let root = lay(
        "<!DOCTYPE html><body><table><tr><td>x</td></tr></table></body>",
        "body { color: red; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// В Quirks font-weight у `<table>` сбрасывается к NORMAL.
#[test]
fn quirks_table_font_weight_resets_to_normal() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-weight: bold; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.font_weight, FontWeight::BOLD);
    assert_eq!(table.style.font_weight, FontWeight::NORMAL);
}

/// В Quirks font-style у `<table>` сбрасывается к Normal.
#[test]
fn quirks_table_font_style_resets_to_normal() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-style: italic; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.font_style, FontStyle::Italic);
    assert_eq!(table.style.font_style, FontStyle::Normal);
}

/// В Quirks text-align у `<table>` сбрасывается к initial (Start).
#[test]
fn quirks_table_text_align_resets_to_left() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { text-align: center; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.text_align, TextAlign::Center);
    assert_eq!(table.style.text_align, TextAlign::Start);
}

/// В Quirks white-space у `<table>` сбрасывается к Normal.
#[test]
fn quirks_table_white_space_resets_to_normal() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { white-space: nowrap; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert_eq!(body.style.white_space, WhiteSpace::Nowrap);
    assert_eq!(table.style.white_space, WhiteSpace::Normal);
}

/// Author CSS поверх Quirks-reset выигрывает: spec-rule идёт как
/// низший cascade origin (UA).
#[test]
fn quirks_table_author_css_wins_over_reset() {
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; } table { font-size: 24px; color: blue; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (table.style.font_size - 24.0).abs() < 0.01,
        "author CSS должен переопределить Quirks-reset"
    );
    assert_eq!(table.style.color, Color { r: 0, g: 0, b: 255, a: 255 });
}

/// Дочерние элементы `<table>` в Quirks наследуют от сброшенных
/// значений таблицы, не от прародителя.
#[test]
fn quirks_table_children_inherit_reset_values() {
    // <body>=30px → <table>=16 (reset) → <td>=16 (inherits from table).
    let root = lay(
        "<body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; }",
    );
    let body = &root;
    let table = first_element_child(body);
    // HTML5 parser inserts implicit <tbody>: table → tbody → tr → td.
    // Идём вглубь, пока не найдём td (Block inside a TableRow).
    fn find_td(b: &LayoutBox) -> Option<&LayoutBox> {
        for c in &b.children {
            if matches!(&c.kind, BoxKind::TableRow | BoxKind::TableRowGroup) {
                if let Some(td) = find_td(c) {
                    return Some(td);
                }
            } else if matches!(&c.kind, BoxKind::Block) {
                if let Some(td) = find_td(c) {
                    return Some(td);
                }
                return Some(c);
            }
        }
        None
    }
    let td = find_td(table).expect("td не найден");
    assert!(
        (td.style.font_size - 16.0).abs() < 0.01,
        "td должен унаследовать от table сброшенные 16px, получено {}",
        td.style.font_size
    );
}

/// Не-`<table>` элементы в Quirks-mode не сбрасывают inherited.
#[test]
fn quirks_non_table_inherits_normally() {
    let root = lay(
        "<body><p>x</p></body>",
        "body { font-size: 30px; color: red; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert!(
        (p.style.font_size - 30.0).abs() < 0.01,
        "<p> в Quirks-mode должен наследовать font-size, получено {}",
        p.style.font_size
    );
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// LimitedQuirks (HTML 4.01 Transitional) — table-reset не применяется
/// (spec §4.1: только в Quirks-mode).
#[test]
fn limited_quirks_does_not_apply_table_reset() {
    let root = lay(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><table><tr><td>x</td></tr></table></body>",
        "body { font-size: 30px; color: red; }",
    );
    let body = &root;
    let table = first_element_child(body);
    assert!(
        (table.style.font_size - 30.0).abs() < 0.01,
        "table в LimitedQuirks должен наследовать font-size как в Standards"
    );
    assert_eq!(table.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

// ── CSS Quirks Mode §3.4 — «hashless hex color quirk» ──────────────────

/// В Quirks-mode `color: ff0000` (без `#`) парсится как red.
/// Это эквивалент `color: #ff0000` (CSS Quirks Mode §3.4).
#[test]
fn quirks_hashless_hex_in_color_property() {
    // Нет DOCTYPE → Quirks.
    let root = lay(
        "<body><p>x</p></body>",
        "p { color: ff0000; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// В Standards-mode `color: ff0000` (без `#`) — невалидное значение,
/// игнорируется. Цвет наследуется (по умолчанию BLACK).
#[test]
fn standards_hashless_hex_rejected_in_color_property() {
    let root = lay(
        "<!DOCTYPE html><body><p>x</p></body>",
        "p { color: ff0000; }",
    );
    let body = &root;
    let p = first_element_child(body);
    // ff0000 без `#` — невалидно в Standards, color остаётся inherited
    // от body (BLACK).
    assert_eq!(p.style.color, Color::BLACK);
}

/// В Quirks `background-color: 00ff00` (6-hex без `#`) парсится как green.
#[test]
fn quirks_hashless_hex_in_background_color() {
    let root = lay(
        "<body><p>x</p></body>",
        "p { background-color: 00ff00; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(p.style.background_color, Some(CssColor::Rgba(Color { r: 0, g: 255, b: 0, a: 255 })));
}

/// В Quirks 3-hex bare digit-ы тоже парсятся: `f00` → red.
#[test]
fn quirks_hashless_hex_3_digit_short() {
    let root = lay(
        "<body><p>x</p></body>",
        "p { color: f00; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(p.style.color, Color { r: 255, g: 0, b: 0, a: 255 });
}

/// В Quirks border-color принимает bare hex.
#[test]
fn quirks_hashless_hex_in_border_color() {
    let root = lay(
        "<body><p>x</p></body>",
        "p { border: 1px solid 0000ff; }",
    );
    let body = &root;
    let p = first_element_child(body);
    assert_eq!(
        p.style.border_top_color,
        CssColor::Rgba(Color { r: 0, g: 0, b: 255, a: 255 }),
    );
}

/// LimitedQuirks (HTML 4.01 Transitional) — hashless hex quirk
/// НЕ применяется (spec §1.1.1: «full quirks mode only»).
#[test]
fn limited_quirks_hashless_hex_rejected() {
    let root = lay(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\"><body><p>x</p></body>",
        "p { color: ff0000; }",
    );
    let body = &root;
    let p = first_element_child(body);
    // В LimitedQuirks bare hex — invalid, как в Standards.
    assert_eq!(p.style.color, Color::BLACK);
}

// ──────────────── CSS Quirks Mode §3.5 — html viewport height ────────────────

/// В quirks-mode `<html>` получает UA-правило `height: 100vh`, поэтому
/// его rect.height равен высоте viewport (600.0 в тестовом lay).
#[test]
fn quirks_html_height_equals_viewport() {
    let root = lay_full("<html><body><p>x</p></body></html>", "");
    let (html, _body) = html_and_body(&root);
    assert!(
        (html.rect.height - 600.0).abs() < 0.1,
        "quirks: html.rect.height={} (ожидалось 600.0)",
        html.rect.height
    );
}

/// В quirks-mode `body { height: 100% }` резолвится против viewport
/// через html-box с высотой 100vh.
#[test]
fn quirks_body_height_100pct_resolves_to_viewport() {
    let root = lay_full(
        "<html><body></body></html>",
        "body { height: 100%; }",
    );
    let (_html, body) = html_and_body(&root);
    assert!(
        (body.rect.height - 600.0).abs() < 0.1,
        "quirks: body.rect.height={} (ожидалось 600.0)",
        body.rect.height
    );
}

/// В standards-mode (с `<!DOCTYPE html>`) `<html>` с высотой auto
/// НЕ получает 100vh — высота определяется контентом (маленькая).
#[test]
fn standards_html_height_is_content_not_viewport() {
    let root = lay_full(
        "<!DOCTYPE html><html><body><p style=\"height:20px\">x</p></body></html>",
        "",
    );
    let (html, _body) = html_and_body(&root);
    // Контент высотой 20px + margins body → html значительно < 600.
    assert!(
        html.rect.height < 200.0,
        "standards: html.rect.height={} (ожидалось меньше 200.0)",
        html.rect.height
    );
}

/// В quirks-mode author CSS на `<html>` перекрывает UA-правило 100vh.
#[test]
fn quirks_html_author_height_overrides_ua_rule() {
    let root = lay_full(
        "<html><body></body></html>",
        "html { height: 300px; }",
    );
    let (html, _body) = html_and_body(&root);
    assert!(
        (html.rect.height - 300.0).abs() < 0.1,
        "quirks: author height=300px, html.rect.height={} (ожидалось 300.0)",
        html.rect.height
    );
}

/// В limited-quirks mode (HTML 4.01 Transitional + system_id) правило
/// §3.5 НЕ применяется — только full quirks mode.
#[test]
fn limited_quirks_html_height_is_content_not_viewport() {
    let root = lay_full(
        "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \
         \"http://www.w3.org/TR/html4/loose.dtd\">\
         <html><body><p style=\"height:20px\">x</p></body></html>",
        "",
    );
    let (html, _body) = html_and_body(&root);
    assert!(
        html.rect.height < 200.0,
        "limited-quirks: html.rect.height={} (ожидалось меньше 200.0)",
        html.rect.height
    );
}

// ──────────────── :fullscreen / :modal / :popover-open (open-state pseudo-classes) ────────────────

/// `:fullscreen` (Fullscreen API §4.2) — Phase 0 без runtime top-layer
/// никакой элемент не считается fullscreen, правило не применяется.
#[test]
fn fullscreen_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        "<div>x</div>",
        "div:fullscreen { color: red; }",
        "div",
    );
    assert_eq!(c.r, 0);
}

/// `:fullscreen` не активируется даже на дочернем элементе с
/// контейнером — top-layer state runtime-only.
#[test]
fn fullscreen_pseudo_never_matches_nested() {
    let c = element_color(
        "<div><p>x</p></div>",
        "p:fullscreen { color: red; }",
        "p",
    );
    assert_eq!(c.r, 0);
}

/// `:modal` (CSS Selectors L4 §16.5.2) — Phase 0 без dialog runtime.
/// `<dialog open>` НЕ модален: атрибут `open` ставится и через
/// `dialog.show()` (non-modal), поэтому простая DOM-проверка не покрыла
/// бы spec — matcher всегда `false`.
#[test]
fn modal_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        "<dialog open>x</dialog>",
        "dialog:modal { color: red; }",
        "dialog",
    );
    assert_eq!(c.r, 0);
}

/// `:modal` не активируется и без атрибута `open`.
#[test]
fn modal_pseudo_never_matches_closed_dialog() {
    let c = element_color(
        "<dialog>x</dialog>",
        "dialog:modal { color: red; }",
        "dialog",
    );
    assert_eq!(c.r, 0);
}

/// `:popover-open` (HTML LS §6.12.2) — Phase 0 без Popover API runtime.
/// Наличие атрибута `popover` декларирует тип, но не открытое состояние.
#[test]
fn popover_open_pseudo_never_matches_in_phase_0() {
    let c = element_color(
        r#"<div popover="auto">x</div>"#,
        "div:popover-open { color: red; }",
        "div",
    );
    assert_eq!(c.r, 0);
}

/// `:popover-open` не матчит и при отсутствии `popover`-атрибута.
#[test]
fn popover_open_pseudo_never_matches_non_popover() {
    let c = element_color(
        "<div>x</div>",
        "div:popover-open { color: red; }",
        "div",
    );
    assert_eq!(c.r, 0);
}

/// Specificity открытых-состояния pseudo-классов — class-уровня (0,1,0).
/// `:not(:fullscreen)` через always-false означает «всегда true» — это
/// удобный FOUC-protection idiom (если когда-нибудь fullscreen runtime
/// появится, правило сбросится). Проверяем, что `:not(:fullscreen)`
/// действительно матчит обычный element.
#[test]
fn not_fullscreen_matches_all_elements_in_phase_0() {
    let c = element_color(
        "<div>x</div>",
        "div:not(:fullscreen) { color: red; }",
        "div",
    );
    assert_eq!(c.r, 255);
}

/// То же для `:not(:modal)`: элементы не в modal state — все элементы
/// в Phase 0.
#[test]
fn not_modal_matches_all_elements_in_phase_0() {
    let c = element_color(
        "<dialog open>x</dialog>",
        "dialog:not(:modal) { color: red; }",
        "dialog",
    );
    assert_eq!(c.r, 255);
}

/// То же для `:not(:popover-open)`.
#[test]
fn not_popover_open_matches_all_elements_in_phase_0() {
    let c = element_color(
        r#"<div popover="auto">x</div>"#,
        "div:not(:popover-open) { color: red; }",
        "div",
    );
    assert_eq!(c.r, 255);
}

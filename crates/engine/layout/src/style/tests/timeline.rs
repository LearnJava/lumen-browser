//! Тесты `style.rs`: анимации и переходы: шортхенды `animation`/`transition`,
//! функции смягчения.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ──────────────── CSS Easing L2 §2.4: linear(<linear-stop-list>) ────────────────

    fn extract_linear_stops(tf: TimingFunction) -> Vec<LinearEasingPoint> {
        match tf {
            TimingFunction::LinearStops(p) => p,
            other => panic!("expected LinearStops, got {other:?}"),
        }
    }

    #[test]
    fn linear_stops_parse_two_endpoints() {
        // linear(0, 1) — два endpoint-а без percentage; распределяются 0 и 1.
        let pts = extract_linear_stops(TimingFunction::parse("linear(0, 1)").unwrap());
        assert_eq!(pts.len(), 2);
        assert!(approx(pts[0].input, 0.0));
        assert!(approx(pts[0].output, 0.0));
        assert!(approx(pts[1].input, 1.0));
        assert!(approx(pts[1].output, 1.0));
    }

    #[test]
    fn linear_stops_parse_three_evenly_distributed() {
        // linear(0, 0.5, 1) — три точки, средняя без percentage → input=0.5.
        let pts = extract_linear_stops(TimingFunction::parse("linear(0, 0.5, 1)").unwrap());
        assert_eq!(pts.len(), 3);
        assert!(approx(pts[1].input, 0.5));
        assert!(approx(pts[1].output, 0.5));
    }

    #[test]
    fn linear_stops_parse_explicit_percentage() {
        // linear(0, 0.25 75%, 1) — средняя точка явно на 75%.
        let pts = extract_linear_stops(TimingFunction::parse("linear(0, 0.25 75%, 1)").unwrap());
        assert_eq!(pts.len(), 3);
        assert!(approx(pts[1].input, 0.75));
        assert!(approx(pts[1].output, 0.25));
    }

    #[test]
    fn linear_stops_parse_two_lengths_makes_jump() {
        // linear(0 0% 50%, 1 50% 100%) — два stop-а, каждый с двумя
        // percentage-ами → 4 точки; разрыв при input=0.5 (output 0 → 1).
        let pts = extract_linear_stops(
            TimingFunction::parse("linear(0 0% 50%, 1 50% 100%)").unwrap(),
        );
        assert_eq!(pts.len(), 4);
        assert!(approx(pts[0].input, 0.0));
        assert!(approx(pts[0].output, 0.0));
        assert!(approx(pts[1].input, 0.5));
        assert!(approx(pts[1].output, 0.0));
        assert!(approx(pts[2].input, 0.5));
        assert!(approx(pts[2].output, 1.0));
        assert!(approx(pts[3].input, 1.0));
        assert!(approx(pts[3].output, 1.0));
    }

    #[test]
    fn linear_stops_parse_invalid_single_stop_is_none() {
        // linear() с < 2 stop-ами — invalid (§2.5.1 step 3).
        assert!(TimingFunction::parse("linear(0.5)").is_none());
        assert!(TimingFunction::parse("linear()").is_none());
    }

    #[test]
    fn linear_stops_parse_monotonicity_clamps_decreasing_inputs() {
        // §2.5.1 step 4.iii: input clamp-ается до largest_input. Если в исходнике
        // 75% идёт перед 50%, второй stop поднимается до 75%.
        let pts = extract_linear_stops(
            TimingFunction::parse("linear(0, 0.5 75%, 1 50%)").unwrap(),
        );
        assert_eq!(pts.len(), 3);
        assert!(approx(pts[1].input, 0.75));
        assert!(approx(pts[2].input, 0.75));
    }

    #[test]
    fn linear_stops_progress_identity_equivalent_to_linear() {
        // linear(0, 1) поведенчески = Linear.
        let f = TimingFunction::parse("linear(0, 1)").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.25), 0.25));
        assert!(approx(f.progress(0.5), 0.5));
        assert!(approx(f.progress(0.75), 0.75));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn linear_stops_progress_piecewise() {
        // linear(0, 0.25 75%, 1): на [0, 0.75] — наклон 0.25/0.75 ≈ 0.333;
        // на [0.75, 1] — наклон 0.75/0.25 = 3.
        let f = TimingFunction::parse("linear(0, 0.25 75%, 1)").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.375), 0.125));
        assert!(approx(f.progress(0.75), 0.25));
        assert!(approx(f.progress(0.875), 0.625));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn linear_stops_progress_overshoot_allowed() {
        // Output может выходить за [0, 1] — overshoot easing.
        let f = TimingFunction::parse("linear(0, 1.5 50%, 1)").unwrap();
        assert!(approx(f.progress(0.5), 1.5));
        assert!(approx(f.progress(0.25), 0.75));
        assert!(approx(f.progress(0.75), 1.25));
    }

    #[test]
    fn linear_stops_progress_jump_at_discontinuity() {
        // linear(0 0% 50%, 1 50% 100%): output=0 на [0, 0.5), output=1 на [0.5, 1].
        let f = TimingFunction::parse("linear(0 0% 50%, 1 50% 100%)").unwrap();
        assert!(approx(f.progress(0.0), 0.0));
        assert!(approx(f.progress(0.49), 0.0));
        // На самой границе скачка first-match выбирает левую пару → 0
        // (выбор не виден в анимации — discontinuity).
        assert!(approx(f.progress(0.51), 1.0));
        assert!(approx(f.progress(1.0), 1.0));
    }

    #[test]
    fn linear_stops_progress_clamps_inputs_out_of_range() {
        // CSS Easing §2: t вне [0, 1] клампится. linear() не исключение.
        let f = TimingFunction::parse("linear(0, 1)").unwrap();
        assert!(approx(f.progress(-1.0), 0.0));
        assert!(approx(f.progress(2.0), 1.0));
    }

    #[test]
    fn linear_stops_distributes_run_of_missing_inputs() {
        // linear(0, 0.25, 0.5, 0.75, 1): первая=0, последняя=1, три средние
        // равномерно распределяются → 0.25, 0.5, 0.75.
        let pts = extract_linear_stops(
            TimingFunction::parse("linear(0, 0.25, 0.5, 0.75, 1)").unwrap(),
        );
        assert_eq!(pts.len(), 5);
        for (i, expected) in [0.0, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
            assert!(approx(pts[i].input, *expected), "point {i} input mismatch");
        }
    }

    #[test]
    fn linear_stops_parse_in_animation_shorthand() {
        // linear(...) должна корректно распознаваться внутри animation-timing-function.
        let tfs = TimingFunction::parse_list("linear(0, 0.5, 1), ease-in");
        assert_eq!(tfs.len(), 2);
        assert!(matches!(tfs[0], TimingFunction::LinearStops(_)));
        assert_eq!(tfs[1], TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0));
    }

    // === animation shorthand parsing (CSS Animations L1 §4) ===

    fn shorthand(val: &str) -> ComputedStyle {
        let mut s = ComputedStyle::root();
        apply_animation_shorthand(&mut s, val);
        s
    }

    #[test]
    fn shorthand_single_name_only() {
        let s = shorthand("slidein");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert_eq!(s.animation_durations, vec![0.0]);
        assert_eq!(s.animation_delays, vec![0.0]);
        assert_eq!(s.animation_timing_functions.len(), 1);
        assert_eq!(s.animation_iteration_counts, vec![IterationCount::Finite(1.0)]);
        assert_eq!(s.animation_directions, vec![AnimationDirection::Normal]);
        assert_eq!(s.animation_fill_modes, vec![AnimationFillMode::None]);
        assert_eq!(s.animation_play_states, vec![AnimationPlayState::Running]);
    }

    #[test]
    fn shorthand_duration_then_name() {
        // Самый частый кейс в реальном CSS.
        let s = shorthand("2s slidein");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn shorthand_duration_easing_name() {
        let s = shorthand("2s linear slidein");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(s.animation_timing_functions[0], TimingFunction::Linear);
    }

    #[test]
    fn shorthand_two_times_duration_and_delay() {
        // Первое <time> = duration, второе = delay.
        let s = shorthand("2s 0.5s slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.5).abs() < 1e-4);
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
    }

    #[test]
    fn shorthand_negative_delay_allowed() {
        // Spec: negative delay = «анимация началась в прошлом».
        let s = shorthand("2s -0.5s slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - -0.5).abs() < 1e-4);
    }

    #[test]
    fn shorthand_ms_units() {
        let s = shorthand("500ms 100ms slidein");
        assert!((s.animation_durations[0] - 0.5).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.1).abs() < 1e-4);
    }

    #[test]
    fn shorthand_full_form_in_canonical_order() {
        // duration, easing, delay, iter-count, direction, fill-mode, play-state, name.
        let s = shorthand("2s ease-in 1s 3 alternate forwards paused slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0)
        );
        assert!((s.animation_delays[0] - 1.0).abs() < 1e-4);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(3.0));
        assert_eq!(s.animation_directions[0], AnimationDirection::Alternate);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Forwards);
        assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
    }

    #[test]
    fn shorthand_any_order() {
        // `||` operator — токены могут идти в любом порядке.
        let s = shorthand("slidein alternate-reverse 1.5s infinite ease-out");
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
        assert_eq!(
            s.animation_directions[0],
            AnimationDirection::AlternateReverse
        );
        assert!((s.animation_durations[0] - 1.5).abs() < 1e-4);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Infinite);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0)
        );
    }

    #[test]
    fn shorthand_cubic_bezier_with_spaces_inside() {
        // Tokenizer должен трактовать `cubic-bezier(0.42, 0, 0.58, 1)` как
        // один токен, несмотря на запятые/пробелы внутри.
        let s = shorthand("2s cubic-bezier(0.42, 0, 0.58, 1) slidein");
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0)
        );
        assert_eq!(s.animation_names, vec!["slidein".to_string()]);
    }

    #[test]
    fn shorthand_steps_with_args() {
        let s = shorthand("1s steps(4, end) slidein");
        assert!((s.animation_durations[0] - 1.0).abs() < 1e-4);
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::Steps(4, StepPosition::JumpEnd)
        );
    }

    #[test]
    fn shorthand_multiple_layers() {
        // Comma-list: 2 layers, каждый со своим набором.
        let s = shorthand("2s slidein, 3s linear slideout");
        assert_eq!(s.animation_names, vec!["slidein".to_string(), "slideout".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_durations[1] - 3.0).abs() < 1e-4);
        assert_eq!(s.animation_timing_functions[1], TimingFunction::Linear);
        // Layer 1 timing — default (ease).
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
    }

    #[test]
    fn shorthand_three_layers_parallel_lengths() {
        // Все 8 Vec-ов должны иметь одинаковую длину = числу layer-ов.
        let s = shorthand("1s a, 2s b, 3s c");
        assert_eq!(s.animation_names.len(), 3);
        assert_eq!(s.animation_durations.len(), 3);
        assert_eq!(s.animation_timing_functions.len(), 3);
        assert_eq!(s.animation_delays.len(), 3);
        assert_eq!(s.animation_iteration_counts.len(), 3);
        assert_eq!(s.animation_directions.len(), 3);
        assert_eq!(s.animation_fill_modes.len(), 3);
        assert_eq!(s.animation_play_states.len(), 3);
    }

    #[test]
    fn shorthand_none_keyword() {
        // `animation: none` — single layer, `none` падает в fill-mode-slot.
        // Имя остаётся пустым → consumer (animation scheduler) skip-нет.
        let s = shorthand("none");
        assert_eq!(s.animation_names, vec![String::new()]);
        assert_eq!(s.animation_fill_modes, vec![AnimationFillMode::None]);
    }

    #[test]
    fn shorthand_iteration_count_number() {
        let s = shorthand("2s 5 slidein");
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(5.0));
    }

    #[test]
    fn shorthand_iteration_count_infinite() {
        let s = shorthand("2s infinite slidein");
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Infinite);
    }

    #[test]
    fn shorthand_iteration_count_fractional() {
        let s = shorthand("2s 2.5 slidein");
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(2.5));
    }

    #[test]
    fn shorthand_resets_previously_set_longhands() {
        // CSS Cascade L4 §6.2: shorthand сбрасывает ВСЕ longhand-ы к их
        // initial-value, если они не упомянуты в shorthand-е.
        let mut s = ComputedStyle::root();
        s.animation_delays = vec![5.0, 10.0];
        s.animation_fill_modes = vec![AnimationFillMode::Forwards];
        s.animation_directions = vec![AnimationDirection::Reverse];
        apply_animation_shorthand(&mut s, "2s slidein");
        // duration упомянут → 2s. delay/fill/direction не упомянуты → initial.
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.0).abs() < 1e-4);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::None);
        assert_eq!(s.animation_directions[0], AnimationDirection::Normal);
    }

    #[test]
    fn shorthand_empty_value_clears_all() {
        // Пустое значение → нет layer-ов → все Vec-и пустые.
        let s = shorthand("");
        assert!(s.animation_names.is_empty());
        assert!(s.animation_durations.is_empty());
        assert!(s.animation_timing_functions.is_empty());
        assert!(s.animation_delays.is_empty());
        assert!(s.animation_iteration_counts.is_empty());
        assert!(s.animation_directions.is_empty());
        assert!(s.animation_fill_modes.is_empty());
        assert!(s.animation_play_states.is_empty());
    }

    #[test]
    fn shorthand_only_keywords_no_name() {
        // Если имя не указано, name остаётся пустым.
        let s = shorthand("2s linear forwards");
        assert_eq!(s.animation_names, vec![String::new()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert_eq!(s.animation_timing_functions[0], TimingFunction::Linear);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Forwards);
    }

    #[test]
    fn shorthand_step_start_keyword() {
        let s = shorthand("0.5s step-start slidein");
        assert_eq!(
            s.animation_timing_functions[0],
            TimingFunction::Steps(1, StepPosition::JumpStart)
        );
    }

    #[test]
    fn shorthand_paused_play_state() {
        let s = shorthand("2s paused slidein");
        assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
    }

    #[test]
    fn shorthand_reverse_direction() {
        let s = shorthand("2s reverse slidein");
        assert_eq!(s.animation_directions[0], AnimationDirection::Reverse);
    }

    #[test]
    fn shorthand_both_fill_mode() {
        let s = shorthand("2s both slidein");
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Both);
    }

    #[test]
    fn shorthand_through_apply_declaration() {
        // Полная цепочка: Declaration → apply_declaration. Sanity-check
        // что branch в match подхватывает shorthand.
        let mut s = ComputedStyle::root();
        let viewport = Size {
            width: 1024.0,
            height: 768.0,
        };
        let inherited = ComputedStyle::root();
        let decl = Declaration {
            property: "animation".to_string(),
            value: "2s ease-in-out 0.5s 2 alternate forwards paused fade".to_string(),
            important: false,
        };
        apply_declaration(&mut s, &decl, 16.0, viewport, FontWeight::default(), &inherited, &inherited, false, false);
        assert_eq!(s.animation_names, vec!["fade".to_string()]);
        assert!((s.animation_durations[0] - 2.0).abs() < 1e-4);
        assert!((s.animation_delays[0] - 0.5).abs() < 1e-4);
        assert_eq!(s.animation_iteration_counts[0], IterationCount::Finite(2.0));
        assert_eq!(s.animation_directions[0], AnimationDirection::Alternate);
        assert_eq!(s.animation_fill_modes[0], AnimationFillMode::Forwards);
        assert_eq!(s.animation_play_states[0], AnimationPlayState::Paused);
    }

    #[test]
    fn shorthand_tokenize_with_parens_handles_nested() {
        // Sanity-check helper: вложенные скобки не разбиваются на пробелах.
        let tokens = tokenize_with_parens("a cubic-bezier(0.1, 0.2, 0.3, 0.4) b");
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], "a");
        // Внутри скобок пробелы и запятые сохраняются — это один токен.
        assert_eq!(tokens[1], "cubic-bezier(0.1, 0.2, 0.3, 0.4)");
        assert_eq!(tokens[2], "b");
    }

    // === transition shorthand parsing (CSS Transitions L1 §3) ===

    fn ts(val: &str) -> ComputedStyle {
        let mut s = ComputedStyle::root();
        apply_transition_shorthand(&mut s, val);
        s
    }

    #[test]
    fn transition_shorthand_duration_only() {
        // `transition: 1s` → property = initial "all".
        let s = ts("1s");
        assert_eq!(s.transition_properties, vec!["all".to_string()]);
        assert!((s.transition_durations[0] - 1.0).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.0).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_property_and_duration() {
        let s = ts("opacity 0.3s");
        assert_eq!(s.transition_properties, vec!["opacity".to_string()]);
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_full_form() {
        let s = ts("opacity 0.3s ease-out 0.1s");
        assert_eq!(s.transition_properties, vec!["opacity".to_string()]);
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.1).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.0, 0.0, 0.58, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_any_order() {
        // Per spec — `||` оператор, любой порядок.
        let s = ts("ease-in 0.5s transform 0.2s");
        assert_eq!(s.transition_properties, vec!["transform".to_string()]);
        assert!((s.transition_durations[0] - 0.5).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.2).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_ms_units() {
        let s = ts("opacity 200ms 50ms");
        assert!((s.transition_durations[0] - 0.2).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.05).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_multiple_layers() {
        let s = ts("opacity 0.3s, transform 0.5s ease-in");
        assert_eq!(
            s.transition_properties,
            vec!["opacity".to_string(), "transform".to_string()]
        );
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
        assert!((s.transition_durations[1] - 0.5).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[1],
            TimingFunction::CubicBezier(0.42, 0.0, 1.0, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_three_layers_parallel_lengths() {
        // Все 4 Vec-а должны иметь длину = числу layers.
        let s = ts("opacity 1s, transform 2s linear, color 3s ease-in 0.5s");
        assert_eq!(s.transition_properties.len(), 3);
        assert_eq!(s.transition_durations.len(), 3);
        assert_eq!(s.transition_timing_functions.len(), 3);
        assert_eq!(s.transition_delays.len(), 3);
    }

    #[test]
    fn transition_shorthand_none_layer() {
        // `transition: none` — single layer, property=none, остальное — initial.
        let s = ts("none");
        assert_eq!(s.transition_properties, vec!["none".to_string()]);
        assert!((s.transition_durations[0] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_cubic_bezier_with_spaces_inside() {
        let s = ts("opacity 0.5s cubic-bezier(0.1, 0.2, 0.3, 0.4)");
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.1, 0.2, 0.3, 0.4)
        );
    }

    #[test]
    fn transition_shorthand_steps_with_args() {
        let s = ts("opacity 1s steps(4, end)");
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::Steps(4, StepPosition::JumpEnd)
        );
    }

    #[test]
    fn transition_shorthand_resets_previously_set_longhands() {
        // CSS Cascade L4 §6.2: shorthand сбрасывает longhand-ы к initial.
        let mut s = ComputedStyle::root();
        s.transition_durations = vec![5.0, 10.0];
        s.transition_delays = vec![1.0, 2.0];
        s.transition_timing_functions = vec![TimingFunction::Linear];
        apply_transition_shorthand(&mut s, "opacity 0.3s");
        assert!((s.transition_durations[0] - 0.3).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.0).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.25, 0.1, 0.25, 1.0)
        );
        assert_eq!(s.transition_durations.len(), 1);
        assert_eq!(s.transition_delays.len(), 1);
        assert_eq!(s.transition_timing_functions.len(), 1);
    }

    #[test]
    fn transition_shorthand_empty_value_clears_all() {
        let s = ts("");
        assert!(s.transition_properties.is_empty());
        assert!(s.transition_durations.is_empty());
        assert!(s.transition_timing_functions.is_empty());
        assert!(s.transition_delays.is_empty());
    }

    #[test]
    fn transition_shorthand_only_timing() {
        // `transition: linear` — property=all (initial), duration=0.
        let s = ts("linear");
        assert_eq!(s.transition_properties, vec!["all".to_string()]);
        assert_eq!(s.transition_timing_functions[0], TimingFunction::Linear);
        assert!((s.transition_durations[0] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_step_start_keyword() {
        let s = ts("opacity 0.5s step-start");
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::Steps(1, StepPosition::JumpStart)
        );
    }

    #[test]
    fn transition_shorthand_through_apply_declaration() {
        // Полная цепочка: Declaration → apply_declaration.
        let mut s = ComputedStyle::root();
        let viewport = Size {
            width: 1024.0,
            height: 768.0,
        };
        let inherited = ComputedStyle::root();
        let decl = Declaration {
            property: "transition".to_string(),
            value: "transform 0.4s ease-in-out 0.1s".to_string(),
            important: false,
        };
        apply_declaration(&mut s, &decl, 16.0, viewport, FontWeight::default(), &inherited, &inherited, false, false);
        assert_eq!(s.transition_properties, vec!["transform".to_string()]);
        assert!((s.transition_durations[0] - 0.4).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.1).abs() < 1e-4);
        assert_eq!(
            s.transition_timing_functions[0],
            TimingFunction::CubicBezier(0.42, 0.0, 0.58, 1.0)
        );
    }

    #[test]
    fn transition_shorthand_negative_delay_allowed() {
        // CSS Transitions L1 §3: negative delay допустим — анимация
        // начинается с прогрессом, как будто уже игралась.
        let s = ts("opacity 1s -0.2s");
        assert!((s.transition_durations[0] - 1.0).abs() < 1e-4);
        assert!((s.transition_delays[0] - (-0.2)).abs() < 1e-4);
    }

    #[test]
    fn transition_shorthand_two_times_duration_and_delay() {
        // 1s сначала = duration, 0.5s потом = delay.
        let s = ts("1s 0.5s");
        assert!((s.transition_durations[0] - 1.0).abs() < 1e-4);
        assert!((s.transition_delays[0] - 0.5).abs() < 1e-4);
    }

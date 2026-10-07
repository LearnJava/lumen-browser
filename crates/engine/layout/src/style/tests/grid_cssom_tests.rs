//! Тесты `canonical_specified_grid` — строгая грамматика grid-свойств в inline-`style` CSSOM (BUG-1315).

    use crate::style::canonical_specified_grid;

    fn ok(prop: &str, v: &str) {
        assert!(canonical_specified_grid(prop, v).is_some(), "{prop}: {v:?} должно быть валидным");
    }

    fn bad(prop: &str, v: &str) {
        assert!(canonical_specified_grid(prop, v).is_none(), "{prop}: {v:?} должно быть отклонено");
    }

    #[test]
    fn valid_value_is_returned_trimmed() {
        assert_eq!(canonical_specified_grid("grid-template-columns", "  10px 1fr ").as_deref(), Some("10px 1fr"));
        assert_eq!(canonical_specified_grid("width", " -10px ").as_deref(), Some("-10px"));
    }

    #[test]
    fn track_lists() {
        for v in [
            "none", "10px", "20%", "5fr", "auto", "min-content", "calc(0.5em + 10px)", "fit-content(20%)",
            "minmax(10px, auto)", "minmax(auto, calc(0.5em + 10px))", "[] 150px [] 1fr []",
            "repeat(2, [a] 10px [b] 1fr)", "repeat(auto-fill, minmax(30px, 5fr) [two])",
            "[one] repeat(2, minmax(10px, auto)) [two] 30px repeat(auto-fill, 10px) 40px",
            "repeat(auto-fit, auto 100px auto)", "repeat(auto-fill, fit-content(200px))",
            "subgrid", "subgrid [a] [b]", "subgrid [a] repeat(2, [b] [c])", "subgrid repeat(auto-fill, [a])",
            "REPEAT(2, 1FR)",
        ] {
            ok("grid-template-columns", v);
            ok("grid-template-rows", v);
        }
        for v in [
            "-10px", "-20%", "-5fr", "10pxx", "minmax(5fr, 1px)", "minmax(-10px, auto)", "minmax(1px)",
            "fit-content(-10px)", "fit-content", "[one]", "[one] 10px [two] [three]", "[auto] 1px",
            "repeat(20%)", "repeat(0, 10px)", "repeat(2,", "repeat(auto-fill, -10px)", "repeat(auto-fill, 1fr)",
            "repeat(auto-fill, 10px) repeat(auto-fit, 20%)", "repeat(auto-fill, fit-content)",
            "auto repeat(auto-fill, auto) auto", "-5fr repeat(auto-fill, auto)",
            "repeat(auto-fill, minmax(fit-content(200px), auto))", "repeat(2, repeat(2, 10px))",
            "subgrid subgrid", "subgrid none", "subgrid 1px", "subgrid [a] 1px", "subgrid repeat(2, 1px)",
            "subgrid repeat(auto-fill, [a]) repeat(auto-fill, [b]", "10px,", "(1px)", "",
        ] {
            bad("grid-template-columns", v);
            bad("grid-template-rows", v);
        }
    }

    #[test]
    fn auto_tracks_and_flow() {
        for v in ["auto", "10px 1fr", "minmax(1px, 2fr)", "fit-content(1px)", "min-content max-content"] {
            ok("grid-auto-columns", v);
            ok("grid-auto-rows", v);
        }
        for v in [
            "none", "-1px", "-1fr", "minmax(1px)", "minmax(5fr, 1px)", "fit-content(1px, 2px)", "2em / 3em",
            "auto, 10%", "1px [a] 1px", "[] 1px []",
        ] {
            bad("grid-auto-columns", v);
            bad("grid-auto-rows", v);
        }
        for v in ["row", "column", "dense", "row dense", "dense column", "DENSE ROW"] {
            ok("grid-auto-flow", v);
        }
        for v in ["auto", "row row", "row column", "row dense column", "dense row dense", "none"] {
            bad("grid-auto-flow", v);
        }
    }

    #[test]
    fn grid_lines() {
        for v in ["auto", "1", "-2", "foo", "span 2", "span foo", "3 span", "foo 2", "2 foo", "span 2 foo", "SPAN 2"] {
            ok("grid-row-start", v);
            ok("grid-column-end", v);
        }
        for v in [
            "0", "span", "span 0", "span -1", "1.0", "+-3", "'string'", "\"1st\"", "auto 1", "1 auto",
            "span span", "1 2", "span 1 2", "span foo bar", "-3 span", "foo span bar", "1 / 2",
        ] {
            bad("grid-row-start", v);
        }
        ok("grid-row", "1 / 3");
        ok("grid-column", "a / span 2");
        ok("grid-row", "auto / auto");
        ok("grid-area", "a / b / c / d");
        ok("grid-area", "1");
        for v in ["5 /", "/ 5", "5 8", "1 / 2 / 3", "a / b / c", "span / span", "auto / initial", "8 / /"] {
            bad("grid-row", v);
            bad("grid-column", v);
        }
        bad("grid-area", "auto / auto / auto / auto / auto");
        bad("grid-area", "auto 2 auto 4");
    }

    #[test]
    fn template_areas() {
        for v in [
            "none", "\"a\"", "\"a a a\"", "\"a b\" \"a b\"", "\"a .\" \". b\"", "'a' 'a'", "\"a b\" \"c d\"",
            "\"header header\" \"nav main\" \"footer footer\"",
        ] {
            ok("grid-template-areas", v);
        }
        for v in [
            "auto", "none \"first\"", "\"first\" none", "\"\"", "\" \"", "\".\" \"\"", "\"first\" \"\" \"second\"",
            "\"a b\" \"c\"", "\"a b\" \"b a\"", "\"a . a\"", "\"a b\" \"c a\"",
        ] {
            bad("grid-template-areas", v);
        }
    }

    #[test]
    fn template_shorthand() {
        for v in [
            "none", "none / none", "auto / auto", "[a] 10px / auto", "[] 10px [] / [] auto []", "\"a\"", "\"a\" 10px",
            "[a] \"a\" 10px [a]", "\"a a a\"", "\"a\" / 10px", "\"a\" / 5fr", "\"a\" / 0", "\"a\" [] [] \"b\"",
            "\"a\" [a] [b] \"b\"", "\"a\" calc(100% - 10px) / calc(10px)", "\"a\" auto [a] \"b\" auto [b] / 10px",
            "repeat(2, 10px) / repeat(auto-fill, 20px)",
        ] {
            ok("grid-template", v);
            ok("grid", v);
        }
        for v in [
            "auto", "none none", "none []", "10px", "20%", "5fr", "[a]", "[a] 10px", "[a] repeat(2, 10px)", "[]",
            "10px \"a\"", "\"a\" none", "\"a\" 10px 10px", "\"a\" [a] 10px", "\"a\" [a] 10px [a]", "\"a\" [a] [a] 10px",
            "\"a\" [a] [a]", "[a] \"a\" [a] [a]", "\"a\" [a] [a] / none", "none / \"a\"", "\"a\" / none",
            "none / \"a\" []", "\"a\" \"b c\"",
        ] {
            bad("grid-template", v);
            bad("grid", v);
        }
    }

    #[test]
    fn grid_shorthand_auto_flow() {
        for v in [
            "100px / auto-flow dense 100px", "auto-flow dense 1fr / 100px", "auto-flow / 1fr", "none / auto-flow",
            "auto-flow 1fr 2fr / [a] 1fr", "dense auto-flow / auto",
        ] {
            ok("grid", v);
            bad("grid-template", v);
        }
        for v in [
            "auto-flow 100px", "auto-flow / auto-flow", "auto-flow 1fr / auto-flow 1fr", "dense auto-flow / dense auto-flow",
            "auto / auto-flow foo()", "dense / 1fr", "auto-flow auto-flow / 1fr", "auto-flow dense dense / 1fr",
        ] {
            bad("grid", v);
        }
    }

    #[test]
    fn flex_factors_and_flow_tolerance() {
        for v in ["0", "1", "2.5", ".5", "1e2", "+3", "calc(1 + 1)"] {
            ok("flex-grow", v);
            ok("flex-shrink", v);
        }
        for v in ["-1", "foo", "1px", "1 2", "1.", "auto", "inf", "5fr", ""] {
            bad("flex-grow", v);
            bad("flex-shrink", v);
        }
        for v in ["normal", "infinite", "0", "1px", "4%", "5vmin", "calc(2em + 3ex)"] {
            ok("flow-tolerance", v);
        }
        for v in ["auto", "10", "10px 20px", "1fr", "-1px", "-10%", "normal 10px", "10px infinite", "foo"] {
            bad("flow-tolerance", v);
        }
    }

    #[test]
    fn other_properties_pass_through() {
        for p in ["width", "color", "grid-gap", "gap", "align-items"] {
            ok(p, "anything goes (])");
        }
    }

    #[test]
    fn important_priority_is_not_part_of_the_value() {
        ok("grid-template-columns", "1fr 1fr !important");
        ok("grid-row", "1 / 3 ! IMPORTANT");
        ok("flex-grow", "2 !important");
        bad("grid-template-columns", "-1fr !important");
        bad("grid-row", "!important");
        assert_eq!(
            canonical_specified_grid("grid-row", "1 / 3 !important").as_deref(),
            Some("1 / 3 !important")
        );
    }

    #[test]
    fn comments_and_escapes() {
        ok("grid-auto-columns", "auto /**/");
        ok("grid-row", "/* a */ 1 / /* b */ 3");
        ok("grid-column-start", r"\31 st");
        bad("grid-column-start", "1st");
        bad("grid-column-start", r"a\");
        ok("grid-template-areas", "\"1st 2nd\" \"1st 2nd\"");
    }

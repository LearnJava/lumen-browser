use super::*;

    // --- BUG-1068: error recovery must not eat the enclosing block's `}` ---
    //
    // CSS Syntax L3 §5.4.3: a `}` ends the block a malformed construct lives
    // in, so recovery stops *before* it. The hazard is specific to a
    // declaration whose first character cannot start an ident — the IE7 star
    // hack `*zoom:1` is the one still shipped by minified vendor bundles —
    // because CSS Nesting L1 §4 lets a nested rule start with `*`, so such a
    // declaration is parsed as a nested-rule prelude, finds no `{`, and falls
    // into `recover_to_block_end`.

    #[test]
    fn star_hack_as_last_declaration_keeps_following_rule() {
        let s = parse(".x{*zoom:1}.y{text-align:center}");
        assert_eq!(s.rules.len(), 2, "rules: {:?}", s.rules);
        assert_eq!(s.rules[1].selectors, vec![one(SimpleSelector::Class("y".into()))]);
        assert_eq!(s.rules[1].declarations[0].property, "text-align");
        assert_eq!(s.rules[1].declarations[0].value, "center");
    }

    #[test]
    fn star_hack_with_semicolon_keeps_following_rule() {
        // The `;`-terminated form always worked (recovery stopped at the `;`);
        // pinned so the fix cannot regress it.
        let s = parse(".x{*zoom:1;}.y{text-align:center}");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].declarations[0].property, "text-align");
    }

    #[test]
    fn star_hack_after_valid_declaration_keeps_both() {
        let s = parse(".x{color:red;*zoom:1}.y{text-align:center}");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].declarations.len(), 1);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[1].declarations[0].property, "text-align");
    }

    #[test]
    fn star_hack_does_not_reparent_following_rules_as_nested() {
        // The damaging half of the old behaviour: after swallowing `}` the
        // parser stayed inside `.x`, so `.y`/`.z` became `.x .y` / `.x .z` —
        // selectors that match nothing, which is why a whole vendor bundle
        // went silently inert instead of failing loudly.
        let s = parse(".x{*zoom:1}.y{color:red}.z{color:blue}");
        assert_eq!(s.rules.len(), 3);
        assert_eq!(s.rules[1].selectors, vec![one(SimpleSelector::Class("y".into()))]);
        assert_eq!(s.rules[2].selectors, vec![one(SimpleSelector::Class("z".into()))]);
    }

    #[test]
    fn star_hack_inside_media_block_keeps_media_boundary() {
        // Same recovery path inside a `@media` body: the block's `}` must end
        // the media rule, not be consumed into the malformed declaration.
        let s = parse("@media screen{.x{*zoom:1}}.y{color:red}");
        assert_eq!(s.media_rules.len(), 1);
        assert_eq!(s.rules.len(), 1, "rules: {:?}", s.rules);
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Class("y".into()))]);
        assert_eq!(s.rules[0].declarations[0].property, "color");
    }

    #[test]
    fn genuine_nested_rule_still_parses_after_malformed_declaration() {
        // Recovery must stay local: the malformed `*zoom:1` is dropped, the
        // real nested rule that follows it is not.
        let s = parse(".x{*zoom:1; & .y{color:red}}");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(
            s.rules[1].selectors,
            vec![ComplexSelector {
                head: CompoundSelector { parts: vec![SimpleSelector::Class("x".into())] },
                tail: vec![(
                    Combinator::Descendant,
                    CompoundSelector { parts: vec![SimpleSelector::Class("y".into())] },
                )],
            }]
        );
    }

    #[test]
    fn universal_nested_rule_still_works() {
        // `*` in declaration position is not always garbage — CSS Nesting L1
        // §4's universal selector starts the same way. The fix must not cost
        // this case.
        let s = parse(".x{color:red; * {color:blue}}");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].declarations[0].value, "blue");
    }

    // --- BUG-788: cartesian selector-nesting expansion must not blow up ---
    //
    // Malformed CSS full of unclosed `{`/stray `.`/`:`/`[` keeps sending
    // recovery back into `parse_implicit_nested_rule`, and each level's
    // `expand_nesting` multiplies the live selector list by the nested
    // rule's own selector count — a 676-byte fuzzer minimization reached
    // 50 MiB / ×74 000 blowup this way, and doubling the input pushed it
    // past 300 s (superlinear, not just slow). The fix caps a single
    // `expand_nesting` call's output, bounding total work to roughly
    // input-length × cap instead of exploding with nesting depth.
    //
    // A byte-count assertion can't tell a fixed parser from a reverted one
    // (`MAX_EXPANDED_SELECTORS` a few orders too high would still "pass" on
    // this input) — the defect was about *how long*, so the test carries a
    // wall-clock budget, read off a background thread so a genuine hang
    // fails the test instead of the test runner itself. 5 s is generous
    // against the fixed path's ~0.3 s and nowhere close to the old path's
    // 300 s+ on this same repro doubled.
    #[test]
    fn oom_repro_stays_within_time_budget() {
        let input = include_str!(
            "../../../../../../fuzz/regressions/fuzz_css_parser-oom-unclosed-blocks"
        );
        // Doubled, per BUG-788's own measurement: the single copy alone was
        // already reproducing the blowup, but concatenation is what showed
        // the growth is worse than linear in input size.
        let doubled = input.repeat(2);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = parse(&doubled);
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "parse() did not return within the time budget — cartesian \
             selector-nesting expansion regressed (BUG-788)"
        );
    }

    #[test]
    fn vendor_bundle_shape_keeps_every_utility_after_the_hack() {
        // The exact shape found in `rust-lang.org`'s minified vendor bundle
        // (`.cf{*zoom:1}` at byte 7355) that cost the page every layout
        // utility declared after it.
        let s = parse(".cf{*zoom:1}.mw8{max-width:64rem}.center{margin-right:auto;margin-left:auto}.tc{text-align:center}");
        assert_eq!(s.rules.len(), 4, "rules: {:?}", s.rules);
        assert_eq!(s.rules[3].selectors, vec![one(SimpleSelector::Class("tc".into()))]);
        assert_eq!(s.rules[3].declarations[0].value, "center");
    }

    // --- BUG-1376: CDO/CDC на верхнем уровне пропускаются (CSS Syntax §5.4.1) ---

    #[test]
    fn cdo_before_rule_does_not_swallow_it() {
        let s = parse("p{color:red} <!-- p{color:green}");
        assert_eq!(s.rules.len(), 2, "rules: {:?}", s.rules);
        assert_eq!(s.rules[1].declarations[0].value, "green");
    }

    #[test]
    fn cdc_between_rules_is_skipped() {
        let s = parse("p{color:red} --> p{color:green}");
        assert_eq!(s.rules.len(), 2, "rules: {:?}", s.rules);
        assert_eq!(s.rules[1].declarations[0].value, "green");
    }

    #[test]
    fn style_wrapped_in_sgml_comment_keeps_rule() {
        let s = parse("<!-- p{color:green} -->");
        assert_eq!(s.rules.len(), 1, "rules: {:?}", s.rules);
        assert_eq!(s.rules[0].declarations[0].value, "green");
    }

    #[test]
    fn cdo_inside_declaration_block_is_not_skipped() {
        // Только top-level: внутри блока `<!--` остаётся частью значения/мусором.
        let s = parse("p{color:red;<!--}q{color:green}");
        assert_eq!(s.rules.last().unwrap().declarations[0].value, "green");
    }

    // --- BUG-1378: имена свойств ASCII-регистронезависимы (CSS 2.1 §4.1.3) ---

    #[test]
    fn property_name_is_case_insensitive() {
        let s = parse("p{COLOR:green;bACkGRounD:red}");
        let d = &s.rules[0].declarations;
        assert_eq!(d[0].property, "color");
        assert_eq!(d[0].value, "green");
        assert_eq!(d[1].property, "background");
    }

    #[test]
    fn custom_property_name_stays_case_sensitive() {
        let s = parse("p{--Foo:1;--foo:2}");
        let d = &s.rules[0].declarations;
        assert_eq!(d[0].property, "--Foo");
        assert_eq!(d[1].property, "--foo");
    }

    // --- BUG-1379: комментарий внутри значения — один пробел, не обрыв ---

    #[test]
    fn comment_inside_value_is_dropped_not_fatal() {
        for css in [
            "p{color:/*c*/green}",
            "p{color:green/*c*/;}",
            "p{color:/*a*/green/*b*/}",
            "p{color:green/*c*/}",
        ] {
            let d = &parse(css).rules[0].declarations;
            assert_eq!(d.len(), 1, "{css}");
            assert_eq!(d[0].value, "green", "{css}");
        }
    }

    #[test]
    fn comment_separates_tokens_in_value() {
        let s = parse("p{margin:1px/*c*/2px;height:1/*c*/0px;color:gr/*c*/een}");
        let d = &s.rules[0].declarations;
        assert_eq!(d[0].value, "1px 2px");
        assert_eq!(d[1].value, "1 0px");
        assert_eq!(d[2].value, "gr een");
    }

    #[test]
    fn comment_markers_survive_inside_strings_and_urls() {
        let s = parse("p{content:\"a/*b*/c\";background:url(a/*b*/c.png);color:red/*c*/ !important}");
        let d = &s.rules[0].declarations;
        assert_eq!(d[0].value, "\"a/*b*/c\"");
        assert_eq!(d[1].value, "url(a/*b*/c.png)");
        assert_eq!(d[2].value, "red");
        assert!(d[2].important);
    }

    #[test]
    fn escaped_quote_does_not_end_string_in_value() {
        let s = parse("p{content:\"a\\\"/*x*/;b\";color:red}");
        let d = &s.rules[0].declarations;
        assert_eq!(d[0].value, "\"a\\\"/*x*/;b\"");
        assert_eq!(d[1].value, "red");
    }

    // --- BUG-1381: CSS Syntax §5.4 recovery — qualified-rule prelude, brackets, strings, EOF ---

    fn colors(s: &Stylesheet) -> Vec<String> {
        s.rules
            .iter()
            .filter_map(|r| r.declarations.iter().find(|d| d.property == "color"))
            .map(|d| d.value.clone())
            .collect()
    }

    #[test]
    fn at_sign_with_space_is_a_qualified_rule_prelude() {
        // `@ import …` — `@` + whitespace is a delim token, not an at-keyword:
        // the prelude runs `@ import "x.css"; div` up to `{`, the whole rule is
        // invalid and dropped, `;` does not end it.
        let s = parse("@ import \"x.css\"; div{color:red} * {color:green}");
        assert_eq!(s.imports.len(), 0);
        assert_eq!(colors(&s), vec!["green"], "rules: {:?}", s.rules);
    }

    #[test]
    fn semicolon_inside_invalid_top_level_prelude_does_not_end_it() {
        let s = parse("foo; div{color:red} p{color:green}");
        assert_eq!(colors(&s), vec!["green"]);
    }

    #[test]
    fn unclosed_function_at_eof_closes_in_value() {
        let s = parse("div{color:rgb(0, 128, 0");
        assert_eq!(colors(&s), vec!["rgb(0, 128, 0)"], "rules: {:?}", s.rules);
    }

    #[test]
    fn unterminated_import_string_at_eof_closes() {
        let s = parse("@import \"support/eof-green.css");
        assert_eq!(s.imports.len(), 1);
        assert_eq!(s.imports[0].url, "support/eof-green.css");
    }

    #[test]
    fn paren_in_prelude_swallows_braces() {
        // matching-brackets-003: `( … )` hides the `{}` pairs inside it, so
        // `p ( {…} p {…} ) p` is one invalid prelude ending at the last block.
        let s = parse("p { color: green; } p ( { border: solid red; } p { background: red; } ) p { color: red; }");
        assert_eq!(s.rules.len(), 1, "rules: {:?}", s.rules);
        assert_eq!(colors(&s), vec!["green"]);
    }

    #[test]
    fn square_bracket_in_prelude_swallows_rules() {
        // square-brackets-001
        let s = parse("p { color: red; }\n[\np { color: red !important; }\np { color: red !important; }\n] p { color: red !important; }\np { color: green; }");
        assert_eq!(colors(&s), vec!["red", "green"], "rules: {:?}", s.rules);
    }

    #[test]
    fn unterminated_string_ends_at_newline_and_drops_declaration() {
        // unterminated-string-001: bad-string ends at the newline, the
        // declaration then runs to the next `;` and is invalid.
        let s = parse("div{color:green;font-family:'Courier;\ncolor:red;}");
        assert_eq!(colors(&s), vec!["green"], "rules: {:?}", s.rules);
    }

    #[test]
    fn blocks_and_strings_in_garbage_declaration_are_matched() {
        // blocks-and-strings-001
        let s = parse("div{\"this is a string]}\"\"[{\\\"'\";\n{{}}[]'';\ncolor:green;}");
        assert_eq!(colors(&s), vec!["green"], "rules: {:?}", s.rules);
    }

    #[test]
    fn string_with_escaped_newline_in_attribute_selector() {
        // strings-000
        let s = parse("p { color: red }\np[title=\"this is\\\n a test\"] { color: green }");
        assert_eq!(s.rules.len(), 2, "rules: {:?}", s.rules);
        assert_eq!(colors(&s), vec!["red", "green"]);
    }

    #[test]
    fn junk_with_brackets_and_strings_inside_declaration_is_skipped() {
        // matching-brackets-002: the junk after `color:` is ONE declaration
        // (brackets and strings hide its `;` and `}`), the rest of the rule
        // survives, and no phantom `p { color: red }` rules appear.
        let s = parse(MATCHING_BRACKETS_002);
        assert_eq!(s.rules.len(), 1, "rules: {:?}", s.rules);
        let d = &s.rules[0].declarations;
        assert_eq!(d.len(), 4, "{d:?}");
        assert!(d[2].value.starts_with("red ] ) test-token"), "{d:?}");
        assert_eq!((d[3].property.as_str(), d[3].value.as_str()), ("background", "white"));
    }

    const MATCHING_BRACKETS_002: &str = r#"p { background: red; color: green; color: red ] ) test-token \
 ~ ` ! @ # $ % ^ & * - _ + = | : > < ? / , .
 [\]\5D ']' "]"; background: red; } p { color: red; } ]
 (\)\29 ')' ")"; background: red; } p { color: red; } )
 '\'; background: red; } p { color: red; }',
 "\"; background: red; } p { color: red; }' p { color: red; } "
 ;
 background: white; }"#;

    #[test]
    fn unknown_at_rule_with_unbalanced_junk_ends_at_depth_zero_semicolon() {
        // matching-brackets-001 (first half): `@foo …;` hides every `;` and `{}`
        // inside strings, escapes and matched brackets; only the final `;`
        // ends it, and the rules after it survive.
        let s = parse(MATCHING_BRACKETS_001);
        let ids: Vec<_> = s
            .rules
            .iter()
            .map(|r| (format!("{:?}", r.selectors[0].head.parts), r.declarations.len()))
            .collect();
        assert_eq!(s.rules.len(), 5, "rules: {ids:?}");
        assert_eq!(colors(&s), vec!["red", "green", "green"], "rules: {ids:?}");
    }

    const MATCHING_BRACKETS_001: &str = r#"    p {
      color: red;
      background: red;
    }
    #semicolon { background: transparent; }
    @foo ] } ) test-token \
     ~ ` ! @ # $ % ^ & * - _ + = | : > < ? / , .
     [\]\5D ']' "]"; # { background: red; } ]
     (\)\29 ')' ")"; #semicolon { background: red; } } } } )
     '; #semicolon { background: red; } } } }',
     "; #semicolon { background: red; }' } } }"
    ;
    #semicolon { color: green; }
    #block { background: transparent; }
    @foo ] } ) test-token \
     ~ ` ! @ # $ % ^ & * - _ + = | : > < ? / , .
     [\]\5D ']' "]"; #block { background: red; } ]
     (\)\29 ')' ")"; #block { background: red; } )
     '\'; #block { background: red; }',
     "\"; #block { background: red; }'"
     {\}\79 '}' "}"; #block { background: red; }
        #block { background: red; } }
    #block { color: green; }
"#;

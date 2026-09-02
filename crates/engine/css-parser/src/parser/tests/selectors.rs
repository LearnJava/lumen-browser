use super::*;

    // ── is_valid_selector_list (BUG-391) ──────────────────────────────────────

    #[test]
    fn valid_selector_list_accepts_ordinary_selectors() {
        for sel in [
            "div",
            ".foo",
            "#bar",
            "*",
            "a[href]",
            "a[href^='http' i]",
            "div > p + span ~ em",
            "div, p, span",
            "  div  ,  p  ",
            "li:nth-child(2n+1)",
            "li:nth-child(2n+1 of .x)",
            ":not(.a, .b)",
            ":is(.a, .b):hover",
            ":where(div .foo)",
            ":has(> .child)",
            ":lang(en-GB)",
            ":dir(rtl)",
            "input:read-only:required",
            "::before",
            "*::before",
            "foo.bar[baz]::before",
            "::after::marker",
            "::before::marker",
            "::placeholder",
            "::picker(select)",
            "::highlight(name)",
            "::slotted(.a)",
            "div::first-line",
            "p::before:hover",
            "p::before:is(:hover, :focus)",
        ] {
            assert!(is_valid_selector_list(sel), "expected valid: {sel:?}");
        }
    }

    #[test]
    fn valid_selector_list_rejects_syntax_errors() {
        for sel in [
            "",
            "   ",
            "(",
            "div (",
            "div!",
            "div,",
            ",div",
            "div,,p",
            "div{}",
            ">",
            ".",
            "#",
        ] {
            assert!(!is_valid_selector_list(sel), "expected invalid: {sel:?}");
        }
    }

    #[test]
    fn valid_selector_list_rejects_unknown_pseudo() {
        // Драйвер бага: WPT-паттерн feature-detection
        // `assert_throws_dom('SyntaxError', () => el.matches(':halfscreen'))`.
        for sel in [
            ":halfscreen",
            "div:halfscreen",
            "::bogus-pseudo",
            "::highlight",
            "::picker",
            "::picker()",
            "::picker(foo)",
            ":not(:halfscreen)",
            ":has(:halfscreen)",
            ":host-context(.a)",
        ] {
            assert!(!is_valid_selector_list(sel), "expected invalid: {sel:?}");
        }
    }

    #[test]
    fn valid_selector_list_rejects_bad_pseudo_element_structure() {
        for sel in [
            "::before *",
            "::after *",
            "::marker *",
            "::placeholder *",
            "::before > div",
            "::before::before",
            "::after::before",
            "::marker::marker",
            "::placeholder::marker",
            "::before::placeholder",
            "::before.cls",
            "::before#id",
            "::before[attr]",
            "::highlight(foo).a",
            "::highlight(foo) div",
            "::highlight(foo)::after",
            "::highlight(foo):hover",
            "::before:host",
            ":not(::before)",
            "::slotted(::before)",
        ] {
            assert!(!is_valid_selector_list(sel), "expected invalid: {sel:?}");
        }
    }

    #[test]
    fn valid_selector_list_keeps_is_where_forgiving() {
        // CSS Selectors L4 §3.2: `:is()`/`:where()` — forgiving-selector-list,
        // невалидный элемент внутри отбрасывается, а не делает невалидным
        // весь селектор (в отличие от `:not()`).
        assert!(is_valid_selector_list(":is(::before)"));
        assert!(is_valid_selector_list(":where(:halfscreen)"));
        assert!(!is_valid_selector_list(":not(:halfscreen)"));
    }

    #[test]
    fn valid_selector_list_is_independent_of_matching() {
        // Валидный селектор, которому ничего не соответствует, остаётся
        // валидным — иначе `querySelector('.no-match')` бросал бы вместо
        // возврата null.
        assert!(is_valid_selector_list(".no-such-class-anywhere"));
        assert!(is_valid_selector_list("nonexistent-tag"));
    }

    // ── to_css_str tests ──────────────────────────────────────────────────────

    #[test]
    fn to_css_str_type_selector() {
        let sel = parse_selector_list("div");
        assert_eq!(sel[0].to_css_str(), "div");
    }

    #[test]
    fn to_css_str_class_and_id() {
        let sel = parse_selector_list(".foo#bar");
        assert_eq!(sel[0].to_css_str(), ".foo#bar");
    }

    #[test]
    fn to_css_str_descendant_combinator() {
        let sel = parse_selector_list("div p");
        assert_eq!(sel[0].to_css_str(), "div p");
    }

    #[test]
    fn to_css_str_child_combinator() {
        let sel = parse_selector_list("ul > li");
        assert_eq!(sel[0].to_css_str(), "ul > li");
    }

    #[test]
    fn to_css_str_pseudo_class() {
        let sel = parse_selector_list("a:hover");
        assert_eq!(sel[0].to_css_str(), "a:hover");
    }

    #[test]
    fn to_css_str_first_child() {
        let sel = parse_selector_list("p:first-child");
        assert_eq!(sel[0].to_css_str(), "p:first-child");
    }

    #[test]
    fn to_css_str_nth_child() {
        let sel = parse_selector_list("li:nth-child(2n+1)");
        let s = sel[0].to_css_str();
        assert!(s.contains(":nth-child"), "got: {s}");
    }

    #[test]
    fn to_css_str_attribute() {
        let sel = parse_selector_list("[type=\"text\"]");
        let s = sel[0].to_css_str();
        assert!(s.contains("[type") && s.contains("text"), "got: {s}");
    }

    // ── existing test helpers ──────────────────────────────────────────────────

    /// Удобный конструктор для тестов: ComplexSelector из одной compound с
    /// единственным simple-селектором.
    pub(crate) fn one(part: SimpleSelector) -> ComplexSelector {
        ComplexSelector {
            head: CompoundSelector { parts: vec![part] },
            tail: Vec::new(),
        }
    }

    #[test]
    fn empty_input() {
        assert_eq!(parse(""), Stylesheet::default());
    }

    #[test]
    fn whitespace_and_comment_only() {
        assert_eq!(parse("  /* hi */  "), Stylesheet::default());
    }

    #[test]
    fn single_rule() {
        let s = parse("p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Type("p".into()))]);
        assert_eq!(s.rules[0].declarations.len(), 1);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn class_selector() {
        let s = parse(".foo { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Class("foo".into()))]);
    }

    #[test]
    fn id_selector() {
        let s = parse("#bar { color: red; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Id("bar".into()))]);
    }

    #[test]
    fn universal_selector() {
        let s = parse("* { box-sizing: border-box; }");
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Universal)]);
    }

    #[test]
    fn multiple_selectors() {
        let s = parse("p, h1, h2 { color: red; }");
        assert_eq!(
            s.rules[0].selectors,
            vec![
                one(SimpleSelector::Type("p".into())),
                one(SimpleSelector::Type("h1".into())),
                one(SimpleSelector::Type("h2".into())),
            ]
        );
    }

    #[test]
    fn multiple_declarations() {
        let s = parse("p { color: red; font-size: 14px; margin: 0; }");
        assert_eq!(s.rules[0].declarations.len(), 3);
        assert_eq!(s.rules[0].declarations[1].property, "font-size");
        assert_eq!(s.rules[0].declarations[1].value, "14px");
    }

    // ──────────────── !important (CSS Cascade L4 §8.1) ────────────────

    #[test]
    fn declaration_default_not_important() {
        let s = parse("p { color: red; }");
        assert!(!s.rules[0].declarations[0].important);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn declaration_important_basic() {
        let s = parse("p { color: red !important; }");
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, "red");
    }

    #[test]
    fn declaration_important_no_space_before_bang() {
        let s = parse("p { color: red!important; }");
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, "red");
    }

    #[test]
    fn declaration_important_case_insensitive() {
        let s = parse("p { color: red !IMPORTANT; }");
        assert!(s.rules[0].declarations[0].important);
    }

    #[test]
    fn declaration_important_with_whitespace_between_bang_and_word() {
        // CSS Syntax §5.5.4 разрешает whitespace внутри `!important`.
        let s = parse("p { color: red !  important; }");
        assert!(s.rules[0].declarations[0].important);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn declaration_important_inside_quotes_not_stripped() {
        // `content: "!important"` — литерал, не модификатор.
        let s = parse(r#"p { content: "!important"; }"#);
        let d = &s.rules[0].declarations[0];
        assert!(!d.important);
        assert_eq!(d.value, r#""!important""#);
    }

    #[test]
    fn declaration_important_after_quoted_value() {
        // `font-family: "Arial" !important;` — флаг есть, value сохраняется.
        let s = parse(r#"p { font-family: "Arial" !important; }"#);
        let d = &s.rules[0].declarations[0];
        assert!(d.important);
        assert_eq!(d.value, r#""Arial""#);
    }

    #[test]
    fn declaration_important_works_for_multiple() {
        let s = parse("p { color: red !important; font-size: 14px; }");
        assert!(s.rules[0].declarations[0].important);
        assert!(!s.rules[0].declarations[1].important);
    }

    #[test]
    fn declaration_value_ending_with_important_word_alone_not_flag() {
        // `value: important;` — без `!`, не флаг.
        let s = parse("p { font-weight: important; }");
        let d = &s.rules[0].declarations[0];
        assert!(!d.important);
        assert_eq!(d.value, "important");
    }

    #[test]
    fn trailing_semicolon_optional() {
        let with = parse("p { color: red; }");
        let without = parse("p { color: red }");
        assert_eq!(with, without);
    }

    #[test]
    fn empty_rule() {
        let s = parse("p {}");
        assert_eq!(s.rules.len(), 1);
        assert!(s.rules[0].declarations.is_empty());
    }

    #[test]
    fn multiple_rules() {
        let s = parse("p { color: red; } h1 { font-size: 24px; }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].declarations[0].property, "font-size");
    }

    #[test]
    fn comments_between_and_within() {
        let s = parse("/* one */ p /* hmm */ { /* x */ color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn at_import_skipped() {
        let s = parse("@import \"foo.css\"; p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].selectors[0], one(SimpleSelector::Type("p".into())));
    }

    #[test]
    fn at_media_block_skipped() {
        let s = parse("@media print { p { color: black; } } p { color: red; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    #[test]
    fn cyrillic_class_selector() {
        let s = parse(".привет { color: red; }");
        assert_eq!(
            s.rules[0].selectors,
            vec![one(SimpleSelector::Class("привет".into()))]
        );
    }

    #[test]
    fn cyrillic_value_with_quotes() {
        let s = parse(r#"p { font-family: "Иваново", sans-serif; }"#);
        assert_eq!(
            s.rules[0].declarations[0].value,
            r#""Иваново", sans-serif"#
        );
    }

    #[test]
    fn malformed_declaration_skipped() {
        let s = parse("p { color: red; broken; font-size: 14px; }");
        assert_eq!(s.rules[0].declarations.len(), 2);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[1].property, "font-size");
    }

    #[test]
    fn negative_and_complex_values() {
        let s = parse("p { margin: -10px; background: url(\"a.png\"); }");
        assert_eq!(s.rules[0].declarations[0].value, "-10px");
        assert_eq!(s.rules[0].declarations[1].value, "url(\"a.png\")");
    }

    #[test]
    fn vendor_prefix_property() {
        let s = parse("p { -webkit-user-select: none; }");
        assert_eq!(s.rules[0].declarations[0].property, "-webkit-user-select");
    }

    // ──────────────── compound selectors ────────────────

    #[test]
    fn compound_type_and_class() {
        let s = parse("p.foo { color: red; }");
        assert_eq!(s.rules[0].selectors.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Type("p".into()),
                SimpleSelector::Class("foo".into()),
            ]
        );
    }

    #[test]
    fn compound_type_class_id() {
        let s = parse("p.foo#bar { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Type("p".into()),
                SimpleSelector::Class("foo".into()),
                SimpleSelector::Id("bar".into()),
            ]
        );
    }

    #[test]
    fn compound_two_classes() {
        let s = parse(".a.b { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![
                SimpleSelector::Class("a".into()),
                SimpleSelector::Class("b".into()),
            ]
        );
    }

    // ──────────────── combinators ────────────────

    #[test]
    fn descendant_combinator() {
        let s = parse("div p { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("p".into())]);
    }

    #[test]
    fn child_combinator() {
        let s = parse("ul > li { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::Child);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("li".into())]);
    }

    #[test]
    fn next_sibling_combinator() {
        let s = parse("h1 + p { margin-top: 0; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::NextSibling);
    }

    #[test]
    fn later_sibling_combinator() {
        let s = parse("h1 ~ p { color: gray; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.tail[0].0, Combinator::LaterSibling);
    }

    #[test]
    fn chained_combinators() {
        let s = parse("body main > article p { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("body".into())]);
        assert_eq!(sel.tail.len(), 3);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[1].0, Combinator::Child);
        assert_eq!(sel.tail[2].0, Combinator::Descendant);
    }

    #[test]
    fn combinator_around_compound() {
        let s = parse("nav.main > a.link { color: red; }");
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 2);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].1.parts.len(), 2);
    }

    // ──────────────── attribute selectors ────────────────

    #[test]
    fn attribute_presence() {
        let s = parse("[disabled] { opacity: 0.5; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.name, "disabled");
                assert_eq!(a.op, None);
                assert_eq!(a.value, None);
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_equals_unquoted() {
        let s = parse("[type=submit] { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.name, "type");
                assert_eq!(a.op, Some(AttrOp::Equals));
                assert_eq!(a.value.as_deref(), Some("submit"));
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_equals_quoted() {
        let s = parse(r#"[lang="ru-RU"] { font-family: serif; }"#);
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::Attribute(a) => {
                assert_eq!(a.value.as_deref(), Some("ru-RU"));
            }
            _ => panic!("expected attribute selector"),
        }
    }

    #[test]
    fn attribute_all_operators() {
        let ops = [
            ("[a~=v]", AttrOp::Includes),
            ("[a|=v]", AttrOp::DashMatch),
            ("[a^=v]", AttrOp::Prefix),
            ("[a$=v]", AttrOp::Suffix),
            ("[a*=v]", AttrOp::Substring),
        ];
        for (src, expected) in ops {
            let s = parse(&format!("{src} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            match p {
                SimpleSelector::Attribute(a) => assert_eq!(a.op, Some(expected), "src={src}"),
                _ => panic!("expected attribute selector for {src}"),
            }
        }
    }

    #[test]
    fn attribute_combined_with_type() {
        let s = parse("a[href] { color: blue; }");
        let head = &s.rules[0].selectors[0].head;
        assert_eq!(head.parts.len(), 2);
        assert!(matches!(head.parts[0], SimpleSelector::Type(ref t) if t == "a"));
        assert!(matches!(&head.parts[1], SimpleSelector::Attribute(a) if a.name == "href"));
    }

    // ──────────────── case-insensitive attribute (CSS L4 §6.3.6) ────────────

    fn attr_at(s: &Stylesheet, rule: usize) -> &AttrSelector {
        match &s.rules[rule].selectors[0].head.parts[0] {
            SimpleSelector::Attribute(a) => a,
            other => panic!("expected attribute selector, got {other:?}"),
        }
    }

    #[test]
    fn attribute_case_insensitive_flag_lowercase() {
        let s = parse("[type=submit i] { color: red; }");
        let a = attr_at(&s, 0);
        assert!(a.case_insensitive);
        assert_eq!(a.value.as_deref(), Some("submit"));
    }

    #[test]
    fn attribute_case_insensitive_flag_uppercase() {
        // `I` тоже должен работать (флаги ASCII case-insensitive).
        let s = parse("[type=submit I] { color: red; }");
        assert!(attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_sensitive_explicit() {
        // `s` явно ставит case-sensitive (default).
        let s = parse("[type=submit s] { color: red; }");
        assert!(!attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_insensitive_with_quoted_value() {
        let s = parse(r#"[lang="EN-us" i] { color: red; }"#);
        let a = attr_at(&s, 0);
        assert!(a.case_insensitive);
        assert_eq!(a.value.as_deref(), Some("EN-us"));
    }

    #[test]
    fn attribute_case_insensitive_works_for_all_ops() {
        // Флаг `i` совместим со всеми операторами.
        for src in [
            "[a~=v i]",
            "[a|=v i]",
            "[a^=v i]",
            "[a$=v i]",
            "[a*=v i]",
        ] {
            let s = parse(&format!("{src} {{}}"));
            assert!(attr_at(&s, 0).case_insensitive, "ci flag lost in {src}");
        }
    }

    #[test]
    fn attribute_no_flag_default_case_sensitive() {
        let s = parse("[type=submit] { color: red; }");
        assert!(!attr_at(&s, 0).case_insensitive);
    }

    #[test]
    fn attribute_case_insensitive_with_extra_whitespace() {
        // Между value и `i` — любое количество пробелов.
        let s = parse("[type=submit   i ] { color: red; }");
        assert!(attr_at(&s, 0).case_insensitive);
    }

    // ──────────────── pseudo-classes / pseudo-elements ────────────────

    #[test]
    fn pseudo_first_child() {
        let s = parse("p:first-child { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        assert!(matches!(
            &head.parts[1],
            SimpleSelector::PseudoClass(PseudoClass::FirstChild)
        ));
    }

    #[test]
    fn pseudo_known_names() {
        let cases = [
            ("first-child", PseudoClass::FirstChild),
            ("last-child", PseudoClass::LastChild),
            ("only-child", PseudoClass::OnlyChild),
            ("empty", PseudoClass::Empty),
            ("root", PseudoClass::Root),
            ("first-of-type", PseudoClass::FirstOfType),
            ("last-of-type", PseudoClass::LastOfType),
            ("only-of-type", PseudoClass::OnlyOfType),
            ("placeholder-shown", PseudoClass::PlaceholderShown),
            ("required", PseudoClass::Required),
            ("optional", PseudoClass::Optional),
            ("read-only", PseudoClass::ReadOnly),
            ("read-write", PseudoClass::ReadWrite),
            ("disabled", PseudoClass::Disabled),
            ("enabled", PseudoClass::Enabled),
            ("checked", PseudoClass::Checked),
            ("indeterminate", PseudoClass::Indeterminate),
            ("default", PseudoClass::Default),
            ("link", PseudoClass::Link),
            ("visited", PseudoClass::Visited),
            ("any-link", PseudoClass::AnyLink),
            ("in-range", PseudoClass::InRange),
            ("out-of-range", PseudoClass::OutOfRange),
            ("scope", PseudoClass::Scope),
            ("target", PseudoClass::Target),
            ("target-within", PseudoClass::TargetWithin),
            ("defined", PseudoClass::Defined),
            ("fullscreen", PseudoClass::Fullscreen),
            ("modal", PseudoClass::Modal),
            ("popover-open", PseudoClass::PopoverOpen),
            ("current", PseudoClass::Current),
            ("past", PseudoClass::Past),
            ("future", PseudoClass::Future),
        ];
        for (name, expected) in cases {
            let s = parse(&format!(":{name} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            match p {
                SimpleSelector::PseudoClass(pc) => assert_eq!(pc, &expected, "name={name}"),
                _ => panic!("expected pseudo-class for {name}"),
            }
        }
    }

    #[test]
    fn pseudo_unsupported_kept_as_name() {
        let s = parse(":hover { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Hover) => {},
            _ => panic!("expected Hover pseudo-class"),
        }
    }

    #[test]
    fn pseudo_nth_child_parsed() {
        let s = parse(":nth-child(2n+1) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthChild(spec, of)) => {
                assert_eq!(*spec, NthSpec { a: 2, b: 1 });
                assert!(of.is_none(), "no of-clause expected");
            }
            _ => panic!("expected NthChild(2n+1), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_child_with_of_clause() {
        // CSS Selectors L4 §6.6.5.1.
        let s = parse(":nth-child(odd of .visible) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthChild(spec, of)) => {
                assert_eq!(*spec, NthSpec::ODD);
                let list = of.as_ref().expect("of-clause expected");
                assert_eq!(list.len(), 1);
            }
            _ => panic!("expected NthChild with of-clause, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_last_child_with_of_clause() {
        let s = parse(":nth-last-child(1 of li.active) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthLastChild(spec, of)) => {
                assert_eq!(*spec, NthSpec { a: 0, b: 1 });
                assert!(of.is_some(), "of-clause expected");
            }
            _ => panic!("expected NthLastChild with of-clause, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_child_of_selector_list() {
        // `of` принимает selector-list через запятую.
        let s = parse(":nth-child(2n of .x, .y) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::NthChild(_, Some(list))) => {
                assert_eq!(list.len(), 2);
            }
            _ => panic!("expected NthChild with selector-list of, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_nth_child_empty_of_clause_invalid() {
        // `:nth-child(odd of)` без selector-а → invalid, fallback на Unsupported.
        let s = parse(":nth-child(odd of) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "nth-child"
        ));
    }

    #[test]
    fn pseudo_nth_of_type_does_not_accept_of_clause() {
        // CSS Selectors L4 §6.6.5.1: `of` clause НЕ применяется к
        // `:nth-of-type` (type filter — implicit). Если у пользователя там
        // случайно `of` — спека требует invalid; наш парсер собирает всё
        // в spec-string, который parse_nth_spec_str отвергает.
        let s = parse(":nth-of-type(odd of .x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "nth-of-type"
        ));
    }

    #[test]
    fn pseudo_lang_single_tag() {
        let s = parse(":lang(en) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_with_region() {
        let s = parse(":lang(en-US) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en-us".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_comma_list() {
        let s = parse(":lang(en, fr, ru) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en".to_string(), "fr".to_string(), "ru".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_case_normalized_to_lower() {
        let s = parse(":lang(EN, FR-CA) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Lang(tags)) => {
                assert_eq!(tags, &vec!["en".to_string(), "fr-ca".to_string()]);
            }
            _ => panic!("expected Lang, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_lang_empty_falls_back_to_unsupported() {
        // `:lang()` без аргументов — невалидно по spec, парсер откатывает
        // в Unsupported.
        let s = parse(":lang() { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "lang"
        ));
    }

    #[test]
    fn pseudo_dir_ltr() {
        let s = parse(":dir(ltr) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Dir(DirArg::Ltr))
        ));
    }

    #[test]
    fn pseudo_dir_rtl() {
        let s = parse(":dir(rtl) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Dir(DirArg::Rtl))
        ));
    }

    #[test]
    fn pseudo_dir_case_insensitive_keyword() {
        let s = parse(":dir(LTR) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Dir(DirArg::Ltr))
        ));
    }

    #[test]
    fn pseudo_dir_unknown_keyword_falls_back() {
        // `auto` — невалидный аргумент для :dir в spec (только ltr/rtl).
        // Парсер откатывает в Unsupported.
        let s = parse(":dir(auto) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "dir"
        ));
    }

    #[test]
    fn pseudo_dir_empty_falls_back() {
        let s = parse(":dir() { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "dir"
        ));
    }

    #[test]
    fn pseudo_state_basic_ident() {
        let s = parse(":state(open) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::State(name)) => {
                assert_eq!(name, "open");
            }
            _ => panic!("expected State, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_state_is_case_sensitive() {
        // Custom-ident (§17.4), в отличие от `:lang()`, не нормализуется к
        // lowercase — состояния из ElementInternals.states case-sensitive.
        let s = parse(":state(Open) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::State(name)) => {
                assert_eq!(name, "Open");
            }
            _ => panic!("expected State, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_state_hyphenated_ident() {
        let s = parse(":state(is-collapsed) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::State(name)) => {
                assert_eq!(name, "is-collapsed");
            }
            _ => panic!("expected State, got {p:?}"),
        }
    }

    #[test]
    fn pseudo_state_empty_falls_back_to_unsupported() {
        let s = parse(":state() { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "state"
        ));
    }

    #[test]
    fn pseudo_state_to_css_str_roundtrip() {
        let s = parse(":state(checked) { color: red; }");
        assert_eq!(s.rules[0].selectors[0].to_css_str(), ":state(checked)");
    }

    #[test]
    fn pseudo_target_case_insensitive_name() {
        // pseudo-class names ASCII case-insensitive (CSS Syntax §4.4) —
        // `:TARGET` распознаётся как `:target`.
        for src in [":target { }", ":TARGET { }", ":Target { }"] {
            let s = parse(src);
            let p = &s.rules[0].selectors[0].head.parts[0];
            assert!(
                matches!(p, SimpleSelector::PseudoClass(PseudoClass::Target)),
                "name={src}"
            );
        }
    }

    #[test]
    fn pseudo_target_does_not_accept_arguments() {
        // `:target` — не functional pseudo. `:target(x)` — невалидное use,
        // fallback на Unsupported.
        let s = parse(":target(x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "target"
        ));
    }

    #[test]
    fn pseudo_target_specificity_is_pseudo_class_level() {
        // CSS Selectors L4 §16: pseudo-class contributes (0,1,0) — class-уровень.
        let s = parse(":target { color: red; }");
        let spec = s.rules[0].selectors[0].specificity();
        assert_eq!(spec, Specificity { a: 0, b: 1, c: 0 });
    }

    #[test]
    fn pseudo_target_within_recognized() {
        // Подтверждение, что `:target-within` парсится как отдельный variant,
        // а не как `target`-ident с suffix-ом или Unsupported.
        let s = parse(":target-within { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::TargetWithin)
        ));
    }

    #[test]
    fn pseudo_target_within_does_not_accept_arguments() {
        // Не functional pseudo — `:target-within(x)` → Unsupported.
        let s = parse(":target-within(x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "target-within"
        ));
    }

    #[test]
    fn pseudo_defined_case_insensitive_name() {
        // CSS Syntax §4.4: pseudo-class names ASCII case-insensitive.
        for src in [":defined { }", ":DEFINED { }", ":Defined { }"] {
            let s = parse(src);
            let p = &s.rules[0].selectors[0].head.parts[0];
            assert!(
                matches!(p, SimpleSelector::PseudoClass(PseudoClass::Defined)),
                "src={src}"
            );
        }
    }

    #[test]
    fn pseudo_defined_does_not_accept_arguments() {
        // `:defined` — не functional. `:defined(x)` → Unsupported.
        let s = parse(":defined(x) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(matches!(
            p,
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "defined"
        ));
    }

    #[test]
    fn pseudo_defined_specificity_is_pseudo_class_level() {
        let s = parse(":defined { color: red; }");
        let spec = s.rules[0].selectors[0].specificity();
        assert_eq!(spec, Specificity { a: 0, b: 1, c: 0 });
    }

    #[test]
    fn pseudo_element_double_colon() {
        let s = parse("p::before { content: \"\"; }");
        let head = &s.rules[0].selectors[0].head;
        assert!(matches!(&head.parts[1], SimpleSelector::PseudoElement(PseudoElementKind::Before)));
    }

    // ──────────────── specificity ────────────────

    #[test]
    fn specificity_universal_is_zero() {
        let s = parse("* { color: red; }");
        let spec = s.rules[0].selectors[0].specificity();
        assert_eq!(spec, Specificity { a: 0, b: 0, c: 0 });
    }

    #[test]
    fn specificity_type_is_001() {
        let s = parse("p { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 1 }
        );
    }

    #[test]
    fn specificity_class_is_010() {
        let s = parse(".foo { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_id_is_100() {
        let s = parse("#bar { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_complex() {
        // a#b.c[d] p:hover — id=1, class+attr+pseudo=3, type=2 → (1,3,2)
        let s = parse("a#b.c[d] p:hover { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 3, c: 2 }
        );
    }

    #[test]
    fn specificity_ordering() {
        let high = Specificity { a: 0, b: 1, c: 0 }; // .foo
        let low = Specificity { a: 0, b: 0, c: 5 }; // div div div div div
        assert!(high > low);
    }

    // ──────────────── edge cases для recovery ────────────────

    #[test]
    fn unknown_combinator_breaks_rule() {
        // `% p` — `%` не start_ident и не combinator, должен быть recovery.
        // Дальше нормальное правило парсится.
        let s = parse("% p { color: red; } a { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![SimpleSelector::Type("a".into())]
        );
    }

    #[test]
    fn malformed_attribute_recovers() {
        let s = parse("[a$$=foo] { color: red; } p { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(
            s.rules[0].selectors[0].head.parts,
            vec![SimpleSelector::Type("p".into())]
        );
    }

    // ──────────────── functional pseudo: :nth-* ────────────────

    #[test]
    fn nth_spec_str_keywords() {
        assert_eq!(parse_nth_spec_str("odd"), Some(NthSpec { a: 2, b: 1 }));
        assert_eq!(parse_nth_spec_str("even"), Some(NthSpec { a: 2, b: 0 }));
        assert_eq!(parse_nth_spec_str("ODD"), Some(NthSpec { a: 2, b: 1 }));
    }

    #[test]
    fn nth_spec_str_formulas() {
        let cases = [
            ("n", (1, 0)),
            ("+n", (1, 0)),
            ("-n", (-1, 0)),
            ("2n", (2, 0)),
            ("2n+1", (2, 1)),
            ("2n-1", (2, -1)),
            ("-2n+3", (-2, 3)),
            ("3n+0", (3, 0)),
            ("5", (0, 5)),
            ("-5", (0, -5)),
            ("2n + 1", (2, 1)), // пробелы допустимы
            ("  2n  ", (2, 0)),
        ];
        for (s, (a, b)) in cases {
            assert_eq!(
                parse_nth_spec_str(s),
                Some(NthSpec { a, b }),
                "input={s}"
            );
        }
    }

    #[test]
    fn nth_spec_str_invalid() {
        assert_eq!(parse_nth_spec_str(""), None);
        assert_eq!(parse_nth_spec_str("abc"), None);
        assert_eq!(parse_nth_spec_str("2x+1"), None);
        assert_eq!(parse_nth_spec_str("n+"), None); // нет числа после знака
    }

    #[test]
    fn nth_spec_matches_arithmetic() {
        let odd = NthSpec::ODD; // 2n+1: 1, 3, 5, ...
        for i in [1, 3, 5, 7, 999] {
            assert!(odd.matches(i), "i={i}");
        }
        for i in [0, 2, 4, -1] {
            assert!(!odd.matches(i), "i={i}");
        }
    }

    #[test]
    fn nth_spec_matches_first_three() {
        // -n+3 → элементы 1, 2, 3 (n=2, 1, 0). Индексы в CSS — 1-based,
        // нулевой случай в реальном matching-е не возникает.
        let spec = NthSpec { a: -1, b: 3 };
        assert!(spec.matches(1));
        assert!(spec.matches(2));
        assert!(spec.matches(3));
        assert!(!spec.matches(4));
        assert!(!spec.matches(5));
    }

    #[test]
    fn nth_spec_matches_constant() {
        // 5 → ровно пятый.
        let spec = NthSpec { a: 0, b: 5 };
        assert!(spec.matches(5));
        assert!(!spec.matches(4));
        assert!(!spec.matches(10));
    }

    #[test]
    fn pseudo_nth_variants_parsed() {
        let cases = [
            ("nth-child", "(2n+1)"),
            ("nth-last-child", "(odd)"),
            ("nth-of-type", "(3)"),
            ("nth-last-of-type", "(-n+2)"),
        ];
        for (name, arg) in cases {
            let s = parse(&format!(":{name}{arg} {{}}"));
            let p = &s.rules[0].selectors[0].head.parts[0];
            let pc = match p {
                SimpleSelector::PseudoClass(pc) => pc,
                _ => panic!("expected pseudo-class for :{name}{arg}"),
            };
            let is_nth = matches!(
                pc,
                PseudoClass::NthChild(_, _)
                    | PseudoClass::NthLastChild(_, _)
                    | PseudoClass::NthOfType(_)
                    | PseudoClass::NthLastOfType(_)
            );
            assert!(is_nth, "name={name} got {pc:?}");
        }
    }

    #[test]
    fn pseudo_nth_invalid_arg_falls_back_to_unsupported() {
        let s = parse(":nth-child(abc) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) => {
                assert_eq!(n, "nth-child");
            }
            _ => panic!("expected Unsupported(nth-child), got {p:?}"),
        }
        // Парсер должен дойти до конца правила и не оставить мусора.
        assert_eq!(s.rules[0].declarations[0].value, "red");
    }

    // ──────────────── functional pseudo: :not ────────────────

    #[test]
    fn pseudo_not_simple() {
        let s = parse(":not(.foo) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts, vec![SimpleSelector::Class("foo".into())]);
                assert!(list[0].tail.is_empty());
            }
            _ => panic!("expected :not(.foo), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_compound() {
        let s = parse(":not(p.hl) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts.len(), 2);
                assert!(matches!(&list[0].head.parts[0], SimpleSelector::Type(t) if t == "p"));
                assert!(matches!(&list[0].head.parts[1], SimpleSelector::Class(c) if c == "hl"));
            }
            _ => panic!("expected :not(p.hl)"),
        }
    }

    #[test]
    fn pseudo_not_with_combinator_l4() {
        // CSS Selectors L4 §5.4: combinator-ы внутри `:not` разрешены.
        let s = parse(":not(a > b) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].tail.len(), 1);
                assert_eq!(list[0].tail[0].0, Combinator::Child);
            }
            _ => panic!("expected :not(a > b), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_nested_l4() {
        // CSS Selectors L4 §5.4: nested `:not(:not(.x))` разрешён.
        let s = parse(":not(:not(.x)) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(outer)) => {
                assert_eq!(outer.len(), 1);
                let inner_part = &outer[0].head.parts[0];
                assert!(matches!(
                    inner_part,
                    SimpleSelector::PseudoClass(PseudoClass::Not(inner)) if inner.len() == 1
                ));
            }
            _ => panic!("expected :not(:not(.x)), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_selector_list() {
        // CSS Selectors L4 §5.4: список селекторов.
        let s = parse(":not(.foo, #bar) { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        match p {
            SimpleSelector::PseudoClass(PseudoClass::Not(list)) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts, vec![SimpleSelector::Class("foo".into())]);
                assert_eq!(list[1].head.parts, vec![SimpleSelector::Id("bar".into())]);
            }
            _ => panic!("expected :not(.foo, #bar), got {p:?}"),
        }
    }

    #[test]
    fn pseudo_not_empty_falls_back() {
        // `:not()` без аргументов — невалидно, должен дать Unsupported.
        let s = parse(":not() { color: red; }");
        let p = &s.rules[0].selectors[0].head.parts[0];
        assert!(
            matches!(p, SimpleSelector::PseudoClass(PseudoClass::Unsupported(n)) if n == "not"),
            "got {p:?}"
        );
    }

    #[test]
    fn specificity_not_uses_inner() {
        // :not(.foo) → max-of-list = (.foo) даёт b=1; сам :not — ноль.
        let s = parse(":not(.foo) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_not_with_id() {
        // :not(#x) → a=1, b=0, c=0.
        let s = parse(":not(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_not_list_takes_max() {
        // CSS Selectors L4 §16: `:not(.foo, #bar)` contributes max-specificity
        // по списку = (#bar) = (1,0,0).
        let s = parse(":not(.foo, #bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_not_complex_with_combinator() {
        // `:not(a > b)` → max specificity selector-а внутри = (0, 0, 2) (a + b
        // как type selectors).
        let s = parse(":not(a > b) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 2 }
        );
    }

    // ──────────────── functional pseudo: :is, :where ────────────────

    fn pseudo_at(s: &Stylesheet, rule: usize, sel: usize, part: usize) -> &PseudoClass {
        match &s.rules[rule].selectors[sel].head.parts[part] {
            SimpleSelector::PseudoClass(pc) => pc,
            other => panic!("expected pseudo-class, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_is_class_list() {
        let s = parse(":is(.foo, .bar) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Is(list) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts, vec![SimpleSelector::Class("foo".into())]);
                assert_eq!(list[1].head.parts, vec![SimpleSelector::Class("bar".into())]);
            }
            _ => panic!("expected :is(...), got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_where_class_list() {
        let s = parse(":where(.foo, #bar) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Where(list) if list.len() == 2), "got {pc:?}");
    }

    #[test]
    fn pseudo_is_with_combinator_inside() {
        // CSS4 разрешает combinator-ы внутри :is — в отличие от :not.
        let s = parse(":is(a > b, c d) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Is(list) => {
                assert_eq!(list.len(), 2);
                // a > b: head 'a', tail [(Child, 'b')]
                assert_eq!(list[0].tail.len(), 1);
                assert_eq!(list[0].tail[0].0, Combinator::Child);
                // c d: head 'c', tail [(Descendant, 'd')]
                assert_eq!(list[1].tail.len(), 1);
                assert_eq!(list[1].tail[0].0, Combinator::Descendant);
            }
            _ => panic!("expected :is, got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_is_with_type_selector() {
        let s = parse("article :is(h1, h2) { color: red; }");
        let sel = &s.rules[0].selectors[0];
        // head = 'article', tail = [(Descendant, compound{:is(h1, h2)})]
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("article".into())]);
        assert_eq!(sel.tail.len(), 1);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert!(matches!(
            &sel.tail[0].1.parts[0],
            SimpleSelector::PseudoClass(PseudoClass::Is(list)) if list.len() == 2
        ));
    }

    #[test]
    fn pseudo_is_empty_falls_back() {
        // `:is()` без аргументов — невалидно, должен дать Unsupported.
        let s = parse(":is() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "is"), "got {pc:?}");
    }

    #[test]
    fn pseudo_where_empty_falls_back() {
        let s = parse(":where() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "where"), "got {pc:?}");
    }

    #[test]
    fn specificity_is_takes_max_of_list() {
        // :is(.foo, #bar) → max = (#bar) = (1,0,0).
        let s = parse(":is(.foo, #bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_is_only_classes() {
        // :is(.foo, .bar) → max = (0,1,0).
        let s = parse(":is(.foo, .bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }

    #[test]
    fn specificity_where_always_zero() {
        // :where(#x) → 0,0,0 даже при id внутри.
        let s = parse(":where(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_where_combined_with_outer() {
        // `p:where(#x)` → p (c=1), :where contributes 0 → (0,0,1).
        let s = parse("p:where(#x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 0, c: 1 }
        );
    }

    #[test]
    fn pseudo_is_with_whitespace_around_list() {
        // Внутри `:is( .foo , .bar )` бывают пробелы — парсер не должен терять
        // последний селектор из-за trailing whitespace перед `)`.
        let s = parse(":is( .foo , .bar ) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Is(list) if list.len() == 2), "got {pc:?}");
    }

    // ──────────────── :has() (CSS Selectors L4 §17.2) ────────────────

    #[test]
    fn pseudo_has_descendant_implicit() {
        // `article:has(img)` — implicit descendant.
        let s = parse("article:has(img) { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        assert_eq!(head.parts.len(), 2);
        assert!(matches!(&head.parts[0], SimpleSelector::Type(t) if t == "article"));
        match &head.parts[1] {
            SimpleSelector::PseudoClass(PseudoClass::Has(list)) => {
                assert_eq!(list.len(), 1);
                assert!(list[0].combinator.is_none());
                assert_eq!(list[0].selector.head.parts, vec![SimpleSelector::Type("img".into())]);
            }
            other => panic!("expected :has, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_has_with_child_combinator() {
        // `:has(> .featured)` — прямой child.
        let s = parse(":has(> .featured) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Has(list) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].combinator, Some(Combinator::Child));
                assert_eq!(list[0].selector.head.parts, vec![SimpleSelector::Class("featured".into())]);
            }
            _ => panic!("expected :has, got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_has_with_next_sibling() {
        // `h1:has(+ p)` — h1 followed by p.
        let s = parse("h1:has(+ p) { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        match &head.parts[1] {
            SimpleSelector::PseudoClass(PseudoClass::Has(list)) => {
                assert_eq!(list[0].combinator, Some(Combinator::NextSibling));
            }
            other => panic!("expected :has, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_has_with_later_sibling() {
        let s = parse("h1:has(~ p) { color: red; }");
        let head = &s.rules[0].selectors[0].head;
        match &head.parts[1] {
            SimpleSelector::PseudoClass(PseudoClass::Has(list)) => {
                assert_eq!(list[0].combinator, Some(Combinator::LaterSibling));
            }
            other => panic!("expected :has, got {other:?}"),
        }
    }

    #[test]
    fn pseudo_has_multiple_relative_selectors() {
        // Список через запятую.
        let s = parse(":has(.a, > .b, + p) { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        match pc {
            PseudoClass::Has(list) => {
                assert_eq!(list.len(), 3);
                assert!(list[0].combinator.is_none());
                assert_eq!(list[1].combinator, Some(Combinator::Child));
                assert_eq!(list[2].combinator, Some(Combinator::NextSibling));
            }
            _ => panic!("expected :has, got {pc:?}"),
        }
    }

    #[test]
    fn pseudo_has_empty_falls_back() {
        let s = parse(":has() { color: red; }");
        let pc = pseudo_at(&s, 0, 0, 0);
        assert!(matches!(pc, PseudoClass::Unsupported(n) if n == "has"), "got {pc:?}");
    }

    #[test]
    fn specificity_has_takes_max_of_inner() {
        // :has(.foo, #bar) → max = (1,0,0) от #bar.
        let s = parse(":has(.foo, #bar) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 1, b: 0, c: 0 }
        );
    }

    #[test]
    fn specificity_has_combinator_does_not_count() {
        // `:has(> .x)` — combinator не contributes specificity, только .x = (0,1,0).
        let s = parse(":has(> .x) { color: red; }");
        assert_eq!(
            s.rules[0].selectors[0].specificity(),
            Specificity { a: 0, b: 1, c: 0 }
        );
    }


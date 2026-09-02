use super::*;

    // --- CSS Nesting L1 ---

    fn two(a: SimpleSelector, comb: Combinator, b: SimpleSelector) -> ComplexSelector {
        ComplexSelector {
            head: CompoundSelector { parts: vec![a] },
            tail: vec![(comb, CompoundSelector { parts: vec![b] })],
        }
    }

    #[test]
    fn nesting_descendant_simple() {
        // `div { color: red; & span { color: blue; } }` →
        // 2 rules: div { color: red } and div span { color: blue }
        let s = parse("div { color: red; & span { color: blue; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Type("div".into()))]);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[0].value, "red");
        assert_eq!(
            s.rules[1].selectors,
            vec![two(SimpleSelector::Type("div".into()), Combinator::Descendant, SimpleSelector::Type("span".into()))]
        );
        assert_eq!(s.rules[1].declarations[0].property, "color");
        assert_eq!(s.rules[1].declarations[0].value, "blue");
    }

    #[test]
    fn nesting_child_combinator() {
        // `ul { & > li { list-style: none; } }`
        let s = parse("ul { & > li { list-style: none; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(
            s.rules[1].selectors,
            vec![two(SimpleSelector::Type("ul".into()), Combinator::Child, SimpleSelector::Type("li".into()))]
        );
        assert_eq!(s.rules[1].declarations[0].property, "list-style");
    }

    #[test]
    fn nesting_compound_join() {
        // `div { &.active { color: red; } }` → `div.active { color: red; }`
        let s = parse("div { &.active { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let sel = &s.rules[1].selectors[0];
        assert_eq!(sel.head.parts, vec![
            SimpleSelector::Type("div".into()),
            SimpleSelector::Class("active".into()),
        ]);
        assert!(sel.tail.is_empty());
    }

    #[test]
    fn nesting_bare_amp() {
        // `div { & { color: red; } }` → `div { color: red; }` (same element)
        let s = parse("div { color: blue; & { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[1].selectors, vec![one(SimpleSelector::Type("div".into()))]);
        assert_eq!(s.rules[1].declarations[0].value, "red");
    }

    #[test]
    fn nesting_multiple_parent_selectors() {
        // `h1, h2 { & span { color: red; } }` → `h1 span` and `h2 span`
        let s = parse("h1, h2 { & span { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested_sels = &s.rules[1].selectors;
        assert_eq!(nested_sels.len(), 2);
        assert_eq!(
            nested_sels[0],
            two(SimpleSelector::Type("h1".into()), Combinator::Descendant, SimpleSelector::Type("span".into()))
        );
        assert_eq!(
            nested_sels[1],
            two(SimpleSelector::Type("h2".into()), Combinator::Descendant, SimpleSelector::Type("span".into()))
        );
    }

    #[test]
    fn nesting_deep_two_levels() {
        // `div { & p { & em { color: red; } } }` → 3 rules: div, div p, div p em
        let s = parse("div { & p { & em { color: red; } } }");
        assert_eq!(s.rules.len(), 3);
        // div p em
        let sel = &s.rules[2].selectors[0];
        assert_eq!(sel.head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(sel.tail.len(), 2);
        assert_eq!(sel.tail[0].0, Combinator::Descendant);
        assert_eq!(sel.tail[0].1.parts, vec![SimpleSelector::Type("p".into())]);
        assert_eq!(sel.tail[1].0, Combinator::Descendant);
        assert_eq!(sel.tail[1].1.parts, vec![SimpleSelector::Type("em".into())]);
    }

    #[test]
    fn nesting_declarations_not_mixed_with_nested() {
        // Declarations and nested rules don't interfere
        let s = parse("p { margin: 0; & b { font-weight: bold; } padding: 5px; }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].declarations.len(), 2); // margin + padding
        assert_eq!(s.rules[0].declarations[0].property, "margin");
        assert_eq!(s.rules[0].declarations[1].property, "padding");
        assert_eq!(s.rules[1].declarations[0].property, "font-weight");
    }

    // ── CSS Nesting L1 §4: implicit nesting (without `&`) ───────────────────

    #[test]
    fn implicit_nesting_class_descendant() {
        // `.parent { .child { color: blue; } }` → `.parent .child { color: blue; }`
        let s = parse(".parent { .child { color: blue; } }");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.rules[0].selectors, vec![one(SimpleSelector::Class("parent".into()))]);
        assert!(s.rules[0].declarations.is_empty());
        // Nested rule: `.parent .child`
        let nested = &s.rules[1];
        assert_eq!(nested.selectors.len(), 1);
        assert_eq!(nested.selectors[0].head.parts, vec![SimpleSelector::Class("parent".into())]);
        assert_eq!(nested.selectors[0].tail.len(), 1);
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Class("child".into())]
        );
        assert_eq!(nested.declarations[0].property, "color");
        assert_eq!(nested.declarations[0].value, "blue");
    }

    #[test]
    fn implicit_nesting_id_descendant() {
        // `div { #hero { color: red; } }` → `div #hero { color: red; }`
        let s = parse("div { #hero { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Id("hero".into())]
        );
    }

    #[test]
    fn implicit_nesting_pseudo_class() {
        // `.btn { :hover { opacity: 0.8; } }` → `.btn :hover { opacity: 0.8; }`
        let s = parse(".btn { :hover { opacity: 0.8; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        // :hover is PseudoClass::Unsupported("hover") since not stateful-matched
        assert_eq!(nested.declarations[0].property, "opacity");
        assert_eq!(nested.declarations[0].value, "0.8");
    }

    #[test]
    fn implicit_nesting_universal() {
        // `div { * { box-sizing: border-box; } }` → `div * { box-sizing: border-box; }`
        let s = parse("div { * { box-sizing: border-box; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Universal]
        );
    }

    #[test]
    fn implicit_nesting_relative_child() {
        // `ul { > li { list-style: none; } }` → `ul > li { list-style: none; }`
        let s = parse("ul { > li { list-style: none; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Child);
        assert_eq!(
            nested.selectors[0].tail[0].1.parts,
            vec![SimpleSelector::Type("li".into())]
        );
        assert_eq!(nested.declarations[0].property, "list-style");
    }

    #[test]
    fn implicit_nesting_relative_next_sibling() {
        // `.a { + .b { color: red; } }` → `.a + .b { color: red; }`
        let s = parse(".a { + .b { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::NextSibling);
    }

    #[test]
    fn implicit_nesting_relative_later_sibling() {
        // `.a { ~ .b { color: red; } }` → `.a ~ .b { color: red; }`
        let s = parse(".a { ~ .b { color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::LaterSibling);
    }

    #[test]
    fn implicit_nesting_with_declarations_mixed() {
        // `.card { color: red; .title { font-weight: bold; } padding: 8px; }`
        let s = parse(".card { color: red; .title { font-weight: bold; } padding: 8px; }");
        assert_eq!(s.rules.len(), 2);
        // Parent keeps both declarations
        assert_eq!(s.rules[0].declarations.len(), 2);
        assert_eq!(s.rules[0].declarations[0].property, "color");
        assert_eq!(s.rules[0].declarations[1].property, "padding");
        // Nested rule gets its declaration
        assert_eq!(s.rules[1].declarations[0].property, "font-weight");
    }

    #[test]
    fn implicit_nesting_deep_two_levels() {
        // `div { .a { .b { color: red; } } }` → 3 rules: div, div .a, div .a .b
        let s = parse("div { .a { .b { color: red; } } }");
        assert_eq!(s.rules.len(), 3);
        let deepest = &s.rules[2];
        assert_eq!(deepest.selectors[0].head.parts, vec![SimpleSelector::Type("div".into())]);
        assert_eq!(deepest.selectors[0].tail.len(), 2);
        assert_eq!(deepest.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(deepest.selectors[0].tail[1].0, Combinator::Descendant);
    }

    #[test]
    fn implicit_nesting_attribute_selector() {
        // `form { [required] { border-color: red; } }` → `form [required] { border-color: red; }`
        let s = parse("form { [required] { border-color: red; } }");
        assert_eq!(s.rules.len(), 2);
        let nested = &s.rules[1];
        assert_eq!(nested.selectors[0].tail[0].0, Combinator::Descendant);
        assert_eq!(nested.declarations[0].property, "border-color");
    }

    // ── CSS Nesting L1 §5: nested at-rules ─────────────────────────────────

    #[test]
    fn nested_at_media_basic() {
        // `.card { @media (min-width: 800px) { color: blue; } }`
        // → `@media (min-width: 800px) { .card { color: blue; } }` in media_rules
        let s = parse(".card { @media (min-width: 800px) { color: blue; } }");
        assert_eq!(s.rules.len(), 1); // parent rule (no decls)
        assert_eq!(s.media_rules.len(), 1);
        let mr = &s.media_rules[0];
        assert_eq!(mr.rules.len(), 1);
        assert_eq!(mr.rules[0].selectors, vec![one(SimpleSelector::Class("card".into()))]);
        assert_eq!(mr.rules[0].declarations[0].property, "color");
        assert_eq!(mr.rules[0].declarations[0].value, "blue");
    }

    #[test]
    fn nested_at_media_with_nested_rule() {
        // `.parent { @media (max-width: 600px) { .child { color: red; } } }`
        // → `@media ... { .parent .child { color: red; } }`
        let s = parse(".parent { @media (max-width: 600px) { .child { color: red; } } }");
        assert_eq!(s.media_rules.len(), 1);
        let mr = &s.media_rules[0];
        // .parent (empty decls rule from parent) is absent since decls empty; only the nested rule
        assert!(mr.rules.iter().any(|r| {
            r.selectors.iter().any(|sel| {
                sel.head.parts == vec![SimpleSelector::Class("parent".into())]
                    && sel.tail.len() == 1
                    && sel.tail[0].1.parts == vec![SimpleSelector::Class("child".into())]
            })
        }));
        let nested = mr.rules.iter().find(|r| r.selectors[0].tail.len() == 1).unwrap();
        assert_eq!(nested.declarations[0].property, "color");
    }

    #[test]
    fn nested_at_media_mixed_decls_and_rules() {
        // `.el { @media screen { color: red; .inner { opacity: 0.5; } } }`
        // → media_rules has: [.el { color: red }, .el .inner { opacity: 0.5 }]
        let s =
            parse(".el { @media screen { color: red; .inner { opacity: 0.5; } } }");
        assert_eq!(s.media_rules.len(), 1);
        let mr = &s.media_rules[0];
        assert_eq!(mr.rules.len(), 2);
        // First rule: .el { color: red }
        assert_eq!(mr.rules[0].selectors, vec![one(SimpleSelector::Class("el".into()))]);
        assert_eq!(mr.rules[0].declarations[0].property, "color");
        // Second rule: .el .inner { opacity: 0.5 }
        assert_eq!(mr.rules[1].declarations[0].property, "opacity");
    }

    #[test]
    fn nested_at_supports() {
        // `div { @supports (display: grid) { display: grid; } }`
        // → `@supports ... { div { display: grid; } }` in supports_rules
        let s = parse("div { @supports (display: grid) { display: grid; } }");
        assert_eq!(s.supports_rules.len(), 1);
        let sr = &s.supports_rules[0];
        assert_eq!(sr.rules.len(), 1);
        assert_eq!(sr.rules[0].selectors, vec![one(SimpleSelector::Type("div".into()))]);
        assert_eq!(sr.rules[0].declarations[0].property, "display");
    }

    #[test]
    fn nested_at_layer() {
        // `.btn { @layer base { color: red; } }`
        // → `@layer base { .btn { color: red; } }` in layers
        let s = parse(".btn { @layer base { color: red; } }");
        assert_eq!(s.layers.len(), 1);
        assert_eq!(s.layers[0].name, "base");
        assert_eq!(s.layers[0].rules.len(), 1);
        assert_eq!(s.layers[0].rules[0].declarations[0].property, "color");
    }

    #[test]
    fn nested_at_container() {
        // `.grid { @container sidebar (min-width: 300px) { gap: 1rem; } }`
        let s = parse(".grid { @container sidebar (min-width: 300px) { gap: 1rem; } }");
        assert_eq!(s.container_rules.len(), 1);
        let cr = &s.container_rules[0];
        assert_eq!(cr.rules.len(), 1);
        assert_eq!(cr.rules[0].declarations[0].property, "gap");
    }

    // ──────────────── :host pseudo-class (CSS Scoping L1 §6.1) ────────────

    #[test]
    fn host_pseudo_class_simple() {
        // `:host { color: red; }` — простой :host без аргументов
        let s = parse(":host { color: red; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 1);
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(None)) => {}
            _ => panic!("Expected Host(None), got {:?}", sel.head.parts[0]),
        }
        assert_eq!(s.rules[0].declarations[0].property, "color");
    }

    #[test]
    fn host_pseudo_class_with_selector_list() {
        // `:host(.foo) { display: block; }` — :host(selector-list)
        let s = parse(":host(.foo) { display: block; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("foo".into()));
            }
            _ => panic!("Expected Host(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn host_pseudo_class_multiple_selectors_in_list() {
        // `:host(.primary, .secondary) { border: 1px solid; }` — multiple selectors
        let s = parse(":host(.primary, .secondary) { border: 1px solid; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(Some(list))) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("primary".into()));
                assert_eq!(list[1].head.parts[0], SimpleSelector::Class("secondary".into()));
            }
            _ => panic!("Expected Host(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn host_pseudo_class_with_complex_selector() {
        // `:host(div.wrapper) { padding: 10px; }` — complex selector inside
        let s = parse(":host(div.wrapper) { padding: 10px; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoClass(PseudoClass::Host(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Type("div".into()));
                assert_eq!(list[0].head.parts[1], SimpleSelector::Class("wrapper".into()));
            }
            _ => panic!("Expected Host(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    // ────────────────── ::slotted pseudo-element (CSS Scoping L1 §6.2) ──

    #[test]
    fn slotted_pseudo_element_simple() {
        // `::slotted(.slot-content) { color: blue; }` — :slotted with selector
        let s = parse("::slotted(.slot-content) { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 1);
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("slot-content".into()));
            }
            _ => panic!("Expected Slotted(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn slotted_pseudo_element_multiple_selectors() {
        // `::slotted(.primary, .secondary) { margin: 5px; }` — multiple selectors
        let s = parse("::slotted(.primary, .secondary) { margin: 5px; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Class("primary".into()));
                assert_eq!(list[1].head.parts[0], SimpleSelector::Class("secondary".into()));
            }
            _ => panic!("Expected Slotted(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn slotted_pseudo_element_with_type_selector() {
        // `::slotted(input[type="text"]) { border-color: green; }` — type selector with attribute
        let s = parse("::slotted(input[type=\"text\"]) { border-color: green; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Slotted(Some(list))) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].head.parts[0], SimpleSelector::Type("input".into()));
                assert!(list[0].head.parts.iter().any(|p| matches!(p, SimpleSelector::Attribute(_))));
            }
            _ => panic!("Expected Slotted(Some(...)), got {:?}", sel.head.parts[0]),
        }
    }

    // ────────────────── ::highlight pseudo-element (CSS Highlight API L1 §3) ──

    #[test]
    fn highlight_pseudo_element_simple() {
        // `::highlight(search) { color: red; background: yellow; }` — simple name
        let s = parse("::highlight(search) { color: red; background: yellow; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 1);
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Highlight(name)) => {
                assert_eq!(name, "search");
            }
            _ => panic!("Expected Highlight(\"search\"), got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn highlight_pseudo_element_custom_name() {
        // `::highlight(custom-highlight-name) { ... }` — hyphenated name
        let s = parse("::highlight(custom-highlight-name) { color: blue; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        match &sel.head.parts[0] {
            SimpleSelector::PseudoElement(PseudoElementKind::Highlight(name)) => {
                assert_eq!(name, "custom-highlight-name");
            }
            _ => panic!("Expected Highlight with name, got {:?}", sel.head.parts[0]),
        }
    }

    #[test]
    fn highlight_pseudo_element_with_combinator() {
        // `span::highlight(spelling) { color: red; }` — type selector + highlight
        let s = parse("span::highlight(spelling) { color: red; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 2);
        assert_eq!(sel.head.parts[0], SimpleSelector::Type("span".into()));
        match &sel.head.parts[1] {
            SimpleSelector::PseudoElement(PseudoElementKind::Highlight(name)) => {
                assert_eq!(name, "spelling");
            }
            _ => panic!("Expected Highlight pseudo-element, got {:?}", sel.head.parts[1]),
        }
    }

    // ── Customizable Select pseudo-elements (HTML/CSS «base-select») ─────────

    #[test]
    fn picker_select_pseudo_element() {
        // `::picker(select) { ... }` — functional picker pseudo-element.
        let s = parse("select::picker(select) { background: white; }");
        assert_eq!(s.rules.len(), 1);
        let sel = &s.rules[0].selectors[0];
        assert_eq!(sel.head.parts.len(), 2);
        assert_eq!(sel.head.parts[0], SimpleSelector::Type("select".into()));
        match &sel.head.parts[1] {
            SimpleSelector::PseudoElement(PseudoElementKind::Picker(arg)) => {
                assert_eq!(arg, "select");
            }
            _ => panic!("Expected Picker(\"select\"), got {:?}", sel.head.parts[1]),
        }
    }

    #[test]
    fn checkmark_and_picker_icon_pseudo_elements() {
        // Simple `::checkmark` and `::picker-icon` pseudo-elements.
        let s = parse("option::checkmark { color: green; } select::picker-icon { color: blue; }");
        assert_eq!(s.rules.len(), 2);
        match &s.rules[0].selectors[0].head.parts[1] {
            SimpleSelector::PseudoElement(PseudoElementKind::Checkmark) => {}
            other => panic!("Expected Checkmark, got {other:?}"),
        }
        match &s.rules[1].selectors[0].head.parts[1] {
            SimpleSelector::PseudoElement(PseudoElementKind::PickerIcon) => {}
            other => panic!("Expected PickerIcon, got {other:?}"),
        }
    }

    // ── @font-palette-values tests ──────────────────────────────────────────

    #[test]
    fn font_palette_values_basic() {
        let s = parse(r#"@font-palette-values --warm { font-family: "Bungee Spore"; base-palette: 0; }"#);
        assert_eq!(s.font_palette_values.len(), 1);
        let fp = &s.font_palette_values[0];
        assert_eq!(fp.name, "--warm");
        assert_eq!(fp.font_family.as_deref(), Some("Bungee Spore"));
        assert_eq!(fp.base_palette, Some(0));
        assert!(fp.override_colors.is_empty());
    }

    #[test]
    fn font_palette_values_override_colors() {
        let s = parse("@font-palette-values --cool { override-colors: 0 #ff0000, 1 #00ff00; }");
        let fp = &s.font_palette_values[0];
        assert_eq!(fp.override_colors.len(), 2);
        assert_eq!(fp.override_colors[0], (0, "#ff0000".to_string()));
        assert_eq!(fp.override_colors[1], (1, "#00ff00".to_string()));
    }

    #[test]
    fn font_palette_values_multiple_rules() {
        let s = parse(
            "@font-palette-values --a { base-palette: 1; } @font-palette-values --b { base-palette: 2; }",
        );
        assert_eq!(s.font_palette_values.len(), 2);
        assert_eq!(s.font_palette_values[0].name, "--a");
        assert_eq!(s.font_palette_values[1].name, "--b");
    }

    #[test]
    fn font_palette_values_no_double_dash_ignored() {
        // Prelude without '--' is invalid per CSS Fonts L4 §13 — treated as unknown.
        let s = parse("@font-palette-values myname { base-palette: 0; }");
        assert!(s.font_palette_values.is_empty());
    }

    #[test]
    fn font_palette_values_base_palette_none_when_absent() {
        let s = parse("@font-palette-values --x { font-family: F; }");
        assert_eq!(s.font_palette_values[0].base_palette, None);
    }

    #[test]
    fn font_palette_values_coexists_with_other_rules() {
        let s = parse(
            r#"div { color: red; } @font-palette-values --p { base-palette: 3; } p { margin: 0; }"#,
        );
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.font_palette_values.len(), 1);
        assert_eq!(s.font_palette_values[0].base_palette, Some(3));
    }

    // ── @color-profile tests ────────────────────────────────────────────────

    #[test]
    fn color_profile_basic() {
        let s = parse(r#"@color-profile --swop5c { src: url("swop5c.icc"); rendering-intent: relative-colorimetric; }"#);
        assert_eq!(s.color_profiles.len(), 1);
        let cp = &s.color_profiles[0];
        assert_eq!(cp.name, "--swop5c");
        assert_eq!(cp.src.as_deref(), Some("swop5c.icc"));
        assert_eq!(cp.rendering_intent.as_deref(), Some("relative-colorimetric"));
    }

    #[test]
    fn color_profile_src_only() {
        let s = parse(r#"@color-profile --display-p3 { src: url("display-p3.icc"); }"#);
        let cp = &s.color_profiles[0];
        assert_eq!(cp.src.as_deref(), Some("display-p3.icc"));
        assert_eq!(cp.rendering_intent, None);
    }

    #[test]
    fn color_profile_multiple_rules() {
        let s = parse(
            r#"@color-profile --a { src: url("a.icc"); } @color-profile --b { src: url("b.icc"); }"#,
        );
        assert_eq!(s.color_profiles.len(), 2);
        assert_eq!(s.color_profiles[0].name, "--a");
        assert_eq!(s.color_profiles[1].name, "--b");
    }

    #[test]
    fn color_profile_no_double_dash_ignored() {
        // Prelude without '--' is invalid per CSS Color L5 §4 — treated as unknown.
        let s = parse(r#"@color-profile myname { src: url("a.icc"); }"#);
        assert!(s.color_profiles.is_empty());
    }

    #[test]
    fn color_profile_coexists_with_other_rules() {
        let s = parse(
            r#"div { color: red; } @color-profile --p { src: url("p.icc"); } p { margin: 0; }"#,
        );
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.color_profiles.len(), 1);
        assert_eq!(s.color_profiles[0].name, "--p");
    }

    // ── @function tests ─────────────────────────────────────────────────────

    #[test]
    fn function_basic_single_param() {
        let s = parse("@function --double(--x) { result: calc(var(--x) * 2); }");
        assert_eq!(s.function_rules.len(), 1);
        let f = &s.function_rules[0];
        assert_eq!(f.name, "--double");
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].name, "--x");
        assert_eq!(f.parameters[0].default, None);
        assert_eq!(f.declarations.len(), 1);
        assert_eq!(f.declarations[0].property, "result");
        assert_eq!(f.declarations[0].value, "calc(var(--x) * 2)");
    }

    #[test]
    fn function_multiple_params_with_default() {
        let s = parse("@function --pad(--a, --b: 10px) { result: var(--a); }");
        let f = &s.function_rules[0];
        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[0].name, "--a");
        assert_eq!(f.parameters[0].default, None);
        assert_eq!(f.parameters[1].name, "--b");
        assert_eq!(f.parameters[1].default.as_deref(), Some("10px"));
    }

    #[test]
    fn function_zero_params() {
        let s = parse("@function --pi() { result: 3.14159; }");
        let f = &s.function_rules[0];
        assert_eq!(f.name, "--pi");
        assert!(f.parameters.is_empty());
    }

    #[test]
    fn function_returns_type_stored_raw() {
        let s = parse("@function --double(--x) returns <length> { result: calc(var(--x) * 2); }");
        let f = &s.function_rules[0];
        assert_eq!(f.returns.as_deref(), Some("<length>"));
    }

    #[test]
    fn function_local_declarations_and_result_order_preserved() {
        let s = parse(
            "@function --clamped(--min, --val, --max) { \
                 --c: clamp(var(--min), var(--val), var(--max)); \
                 result: var(--c); \
             }",
        );
        let f = &s.function_rules[0];
        assert_eq!(f.declarations.len(), 2);
        assert_eq!(f.declarations[0].property, "--c");
        assert_eq!(f.declarations[1].property, "result");
    }

    #[test]
    fn function_name_without_double_dash_ignored() {
        // Prelude must be a dashed-ident (function-token grammar); a bare
        // ident is not a valid `@function` name per CSS Functions & Mixins L1.
        let s = parse("@function double(--x) { result: var(--x); }");
        assert!(s.function_rules.is_empty());
    }

    #[test]
    fn function_multiple_rules_and_coexists_with_other_at_rules() {
        let s = parse(
            r#"@function --a() { result: 1px; } div { color: red; } @function --b() { result: 2px; }"#,
        );
        assert_eq!(s.function_rules.len(), 2);
        assert_eq!(s.function_rules[0].name, "--a");
        assert_eq!(s.function_rules[1].name, "--b");
        assert_eq!(s.rules.len(), 1);
    }

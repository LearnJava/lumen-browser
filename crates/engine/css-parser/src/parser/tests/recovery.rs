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

//! Тесты `style.rs`: фан-аут инкрементального рестайла (BUG-341): индекс каскада
//! и пер-узловые псевдоэлементные каскады.
//!
//! Перенесено батчем SPLIT-ST1 без правок тел.

use super::*;

    // ── BUG-341 S10: per-node pseudo-element cascades ────────────────────
    //
    // The profile of the *incremental* path (not the full-layout profile in
    // the brief's §1) put 55% of `compute_style` in the three
    // `::-webkit-scrollbar*` cascades run for every element, and most of the
    // rest of `precompute_counters` in the `::before`/`::after` probe run for
    // every node — including nodes whose style was reused wholesale. Neither
    // shows up in any differential test, because doing the work and throwing
    // the result away produces identical output. Hence counter gates.

    /// Elements in `html`, each with its own computed style, cascaded in
    /// document order so parent styles are the real inherited ones.
    fn cascade_all(html: &str, css: &str) -> Vec<(String, ComputedStyle)> {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let vp = Size::new(800.0, 600.0);
        let mut out = Vec::new();
        let mut stack = vec![(doc.root(), ComputedStyle::root())];
        while let Some((id, inherited)) = stack.pop() {
            let style = compute_style(&doc, id, &sheet, &inherited, vp, false);
            for &child in doc.get(id).children.iter().rev() {
                stack.push((child, style.clone()));
            }
            if let Some(q) = doc.get(id).element_name() {
                out.push((q.local.to_string(), style));
            }
        }
        out
    }

    /// The one element with tag `tag` in a `cascade_all` result.
    fn only(styles: &[(String, ComputedStyle)], tag: &str) -> ComputedStyle {
        let mut found = styles.iter().filter(|(t, _)| t == tag);
        let style = found.next().unwrap_or_else(|| panic!("no <{tag}> cascaded")).1.clone();
        assert!(found.next().is_none(), "more than one <{tag}>");
        style
    }

    #[test]
    fn webkit_scrollbar_cascade_skipped_when_sheet_declares_none() {
        reset_scrollbar_pseudo_cascades();
        let styles = cascade_all(
            "<div><p>a</p><p>b</p></div>",
            "div { color: red; } p { margin: 1px; }",
        );
        assert!(styles.len() >= 4, "html/body/div/p… all cascaded");
        assert_eq!(
            scrollbar_pseudo_cascades(),
            0,
            "a sheet without a `::-webkit-scrollbar*` rule must not run the \
             pseudo-element cascade even once"
        );
    }

    // BUG-341 S11: the translation runs only where a scrollbar can appear.
    // `element_can_have_scrollbar`'s doc comment has the reasoning; these pin
    // both halves of it — that scroll containers still get styled, and that
    // nothing else pays for (or inherits) the translation.

    #[test]
    fn webkit_scrollbar_cascade_only_for_scroll_containers() {
        reset_scrollbar_pseudo_cascades();
        let styles = cascade_all(
            "<div class='sc'>a</div><section><p>b</p></section>",
            ".sc::-webkit-scrollbar { width: 0; } .sc { overflow-y: auto; }",
        );
        assert_eq!(
            only(&styles, "div").scrollbar_width,
            ScrollbarWidth::None,
            "a scroll container must still pick up its `::-webkit-scrollbar` rule"
        );
        assert_eq!(
            only(&styles, "section").scrollbar_width,
            ScrollbarWidth::Auto,
            "an element outside the scroll container's subtree is untouched"
        );
        // `<html>` and `<body>` are in the gate unconditionally (they are the
        // conventional page-scrollbar target), plus the one real scroll
        // container — everything else is skipped.
        assert_eq!(
            scrollbar_pseudo_cascades(),
            3,
            "only html, body and the scroll container may cascade"
        );
    }

    #[test]
    fn webkit_scrollbar_translation_does_not_leak_from_a_non_scrollable_element() {
        // The behaviour BUG-341 S11 removed: a `::-webkit-scrollbar` rule
        // matching a *non-scrollable* element used to write the inherited
        // `scrollbar-width` there, from where every descendant picked it up.
        reset_scrollbar_pseudo_cascades();
        let styles = cascade_all(
            "<div class='plain'><p class='sc'>a</p></div>",
            ".plain::-webkit-scrollbar { width: 0; } .sc { overflow: auto; }",
        );
        assert_eq!(
            only(&styles, "div").scrollbar_width,
            ScrollbarWidth::Auto,
            "`.plain` cannot show a scrollbar, so its rule must not apply"
        );
        assert_eq!(
            only(&styles, "p").scrollbar_width,
            ScrollbarWidth::Auto,
            "and the scrollable descendant, which matches no rule of its own, \
             must not inherit it — WebKit has no such inheritance"
        );
    }

    #[test]
    fn webkit_scrollbar_bare_rule_still_reaches_the_page_through_body() {
        // A bare `::-webkit-scrollbar` (the common page-scrollbar idiom, and
        // what `assets/chrome/chrome.html` uses) matches `<body>`, which is in
        // the gate unconditionally — so the standard inherited property carries
        // the value down exactly as before S11. Nothing on a real page changes.
        let styles = cascade_all(
            "<div><p>a</p></div>",
            "::-webkit-scrollbar { width: 9px; }",
        );
        assert_eq!(only(&styles, "body").scrollbar_width, ScrollbarWidth::Thin);
        assert_eq!(only(&styles, "p").scrollbar_width, ScrollbarWidth::Thin);
    }

    #[test]
    fn webkit_scrollbar_cascade_skipped_for_overflow_hidden() {
        reset_scrollbar_pseudo_cascades();
        let styles = cascade_all(
            "<div class='clip'>a</div>",
            ".clip::-webkit-scrollbar { width: 0; } .clip { overflow: hidden; }",
        );
        // `overflow: hidden` scrolls programmatically but draws no bar — the
        // same condition paint's `emit_scrollbars` uses.
        assert_eq!(only(&styles, "div").scrollbar_width, ScrollbarWidth::Auto);
        assert_eq!(scrollbar_pseudo_cascades(), 2, "html and body only");
    }

    #[test]
    fn standard_scrollbar_width_still_inherits() {
        // The narrowing applies to the `::-webkit-scrollbar*` translation, not
        // to the standard properties: `scrollbar-width` is inherited by CSS
        // Scrollbars L1 §2 and must keep reaching non-scrollable descendants.
        let styles = cascade_all("<div><p>a</p></div>", "div { scrollbar-width: thin; }");
        assert_eq!(only(&styles, "p").scrollbar_width, ScrollbarWidth::Thin);
    }

    #[test]
    fn webkit_scrollbar_class_qualified_rule_applies_to_its_scroll_container() {
        reset_scrollbar_pseudo_cascades();
        let styles = cascade_all(
            "<div class='sb'><p>a</p></div>",
            ".sb::-webkit-scrollbar { width: 0; } .sb { overflow: scroll; }",
        );
        assert_eq!(
            only(&styles, "div").scrollbar_width,
            ScrollbarWidth::None,
            "`.sb::-webkit-scrollbar {{ width: 0 }}` must still suppress the bar"
        );
        assert_eq!(scrollbar_pseudo_cascades(), 3, "html, body and .sb");
    }

    #[test]
    fn pseudo_base_not_built_when_no_rule_matches() {
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("p::after { color: red; }");
        let vp = Size::new(800.0, 600.0);
        let div = doc.get(doc.body().unwrap()).children[0];
        let parent = ComputedStyle::root();

        reset_pseudo_base_builds();
        assert!(
            compute_pseudo_element_style(&doc, div, "before", &sheet, &parent, vp, false).is_none()
        );
        assert_eq!(
            pseudo_base_builds(),
            0,
            "no `div::before` rule matches — the 302-field starting style must \
             not be built just to be thrown away"
        );

        let p = doc.get(div).children[0];
        reset_pseudo_base_builds();
        assert!(
            compute_pseudo_element_style(&doc, p, "after", &sheet, &parent, vp, false).is_none(),
            "`content` is absent, so ::after still generates nothing"
        );
        assert_eq!(pseudo_base_builds(), 1, "a matching rule does build one");
    }

    #[test]
    fn sheet_quote_content_flag_tracks_declarations() {
        let vp = Size::new(800.0, 600.0);
        let plain = lumen_css_parser::parse("p::before { content: 'x'; }");
        assert!(!sheet_has_quote_content(&plain, vp, false));

        let quoted = lumen_css_parser::parse("p::before { content: open-quote; }");
        assert!(sheet_has_quote_content(&quoted, vp, false));

        // Over-approximating is the point: a `var()` can carry `open-quote` in
        // from a custom property, so any mention anywhere arms the probe.
        let indirect = lumen_css_parser::parse(":root { --q: open-quote; }");
        assert!(sheet_has_quote_content(&indirect, vp, false));
    }

    /// BUG-341 S23 — the predicate that lets callers skip a whole traversal.
    ///
    /// It must be exactly as wide as `matches_complex_for_pseudo`, which looks
    /// only at the **subject** compound: too narrow and a pseudo-element silently
    /// loses its styling (visible corruption, not a slow frame), too wide and
    /// the fast path is merely missed.
    #[test]
    fn sheet_pseudo_subjects_cover_every_container_and_only_the_subject() {
        let vp = Size::new(800.0, 600.0);

        let plain = lumen_css_parser::parse("p { color: red; } p::before { content: 'x'; }");
        assert!(!sheet_targets_pseudo(&plain, vp, false, "first-line"));
        assert!(sheet_targets_pseudo(&plain, vp, false, "before"));

        // A rule's container decides *whether* it applies, never whether the
        // sheet mentions the pseudo-element at all — same reasoning as
        // `all_rules`. Each container must arm the predicate on its own.
        for css in [
            "p::first-line { color: red; }",
            "@media (min-width: 1px) { p::first-line { color: red; } }",
            "@supports (color: red) { p::first-line { color: red; } }",
            "@layer base { p::first-line { color: red; } }",
        ] {
            let sheet = lumen_css_parser::parse(css);
            assert!(
                sheet_targets_pseudo(&sheet, vp, false, "first-line"),
                "`{css}` must arm the ::first-line predicate"
            );
        }

        // Not the subject: `::first-line` cannot match through a descendant
        // combinator, and `matches_complex_for_pseudo` never looks there either.
        let non_subject = lumen_css_parser::parse("p::first-line span { color: red; }");
        assert!(!sheet_targets_pseudo(&non_subject, vp, false, "first-line"));

        // Case-insensitive both ways, and unknown (vendor) names carry through
        // verbatim — that is how `::-webkit-scrollbar*` reaches the predicate.
        let mixed = lumen_css_parser::parse("p::FIRST-LINE { color: red; } p::-WEBKIT-SCROLLBAR { width: 0; }");
        assert!(sheet_targets_pseudo(&mixed, vp, false, "first-line"));
        assert!(sheet_targets_pseudo(&mixed, vp, false, "-webkit-scrollbar"));
    }

    /// BUG-341 S23 — `::marker` is the exception the short-circuit must honour.
    ///
    /// CSS Lists L3 §2.1 synthesizes a marker style out of `list-style-type`
    /// with no rule at all, so "the sheet does not mention `::marker`" says
    /// nothing about whether `compute_pseudo_element_style` should return one.
    #[test]
    fn marker_pseudo_style_survives_a_sheet_that_never_mentions_it() {
        let doc = lumen_html_parser::parse("<ul><li>x</li></ul>");
        let sheet = lumen_css_parser::parse("li { color: red; }");
        let vp = Size::new(800.0, 600.0);
        let li = doc
            .get(doc.get(doc.body().unwrap()).children[0])
            .children
            .iter()
            .copied()
            .find(|&n| matches!(doc.get(n).data, NodeData::Element { .. }))
            .expect("<li>");
        assert!(!sheet_targets_pseudo(&sheet, vp, false, "marker"));
        assert!(
            compute_pseudo_element_style(&doc, li, "marker", &sheet, &ComputedStyle::root(), vp, false)
                .is_some(),
            "::marker is generated without a rule and must not be short-circuited"
        );
    }

    // ── BUG-341 S21: the cascade index survives the pass that built it ───────
    //
    // Gates by counter, not by output: an index rebuilt from scratch on every
    // node of every pass produces byte-identical styles. Each one asserts both
    // arms — that the index is reused when it may be, *and* that it is rebuilt
    // when it must be — because "never reuse" and "never rebuild" are both
    // trivially passable one-liners, and the second one is silently wrong
    // styles rather than a slow frame.

    #[test]
    fn bug341_s21_repeated_cascades_over_one_sheet_build_the_index_once() {
        let doc = lumen_html_parser::parse("<div class='a'><p id='x'>t</p></div>");
        let sheet = lumen_css_parser::parse(".a { color: red } #x { color: blue }");
        let vp = Size::new(800.0, 600.0);
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        clear_rule_idx_cache();
        let _ = take_cascade_index_stats();
        let first = compute_style(&doc, div, &sheet, &root, vp, false);
        assert_eq!(
            take_cascade_index_stats().builds,
            1,
            "a cold thread must index the sheet once"
        );

        for _ in 0..20 {
            let s = compute_style(&doc, div, &sheet, &root, vp, false);
            assert_eq!(s.color, first.color, "the reused index must match the same rules");
        }
        assert_eq!(
            take_cascade_index_stats().builds,
            0,
            "the index is keyed by `Stylesheet::revision`, which did not change — \
             rebuilding it again is pure waste that no differential test can see"
        );
    }

    #[test]
    fn bug341_s21_a_mutated_sheet_is_reindexed_and_its_new_rules_apply() {
        let doc = lumen_html_parser::parse("<div class='a'>t</div>");
        let mut sheet = lumen_css_parser::parse(".a { color: red }");
        let vp = Size::new(800.0, 600.0);
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        clear_rule_idx_cache();
        let _ = take_cascade_index_stats();
        let before = compute_style(&doc, div, &sheet, &root, vp, false);
        assert_eq!(take_cascade_index_stats().builds, 1);

        // A rule added after the index was built. Note the added rule keeps the
        // sheet's `rules.len()` growing, which the old address-plus-length key
        // also caught — the point of the assertion is the *new* key: the
        // revision moved, so the stale index cannot be served.
        sheet.merge_from(lumen_css_parser::parse(".a { color: green }"));
        let after = compute_style(&doc, div, &sheet, &root, vp, false);
        assert_eq!(
            take_cascade_index_stats().builds,
            1,
            "a sheet whose rules changed must be re-indexed"
        );
        assert_ne!(
            before.color, after.color,
            "the rule added after the index was built must actually apply — this is \
             the arm that fails if the cache is keyed by something that does not \
             move when the sheet's content does"
        );
    }

    #[test]
    fn bug341_s21_a_resize_reindexes_because_it_changes_which_media_blocks_apply() {
        let doc = lumen_html_parser::parse("<div class='a'>t</div>");
        let sheet = lumen_css_parser::parse(
            ".a { color: red } @media (min-width: 900px) { .a { color: green } }",
        );
        let narrow = Size::new(800.0, 600.0);
        let wide = Size::new(1000.0, 600.0);
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        clear_rule_idx_cache();
        let _ = take_cascade_index_stats();
        let a = compute_style(&doc, div, &sheet, &root, narrow, false);
        let b = compute_style(&doc, div, &sheet, &root, wide, false);
        assert_eq!(
            take_cascade_index_stats().builds,
            2,
            "`active_media` is baked into the index, so the viewport is part of its key"
        );
        assert_ne!(a.color, b.color, "the `@media` block must take effect at 1000px");
    }

    #[test]
    fn bug341_s21_two_documents_on_one_thread_do_not_evict_each_other() {
        // The shape a real frame has: the browser's own chrome and the page it
        // shows are laid out on the same thread, one after the other. With a
        // single cache slot each pass would evict the other's index and rebuild
        // it — the per-pass rebuild this slice removed, reintroduced by the
        // cache's size rather than by its key.
        let doc = lumen_html_parser::parse("<div class='a'>t</div>");
        let chrome = lumen_css_parser::parse(".a { color: red }");
        let page = lumen_css_parser::parse(".a { color: blue }");
        let vp = Size::new(800.0, 600.0);
        let root = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];

        clear_rule_idx_cache();
        let _ = take_cascade_index_stats();
        for _ in 0..10 {
            let c = compute_style(&doc, div, &chrome, &root, vp, false);
            let p = compute_style(&doc, div, &page, &root, vp, false);
            assert_ne!(c.color, p.color, "each sheet must be matched with its own index");
        }
        assert_eq!(
            take_cascade_index_stats().builds,
            2,
            "one index per sheet, not one per alternation"
        );

        // The eviction arm: a third sheet does push the least-recently-used one
        // out, so the cache stays bounded rather than growing per navigation.
        let third = lumen_css_parser::parse(".a { color: yellow }");
        let _ = compute_style(&doc, div, &third, &root, vp, false);
        let _ = take_cascade_index_stats();
        let _ = compute_style(&doc, div, &chrome, &root, vp, false);
        assert_eq!(
            take_cascade_index_stats().builds,
            1,
            "the cache holds {CASCADE_INDEX_SLOTS} sheets, so the oldest is re-indexed"
        );
    }

    #[test]
    fn custom_props_shared_with_parent_when_child_declares_none() {
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { --gap: 8px; }");
        let vp = Size::new(800.0, 600.0);
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, vp, false);
        let p_style = compute_style(&doc, p, &sheet, &div_style, vp, false);

        assert_eq!(p_style.custom_props.get("--gap").map(String::as_str), Some("8px"));
        assert!(
            div_style.custom_props.ptr_eq(&p_style.custom_props),
            "a child that declares no custom property must share its parent's map, \
             not copy it — see CustomProps"
        );
    }

    #[test]
    fn custom_props_copy_on_write_when_child_declares_one() {
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet = lumen_css_parser::parse("div { --gap: 8px; } p { --pad: 2px; }");
        let vp = Size::new(800.0, 600.0);
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, vp, false);
        let p_style = compute_style(&doc, p, &sheet, &div_style, vp, false);

        assert!(
            !div_style.custom_props.ptr_eq(&p_style.custom_props),
            "a child that declares its own property must have forked the map"
        );
        // The fork must not have been visible upwards.
        assert_eq!(p_style.custom_props.get("--gap").map(String::as_str), Some("8px"));
        assert_eq!(p_style.custom_props.get("--pad").map(String::as_str), Some("2px"));
        assert!(div_style.custom_props.get("--pad").is_none());
    }

    #[test]
    fn custom_props_empty_map_is_a_shared_singleton() {
        // Documents that declare no custom property at all must not allocate
        // one map per node — every empty `CustomProps` is the same allocation,
        // so `ComputedStyle`'s own `PartialEq` short-circuits on it too.
        let a = ComputedStyle::root();
        let b = ComputedStyle::root();
        assert!(a.custom_props.is_empty());
        assert!(a.custom_props.ptr_eq(&b.custom_props));
    }

    #[test]
    fn custom_props_eq_compares_contents_when_not_shared() {
        // The pointer check is a fast path, not the semantics: two maps built
        // independently must still compare equal by content.
        let a: CustomProps = [("--x".to_string(), "1".to_string())].into_iter().collect();
        let b: CustomProps = [("--x".to_string(), "1".to_string())].into_iter().collect();
        assert!(!a.ptr_eq(&b));
        assert_eq!(a, b);
        let c: CustomProps = [("--x".to_string(), "2".to_string())].into_iter().collect();
        assert_ne!(a, c);
    }

    #[test]
    fn custom_prop_stored_in_computed_style() {
        let s = style_for("--main-color: red");
        assert_eq!(
            s.custom_props.get("--main-color").map(String::as_str),
            Some("red")
        );
    }

    #[test]
    fn custom_prop_does_not_match_known_property() {
        // `--display: block` НЕ должно повлиять на свойство display.
        // Должно только лечь в custom_props.
        let s = style_for("--display: block");
        assert_eq!(s.display, Display::Block); // default для <p>
        assert_eq!(s.custom_props.get("--display").map(String::as_str), Some("block"));
    }

    #[test]
    fn var_substitutes_simple_value() {
        let s = style_for("--c: red; color: var(--c)");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_substitutes_length_value() {
        let s = style_for("--w: 50px; width: var(--w)");
        assert_eq!(s.width, Some(Length::Px(50.0)));
    }

    #[test]
    fn var_uses_fallback_when_name_unknown() {
        // --c не задан — берём fallback (blue).
        let s = style_for("color: var(--unknown, blue)");
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn var_without_fallback_and_unknown_is_dropped() {
        // var() не разрешается и нет fallback → декларация игнорится,
        // color остаётся inherited (root() = black).
        let s = style_for("color: var(--unknown)");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn var_resolved_value_overrides_default() {
        // --c определён, fallback есть, но не используется (имя найдено).
        let s = style_for("--c: red; color: var(--c, blue)");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_cascade_later_wins() {
        // Последняя декларация --x с той же specificity побеждает.
        let s = style_for("--x: red; --x: blue; color: var(--x)");
        assert_eq!(s.color, Color { r: 0, g: 0, b: 255, a: 255 });
    }

    #[test]
    fn var_resolved_after_main_pass_regardless_of_source_order() {
        // --c объявлен ПОСЛЕ color: var(--c) — всё равно подставляется,
        // потому что custom-pass идёт до main-pass.
        let s = style_for("color: var(--c); --c: red");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_nested_substitution() {
        // var() resolves to another var() — должен раскрываться рекурсивно.
        let s = style_for("--a: var(--b); --b: red; color: var(--a)");
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_cycle_dropped_safely() {
        // --a -> --b -> --a — рекурсия превышает лимит → declaration ignored
        // → color остаётся default (black).
        let s = style_for("--a: var(--b); --b: var(--a); color: var(--a)");
        assert_eq!(s.color, Color::BLACK);
    }

    #[test]
    fn var_inherits_from_parent() {
        // Custom properties inherit (CSS Variables L1 §2). Объявленное на
        // <div> --main должно быть видно у потомка <p>.
        let doc = lumen_html_parser::parse("<div><p>x</p></div>");
        let sheet =
            lumen_css_parser::parse("div { --main: green; } p { color: var(--main); }");
        let root_style = ComputedStyle::root();
        let div = doc.get(doc.body().unwrap()).children[0];
        let p = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root_style, Size::new(800.0, 600.0), false);
        let p_style = compute_style(&doc, p, &sheet, &div_style, Size::new(800.0, 600.0), false);
        // Inherited custom prop виден у потомка.
        assert_eq!(p_style.custom_props.get("--main").map(String::as_str), Some("green"));
        assert_eq!(p_style.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn var_fallback_with_inner_comma_and_parens() {
        // Fallback содержит rgba(...) с запятыми — не должен порваться по
        // первой `,`. Top-level запятая отделяет имя от fallback, остальные —
        // часть fallback.
        let s = style_for("color: var(--c, rgba(255, 0, 0, 0.5))");
        let c = s.color;
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 0);
        assert_eq!(c.b, 0);
        assert!((c.a as i32 - 128).abs() <= 1);
    }

    #[test]
    fn var_within_string_literal_not_expanded() {
        // `"var(--x)"` внутри строкового литерала — это литерал, не
        // substitution. Свойство `content` мы не applay-им в Phase 0, поэтому
        // проверка идёт от обратного: find_var_open видит `var(` ВНЕ строки.
        // Берём color: чтобы content-like ситуация не помешала, проверяем
        // напрямую expand_vars.
        let mut custom = HashMap::new();
        custom.insert("--x".to_string(), "red".to_string());
        // Только литерал — никакого реального var() — должен остаться как есть.
        assert_eq!(
            expand_vars("\"var(--x)\"", &custom, 0).as_deref(),
            Some("\"var(--x)\"")
        );
    }

    #[test]
    fn var_specificity_more_important() {
        // !important на --x перебивает обычный --x с большей specificity?
        // Нет — !important побеждает (CSS Cascade L4 §8.1).
        let doc = lumen_html_parser::parse("<p class=\"a\">x</p>");
        let sheet = lumen_css_parser::parse(
            "p { --c: red !important; } .a { --c: blue; } p { color: var(--c); }",
        );
        let root_style = ComputedStyle::root();
        let p = doc.get(doc.body().unwrap()).children[0];
        let s = compute_style(&doc, p, &sheet, &root_style, Size::new(800.0, 600.0), false);
        assert_eq!(s.color, Color { r: 255, g: 0, b: 0, a: 255 });
    }

    #[test]
    fn var_multiple_in_one_value_via_border_shorthand() {
        // border shorthand принимает `<width> <style> <color>` — три токена.
        // Все три могут прийти из var(). Проверяем, что expand_vars
        // корректно разворачивает несколько var() в одной строке.
        let s = style_for("--w: 2px; --s: solid; --c: red; border: var(--w) var(--s) var(--c)");
        assert!((s.border_top_width - 2.0).abs() < 0.01);
        assert_eq!(s.border_top_style, BorderStyle::Solid);
        assert_eq!(s.border_top_color, CssColor::Rgba(Color { r: 255, g: 0, b: 0, a: 255 }));
    }

    #[test]
    fn expand_vars_pure_passthrough() {
        // Нет var() — должен вернуть точно такую же строку.
        let custom = HashMap::new();
        assert_eq!(expand_vars("10px solid red", &custom, 0).as_deref(), Some("10px solid red"));
    }

    #[test]
    fn expand_vars_unclosed_paren_is_none() {
        // Сломанный синтаксис — declaration treated as invalid.
        let mut custom = HashMap::new();
        custom.insert("--x".to_string(), "red".to_string());
        assert_eq!(expand_vars("color: var(--x", &custom, 0), None);
    }

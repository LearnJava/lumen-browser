//! Тесты `style.rs`: `attr()` с типизированной подстановкой, `@function`/`@mixin`/`@apply`,
//! `scrollbar-gutter`, `overflow-anchor`, `scroll-marker-group`.
//!
//! Хвост `values.rs`, вынесенный батчем SPLIT-LT1 без правок тел.

use super::*;

    // === CSS Values L4 §7.7 attr() typed substitution ===

    fn make_doc_with_div(html: &str) -> (lumen_dom::Document, lumen_dom::NodeId) {
        let doc = lumen_html_parser::parse(html);
        let body = doc.body().expect("body");
        let node = doc.get(body).children[0];
        (doc, node)
    }

    #[test]
    fn attr_typed_width_px() {
        // attr(data-w px) with data-w="200" should set width to 200px.
        let (doc, node) = make_doc_with_div(r#"<div data-w="200"></div>"#);
        let sheet = lumen_css_parser::parse("div { width: attr(data-w px); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        assert_eq!(style.width, Some(Length::Px(200.0)), "width should be 200px via attr(data-w px)");
    }

    #[test]
    fn attr_typed_fallback_when_absent() {
        // attr(data-missing px, 50px) — attribute absent, fallback 50px used.
        let (doc, node) = make_doc_with_div("<div></div>");
        let sheet = lumen_css_parser::parse("div { width: attr(data-missing px, 50px); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        assert_eq!(style.width, Some(Length::Px(50.0)), "fallback 50px should apply when attr absent");
    }

    #[test]
    fn attr_typed_absent_no_fallback_skipped() {
        // attr(data-missing px) with no fallback — declaration invalid, width stays None.
        let (doc, node) = make_doc_with_div("<div></div>");
        let sheet = lumen_css_parser::parse("div { width: attr(data-missing px); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        assert_eq!(style.width, None, "absent attr without fallback should leave width at None");
    }

    #[test]
    fn attr_typed_color() {
        // attr(data-bg color) — attribute value used as CSS color for background-color.
        let (doc, node) = make_doc_with_div(r#"<div data-bg="red"></div>"#);
        let sheet = lumen_css_parser::parse("div { background-color: attr(data-bg color); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        // red = rgb(255, 0, 0)
        let bg = style.background_color.expect("background-color should be set via attr(data-bg color)");
        let CssColor::Rgba(c) = bg else { panic!("expected Rgba, got {:?}", bg) };
        assert_eq!(c.r, 255, "red component");
        assert_eq!(c.g, 0,   "green component");
        assert_eq!(c.b, 0,   "blue component");
    }

    #[test]
    fn bug_1010_attr_in_own_custom_property_value_resolves() {
        // BUG-1010: `attr()` inside a custom property's OWN declared value
        // must resolve there too, not just when a typed property references
        // the custom property via `var()`.
        let (doc, node) = make_doc_with_div(r#"<div data-x="200"></div>"#);
        let sheet = lumen_css_parser::parse("div { --x: attr(data-x px); }");
        let parent = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 1024.0, height: 768.0 };
        let style = compute_style(&doc, node, &sheet, &parent, vp, false);
        assert_eq!(style.custom_props.get("--x").map(String::as_str), Some("200px"));
    }

    #[test]
    fn css_function_direct_call_resolves() {
        // CSS Functions and Mixins L1 — a direct call in a property value
        // (`width: --double(10px);`) should bind the positional argument and
        // resolve `result:` via calc().
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --double(--x) { result: calc(var(--x) * 2); } \
             .box { width: --double(10px); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(20.0));
    }

    #[test]
    fn css_function_default_parameter_used_when_arg_omitted() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --pad(--n: 5px) { result: var(--n); } \
             .box { margin-left: --pad(); }",
            &[0],
        );
        assert_eq!(s.margin_left, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn css_function_call_through_custom_property_chain_resolves() {
        // A call reached indirectly through `var()` — the author computed a
        // custom property from a function call, then referenced it — must
        // resolve the same as a direct call.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --double(--x) { result: calc(var(--x) * 2); } \
             .box { --gap: --double(10px); width: var(--gap); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(20.0));
        // BUG-1010: the custom property's OWN computed value (what
        // `getComputedStyle().getPropertyValue('--gap')` reads) must have its
        // `--fn()` call substituted — not remain the raw, wholly unexpanded
        // `--double(10px)` source text. The substitution stops at the same
        // point a typed property's own pipeline would (var()/function calls
        // expanded, `calc()` arithmetic left for the property-specific parser
        // to fold at point of use), so `calc(10px * 2)` is the correct result
        // here, not a further-reduced `20px`.
        assert_eq!(s.custom_props.get("--gap").map(String::as_str), Some("calc(10px * 2)"));
    }

    #[test]
    fn css_function_local_declaration_feeds_result() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --clamped(--min, --val, --max) { \
                 --c: clamp(var(--min), var(--val), var(--max)); \
                 result: var(--c); \
             } \
             .box { width: --clamped(10px, 5px, 50px); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(10.0));
    }

    #[test]
    fn css_function_missing_required_argument_invalidates_declaration() {
        // No default for `--x` and no argument supplied → invalid at
        // computed-value time, same treatment as an unresolvable `var()`.
        // `width` must stay unset (initial/inherited), not panic or use 0.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --double(--x) { result: calc(var(--x) * 2); } \
             .box { width: --double(); }",
            &[0],
        );
        assert_eq!(s.width, None);
    }

    #[test]
    fn css_function_named_argument_syntax_invalidates_call() {
        // csswg-drafts#11749: an argument starting `--ident:` is reserved for
        // named arguments → invalid at computed-value time, not bound
        // positionally as the text "--x: 10px".
        for call in ["--id(--x: 10px)", "--id( --x : 10px)", "--id(1px, --x:)"] {
            let s = cascade_at(
                "<div class=\"box\"></div>",
                &format!(
                    "@function --id(--x, --y: 0px) {{ result: var(--x); }} .box {{ width: {call}; }}"
                ),
                &[0],
            );
            assert_eq!(s.width, None, "{call}");
        }
    }

    #[test]
    fn css_function_dashed_ident_not_followed_by_colon_is_positional() {
        // `--id(--v)` / `--id(50px --v:)`-style args are NOT named syntax;
        // a `{...}`-wrapped `--x: ...` is an ordinary argument.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --id(--x) { result: var(--x); } \
             .box { --v: 7px; width: --id(var(--v)); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(7.0));
    }

    #[test]
    fn css_function_unknown_call_invalidates_declaration() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            ".box { width: --not-defined(10px); }",
            &[0],
        );
        assert_eq!(s.width, None);
    }

    #[test]
    fn css_function_self_recursion_invalidates_instead_of_hanging() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --loop(--x) { result: --loop(var(--x)); } \
             .box { width: --loop(10px); }",
            &[0],
        );
        assert_eq!(s.width, None);
    }

    // ── BUG-519: unsupported nested at-rule inside `@function`/`@mixin` body
    // must not corrupt parsing of the declarations after it ─────────────────

    #[test]
    fn css_function_body_recovers_after_unsupported_nested_at_rule() {
        // `@supports`/`@media`/`@container` conditional group rules nested
        // inside a `@function` body (CSS Functions and Mixins L1's
        // "conditional rules", not implemented) have no dedicated grammar in
        // `parse_declaration_block` — the parser must skip the whole nested
        // `{ ... }` as one unit and keep parsing `result:` afterward, rather
        // than losing track of brace depth and dropping it.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@function --f() { \
                 @supports (width: 100px) { --unused: 1; } \
                 result: 5px; \
             } \
             .box { width: --f(); }",
            &[0],
        );
        let w = s.width.expect("result: after the nested at-rule must still parse");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(5.0));
    }

    #[test]
    fn css_value_with_semicolon_inside_parens_is_not_truncated() {
        // CSS Syntax L3 §5.4.4: a declaration's value ends at a top-level
        // `;`/`}` only — a matched `(...)` is one component value regardless
        // of what it contains. CSS Values L5's `if()` uses `;` *inside* its
        // own parens to separate branches, which must not end the value
        // early (BUG-519). `--x` is deliberately unresolvable so the whole
        // declaration is invalid either way; what this test actually checks
        // is that `width` (which comes after) still parses at all — before
        // the fix, the stray `else: FAIL;)` tail became a bogus second
        // declaration that swallowed `width`'s own text.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            ".box { --x: if(style(--y: 1px): PASS; else: FAIL;); width: 7px; }",
            &[0],
        );
        let w = s.width.expect("width after an if()-valued declaration must still parse");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(7.0));
    }

    // ── `@mixin`/`@apply`/`@contents` (BUG-518, cascade-time expansion) ─────

    #[test]
    fn css_mixin_basic_apply_result_declaration_resolves() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --pad(--n) { @result { margin-left: var(--n); } } \
             .box { @apply --pad(5px); }",
            &[0],
        );
        assert_eq!(s.margin_left, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn css_mixin_positional_argument_resolves_through_calc() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --double(--x) { @result { width: calc(var(--x) * 2); } } \
             .box { @apply --double(10px); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(20.0));
    }

    #[test]
    fn css_mixin_default_parameter_used_when_call_has_no_parens() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --pad(--n: 5px) { @result { margin-left: var(--n); } } \
             .box { @apply --pad; }",
            &[0],
        );
        assert_eq!(s.margin_left, LengthOrAuto::Length(Length::Px(5.0)));
    }

    #[test]
    fn css_mixin_local_declaration_feeds_result() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --clamped(--min, --val, --max) { \
                 --c: clamp(var(--min), var(--val), var(--max)); \
                 @result { width: var(--c); } \
             } \
             .box { @apply --clamped(10px, 5px, 50px); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(10.0));
    }

    #[test]
    fn css_mixin_locals_do_not_see_caller_scope() {
        // Mirrors `mixin-locals.html` "Parameters do not resolve against
        // locals": a mixin parameter's DEFAULT resolves against the call
        // site's scope, not a same-named local declared inside the mixin.
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --m(--w: var(--outer-w)) { \
                 --outer-w: 999px; \
                 @result { width: var(--w); } \
             } \
             .box { --outer-w: 7px; @apply --m; }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(7.0));
    }

    #[test]
    fn css_mixin_unknown_name_is_a_noop() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            ".box { @apply --not-defined; }",
            &[0],
        );
        assert_eq!(s.width, None);
    }

    #[test]
    fn css_mixin_without_result_is_a_noop() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --empty() {} \
             .box { width: 5px; @apply --empty; }",
            &[0],
        );
        assert_eq!(s.width, Some(Length::Px(5.0)));
    }

    #[test]
    fn css_mixin_nested_apply_inside_result() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --inner(--v) { @result { width: var(--v); } } \
             @mixin --outer(--v) { @result { @apply --inner(var(--v)); } } \
             .box { @apply --outer(30px); }",
            &[0],
        );
        let w = s.width.expect("width should be set");
        assert_eq!(w.resolve(16.0, None, Size::new(800.0, 600.0)), Some(30.0));
    }

    #[test]
    fn css_mixin_redefinition_last_registration_wins() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --pick() { @result { width: 1px; } } \
             @mixin --pick() { @result { width: 2px; } } \
             .box { @apply --pick; }",
            &[0],
        );
        assert_eq!(s.width, Some(Length::Px(2.0)));
    }

    #[test]
    fn css_mixin_contents_uses_apply_block_when_given() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --wrap() { @result { @contents; } } \
             .box { @apply --wrap { width: 40px; } }",
            &[0],
        );
        assert_eq!(s.width, Some(Length::Px(40.0)));
    }

    #[test]
    fn css_mixin_contents_uses_own_fallback_when_apply_has_no_block() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --wrap() { @result { @contents { width: 50px; } } } \
             .box { @apply --wrap; }",
            &[0],
        );
        assert_eq!(s.width, Some(Length::Px(50.0)));
    }

    #[test]
    fn css_mixin_brace_wrapped_argument_is_stripped() {
        let s = cascade_at(
            "<div class=\"box\"></div>",
            "@mixin --m(--v) { @result { width: var(--v); } } \
             .box { @apply --m({60px}); }",
            &[0],
        );
        assert_eq!(s.width, Some(Length::Px(60.0)));
    }

    #[test]
    fn css_mixin_visible_across_concatenated_style_elements() {
        // Transcribes `mixin-cross-stylesheet.html`: a `<style>` calling
        // `@apply --m1` and a *second*, later `<style>` defining `@mixin --m1`.
        // The shell (`build_page_cascade`) concatenates every inline `<style>`'s
        // text, in document order, into one string before the single
        // `lumen_css_parser::parse` call — so by the time this reaches the
        // parser there are no separate "stylesheets" left to scope by, only
        // one flat source with a forward reference, already supported since
        // срез 1 (cascade-time lookup, not parse-time). No code change needed
        // for this case — this test exists to pin the behaviour down.
        let css = "div { color: red; @apply --m1; } \
                   @mixin --m1() { @result { color: green; } }";
        let s = cascade_at(
            "<div><div class=\"cls\" id=\"target\">x</div></div>",
            css,
            &[0, 0],
        );
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    #[test]
    fn css_mixin_visible_across_at_import() {
        // Transcribes `mixin-from-import.html`: `@import` pulls in a sheet whose
        // only content is `@mixin --m1`, then the importing sheet's own rule
        // applies it. The shell resolves `@import` at the text level
        // (`inline_css_imports`), splicing the imported file's raw CSS text
        // *before* the importing sheet's own text — matching that here.
        let imported = "@mixin --m1() { @result { color: green; } }";
        let importer = "div { color: red; @apply --m1; }";
        let css = format!("{imported} {importer}");
        let s = cascade_at("<div id=\"target\"></div>", &css, &[0]);
        assert_eq!(s.color, Color { r: 0, g: 128, b: 0, a: 255 });
    }

    // ── `scrollbar-gutter` (BUG-505): `stable && both-edges?`, order-independent ──

    #[test]
    fn scrollbar_gutter_parses_all_three_forms() {
        assert_eq!(ScrollbarGutter::parse("auto"), Some(ScrollbarGutter::Auto));
        assert_eq!(ScrollbarGutter::parse("stable"), Some(ScrollbarGutter::Stable));
        assert_eq!(
            ScrollbarGutter::parse("stable both-edges"),
            Some(ScrollbarGutter::StableBothEdges)
        );
    }

    #[test]
    fn scrollbar_gutter_accepts_both_edges_stable_reversed() {
        // CSS Overflow L4 §3.3's `&&` combinator doesn't fix token order —
        // confirmed by WPT `scrollbar-gutter-valid.html`'s
        // `"both-edges stable"` case, which must parse identically to
        // `"stable both-edges"`.
        assert_eq!(
            ScrollbarGutter::parse("both-edges stable"),
            Some(ScrollbarGutter::StableBothEdges)
        );
    }

    #[test]
    fn scrollbar_gutter_rejects_invalid_combinations() {
        assert_eq!(ScrollbarGutter::parse("both-edges"), None);
        assert_eq!(ScrollbarGutter::parse("stable both"), None);
        assert_eq!(ScrollbarGutter::parse("auto stable"), None);
        assert_eq!(ScrollbarGutter::parse(""), None);
    }

    // ── `overflow-anchor` (BUG-524 срез 1, CSS Scroll Anchoring 1) ──

    #[test]
    fn overflow_anchor_parses_auto_and_none() {
        assert_eq!(OverflowAnchor::parse("auto"), Some(OverflowAnchor::Auto));
        assert_eq!(OverflowAnchor::parse("none"), Some(OverflowAnchor::None));
        assert_eq!(OverflowAnchor::parse("AUTO"), Some(OverflowAnchor::Auto));
    }

    #[test]
    fn overflow_anchor_rejects_invalid_values() {
        assert_eq!(OverflowAnchor::parse("all"), None);
        assert_eq!(OverflowAnchor::parse("auto none"), None);
        assert_eq!(OverflowAnchor::parse(""), None);
    }

    #[test]
    fn overflow_anchor_default_is_auto() {
        assert_eq!(ComputedStyle::root().overflow_anchor, OverflowAnchor::Auto);
    }

    #[test]
    fn overflow_anchor_applies_through_cascade() {
        let s = cascade_at(
            "<div id=\"target\"></div>",
            "div { overflow-anchor: none; }",
            &[0],
        );
        assert_eq!(s.overflow_anchor, OverflowAnchor::None);
    }

    #[test]
    fn overflow_anchor_css_wide_keyword_inherit() {
        // Non-inherited property: `inherit` on the child still copies the
        // parent's specified value (per CSS-wide-keyword semantics, distinct
        // from ordinary property inheritance).
        let s = cascade_at(
            "<div><p></p></div>",
            "div { overflow-anchor: none; } p { overflow-anchor: inherit; }",
            &[0, 0],
        );
        assert_eq!(s.overflow_anchor, OverflowAnchor::None);
    }

    #[test]
    fn overflow_anchor_css_wide_keyword_initial() {
        let s = cascade_at(
            "<div><p></p></div>",
            "div { overflow-anchor: none; } p { overflow-anchor: initial; }",
            &[0, 0],
        );
        assert_eq!(s.overflow_anchor, OverflowAnchor::Auto);
    }

    // ── `scroll-marker-group`/`scroll-target-group` (BUG-505 срез 6, CSS
    // Overflow L5) ──

    #[test]
    fn scroll_marker_group_parses_none_and_bare_placement() {
        assert_eq!(ScrollMarkerGroup::parse("none"), Some(None));
        assert_eq!(
            ScrollMarkerGroup::parse("before"),
            Some(Some(ScrollMarkerGroup {
                placement: ScrollMarkerGroupPlacement::Before,
                mode: None,
            }))
        );
        assert_eq!(
            ScrollMarkerGroup::parse("after"),
            Some(Some(ScrollMarkerGroup {
                placement: ScrollMarkerGroupPlacement::After,
                mode: None,
            }))
        );
    }

    #[test]
    fn scroll_marker_group_parses_placement_plus_mode() {
        // Order-dependent — direction always comes first (tentative
        // tabs/links extension, github.com/w3c/csswg-drafts/issues/12122).
        assert_eq!(
            ScrollMarkerGroup::parse("before tabs"),
            Some(Some(ScrollMarkerGroup {
                placement: ScrollMarkerGroupPlacement::Before,
                mode: Some(ScrollMarkerGroupMode::Tabs),
            }))
        );
        assert_eq!(
            ScrollMarkerGroup::parse("after links"),
            Some(Some(ScrollMarkerGroup {
                placement: ScrollMarkerGroupPlacement::After,
                mode: Some(ScrollMarkerGroupMode::Links),
            }))
        );
    }

    #[test]
    fn scroll_marker_group_rejects_reversed_order_and_bad_tokens() {
        // WPT `scroll-markers-invalid{,.tentative}.html`'s own matrix.
        assert_eq!(ScrollMarkerGroup::parse("before before"), None);
        assert_eq!(ScrollMarkerGroup::parse("after before"), None);
        assert_eq!(ScrollMarkerGroup::parse("after tab"), None);
        assert_eq!(ScrollMarkerGroup::parse("after link"), None);
        assert_eq!(ScrollMarkerGroup::parse("links after"), None);
        assert_eq!(ScrollMarkerGroup::parse("tabs before"), None);
        assert_eq!(ScrollMarkerGroup::parse("10"), None);
        assert_eq!(ScrollMarkerGroup::parse("default"), None);
    }

    #[test]
    fn scroll_target_group_parses_none_and_auto() {
        assert_eq!(ScrollTargetGroup::parse("none"), Some(ScrollTargetGroup::None));
        assert_eq!(ScrollTargetGroup::parse("auto"), Some(ScrollTargetGroup::Auto));
        assert_eq!(ScrollTargetGroup::parse("10"), None);
        assert_eq!(ScrollTargetGroup::parse("all"), None);
    }

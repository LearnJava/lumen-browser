//! Тесты `style.rs`: селекторы Shadow DOM.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

// ─────────────────────────────────────────────────────────────────────────────
// Shadow DOM pseudo-class / pseudo-element tests (CSS Scoping L1 §6.1-6.2)
// ─────────────────────────────────────────────────────────────────────────────

    use super::*;
    use lumen_core::geom::Size;
    use lumen_dom::ShadowRootMode;
    use std::collections::HashMap;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    /// Build a minimal Document: `<div id="host">` as the shadow host with an
    /// attached open shadow root. Returned tuple: (doc, host_id).
    fn make_shadow_host() -> (lumen_dom::Document, NodeId) {
        let mut doc = lumen_html_parser::parse(r#"<div id="host"></div>"#);
        let body = doc.body().expect("body");
        let host = doc.get(body).children[0];
        doc.attach_shadow(host, ShadowRootMode::Open);
        (doc, host)
    }

    /// Build a Document with shadow host + one light-tree child `<span class="item">`.
    fn make_shadow_host_with_slotted() -> (lumen_dom::Document, NodeId, NodeId) {
        let mut doc = lumen_html_parser::parse(
            r#"<div id="host"><span class="item"></span></div>"#,
        );
        let body = doc.body().expect("body");
        let host = doc.get(body).children[0];
        let slotted = doc.get(host).children[0];
        doc.attach_shadow(host, ShadowRootMode::Open);
        (doc, host, slotted)
    }

    /// Install a single shadow-tree stylesheet for `host` (CSS Scoping L1 scope).
    /// Mirrors what `build_shadow_sheets` does for real `<template shadowrootmode>`
    /// markup; tests must clear afterwards to avoid leaking across `NodeId` reuse.
    fn install_shadow_sheet(host: NodeId, css: &str) {
        let mut map: HashMap<NodeId, Stylesheet> = HashMap::new();
        map.insert(host, lumen_css_parser::parse(css));
        set_shadow_sheets(map);
    }

    #[test]
    fn host_simple_matches_shadow_host() {
        // `:host { background-color: red; }` in the shadow tree applies to the host.
        let (doc, host) = make_shadow_host();
        install_shadow_sheet(host, ":host { background-color: red; }");
        let root = ComputedStyle::root();
        let s = compute_style(&doc, host, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        let bg = s.background_color.expect("background-color set").resolve(s.color);
        assert_eq!((bg.r, bg.g, bg.b), (255, 0, 0), ":host must apply to shadow host");
    }

    #[test]
    fn host_with_selector_matches_when_host_satisfies_inner() {
        // `:host(#host) { background-color: blue; }` — host has id="host", must match.
        let (doc, host) = make_shadow_host();
        install_shadow_sheet(host, ":host(#host) { background-color: blue; }");
        let root = ComputedStyle::root();
        let s = compute_style(&doc, host, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        let bg = s.background_color.expect("background-color set").resolve(s.color);
        assert_eq!((bg.r, bg.g, bg.b), (0, 0, 255), ":host(#host) must match host with id=host");
    }

    #[test]
    fn host_with_selector_does_not_match_when_inner_fails() {
        // `:host(.missing) { background-color: red; }` — host has no class "missing".
        let (doc, host) = make_shadow_host();
        install_shadow_sheet(host, ":host(.missing) { background-color: red; }");
        let root = ComputedStyle::root();
        let s = compute_style(&doc, host, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        assert!(s.background_color.is_none(), ":host(.missing) must NOT match when class absent");
    }

    #[test]
    fn host_rule_in_document_scope_is_noop() {
        // CSS Scoping L1 §6.1 — a `:host` rule in the page's document stylesheet
        // (NOT inside a shadow tree) must not match any host. This is the BUG-142
        // root cause: previously the document `:host` coloured every shadow host.
        let (doc, host) = make_shadow_host();
        let sheet = lumen_css_parser::parse(":host { background-color: red; }");
        clear_shadow_sheets();
        let root = ComputedStyle::root();
        let s = compute_style(&doc, host, &sheet, &root, VP, false);
        assert!(s.background_color.is_none(),
            "document-scope :host must be a no-op (only matches from within its shadow tree)");
    }

    #[test]
    fn slotted_applies_to_light_tree_child_of_shadow_host() {
        // `::slotted(.item) { color: green; }` in the host's shadow tree applies to
        // the slotted light-tree child.
        let (doc, host, slotted) = make_shadow_host_with_slotted();
        install_shadow_sheet(host, "::slotted(.item) { color: green; }");
        let root = ComputedStyle::root();
        let s = compute_style(&doc, slotted, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 128, 0),
            "::slotted(.item) must apply to light-tree child");
    }

    #[test]
    fn slotted_does_not_apply_to_non_slotted_element() {
        // Regular `<span class="item">` not inside a shadow host must not match `::slotted`.
        let doc = lumen_html_parser::parse(r#"<span class="item"></span>"#);
        let sheet = lumen_css_parser::parse("::slotted(.item) { color: green; }");
        clear_shadow_sheets();
        let body = doc.body().expect("body");
        let span = doc.get(body).children[0];
        let root = ComputedStyle::root();
        let s = compute_style(&doc, span, &sheet, &root, VP, false);
        // Default text color is black (0,0,0).
        assert_ne!((s.color.r, s.color.g, s.color.b), (0, 128, 0),
            "::slotted must not apply to non-slotted span");
    }

    #[test]
    fn slotted_rule_in_document_scope_is_noop() {
        // CSS Scoping L1 §6.2 — a `::slotted()` rule in the document stylesheet must
        // not match: it only has effect inside the host's shadow tree.
        let (doc, _host, slotted) = make_shadow_host_with_slotted();
        let sheet = lumen_css_parser::parse("::slotted(.item) { color: green; }");
        clear_shadow_sheets();
        let root = ComputedStyle::root();
        let s = compute_style(&doc, slotted, &sheet, &root, VP, false);
        assert_ne!((s.color.r, s.color.g, s.color.b), (0, 128, 0),
            "document-scope ::slotted must be a no-op");
    }

    // ─────────────────────────────────────────────────────────────────────
    // BUG-1009 — regular selectors inside a shadow tree's own stylesheet
    // matching its own interior descendants (CSS Scoping L1 §6 core case,
    // distinct from the `:host`/`::slotted` boundary cases above).
    // ─────────────────────────────────────────────────────────────────────

    /// Build a Document: `<div id="host">` shadow host whose open shadow root
    /// contains one interior child `<div id="e1">` — a real DOM descendant of
    /// the shadow root itself (BUG-1009's shape), not of the host. Returned
    /// tuple: (doc, host_id, e1_id).
    fn make_shadow_host_with_interior_child() -> (lumen_dom::Document, NodeId, NodeId) {
        let (mut doc, host) = make_shadow_host();
        let sr = doc.shadow_root_of(host).expect("shadow root");
        let e1 = doc.create_element(lumen_dom::QualName::html("div"));
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(e1).data {
            attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html("id"), value: "e1".into() });
        }
        doc.append_child(sr, e1);
        (doc, host, e1)
    }

    /// Build two nested shadow hosts: `outer-host`'s shadow root directly
    /// contains `inner-host`, which has its own nested shadow root containing
    /// `<div id="inner-e">`. Returned tuple: (doc, outer_host, inner_host, inner_e).
    fn make_nested_shadow_hosts() -> (lumen_dom::Document, NodeId, NodeId, NodeId) {
        let (mut doc, outer_host) = make_shadow_host();
        let sr_outer = doc.shadow_root_of(outer_host).expect("outer shadow root");
        let inner_host = doc.create_element(lumen_dom::QualName::html("div"));
        doc.append_child(sr_outer, inner_host);
        let sr_inner = doc.attach_shadow(inner_host, ShadowRootMode::Open);
        let inner_e = doc.create_element(lumen_dom::QualName::html("div"));
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(inner_e).data {
            attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html("id"), value: "inner-e".into() });
        }
        doc.append_child(sr_inner, inner_e);
        (doc, outer_host, inner_host, inner_e)
    }

    #[test]
    fn regular_selector_in_own_shadow_sheet_applies_to_its_interior_element() {
        // `#e1 { color: red; }` written inside a shadow tree's own `<style>`
        // must match `#e1` living inside that same shadow tree — previously
        // unmatched entirely because neither `:host` (case a) nor `::slotted()`
        // (case b) cover a plain interior selector.
        let (doc, host, e1) = make_shadow_host_with_interior_child();
        install_shadow_sheet(host, "#e1 { color: red; }");
        let root = ComputedStyle::root();
        let s = compute_style(&doc, e1, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        let c = s.color;
        assert_eq!((c.r, c.g, c.b), (255, 0, 0),
            "regular selector in shadow tree's own stylesheet must match its own interior element");
    }

    #[test]
    fn interior_regular_selector_does_not_apply_to_slotted_light_child() {
        // CSS Scoping L1 §6.2: a shadow tree's own plain selectors must not
        // reach slotted light-tree content — only `::slotted()` can. Guards
        // the new interior-match case against over-matching into the
        // `::slotted()` case's territory: a slotted child's real DOM parent
        // is the host itself, not the shadow root, so it must not resolve to
        // an enclosing shadow tree here.
        let (doc, host, slotted) = make_shadow_host_with_slotted();
        install_shadow_sheet(host, ".item { color: red; }");
        let root = ComputedStyle::root();
        let s = compute_style(&doc, slotted, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        assert_ne!((s.color.r, s.color.g, s.color.b), (255, 0, 0),
            "shadow tree's own plain selector must not match a slotted light-tree child");
    }

    #[test]
    fn nested_shadow_tree_uses_nearest_enclosing_scope_not_outer() {
        // An element inside a nested inner shadow tree must be styled by that
        // inner tree's own stylesheet, not by an enclosing outer shadow
        // tree's stylesheet of the same specificity/selector — tree scoping
        // resolves to the *nearest* shadow root, matching how `getRootNode()`
        // (without `composed: true`) would resolve for the same node.
        let (doc, outer_host, inner_host, inner_e) = make_nested_shadow_hosts();
        let mut map: HashMap<NodeId, Stylesheet> = HashMap::new();
        map.insert(outer_host, lumen_css_parser::parse("#inner-e { color: red; }"));
        map.insert(inner_host, lumen_css_parser::parse("#inner-e { color: blue; }"));
        set_shadow_sheets(map);
        let root = ComputedStyle::root();
        let s = compute_style(&doc, inner_e, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 0, 255),
            "nested shadow tree's own stylesheet (nearest scope) must win over an outer one");
    }

    #[test]
    fn slotted_inner_selector_filters_correctly() {
        // `::slotted(.other)` must NOT apply to `<span class="item">` (wrong class).
        let (doc, host, slotted) = make_shadow_host_with_slotted();
        install_shadow_sheet(host, "::slotted(.other) { color: red; }");
        let root = ComputedStyle::root();
        let s = compute_style(&doc, slotted, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        // Should retain default color, not red.
        assert_ne!((s.color.r, s.color.g, s.color.b), (255, 0, 0),
            "::slotted(.other) must not match span with class=item");
    }

    // ─────────────────────────────────────────────────────────────────────
    // BUG-518 mixin-shadow-dom.html follow-up — `@apply` written inside a
    // shadow tree's own `<style>` must resolve `@mixin` names against that
    // tree's own `mixin_rules` (SHADOW_SHEETS[host]), not only the
    // document-level `sheet` — the two are separate `Stylesheet`s (BUG-1009's
    // fix gave a shadow-interior element access to its OWN tree's plain
    // selectors; this closes the matching gap for that tree's OWN mixins).
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn apply_inside_shadow_sees_outside_mixin() {
        // mixin-shadow-dom.html #e1: "Style in shadow DOM should have access
        // to outside non-adopted mixins" — `@mixin` declared in the document's
        // own <style>, `@apply`'d from a rule inside the shadow tree.
        let (doc, host, e1) = make_shadow_host_with_interior_child();
        install_shadow_sheet(host, "#e1 { color: red; @apply --exists-only-outside-shadow; }");
        let sheet = lumen_css_parser::parse(
            "@mixin --exists-only-outside-shadow() { @result { color: green; } }",
        );
        let root = ComputedStyle::root();
        let s = compute_style(&doc, e1, &sheet, &root, VP, false);
        clear_shadow_sheets();
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 128, 0),
            "@apply inside a shadow tree must see a mixin declared outside it");
    }

    #[test]
    fn apply_inside_shadow_sees_own_shadow_mixin() {
        // mixin-shadow-dom.html #e2: "Style in shadow DOM should have access
        // to inside mixins" — both the `@apply`-bearing rule and the `@mixin`
        // itself live in the SAME shadow tree's own stylesheet (two <style>
        // elements inside one shadow root are concatenated into one
        // Stylesheet by `build_shadow_sheets`, mirrored here in one `parse`).
        let (doc, host, e1) = make_shadow_host_with_interior_child();
        install_shadow_sheet(
            host,
            "#e1 { color: red; @apply --m1; } @mixin --m1() { @result { color: green; } }",
        );
        let root = ComputedStyle::root();
        let s = compute_style(&doc, e1, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 128, 0),
            "@apply inside a shadow tree must see a mixin declared in that same tree");
    }

    #[test]
    fn apply_outside_shadow_does_not_see_inside_mixin() {
        // mixin-shadow-dom.html #e4: "Style outside shadow DOM should _not_
        // have access to inside mixins" — `@mixin` declared only inside a
        // shadow tree's own <style> must stay invisible to a light-DOM
        // `@apply` naming it; the unresolved `@apply` is a no-op, so the
        // element keeps its own directly-declared `color: green`.
        let (mut doc, host) = make_shadow_host();
        install_shadow_sheet(host, "@mixin --in-shadow() { @result { color: red; } }");
        let sheet = lumen_css_parser::parse("#e4 { color: green; @apply --in-shadow; }");
        let body = doc.body().expect("body");
        let e4 = doc.create_element(lumen_dom::QualName::html("div"));
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(e4).data {
            attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html("id"), value: "e4".into() });
        }
        doc.append_child(body, e4);
        let root = ComputedStyle::root();
        let s = compute_style(&doc, e4, &sheet, &root, VP, false);
        clear_shadow_sheets();
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 128, 0),
            "@apply outside a shadow tree must not see a mixin declared only inside it");
    }

    // ─────────────────────────────────────────────────────────────────────
    // BUG-519 — `@function` names are tree-scoped (CSS Scoping L1 §3.5):
    // a declaration from a shadow tree's `<style>` resolves `--fn()` in that
    // tree first, then in each enclosing tree, then in the document; a
    // function body resolves its own calls from the tree it was defined in.
    // Transcribes `css/css-mixins/functions/function-shadow.html`.
    // ─────────────────────────────────────────────────────────────────────

    fn custom(s: &ComputedStyle, name: &str) -> Option<String> {
        s.custom_props.get(name).map(|v| v.trim().to_string())
    }

    #[test]
    fn function_inside_shadow_shadows_document_function() {
        // "@function works inside shadow" — was 42px (document's --a()).
        let (doc, host, e1) = make_shadow_host_with_interior_child();
        install_shadow_sheet(host, "@function --a() { result: 10px; } #e1 { --actual: --a(); }");
        let sheet = lumen_css_parser::parse("@function --a() { result: 42px; }");
        let s = compute_style(&doc, e1, &sheet, &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(custom(&s, "--actual").as_deref(), Some("10px"));
    }

    #[test]
    fn document_function_visible_inside_shadow() {
        // "Looking up document-global function" — must keep working.
        let (doc, host, e1) = make_shadow_host_with_interior_child();
        install_shadow_sheet(host, "#e1 { --actual: --a(); }");
        let sheet = lumen_css_parser::parse("@function --a() { result: 42px; }");
        let s = compute_style(&doc, e1, &sheet, &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(custom(&s, "--actual").as_deref(), Some("42px"));
    }

    #[test]
    fn function_from_outer_shadow_visible_in_nested_shadow() {
        // "@function defined in outer shadow is visible" + "Combining
        // functions from various scopes".
        let (doc, outer_host, inner_host, inner_e) = make_nested_shadow_hosts();
        let mut map: HashMap<NodeId, Stylesheet> = HashMap::new();
        map.insert(outer_host, lumen_css_parser::parse("@function --b() { result: B; }"));
        map.insert(
            inner_host,
            lumen_css_parser::parse("@function --c() { result: C; } #inner-e { --actual: --a() --b() --c(); }"),
        );
        set_shadow_sheets(map);
        let sheet = lumen_css_parser::parse("@function --a() { result: 42px; }");
        let s = compute_style(&doc, inner_e, &sheet, &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(custom(&s, "--actual").as_deref(), Some("42px B C"));
    }

    #[test]
    fn outer_function_body_does_not_see_inner_same_named_function() {
        // "Outer functions can't see inner functions": --b() is defined in
        // the outer tree, so its `--c()` call must bind to the OUTER --c().
        let (doc, outer_host, inner_host, inner_e) = make_nested_shadow_hosts();
        let mut map: HashMap<NodeId, Stylesheet> = HashMap::new();
        map.insert(
            outer_host,
            lumen_css_parser::parse("@function --b() { result: --c(); } @function --c() { result: 20px; }"),
        );
        map.insert(
            inner_host,
            lumen_css_parser::parse("@function --c() { result: C; } #inner-e { --actual: --b() --c(); }"),
        );
        set_shadow_sheets(map);
        let s = compute_style(&doc, inner_e, &Stylesheet::default(), &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(custom(&s, "--actual").as_deref(), Some("20px C"));
    }

    #[test]
    fn same_named_function_in_different_scopes_is_not_a_cycle() {
        // "Function with same name in different scopes": inner --a() calls
        // outer --b(), which calls outer --a() — no cycle, result 24px.
        let (doc, outer_host, inner_host, inner_e) = make_nested_shadow_hosts();
        let mut map: HashMap<NodeId, Stylesheet> = HashMap::new();
        map.insert(
            outer_host,
            lumen_css_parser::parse("@function --a() { result: 24px; } @function --b() { result: --a(); }"),
        );
        map.insert(
            inner_host,
            lumen_css_parser::parse("@function --a() { result: --b(); } #inner-e { --actual: --a(); }"),
        );
        set_shadow_sheets(map);
        let s = compute_style(&doc, inner_e, &Stylesheet::default(), &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(custom(&s, "--actual").as_deref(), Some("24px"));
    }

    // ── BUG-1453: `:host` как предок в составном селекторе листа теневого дерева ──

    fn interior_color(css: &str) -> (u8, u8, u8) {
        let (doc, host, e1) = make_shadow_host_with_interior_child();
        install_shadow_sheet(host, css);
        let s = compute_style(&doc, e1, &Stylesheet::default(), &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        (s.color.r, s.color.g, s.color.b)
    }

    #[test]
    fn host_descendant_combinator_matches_interior_element() {
        assert_eq!(interior_color(":host div { color: green; }"), (0, 128, 0));
        assert_eq!(interior_color(":host #e1 { color: green; }"), (0, 128, 0));
    }

    #[test]
    fn host_child_combinator_matches_interior_element() {
        assert_eq!(interior_color(":host > div { color: green; }"), (0, 128, 0));
    }

    #[test]
    fn host_descendant_beats_lower_specificity_interior_rule() {
        // featureless-001: `:host div.red` (0,2,1+) выигрывает у `div.red`.
        assert_eq!(
            interior_color(":host #e1 { color: green; } div { color: red; }"),
            (0, 128, 0),
        );
    }

    #[test]
    fn host_function_with_descendant_checks_host_selector() {
        assert_eq!(interior_color(":host(#host) div { color: green; }"), (0, 128, 0));
        assert_eq!(interior_color(":host(.missing) div { color: green; }"), (0, 0, 0));
    }

    #[test]
    fn host_descendant_rule_does_not_apply_to_slotted_light_child() {
        let (doc, host, slotted) = make_shadow_host_with_slotted();
        install_shadow_sheet(host, ":host span { color: green; }");
        let s = compute_style(&doc, slotted, &Stylesheet::default(), &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 0, 0));
    }

    // ── CSS Shadow Parts L1: `::part()`, `exportparts`, контекст инкапсуляции ──

    fn set_attr(doc: &mut lumen_dom::Document, id: NodeId, name: &str, value: &str) {
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
            attrs.push(lumen_dom::Attribute { name: lumen_dom::QualName::html(name), value: value.into() });
        }
    }

    /// `<div id="host">` с shadow tree, внутри `<span part="p1 p2">`. (doc, host, span).
    fn make_part_host() -> (lumen_dom::Document, NodeId, NodeId) {
        let (mut doc, host) = make_shadow_host();
        let sr = doc.shadow_root_of(host).expect("shadow root");
        let span = doc.create_element(lumen_dom::QualName::html("span"));
        set_attr(&mut doc, span, "part", "p1 p2");
        doc.append_child(sr, span);
        (doc, host, span)
    }

    fn color_of(s: &ComputedStyle) -> (u8, u8, u8) {
        (s.color.r, s.color.g, s.color.b)
    }

    #[test]
    fn part_rule_in_document_styles_part_element() {
        let (doc, _host, span) = make_part_host();
        let sheet = lumen_css_parser::parse("#host::part(p1) { color: red; }");
        clear_shadow_sheets();
        let s = compute_style(&doc, span, &sheet, &ComputedStyle::root(), VP, false);
        assert_eq!(color_of(&s), (255, 0, 0));
    }

    #[test]
    fn part_rule_needs_every_listed_name() {
        let (doc, _host, span) = make_part_host();
        let both = lumen_css_parser::parse("#host::part(p1 p2) { color: red; }");
        let missing = lumen_css_parser::parse("#host::part(p1 p3) { color: red; }");
        let wrong_host = lumen_css_parser::parse("#other::part(p1) { color: red; }");
        let root = ComputedStyle::root();
        assert_eq!(color_of(&compute_style(&doc, span, &both, &root, VP, false)), (255, 0, 0));
        assert_eq!(color_of(&compute_style(&doc, span, &missing, &root, VP, false)), (0, 0, 0));
        assert_eq!(color_of(&compute_style(&doc, span, &wrong_host, &root, VP, false)), (0, 0, 0));
    }

    #[test]
    fn part_rule_does_not_style_element_without_part_attribute() {
        let (mut doc, _host, span) = make_part_host();
        let sr = doc.shadow_root_of(_host).expect("shadow root");
        let plain = doc.create_element(lumen_dom::QualName::html("span"));
        doc.append_child(sr, plain);
        let sheet = lumen_css_parser::parse("#host::part(p1) { color: red; }");
        let root = ComputedStyle::root();
        assert_eq!(color_of(&compute_style(&doc, plain, &sheet, &root, VP, false)), (0, 0, 0));
        assert_eq!(color_of(&compute_style(&doc, span, &sheet, &root, VP, false)), (255, 0, 0));
    }

    #[test]
    fn part_with_pseudo_class_after_it_matches_the_part() {
        let (doc, _host, span) = make_part_host();
        let sheet = lumen_css_parser::parse(
            "#host::part(p1):first-child { color: red; } #host::part(p1):not(.x) { color: blue; }",
        );
        let s = compute_style(&doc, span, &sheet, &ComputedStyle::root(), VP, false);
        assert_eq!(color_of(&s), (0, 0, 255), "structural pseudo-classes never match a part");
    }

    #[test]
    fn document_rule_does_not_reach_into_shadow_tree() {
        let (doc, _host, span) = make_part_host();
        let sheet = lumen_css_parser::parse("span { color: red; }");
        let s = compute_style(&doc, span, &sheet, &ComputedStyle::root(), VP, false);
        assert_eq!(color_of(&s), (0, 0, 0));
    }

    #[test]
    fn normal_declaration_of_outer_context_beats_inner_one() {
        // Cascade L5 §6.1: для normal выигрывает внешний контекст, даже при меньшей
        // специфичности; для !important — внутренний.
        let (doc, host, span) = make_part_host();
        install_shadow_sheet(host, "span#x, span { color: green; }");
        let sheet = lumen_css_parser::parse("#host::part(p1) { color: red; }");
        let s = compute_style(&doc, span, &sheet, &ComputedStyle::root(), VP, false);
        assert_eq!(color_of(&s), (255, 0, 0));
        install_shadow_sheet(host, "span { color: green !important; }");
        let s = compute_style(&doc, span, &sheet, &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(color_of(&s), (0, 128, 0));
    }

    #[test]
    fn document_normal_rule_beats_host_rule_of_shadow_sheet() {
        // BUG-1580: `:host` из shadow tree проигрывает обычному правилу документа.
        let (doc, host) = make_shadow_host();
        install_shadow_sheet(host, ":host { color: green; }");
        let sheet = lumen_css_parser::parse("#host { color: red; }");
        let s = compute_style(&doc, host, &sheet, &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(color_of(&s), (255, 0, 0));
    }

    /// outer-host › shadow › (`inner-host` [exportparts] › shadow › `<i part="deep">`).
    fn make_exported_part(exportparts: Option<&str>) -> (lumen_dom::Document, NodeId, NodeId, NodeId) {
        let (mut doc, outer, inner, deep) = make_nested_shadow_hosts();
        set_attr(&mut doc, deep, "part", "deep");
        set_attr(&mut doc, inner, "id", "inner");
        if let Some(v) = exportparts {
            set_attr(&mut doc, inner, "exportparts", v);
        }
        (doc, outer, inner, deep)
    }

    #[test]
    fn part_is_visible_one_tree_out_without_exportparts() {
        let (doc, outer, _inner, deep) = make_exported_part(None);
        let mut map: HashMap<NodeId, Stylesheet> = HashMap::new();
        map.insert(outer, lumen_css_parser::parse("#inner::part(deep) { color: red; }"));
        set_shadow_sheets(map);
        let doc_sheet = lumen_css_parser::parse("#host::part(deep) { color: blue; }");
        let s = compute_style(&doc, deep, &doc_sheet, &ComputedStyle::root(), VP, false);
        clear_shadow_sheets();
        assert_eq!(color_of(&s), (255, 0, 0), "the document cannot see a part nobody exports");
    }

    #[test]
    fn exportparts_forwards_part_to_the_outer_tree() {
        // `inner` forwards `deep` as `shown` into `outer`'s shadow tree, so the document
        // reaches it as `outer::part(shown)`; the old name stays visible only to `outer`'s sheet.
        let (doc, outer, _inner, deep) = make_exported_part(Some("deep: shown"));
        let doc_sheet = lumen_css_parser::parse("#host::part(shown) { color: blue; }");
        let root = ComputedStyle::root();
        clear_shadow_sheets();
        assert_eq!(color_of(&compute_style(&doc, deep, &doc_sheet, &root, VP, false)), (0, 0, 255));
        let by_old_name = lumen_css_parser::parse("#host::part(deep) { color: blue; }");
        assert_eq!(color_of(&compute_style(&doc, deep, &by_old_name, &root, VP, false)), (0, 0, 0));
        let mut map: HashMap<NodeId, Stylesheet> = HashMap::new();
        map.insert(outer, lumen_css_parser::parse("#inner::part(deep) { color: green; }"));
        set_shadow_sheets(map);
        let s = compute_style(&doc, deep, &Stylesheet::default(), &root, VP, false);
        clear_shadow_sheets();
        assert_eq!(color_of(&s), (0, 128, 0));
    }

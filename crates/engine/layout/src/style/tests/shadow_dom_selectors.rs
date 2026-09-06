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

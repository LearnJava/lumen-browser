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

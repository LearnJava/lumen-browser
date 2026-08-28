//! Тесты `style.rs`: `masonry-auto-flow`.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

// ─────────────────────────────────────────────────────────────────────────────
// masonry-auto-flow tests (CSS Masonry Layout §9)
// ─────────────────────────────────────────────────────────────────────────────

    use super::*;

    #[test]
    fn masonry_auto_flow_parse_definite_first() {
        assert_eq!(MasonryAutoFlow::parse("definite-first"), Some(MasonryAutoFlow::DefiniteFirst));
    }

    #[test]
    fn masonry_auto_flow_parse_next() {
        assert_eq!(MasonryAutoFlow::parse("next"), Some(MasonryAutoFlow::Next));
    }

    #[test]
    fn masonry_auto_flow_parse_ordered() {
        assert_eq!(MasonryAutoFlow::parse("ordered"), Some(MasonryAutoFlow::Ordered));
    }

    #[test]
    fn masonry_auto_flow_parse_unknown_is_none() {
        assert_eq!(MasonryAutoFlow::parse("dense"), None);
        assert_eq!(MasonryAutoFlow::parse(""), None);
    }

    #[test]
    fn masonry_auto_flow_default_is_definite_first() {
        assert_eq!(MasonryAutoFlow::default(), MasonryAutoFlow::DefiniteFirst);
    }

    #[test]
    fn masonry_auto_flow_root_style_default() {
        let s = ComputedStyle::root();
        assert_eq!(s.masonry_auto_flow, MasonryAutoFlow::DefiniteFirst);
    }

    #[test]
    fn masonry_auto_flow_apply_declaration() {
        let doc = lumen_html_parser::parse(r#"<div></div>"#);
        let sheet = lumen_css_parser::parse("div { masonry-auto-flow: ordered; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let style = compute_style(&doc, div, &sheet, &root, lumen_core::geom::Size { width: 800.0, height: 600.0 }, false);
        assert_eq!(style.masonry_auto_flow, MasonryAutoFlow::Ordered);
    }

    #[test]
    fn masonry_auto_flow_not_inherited() {
        let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
        let sheet = lumen_css_parser::parse("div { masonry-auto-flow: next; }");
        let root = ComputedStyle::root();
        let vp = lumen_core::geom::Size { width: 800.0, height: 600.0 };
        let body = doc.body().expect("body");
        let div = doc.get(body).children[0];
        let span = doc.get(div).children[0];
        let div_style = compute_style(&doc, div, &sheet, &root, vp, false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, vp, false);
        assert_eq!(div_style.masonry_auto_flow, MasonryAutoFlow::Next);
        assert_eq!(span_style.masonry_auto_flow, MasonryAutoFlow::DefiniteFirst, "masonry_auto_flow must not be inherited");
    }


//! Тесты `style.rs`: правило `gap` в каскаде.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

    use super::*;
    use lumen_core::geom::Size;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn parse_gap_rule(css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(r#"<div></div>"#);
        let sheet = lumen_css_parser::parse(&format!("div {{ display: flex; {} }}", css));
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let child = doc.get(body).children.first().copied().expect("div");
        compute_style(&doc, child, &sheet, &root, VP, false)
    }

    #[test]
    fn gap_rule_width_parses() {
        let s = parse_gap_rule("gap-rule-width: 4px;");
        assert!((s.gap_rule_width - 4.0).abs() < 0.01, "gap_rule_width={}", s.gap_rule_width);
    }

    #[test]
    fn gap_rule_style_solid_parses() {
        let s = parse_gap_rule("gap-rule-style: solid;");
        assert_eq!(s.gap_rule_style, BorderStyle::Solid);
    }

    #[test]
    fn gap_rule_color_parses() {
        let s = parse_gap_rule("gap-rule-color: #ff0000;");
        if let CssColor::Rgba(c) = s.gap_rule_color {
            assert_eq!((c.r, c.g, c.b), (255, 0, 0));
        } else {
            panic!("expected Rgba, got {:?}", s.gap_rule_color);
        }
    }

    #[test]
    fn gap_rule_shorthand_parses_all_components() {
        let s = parse_gap_rule("gap-rule: 3px dashed blue;");
        assert!((s.gap_rule_width - 3.0).abs() < 0.01, "width={}", s.gap_rule_width);
        assert_eq!(s.gap_rule_style, BorderStyle::Dashed);
        if let CssColor::Rgba(c) = s.gap_rule_color {
            assert_eq!((c.r, c.g, c.b), (0, 0, 255));
        } else {
            panic!("expected Rgba color for gap-rule shorthand");
        }
    }

    #[test]
    fn gap_rule_not_inherited() {
        // gap-rule-* are non-inherited; child div should get default 0/None/CurrentColor.
        let doc = lumen_html_parser::parse(r#"<div><span></span></div>"#);
        let sheet = lumen_css_parser::parse("div { gap-rule-width: 5px; gap-rule-style: solid; }");
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let div = doc.get(body).children.first().copied().expect("div");
        let span = doc.get(div).children.first().copied().expect("span");
        let div_style = compute_style(&doc, div, &sheet, &root, VP, false);
        let span_style = compute_style(&doc, span, &sheet, &div_style, VP, false);
        assert!((div_style.gap_rule_width - 5.0).abs() < 0.01);
        assert_eq!(span_style.gap_rule_width, 0.0, "gap_rule_width must not be inherited");
        assert_eq!(span_style.gap_rule_style, BorderStyle::None, "gap_rule_style must not be inherited");
    }

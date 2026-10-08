//! Тесты `style.rs`: регрессия индексированного каскада.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

//! Regression: indexed compute_style produces identical results to brute-force.
//!
//! These tests verify that the `RuleIndex` optimisation in `compute_style`
//! does not change which declarations are applied or their cascade order.

    use super::*;
    use lumen_core::geom::Size;

    const VP: Size = Size { width: 800.0, height: 600.0 };

    fn first_child_style(html: &str, css: &str) -> ComputedStyle {
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = ComputedStyle::root();
        let body = doc.body().expect("body");
        let child = doc.get(body).children.first().copied().expect("child");
        compute_style(&doc, child, &sheet, &root, VP, false)
    }

    #[test]
    fn id_selector_applies_correctly() {
        let s = first_child_style(r#"<div id="hero"></div>"#, "#hero { color: red; }");
        // `color` field on ComputedStyle is Color (plain struct, not CssColor)
        assert_eq!((s.color.r, s.color.g, s.color.b), (255, 0, 0));
    }

    #[test]
    fn class_selector_applies_correctly() {
        let s = first_child_style(
            r#"<div class="card active"></div>"#,
            ".card { width: 200px; } .active { background-color: blue; }",
        );
        assert_eq!(s.width, Some(Length::Px(200.0)));
        let bg = s.background_color.expect("bg").resolve(s.color);
        assert_eq!((bg.r, bg.g, bg.b), (0, 0, 255));
    }

    #[test]
    fn type_selector_applies_correctly() {
        let s = first_child_style("<p></p>", "p { color: green; }");
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 128, 0));
    }

    #[test]
    fn descendant_selector_applies_via_index() {
        // `.card .title` — subject is `.title`; only `.title` nodes are candidates.
        let doc = lumen_html_parser::parse(r#"<div class="card"><span class="title"></span></div>"#);
        let sheet = lumen_css_parser::parse(".card .title { color: red; }");
        let root = ComputedStyle::root();
        let body = doc.body().unwrap();
        let card = doc.get(body).children[0];
        let title = doc.get(card).children[0];
        // `.card` alone must NOT pick up the rule (no .title class)
        let card_style = compute_style(&doc, card, &sheet, &root, VP, false);
        assert_ne!((card_style.color.r, card_style.color.g, card_style.color.b), (255, 0, 0),
            "card must not pick up .card .title rule");
        // `.title` inside `.card` MUST pick up the rule
        let title_style = compute_style(&doc, title, &sheet, &root, VP, false);
        assert_eq!((title_style.color.r, title_style.color.g, title_style.color.b), (255, 0, 0),
            "title inside card must match .card .title");
    }

    #[test]
    fn multi_class_compound_not_false_positive() {
        // `.a.b` must match only nodes with BOTH classes; node with only `.a` must not match.
        let doc = lumen_html_parser::parse(r#"<div class="a"></div>"#);
        let sheet = lumen_css_parser::parse(".a.b { color: red; }");
        let root = ComputedStyle::root();
        let body = doc.body().unwrap();
        let div = doc.get(body).children[0];
        let s = compute_style(&doc, div, &sheet, &root, VP, false);
        assert_ne!((s.color.r, s.color.g, s.color.b), (255, 0, 0),
            "node with only .a must not match .a.b");
    }

    #[test]
    fn universal_applies_to_all() {
        let s = first_child_style("<span></span>", "* { color: blue; }");
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 0, 255));
    }

    #[test]
    fn specificity_order_preserved() {
        // `.card` (0,1,0) overrides `div` (0,0,1).
        let s = first_child_style(
            r#"<div class="card"></div>"#,
            "div { color: blue; } .card { color: red; }",
        );
        assert_eq!((s.color.r, s.color.g, s.color.b), (255, 0, 0), ".card must win over div");
    }

    #[test]
    fn source_order_preserved_within_same_specificity() {
        // Two class rules, same specificity → later wins.
        let s = first_child_style(
            r#"<div class="a b"></div>"#,
            ".a { color: red; } .b { color: blue; }",
        );
        assert_eq!((s.color.r, s.color.g, s.color.b), (0, 0, 255),
            "later same-specificity rule must win");
    }

    /// BUG-935 срез 90: `compute_pseudo_element_style` reads candidates from the
    /// pseudo-subject indices (top level, active `@media`, active `@supports`),
    /// which carry the general index's rule numbering. Cascade order across the
    /// containers (top level < `@media` < `@supports`) and the activity of each
    /// block must come out as before, and blocks without a pseudo rule must not
    /// disturb the numbering of the ones after them.
    #[test]
    fn pseudo_probe_keeps_cascade_order_across_containers() {
        let doc = lumen_html_parser::parse("<p class=x>t</p><span>s</span>");
        let body = doc.body().expect("body");
        let p = doc.get(body).children[0];
        let span = doc.get(body).children[1];
        let parent = ComputedStyle::root();
        let content_of = |css: &str, node: NodeId, pseudo: &str| {
            let sheet = lumen_css_parser::parse(css);
            compute_pseudo_element_style(&doc, node, pseudo, &sheet, &parent, VP, false)
                .map(|s| format!("{:?}", s.content))
        };
        let blocks = "@media (min-width: 100px) { .nope { margin: 0 } } \
                      p::before { content: \"top\" } \
                      @media (max-width: 10px) { p::before { content: \"inactive\" } } \
                      @media (min-width: 100px) { .nope2 { margin: 0 } p::before { content: \"media\" } }";
        let c = content_of(blocks, p, "before").expect("::before must match");
        assert!(c.contains("media") && !c.contains("inactive") && !c.contains("top"), "@media beats top level: {c}");
        let c = content_of(&format!("{blocks} @supports (display: grid) {{ p::before {{ content: \"sup\" }} }}"), p, "before")
            .expect("::before must match");
        assert!(c.contains("sup"), "@supports beats @media: {c}");
        let c = content_of("p::before { content: \"a\" } .x::before { content: \"b\" }", p, "before").expect("match");
        assert!(c.contains('b'), "class rule outranks type rule: {c}");
        assert!(content_of(blocks, span, "before").is_none(), "<span> matches no rule");
        assert!(content_of(blocks, p, "after").is_none(), "no ::after rule at all");
        assert!(
            content_of("p { color: red } p::after { content: \"z\" }", p, "after").is_some(),
            "a plain rule before the pseudo rule must not shift it out of the index",
        );
    }

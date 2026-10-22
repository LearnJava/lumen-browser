use super::*;
use super::state_pseudo_classes::color_by_id;

    // ── Функциональные pseudo: :nth-*, :*-of-type, :not ───────────────────

    /// Собирает все элементы с тегом `tag` из children корневого LayoutBox.
    fn block_children_by_tag<'a>(
        root: &'a LayoutBox,
        doc: &lumen_dom::Document,
        tag: &str,
    ) -> Vec<&'a LayoutBox> {
        root.children
            .iter()
            .filter(|c| {
                matches!(
                    &doc.get(c.node).data,
                    lumen_dom::NodeData::Element { name, .. } if name.local == tag
                )
            })
            .collect()
    }

    #[test]
    fn nth_child_odd_matches_1_3_5() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p><p>e</p>",
            "p:nth-child(odd) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps.len(), 5);
        for (i, p) in ps.iter().enumerate() {
            let one_based = (i + 1) as i32;
            let expected_red = one_based % 2 == 1;
            assert_eq!(
                p.style.color.r == 255,
                expected_red,
                "index={one_based}"
            );
        }
    }

    #[test]
    fn nth_child_specific_index() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-child(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn nth_child_formula_2n() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p><p>d</p>",
            "p:nth-child(2n) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // 2n: 2, 4, ...
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
        assert_eq!(ps[3].style.color.r, 255);
    }

    #[test]
    fn nth_last_child_matches_from_end() {
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-last-child(1) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // Последний матчит.
        assert_eq!(ps[2].style.color.r, 255);
        assert_eq!(ps[0].style.color.r, 0);
    }

    #[test]
    fn nth_of_type_counts_only_matching_tag() {
        // <h1><p1><h2><p2><p3> — :nth-of-type(2) для p должен попасть в p2.
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><h2>x</h2><p>p2</p><p>p3</p>",
            "p:nth-of-type(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        // p1 — это of-type index 1 → 0, p2 → 2 → 255, p3 → 3 → 0.
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn nth_child_of_selector_filters_pool() {
        // CSS Selectors L4 §6.6.5.1: `:nth-child(odd of .v)` нумерует ТОЛЬКО
        // элементы с классом `v`, остальные siblings не участвуют. Из
        // .v#a (index 1), .v#b (2), .v#c (3) — odd = a и c.
        let (root, doc) = lay_with_doc(
            r#"<p>x</p><p class="v" id="a">x</p><p>x</p><p class="v" id="b">x</p><p class="v" id="c">x</p>"#,
            "p:nth-child(odd of .v) { color: red; }",
        );
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 255);
    }

    #[test]
    fn nth_child_of_selector_does_not_match_non_filtered() {
        // Элемент, не матчащий of-selector, никогда не матчит pseudo —
        // независимо от того, какой у него index среди ВСЕХ siblings.
        let (root, doc) = lay_with_doc(
            r#"<p class="v" id="a">x</p><p id="b">x</p><p class="v" id="c">x</p>"#,
            "p:nth-child(1 of .v) { color: red; }",
        );
        // .v#a — первый матчащий .v → matches.
        // #b — не .v, не матчит вообще.
        // .v#c — второй матчащий .v → не matches 1.
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
    }

    #[test]
    fn nth_last_child_of_selector_filters_from_end() {
        let (root, doc) = lay_with_doc(
            r#"<p class="v" id="a">x</p><p class="v" id="b">x</p><p id="c">x</p><p class="v" id="d">x</p>"#,
            "p:nth-last-child(1 of .v) { color: red; }",
        );
        // С конца: первый .v — d (matches), второй .v — b (no), третий — a (no).
        assert_eq!(color_by_id(&doc, &root, "a").r, 0);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
        assert_eq!(color_by_id(&doc, &root, "d").r, 255);
    }

    #[test]
    fn nth_child_of_selector_list_union() {
        // of-clause принимает selector-list через запятую: соответствие
        // хотя бы одному → элемент в pool.
        let (root, doc) = lay_with_doc(
            r#"<p class="x" id="a">x</p><p id="b">x</p><p class="y" id="c">x</p><p class="x" id="d">x</p>"#,
            "p:nth-child(odd of .x, .y) { color: red; }",
        );
        // Pool по «.x OR .y»: a, c, d. odd-index в этом pool: a(1), d(3).
        assert_eq!(color_by_id(&doc, &root, "a").r, 255);
        assert_eq!(color_by_id(&doc, &root, "b").r, 0);
        assert_eq!(color_by_id(&doc, &root, "c").r, 0);
        assert_eq!(color_by_id(&doc, &root, "d").r, 255);
    }

    #[test]
    fn nth_child_backward_compat_without_of() {
        // Базовое поведение без of-clause не должно регрессировать.
        let (root, doc) = lay_with_doc(
            "<p>a</p><p>b</p><p>c</p>",
            "p:nth-child(2) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        assert_eq!(ps[1].style.color.r, 255);
        assert_eq!(ps[2].style.color.r, 0);
    }

    #[test]
    fn first_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<h1>x</h1><p>p1</p><p>p2</p>",
            "p:first-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255);
        assert_eq!(ps[1].style.color.r, 0);
    }

    #[test]
    fn last_of_type_matches() {
        let (root, doc) = lay_with_doc(
            "<p>p1</p><p>p2</p><h1>x</h1>",
            "p:last-of-type { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0);
        // p2 — последний `<p>` (h1 после него — другой тип), значит матчит.
        assert_eq!(ps[1].style.color.r, 255);
    }

    #[test]
    fn not_class_excludes() {
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p><p>c</p>"#,
            "p:not(.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "a should match");
        assert_eq!(ps[1].style.color.r, 0, "b.hl should NOT match");
        assert_eq!(ps[2].style.color.r, 255, "c should match");
    }

    #[test]
    fn not_with_compound_excludes_full() {
        // :not(p.hl) — исключает только p с классом hl, не любой <p> и не любой `.hl`.
        // Используем scope через body-класс чтобы не загрязнять html/body.
        let (root, doc) = lay_with_doc(
            r#"<body class="t"><p>x</p><p class="hl">y</p><div class="hl">z</div></body>"#,
            "body.t *:not(p.hl) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        let divs = block_children_by_tag(&root, &doc, "div");
        assert_eq!(ps[0].style.color.r, 255, "p без класса — матчит");
        assert_eq!(ps[1].style.color.r, 0, "p.hl — исключается");
        assert_eq!(divs[0].style.color.r, 255, "div.hl — не исключается");
    }

    #[test]
    fn not_selector_list_l4() {
        // CSS Selectors L4 §5.4: список селекторов внутри `:not(...)` —
        // элемент исключается, если матчит ХОТЯ БЫ ОДИН селектор списка.
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p><p id="x">c</p><p>d</p>"#,
            "p:not(.hl, #x) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "a — матчит");
        assert_eq!(ps[1].style.color.r, 0, "b.hl — исключается");
        assert_eq!(ps[2].style.color.r, 0, "c#x — исключается");
        assert_eq!(ps[3].style.color.r, 255, "d — матчит");
    }

    #[test]
    fn not_complex_with_descendant_combinator_l4() {
        // CSS Selectors L4 §5.4: combinator-ы внутри `:not` разрешены.
        // Исключаем <p>, у которых внутри (descendant) есть <a>.
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p>b <a>link</a></p><p>c</p>"#,
            "p:not(:has(a)) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 255, "p без <a> — матчит");
        assert_eq!(ps[1].style.color.r, 0, "p с <a> — исключается");
        assert_eq!(ps[2].style.color.r, 255, "p без <a> — матчит");
    }

    #[test]
    fn not_nested_double_negation_l4() {
        // CSS Selectors L4 §5.4: nested `:not(:not(...))` разрешён.
        // `:not(:not(.hl))` ≡ `.hl` (двойное отрицание).
        let (root, doc) = lay_with_doc(
            r#"<p>a</p><p class="hl">b</p>"#,
            "p:not(:not(.hl)) { color: red; }",
        );
        let ps = block_children_by_tag(&root, &doc, "p");
        assert_eq!(ps[0].style.color.r, 0, "a (нет .hl) — не матчит");
        assert_eq!(ps[1].style.color.r, 255, "b.hl — матчит (двойное :not)");
    }

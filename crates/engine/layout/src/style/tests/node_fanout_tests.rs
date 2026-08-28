//! Тесты `style.rs`: фан-аут рестайла по мутации DOM.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

// ─── BUG-341 S17: DOM-mutation fan-out narrowing ─────────────────────────────

    use super::*;
    use lumen_css_parser::parse as parse_css;
    use lumen_html_parser::parse as parse_html;

    /// `<ul>` with two `.item` siblings plus an unrelated subtree — the same
    /// shape the S3/S7 differential tests use.
    fn fixture() -> Document {
        parse_html(
            r#"<ul id="menu">
                <li id="a" class="item" data-x="1">a</li>
                <li id="b" class="item">b</li>
            </ul>
            <div id="unrelated"><p>x</p></div>"#,
        )
    }

    fn roots(doc: &Document, sheet: &Stylesheet, node: NodeId, attr: &str) -> HashSet<NodeId> {
        let index = restyle_node_index(doc, sheet);
        restyle_root_set_for_node_change(doc, [(node, NodeChange::Attr(attr))], &index)
    }

    #[test]
    fn a_sheet_without_sibling_combinators_narrows_to_the_node() {
        let doc = fixture();
        let sheet = parse_css(".item { color: black; } .item .icon { color: red; }");
        let a = doc.find_by_id("a").expect("#a");
        assert_eq!(
            roots(&doc, &sheet, a, "data-x"),
            [a].into_iter().collect::<HashSet<_>>(),
            "no selector reaches a sibling, so the changed node's own subtree is the whole root-set",
        );
    }

    #[test]
    fn a_sibling_rule_keyed_on_the_changed_attribute_widens_to_the_parent() {
        let doc = fixture();
        let sheet = parse_css("[data-x=\"1\"] + .item { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        let menu = doc.find_by_id("menu").expect("#menu");
        assert_eq!(
            roots(&doc, &sheet, a, "data-x"),
            [menu].into_iter().collect::<HashSet<_>>(),
            "writing `data-x` can flip the sibling rule — the parent's subtree covers that",
        );
    }

    #[test]
    fn a_sibling_rule_that_cannot_match_the_changed_node_still_narrows() {
        // The sheet has a sibling combinator, but its left compound (`.other`)
        // cannot match `#a` no matter what `data-x` becomes. This is the case a
        // sheet-wide "does any selector use `+`/`~`" check would get wrong, and
        // the reason the narrowing is per-node.
        let doc = fixture();
        let sheet = parse_css(".other + .item { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        assert_eq!(
            roots(&doc, &sheet, a, "data-x"),
            [a].into_iter().collect::<HashSet<_>>(),
            "`.other` cannot match #a, so no sibling of #a can react to its `data-x`",
        );
    }

    #[test]
    fn a_class_write_widens_when_the_sibling_rule_is_class_keyed() {
        // `.item.active + .item` — the left compound is entirely class-keyed,
        // so a `class` write on #a could make it match. Must widen.
        let doc = fixture();
        let sheet = parse_css(".item.active + .item { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        let menu = doc.find_by_id("menu").expect("#menu");
        assert_eq!(roots(&doc, &sheet, a, "class"), [menu].into_iter().collect::<HashSet<_>>());
        // …but a `data-x` write cannot: `.item.active` doesn't currently match
        // #a and no `data-x` value can change that.
        assert_eq!(roots(&doc, &sheet, a, "data-x"), [a].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn a_sibling_combinator_before_the_matching_compound_does_not_widen() {
        // `[data-y] + [data-x]` — the compound `data-x` keys on is the subject;
        // nothing follows it, so a write on it reaches nobody else.
        let doc = fixture();
        let sheet = parse_css("[data-y] + [data-x] { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        assert_eq!(roots(&doc, &sheet, a, "data-x"), [a].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn a_descendant_after_a_sibling_combinator_still_widens() {
        let doc = fixture();
        let sheet = parse_css("[data-x] ~ .item .icon { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        let menu = doc.find_by_id("menu").expect("#menu");
        assert_eq!(roots(&doc, &sheet, a, "data-x"), [menu].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn a_structural_change_always_widens() {
        // No attribute name describes "the child list moved", and
        // `:nth-child`/`:empty`/sibling combinators all react to it.
        let doc = fixture();
        let sheet = parse_css(".item { color: black; }");
        let menu = doc.find_by_id("menu").expect("#menu");
        let index = restyle_node_index(&doc, &sheet);
        let parent = doc.get(menu).parent.expect("#menu has a parent");
        assert_eq!(
            restyle_root_set_for_node_change(&doc, [(menu, NodeChange::Unattributed)], &index),
            [parent].into_iter().collect::<HashSet<_>>(),
        );
    }

    #[test]
    fn has_anywhere_in_the_sheet_widens_to_the_whole_document() {
        // BUG-349: `:has()` binds an ancestor's match to a descendant's state,
        // and that ancestor can sit arbitrarily far above the mutated node's
        // parent — parent-only widening (S17's pre-BUG-349 fallback) is not
        // enough to catch it, so the root-set must cover the whole document.
        let doc = fixture();
        let sheet = parse_css("ul:has(.item) { color: green; }");
        let index = restyle_node_index(&doc, &sheet);
        assert!(index.is_conservative(), ":has() anywhere must force the conservative path");
        assert!(index.has_has_dependency(), ":has() anywhere must set the has-dependency flag");
        let a = doc.find_by_id("a").expect("#a");
        assert_eq!(
            roots(&doc, &sheet, a, "data-x"),
            [doc.root()].into_iter().collect::<HashSet<_>>(),
            "a `:has()`-affected ancestor can be more than one level up, so the whole \
             document must widen, not just #a's parent",
        );
    }

    #[test]
    fn has_far_above_the_mutated_node_is_caught_by_the_document_wide_widening() {
        // The exact shape BUG-349 documents: `article:has(.expanded)` reacts to
        // a class toggle on a node several levels below `<article>`, which the
        // old parent-only widening (still correct for plain sibling-reach
        // selectors) could never reach.
        let doc = parse_html(
            r#"<article id="art">
                <section><div><span id="leaf" class="collapsed"></span></div></section>
            </article>"#,
        );
        let sheet = parse_css("article:has(.expanded) { border: 1px solid red; }");
        let leaf = doc.find_by_id("leaf").expect("#leaf");
        let art = doc.find_by_id("art").expect("#art");
        let index = restyle_node_index(&doc, &sheet);
        let got = restyle_root_set_for_node_change(&doc, [(leaf, NodeChange::Attr("class"))], &index);
        assert_eq!(got, [doc.root()].into_iter().collect::<HashSet<_>>());
        assert!(
            got.contains(&doc.root()) && doc.root() != art,
            "the whole-document root-set must cover #art even though it is three levels \
             above #leaf, well outside #leaf's parent's subtree",
        );
    }

    #[test]
    fn nth_child_of_selector_disables_narrowing() {
        // `:nth-child(2 of .item)` makes one element's match depend on which of
        // its *siblings* carry `.item` — sibling reach with no combinator to
        // see it.
        let doc = fixture();
        let sheet = parse_css("li:nth-child(2 of .item) { color: green; }");
        let index = restyle_node_index(&doc, &sheet);
        assert!(index.is_conservative(), ":nth-child(… of …) must force the conservative path");
    }

    #[test]
    fn a_plain_nth_child_does_not_disable_narrowing() {
        // Positions don't move on an attribute write, so plain structural
        // pseudo-classes are irrelevant to this narrowing (a *structural*
        // change reports `Unattributed` and widens regardless).
        let doc = fixture();
        let sheet = parse_css("li:nth-child(2) { color: green; }");
        let index = restyle_node_index(&doc, &sheet);
        assert!(!index.is_conservative());
        let a = doc.find_by_id("a").expect("#a");
        assert_eq!(roots(&doc, &sheet, a, "data-x"), [a].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn a_pseudo_class_in_a_sibling_source_compound_is_treated_as_possible() {
        // `.item:checked + .item` — `:checked` reads the `checked` attribute,
        // so a `checked` write could flip the sibling rule. The narrowing must
        // not look through pseudo-classes and conclude otherwise.
        let doc = fixture();
        let sheet = parse_css(".item:checked + .item { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        let menu = doc.find_by_id("menu").expect("#menu");
        assert_eq!(roots(&doc, &sheet, a, "checked"), [menu].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn media_blocks_are_scanned_too() {
        // A sibling rule hidden inside `@media` must count — the same carve-out
        // S7 made for `restyle_state_index`.
        let doc = fixture();
        let sheet = parse_css("@media (min-width: 1px) { [data-x] + .item { color: green; } }");
        let a = doc.find_by_id("a").expect("#a");
        let menu = doc.find_by_id("menu").expect("#menu");
        assert_eq!(roots(&doc, &sheet, a, "data-x"), [menu].into_iter().collect::<HashSet<_>>());
    }

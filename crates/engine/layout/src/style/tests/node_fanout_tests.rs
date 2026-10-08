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
    fn has_names_the_ancestors_it_can_flip_not_the_whole_document() {
        // BUG-349: `:has()` binds an ancestor's match to a descendant's state,
        // and that ancestor can sit arbitrarily far above the mutated node's
        // parent. BUG-935 s58: the root-set names exactly the ancestors that
        // could match a `:has()`-carrying compound (`ul` here), plus the usual
        // `#a` itself — not the document.
        let doc = fixture();
        let sheet = parse_css("ul:has(.item) { color: green; }");
        let index = restyle_node_index(&doc, &sheet);
        assert!(!index.is_conservative(), "`:has()` has its own reach analysis");
        assert!(index.has_has_dependency(), ":has() anywhere must set the has-dependency flag");
        let a = doc.find_by_id("a").expect("#a");
        let got = roots(&doc, &sheet, a, "class");
        assert!(got.contains(&a));
        assert!(!got.contains(&doc.root()), "the document must not be the root: {got:?}");
        let uls: Vec<_> = got.iter().filter(|&&n| doc.get(n).element_name().is_some_and(|q| q.local == "ul")).collect();
        assert_eq!(uls.len(), 1, "the `ul` ancestor that could match `ul:has(.item)` is a root: {got:?}");
    }

    #[test]
    fn a_write_no_has_argument_reads_does_not_reach_the_has_subject() {
        // BUG-1211: `data-*`/`aria-*`/`style` writes and class tokens that no `:has()` argument names
        // cannot flip `ul:has(.item)`; the ancestor is not a root (cnn.com's script writes ~30
        // `data-zjs-*` attributes per link and read `innerText` after each).
        let doc = fixture();
        let sheet = parse_css("ul:has(.item) { color: green; }");
        let index = restyle_node_index(&doc, &sheet);
        let a = doc.find_by_id("a").expect("#a");
        let has_ul = |got: &HashSet<NodeId>| got.iter().any(|&n| doc.get(n).element_name().is_some_and(|q| q.local == "ul"));
        for attr in ["data-x", "aria-label", "style"] {
            let got = roots(&doc, &sheet, a, attr);
            assert!(!has_ul(&got), "`{attr}` write must not make the `ul` a root: {got:?}");
        }
        let from = |name: &'static str, old: &'static str| {
            restyle_root_set_for_node_change(&doc, [(a, NodeChange::AttrFrom { name, old })], &index)
        };
        // `#a` keeps `item`; only `foo` came off, which no `:has()` argument names.
        assert!(!has_ul(&from("class", "item foo")), "an unrelated class token write must not reach the `ul`");
        assert!(has_ul(&from("class", "")), "gaining `item` can flip `ul:has(.item)`");
        // A plain `class` write without the old value, and attributes read through pseudo-classes, stay wide.
        assert!(has_ul(&roots(&doc, &sheet, a, "class")));
        assert!(has_ul(&roots(&doc, &sheet, a, "disabled")));
    }

    #[test]
    fn an_attribute_a_has_argument_names_still_reaches_the_has_subject() {
        let doc = fixture();
        let sheet = parse_css("ul:has([data-x]) { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        let got = roots(&doc, &sheet, a, "data-x");
        assert!(got.iter().any(|&n| doc.get(n).element_name().is_some_and(|q| q.local == "ul")), "{got:?}");
        let got = roots(&doc, &sheet, a, "data-y");
        assert!(!got.iter().any(|&n| doc.get(n).element_name().is_some_and(|q| q.local == "ul")), "{got:?}");
    }

    #[test]
    fn a_class_attribute_selector_in_has_flips_only_when_its_match_changes() {
        // BUG-1211: `:has([class*="video"])` reads the whole `class` value, but a write that leaves its
        // match as it was (`foo` -> `foo bar`) cannot flip the `ul`.
        let doc = fixture();
        let sheet = parse_css(r#"ul:has([class*="video"]) { color: green; }"#);
        let index = restyle_node_index(&doc, &sheet);
        let a = doc.find_by_id("a").expect("#a");
        let has_ul = |got: &HashSet<NodeId>| got.iter().any(|&n| doc.get(n).element_name().is_some_and(|q| q.local == "ul"));
        let from = |old: &'static str| {
            restyle_root_set_for_node_change(&doc, [(a, NodeChange::AttrFrom { name: "class", old })], &index)
        };
        // `#a` is now `item`: neither it nor `item foo` contains `video`.
        assert!(!has_ul(&from("item foo")), "match stays false on both sides");
        assert!(has_ul(&from("item video")), "`video` came off: the match flipped");
        assert!(has_ul(&roots(&doc, &sheet, a, "class")), "old value unknown: stay wide");
    }

    #[test]
    fn has_far_above_the_mutated_node_is_caught() {
        // The exact shape BUG-349 documents: `article:has(.expanded)` reacts to
        // a class toggle on a node several levels below `<article>`, which a
        // parent-only widening (still correct for plain sibling-reach selectors)
        // could never reach.
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
        assert_eq!(got, [leaf, art].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn a_has_subject_followed_by_a_sibling_combinator_widens_to_its_parent() {
        // `.card:has(.x) + .after` — `.after` restyles when `.card`'s result flips,
        // and it is not in `.card`'s subtree.
        let doc = parse_html(
            r#"<div id="wrap"><div class="card" id="c"><i id="leaf"></i></div><p class="after"></p></div>"#,
        );
        let sheet = parse_css(".card:has(.x) + .after { color: red; }");
        let leaf = doc.find_by_id("leaf").expect("#leaf");
        let wrap = doc.find_by_id("wrap").expect("#wrap");
        let index = restyle_node_index(&doc, &sheet);
        let got = restyle_root_set_for_node_change(&doc, [(leaf, NodeChange::Attr("class"))], &index);
        assert!(got.contains(&wrap), "{got:?}");
    }

    #[test]
    fn a_forward_sibling_has_argument_reaches_previous_siblings_of_ancestors() {
        // `.a:has(+ .b)` flips when `.b` (a later sibling of `.a`) changes.
        let doc = parse_html(r#"<div><i class="a" id="a"></i><i class="b" id="b"></i></div>"#);
        let sheet = parse_css(".a:has(+ .b) { color: red; }");
        let a = doc.find_by_id("a").expect("#a");
        let b = doc.find_by_id("b").expect("#b");
        let index = restyle_node_index(&doc, &sheet);
        let got = restyle_root_set_for_node_change(&doc, [(b, NodeChange::Attr("class"))], &index);
        assert!(got.contains(&a), "{got:?}");
    }

    #[test]
    fn has_with_a_shadow_root_in_the_document_still_widens_to_the_whole_document() {
        let mut doc = fixture();
        let host = doc.find_by_id("a").expect("#a");
        doc.attach_shadow(host, lumen_dom::ShadowRootMode::Open);
        let sheet = parse_css("ul:has(.item) { color: green; }");
        let index = restyle_node_index(&doc, &sheet);
        assert!(index.is_conservative());
        let got = restyle_root_set_for_node_change(&doc, [(host, NodeChange::Attr("class"))], &index);
        assert_eq!(got, [doc.root()].into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn nth_child_of_selector_it_cannot_model_disables_narrowing() {
        // `:nth-child(2 of .item)` makes one element's match depend on which of
        // its *siblings* carry `.item` — sibling reach with no combinator to
        // see it. An `S` that reads more than the sibling's own attributes (a
        // pseudo-class, a combinator) is not modelled: the whole sheet widens.
        let doc = fixture();
        for css in ["li:nth-child(2 of .item:checked) { color: green; }", "li:nth-child(2 of ul .item) { color: green; }"] {
            let sheet = parse_css(css);
            let index = restyle_node_index(&doc, &sheet);
            assert!(index.is_conservative(), "{css}: must force the conservative path");
        }
    }

    #[test]
    fn nth_child_of_a_simple_selector_sends_a_flipping_write_to_the_parent() {
        // BUG-1211: `S` = `.item` is decided by the sibling's own `class`, so only a write that can
        // flip it reaches the siblings (the parent is the root); anything else narrows to the node.
        let doc = fixture();
        let sheet = parse_css("li:nth-child(2 of .item) { color: green; }");
        let index = restyle_node_index(&doc, &sheet);
        assert!(!index.is_conservative());
        let a = doc.find_by_id("a").expect("#a");
        let menu = doc.find_by_id("menu").expect("#menu");
        let one = |change| restyle_root_set_for_node_change(&doc, [(a, change)], &index);
        let only = |n: NodeId| [n].into_iter().collect::<HashSet<_>>();
        assert_eq!(one(NodeChange::Attr("data-x")), only(a));
        assert_eq!(one(NodeChange::AttrFrom { name: "class", old: "item loaded" }), only(a));
        assert_eq!(one(NodeChange::AttrFrom { name: "class", old: "loaded" }), only(menu));
        assert_eq!(one(NodeChange::Attr("class")), only(menu));
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

    /// BUG-935 срез 74: an index built over a shared sheet (the one the same-tick flush keeps from
    /// one flush to the next) answers every question like the one that borrows it — sibling reach,
    /// `:has()`, structure, the ancestor readers of a `class` write — and keeps answering after the
    /// borrowed sheet's own scope has ended.
    #[test]
    fn a_shared_index_answers_like_a_borrowed_one() {
        let sheets = [
            ".item { color: black; }",
            "[data-x=\"1\"] + .item { color: green; }",
            "ul:has(.item) { color: green; } .item:first-child + .item { color: red; }",
            "#menu.open .item { color: blue; } .a ~ .b { color: red; } @media (min-width: 1px) { .item:checked + li { color: red; } }",
        ];
        for text in sheets {
            let doc = fixture();
            let shared = std::sync::Arc::new(parse_css(text));
            let kept = restyle_node_index_shared(&doc, &shared);
            drop(shared);
            let sheet = parse_css(text);
            let borrowed = restyle_node_index(&doc, &sheet);
            let (a, menu) = (doc.find_by_id("a").expect("#a"), doc.find_by_id("menu").expect("#menu"));
            for attr in ["data-x", "checked", "class", "id"] {
                let change = [(a, NodeChange::AttrFrom { name: attr, old: "x" })];
                assert_eq!(
                    restyle_root_set_for_node_change(&doc, change, &kept),
                    restyle_root_set_for_node_change(&doc, change, &borrowed),
                    "{text} / {attr}",
                );
            }
            let change = [(menu, NodeChange::ChildList)];
            assert_eq!(
                restyle_roots_for_node_changes(&doc, change, &kept).shallow,
                restyle_roots_for_node_changes(&doc, change, &borrowed).shallow,
                "{text} / child list",
            );
            assert_eq!(kept.sibling_source_count(), borrowed.sibling_source_count(), "{text}");
            assert_eq!(kept.has_has_dependency(), borrowed.has_has_dependency(), "{text}");
            assert_eq!(
                kept.affected_descendants(&doc, menu, "class", Some("")),
                borrowed.affected_descendants(&doc, menu, "class", Some("")),
                "{text} / readers",
            );
        }
    }

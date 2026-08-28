//! Тесты `style.rs`: фан-аут рестайла по состоянию.
//!
//! Перенесено батчем SPLIT-ST2 без правок тел.

// ─── BUG-341 S7: hover fan-out narrowing ────────────────────────────────────

    use super::*;
    use lumen_css_parser::parse as parse_css;

    #[test]
    fn subject_position_hover_needs_no_fanout() {
        let sheet = parse_css(".item:hover { color: blue; }");
        assert!(!stylesheet_needs_state_fanout(&sheet), "hover on the subject alone stays within its own node");
    }

    #[test]
    fn descendant_of_hover_needs_no_fanout() {
        // `.item:hover .icon` — the styled subject (`.icon`) is a descendant
        // of the hovered node, already covered by invalidating that node's
        // own subtree without widening to its parent.
        let sheet = parse_css(".item:hover .icon { color: blue; }");
        assert!(!stylesheet_needs_state_fanout(&sheet), "descendant combinator after :hover stays within the subtree");
    }

    #[test]
    fn next_sibling_of_hover_needs_fanout() {
        let sheet = parse_css(".item:hover + .item { color: green; }");
        assert!(stylesheet_needs_state_fanout(&sheet), "`+` after :hover reaches outside the hovered node's subtree");
    }

    #[test]
    fn later_sibling_of_hover_needs_fanout() {
        let sheet = parse_css(".item:hover ~ .item { color: green; }");
        assert!(stylesheet_needs_state_fanout(&sheet), "`~` after :hover reaches outside the hovered node's subtree");
    }

    #[test]
    fn sibling_combinator_before_hover_needs_no_fanout() {
        // `.a + .b:hover` — the dynamic-state compound (`.b:hover`) IS the
        // subject; nothing follows it, so no widening is needed even though
        // an earlier compound in the chain used a sibling combinator.
        let sheet = parse_css(".a + .b:hover { color: red; }");
        assert!(!stylesheet_needs_state_fanout(&sheet), "a sibling combinator before the dynamic-state compound doesn't matter");
    }

    #[test]
    fn descendant_then_sibling_after_hover_needs_fanout() {
        // `.item:hover .a ~ .b` — subject `.b` is a sibling of a descendant
        // of the hovered node, still outside the hovered node's own subtree.
        let sheet = parse_css(".item:hover .a ~ .b { color: green; }");
        assert!(stylesheet_needs_state_fanout(&sheet), "a sibling combinator anywhere after the dynamic-state compound needs fanout");
    }

    #[test]
    fn is_wrapping_hover_followed_by_sibling_needs_fanout() {
        let sheet = parse_css(":is(.item:hover) ~ .item { color: green; }");
        assert!(stylesheet_needs_state_fanout(&sheet), ":is() wrapping a dynamic-state selector must still be detected");
    }

    #[test]
    fn not_wrapping_focus_followed_by_sibling_needs_fanout() {
        let sheet = parse_css(".item:not(.disabled):focus ~ .item { color: green; }");
        assert!(stylesheet_needs_state_fanout(&sheet), ":not() alongside :focus in the same compound must still be detected");
    }

    #[test]
    fn plain_not_without_dynamic_state_needs_no_fanout() {
        // `:not()` on its own (no dynamic pseudo inside or alongside it) must
        // not trip the conservative fallback — only :has()-with-dynamic-state
        // and functional pseudo-classes actually *containing* dynamic state
        // force it.
        let sheet = parse_css(".item:not(.disabled) ~ .item { color: red; }");
        assert!(!stylesheet_needs_state_fanout(&sheet), "a plain :not() with no dynamic pseudo-class must not force fanout");
    }

    #[test]
    fn has_with_dynamic_state_always_needs_fanout() {
        // `:has()`'s search direction isn't modelled by this v1 narrowing —
        // any dynamic-state pseudo inside it must force the conservative path.
        let sheet = parse_css("article:has(.child:hover) { color: blue; }");
        assert!(stylesheet_needs_state_fanout(&sheet), ":has() containing a dynamic-state pseudo must force fanout");
    }

    #[test]
    fn has_without_dynamic_state_needs_no_fanout() {
        let sheet = parse_css("article:has(.child) { color: blue; }");
        assert!(!stylesheet_needs_state_fanout(&sheet), ":has() with no dynamic-state pseudo inside must not force fanout");
    }

    #[test]
    fn rule_inside_media_block_is_scanned() {
        let sheet = parse_css("@media (min-width: 1px) { .item:hover ~ .item { color: green; } }");
        assert!(stylesheet_needs_state_fanout(&sheet), "selectors inside @media must be scanned too");
    }

    #[test]
    fn shadow_root_forces_conservative_fanout() {
        let mut doc = lumen_html_parser::parse(r#"<div id="host"></div>"#);
        let body = doc.body().expect("body");
        let host = doc.get(body).children[0];
        doc.attach_shadow(host, lumen_dom::ShadowRootMode::Open);
        let sheet = parse_css(".item:hover { color: blue; }");
        let index = restyle_state_index(&doc, &sheet);
        assert!(
            index.needs_fanout(),
            "a document with any shadow root must stay on the conservative path (shadow selectors aren't scanned)",
        );
        assert!(index.is_conservative(), "a shadow root must also disable S14's per-node narrowing");
    }

    #[test]
    fn no_shadow_root_and_safe_sheet_allows_narrowing() {
        let doc = lumen_html_parser::parse(r#"<div class="item"></div>"#);
        let sheet = parse_css(".item:hover { color: blue; }");
        let index = restyle_state_index(&doc, &sheet);
        assert!(!index.needs_fanout(), "no shadow roots + no fanout-needing selector must narrow");
        assert!(!index.is_conservative(), "no shadow roots + no dynamic :has() must allow per-node narrowing");
    }

    #[test]
    fn root_set_narrows_to_flipped_nodes_when_no_fanout_needed() {
        // Real chrome-shaped CSS: `.tab-row:hover` (subject) and
        // `.tab-row:hover .tab-close` (descendant) — no sibling combinator
        // anywhere, matching `assets/chrome/chrome.html`'s actual pattern.
        let html = r#"<ul>
            <li id="a" class="tab-row"><span class="tab-close"></span></li>
            <li id="b" class="tab-row"><span class="tab-close"></span></li>
        </ul>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css(
            ".tab-row:hover { background: red; } .tab-row:hover .tab-close { display: flex; }",
        );
        let a = doc.find_by_id("a").expect("#a");
        let b = doc.find_by_id("b").expect("#b");

        let index = restyle_state_index(&doc, &sheet);
        assert!(!index.needs_fanout(), "chrome-shaped hover CSS must not need fanout");

        let roots = restyle_root_set_for_state_change(&doc, Some(a), Some(b), &index);
        assert_eq!(
            roots,
            [a, b].into_iter().collect::<HashSet<_>>(),
            "narrowed root-set must be exactly the flipped nodes, not their parents",
        );
    }

    #[test]
    fn root_set_still_widens_to_parent_when_fanout_needed() {
        let html = r#"<ul>
            <li id="a" class="item"></li>
            <li id="b" class="item"></li>
        </ul>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css(".item:hover + .item { color: green; }");
        let a = doc.find_by_id("a").expect("#a");
        let b = doc.find_by_id("b").expect("#b");
        let parent = doc.get(a).parent.expect("#a has a parent");

        let index = restyle_state_index(&doc, &sheet);
        assert!(index.needs_fanout(), "sibling-combinator hover CSS must need fanout");

        let roots = restyle_root_set_for_state_change(&doc, Some(a), Some(b), &index);
        assert_eq!(
            roots,
            [parent].into_iter().collect::<HashSet<_>>(),
            "widened root-set must be the flipped nodes' shared parent",
        );
    }

    // ── BUG-341 S14: per-node narrowing of the flipped ancestor chain ────────

    /// The shape CC-12 hits every other cycle and the reason S14 exists: on a
    /// "nothing was hovered → deep node is hovered" transition `:hover` really
    /// does flip on every ancestor up to the document root, so the pre-S14
    /// root-set contained the root and forced a whole-document re-cascade.
    #[test]
    fn nothing_hovered_to_deep_node_drops_ancestors_no_hover_rule_can_match() {
        let html = r#"<main><section><div id="deep"><span>x</span></div></section></main>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css("button:hover { color: red; } .tab-row:hover .icon { display: none; }");
        let deep = doc.find_by_id("deep").expect("#deep");

        let index = restyle_state_index(&doc, &sheet);
        assert!(!index.is_conservative(), "plain hover rules must allow narrowing");

        let roots = restyle_root_set_for_state_change(&doc, None, Some(deep), &index);
        assert!(
            roots.is_empty(),
            "no selector can react to hover on #deep or any of its ancestors, so the whole \
             flipped chain must drop out of the root-set, got {roots:?}",
        );
    }

    #[test]
    fn only_the_ancestors_a_hover_rule_can_match_stay_in_the_root_set() {
        let html = r#"<main><button id="btn"><span id="label">x</span></button></main>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css("button:hover { color: red; }");
        let btn = doc.find_by_id("btn").expect("#btn");
        let label = doc.find_by_id("label").expect("#label");

        let index = restyle_state_index(&doc, &sheet);
        let roots = restyle_root_set_for_state_change(&doc, None, Some(label), &index);
        assert_eq!(
            roots,
            [btn].into_iter().collect::<HashSet<_>>(),
            "`button:hover` can only observe the flip on the <button> itself — <main>, <body>, \
             <html> and the document node must all drop out",
        );
    }

    #[test]
    fn pseudo_element_after_a_hover_compound_still_attributes_to_its_element() {
        // `.tab:hover::before` — the real matcher strips `::before` before
        // matching the compound against an element, so the narrowing must not
        // let the pseudo-element part veto the match.
        let html = r#"<ul><li id="a" class="tab"></li></ul>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css(".tab:hover::before { content: \"x\"; }");
        let a = doc.find_by_id("a").expect("#a");

        let index = restyle_state_index(&doc, &sheet);
        let roots = restyle_root_set_for_state_change(&doc, None, Some(a), &index);
        assert_eq!(
            roots,
            [a].into_iter().collect::<HashSet<_>>(),
            "a `::before` on a hover compound must keep its element in the root-set",
        );
    }

    #[test]
    fn dynamic_has_disables_per_node_narrowing() {
        // `:has()` binds the state to a node other than the one carrying the
        // compound, so S14's "the flip is only observable on the node it
        // matches" argument does not hold — the whole chain must stay.
        let html = r#"<main><section><div id="deep"></div></section></main>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css("article:has(.child:hover) { color: blue; }");
        let deep = doc.find_by_id("deep").expect("#deep");

        let index = restyle_state_index(&doc, &sheet);
        assert!(index.is_conservative(), "dynamic :has() must disable narrowing");
        let roots = restyle_root_set_for_state_change(&doc, None, Some(deep), &index);
        assert!(
            roots.len() > 1,
            "the conservative path must keep the whole flipped ancestor chain, got {roots:?}",
        );
    }

    #[test]
    fn shadow_root_disables_per_node_narrowing() {
        let mut doc = lumen_html_parser::parse(r#"<div id="host"><div id="deep"></div></div>"#);
        let host = doc.find_by_id("host").expect("#host");
        doc.attach_shadow(host, lumen_dom::ShadowRootMode::Open);
        let sheet = parse_css("button:hover { color: red; }");
        let deep = doc.find_by_id("deep").expect("#deep");

        let index = restyle_state_index(&doc, &sheet);
        assert!(index.is_conservative(), "a shadow root must disable narrowing (shadow sheets unscanned)");
        let roots = restyle_root_set_for_state_change(&doc, None, Some(deep), &index);
        assert!(!roots.is_empty(), "the conservative path must keep the flipped chain, got {roots:?}");
    }

    #[test]
    fn state_nested_in_is_matches_conservatively() {
        // `:is(.tab:hover, .x)` — the narrowing cannot tell which branch the
        // state sits in, so the whole compound is treated as "could match
        // anything". Over-approximating costs narrowing, never correctness.
        let html = r#"<main><div id="deep"></div></main>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css(":is(.tab:hover, .x) { color: red; }");
        let deep = doc.find_by_id("deep").expect("#deep");

        let index = restyle_state_index(&doc, &sheet);
        assert!(!index.is_conservative(), ":is() with dynamic state is not the :has() case");
        let roots = restyle_root_set_for_state_change(&doc, None, Some(deep), &index);
        assert!(
            roots.len() > 1,
            "a compound whose state hides in a nested list must keep the whole chain, got {roots:?}",
        );
    }

    #[test]
    fn focus_within_on_a_matching_ancestor_stays_in_the_root_set() {
        // `:focus-within` is the one dynamic-state pseudo that legitimately
        // matches ancestors, and chrome.html uses exactly this shape
        // (`.omnibox:focus-within`) — the narrowing must keep the matching
        // ancestor while still dropping the ones above it.
        let html = r#"<main><div id="ob" class="omnibox"><input id="inp"></div></main>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = parse_css(".omnibox:focus-within { border-color: blue; }");
        let ob = doc.find_by_id("ob").expect("#ob");
        let inp = doc.find_by_id("inp").expect("#inp");

        let index = restyle_state_index(&doc, &sheet);
        let roots = restyle_root_set_for_state_change(&doc, None, Some(inp), &index);
        assert_eq!(
            roots,
            [ob].into_iter().collect::<HashSet<_>>(),
            "the `.omnibox` ancestor observes `:focus-within`; nothing above it does",
        );
    }

use super::*;
use lumen_core::geom::Size;

/// BUG-1424: alpha prints as the shortest decimal that rounds back to the same 8-bit value.
#[test]
fn color_alpha_serialises_shortest_round_tripping_decimal() {
    let c = |a| color_to_css(Color { r: 0, g: 128, b: 0, a });
    assert_eq!(c(128), "rgba(0, 128, 0, 0.5)");
    assert_eq!(c(51), "rgba(0, 128, 0, 0.2)");
    assert_eq!(c(0), "rgba(0, 128, 0, 0)");
    assert_eq!(c(255), "rgb(0, 128, 0)");
    assert_eq!(c(77), "rgba(0, 128, 0, 0.3)");
    // 1/255 = 0.0039 has no two-digit form (0.00 -> 0, 0.01 -> 3): three digits.
    assert_eq!(c(1), "rgba(0, 128, 0, 0.004)");
    assert_eq!(c(127), "rgba(0, 128, 0, 0.498)");
}

fn layout_tree(html: &str, css: &str) -> (lumen_dom::Document, LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let tree = crate::layout(&doc, &sheet, Size::new(1024.0, 600.0));
    (doc, tree)
}

#[test]
fn computed_transform_is_matrix_resolved_value() {
    let t = |v: Vec<TransformFn>| transform_list_to_css(&v);
    assert_eq!(t(vec![]), "none");
    assert_eq!(t(vec![TransformFn::TranslateX(10.0)]), "matrix(1, 0, 0, 1, 10, 0)");
    assert_eq!(
        t(vec![TransformFn::Scale(2.0, 2.0), TransformFn::Rotate(0.0)]),
        "matrix(2, 0, 0, 2, 0, 0)"
    );
    assert_eq!(
        t(vec![TransformFn::TranslateX(5.0), TransformFn::TranslateY(3.0)]),
        "matrix(1, 0, 0, 1, 5, 3)"
    );
    assert_eq!(
        t(vec![TransformFn::Translate3d(1.0, 2.0, 3.0)]),
        "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 3, 1)"
    );
}

#[test]
fn find_by_tag() {
    let (doc, tree) = layout_tree("<div>hello</div>", "");
    assert!(find_box_by_selector(&tree, &doc, "div").is_some());
}

#[test]
fn find_by_id() {
    let (doc, tree) = layout_tree(r#"<div id="main">text</div>"#, "");
    let found = find_box_by_selector(&tree, &doc, "#main");
    assert!(found.is_some());
}

#[test]
fn find_by_class() {
    let (doc, tree) = layout_tree(r#"<div class="container active">x</div>"#, "");
    assert!(find_box_by_selector(&tree, &doc, ".container").is_some());
    assert!(find_box_by_selector(&tree, &doc, ".active").is_some());
}

#[test]
fn find_miss_returns_none() {
    let (doc, tree) = layout_tree("<div>text</div>", "");
    assert!(find_box_by_selector(&tree, &doc, "#nonexistent").is_none());
}

/// BUG-506 residual: `writing-mode`/`direction`/`unicode-bidi`/
/// `text-orientation` were cascaded but never serialised, so
/// `getComputedStyle()` read them back as `""`.
#[test]
fn writing_mode_family_is_serialised() {
    let (doc, tree) = layout_tree(
        r#"<div id="d">x</div><div id="v">y</div>"#,
        "#v { writing-mode: vertical-rl; direction: rtl; unicode-bidi: isolate-override; \
             text-orientation: upright }",
    );
    let get = |sel: &str, prop: &str| {
        let b = find_box_by_selector(&tree, &doc, sel).expect("box");
        computed_style_to_map(&b.style).get(prop).cloned()
    };
    assert_eq!(get("#d", "writing-mode").as_deref(), Some("horizontal-tb"));
    assert_eq!(get("#d", "direction").as_deref(), Some("ltr"));
    assert_eq!(get("#d", "unicode-bidi").as_deref(), Some("normal"));
    assert_eq!(get("#d", "text-orientation").as_deref(), Some("mixed"));
    assert_eq!(get("#v", "writing-mode").as_deref(), Some("vertical-rl"));
    assert_eq!(get("#v", "direction").as_deref(), Some("rtl"));
    assert_eq!(get("#v", "unicode-bidi").as_deref(), Some("isolate-override"));
    assert_eq!(get("#v", "text-orientation").as_deref(), Some("upright"));
}

/// BUG-1435: `interpolate-size` is inherited and readable through `getComputedStyle()`.
#[test]
fn interpolate_size_is_serialised_and_inherited() {
    let (doc, tree) = layout_tree(
        r#"<div id="d">x</div><div id="v"><p id="c">y</p></div>"#,
        "#v { interpolate-size: allow-keywords }",
    );
    let get = |sel: &str| {
        let b = find_box_by_selector(&tree, &doc, sel).expect("box");
        computed_style_to_map(&b.style).get("interpolate-size").cloned()
    };
    assert_eq!(get("#d").as_deref(), Some("numeric-only"));
    assert_eq!(get("#v").as_deref(), Some("allow-keywords"));
    assert_eq!(get("#c").as_deref(), Some("allow-keywords"));
}

/// BUG-1325: the CSS Text longhands that `getComputedStyle()` used to read back as `""`.
#[test]
fn css_text_longhands_are_serialised() {
    let (doc, tree) = layout_tree(
        r#"<div id="d">x</div><div id="v"><p id="m">y</p><p id="r">z</p></div>"#,
        "#v { tab-size: 4; text-wrap: nowrap balance; line-break: strict; word-break: auto-phrase;                word-wrap: anywhere; hyphens: auto; text-align: center; text-align-last: match-parent; text-justify: distribute;                text-transform: full-width capitalize; white-space: preserve-breaks nowrap;                font-size: 20px; text-indent: each-line calc(10px + 0.5em) hanging }              #m { text-align: match-parent } #r { tab-size: 10px; letter-spacing: 0px }",
    );
    let get = |sel: &str, prop: &str| {
        let b = find_box_by_selector(&tree, &doc, sel).expect("box");
        computed_style_to_map(&b.style).get(prop).cloned()
    };
    assert_eq!(get("#d", "tab-size").as_deref(), Some("8"));
    assert_eq!(get("#d", "text-wrap").as_deref(), Some("wrap"));
    assert_eq!(get("#d", "white-space").as_deref(), Some("normal"));
    assert_eq!(get("#d", "letter-spacing").as_deref(), Some("normal"));
    assert_eq!(get("#v", "tab-size").as_deref(), Some("4"));
    assert_eq!(get("#v", "text-wrap").as_deref(), Some("nowrap balance"));
    assert_eq!(get("#v", "text-wrap-mode").as_deref(), Some("nowrap"));
    assert_eq!(get("#v", "text-wrap-style").as_deref(), Some("balance"));
    assert_eq!(get("#v", "line-break").as_deref(), Some("strict"));
    assert_eq!(get("#v", "word-break").as_deref(), Some("auto-phrase"));
    assert_eq!(get("#v", "overflow-wrap").as_deref(), Some("anywhere"));
    assert_eq!(get("#v", "word-wrap").as_deref(), Some("anywhere"));
    assert_eq!(get("#v", "hyphens").as_deref(), Some("auto"));
    assert_eq!(get("#v", "text-align-last").as_deref(), Some("match-parent"));
    assert_eq!(get("#v", "text-justify").as_deref(), Some("inter-character"));
    assert_eq!(get("#d", "text-justify").as_deref(), Some("auto"));
    assert_eq!(get("#v", "text-transform").as_deref(), Some("capitalize full-width"));
    assert_eq!(get("#v", "white-space").as_deref(), Some("preserve-breaks nowrap"));
    assert_eq!(get("#v", "text-indent").as_deref(), Some("20px hanging each-line"));
    // `match-parent` takes the parent's `center`; a length `tab-size` is an absolute length.
    assert_eq!(get("#m", "text-align").as_deref(), Some("center"));
    assert_eq!(get("#r", "tab-size").as_deref(), Some("10px"));
    assert_eq!(get("#r", "letter-spacing").as_deref(), Some("normal"));
}

/// BUG-1438: a percentage in `letter-spacing`/`word-spacing` stays a percentage in the
/// computed value, and a number-valued `calc()` in `tab-size` is evaluated.
#[test]
fn percent_spacing_and_calc_tab_size_are_computed() {
    let (doc, tree) = layout_tree(
        r#"<div id="c"><div id="a">x</div><div id="b">x</div><div id="d" style="letter-spacing: calc(10px - (5% + 10%)">x</div><div id="w" style="word-spacing: calc(10px - (5% + 10%)">x</div><div id="e">x</div><div id="f">x</div><div id="g">x</div><div id="h">x</div><div id="k">x</div></div>"#,
        "#c { container-type: inline-size; width: 100px } #c div { font-size: 40px }
             #a { letter-spacing: 110%; word-spacing: 110% }
             #b { letter-spacing: calc(10% - 20%); word-spacing: calc(10% - 20%) }
             #e { tab-size: calc(10 + (sign(2px - 10px) * 5)) }
             #f { tab-size: calc(10px + (sign(2px - 10px) * 5px)) }
             #g { letter-spacing: 10%; word-spacing: 0 }
             #h { letter-spacing: calc(10px - 0.5em) }
             #k { tab-size: calc(10px + 0.5em) }",
    );
    let get = |sel: &str, prop: &str| {
        let b = find_box_by_selector(&tree, &doc, sel).expect("box");
        computed_style_to_map(&b.style).get(prop).cloned()
    };
    assert_eq!(get("#a", "letter-spacing").as_deref(), Some("110%"));
    assert_eq!(get("#a", "word-spacing").as_deref(), Some("110%"));
    assert_eq!(get("#b", "letter-spacing").as_deref(), Some("-10%"));
    assert_eq!(get("#b", "word-spacing").as_deref(), Some("-10%"));
    assert_eq!(get("#d", "letter-spacing").as_deref(), Some("calc(-15% + 10px)"));
    assert_eq!(get("#w", "word-spacing").as_deref(), Some("calc(-15% + 10px)"));
    assert_eq!(get("#e", "tab-size").as_deref(), Some("5"));
    assert_eq!(get("#f", "tab-size").as_deref(), Some("5px"));
    assert_eq!(get("#h", "letter-spacing").as_deref(), Some("-10px"));
    assert_eq!(get("#k", "tab-size").as_deref(), Some("30px"));
    // The share is resolved against the element's own font-size in layout: 10% of 40px.
    let g = find_box_by_selector(&tree, &doc, "#g").expect("box");
    assert!((g.style.letter_spacing - 4.0).abs() < 0.01, "{}", g.style.letter_spacing);
}

/// BUG-1438: the `%` share of an inherited `letter-spacing` follows the heir's font-size.
#[test]
fn inherited_percent_letter_spacing_follows_font_size() {
    let (doc, tree) = layout_tree(
        r#"<div id="p"><div id="s">x</div></div>"#,
        "#p { font-size: 20px; letter-spacing: 10% } #s { font-size: 10px }",
    );
    let ls = |sel: &str| find_box_by_selector(&tree, &doc, sel).expect("box").style.letter_spacing;
    assert!((ls("#p") - 2.0).abs() < 0.01, "{}", ls("#p"));
    assert!((ls("#s") - 1.0).abs() < 0.01, "{}", ls("#s"));
}

/// BUG-1437: `initial`/`inherit`/`unset` reset the whole value of `tab-size`, `text-indent`
/// (with its `hanging`/`each-line`) and `hyphens` — a transition from them starts at the
/// right computed value.
#[test]
fn css_wide_keywords_reset_text_longhands() {
    let (doc, tree) = layout_tree(
        r#"<div id="p"><div id="i">a</div><div id="n">b</div><div id="u">c</div></div>"#,
        "#p { tab-size: 20; text-indent: 30px hanging each-line; hyphens: auto }              #i { tab-size: initial; text-indent: initial; hyphens: initial }              #n { tab-size: 3; text-indent: 5px hanging; hyphens: none }              #n { tab-size: inherit; text-indent: inherit; hyphens: inherit }              #u { tab-size: 3px; text-indent: 5px hanging; hyphens: none }              #u { tab-size: unset; text-indent: unset; hyphens: unset }",
    );
    let get = |sel: &str, prop: &str| {
        let b = find_box_by_selector(&tree, &doc, sel).expect("box");
        computed_style_to_map(&b.style).get(prop).cloned()
    };
    assert_eq!(get("#i", "tab-size").as_deref(), Some("8"));
    assert_eq!(get("#i", "text-indent").as_deref(), Some("0px"));
    assert_eq!(get("#i", "hyphens").as_deref(), Some("manual"));
    assert_eq!(get("#n", "tab-size").as_deref(), Some("20"));
    assert_eq!(get("#n", "text-indent").as_deref(), Some("30px hanging each-line"));
    assert_eq!(get("#n", "hyphens").as_deref(), Some("auto"));
    assert_eq!(get("#u", "tab-size").as_deref(), Some("20"));
    assert_eq!(get("#u", "text-indent").as_deref(), Some("30px hanging each-line"));
    assert_eq!(get("#u", "hyphens").as_deref(), Some("auto"));
}

/// CSS Text L3 §7.1: `start` inherited from an RTL parent becomes `right` under
/// `match-parent` (the root-element case is covered by WPT `text-align-match-parent-002`).
#[test]
fn text_align_match_parent_resolution() {
    let (doc, tree) = layout_tree(
        r#"<div id="p"><p id="c">x</p></div>"#,
        "#p { direction: rtl } #c { text-align: match-parent }",
    );
    let get = |sel: &str| {
        let b = find_box_by_selector(&tree, &doc, sel).expect("box");
        computed_style_to_map(&b.style).get("text-align").cloned()
    };
    assert_eq!(get("#c").as_deref(), Some("right"));
}

#[test]
fn find_first_dom_node_by_selector_finds_display_none() {
    let (doc, _tree) = layout_tree(r#"<div id="hidden" style="display:none">x</div>"#, "");
    // `find_box_by_selector` cannot see it — no layout box is built for
    // `display: none` — but the node is still in the DOM.
    assert!(find_box_by_selector(&_tree, &doc, "#hidden").is_none());
    assert!(find_first_dom_node_by_selector(&doc, "#hidden").is_some());
}

#[test]
fn find_first_dom_node_by_selector_miss_returns_none() {
    let (doc, _tree) = layout_tree("<div>text</div>", "");
    assert!(find_first_dom_node_by_selector(&doc, "#nonexistent").is_none());
}

#[test]
fn empty_selector_returns_none() {
    let (doc, tree) = layout_tree("<div>text</div>", "");
    assert!(find_box_by_selector(&tree, &doc, "").is_none());
}

// BUG-291: `query_all_scoped` must find matches inside a subtree that has
// no path to `doc.root()` at all — `query_all` (whole-document search)
// cannot, which is exactly what broke `Element.querySelector` on
// `testharness.js`'s off-document results table.
#[test]
fn query_all_scoped_finds_matches_in_detached_subtree() {
    let mut doc = lumen_dom::Document::new();
    let table = doc.create_element(lumen_dom::QualName::html("table"));
    let tbody = doc.create_element(lumen_dom::QualName::html("tbody"));
    doc.append_child(table, tbody);
    // `table` is intentionally never attached to `doc.root()`.

    assert_eq!(query_all_scoped(&doc, table, "tbody"), vec![tbody]);
    assert!(query_all(&doc, "tbody").is_empty());
}

// BUG-291: scoped queries match descendants only — never the scope node
// itself, and never nodes outside its subtree.
#[test]
fn query_all_scoped_excludes_self_and_siblings() {
    let mut doc = lumen_dom::Document::new();
    let root = doc.root();
    let a = doc.create_element(lumen_dom::QualName::html("div"));
    let b = doc.create_element(lumen_dom::QualName::html("div"));
    let c = doc.create_element(lumen_dom::QualName::html("div"));
    doc.append_child(root, a);
    doc.append_child(root, b);
    doc.append_child(a, c);

    // Only the descendant `c` matches — not `a` itself, not sibling `b`.
    assert_eq!(query_all_scoped(&doc, a, "div"), vec![c]);
}

// LAYOUT-1 срез 3: regression guards for the iterative `find_dom_rec`/
// `query_all_rec` — a single-child DOM chain deep enough that the old
// recursive walk would have overflowed a normal thread stack (BUG-987),
// same shape as `box_tree::tests::layout_box_drop`. Built directly via
// `Document`'s arena API (not `lumen_html_parser::parse`), since parsing
// a matching HTML string would additionally exercise the parser's own
// recursion, outside this test's scope.
//
// BUG-1060: depth was 200_000. `find_dom_rec`/`query_all_rec` themselves
// are already iterative (a heap `Vec` stack, not the call stack) and walk
// the chain in O(depth) either way — building the fixture is what
// exploded. Every `Document::append_child` call runs
// `debug_assert!(!self.is_self_or_ancestor(child, parent))`, which walks
// `parent`'s whole ancestor chain; appending one node at a time down a
// straight chain makes that O(depth) check run at every depth, so the
// fixture alone is O(depth²) — only in the debug/test profile, since
// `debug_assert!` compiles out of `dev-release`/`release`. At 200_000 that
// is tens of billions of ancestor-chain steps, multi-minute hangs under
// `cargo test`. 20_000 keeps a >20x margin over the deepest overflow the
// old recursive walk hit in BUG-987 (150-800 levels) while cutting the
// O(depth²) fixture cost ~100x — seconds instead of minutes.
const DEEP_CHAIN_DEPTH: usize = 20_000;

#[test]
fn find_first_dom_node_by_selector_deep_chain_does_not_overflow_the_stack() {
    let mut doc = lumen_dom::Document::new();
    let mut parent = doc.root();
    for _ in 0..DEEP_CHAIN_DEPTH {
        let div = doc.create_element(lumen_dom::QualName::html("div"));
        doc.append_child(parent, div);
        parent = div;
    }
    let leaf = doc.create_element(lumen_dom::QualName::html("span"));
    doc.append_child(parent, leaf);

    assert_eq!(find_first_dom_node_by_selector(&doc, "span"), Some(leaf));
    assert_eq!(find_first_dom_node_by_selector(&doc, "em"), None);
}

#[test]
fn query_all_deep_chain_does_not_overflow_the_stack() {
    let mut doc = lumen_dom::Document::new();
    let mut parent = doc.root();
    for _ in 0..DEEP_CHAIN_DEPTH {
        let div = doc.create_element(lumen_dom::QualName::html("div"));
        doc.append_child(parent, div);
        parent = div;
    }

    assert_eq!(query_all(&doc, "div").len(), DEEP_CHAIN_DEPTH);
}

#[test]
fn find_nested_block() {
    // Block-level elements get their own LayoutBox and are findable by selector.
    let (doc, tree) = layout_tree(
        r#"<div><div id="target">inner</div></div>"#,
        "",
    );
    assert!(find_box_by_selector(&tree, &doc, "#target").is_some());
}

#[test]
fn inline_elements_not_in_layout_tree() {
    // Inline elements (<span>, <a>, etc.) are merged into anonymous InlineRun
    // boxes in Phase 0 and do NOT get a dedicated LayoutBox. find_box_by_selector
    // returns None for them — this is a documented Phase 0 limitation.
    let (doc, tree) = layout_tree(
        r#"<div><span id="inline-target">text</span></div>"#,
        "",
    );
    assert!(find_box_by_selector(&tree, &doc, "#inline-target").is_none());
}

#[test]
fn comma_selector_matches_either() {
    let (doc, tree) = layout_tree(r#"<div id="foo">x</div>"#, "");
    assert!(find_box_by_selector(&tree, &doc, "#bar, #foo").is_some());
}

#[test]
fn computed_style_returns_snapshot() {
    let (doc, tree) = layout_tree(r#"<div id="x">text</div>"#, "");
    let snap = computed_style_by_selector(&tree, &doc, "#x");
    assert!(snap.is_some());
}

#[test]
fn computed_style_reflects_css() {
    let (doc, tree) = layout_tree(
        r#"<div id="box">text</div>"#,
        "#box { opacity: 0.5; }",
    );
    let snap = computed_style_by_selector(&tree, &doc, "#box").unwrap();
    assert!((snap.opacity - 0.5).abs() < 0.001);
}

#[test]
fn find_all_returns_multiple() {
    let (doc, tree) = layout_tree(
        "<div class=\"item\">a</div><div class=\"item\">b</div>",
        "",
    );
    let all = find_all_by_selector(&tree, &doc, ".item");
    assert_eq!(all.len(), 2);
}

#[test]
fn query_all_within_only_matches_descendants_of_start() {
    // BUG-298: `Element.querySelectorAll` must be scoped to the calling
    // node's subtree — `#outer` has one `.item` inside it and there's a
    // second `.item` sibling outside; `query_all_within` must find only
    // the inner one, unlike `query_all` (document-wide) which finds both.
    let (doc, tree) = layout_tree(
        r#"<div id="outer"><span class="item">a</span></div><span class="item">b</span>"#,
        "",
    );
    let outer = find_box_by_selector(&tree, &doc, "#outer").unwrap().node;
    let scoped = query_all_within(&doc, outer, ".item");
    assert_eq!(scoped.len(), 1);
    assert_eq!(query_all(&doc, ".item").len(), 2);
}

#[test]
fn query_all_within_excludes_start_itself() {
    let (doc, tree) = layout_tree(r#"<div id="outer" class="item"></div>"#, "");
    let outer = find_box_by_selector(&tree, &doc, "#outer").unwrap().node;
    assert!(query_all_within(&doc, outer, ".item").is_empty());
}

#[test]
fn compound_selector_tag_and_class() {
    let (doc, tree) = layout_tree(r#"<div class="hero">x</div>"#, "");
    assert!(find_box_by_selector(&tree, &doc, "div.hero").is_some());
    assert!(find_box_by_selector(&tree, &doc, "span.hero").is_none());
}

#[test]
fn descendant_combinator() {
    let (doc, tree) = layout_tree(
        r#"<section><p id="inner">text</p></section>"#,
        "",
    );
    assert!(find_box_by_selector(&tree, &doc, "section p").is_some());
    assert!(find_box_by_selector(&tree, &doc, "section #inner").is_some());
}

#[test]
fn find_all_empty_for_no_match() {
    let (doc, tree) = layout_tree("<p>text</p>", "");
    assert!(find_all_by_selector(&tree, &doc, "h1").is_empty());
}

#[test]
fn layout_box_method_find_descendant() {
    let (doc, tree) = layout_tree(
        r#"<div class="container"><p id="target">text</p></div>"#,
        "",
    );
    let found = tree.find_descendant_by_selector(&doc, "#target");
    assert!(found.is_some());
}

#[test]
fn layout_box_method_find_all_descendants() {
    let (doc, tree) = layout_tree(
        "<div><p class=\"item\">a</p><p class=\"item\">b</p></div>",
        "",
    );
    let all = tree.find_all_descendants_by_selector(&doc, ".item");
    assert_eq!(all.len(), 2);
}

#[test]
fn layout_box_method_style_snapshot() {
    let (doc, tree) = layout_tree(
        r#"<div id="box">text</div>"#,
        "#box { opacity: 0.5; }",
    );
    let found = tree.find_descendant_by_selector(&doc, "#box");
    assert!(found.is_some());
    let snap = found.unwrap().style_snapshot();
    assert!((snap.opacity - 0.5).abs() < 0.001);
}

// ──────────────── computed_style_json ────────────────

#[test]
fn json_is_well_formed_object() {
    let (doc, tree) = layout_tree(
        r#"<div id="x" style="font-size:20px;color:red"></div>"#,
        "",
    );
    let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
    assert!(json.starts_with('{') && json.ends_with('}'));
    // Contains a couple of known properties.
    assert!(json.contains(r#""font-size":"20px""#), "json: {json}");
    assert!(json.contains(r#""color":"rgb(255, 0, 0)""#), "json: {json}");
}

#[test]
fn json_keys_are_sorted() {
    let (doc, tree) = layout_tree(r#"<div id="x"></div>"#, "");
    let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
    // Keys are emitted in sorted order, so the byte offset of each successive
    // `"<key>":` marker must be strictly increasing. (Naive comma-splitting
    // would break on values like `rgb(255, 0, 0)`, so probe by marker.)
    let markers = [
        "\"align-items\":",
        "\"color\":",
        "\"display\":",
        "\"opacity\":",
        "\"width\":",
        "\"z-index\":",
    ];
    let mut last = 0usize;
    for m in markers {
        let pos = json.find(m).unwrap_or_else(|| panic!("missing key marker {m}"));
        assert!(pos >= last, "key {m} out of sorted order");
        last = pos;
    }
}

#[test]
fn json_missing_selector_returns_none() {
    let (doc, tree) = layout_tree(r#"<div></div>"#, "");
    assert!(computed_style_json_by_selector(&tree, &doc, "#nope").is_none());
}

#[test]
fn json_round_trips_via_string_parsing() {
    // The output must be parseable back into the same key/value map.
    let (doc, tree) = layout_tree(
        r#"<div id="x" style="display:flex;opacity:0.5"></div>"#,
        "",
    );
    let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
    assert!(json.contains(r#""display":"flex""#), "json: {json}");
    assert!(json.contains(r#""opacity":"0.5""#), "json: {json}");
    // No trailing comma / empty entries.
    assert!(!json.contains(",,"));
    assert!(!json.contains("{,") && !json.contains(",}"));
}

#[test]
fn json_escapes_font_family_quotes() {
    // A multi-word family name is quoted inside the value; the surrounding
    // JSON string must escape those inner quotes.
    let (doc, tree) = layout_tree(
        r#"<div id="x" style="font-family:Times New Roman"></div>"#,
        "",
    );
    let json = computed_style_json_by_selector(&tree, &doc, "#x").expect("box found");
    assert!(json.contains(r#"\"Times New Roman\""#), "json: {json}");
}

// ──────────────── matched_rules_for_node ────────────────

fn parse_doc_sheet(html: &str, css: &str) -> (lumen_dom::Document, lumen_css_parser::Stylesheet) {
    (lumen_html_parser::parse(html), lumen_css_parser::parse(css))
}

#[test]
fn matched_rules_empty_sheet_returns_empty() {
    let (doc, sheet) = parse_doc_sheet(r#"<div id="box">text</div>"#, "");
    let node = doc.find_by_id("box").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    assert!(rules.is_empty());
}

#[test]
fn matched_rules_tag_selector_matches() {
    let (doc, sheet) = parse_doc_sheet(r#"<div id="x">text</div>"#, "div { color: red; }");
    let node = doc.find_by_id("x").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].selector, "div");
    assert_eq!(rules[0].specificity, (0, 0, 1));
    assert!(rules[0].declarations.iter().any(|(p, v)| p == "color" && v == "red"));
}

#[test]
fn matched_rules_id_selector_matches() {
    let (doc, sheet) = parse_doc_sheet(
        r#"<div id="box">text</div>"#,
        "#box { opacity: 0.5; display: flex; }",
    );
    let node = doc.find_by_id("box").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].selector, "#box");
    assert_eq!(rules[0].specificity, (1, 0, 0));
    assert!(rules[0].declarations.iter().any(|(p, v)| p == "opacity" && v == "0.5"));
    assert!(rules[0].declarations.iter().any(|(p, v)| p == "display" && v == "flex"));
}

#[test]
fn matched_rules_class_selector_matches() {
    let (doc, sheet) = parse_doc_sheet(
        r#"<p id="intro-p" class="intro">text</p>"#,
        ".intro { font-size: 18px; }",
    );
    let node = doc.find_by_id("intro-p").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].selector, ".intro");
    assert_eq!(rules[0].specificity, (0, 1, 0));
}

#[test]
fn matched_rules_non_matching_selector_not_included() {
    let (doc, sheet) = parse_doc_sheet(
        r#"<div id="box">text</div>"#,
        "#other { color: red; }",
    );
    let node = doc.find_by_id("box").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    assert!(rules.is_empty());
}

#[test]
fn matched_rules_multiple_matching_rules() {
    let (doc, sheet) = parse_doc_sheet(
        r#"<div id="box" class="card">text</div>"#,
        "div { color: blue; } .card { margin: 10px; } #box { opacity: 1; }",
    );
    let node = doc.find_by_id("box").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    assert_eq!(rules.len(), 3);
    let selectors: Vec<&str> = rules.iter().map(|r| r.selector.as_str()).collect();
    assert!(selectors.contains(&"div"));
    assert!(selectors.contains(&".card"));
    assert!(selectors.contains(&"#box"));
}

#[test]
fn matched_rules_specificity_correct_for_tag_and_class() {
    let (doc, sheet) = parse_doc_sheet(
        r#"<div id="x" class="hero">text</div>"#,
        "div.hero { display: block; }",
    );
    let node = doc.find_by_id("x").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    assert_eq!(rules.len(), 1);
    // tag (0,0,1) + class (0,1,0) = (0,1,1)
    assert_eq!(rules[0].specificity, (0, 1, 1));
}

#[test]
fn matched_rules_rule_included_once_per_matching_selector() {
    // A rule with comma selectors — each matching selector yields one entry.
    let (doc, sheet) = parse_doc_sheet(
        r#"<div id="foo">text</div>"#,
        "#foo, #bar { color: green; }",
    );
    let node = doc.find_by_id("foo").expect("node exists");
    let rules = matched_rules_for_node(&doc, node, &sheet);
    // Only "#foo" matches; the rule appears once with the matching selector.
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].selector, "#foo");
}

// ── computed_style_to_map: BUG-388 additions ─────────────────────────────

/// `computed_style_to_map` for the first `<div>` box of the laid-out page.
fn div_computed_map(html: &str, css: &str) -> HashMap<String, String> {
    let (doc, tree) = layout_tree(html, css);
    let b = find_box_by_selector(&tree, &doc, "div").expect("div box exists");
    computed_style_to_map(&b.style)
}

#[test]
fn computed_map_carries_the_view_transition_name() {
    // VT-API: `getComputedStyle(el).viewTransitionName` was `""`.
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("view-transition-name").map(String::as_str), Some("none"));
    let m = div_computed_map("<div>x</div>", "div { view-transition-name: hero; }");
    assert_eq!(m.get("view-transition-name").map(String::as_str), Some("hero"));
}

#[test]
fn computed_map_scrollbar_color_defaults_to_auto() {
    // Before BUG-388 the key was absent entirely, so `getComputedStyle`
    // answered "" where every other engine answers "auto".
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("scrollbar-color").map(String::as_str), Some("auto"));
}

#[test]
fn computed_map_scrollbar_color_serialises_pair() {
    let m = div_computed_map("<div>x</div>", "div { scrollbar-color: lime red; }");
    assert_eq!(
        m.get("scrollbar-color").map(String::as_str),
        Some("rgb(0, 255, 0) rgb(255, 0, 0)")
    );
}

// ── computed_style_to_map: BUG-852 containment additions ─────────────────

#[test]
fn computed_map_containment_defaults() {
    // All five keys were absent before BUG-852, so `getComputedStyle`
    // answered `""` — a value neither property can ever take.
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("content-visibility").map(String::as_str), Some("visible"));
    assert_eq!(m.get("contain").map(String::as_str), Some("none"));
    assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("none"));
    assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("none"));
    assert_eq!(m.get("contain-intrinsic-height").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_content_visibility_auto() {
    let m = div_computed_map("<div>x</div>", "div { content-visibility: auto; }");
    assert_eq!(m.get("content-visibility").map(String::as_str), Some("auto"));
    let m = div_computed_map("<div>x</div>", "div { content-visibility: hidden; }");
    assert_eq!(m.get("content-visibility").map(String::as_str), Some("hidden"));
}

#[test]
fn computed_map_contain_collapses_full_sets_to_shorthand_keywords() {
    // CSS Containment L3 §3 (as serialised by Chrome, WPT `contain-computed`): the flag sets
    // `size layout style paint` / `layout style paint` serialize as `strict` / `content`;
    // every other combination lists its flags in canonical order (BUG-1401).
    let m = div_computed_map("<div>x</div>", "div { contain: layout paint; }");
    assert_eq!(m.get("contain").map(String::as_str), Some("layout paint"));
    let m = div_computed_map("<div>x</div>", "div { contain: content; }");
    assert_eq!(m.get("contain").map(String::as_str), Some("content"));
    let m = div_computed_map("<div>x</div>", "div { contain: strict; }");
    assert_eq!(m.get("contain").map(String::as_str), Some("strict"));
    let m = div_computed_map("<div>x</div>", "div { contain: paint style layout size; }");
    assert_eq!(m.get("contain").map(String::as_str), Some("strict"));
    let m = div_computed_map("<div>x</div>", "div { contain: size layout paint; }");
    assert_eq!(m.get("contain").map(String::as_str), Some("size layout paint"));
    let m = div_computed_map("<div>x</div>", "div { contain: inline-size layout style paint; }");
    assert_eq!(m.get("contain").map(String::as_str), Some("inline-size layout style paint"));
}

#[test]
fn computed_map_contain_invalid_value_is_dropped() {
    // CSS Containment L3 §3 grammar: `strict`/`content` stand alone, no repeats, and `size`
    // excludes `inline-size` — an invalid declaration leaves the initial value.
    for bad in ["strict layout", "layout layout", "size inline-size", "strict content", "auto", "none strict"] {
        let m = div_computed_map("<div>x</div>", &format!("div {{ contain: {bad}; }}"));
        assert_eq!(m.get("contain").map(String::as_str), Some("none"), "contain: {bad}");
    }
}

#[test]
fn computed_map_min_max_size_initial_and_calc() {
    let m = div_computed_map("<div>x</div>", "div { }");
    assert_eq!(m.get("min-width").map(String::as_str), Some("auto"));
    assert_eq!(m.get("min-height").map(String::as_str), Some("auto"));
    assert_eq!(m.get("max-width").map(String::as_str), Some("none"));
    let m = div_computed_map(
        "<div>x</div>",
        "div { font-size: 40px; min-width: calc(10px - 0.5em); max-width: calc(10px + 0.5em);              min-height: fit-content(calc(10px + 0.5em)); max-height: calc(10% + 40px); }",
    );
    assert_eq!(m.get("min-width").map(String::as_str), Some("0px"));
    assert_eq!(m.get("max-width").map(String::as_str), Some("30px"));
    assert_eq!(m.get("min-height").map(String::as_str), Some("fit-content(30px)"));
    assert_eq!(m.get("max-height").map(String::as_str), Some("calc(10% + 40px)"));
}

#[test]
fn computed_map_contain_intrinsic_size_keeps_auto_keyword() {
    // The `auto` keyword changes nothing in layout, which is why the
    // parser used to drop it — but it is part of the computed value, so
    // `auto 1px` must not read back as `1px`.
    let m = div_computed_map("<div>x</div>", "div { contain-intrinsic-size: auto 1px; }");
    assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("auto 1px"));
    assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("auto 1px"));
    assert_eq!(m.get("contain-intrinsic-height").map(String::as_str), Some("auto 1px"));
}

#[test]
fn computed_map_contain_intrinsic_size_two_components() {
    let m = div_computed_map("<div>x</div>", "div { contain-intrinsic-size: 30px 40px; }");
    assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("30px 40px"));
    assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("30px"));
    assert_eq!(m.get("contain-intrinsic-height").map(String::as_str), Some("40px"));
    // A longhand set on its own leaves the other axis at `none`.
    // (Relative units are not absolutised here — `length_to_css` reports
    // `5em` as `5em` for every length property in this map, not just this
    // one; that is a pre-existing property of the snapshot, so the case is
    // written in px rather than papering over it.)
    let m = div_computed_map("<div>x</div>", "div { contain-intrinsic-height: 80px; }");
    assert_eq!(m.get("contain-intrinsic-width").map(String::as_str), Some("none"));
    assert_eq!(m.get("contain-intrinsic-size").map(String::as_str), Some("none 80px"));
}

// ──────────────── CSSOM-3 срез 1: extended computed-style coverage ────────────────

#[test]
fn computed_map_box_shorthands_bug1050() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { margin: 1px 2px 3px 4px; padding: 5px 6px; scroll-margin: 7px;              scroll-padding: 8px 9px 10px; position: absolute; top: 1px; right: 2px; bottom: 1px; left: 2px;              text-decoration-thickness: 3px; text-underline-offset: 4px; line-height: 20px; }",
    );
    let g = |k: &str| m.get(k).map(String::as_str);
    assert_eq!(g("margin"), Some("1px 2px 3px 4px"));
    assert_eq!(g("padding"), Some("5px 6px"));
    assert_eq!(g("scroll-margin"), Some("7px"));
    assert_eq!(g("scroll-padding"), Some("8px 9px 10px"));
    assert_eq!(g("inset"), Some("1px 2px"));
    assert_eq!(g("text-decoration-thickness"), Some("3px"));
    assert_eq!(g("text-underline-offset"), Some("4px"));
    assert_eq!(g("line-height"), Some("20px"));
}

#[test]
fn computed_map_border_width_style_shorthands() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { border-width: 2px; border-style: dashed; }",
    );
    assert_eq!(m.get("border-width").map(String::as_str), Some("2px"));
    assert_eq!(m.get("border-style").map(String::as_str), Some("dashed"));

    // Differing per-side values → 4-value shorthand (BUG-1050).
    let m = div_computed_map(
        "<div>x</div>",
        "div { border-top-width: 1px; border-right-width: 2px; border-bottom-width: 3px; border-left-width: 4px; border-style: solid; }",
    );
    assert_eq!(m.get("border-width").map(String::as_str), Some("1px 2px 3px 4px"));
}

#[test]
fn computed_map_background_per_layer_longhands() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { background-attachment: fixed; background-origin: content-box; \
             background-clip: padding-box; background-repeat: repeat-x; \
             background-size: 50% 10px; background-blend-mode: multiply; }",
    );
    assert_eq!(m.get("background-attachment").map(String::as_str), Some("fixed"));
    assert_eq!(m.get("background-origin").map(String::as_str), Some("content-box"));
    assert_eq!(m.get("background-clip").map(String::as_str), Some("padding-box"));
    assert_eq!(m.get("background-repeat").map(String::as_str), Some("repeat-x"));
    assert_eq!(m.get("background-size").map(String::as_str), Some("50% 10px"));
    assert_eq!(m.get("background-blend-mode").map(String::as_str), Some("multiply"));
}

#[test]
fn computed_map_background_per_layer_longhands_default_without_layers() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("background-attachment").map(String::as_str), Some("scroll"));
    assert_eq!(m.get("background-origin").map(String::as_str), Some("padding-box"));
    assert_eq!(m.get("background-clip").map(String::as_str), Some("border-box"));
    assert_eq!(m.get("background-repeat").map(String::as_str), Some("repeat"));
    assert_eq!(m.get("background-size").map(String::as_str), Some("auto"));
    assert_eq!(m.get("background-blend-mode").map(String::as_str), Some("normal"));
}

#[test]
fn computed_map_box_shadow_and_text_shadow() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { box-shadow: 1px 2px 3px 4px red inset; text-shadow: 5px 6px 7px blue; }",
    );
    assert_eq!(
        m.get("box-shadow").map(String::as_str),
        Some("rgb(255, 0, 0) 1px 2px 3px 4px inset"),
    );
    assert_eq!(
        m.get("text-shadow").map(String::as_str),
        Some("rgb(0, 0, 255) 5px 6px 7px"),
    );

    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("box-shadow").map(String::as_str), Some("none"));
    assert_eq!(m.get("text-shadow").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_transition_and_animation_lists() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { transition-duration: 1s, 200ms; transition-timing-function: linear, ease; \
             animation-duration: 2s; animation-timing-function: steps(4, jump-end); \
             animation-iteration-count: infinite; animation-direction: alternate; \
             animation-fill-mode: both; animation-play-state: paused; }",
    );
    assert_eq!(m.get("transition-duration").map(String::as_str), Some("1s, 0.2s"));
    assert_eq!(
        m.get("transition-timing-function").map(String::as_str),
        Some("linear, cubic-bezier(0.25, 0.1, 0.25, 1)"),
    );
    assert_eq!(m.get("animation-duration").map(String::as_str), Some("2s"));
    assert_eq!(
        m.get("animation-timing-function").map(String::as_str),
        Some("steps(4, jump-end)"),
    );
    assert_eq!(m.get("animation-iteration-count").map(String::as_str), Some("infinite"));
    assert_eq!(m.get("animation-direction").map(String::as_str), Some("alternate"));
    assert_eq!(m.get("animation-fill-mode").map(String::as_str), Some("both"));
    assert_eq!(m.get("animation-play-state").map(String::as_str), Some("paused"));
}

#[test]
fn computed_map_animation_composition() {
    let m = div_computed_map("<div>x</div>", "div { animation-composition: add, accumulate; }");
    assert_eq!(m.get("animation-composition").map(String::as_str), Some("add, accumulate"));
    // Invalid list is dropped as a whole → initial value.
    let m = div_computed_map("<div>x</div>", "div { animation-composition: add, bogus; }");
    assert_eq!(m.get("animation-composition").map(String::as_str), Some("replace"));
    let m = div_computed_map("<div>x</div>", "div { }");
    assert_eq!(m.get("animation-composition").map(String::as_str), Some("replace"));
}

#[test]
fn computed_map_transition_defaults_without_declaration() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("transition-duration").map(String::as_str), Some("0s"));
    assert_eq!(m.get("transition-property").map(String::as_str), Some("all"));
    assert_eq!(m.get("animation-name").map(String::as_str), Some("none"));
    assert_eq!(m.get("animation-iteration-count").map(String::as_str), Some("1"));
    assert_eq!(m.get("animation-play-state").map(String::as_str), Some("running"));
}

#[test]
fn computed_map_overscroll_behavior() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { overscroll-behavior-x: contain; overscroll-behavior-y: none; }",
    );
    assert_eq!(m.get("overscroll-behavior-x").map(String::as_str), Some("contain"));
    assert_eq!(m.get("overscroll-behavior-y").map(String::as_str), Some("none"));
}

// BUG-516: `overscroll-behavior-block`/`overscroll-behavior-inline` (CSS
// Overscroll Behavior L1 §2) map to `overscroll-behavior-y`/`-x` under
// `horizontal-tb` and swap to `-x`/`-y` under a vertical writing mode,
// same shape as `overflow-block`/`-inline` (BUG-505).
#[test]
fn computed_map_overscroll_behavior_logical_horizontal_tb() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { overscroll-behavior-block: contain; overscroll-behavior-inline: none; }",
    );
    assert_eq!(m.get("overscroll-behavior-x").map(String::as_str), Some("none"));
    assert_eq!(m.get("overscroll-behavior-y").map(String::as_str), Some("contain"));
    assert_eq!(m.get("overscroll-behavior-block").map(String::as_str), Some("contain"));
    assert_eq!(m.get("overscroll-behavior-inline").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_overscroll_behavior_logical_vertical_rl_swaps_axes() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { writing-mode: vertical-rl; overscroll-behavior-block: contain; overscroll-behavior-inline: none; }",
    );
    assert_eq!(m.get("overscroll-behavior-x").map(String::as_str), Some("contain"));
    assert_eq!(m.get("overscroll-behavior-y").map(String::as_str), Some("none"));
    assert_eq!(m.get("overscroll-behavior-block").map(String::as_str), Some("contain"));
    assert_eq!(m.get("overscroll-behavior-inline").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_color_scheme_family() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { color-scheme: light dark; forced-color-adjust: none; print-color-adjust: exact; }",
    );
    assert_eq!(m.get("color-scheme").map(String::as_str), Some("light dark"));
    assert_eq!(m.get("forced-color-adjust").map(String::as_str), Some("none"));
    assert_eq!(m.get("print-color-adjust").map(String::as_str), Some("exact"));
}

#[test]
fn computed_map_color_keeps_lab_and_wide_gamut_spaces() {
    // BUG-1425: `color` не схлопывается в rgb() для lab()/oklch()/color()/color-mix().
    let color_of = |decl: &str| {
        let m = div_computed_map("<div>x</div>", &format!("div {{ color: {decl}; }}"));
        m.get("color").cloned().unwrap_or_default()
    };
    assert_eq!(color_of("lab(46 -50 50)"), "lab(46 -50 50)");
    assert_eq!(color_of("oklch(0.5 0.1 90)"), "oklch(0.5 0.1 90)");
    assert_eq!(color_of("color(srgb 0 0.5 0)"), "color(srgb 0 0.5 0)");
    assert_eq!(color_of("lab(20 0 10/50%)"), "lab(20 0 10 / 0.5)");
    assert_eq!(color_of("lab(none 0 0)"), "lab(none 0 0)");
    assert_eq!(color_of("color-mix(in srgb, red, blue)"), "color(srgb 0.5 0 0.5)");
    assert_eq!(color_of("rgb(from rebeccapurple r g b)"), "color(srgb 0.4 0.2 0.6)");
    assert_eq!(color_of("hwb(120 0% 50%)"), "rgb(0, 128, 0)");
    // Наследование и перекрытие обычным цветом.
    let m = div_computed_map(
        "<section><div>x</div></section>",
        "section { color: lab(46 -50 50); }",
    );
    assert_eq!(m.get("color").map(String::as_str), Some("lab(46 -50 50)"));
    assert_eq!(color_of("red"), "rgb(255, 0, 0)");
}

#[test]
fn computed_map_color_adjust_mirrors_print_color_adjust_alias() {
    let m = div_computed_map("<div>x</div>", "div { color-adjust: exact; }");
    assert_eq!(m.get("print-color-adjust").map(String::as_str), Some("exact"));
    assert_eq!(m.get("color-adjust").map(String::as_str), Some("exact"));
}

#[test]
fn computed_map_svg_paint_properties() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { fill: none; stroke: url(#grad); stroke-width: 3px; fill-rule: evenodd; \
             stroke-linecap: round; stroke-linejoin: bevel; stroke-dasharray: 1px 2px; }",
    );
    assert_eq!(m.get("fill").map(String::as_str), Some("none"));
    assert_eq!(m.get("stroke").map(String::as_str), Some("url(#grad)"));
    assert_eq!(m.get("stroke-width").map(String::as_str), Some("3px"));
    assert_eq!(m.get("fill-rule").map(String::as_str), Some("evenodd"));
    assert_eq!(m.get("stroke-linecap").map(String::as_str), Some("round"));
    assert_eq!(m.get("stroke-linejoin").map(String::as_str), Some("bevel"));
    assert_eq!(m.get("stroke-dasharray").map(String::as_str), Some("1px, 2px"));
}

#[test]
fn computed_map_content_and_quotes() {
    // `attr(data-x)` is deliberately not exercised here: `content`'s
    // `attr()` (a bare string substitution, CSS2.1 §12.2) shares a name
    // with the newer typed `attr()` substitution `cascade.rs` expands
    // pre-`apply_declaration` (CSS Values L4 §7.7) — that pre-pass drops
    // the whole declaration when the referenced attribute is absent,
    // which is a distinct, pre-existing gap from the map coverage this
    // test targets.
    let m = div_computed_map(
        "<div>x</div>",
        r#"div { content: "a" counter(x); quotes: "«" "»"; }"#,
    );
    assert_eq!(m.get("content").map(String::as_str), Some(r#""a" counter(x)"#));
    assert_eq!(m.get("quotes").map(String::as_str), Some(r#""«" "»""#));

    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("content").map(String::as_str), Some("normal"));
    assert_eq!(m.get("quotes").map(String::as_str), Some("auto"));
}

#[test]
fn computed_map_scrollbar_width_and_ruby() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { scrollbar-width: thin; ruby-position: under; ruby-align: center; ruby-merge: merge; }",
    );
    assert_eq!(m.get("scrollbar-width").map(String::as_str), Some("thin"));
    assert_eq!(m.get("ruby-position").map(String::as_str), Some("under"));
    assert_eq!(m.get("ruby-align").map(String::as_str), Some("center"));
    assert_eq!(m.get("ruby-merge").map(String::as_str), Some("merge"));
}

fn computed_of(css: &str, prop: &str) -> String {
    div_computed_map("<div>x</div>", css).get(prop).cloned().unwrap_or_else(|| format!("<missing {prop}>"))
}

#[test]
fn computed_map_font_settings_defaults_and_values() {
    assert_eq!(computed_of("", "font-feature-settings"), "normal");
    assert_eq!(computed_of("", "font-variation-settings"), "normal");
    assert_eq!(computed_of("", "font-optical-sizing"), "auto");
    assert_eq!(computed_of("", "font-palette"), "normal");
    assert_eq!(computed_of("", "font-size-adjust"), "none");
    // Дубли — побеждает последний, порядок — по тегу, `1` у feature опускается.
    assert_eq!(
        computed_of("div { font-feature-settings: \"dlig\", \"smcp\", \"dlig\" 0; }", "font-feature-settings"),
        "\"dlig\" 0, \"smcp\""
    );
    assert_eq!(computed_of("div { font-feature-settings: \"tnum\", \"hist\"; }", "font-feature-settings"), "\"hist\", \"tnum\"");
    assert_eq!(
        computed_of("div { font-variation-settings: \"wght\" 100, \"wdth\" 200, \"wght\" 300; }", "font-variation-settings"),
        "\"wdth\" 200, \"wght\" 300"
    );
    assert_eq!(computed_of("div { font-palette: --pitchfork; }", "font-palette"), "--pitchfork");
    assert_eq!(computed_of("div { font-optical-sizing: none; }", "font-optical-sizing"), "none");
    assert_eq!(computed_of("div { font-size-adjust: 0.5; }", "font-size-adjust"), "0.5");
}

#[test]
fn computed_map_font_shorthand() {
    assert_eq!(
        computed_of("div { font: italic 700 20px/1.5 Arial, sans-serif; }", "font"),
        "italic 700 20px/1.5 Arial, sans-serif"
    );
    // Лонгхенд, который шорткод не выражает, — пустая строка.
    assert_eq!(computed_of("div { font-feature-settings: \"liga\" 0; }", "font"), "");
}

#[test]
fn computed_map_clip_path_and_rule() {
    assert_eq!(computed_of("", "clip-path"), "none");
    assert_eq!(computed_of("", "clip-rule"), "nonzero");
    assert_eq!(computed_of("div { clip-rule: evenodd; }", "clip-rule"), "evenodd");
    assert_eq!(computed_of("div { clip-path: circle(10px at 50% 50%); }", "clip-path"), "circle(10px at 50% 50%)");
    assert_eq!(computed_of("div { clip-path: ellipse(at 50% 50%); }", "clip-path"), "ellipse(at 50% 50%)");
    assert_eq!(computed_of("div { clip-path: inset(5px 5px 5px 5px); }", "clip-path"), "inset(5px)");
    assert_eq!(computed_of("div { clip-path: inset(5px 10px round 4px); }", "clip-path"), "inset(5px 10px round 4px)");
    // `xywh()`/`rect()` сводятся к `inset()`; смешанные px и % — `calc()`.
    assert_eq!(
        computed_of("div { clip-path: xywh(10px 20px 50% 30px); }", "clip-path"),
        "inset(20px calc(50% - 10px) calc(100% - 50px) 10px)"
    );
    assert_eq!(computed_of("div { clip-path: rect(auto auto 50% 10%); }", "clip-path"), "inset(0px 0px 50% 10%)");
    assert_eq!(
        computed_of("div { clip-path: polygon(0 0, 100% 0, 50% 100%); }", "clip-path"),
        "polygon(0px 0px, 100% 0px, 50% 100%)"
    );
}

#[test]
fn computed_map_mask_longhands() {
    for (p, init) in [
        ("mask-image", "none"),
        ("mask-repeat", "repeat"),
        ("mask-size", "auto"),
        ("mask-origin", "border-box"),
        ("mask-clip", "border-box"),
        ("mask-composite", "add"),
        ("mask-mode", "match-source"),
        ("mask", "none"),
    ] {
        assert_eq!(computed_of("", p), init, "{p}");
    }
    let css = "div { mask-image: linear-gradient(red, blue), none; mask-repeat: no-repeat; \
                   mask-size: 10px 20px, cover; mask-composite: intersect; }";
    assert_eq!(computed_of(css, "mask-image"), "linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)), none");
    assert_eq!(computed_of(css, "mask-repeat"), "no-repeat, no-repeat");
    assert_eq!(computed_of(css, "mask-size"), "10px 20px, cover");
    assert_eq!(computed_of(css, "mask-composite"), "intersect, intersect");
}

#[test]
fn computed_map_font_variant_emoji() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("font-variant-emoji").map(String::as_str), Some("normal"));
    let m = div_computed_map("<div>x</div>", "div { font-variant-emoji: unicode; }");
    assert_eq!(m.get("font-variant-emoji").map(String::as_str), Some("unicode"));
}

#[test]
fn computed_map_font_variant_new_longhands() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("font-variant-ligatures").map(String::as_str), Some("normal"));
    assert_eq!(m.get("font-variant-numeric").map(String::as_str), Some("normal"));
    assert_eq!(m.get("font-variant-position").map(String::as_str), Some("normal"));
    let m = div_computed_map(
        "<div>x</div>",
        "div { font-variant-ligatures: none; font-variant-numeric: slashed-zero tabular-nums; \
             font-variant-position: sub; }",
    );
    assert_eq!(m.get("font-variant-ligatures").map(String::as_str), Some("none"));
    assert_eq!(
        m.get("font-variant-numeric").map(String::as_str),
        Some("tabular-nums slashed-zero")
    );
    assert_eq!(m.get("font-variant-position").map(String::as_str), Some("sub"));
    let m = div_computed_map(
        "<div>x</div>",
        "div { font-variant: no-contextual small-caps tabular-nums; }",
    );
    assert_eq!(
        m.get("font-variant").map(String::as_str),
        Some("no-contextual small-caps tabular-nums")
    );
}

#[test]
fn computed_map_font_variant_shorthand_joins_implemented_components() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("font-variant").map(String::as_str), Some("normal"));
    let m = div_computed_map("<div>x</div>", "div { font-variant: small-caps unicode; }");
    assert_eq!(m.get("font-variant").map(String::as_str), Some("small-caps unicode"));
    let m = div_computed_map("<div>x</div>", "div { font-variant-emoji: text; }");
    assert_eq!(m.get("font-variant").map(String::as_str), Some("text"));
}

// ── computed_style_to_map: BUG-603 additions ─────────────────────────────

#[test]
fn computed_map_background_image_defaults_to_none() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("background-image").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_background_image_url() {
    let m = div_computed_map("<div>x</div>", "div { background-image: url(a.png); }");
    assert_eq!(m.get("background-image").map(String::as_str), Some("url(\"a.png\")"));
}

#[test]
fn computed_map_background_image_functions() {
    // BUG-1428: градиент / image-set() / cross-fade() читаются целиком, а не как `none`.
    let css = |decl: &str| {
        let m = div_computed_map("<div>x</div>", &format!("div {{ {decl} }}"));
        m.get("background-image").cloned().unwrap_or_default()
    };
    assert_eq!(
        css("background-image: linear-gradient(90deg in oklab, red, blue)"),
        "linear-gradient(90deg in oklab, rgb(255, 0, 0), rgb(0, 0, 255))"
    );
    assert_eq!(
        css("background: url(a.png), radial-gradient(at 10% 20%, red, blue)"),
        "url(\"a.png\"), radial-gradient(at 10% 20%, rgb(255, 0, 0), rgb(0, 0, 255))"
    );
    assert_eq!(
        css("background-image: image-set(url(a.png) 1x)"),
        "image-set(url(\"a.png\") 1dppx)"
    );
    assert_eq!(
        css("background-image: -webkit-image-set(url(a.png) 2x)"),
        "image-set(url(\"a.png\") 2dppx)"
    );
    assert_eq!(
        css("background-image: cross-fade(50% red, 50% green, 50% blue)"),
        "cross-fade(rgb(255, 0, 0) 50%, rgb(0, 128, 0) 50%, rgb(0, 0, 255) 50%)"
    );
    assert_eq!(css("background-image: conic-gradient(red, blue)"), "conic-gradient(rgb(255, 0, 0), rgb(0, 0, 255))");
}

// ── computed_style_to_map: BUG-495 additions ─────────────────────────────

#[test]
fn computed_map_background_position_xy_default_top_left() {
    // No layer at all — falls back to the `background-position` initial
    // value (`0% 0%`), same as an explicit single default layer would.
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("background-position-x").map(String::as_str), Some("0%"));
    assert_eq!(m.get("background-position-y").map(String::as_str), Some("0%"));
}

#[test]
fn computed_map_background_position_x_percent_and_px() {
    let m = div_computed_map("<div>x</div>", "div { background-position-x: 25%; }");
    assert_eq!(m.get("background-position-x").map(String::as_str), Some("25%"));
    let m = div_computed_map("<div>x</div>", "div { background-position-y: 10px; }");
    assert_eq!(m.get("background-position-y").map(String::as_str), Some("10px"));
}

#[test]
fn computed_map_background_position_x_multi_layer_joins_commas() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { background-image: url(a.png), url(b.png); background-position-x: 10%, 90%; }",
    );
    assert_eq!(m.get("background-position-x").map(String::as_str), Some("10%, 90%"));
}

#[test]
fn computed_map_background_position_x_edge_offset_serialization() {
    // WPT `background-position-x-computed.html`: `left -20%` → `-20%`,
    // `right -10px` → `calc(100% + 10px)`, `right 10px` → `calc(100% - 10px)`,
    // `x-start`/`x-end` → `0%`/`100%`.
    let m = div_computed_map(
        "<div>x</div>",
        "div { background-image: url(a.png), url(b.png), url(c.png);                    background-position-x: calc(10px - 0.5em), left -20%, right 10px; }",
    );
    assert_eq!(
        m.get("background-position-x").map(String::as_str),
        Some("2px, -20%, calc(100% - 10px)")
    );
    let m = div_computed_map("<div>x</div>", "div { background-position-x: right -10px; }");
    assert_eq!(m.get("background-position-x").map(String::as_str), Some("calc(100% + 10px)"));
    let m = div_computed_map(
        "<div>x</div>",
        "div { background-image: url(a.png), url(b.png); background-position-x: x-start, x-end; }",
    );
    assert_eq!(m.get("background-position-x").map(String::as_str), Some("0%, 100%"));
}

#[test]
fn computed_map_border_color_uniform_sides() {
    let m = div_computed_map("<div>x</div>", "div { border-color: red; }");
    assert_eq!(m.get("border-color").map(String::as_str), Some("rgb(255, 0, 0)"));
}

#[test]
fn computed_map_border_color_shortest_form_when_sides_differ() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { border-top-color: red; border-right-color: blue; \
             border-bottom-color: red; border-left-color: red; }",
    );
    assert_eq!(
        m.get("border-color").map(String::as_str),
        Some("rgb(255, 0, 0) rgb(0, 0, 255) rgb(255, 0, 0) rgb(255, 0, 0)")
    );
    let m = div_computed_map("<div>x</div>", "div { color: lime; border-color: red currentcolor; }");
    assert_eq!(m.get("border-color").map(String::as_str), Some("rgb(255, 0, 0) rgb(0, 255, 0)"));
}

/// BUG-1297: the computed lists keep the author's length, independent of the layer count.
#[test]
fn computed_map_background_lists_keep_specified_length() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { background-image: none, none; background-repeat: repeat-x; \
             background-clip: border-box, padding-box, content-box; background-size: 100%; \
             background-position: center, left; }",
    );
    assert_eq!(m.get("background-repeat").map(String::as_str), Some("repeat-x"));
    assert_eq!(
        m.get("background-clip").map(String::as_str),
        Some("border-box, padding-box, content-box")
    );
    assert_eq!(m.get("background-size").map(String::as_str), Some("100% auto"));
    assert_eq!(m.get("background-position").map(String::as_str), Some("50% 50%, 0% 50%"));
}

#[test]
fn computed_map_background_invalid_keyword_list_is_dropped() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { background-attachment: fixed; background-attachment: local, none; }",
    );
    assert_eq!(m.get("background-attachment").map(String::as_str), Some("fixed"));
}

/// BUG-1297: `currentcolor` of `box-shadow` serialises as the used colour; `border-radius`
/// composes the four circular radii.
#[test]
fn computed_map_box_shadow_color_and_border_radius() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { color: blue; box-shadow: 1px 2px; border-radius: 1px 2px 3px 2px; }",
    );
    assert_eq!(m.get("box-shadow").map(String::as_str), Some("rgb(0, 0, 255) 1px 2px 0px 0px"));
    assert_eq!(m.get("border-radius").map(String::as_str), Some("1px 2px 3px"));
}

/// BUG-1297: a shorthand whose values are functions (`rgb(…, …)`) splits at top-level
/// whitespace only; `hidden` is a style of its own; elliptical radii keep both axes.
#[test]
fn computed_map_border_shorthands_with_functions_and_ellipses() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { border-color: rgb(255, 0, 0) rgb(0, 0, 255); border-style: hidden dotted;              border-radius: calc(0.5em + 4px) 10% / 3px; border-bottom-left-radius: 5px 7px;              border-top-width: calc(-1px); }",
    );
    assert_eq!(m.get("border-top-color").map(String::as_str), Some("rgb(255, 0, 0)"));
    assert_eq!(m.get("border-right-color").map(String::as_str), Some("rgb(0, 0, 255)"));
    assert_eq!(m.get("border-style").map(String::as_str), Some("hidden dotted"));
    assert_eq!(m.get("border-top-width").map(String::as_str), Some("0px"));
    assert_eq!(m.get("border-top-left-radius").map(String::as_str), Some("12px 3px"));
    assert_eq!(m.get("border-top-right-radius").map(String::as_str), Some("10% 3px"));
    assert_eq!(m.get("border-bottom-left-radius").map(String::as_str), Some("5px 7px"));
}

#[test]
fn computed_map_border_spacing_single_value_when_equal() {
    let m = div_computed_map("<div>x</div>", "div { border-spacing: 10px; }");
    assert_eq!(m.get("border-spacing").map(String::as_str), Some("10px"));
}

#[test]
fn computed_map_border_spacing_two_values_when_differ() {
    let m = div_computed_map("<div>x</div>", "div { border-spacing: 10px 20px; }");
    assert_eq!(m.get("border-spacing").map(String::as_str), Some("10px 20px"));
}

// ──────────────── CSSOM-3 срез 2: CSS Logical Properties L1 ────────────────

#[test]
fn computed_map_logical_sizing_mirrors_physical() {
    let m = div_computed_map("<div>x</div>", "div { inline-size: 120px; block-size: 80px; }");
    assert_eq!(m.get("inline-size").map(String::as_str), Some("120px"));
    assert_eq!(m.get("block-size").map(String::as_str), Some("80px"));
    assert_eq!(m.get("width").map(String::as_str), Some("120px"));
    assert_eq!(m.get("height").map(String::as_str), Some("80px"));
}

#[test]
fn computed_map_logical_margin_padding_start_end_mirror_physical() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { margin-inline-start: 5px; margin-inline-end: 6px; \
             padding-block-start: 7px; padding-block-end: 8px; }",
    );
    assert_eq!(m.get("margin-inline-start").map(String::as_str), Some("5px"));
    assert_eq!(m.get("margin-left").map(String::as_str), Some("5px"));
    assert_eq!(m.get("margin-inline-end").map(String::as_str), Some("6px"));
    assert_eq!(m.get("margin-right").map(String::as_str), Some("6px"));
    assert_eq!(m.get("padding-block-start").map(String::as_str), Some("7px"));
    assert_eq!(m.get("padding-top").map(String::as_str), Some("7px"));
    assert_eq!(m.get("padding-block-end").map(String::as_str), Some("8px"));
    assert_eq!(m.get("padding-bottom").map(String::as_str), Some("8px"));
}

#[test]
fn computed_map_logical_inset_mirrors_physical() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { position: absolute; inset-inline-start: 1px; inset-inline-end: 2px; \
             inset-block-start: 3px; inset-block-end: 4px; }",
    );
    assert_eq!(m.get("inset-inline-start").map(String::as_str), Some("1px"));
    assert_eq!(m.get("left").map(String::as_str), Some("1px"));
    assert_eq!(m.get("inset-inline-end").map(String::as_str), Some("2px"));
    assert_eq!(m.get("right").map(String::as_str), Some("2px"));
    assert_eq!(m.get("inset-block-start").map(String::as_str), Some("3px"));
    assert_eq!(m.get("top").map(String::as_str), Some("3px"));
    assert_eq!(m.get("inset-block-end").map(String::as_str), Some("4px"));
    assert_eq!(m.get("bottom").map(String::as_str), Some("4px"));
}

#[test]
fn computed_map_logical_border_width_style_color_mirror_physical() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { border-inline-start-width: 2px; border-inline-start-style: dashed; \
             border-inline-start-color: red; }",
    );
    assert_eq!(m.get("border-inline-start-width").map(String::as_str), Some("2px"));
    assert_eq!(m.get("border-left-width").map(String::as_str), Some("2px"));
    assert_eq!(m.get("border-inline-start-style").map(String::as_str), Some("dashed"));
    assert_eq!(m.get("border-left-style").map(String::as_str), Some("dashed"));
    assert_eq!(m.get("border-inline-start-color").map(String::as_str), Some("rgb(255, 0, 0)"));
    assert_eq!(m.get("border-left-color").map(String::as_str), Some("rgb(255, 0, 0)"));
}

#[test]
fn computed_map_logical_two_value_shorthand_collapses_when_equal() {
    let m = div_computed_map("<div>x</div>", "div { margin-inline: 10px; }");
    assert_eq!(m.get("margin-inline").map(String::as_str), Some("10px"));
}

#[test]
fn computed_map_logical_two_value_shorthand_keeps_both_when_differ() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { margin-inline-start: 10px; margin-inline-end: 20px; }",
    );
    assert_eq!(m.get("margin-inline").map(String::as_str), Some("10px 20px"));
}

#[test]
fn computed_map_logical_properties_absent_outside_horizontal_tb_ltr() {
    // Margin/padding/inset/border logical longhands are still only
    // resolved onto physical fields for horizontal-tb/LTR — the CSSOM
    // mirror gates the same way rather than serve a made-up value.
    let m = div_computed_map(
        "<div>x</div>",
        "div { writing-mode: vertical-rl; margin-inline-start: 12px; }",
    );
    assert_eq!(m.get("margin-inline-start"), None);
}

#[test]
fn computed_map_logical_sizing_swaps_in_vertical_writing_mode() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { writing-mode: vertical-rl; inline-size: 120px; block-size: 80px; }",
    );
    assert_eq!(m.get("inline-size").map(String::as_str), Some("120px"));
    assert_eq!(m.get("block-size").map(String::as_str), Some("80px"));
    assert_eq!(m.get("height").map(String::as_str), Some("120px"));
    assert_eq!(m.get("width").map(String::as_str), Some("80px"));
}

// ── computed_style_to_map: CSSOM-3 slice 3 (BUG-537 family) ──────────────

#[test]
fn computed_map_object_fit_and_position_defaults() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("object-fit").map(String::as_str), Some("fill"));
    assert_eq!(m.get("object-position").map(String::as_str), Some("50% 50%"));
    assert_eq!(m.get("image-rendering").map(String::as_str), Some("auto"));
}

#[test]
fn computed_map_object_fit_and_position_set() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { object-fit: cover; object-position: left 20px; image-rendering: pixelated; }",
    );
    assert_eq!(m.get("object-fit").map(String::as_str), Some("cover"));
    assert_eq!(m.get("object-position").map(String::as_str), Some("0% 20px"));
    assert_eq!(m.get("image-rendering").map(String::as_str), Some("pixelated"));
}

#[test]
fn computed_map_aspect_ratio_default_and_set() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("aspect-ratio").map(String::as_str), Some("auto"));
    let m = div_computed_map("<div>x</div>", "div { aspect-ratio: 16 / 9; }");
    assert_eq!(m.get("aspect-ratio").map(String::as_str), Some("auto 16 / 9"));
    let m = div_computed_map("<div>x</div>", "div { aspect-ratio: 1.5; }");
    assert_eq!(m.get("aspect-ratio").map(String::as_str), Some("auto 1.5 / 1"));
}

#[test]
fn computed_map_vertical_align_keyword_default_and_set() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("vertical-align").map(String::as_str), Some("baseline"));
    let m = div_computed_map("<div>x</div>", "div { vertical-align: text-top; }");
    assert_eq!(m.get("vertical-align").map(String::as_str), Some("text-top"));
}

#[test]
fn computed_map_vertical_align_percent_and_length() {
    let m = div_computed_map("<div>x</div>", "div { vertical-align: 25%; }");
    assert_eq!(m.get("vertical-align").map(String::as_str), Some("25%"));
    let m = div_computed_map("<div>x</div>", "div { vertical-align: 4px; }");
    assert_eq!(m.get("vertical-align").map(String::as_str), Some("4px"));
}

// ── computed_style_to_map: BUG-505 additions ─────────────────────────────

#[test]
fn computed_map_line_clamp_defaults_to_none() {
    // Both names were absent entirely before BUG-505, so `getComputedStyle`
    // answered "" instead of the property's own initial value.
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("line-clamp").map(String::as_str), Some("none"));
    assert_eq!(m.get("-webkit-line-clamp").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_line_clamp_reports_integer() {
    let m = div_computed_map("<div>x</div>", "div { -webkit-line-clamp: 3; }");
    assert_eq!(m.get("line-clamp").map(String::as_str), Some("3"));
    assert_eq!(m.get("-webkit-line-clamp").map(String::as_str), Some("3"));
}

#[test]
fn computed_map_webkit_box_orient_and_continue_default() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("-webkit-box-orient").map(String::as_str), Some("horizontal"));
    assert_eq!(m.get("continue").map(String::as_str), Some("normal"));
}

#[test]
fn computed_map_webkit_box_orient_and_continue_set() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { -webkit-box-orient: vertical; continue: discard; }",
    );
    assert_eq!(m.get("-webkit-box-orient").map(String::as_str), Some("vertical"));
    assert_eq!(m.get("continue").map(String::as_str), Some("discard"));
}

// ── computed_style_to_map: BUG-505 срез 5, `display: -webkit-box` quirk
// (CSS Overflow L4 §continue / WHATWG Compat §2.1) — matrix transcribed
// from `css/css-overflow/parsing/webkit-box-computed.html`.

#[test]
fn webkit_box_display_computes_as_specified_without_orient_or_clamp() {
    let m = div_computed_map("<div>x</div>", "div { display: -webkit-box; }");
    assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
}

#[test]
fn webkit_box_display_unaffected_by_orient_alone() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; -webkit-box-orient: vertical; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
}

#[test]
fn webkit_box_display_unaffected_by_vertical_orient_plus_clamp_none() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: none; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
}

#[test]
fn webkit_box_display_unaffected_by_clamp_without_vertical_orient() {
    // Default box-orient is horizontal — clamp alone doesn't trigger the quirk.
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; -webkit-line-clamp: 3; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
}

#[test]
fn webkit_box_display_unaffected_by_explicit_horizontal_orient_plus_clamp() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; -webkit-box-orient: horizontal; -webkit-line-clamp: 3; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
}

#[test]
fn webkit_box_display_becomes_flow_root_with_vertical_orient_and_clamp() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("flow-root"));
}

#[test]
fn webkit_inline_box_display_becomes_inline_block_with_vertical_orient_and_clamp() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-inline-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("inline-block"));
}

#[test]
fn webkit_box_display_becomes_flow_root_with_continue_discard() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; -webkit-box-orient: vertical; continue: discard; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("flow-root"));
}

#[test]
fn webkit_box_display_unaffected_by_continue_discard_without_vertical_orient() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; continue: discard; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
}

#[test]
fn webkit_box_display_unaffected_by_continue_none() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-box; -webkit-box-orient: vertical; continue: none; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("-webkit-box"));
}

#[test]
fn webkit_flex_alias_ignores_the_webkit_box_quirk() {
    // WPT explicitly asserts `-webkit-flex`/`flex`/`inline-flex` do NOT
    // get the quirk even under the identical orient+clamp combination
    // that turns `-webkit-box` into `flow-root` — they alias straight
    // to `Display::Flex` at parse time and never become `WebkitBox`.
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: flex; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("flex"));
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-flex; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("flex"));
    let m = div_computed_map(
        "<div>x</div>",
        "div { display: -webkit-inline-flex; -webkit-box-orient: vertical; -webkit-line-clamp: 3; }",
    );
    assert_eq!(m.get("display").map(String::as_str), Some("inline-flex"));
}

#[test]
fn multi_word_display_serialises_canonically() {
    // BUG-1476: CSS Display 3 §2 — двухсловные формы и слова, которых нет в `Display`.
    for (decl, want) in [
        ("inline flow-root", "inline-block"),
        ("block flow-root", "flow-root"),
        ("list-item block", "list-item"),
        ("inline list-item", "inline list-item"),
        ("list-item flow-root inline", "inline flow-root list-item"),
        ("inline grid", "inline-grid"),
        ("block flex", "flex"),
        ("block ruby", "block ruby"),
        ("grid-lanes", "grid-lanes"),
    ] {
        let (doc, tree) = layout_tree("<div>x</div>", &format!("div {{ display: {decl}; }}"));
        let b = find_box_by_selector(&tree, &doc, "div").unwrap_or_else(|| panic!("display: {decl}: no div box"));
        let m = computed_style_to_map(&b.style);
        assert_eq!(m.get("display").map(String::as_str), Some(want), "display: {decl}");
    }
}

#[test]
fn computed_map_scrollbar_gutter_defaults_to_auto() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("scrollbar-gutter").map(String::as_str), Some("auto"));
}

#[test]
fn computed_map_scrollbar_gutter_reports_stable_both_edges() {
    let m = div_computed_map("<div>x</div>", "div { scrollbar-gutter: stable both-edges; }");
    assert_eq!(m.get("scrollbar-gutter").map(String::as_str), Some("stable both-edges"));
    // CSS Overflow L4 §3.3 double-bar grammar: token order doesn't matter.
    let m = div_computed_map("<div>x</div>", "div { scrollbar-gutter: both-edges stable; }");
    assert_eq!(m.get("scrollbar-gutter").map(String::as_str), Some("stable both-edges"));
}

#[test]
fn computed_map_overflow_anchor_defaults_to_auto() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("overflow-anchor").map(String::as_str), Some("auto"));
}

#[test]
fn computed_map_overflow_anchor_reports_none() {
    let m = div_computed_map("<div>x</div>", "div { overflow-anchor: none; }");
    assert_eq!(m.get("overflow-anchor").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_overflow_clip_margin_defaults_to_zero() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("0px"));
}

#[test]
fn computed_map_overflow_clip_margin_reports_length() {
    let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: 10px; }");
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("10px"));
}

// BUG-505 срез 6 — CSS Overflow L5 `scroll-marker-group`/`scroll-target-group`.
#[test]
fn computed_map_scroll_marker_group_defaults_to_none() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("scroll-marker-group").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_scroll_marker_group_reports_placement_and_mode() {
    let m = div_computed_map("<div>x</div>", "div { scroll-marker-group: before; }");
    assert_eq!(m.get("scroll-marker-group").map(String::as_str), Some("before"));
    let m = div_computed_map("<div>x</div>", "div { scroll-marker-group: after tabs; }");
    assert_eq!(m.get("scroll-marker-group").map(String::as_str), Some("after tabs"));
}

#[test]
fn computed_map_scroll_marker_group_rejects_invalid_value() {
    // Invalid values never reach the cascade (CSSOM validation drops the
    // declaration before it gets here) — this exercises the Rust-side
    // parser's own rejection independently of the JS shim gate.
    let m = div_computed_map("<div>x</div>", "div { scroll-marker-group: before before; }");
    assert_eq!(m.get("scroll-marker-group").map(String::as_str), Some("none"));
}

// GAP-INTERESTINVOKER — `interest-delay-start`/`-end` и шортхенд.
#[test]
fn computed_map_interest_delay_defaults_to_normal() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("interest-delay-start").map(String::as_str), Some("normal"));
    assert_eq!(m.get("interest-delay-end").map(String::as_str), Some("normal"));
}

#[test]
fn computed_map_interest_delay_reports_seconds() {
    let m = div_computed_map("<div>x</div>", "div { interest-delay-start: 123ms; interest-delay-end: 32s; }");
    assert_eq!(m.get("interest-delay-start").map(String::as_str), Some("0.123s"));
    assert_eq!(m.get("interest-delay-end").map(String::as_str), Some("32s"));
}

#[test]
fn computed_map_interest_delay_shorthand_fills_both() {
    let m = div_computed_map("<div>x</div>", "div { interest-delay: 0s; }");
    assert_eq!(m.get("interest-delay-start").map(String::as_str), Some("0s"));
    assert_eq!(m.get("interest-delay-end").map(String::as_str), Some("0s"));
    let m = div_computed_map("<div>x</div>", "div { interest-delay: normal 450ms; }");
    assert_eq!(m.get("interest-delay-start").map(String::as_str), Some("normal"));
    assert_eq!(m.get("interest-delay-end").map(String::as_str), Some("0.45s"));
}

#[test]
fn computed_map_interest_delay_rejects_invalid_and_does_not_inherit() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { interest-delay-start: 1s; interest-delay-start: -1s; interest-delay-end: 0; }",
    );
    assert_eq!(m.get("interest-delay-start").map(String::as_str), Some("1s"));
    assert_eq!(m.get("interest-delay-end").map(String::as_str), Some("normal"));
    let m = div_computed_map("<div>x</div>", "body { interest-delay: 2s; }");
    assert_eq!(m.get("interest-delay-start").map(String::as_str), Some("normal"));
}

#[test]
fn computed_map_scroll_target_group_defaults_to_none() {
    let m = div_computed_map("<div>x</div>", "");
    assert_eq!(m.get("scroll-target-group").map(String::as_str), Some("none"));
}

#[test]
fn computed_map_scroll_target_group_reports_auto() {
    let m = div_computed_map("<div>x</div>", "div { scroll-target-group: auto; }");
    assert_eq!(m.get("scroll-target-group").map(String::as_str), Some("auto"));
}

// BUG-505 срез 4: `overflow-clip-margin`'s `<visual-box>` component,
// confirmed against WPT `overflow-clip-margin-computed.html`.

#[test]
fn computed_map_overflow_clip_margin_box_alone_elides_zero_length() {
    let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: content-box; }");
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("content-box"));
    let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: border-box 0px; }");
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("border-box"));
}

#[test]
fn computed_map_overflow_clip_margin_box_and_length_reorder_canonical() {
    let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: 10px content-box; }");
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("content-box 10px"));
}

#[test]
fn computed_map_overflow_clip_margin_padding_box_elides_keyword() {
    let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: padding-box 10px; }");
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("10px"));
}

#[test]
fn computed_map_overflow_clip_margin_resolves_calc_of_absolute_units() {
    // `calc(100px - 50px)` — both operands absolute, resolves to a
    // plain px number at computed-value time (unlike the specified
    // value, which keeps the `calc(...)` text).
    let m = div_computed_map("<div>x</div>", "div { overflow-clip-margin: calc(100px - 50px); }");
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("50px"));
}

#[test]
fn computed_map_overflow_clip_margin_resolves_calc_with_em() {
    // Default computed font-size is 16px, so `0.5em` resolves to 8px —
    // WPT `overflow-clip-margin-computed.html`'s own case (108px at its
    // default font-size, same 16px root).
    let m = div_computed_map(
        "<div>x</div>",
        "div { overflow-clip-margin: calc(0.5em + 100px); }",
    );
    assert_eq!(m.get("overflow-clip-margin").map(String::as_str), Some("108px"));
}

// BUG-505 срез 3: `overflow` shorthand computed-value serialization
// (`css/css-overflow/parsing/overflow-computed.html`,
// `overflow-shorthand-001.html`) — collapses to one keyword when both
// axes agree, else `"x y"`, reading the axis-coerced values.
#[test]
fn computed_map_overflow_shorthand_collapses_equal_axes() {
    let m = div_computed_map("<div>x</div>", "div { overflow: hidden; }");
    assert_eq!(m.get("overflow").map(String::as_str), Some("hidden"));
}

#[test]
fn computed_map_overflow_shorthand_reports_pair() {
    let m = div_computed_map("<div>x</div>", "div { overflow-x: hidden; overflow-y: scroll; }");
    assert_eq!(m.get("overflow").map(String::as_str), Some("hidden scroll"));
}

#[test]
fn computed_map_overflow_shorthand_reflects_axis_coercion() {
    // overflow-x: visible + overflow-y: hidden → overflow-x computes to
    // `auto` (CSS Overflow L3 §2.1), so the shorthand reports "auto hidden",
    // not the raw specified "visible hidden".
    let m = div_computed_map("<div>x</div>", "div { overflow-x: visible; overflow-y: hidden; }");
    assert_eq!(m.get("overflow-x").map(String::as_str), Some("auto"));
    assert_eq!(m.get("overflow").map(String::as_str), Some("auto hidden"));
}

// BUG-505 срез 3: `overflow-block`/`overflow-inline` (CSS Overflow L3
// §logical) map to `overflow-y`/`overflow-x` under `horizontal-tb` and
// swap to `overflow-x`/`overflow-y` under a vertical writing mode
// (`css/css-overflow/logical-overflow-001.html`).
#[test]
fn computed_map_overflow_logical_horizontal_tb() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { overflow-block: hidden; overflow-inline: scroll; }",
    );
    assert_eq!(m.get("overflow-x").map(String::as_str), Some("scroll"));
    assert_eq!(m.get("overflow-y").map(String::as_str), Some("hidden"));
    assert_eq!(m.get("overflow-block").map(String::as_str), Some("hidden"));
    assert_eq!(m.get("overflow-inline").map(String::as_str), Some("scroll"));
}

#[test]
fn computed_map_overflow_logical_vertical_rl_swaps_axes() {
    let m = div_computed_map(
        "<div>x</div>",
        "div { writing-mode: vertical-rl; overflow-block: hidden; overflow-inline: scroll; }",
    );
    assert_eq!(m.get("overflow-x").map(String::as_str), Some("hidden"));
    assert_eq!(m.get("overflow-y").map(String::as_str), Some("scroll"));
    assert_eq!(m.get("overflow-block").map(String::as_str), Some("hidden"));
    assert_eq!(m.get("overflow-inline").map(String::as_str), Some("scroll"));
}

#[test]
fn computed_map_container_shorthand() {
    // BUG-1514: `container` serialises as `<name> [ / <type> ]?`, the type omitted when `normal`.
    let m = div_computed_map("<div>x</div>", "div { container-type: inline-size; container-name: foo bar }");
    assert_eq!(m.get("container").map(String::as_str), Some("foo bar / inline-size"));
    assert_eq!(m.get("container-type").map(String::as_str), Some("inline-size"));
    assert_eq!(m.get("container-name").map(String::as_str), Some("foo bar"));
    let m = div_computed_map("<div>x</div>", "div { container: foo }");
    assert_eq!(m.get("container").map(String::as_str), Some("foo"));
    let m = div_computed_map("<div>x</div>", "div { container-type: size }");
    assert_eq!(m.get("container").map(String::as_str), Some("none / size"));
    let m = div_computed_map("<div>x</div>", "div { }");
    assert_eq!(m.get("container").map(String::as_str), Some("none"));
}

/// BUG-1515: `(декларация, свойство, ожидаемое computed-значение)` на `div { font-size: 40px; color: blue }`.
#[test]
fn computed_map_text_decoration_and_shapes_bug1515() {
    let cases: &[(&str, &str, &str)] = &[
        // text-decoration-line: blink и ключевые слова L4.
        ("text-decoration-line: underline blink", "text-decoration-line", "underline blink"),
        ("text-decoration-line: blink", "text-decoration-line", "blink"),
        ("text-decoration-line: spelling-error", "text-decoration-line", "spelling-error"),
        // text-decoration — шорткат с thickness из L4.
        ("text-decoration: none", "text-decoration", "none"),
        ("text-decoration: solid", "text-decoration", "none"),
        ("text-decoration: currentcolor", "text-decoration", "none"),
        ("text-decoration: double overline underline", "text-decoration", "underline overline double"),
        ("text-decoration: underline overline line-through red", "text-decoration",
            "underline overline line-through rgb(255, 0, 0)"),
        ("text-decoration: underline red from-font", "text-decoration", "underline from-font rgb(255, 0, 0)"),
        ("text-decoration: 10px", "text-decoration", "10px"),
        ("text-decoration: auto", "text-decoration", "none"),
        // text-decoration-color: currentcolor → used value.
        ("text-decoration-color: currentcolor", "text-decoration-color", "rgb(0, 0, 255)"),
        // text-underline-offset / thickness: em от font-size элемента, calc, процент.
        ("text-underline-offset: 1em", "text-underline-offset", "40px"),
        ("text-underline-offset: calc(10px - 8px)", "text-underline-offset", "2px"),
        ("text-underline-offset: 200%", "text-underline-offset", "200%"),
        ("text-underline-offset: calc(2em - 50%)", "text-underline-offset", "calc(-50% + 80px)"),
        ("text-underline-offset: auto", "text-underline-offset", "auto"),
        ("text-decoration-thickness: 200%", "text-decoration-thickness", "200%"),
        ("text-decoration-thickness: calc(200% - 0.5em)", "text-decoration-thickness", "calc(200% - 20px)"),
        ("text-decoration-thickness: calc(200% - 50%)", "text-decoration-thickness", "150%"),
        // text-shadow: currentcolor → used value.
        ("text-shadow: 10px 20px", "text-shadow", "rgb(0, 0, 255) 10px 20px 0px"),
        // Остальные свойства L3/L4.
        ("text-underline-position: from-font left", "text-underline-position", "from-font left"),
        ("text-underline-position: left under", "text-underline-position", "under left"),
        ("text-decoration-skip-ink: all", "text-decoration-skip-ink", "all"),
        ("text-decoration-skip-spaces: end start", "text-decoration-skip-spaces", "start end"),
        ("text-decoration-skip-spaces: none", "text-decoration-skip-spaces", "none"),
        ("text-decoration-inset: 0.5em", "text-decoration-inset", "20px"),
        ("text-decoration-inset: 1px 2px", "text-decoration-inset", "1px 2px"),
        ("text-decoration-inset: 0 20%", "text-decoration-inset", "0px 20%"),
        ("text-decoration-inset: calc(10% - 1em) 0", "text-decoration-inset", "calc(10% - 40px) 0px"),
        ("text-decoration-inset: auto", "text-decoration-inset", "auto"),
        // text-emphasis.
        ("text-emphasis: none", "text-emphasis", "none rgb(0, 0, 255)"),
        ("text-emphasis: open sesame", "text-emphasis", "open sesame rgb(0, 0, 255)"),
        ("text-emphasis: '*'", "text-emphasis", "\"*\" rgb(0, 0, 255)"),
        ("text-emphasis: dot red", "text-emphasis", "dot rgb(255, 0, 0)"),
        ("text-emphasis-style: filled", "text-emphasis-style", "circle"),
        ("text-emphasis-style: open", "text-emphasis-style", "open circle"),
        ("text-emphasis-position: over right", "text-emphasis-position", "over"),
        ("text-emphasis-position: under left", "text-emphasis-position", "under left"),
        ("text-emphasis-position: auto", "text-emphasis-position", "auto"),
        ("text-emphasis-color: currentcolor", "text-emphasis-color", "rgb(0, 0, 255)"),
        // CSS Shapes.
        ("shape-margin: calc(10px + 0.5em)", "shape-margin", "30px"),
        ("shape-margin: calc(10px - 0.5em)", "shape-margin", "0px"),
        ("shape-margin: 50%", "shape-margin", "50%"),
        ("shape-image-threshold: 12.5", "shape-image-threshold", "1"),
        ("shape-image-threshold: 50%", "shape-image-threshold", "0.5"),
        ("shape-image-threshold: -100%", "shape-image-threshold", "0"),
        ("shape-outside: circle(at 10% 20%)", "shape-outside", "circle(at 10% 20%)"),
        ("shape-outside: none", "shape-outside", "none"),
    ];
    for (decl, prop, want) in cases {
        let m = div_computed_map("<div>x</div>", &format!("div {{ font-size: 40px; color: blue; {decl} }}"));
        assert_eq!(m.get(*prop).map(String::as_str), Some(*want), "{decl} → {prop}");
    }
    // Начальные значения.
    let m = div_computed_map("<div>x</div>", "div { }");
    for (prop, want) in [
        ("text-decoration", "none"),
        ("text-decoration-skip-spaces", "start end"),
        ("text-decoration-inset", "auto"),
        ("text-underline-position", "auto"),
        ("text-emphasis-position", "auto"),
        ("shape-outside", "none"),
        ("shape-margin", "0px"),
        ("shape-image-threshold", "0"),
    ] {
        assert_eq!(m.get(prop).map(String::as_str), Some(want), "initial {prop}");
    }
}

#[test]
fn computed_map_inline_properties_bug1529() {
    let get = |css: &str, key: &str| {
        div_computed_map("<div>x</div>", css).get(key).cloned().unwrap_or_default()
    };
    // `line-height`: `normal` stays a keyword, a number resolves to px, negatives are ignored.
    assert_eq!(get("div { font-size: 40px }", "line-height"), "normal");
    assert_eq!(get("div { font-size: 40px; line-height: 2 }", "line-height"), "80px");
    assert_eq!(get("div { font-size: 40px; line-height: 0 }", "line-height"), "0px");
    assert_eq!(get("div { font-size: 40px; line-height: 200% }", "line-height"), "80px");
    assert_eq!(get("div { font-size: 40px; line-height: calc(200% + 10px) }", "line-height"), "90px");
    assert_eq!(get("div { font-size: 40px; line-height: calc(10px - 0.5em) }", "line-height"), "0px");
    assert_eq!(get("div { font-size: 40px; line-height: -2 }", "line-height"), "normal");
    assert_eq!(get("div { font-size: 40px; line-height: auto }", "line-height"), "normal");
    // `baseline-shift`.
    assert_eq!(get("", "baseline-shift"), "0px");
    assert_eq!(get("div { baseline-shift: -10px }", "baseline-shift"), "-10px");
    assert_eq!(get("div { baseline-shift: 20% }", "baseline-shift"), "20%");
    assert_eq!(get("div { baseline-shift: center }", "baseline-shift"), "center");
    // `initial-letter`.
    assert_eq!(get("", "initial-letter"), "normal");
    assert_eq!(get("div { initial-letter: 3 2 }", "initial-letter"), "3 2");
    assert_eq!(get("div { initial-letter: 1.23 }", "initial-letter"), "1.23");
    assert_eq!(get("div { initial-letter: drop 2 }", "initial-letter"), "2 drop");
    assert_eq!(get("div { initial-letter: 2 raise }", "initial-letter"), "2 raise");
    assert_eq!(get("div { initial-letter: 1.23 calc(45.6) }", "initial-letter"), "1.23 46");
    assert_eq!(get("div { initial-letter: calc(sign(1rem - 1px) * 1.5) }", "initial-letter"), "1.5");
}

#[test]
fn computed_map_table_properties_bug1535() {
    let get = |css: &str, key: &str| {
        div_computed_map("<div>x</div>", css).get(key).cloned().unwrap_or_default()
    };
    for (prop, init) in [
        ("border-collapse", "separate"),
        ("caption-side", "top"),
        ("empty-cells", "show"),
        ("table-layout", "auto"),
    ] {
        assert_eq!(get("", prop), init, "initial {prop}");
    }
    assert_eq!(get("div { border-collapse: collapse }", "border-collapse"), "collapse");
    assert_eq!(get("div { caption-side: bottom }", "caption-side"), "bottom");
    assert_eq!(get("div { empty-cells: hide }", "empty-cells"), "hide");
    assert_eq!(get("div { table-layout: fixed }", "table-layout"), "fixed");
    // `border-spacing`: `calc()` resolves against the element's font; a negative result clamps to 0.
    let fs = "div { font-size: 40px; ";
    assert_eq!(get(&format!("{fs}border-spacing: calc(10px + 0.5em) calc(10px - 0.5em) }}"), "border-spacing"), "30px 0px");
    assert_eq!(get(&format!("{fs}border-spacing: calc(10px - 0.5em) calc(10px + 0.5em) }}"), "border-spacing"), "0px 30px");
    // `%`, a literal negative and a third component are invalid: the initial value stays.
    for bad in ["10%", "-20px", "10px 20px 30px", "calc(10px + 5%)"] {
        assert_eq!(get(&format!("{fs}border-spacing: {bad} }}"), "border-spacing"), "0px", "{bad}");
    }
}

fn filter_computed(prop: &str, value: &str) -> String {
    let m = div_computed_map("<div>x</div>", &format!("div {{ color: rgb(0, 255, 0); {prop}: {value}; }}"));
    m.get(prop).cloned().unwrap_or_default()
}

#[test]
fn computed_map_backdrop_filter_and_filter_canonical_forms() {
    // BUG-1541: `backdrop-filter` is in the computed map and both properties canonicalize.
    for prop in ["filter", "backdrop-filter"] {
        for (specified, want) in [
            ("none", "none"),
            ("blur(100px)", "blur(100px)"),
            ("blur()", "blur(0px)"),
            ("blur(0)", "blur(0px)"),
            ("brightness(300%)", "brightness(3)"),
            ("brightness()", "brightness(1)"),
            ("grayscale(50%)", "grayscale(0.5)"),
            ("grayscale(2)", "grayscale(1)"),
            ("invert(100%)", "invert(1)"),
            ("opacity(2)", "opacity(1)"),
            ("sepia()", "sepia(1)"),
            ("hue-rotate()", "hue-rotate(0deg)"),
            ("hue-rotate(0)", "hue-rotate(0deg)"),
            ("drop-shadow(1px 2px)", "drop-shadow(rgb(0, 255, 0) 1px 2px 0px)"),
            ("blur(2px) contrast(20)", "blur(2px) contrast(20)"),
            ("blur(calc(100px))", "blur(100px)"),
            ("brightness(calc(300%))", "brightness(3)"),
            ("brightness(calc(-10))", "brightness(0)"),
            ("grayscale(calc(300%))", "grayscale(1)"),
            ("opacity(calc(-10%))", "opacity(0)"),
            ("drop-shadow(rgb(4, 5, 6) calc(1px) 2px 0px)", "drop-shadow(rgb(4, 5, 6) 1px 2px 0px)"),
        ] {
            assert_eq!(filter_computed(prop, specified), want, "{prop}: {specified}");
        }
    }
}

#[test]
fn computed_map_filter_invalid_value_is_dropped() {
    for prop in ["filter", "backdrop-filter"] {
        for bad in [
            "auto", "none blur(1px)", "blur(10)", "blur(-100px)", "blur(5px 6px)", "brightness(-20)",
            "brightness(30px)", "contrast(-20)", "drop-shadow(10 20)", "drop-shadow(1px)",
            "drop-shadow(1px 2px 3px 4px)", "drop-shadow(rgb(4, 5, 6))", "drop-shadow()",
            "hue-rotate(90)", "opacity(-20)", "saturate(30px)", "sepia(-20)", "blur(1px) bogus(2)",
        ] {
            assert_eq!(filter_computed(prop, bad), "none", "{prop}: {bad}");
        }
    }
}

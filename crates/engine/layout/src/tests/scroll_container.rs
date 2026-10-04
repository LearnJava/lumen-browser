use super::*;

// ─── Scroll container tests ───────────────────────────────────────────────

#[test]
fn collect_scroll_containers_overflow_scroll() {
    let root = lay_full(
        "<div id=\"s\"><p>a</p></div>",
        "#s { overflow: scroll; width: 100px; height: 50px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1, "one scroll container expected");
    assert_eq!(containers[0].scroll_x, 0.0);
    assert_eq!(containers[0].scroll_y, 0.0);
    // clip rect should be approximately the padding-box of the div
    assert!(containers[0].clip_rect.width > 0.0);
    assert!(containers[0].clip_rect.height > 0.0);
}

#[test]
fn collect_scroll_containers_overflow_auto() {
    let root = lay_full(
        "<div id=\"s\"><p>b</p></div>",
        "#s { overflow: auto; width: 100px; height: 50px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
}

#[test]
fn collect_scroll_containers_overflow_hidden_excluded() {
    let root = lay_full(
        "<div id=\"s\"><p>c</p></div>",
        "#s { overflow: hidden; width: 100px; height: 50px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 0, "overflow:hidden should not be a scroll container");
}

// BUG-504 part 8: `collect_scroll_containers` (wheel-routable) and
// `collect_scroll_containers_for_js_state` (JS-visible scrollLeft/Top/Width/
// Height) must diverge on `hidden`/`clip` — see the doc comment on the latter
// for why a single shared list broke `hidden`/`clip` JS-visible scroll state.

#[test]
fn collect_scroll_containers_for_js_state_includes_hidden() {
    let root = lay_full(
        "<div id=\"s\"><p>c</p></div>",
        "#s { overflow: hidden; width: 100px; height: 50px; }",
    );
    assert_eq!(collect_scroll_containers(&root).len(), 0, "still not wheel-routable");
    let js_containers = collect_scroll_containers_for_js_state(&root);
    assert_eq!(js_containers.len(), 1, "hidden must be JS-visible");
}

#[test]
fn collect_scroll_containers_for_js_state_includes_clip() {
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { overflow: clip; width: 100px; height: 50px; } \
         #child { width: 300px; height: 30px; }",
    );
    assert_eq!(collect_scroll_containers(&root).len(), 0, "clip is not wheel-routable");
    let js_containers = collect_scroll_containers_for_js_state(&root);
    assert_eq!(js_containers.len(), 1, "clip must be JS-visible");
    assert!(
        (js_containers[0].scroll_width - 300.0).abs() < 0.5,
        "clip still reports the real scrollable-overflow size, only writes are rejected \
         (that half is set_scroll_position's job, not collection's), got {}",
        js_containers[0].scroll_width
    );
}

#[test]
fn collect_scroll_containers_for_js_state_reflects_programmatic_scroll_on_hidden() {
    // Mirrors the WPT repro this bug is chasing: `overflow:hidden` must be
    // programmatically scrollable (`set_scroll_position` already allows it,
    // BUG-504 part 7) AND that offset must reach the JS-visible collection —
    // before this split, `hidden` was invisible to it entirely.
    let mut root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { overflow: hidden; width: 100px; height: 50px; } \
         #child { width: 300px; height: 30px; }",
    );
    let node = collect_scroll_containers_for_js_state(&root)[0].node;
    assert!(set_scroll_position(&mut root, node, 40.0, 0.0));
    let js_containers = collect_scroll_containers_for_js_state(&root);
    assert_eq!(js_containers.len(), 1);
    assert_eq!(js_containers[0].scroll_x, 40.0);
}

#[test]
fn collect_scroll_containers_transform_grows_scroll_width() {
    // BUG-504: a child that only overflows via `transform` must still grow
    // scrollWidth — CSS Overflow L3 §3.4 treats transform as contributing
    // to the scrollable overflow rectangle, even though it never moves the
    // child's own flow `rect`. 100px container, 50px child translated 200px
    // right → painted right edge at 250px, well past the untransformed 100px.
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { overflow: auto; width: 100px; height: 100px; } \
         #child { width: 50px; height: 50px; transform: translateX(200px); }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_width - 250.0).abs() < 0.5,
        "expected scroll_width≈250 (100 container is not enough, transform pushes child to \
         x=200..250), got {}",
        containers[0].scroll_width
    );
}

#[test]
fn collect_scroll_containers_transform_grows_scroll_height() {
    // Same as above, vertical axis: translateY(300px) on a 50px child inside
    // a 100px container should push scroll_height to ≈350.
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { overflow: auto; width: 100px; height: 100px; } \
         #child { width: 50px; height: 50px; transform: translateY(300px); }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_height - 350.0).abs() < 0.5,
        "expected scroll_height≈350, got {}",
        containers[0].scroll_height
    );
}

#[test]
fn collect_scroll_containers_rtl_overflow_grows_scroll_width() {
    // BUG-504 (`overflow-rtl-scroll-left.html`): regression guard for
    // `content_width` under `direction: rtl` — a 500px child in a 300px RTL
    // container must still report `scroll_width == 500`, same as the LTR
    // case, since `content_width`'s fold only cares about the child's
    // rightmost edge relative to the padding edge, not which direction the
    // overflow visually hangs off. This function was never the actual bug
    // (confirmed correct by this test); the real defect was `apply_loaded_page`
    // (`crates/shell/src/page_load.rs`) never seeding the JS-side
    // `_lumen_get_scroll_state` cache on initial page load, so `scrollWidth`
    // read the fallback border-box size (300) until an unrelated relayout
    // raced ahead of the first script — same shape as BUG-382, for scroll
    // state instead of rects/styles.
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { direction: rtl; overflow: auto; width: 300px; height: 200px; } \
         #child { width: 500px; height: 200px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_width - 500.0).abs() < 0.5,
        "expected scroll_width≈500 under RTL, got {}",
        containers[0].scroll_width
    );
    assert!((containers[0].scroll_x - 0.0).abs() < 0.5, "default scroll_x must stay 0");
}

#[test]
fn collect_scroll_containers_no_transform_unaffected() {
    // Regression guard: an untransformed child must still compute exactly as
    // before — the child_scrollable_bounds fast path (`c.rect` passthrough)
    // must not perturb the plain flow case.
    let root = lay_full(
        "<div id=\"s\"><div style=\"height:200px\"></div></div>",
        "#s { overflow: auto; width: 100px; height: 50px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_height - 200.0).abs() < 0.5,
        "expected scroll_height≈200 (untransformed child), got {}",
        containers[0].scroll_height
    );
}

#[test]
fn collect_scroll_containers_abspos_wholly_outside_padding_excluded() {
    // BUG-504 (WPT `overflow-outside-padding.html`): CSS Overflow L3 §3.3 — an
    // absolutely positioned descendant landing wholly outside the padding
    // edges on *either* axis contributes nothing at all, even on the axis
    // where it partially overlaps. A 1000x1000 child pinned via `top:
    // -1000px` sits directly above a 100x100 container: its bottom edge
    // lands exactly on the container's padding-top edge (zero overlap on Y)
    // despite overlapping horizontally, so scrollWidth/scrollHeight must
    // stay at the container's own padding-box size, not the child's extent.
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { position: relative; overflow: auto; width: 100px; height: 100px; } \
         #child { position: absolute; width: 1000px; height: 1000px; top: -1000px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_width - 100.0).abs() < 0.5,
        "expected scroll_width=100 (child wholly above container, no Y overlap → excluded), got {}",
        containers[0].scroll_width
    );
    assert!(
        (containers[0].scroll_height - 100.0).abs() < 0.5,
        "expected scroll_height=100 (child wholly above container, no Y overlap → excluded), got {}",
        containers[0].scroll_height
    );
}

#[test]
fn collect_scroll_containers_abspos_overlapping_child_still_contributes() {
    // Regression guard for the exclusion above: an abspos child that
    // genuinely overlaps the padding box on both axes must still grow
    // scrollWidth normally — only a child wholly outside on some axis is
    // excluded.
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { position: relative; overflow: auto; width: 100px; height: 100px; } \
         #child { position: absolute; width: 50px; height: 50px; top: 0; left: 80px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_width - 130.0).abs() < 0.5,
        "expected scroll_width=130 (child at x=80..130 overlaps container, extends past right \
         edge), got {}",
        containers[0].scroll_width
    );
}

#[test]
fn collect_scroll_containers_scroll_width_floor_is_padding_box_not_border_box() {
    // BUG-504 (WPT `overflow-outside-padding.html`): scrollWidth's floor must
    // be the padding-box size (CSS Overflow L3 §3.3), not the border-box
    // size — a 200px-wide content box with an 80px left border has a 280px
    // border box but only a 200px padding box (no padding declared).
    let root = lay_full(
        "<div id=\"s\"></div>",
        "#s { overflow: auto; width: 200px; height: 200px; border-style: solid; \
         border-width: 0 0 50px 80px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_width - 200.0).abs() < 0.5,
        "expected scroll_width=200 (padding-box width, border-left excluded), got {}",
        containers[0].scroll_width
    );
}

#[test]
fn set_scroll_position_clamps_to_zero() {
    let mut root = lay_full(
        "<div id=\"s\"><p>d</p></div>",
        "#s { overflow: scroll; width: 100px; height: 50px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    let node = containers[0].node;
    set_scroll_position(&mut root, node, -50.0, -50.0);
    let containers2 = collect_scroll_containers(&root);
    assert_eq!(containers2[0].scroll_x, 0.0, "negative scroll_x should clamp to 0");
    assert_eq!(containers2[0].scroll_y, 0.0, "negative scroll_y should clamp to 0");
}

#[test]
fn set_scroll_position_sets_value() {
    let mut root = lay_full(
        "<div id=\"s\"><div style=\"height:200px\"></div></div>",
        "#s { overflow: scroll; width: 100px; height: 50px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    let node = containers[0].node;
    let found = set_scroll_position(&mut root, node, 0.0, 10.0);
    assert!(found, "set_scroll_position should return true when node found");
    let containers2 = collect_scroll_containers(&root);
    assert_eq!(containers2[0].scroll_y, 10.0);
}

/// BUG-935 срез 63: the one-walk restore gives exactly what a `set_scroll_position`
/// loop over the same map gave — nested containers, an offset above and below the
/// scrollable range, a `(0, 0)` request, and a key that is not in the tree.
#[test]
fn restore_scroll_positions_matches_the_per_node_loop() {
    let html = "<div id=\"a\"><div style=\"height:400px\"></div>        <div id=\"b\"><div style=\"height:300px\"></div></div></div>        <div id=\"c\"><div style=\"height:200px\"></div></div>";
    let css = "#a, #b, #c { overflow: scroll; width: 100px; height: 50px; }";
    let mut by_loop = lay_full(html, css);
    let mut by_walk = lay_full(html, css);
    let nodes: Vec<_> = collect_scroll_containers(&by_loop).iter().map(|c| c.node).collect();
    assert_eq!(nodes.len(), 3);
    let wanted = [[0.0, 25.0], [0.0, 9999.0], [0.0, 0.0]];
    let mut states = std::collections::HashMap::new();
    for (n, w) in nodes.iter().zip(wanted) {
        states.insert(n.raw(), [w[0], w[1], 0.0, 0.0]);
    }
    states.insert(lumen_dom::NodeId::from_index(9999).raw(), [5.0, 5.0, 0.0, 0.0]);
    for (&nid, s) in &states {
        set_scroll_position(&mut by_loop, lumen_dom::NodeId::from_raw(nid), s[0], s[1]);
    }
    restore_scroll_positions(&mut by_walk, &states);
    let offsets = |root: &LayoutBox| -> Vec<(u32, f32, f32)> {
        collect_scroll_containers(root).iter().map(|c| (c.node.raw(), c.scroll_x, c.scroll_y)).collect()
    };
    assert_eq!(offsets(&by_walk), offsets(&by_loop));
    let got = offsets(&by_walk);
    assert_eq!(got[0].2, 25.0, "in-range offset applied");
    assert!(got[1].2 > 0.0 && got[1].2 < 9999.0, "out-of-range offset clamped, got {}", got[1].2);
    assert_eq!(got[2].2, 0.0);
}

/// A request for `(0, 0)` on a box that is not at the origin must still reset it
/// (the skip only covers a box already there).
#[test]
fn restore_scroll_positions_resets_a_box_not_at_the_origin() {
    let mut root = lay_full(
        "<div id=\"s\"><div style=\"height:200px\"></div></div>",
        "#s { overflow: scroll; width: 100px; height: 50px; }",
    );
    let node = collect_scroll_containers(&root)[0].node;
    set_scroll_position(&mut root, node, 0.0, 30.0);
    assert_eq!(collect_scroll_containers(&root)[0].scroll_y, 30.0);
    let states = [(node.raw(), [0.0, 0.0, 0.0, 0.0])].into_iter().collect();
    restore_scroll_positions(&mut root, &states);
    assert_eq!(collect_scroll_containers(&root)[0].scroll_y, 0.0);
}

#[test]
fn set_scroll_position_returns_false_for_unknown_node() {
    use lumen_dom::NodeId;
    let mut root = lay_full("<div></div>", "");
    let found = set_scroll_position(&mut root, NodeId::from_index(9999), 0.0, 0.0);
    assert!(!found, "should return false for unknown node");
}

/// Recursively finds the first box matching `pred` — used by the tests below
/// to locate a scroll container whose overflow value (`hidden`/`clip`)
/// excludes it from [`collect_scroll_containers`] (which only registers
/// `Scroll`/`Auto`), so they can't get a `NodeId` the way the tests above do.
fn find_box_where<'a>(
    b: &'a LayoutBox,
    pred: &impl Fn(&LayoutBox) -> bool,
) -> Option<&'a LayoutBox> {
    if pred(b) {
        return Some(b);
    }
    b.children.iter().find_map(|c| find_box_where(c, pred))
}

#[test]
fn set_scroll_position_vertical_rl_allows_negative_scroll_x() {
    // BUG-504 (WPT `overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html`):
    // under `writing-mode: vertical-rl` the block-progression direction is
    // physically right-to-left, so the vertical-writing-mode block layout
    // path positions an only in-flow child flush with the container's
    // *right* padding edge and lets it extend left — the scrollable
    // overflow lands on the negative-x side. `scrollable_extent_x` used to
    // only ever grow the positive (rightward) edge, so `content_width`
    // reported no overflow at all here (300px child entirely "left" of a
    // formula that only looked right) and every scroll request clamped to
    // 0. A 300px child in a 100px container must allow `scrollLeft` down to
    // -200 (300-100).
    let mut root = lay_full(
        "<div id=\"s\"><div id=\"c\"></div></div>",
        "#s { writing-mode: vertical-rl; overflow: hidden; width: 100px; height: 100px; } \
         #c { width: 300px; height: 300px; }",
    );
    let node = find_box_where(&root, &|b| b.style.writing_mode == style::WritingMode::VerticalRl)
        .expect("vertical-rl box")
        .node;

    let found = set_scroll_position(&mut root, node, -40.0, 50.0);
    assert!(found);
    let b = find_box_by_node(&root, node).expect("box still there");
    assert!((b.scroll_x - -40.0).abs() < 0.5, "expected scroll_x=-40, got {}", b.scroll_x);
    assert!((b.scroll_y - 50.0).abs() < 0.5, "expected scroll_y=50, got {}", b.scroll_y);

    // Range floor: -200 == -(300-100), further negative still clamps there;
    // positive y still clamps at 200 == 300-100, same as the plain LTR case.
    set_scroll_position(&mut root, node, -1000.0, 1000.0);
    let b2 = find_box_by_node(&root, node).expect("box still there");
    assert!(
        (b2.scroll_x - -200.0).abs() < 0.5,
        "expected scroll_x clamped to -200, got {}",
        b2.scroll_x
    );
    assert!(
        (b2.scroll_y - 200.0).abs() < 0.5,
        "expected scroll_y clamped to 200, got {}",
        b2.scroll_y
    );
}

#[test]
fn set_scroll_position_overflow_clip_forces_zero_and_rejects_writes() {
    // BUG-504 (same WPT file): `overflow: clip` disables the scrolling
    // machinery outright (CSS Overflow L3 §3.4) — unlike `hidden`, which
    // stays programmatically scrollable, a `clip` axis must report exactly
    // 0 and ignore every scroll request, not just clamp to a shrunken range.
    let mut root = lay_full(
        "<div id=\"s\"><div id=\"c\"></div></div>",
        "#s { overflow: clip; width: 100px; height: 100px; } \
         #c { width: 300px; height: 300px; }",
    );
    let node = find_box_where(&root, &|b| b.style.overflow_x == style::Overflow::Clip)
        .expect("clip box")
        .node;

    set_scroll_position(&mut root, node, -40.0, 50.0);
    let b = find_box_by_node(&root, node).expect("box still there");
    assert_eq!(b.scroll_x, 0.0, "overflow:clip must reject scroll requests (x)");
    assert_eq!(b.scroll_y, 0.0, "overflow:clip must reject scroll requests (y)");
}

#[test]
fn set_scroll_position_overflow_clip_clamps_existing_offset_back_to_zero() {
    // BUG-504 (same WPT file, part 2 of the scenario): a container that
    // already carries a nonzero scroll offset (established while it was
    // still `overflow: hidden`) must have it clamped back to 0 once
    // `set_scroll_position` next runs against it under `overflow: clip` —
    // exercised here by driving the same `LayoutBox` through both overflow
    // values directly (the style-mutation → relayout → re-seed path that
    // would carry this in the live shell is a separate, JS/shell-side
    // concern, not this function's).
    let mut root = lay_full(
        "<div id=\"s\"><div id=\"c\"></div></div>",
        "#s { writing-mode: vertical-rl; overflow: hidden; width: 100px; height: 100px; } \
         #c { width: 300px; height: 300px; }",
    );
    let node = find_box_where(&root, &|b| b.style.writing_mode == style::WritingMode::VerticalRl)
        .expect("vertical-rl box")
        .node;
    set_scroll_position(&mut root, node, -40.0, 50.0);
    let b = find_box_by_node(&root, node).expect("box still there");
    assert!((b.scroll_x - -40.0).abs() < 0.5, "sanity: hidden allows -40");

    let mut clipped = lay_full(
        "<div id=\"s\"><div id=\"c\"></div></div>",
        "#s { writing-mode: vertical-rl; overflow: clip; width: 100px; height: 100px; } \
         #c { width: 300px; height: 300px; }",
    );
    let clip_node =
        find_box_where(&clipped, &|b| b.style.writing_mode == style::WritingMode::VerticalRl)
            .expect("vertical-rl box")
            .node;
    // Any request — even repeating the previously-accepted -40/50 — is a
    // no-op once the axis reports `clip`.
    set_scroll_position(&mut clipped, clip_node, -40.0, 50.0);
    let c = find_box_by_node(&clipped, clip_node).expect("box still there");
    assert_eq!(c.scroll_x, 0.0);
    assert_eq!(c.scroll_y, 0.0);
}

#[test]
fn set_scroll_position_max_scroll_uses_padding_box_not_border_box() {
    // Regression guard: `set_scroll_position`'s clamp used to subtract
    // `LayoutBox::rect`'s BORDER-box width from `content_width`'s
    // padding-box-relative magnitude, so a container with a nonzero border
    // capped its maximum scroll offset `2 × border` short of the true
    // value (here it would have clamped to 60 instead of 100, and with a
    // border ≥ half the overflow it could clamp the max *below* the min,
    // rejecting every scroll request outright). 300px content in a
    // 200px-wide container with a 40px left border (240px border-box, 200px
    // padding-box, no padding declared): max scrollLeft must be 100
    // (300-200), not 300-240=60.
    let mut root = lay_full(
        "<div id=\"s\"><div id=\"c\"></div></div>",
        "#s { overflow: scroll; width: 200px; height: 50px; border-style: solid; \
         border-width: 0 0 0 40px; } \
         #c { width: 300px; height: 10px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    let node = containers[0].node;
    set_scroll_position(&mut root, node, 1000.0, 0.0);
    let b = find_box_by_node(&root, node).expect("box still there");
    assert!(
        (b.scroll_x - 100.0).abs() < 0.5,
        "expected scroll_x clamped to 100 (padding-box max), got {}",
        b.scroll_x
    );
}

// ── BUG-960: scrollable-overflow rollup through non-clipping descendants ──

#[test]
fn scroll_width_nested_overflow_visible_reaches_outer_container() {
    // A grandchild that overflows only reaches the outer scroll container's
    // scroll_width when the intermediate box doesn't clip (`overflow: visible`,
    // the default) — before BUG-960's fix, `content_width` only walked direct
    // children, so this nested overflow was invisible to `#s`.
    let root = lay_full(
        "<div id=\"s\"><div id=\"mid\"><div id=\"deep\"></div></div></div>",
        "#s { overflow: auto; width: 100px; height: 100px; } \
         #mid { width: 50px; height: 50px; } \
         #deep { width: 400px; height: 40px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_width - 400.0).abs() < 0.5,
        "nested overflow (through a non-clipping #mid) must still grow #s's \
         scroll_width to ≈400, got {}",
        containers[0].scroll_width
    );
}

#[test]
fn scroll_width_nested_overflow_stops_at_clipping_descendant() {
    // Mirror of the above: when the intermediate box itself clips
    // (`overflow: hidden`), its own overflowing content must NOT roll up
    // into the outer container — it's already contained by `#mid`. `#mid`
    // is wider than `#s`'s padding box (150 > 100) so its own border box
    // still grows `#s`'s scroll_width past the floor, but `#deep`'s 400px
    // must not reach any further than that.
    let root = lay_full(
        "<div id=\"s\"><div id=\"mid\"><div id=\"deep\"></div></div></div>",
        "#s { overflow: auto; width: 100px; height: 100px; } \
         #mid { width: 150px; height: 50px; overflow: hidden; } \
         #deep { width: 400px; height: 40px; }",
    );
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1);
    assert!(
        (containers[0].scroll_width - 150.0).abs() < 0.5,
        "#mid clips its own overflow, so #s's scroll_width should stop at \
         #mid's own border-box width (≈150), not reach #deep's 400, got {}",
        containers[0].scroll_width
    );
}

#[test]
fn scroll_width_overflow_visible_reports_exact_overflow_not_just_border_box() {
    // CSSOM View defines scrollWidth/scrollHeight for every element, not
    // just designated scroll containers — an `overflow: visible` box whose
    // content overflows its own padding box must be JS-visible with the
    // exact scrollable-overflow magnitude (not just the border-box floor
    // BUG-475 left in place).
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { width: 80px; height: 80px; } \
         #child { width: 300px; height: 30px; }",
    );
    assert_eq!(
        collect_scroll_containers(&root).len(),
        0,
        "overflow:visible is never wheel-routable"
    );
    let js_containers = collect_scroll_containers_for_js_state(&root);
    assert_eq!(js_containers.len(), 1, "overflowing visible box must be JS-visible");
    assert!(
        (js_containers[0].scroll_width - 300.0).abs() < 0.5,
        "expected exact scroll_width≈300 (child's width), got {}",
        js_containers[0].scroll_width
    );
}

#[test]
fn scroll_width_overflow_visible_non_overflowing_absent_from_js_state() {
    // The common case — content that fits inside the padding box — must NOT
    // be published: the JS shim's border-box fallback already gives the
    // right answer more cheaply, and publishing every non-overflowing box
    // would blow up `update_scroll_states`'s per-frame cost.
    let root = lay_full(
        "<div id=\"s\"><div id=\"child\"></div></div>",
        "#s { width: 200px; height: 200px; } \
         #child { width: 50px; height: 30px; }",
    );
    let js_containers = collect_scroll_containers_for_js_state(&root);
    assert_eq!(
        js_containers.len(),
        0,
        "non-overflowing overflow:visible box must not be published"
    );
}

// ── text-wrap: balance / pretty ─────────────────────────────────────────

fn twrap_find_run(b: &LayoutBox) -> Option<&LayoutBox> {
    if matches!(b.kind, BoxKind::InlineRun { .. }) {
        return Some(b);
    }
    for c in &b.children {
        if let Some(f) = twrap_find_run(c) {
            return Some(f);
        }
    }
    None
}

pub(super) fn twrap_line_count(root: &LayoutBox) -> usize {
    twrap_find_run(root)
        .and_then(|b| {
            if let BoxKind::InlineRun { lines, .. } = &b.kind {
                Some(lines.len())
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn twrap_last_end_x(root: &LayoutBox) -> f32 {
    twrap_find_run(root)
        .and_then(|b| {
            if let BoxKind::InlineRun { lines, .. } = &b.kind {
                lines.last().and_then(|l| l.last()).map(|f| f.x + f.width)
            } else {
                None
            }
        })
        .unwrap_or(0.0)
}

// Fixed8: "aaaa"=32px, "bb"=16px, "cc"=16px, "dd"=16px, space=8px.
// Greedy at 80px: ["aaaa"(32) "bb"(16) "cc"(16)] end=80, ["dd"(16)] end=16.
// Balance: binary search → wrap_width≈56 → ["aaaa" "bb"] end=56, ["cc" "dd"] end=40.

#[test]
fn text_wrap_balance_preserves_line_count() {
    let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
    let balanced = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: balance; }", 80.0);
    assert_eq!(twrap_line_count(&greedy), 2, "greedy should produce 2 lines");
    assert_eq!(twrap_line_count(&balanced), 2, "balance must keep same line count");
}

#[test]
fn text_wrap_balance_widens_last_line() {
    let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
    let balanced = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: balance; }", 80.0);
    // last line: greedy=16px ("dd"), balanced=40px ("cc dd")
    assert!(
        twrap_last_end_x(&balanced) > twrap_last_end_x(&greedy),
        "balance must widen last line: {} <= {}",
        twrap_last_end_x(&balanced),
        twrap_last_end_x(&greedy)
    );
}

#[test]
fn text_wrap_balance_narrows_first_line() {
    let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
    let balanced = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: balance; }", 80.0);
    // first line: greedy=80px, balanced=56px
    let greedy_end = twrap_find_run(&greedy)
        .and_then(|b| {
            if let BoxKind::InlineRun { lines, .. } = &b.kind {
                lines.first().and_then(|l| l.last()).map(|f| f.x + f.width)
            } else {
                None
            }
        })
        .unwrap_or(0.0);
    let balanced_end = twrap_find_run(&balanced)
        .and_then(|b| {
            if let BoxKind::InlineRun { lines, .. } = &b.kind {
                lines.first().and_then(|l| l.last()).map(|f| f.x + f.width)
            } else {
                None
            }
        })
        .unwrap_or(0.0);
    assert!(
        balanced_end < greedy_end,
        "balance must narrow first line: {} >= {}",
        balanced_end,
        greedy_end
    );
}

#[test]
fn text_wrap_balance_single_line_is_noop() {
    // Single-line text must not be touched by balance.
    let normal = lay_measured("<p>hello</p>", "", 200.0);
    let balanced = lay_measured("<p>hello</p>", "p { text-wrap: balance; }", 200.0);
    assert_eq!(twrap_line_count(&normal), 1);
    assert_eq!(twrap_line_count(&balanced), 1);
    assert_eq!(twrap_last_end_x(&normal), twrap_last_end_x(&balanced));
}

#[test]
fn text_wrap_stable_behaves_like_auto() {
    // For static layout `stable` is identical to `auto` (stability is an
    // incremental-editing concern, not a static-render concern).
    let auto = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: auto; }", 80.0);
    let stable = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: stable; }", 80.0);
    assert_eq!(
        twrap_line_count(&auto),
        twrap_line_count(&stable),
        "stable must produce same line count as auto"
    );
    assert_eq!(
        twrap_last_end_x(&auto),
        twrap_last_end_x(&stable),
        "stable last line must match auto"
    );
}

#[test]
fn text_wrap_pretty_prevents_widow() {
    // Greedy: last line is just "dd" (16px). Pretty must widen it to "cc dd" (40px).
    // Words may be merged into one InlineFrag, so we check end_x, not frag count.
    let greedy = lay_measured("<p>aaaa bb cc dd</p>", "", 80.0);
    let pretty = lay_measured("<p>aaaa bb cc dd</p>", "p { text-wrap: pretty; }", 80.0);
    assert_eq!(twrap_line_count(&pretty), 2, "pretty must keep 2 lines");
    assert!(
        twrap_last_end_x(&pretty) > twrap_last_end_x(&greedy),
        "pretty must widen last line: {} <= {}",
        twrap_last_end_x(&pretty),
        twrap_last_end_x(&greedy)
    );
}

#[test]
fn text_wrap_pretty_no_widow_noop() {
    // If last line already has ≥2 words, pretty must not change anything.
    // "aaaa bb cc dd ee" at 80px → greedy: ["aaaa bb cc"(80), "dd ee"(40)].
    // Last line already has 2 frags → pretty is a no-op.
    let auto = lay_measured("<p>aaaa bb cc dd ee</p>", "", 80.0);
    let pretty = lay_measured("<p>aaaa bb cc dd ee</p>", "p { text-wrap: pretty; }", 80.0);
    assert_eq!(
        twrap_line_count(&auto),
        twrap_line_count(&pretty),
        "pretty must not change non-widow layout"
    );
    assert_eq!(
        twrap_last_end_x(&auto),
        twrap_last_end_x(&pretty),
        "pretty last line end must match auto when no widow"
    );
}

#[test]
fn text_wrap_shorthand_after_nowrap_reenables_wrap() {
    // CSS Text L4 §6.4.3: text-wrap — shorthand над text-wrap-mode/style
    // и сбрасывает mode к initial (wrap). Объявленный после
    // white-space: nowrap, он снова включает перенос строк.
    let root = lay_measured(
        "<p>aaaa bb cc dd</p>",
        "p { white-space: nowrap; text-wrap: balance; }",
        80.0,
    );
    assert!(
        twrap_line_count(&root) >= 2,
        "text-wrap after nowrap must reset wrap mode and wrap lines"
    );
}

#[test]
fn white_space_nowrap_after_text_wrap_stays_single_line() {
    // Обратный порядок: white-space (shorthand) объявлен позже и
    // сбрасывает text-wrap-mode к nowrap — одна строка.
    let root = lay_measured(
        "<p>aaaa bb cc dd</p>",
        "p { text-wrap: balance; white-space: nowrap; }",
        80.0,
    );
    assert_eq!(
        twrap_line_count(&root),
        1,
        "later white-space: nowrap must win over earlier text-wrap"
    );
}

#[test]
fn text_wrap_balance_longer_sequence() {
    // "aa bb cc dd ee ff" — 6 two-char words × 8px = 16px each, space=8px.
    // At 80px greedy: 3 lines → balance should equalize.
    let balanced = lay_measured(
        "<p>aa bb cc dd ee ff</p>",
        "p { text-wrap: balance; }",
        80.0,
    );
    let count = twrap_line_count(&balanced);
    assert!((2..=3).contains(&count), "balanced should have 2-3 lines, got {count}");
    // Last line must be wider than a single 2-char word (16px).
    assert!(
        twrap_last_end_x(&balanced) > 16.0,
        "last line must have more than one word after balance"
    );
}

#[test]
fn range_input_creates_range_kind() {
    use box_tree::FormControlKind;
    let doc = lumen_html_parser::parse(r#"<input type="range" min="10" max="90" value="50">"#);
    let sheet = lumen_css_parser::parse("");
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let found = find_range_kind(&root);
    assert!(found.is_some(), "range input should produce FormControlKind::Range");
    if let Some(FormControlKind::Range { value, min, max }) = found {
        assert!((value - 50.0).abs() < 0.001, "value should be 50, got {value}");
        assert!((min - 10.0).abs() < 0.001, "min should be 10, got {min}");
        assert!((max - 90.0).abs() < 0.001, "max should be 90, got {max}");
    }
}

#[test]
fn range_input_defaults_min_max() {
    use box_tree::FormControlKind;
    let doc = lumen_html_parser::parse(r#"<input type="range">"#);
    let sheet = lumen_css_parser::parse("");
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let found = find_range_kind(&root);
    assert!(found.is_some(), "range input without min/max should produce FormControlKind::Range");
    if let Some(FormControlKind::Range { value, min, max }) = found {
        assert!((min - 0.0).abs() < 0.001, "default min should be 0");
        assert!((max - 100.0).abs() < 0.001, "default max should be 100");
        assert!((value - 50.0).abs() < 0.001, "default value should be midpoint 50");
    }
}

#[test]
fn range_input_value_clamped_to_max() {
    use box_tree::FormControlKind;
    let doc = lumen_html_parser::parse(r#"<input type="range" min="0" max="10" value="999">"#);
    let sheet = lumen_css_parser::parse("");
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    if let Some(FormControlKind::Range { value, max, .. }) = find_range_kind(&root) {
        assert!(value <= max, "value {value} should be clamped to max {max}");
    }
}

#[test]
fn range_input_is_clickable() {
    let doc = lumen_html_parser::parse(r#"<input type="range">"#);
    let sheet = lumen_css_parser::parse("");
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let elems = collect_clickable_elements(&root, &doc);
    assert!(
        elems.iter().any(|e| matches!(e.kind, ClickableKind::Input)),
        "range input should be collected as clickable Input"
    );
}

fn find_range_kind(root: &LayoutBox) -> Option<box_tree::FormControlKind> {
    if let BoxKind::FormControl { kind } = &root.kind
        && matches!(kind, box_tree::FormControlKind::Range { .. })
    {
        return Some(kind.clone());
    }
    for child in &root.children {
        if let Some(k) = find_range_kind(child) {
            return Some(k);
        }
    }
    None
}

// ── find_scroll_container_at ──────────────────────────────────────────────

fn make_scroll_container(node_idx: usize, x: f32, y: f32, w: f32, h: f32) -> ScrollContainer {
    use lumen_core::geom::Rect;
    ScrollContainer {
        node: lumen_dom::NodeId::from_index(node_idx),
        clip_rect: Rect::new(x, y, w, h),
        scroll_width: w + 200.0,
        scroll_height: h + 400.0,
        scroll_x: 0.0,
        scroll_y: 0.0,
        overscroll_behavior_x: style::OverscrollBehavior::Auto,
        overscroll_behavior_y: style::OverscrollBehavior::Auto,
    }
}

#[test]
fn find_scroll_container_at_hit() {
    let c = make_scroll_container(1, 10.0, 20.0, 100.0, 200.0);
    let result = find_scroll_container_at(&[c], 50.0, 80.0);
    assert_eq!(result, Some(lumen_dom::NodeId::from_index(1)));
}

#[test]
fn find_scroll_container_at_miss() {
    let c = make_scroll_container(1, 10.0, 20.0, 100.0, 200.0);
    // Point outside the container
    assert_eq!(find_scroll_container_at(&[c], 5.0, 80.0), None);
}

#[test]
fn find_scroll_container_at_empty() {
    assert_eq!(find_scroll_container_at(&[], 50.0, 50.0), None);
}

#[test]
fn find_scroll_container_at_innermost_wins() {
    // Outer container covers (0,0,200,200), inner covers (50,50,50,50).
    // A point inside both should return the inner (last in list = deeper in DOM).
    let outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
    let inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
    let result = find_scroll_container_at(&[outer, inner], 60.0, 60.0);
    assert_eq!(result, Some(lumen_dom::NodeId::from_index(2)));
}

#[test]
fn find_scroll_container_at_only_outer_when_point_outside_inner() {
    let outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
    let inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
    // Point in outer but not in inner
    let result = find_scroll_container_at(&[outer, inner], 10.0, 10.0);
    assert_eq!(result, Some(lumen_dom::NodeId::from_index(1)));
}

// ── find_scroll_container_for_node (BUG-338) ────────────────────────────

#[test]
fn find_scroll_container_for_node_walks_dom_ancestors() {
    let doc = lumen_html_parser::parse(
        "<div id=\"outer\"><div id=\"inner\"><p id=\"leaf\">x</p></div></div>",
    );
    let sheet = lumen_css_parser::parse(
        "#outer { overflow: auto; width: 200px; height: 100px; }\n\
         #inner { height: 400px; }",
    );
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let containers = collect_scroll_containers(&root);
    assert_eq!(containers.len(), 1, "only #outer should be a scroll container");
    let outer_node = containers[0].node;

    let leaf = crate::selector_query::find_box_by_selector(&root, &doc, "#leaf")
        .expect("#leaf should have a layout box")
        .node;
    let result = find_scroll_container_for_node(&containers, &doc, leaf);
    assert_eq!(result, Some(outer_node), "#leaf's nearest scrolling ancestor is #outer");
}

#[test]
fn find_scroll_container_for_node_matches_node_itself() {
    let doc = lumen_html_parser::parse("<div id=\"outer\"><p>x</p></div>");
    let sheet = lumen_css_parser::parse("#outer { overflow: auto; width: 200px; height: 100px; }");
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let containers = collect_scroll_containers(&root);
    let outer_node = containers[0].node;
    let result = find_scroll_container_for_node(&containers, &doc, outer_node);
    assert_eq!(result, Some(outer_node), "the scroll container itself matches, no walk needed");
}

#[test]
fn find_scroll_container_for_node_none_when_no_scrolling_ancestor() {
    let doc = lumen_html_parser::parse("<div id=\"leaf\">x</div>");
    let sheet = lumen_css_parser::parse("");
    let root = layout(&doc, &sheet, Size::new(800.0, 600.0));
    let containers = collect_scroll_containers(&root);
    assert!(containers.is_empty());
    let leaf = crate::selector_query::find_box_by_selector(&root, &doc, "#leaf")
        .expect("#leaf should have a layout box")
        .node;
    assert_eq!(find_scroll_container_for_node(&containers, &doc, leaf), None);
}

// ── collect_view_transition_names ─────────────────────────────────────────

#[test]
fn vt_names_empty_without_property() {
    let root = lay("<div></div>", "div { width: 100px; height: 50px; }");
    let names = collect_view_transition_names(&root);
    assert!(names.is_empty(), "no view-transition-name set → empty");
}

#[test]
fn vt_names_single_named_element() {
    let root = lay(
        "<div></div>",
        "div { view-transition-name: hero; width: 100px; height: 50px; }",
    );
    let names = collect_view_transition_names(&root);
    assert_eq!(names.len(), 1, "one named element");
    assert_eq!(names[0].1.as_ref(), "hero");
}

#[test]
fn vt_names_multiple_elements_document_order() {
    let root = lay(
        "<div id='a'></div><div id='b'></div>",
        "#a { view-transition-name: first; width: 100px; height: 50px; } \
         #b { view-transition-name: second; width: 100px; height: 50px; }",
    );
    let names = collect_view_transition_names(&root);
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].1.as_ref(), "first");
    assert_eq!(names[1].1.as_ref(), "second");
}

#[test]
fn vt_names_none_value_excluded() {
    let root = lay(
        "<div></div>",
        "div { view-transition-name: none; width: 100px; height: 50px; }",
    );
    let names = collect_view_transition_names(&root);
    assert!(names.is_empty(), "view-transition-name:none should not appear");
}

// ── collect_view_transition_groups (name + border-box rect) ───────────────

#[test]
fn vt_groups_empty_without_property() {
    let root = lay("<div></div>", "div { width: 100px; height: 50px; }");
    assert!(collect_view_transition_groups(&root).is_empty());
}

#[test]
fn vt_groups_returns_border_box_rect() {
    // A named, absolutely-sized box at a known offset: the collector must
    // report its border-box rect (the geometry the morph animates from/to).
    let root = lay_viewport(
        "<div class='f'><div class='hero'></div></div>",
        ".f { width: 1022px; height: 718px; } \
         .hero { view-transition-name: hero; width: 200px; height: 120px; \
                 margin-left: 40px; margin-top: 30px; }",
        Size::new(1024.0, 720.0),
    );
    let groups = collect_view_transition_groups(&root);
    assert_eq!(groups.len(), 1, "one named element");
    let (_, ref name, rect) = groups[0];
    assert_eq!(name.as_ref(), "hero");
    // Border-box excludes margin: width/height are the content+border box,
    // x/y are the top-left after margin.
    assert!((rect.width - 200.0).abs() < 0.5, "width, got {}", rect.width);
    assert!((rect.height - 120.0).abs() < 0.5, "height, got {}", rect.height);
    assert!((rect.x - 40.0).abs() < 0.5, "x after margin-left, got {}", rect.x);
    assert!((rect.y - 30.0).abs() < 0.5, "y after margin-top, got {}", rect.y);
}

#[test]
fn vt_groups_document_order_and_duplicate_names() {
    // Two elements share the name "dup": the collector returns both in
    // document order; the shell keeps the first when pairing.
    let root = lay(
        "<div id='a'></div><div id='b'></div><div id='c'></div>",
        "#a { view-transition-name: dup; width: 100px; height: 50px; } \
         #b { view-transition-name: solo; width: 100px; height: 50px; } \
         #c { view-transition-name: dup; width: 100px; height: 50px; }",
    );
    let groups = collect_view_transition_groups(&root);
    let names: Vec<&str> = groups.iter().map(|(_, n, _)| n.as_ref()).collect();
    assert_eq!(names, ["dup", "solo", "dup"], "all occurrences, document order");
}

// BUG-130: view-transition-name must not affect normal-flow rendering — a box
// carrying the property lays out identically to a plain box (CSS View
// Transitions L1 §10; the property only marks elements for capture during
// document.startViewTransition()). Regression mirrors TEST-81: two equal boxes
// in a centered flex row, one named, one not — same y/size/height.
#[test]
fn vt_name_does_not_affect_layout_geometry() {
    let root = lay_viewport(
        "<div class='f'><div class='box plain'></div><div class='box named'></div></div>",
        ".f { display: flex; align-items: center; justify-content: center; gap: 60px; \
              width: 1022px; height: 718px; } \
         .box { width: 200px; height: 200px; } \
         .named { view-transition-name: hero; }",
        Size::new(1024.0, 720.0),
    );
    let flex = first_element_child(&root);
    let plain = &flex.children[0];
    let named = &flex.children[1];
    // align-items:center → both vertically centered at the same y in the 718px row.
    assert_eq!(plain.rect.y, named.rect.y, "named box must share the plain box y");
    assert_eq!(plain.rect.height, named.rect.height, "same height");
    assert_eq!(plain.rect.width, named.rect.width, "same width");
    // Centered cross-size: (718 - 200) / 2 = 259 (BUG-141), not pinned to row top.
    assert!(
        (plain.rect.y - 259.0).abs() < 0.5,
        "boxes centered on cross axis, got y={}",
        plain.rect.y
    );
}

// ──────────── CSS Overscroll Behavior L1 — scroll chain stop ────────────

#[test]
fn overscroll_collected_from_style() {
    let root = lay(
        "<div class='s'><div class='t'></div></div>",
        ".s { width: 100px; height: 100px; overflow: scroll; \
           overscroll-behavior-x: contain; overscroll-behavior-y: none; } \
         .t { width: 300px; height: 300px; }",
    );
    let containers = collect_scroll_containers(&root);
    let c = containers
        .iter()
        .find(|c| matches!(c.overscroll_behavior_x, style::OverscrollBehavior::Contain))
        .expect("scroll container with overscroll-behavior-x: contain");
    assert_eq!(c.overscroll_behavior_x, style::OverscrollBehavior::Contain);
    assert_eq!(c.overscroll_behavior_y, style::OverscrollBehavior::None);
}

#[test]
fn overscroll_auto_propagates_at_boundary() {
    use style::OverscrollBehavior::Auto;
    // At boundary (no movement), default `auto` lets the delta bubble up.
    assert!(overscroll_should_propagate(Auto, Auto, 0.0, 30.0, false, false));
    assert!(overscroll_should_propagate(Auto, Auto, 30.0, 0.0, false, false));
}

#[test]
fn overscroll_contain_blocks_propagation() {
    use style::OverscrollBehavior::{Auto, Contain, None};
    // Vertical delta at boundary with overscroll-behavior-y: contain stays put.
    assert!(!overscroll_should_propagate(Auto, Contain, 0.0, 30.0, false, false));
    // None behaves like contain for chain-stopping.
    assert!(!overscroll_should_propagate(None, Auto, 30.0, 0.0, false, false));
}

#[test]
fn overscroll_blocked_axis_only_matters_for_its_delta() {
    use style::OverscrollBehavior::{Auto, Contain};
    // contain on Y, but the delta is purely horizontal on an `auto` X axis →
    // the horizontal delta is free to propagate.
    assert!(overscroll_should_propagate(Auto, Contain, 30.0, 0.0, false, false));
    // contain on X but delta is vertical on `auto` Y → propagates.
    assert!(overscroll_should_propagate(Contain, Auto, 0.0, 30.0, false, false));
}

#[test]
fn overscroll_consumed_when_container_moves() {
    use style::OverscrollBehavior::Auto;
    // Any actual movement consumes the gesture — chain never reaches parent,
    // regardless of overscroll-behavior.
    assert!(!overscroll_should_propagate(Auto, Auto, 0.0, 30.0, false, true));
    assert!(!overscroll_should_propagate(Auto, Auto, 30.0, 30.0, true, false));
}

// CSS Overscroll Behavior L1 §3 — scroll chain across nested containers.
#[test]
fn scroll_chain_auto_at_boundary_hands_off_to_outer_container() {
    use style::OverscrollBehavior::Auto;
    let outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
    let mut inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
    // Inner is already at its bottom boundary (max = 400).
    inner.scroll_y = 400.0;
    inner.overscroll_behavior_y = Auto;
    let t = resolve_scroll_chain_target(&[outer, inner], 60.0, 60.0, 0.0, 30.0).unwrap();
    assert_eq!(t.node, lumen_dom::NodeId::from_index(1));
    assert!(t.moved);
    assert_eq!(t.new_y, 30.0);
}

#[test]
fn scroll_chain_contain_at_boundary_stops_on_inner_container() {
    use style::OverscrollBehavior::Contain;
    let outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
    let mut inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
    inner.scroll_y = 400.0;
    inner.overscroll_behavior_y = Contain;
    let t = resolve_scroll_chain_target(&[outer, inner], 60.0, 60.0, 0.0, 30.0).unwrap();
    assert_eq!(t.node, lumen_dom::NodeId::from_index(2));
    assert!(!t.moved);
}

#[test]
fn scroll_chain_all_auto_at_boundary_falls_through_to_viewport() {
    let mut outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
    outer.scroll_y = 400.0;
    let mut inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
    inner.scroll_y = 400.0;
    assert_eq!(resolve_scroll_chain_target(&[outer, inner], 60.0, 60.0, 0.0, 30.0), None);
}

#[test]
fn scroll_chain_moving_inner_wins_over_outer() {
    let outer = make_scroll_container(1, 0.0, 0.0, 200.0, 200.0);
    let inner = make_scroll_container(2, 50.0, 50.0, 50.0, 50.0);
    let t = resolve_scroll_chain_target(&[outer, inner], 60.0, 60.0, 0.0, 30.0).unwrap();
    assert_eq!(t.node, lumen_dom::NodeId::from_index(2));
    assert!(t.moved);
}

/// BUG-158: a `flex: 1` item (which sets `flex-basis: 0`) in an
/// indefinite-height column flex container must not collapse to height 0 —
/// CSS Flexbox §4.5 automatic minimum size keeps it at its content height.
///
/// The container is itself a flex item of a row-flex grandparent, so the row
/// flex lays the column out twice (preliminary + final pass). The first pass
/// writes a resolved px `height` back into the item's style; the regression
/// is that the second pass saw that stale `height` and re-collapsed the item
/// to 0, so sibling cards painted on top of each other (lenta.ru news cards).
#[test]
fn flex_column_basis_zero_item_keeps_content_height() {
    let body = lay_measured(
        "<div class=g>\
           <div class=col>\
             <div class=a>First card single line</div>\
             <div class=mid>Middle card has enough text to wrap onto two lines here ok</div>\
             <div class=b>Last card single line</div>\
           </div>\
         </div>",
        ".g { display: flex; } \
         .col { display: flex; flex-direction: column; width: 280px; } \
         .a, .b { flex: none; } \
         .mid { flex: 1; }",
        800.0,
    );

    let grand = body.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip)).unwrap();
    let col = grand.children.iter().find(|c| !matches!(c.kind, BoxKind::Skip)).unwrap();
    // (y, height) of each card, in source order.
    let cards: Vec<(f32, f32)> = col
        .children
        .iter()
        .filter(|c| !matches!(c.kind, BoxKind::Skip))
        .map(|c| (c.rect.y, c.rect.height))
        .collect();
    assert_eq!(cards.len(), 3, "expected 3 cards, got {}", cards.len());

    // The middle `flex: 1` card must keep a real content height, not collapse.
    assert!(
        cards[1].1 > 10.0,
        "middle flex:1 card collapsed to height {} (BUG-158)",
        cards[1].1
    );

    // Cards stack without overlap: each starts at the bottom edge of the
    // previous one (no two share a y, which is the painted symptom).
    assert!(
        (cards[1].0 - (cards[0].0 + cards[0].1)).abs() < 0.5,
        "card 1 (y={}) does not stack under card 0 (y={}, h={})",
        cards[1].0, cards[0].0, cards[0].1
    );
    assert!(
        (cards[2].0 - (cards[1].0 + cards[1].1)).abs() < 0.5,
        "card 2 (y={}) does not stack under card 1 (y={}, h={})",
        cards[2].0, cards[1].0, cards[1].1
    );
}

// ──────── BUG-728: геометрия не-inline потомка inline-элемента ────────

/// Первый в глубину бокс, удовлетворяющий предикату.
fn find_first<'a>(b: &'a LayoutBox, f: &dyn Fn(&LayoutBox) -> bool) -> Option<&'a LayoutBox> {
    if f(b) {
        return Some(b);
    }
    b.children.iter().find_map(|c| find_first(c, f))
}

#[test]
fn img_inside_inline_element_keeps_its_own_box() {
    // BUG-728: <img> внутри <span>/<a> уплощался в InlineSegment, у которого
    // нет высоты — картинка рисовалась 50×16.8 (высота строки) вместо 50×50.
    for html in [
        "<div><span><img src=\"a.png\"></span></div>",
        "<div><a href=\"#\"><img src=\"a.png\"></a></div>",
        "<div><span><span><img src=\"a.png\"></span></span></div>",
    ] {
        let root = lay_measured(html, "img { width: 50px; height: 50px; }", 800.0);
        let img = find_first(&root, &|b| matches!(b.kind, BoxKind::Image { .. }))
            .unwrap_or_else(|| panic!("нет Image-бокса для {html}"));
        assert!(
            (img.rect.width - 50.0).abs() < 0.5 && (img.rect.height - 50.0).abs() < 0.5,
            "{html}: картинка {}×{}, ожидалось 50×50",
            img.rect.width, img.rect.height
        );
    }
}

#[test]
fn block_child_of_inline_element_keeps_its_height() {
    // CSS 2.1 §9.2.1.1: блочный потомок разрезает inline-бокс и остаётся
    // блоком. До BUG-728 от него оставался текстовый прогон в одну строку.
    let root = lay_measured(
        "<div><span>a<div class=\"b\">bb</div>c</span></div>",
        ".b { display: block; height: 30px; }",
        800.0,
    );
    let b = find_first(&root, &|x| {
        x.style.display == Display::Block && (x.rect.height - 30.0).abs() < 0.5
    });
    assert!(b.is_some(), "блочный потомок inline-элемента потерял height: 30px");
    // Текст до и после блока — два разных анонимных прогона (разрез).
    fn count_runs(b: &LayoutBox) -> usize {
        usize::from(matches!(b.kind, BoxKind::InlineRun { .. }))
            + b.children.iter().map(count_runs).sum::<usize>()
    }
    assert_eq!(
        count_runs(&root), 3,
        "ожидались прогоны «a», «bb» и «c» по разные стороны блока"
    );
}

#[test]
fn flex_child_of_inline_element_stays_a_flex_container() {
    // Флекс-контейнер внутри <span> переставал быть контейнером: рекурсия
    // забирала из него только текст.
    let root = lay_measured(
        "<div><span><div class=\"f\"><i>x</i><i>y</i></div></span></div>",
        ".f { display: flex; height: 40px; } i { width: 20px; }",
        800.0,
    );
    let f = find_first(&root, &|b| b.style.display == Display::Flex)
        .expect("флекс-контейнер внутри inline-элемента не построен");
    assert!(
        (f.rect.height - 40.0).abs() < 0.5,
        "флекс-контейнер height {} вместо 40",
        f.rect.height
    );
}

#[test]
fn form_control_inside_inline_element_gets_a_box() {
    // <input> внутри <span> не эмитил вообще ничего — поле исчезало.
    let root = lay_measured("<div><span><input></span></div>", "", 800.0);
    let input = find_first(&root, &|b| matches!(b.kind, BoxKind::FormControl { .. }))
        .expect("FormControl-бокс внутри inline-элемента не построен");
    assert!(
        input.rect.width > 0.0 && input.rect.height > 0.0,
        "поле схлопнулось в {}×{}",
        input.rect.width, input.rect.height
    );
}

#[test]
fn escaped_child_inherits_from_the_inline_element_not_the_block() {
    // Бокс строится блочным контейнером, но наследовать обязан от <span>,
    // между ними стоящего, — иначе теряются цвет/шрифт inline-родителя.
    let root = lay_measured(
        "<div><span class=\"s\"><div class=\"b\">x</div></span></div>",
        ".s { color: rgb(1, 2, 3); } .b { display: block; }",
        800.0,
    );
    let b = find_first(&root, &|x| {
        x.style.display == Display::Block
            && x.style.color == crate::style::Color { r: 1, g: 2, b: 3, a: 255 }
    });
    assert!(b.is_some(), "блочный потомок не унаследовал color от <span>");
}

#[test]
fn escape_preserves_document_order_around_the_split() {
    // Разрез сохраняет порядок: текст до, всплывший бокс, текст после.
    // `<img>` внутри <span> — тот же escape-механизм BUG-728 (сегментом он
    // стать не может, у сегмента нет своей высоты), но с IFC-2 все три
    // куска остаются в ОДНОЙ строке: escape отдаёт бокс, а
    // `breaks_inline_row` не рвёт на нём ряд — display у картинки inline.
    let root = lay_measured(
        "<div><span>ab<img src=\"a.png\">cd</span></div>",
        "img { width: 50px; height: 50px; }",
        800.0,
    );
    let div = root.children.iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .expect("нет блока <div>");
    let row = div.children.iter()
        .find(|c| matches!(c.kind, BoxKind::InlineBlockRow))
        .expect("нет строки с картинкой");
    let kinds: Vec<&str> = row.children.iter().map(|c| match c.kind {
        BoxKind::InlineRun { .. } => "run",
        BoxKind::Image { .. } => "img",
        _ => "other",
    }).collect();
    assert_eq!(kinds, vec!["run", "img", "run"], "порядок кусков потока нарушен");
    let img = &row.children[1];
    assert!(
        (img.rect.height - 50.0).abs() < 0.5,
        "картинка height {} вместо 50", img.rect.height
    );
    assert!(
        row.children[0].rect.x < img.rect.x && img.rect.x < row.children[2].rect.x,
        "порядок по горизонтали нарушен: {} / {} / {}",
        row.children[0].rect.x, img.rect.x, row.children[2].rect.x
    );
    // CSS 2.1 §10.8.1 — базовая линия замещаемого элемента это нижняя
    // кромка его margin box, поэтому низ картинки и низ соседнего текста
    // расходятся не больше чем на descent строки, а не на её высоту.
    assert!(
        (img.rect.y + img.rect.height
            - (row.children[0].rect.y + row.children[0].rect.height)).abs() < 6.0,
        "картинка не села на базовую линию строки: низ {} против низа текста {}",
        img.rect.y + img.rect.height,
        row.children[0].rect.y + row.children[0].rect.height
    );
}

#[test]
fn display_contents_inside_inline_element_stays_flattened() {
    // `display: contents` бокса не порождает (CSS Display L3 §3.1) — его
    // дети остаются в inline-контексте родителя. Escape-механика BUG-728
    // не должна его выносить: иначе «cc dd ee» разъезжается на три строки.
    let root = lay_measured(
        "<div><span>cc<span class=\"c\">dd</span>ee</span></div>",
        ".c { display: contents; }",
        800.0,
    );
    let div = root.children.iter()
        .find(|c| matches!(c.kind, BoxKind::Block))
        .expect("нет блока <div>");
    assert_eq!(div.children.len(), 1, "inline-контекст разрезан на куски");
    assert!(matches!(div.children[0].kind, BoxKind::InlineRun { .. }));
}

#[test]
fn inline_block_inside_inline_element_stays_in_the_row() {
    // Inline-уровневый всплывший бокс ряд НЕ разрывает: он остаётся в том
    // же анонимном контейнере, что и текст вокруг (breaks_inline_row).
    let root = lay_measured(
        "<div><span>ff<span class=\"ib\"></span>gg</span></div>",
        ".ib { display: inline-block; width: 30px; height: 12px; }",
        800.0,
    );
    let row = find_first(&root, &|b| matches!(b.kind, BoxKind::InlineBlockRow))
        .expect("InlineBlockRow не построен");
    let kinds: Vec<&str> = row.children.iter().map(|c| match c.kind {
        BoxKind::InlineRun { .. } => "run",
        BoxKind::Block => "block",
        _ => "other",
    }).collect();
    assert_eq!(kinds, vec!["run", "block", "run"], "куски не попали в один ряд");
    let ib = &row.children[1];
    assert!(
        (ib.rect.width - 30.0).abs() < 0.5 && (ib.rect.height - 12.0).abs() < 0.5,
        "inline-block {}×{} вместо 30×12", ib.rect.width, ib.rect.height
    );
}

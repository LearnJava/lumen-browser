use lumen_core::geom::Size;

// ── CSS Fonts L5 §4: font-size-adjust ───────────────────────────────────

/// Measurer whose x-height is a fixed fraction of the size (aspect = 0.8),
/// emulating a tall font like Inter so `font-size-adjust` produces a visible
/// change.
struct AspectMeasurer(f32);
impl crate::TextMeasurer for AspectMeasurer {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size * 0.5
    }
    fn x_height_px(&self, size: f32) -> f32 {
        size * self.0
    }
}

#[test]
fn font_size_adjust_value_scales_down_for_tall_font() {
    use crate::style::{ComputedStyle, FontSizeAdjust};
    let m = AspectMeasurer(0.8); // font aspect 0.8
    let mut s = ComputedStyle::root();
    s.font_size = 100.0;
    s.font_size_adjust = FontSizeAdjust::Value(0.5);
    // used = 100 * 0.5 / 0.8 = 62.5
    let used = super::super::font_size_adjust_used(&s, &m);
    assert!((used - 62.5).abs() < 0.01, "expected 62.5, got {used}");
}

#[test]
fn font_size_adjust_value_scales_up_for_short_font() {
    use crate::style::{ComputedStyle, FontSizeAdjust};
    let m = AspectMeasurer(0.4); // short font, aspect 0.4
    let mut s = ComputedStyle::root();
    s.font_size = 100.0;
    s.font_size_adjust = FontSizeAdjust::Value(0.5);
    // used = 100 * 0.5 / 0.4 = 125.0
    let used = super::super::font_size_adjust_used(&s, &m);
    assert!((used - 125.0).abs() < 0.01, "expected 125.0, got {used}");
}

#[test]
fn font_size_adjust_none_and_auto_are_noops() {
    use crate::style::{ComputedStyle, FontSizeAdjust};
    let m = AspectMeasurer(0.8);
    let mut s = ComputedStyle::root();
    s.font_size = 40.0;
    s.font_size_adjust = FontSizeAdjust::None;
    assert_eq!(super::super::font_size_adjust_used(&s, &m), 40.0);
    s.font_size_adjust = FontSizeAdjust::Auto;
    assert_eq!(super::super::font_size_adjust_used(&s, &m), 40.0);
}

#[test]
fn step_line_height_rounds_up_to_multiple() {
    // CSS Rhythmic Sizing L1 §2 — raw 19.2 with step 24 → 24; 30 with step 24 → 48.
    assert!((super::super::step_line_height(19.2, 24.0) - 24.0).abs() < f32::EPSILON);
    assert!((super::super::step_line_height(30.0, 24.0) - 48.0).abs() < f32::EPSILON);
    // Exact multiple stays put.
    assert!((super::super::step_line_height(48.0, 24.0) - 48.0).abs() < f32::EPSILON);
}

#[test]
fn step_line_height_disabled_passthrough() {
    // step <= 0 disables the property — raw height unchanged.
    assert!((super::super::step_line_height(19.2, 0.0) - 19.2).abs() < f32::EPSILON);
    assert!((super::super::step_line_height(19.2, -5.0) - 19.2).abs() < f32::EPSILON);
}

#[test]
fn apply_font_size_adjust_rewrites_box_and_segments() {
    use crate::style::{ComputedStyle, FontSizeAdjust};
    let m = AspectMeasurer(0.8);
    // Block box with font-size-adjust holding an InlineRun child + segment.
    let mut seg_style = ComputedStyle::root();
    seg_style.font_size = 100.0;
    seg_style.font_size_adjust = FontSizeAdjust::Value(0.5);
    let seg = super::super::InlineSegment {
        text: "hi".into(),
        style: seg_style,
        pre_space: 0.0,
        post_space: 0.0,
        is_element_box: false,
        img_src: None,
        img_is_lazy: false,
        img_width: 0.0,
        forced_break: false,
        pseudo_kind: super::super::PseudoKind::None,
        source_node: lumen_dom::NodeId::from_index(0),
        source_char_offset: 0,
        bidi_level: 0,
    };
    let mut inline_style = ComputedStyle::root();
    inline_style.font_size = 100.0;
    inline_style.font_size_adjust = FontSizeAdjust::Value(0.5);
    let inline_box = super::super::LayoutBox {
        node: lumen_dom::NodeId::from_index(0),
        rect: super::super::Rect::new(0.0, 0.0, 0.0, 0.0),
        style: std::sync::Arc::new(inline_style),
        kind: super::super::BoxKind::InlineRun { segments: vec![seg], lines: vec![], first_line_style: None },
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: super::super::BoxOrigin::default(),
    };
    let mut root_style = ComputedStyle::root();
    root_style.font_size = 100.0;
    root_style.font_size_adjust = FontSizeAdjust::Value(0.5);
    let mut root = super::super::LayoutBox {
        node: lumen_dom::NodeId::from_index(0),
        rect: super::super::Rect::new(0.0, 0.0, 0.0, 0.0),
        style: std::sync::Arc::new(root_style),
        kind: super::super::BoxKind::Block,
        children: vec![inline_box],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: super::super::BoxOrigin::default(),
    };
    super::super::apply_font_size_adjust(&mut root, &m);
    assert!((root.style.font_size - 62.5).abs() < 0.01, "root not adjusted: {}", root.style.font_size);
    let child = &root.children[0];
    assert!((child.style.font_size - 62.5).abs() < 0.01, "inline box not adjusted: {}", child.style.font_size);
    if let super::super::BoxKind::InlineRun { segments, .. } = &child.kind {
        assert!((segments[0].style.font_size - 62.5).abs() < 0.01, "segment not adjusted: {}", segments[0].style.font_size);
    } else {
        panic!("expected InlineRun");
    }
}

/// BUG-212: an absolute `line-height` (`<length>`/`<percentage>`) must keep its
/// computed line box constant when `font-size-adjust` rescales the used
/// font-size. `line_height` is ratio-encoded (×font-size), so the ratio is
/// corrected inversely. Here `line-height: 100px` over font-size 60 with a tall
/// font (aspect 0.8) gives used size 60·0.5/0.8 = 37.5; the line box must remain
/// 100px, not collapse to 37.5·(100/60) = 62.5.
#[test]
fn font_size_adjust_keeps_absolute_line_height_fixed() {
    use crate::style::{ComputedStyle, FontSizeAdjust};
    let m = AspectMeasurer(0.8);
    let mut s = ComputedStyle::root();
    s.font_size = 60.0;
    s.line_height = 100.0 / 60.0; // `line-height: 100px` → ratio
    s.line_height_is_relative = false; // absolute length
    s.font_size_adjust = FontSizeAdjust::Value(0.5);
    let mut b = super::super::LayoutBox {
        node: lumen_dom::NodeId::from_index(0),
        rect: super::super::Rect::new(0.0, 0.0, 0.0, 0.0),
        style: std::sync::Arc::new(s),
        kind: super::super::BoxKind::Block,
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: super::super::BoxOrigin::default(),
    };
    super::super::apply_font_size_adjust(&mut b, &m);
    assert!((b.style.font_size - 37.5).abs() < 0.01, "used size {}", b.style.font_size);
    let line_box = b.style.font_size * b.style.line_height;
    assert!(
        (line_box - 100.0).abs() < 0.01,
        "absolute line-height must stay 100px, got {line_box}"
    );
}

/// BUG-212 counterpart: a relative `line-height` (unitless `<number>`) MUST
/// scale with the adjusted used font-size — the ratio is left untouched.
/// `line-height: 1.5` over used size 37.5 → line box 56.25.
#[test]
fn font_size_adjust_scales_relative_number_line_height() {
    use crate::style::{ComputedStyle, FontSizeAdjust};
    let m = AspectMeasurer(0.8);
    let mut s = ComputedStyle::root();
    s.font_size = 60.0;
    s.line_height = 1.5; // unitless number → relative
    s.line_height_is_relative = true;
    s.font_size_adjust = FontSizeAdjust::Value(0.5);
    let mut b = super::super::LayoutBox {
        node: lumen_dom::NodeId::from_index(0),
        rect: super::super::Rect::new(0.0, 0.0, 0.0, 0.0),
        style: std::sync::Arc::new(s),
        kind: super::super::BoxKind::Block,
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: super::super::BoxOrigin::default(),
    };
    super::super::apply_font_size_adjust(&mut b, &m);
    assert!((b.style.line_height - 1.5).abs() < 0.001, "ratio must be unchanged");
    let line_box = b.style.font_size * b.style.line_height;
    assert!(
        (line_box - 56.25).abs() < 0.01,
        "relative line-height must scale to 56.25, got {line_box}"
    );
}

// --- is_open_details ---

#[test]
fn is_open_details_false_without_open_attr() {
    let doc = lumen_html_parser::parse(r#"<details id="d"><summary>Q</summary>A</details>"#);
    let id = doc.find_by_id("d").expect("details element not found");
    assert!(!super::super::is_open_details(&doc, id), "closed <details> must return false");
}

#[test]
fn is_open_details_true_with_open_attr() {
    let doc = lumen_html_parser::parse(r#"<details id="d" open><summary>Q</summary>A</details>"#);
    let id = doc.find_by_id("d").expect("details element not found");
    assert!(super::super::is_open_details(&doc, id), "open <details> must return true");
}

#[test]
fn is_open_details_false_for_summary_child() {
    // is_open_details must be false for <summary>, not just any element without `open`.
    let doc = lumen_html_parser::parse(r#"<details id="d" open><summary id="s">Q</summary>A</details>"#);
    let s = doc.find_by_id("s").expect("summary not found");
    assert!(!super::super::is_open_details(&doc, s), "<summary> is never a details disclosure root");
}

// --- RP-1: percentage sizing in block flow ---
//
// Verifies CSS 2.1 §10 percentage resolution for block-level boxes in normal
// flow: width / horizontal & vertical margin / padding resolve against the
// containing block's *width*; height resolves against the containing block's
// *height* (only when definite, else `auto`).
mod rp1_percentage_sizing {
    use lumen_core::geom::Size;

    /// Lays out `<div class=p><div class=c></div></div>` and returns the inner
    /// child Block box (`.c`). Uses `layout_measured` so the inline/measurer
    /// path matches real rendering rather than the fallback.
    fn child_block(css: &str) -> super::super::super::LayoutBox {
        let html = r#"<div class="p"><div class="c"></div></div>"#;
        let doc = lumen_html_parser::parse(html);
        let sheet = lumen_css_parser::parse(css);
        let root = super::super::super::layout(&doc, &sheet, Size::new(800.0, 600.0));
        // The `.c` box is the deepest empty Block in the tree
        // (html > body > div.p > div.c). Pick the Block at maximum depth.
        fn deepest<'a>(
            b: &'a super::super::super::LayoutBox,
            depth: usize,
            best: &mut (usize, Option<&'a super::super::super::LayoutBox>),
        ) {
            if matches!(b.kind, super::super::super::BoxKind::Block)
                && b.children.is_empty()
                && depth > best.0
            {
                *best = (depth, Some(b));
            }
            for c in &b.children {
                deepest(c, depth + 1, best);
            }
        }
        let mut best = (0usize, None);
        deepest(&root, 0, &mut best);
        best.1.cloned().expect("child .c block not found")
    }

    #[test]
    fn percent_width_resolves_against_containing_block() {
        // Parent 400px, child width:50% → 200px border-box.
        let c = child_block(".p { width: 400px; } .c { width: 50%; }");
        assert!((c.rect.width - 200.0).abs() < 0.5, "width={}", c.rect.width);
    }

    #[test]
    fn percent_horizontal_margin_against_cb_width() {
        // margin: 0 10% in a 400px parent → 40px each side; child fills the
        // remaining 320px (auto width shrinks by the resolved margins). The
        // parent sits 8px in from the viewport edge (UA body margin), so the
        // child's left edge lands at 8 + 40 = 48px.
        let c = child_block(".p { width: 400px; } .c { margin: 0 10%; }");
        assert!((c.rect.x - 48.0).abs() < 0.5, "x={}", c.rect.x);
        assert!((c.rect.width - 320.0).abs() < 0.5, "width={}", c.rect.width);
    }

    #[test]
    fn percent_vertical_padding_against_cb_width() {
        // padding-top: 25% resolves against *width* (400px) → 100px, NOT height.
        let c = child_block(".p { width: 400px; } .c { padding-top: 25%; }");
        // content starts 100px below the child's top border edge.
        let pad = c.rect.height; // empty child: height == padding-top (content 0).
        assert!((pad - 100.0).abs() < 0.5, "padding-derived height={}", pad);
    }

    #[test]
    fn percent_height_auto_when_cb_height_indefinite() {
        // Parent has auto height → child height:50% computes to auto → 0 for an
        // empty box (no content), NOT 50% of viewport.
        let c = child_block(".p { width: 400px; } .c { height: 50%; }");
        assert!(c.rect.height < 0.5, "height should collapse to auto/0, got {}", c.rect.height);
    }

    #[test]
    fn percent_height_resolves_when_cb_height_definite() {
        // Parent height:300px (definite) → child height:50% → 150px.
        let c = child_block(".p { width: 400px; height: 300px; } .c { height: 50%; }");
        assert!((c.rect.height - 150.0).abs() < 0.5, "height={}", c.rect.height);
    }
}

// ── BUG-341 S4: incremental box-build differential tests ───────────────────
//
// `incremental_build_box` must reproduce a full `build_box` pass bit-for-bit
// for the same final state (mirroring the S3 cascade differential tests in
// `incremental.rs`), while actually skipping work — cloning untouched
// subtrees from `prev` instead of rebuilding them.

#[test]
fn box_build_hover_transition_matches_full_and_reuses_subset() {
    // A real interactive-state transition (hover moves between siblings)
    // must produce a box tree bit-identical to a full rebuild of the
    // post-transition state, while cloning the untouched `#unrelated`
    // subtree wholesale instead of rebuilding it.
    use lumen_dom::build_flat_tree;
    use crate::counters::{
        build_counter_style_registry, incremental_precompute_counters, precompute_counters,
        set_incremental_restyle, RestyleDelta,
    };
    use crate::style::{
        clear_interactive_state, restyle_root_set_for_state_change, set_interactive_state,
        ComputedStyle,
    };

    let html = r#"<ul id="menu">
            <li id="a" class="item">a</li>
            <li id="b" class="item">b</li>
            <li id="c" class="item">c</li>
        </ul>
        <div id="unrelated"><p>x</p><p>y</p><p>z</p></div>"#;
    let css = r#"
            .item { color: black; padding: 4px; }
            .item:hover { color: blue; }
            .item:hover + .item { color: green; }
        "#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let vp = Size::new(800.0, 600.0);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let registry = build_counter_style_registry(&sheet);

    let a = doc.find_by_id("a").expect("#a must exist");
    let b = doc.find_by_id("b").expect("#b must exist");

    // Baseline (state a hovered) — the "prev" tree for reuse, built in full.
    set_interactive_state(Some(a), None, None);
    let baseline_counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let mut prev_tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &baseline_counters, &registry, false, None,
    );

    // Reference: full rebuild after the transition (no reuse at all).
    set_interactive_state(Some(b), None, None);
    let full_after_counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let full_after_tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &full_after_counters, &registry, false, None,
    );

    // Incremental: real transition, conservative root-set, box-build reuse on.
    let state_index = crate::style::restyle_state_index(&doc, &sheet);
    let dirty_roots = restyle_root_set_for_state_change(&doc, Some(a), Some(b), &state_index);
    let delta = RestyleDelta {
        prev_styles: baseline_counters.styles().clone(),
        dirty_roots,
        content_dirty: crate::counters::ContentDirty::Nothing,
    };
    set_incremental_restyle(true);
    let incr_counters = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, delta);
    set_incremental_restyle(false);

    super::super::set_incremental_box_build(true);
    let _ = super::super::take_box_build_stats();
    let mut incr_tree = super::super::incremental_build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &incr_counters, &registry, false, &mut prev_tree,
    );
    let bb = super::super::take_box_build_stats();
    let (built, reused) = (bb.built, bb.reused);
    super::super::set_incremental_box_build(false);
    clear_interactive_state();

    // Compare via laid-out geometry, not `Debug` string equality: `ComputedStyle`
    // carries a `custom_props: HashMap<String, String>` (CSS custom properties),
    // and `HashMap`'s `Debug` prints entries in iteration order, which two
    // independently-computed (but content-equal) cascades need not share —
    // `collect_rects` sidesteps that non-determinism entirely (BUG-341 S4).
    let mut full_after_tree = full_after_tree;
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);
    super::super::lay_out(&mut incr_tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);
    super::super::lay_out(&mut full_after_tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);

    fn collect_rects(b: &super::super::LayoutBox, out: &mut Vec<(lumen_dom::NodeId, super::super::Rect)>) {
        out.push((b.node, b.rect));
        for c in &b.children {
            collect_rects(c, out);
        }
    }
    let mut ra = Vec::new();
    let mut rb = Vec::new();
    collect_rects(&incr_tree, &mut ra);
    collect_rects(&full_after_tree, &mut rb);
    assert_eq!(ra.len(), rb.len(), "box count must match a full rebuild");
    for ((na, xa), (nb, xb)) in ra.iter().zip(rb.iter()) {
        assert_eq!(na, nb, "node order must match a full rebuild");
        assert!(
            (xa.x - xb.x).abs() < 0.5 && (xa.y - xb.y).abs() < 0.5
                && (xa.width - xb.width).abs() < 0.5 && (xa.height - xb.height).abs() < 0.5,
            "rect mismatch for {na:?}: incremental {xa:?} vs full {xb:?}",
        );
    }
    assert!(
        reused > 0,
        "incremental box-build reused 0 subtrees — the untouched #unrelated \
         subtree should have been cloned wholesale, not rebuilt (built={built})",
    );
}

#[test]
fn box_build_node_change_disables_reuse_conservatively() {
    // A DOM class mutation is NOT `dom_content_stable` (BUG-341 S4):
    // box-build must fall back to a full rebuild everywhere (reused == 0),
    // never trusting style-equality alone for a content-affecting change.
    use lumen_dom::{build_flat_tree, NodeData};
    use crate::counters::{
        build_counter_style_registry, incremental_precompute_counters, precompute_counters,
        set_incremental_restyle, RestyleDelta,
    };
    use crate::style::{restyle_node_index, restyle_root_set_for_node_change, ComputedStyle, NodeChange};

    let css = r#"
            .item { color: black; }
            .item.active { color: blue; }
        "#;
    let html = r#"<ul id="menu">
            <li id="a" class="item">a</li>
            <li id="b" class="item">b</li>
        </ul>
        <div id="unrelated"><p>x</p></div>"#;
    let mut doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let vp = Size::new(800.0, 600.0);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let registry = build_counter_style_registry(&sheet);

    let baseline_counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let mut prev_tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &baseline_counters, &registry, false, None,
    );

    let a = doc.find_by_id("a").expect("#a must exist");
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(a).data {
        for attr in attrs.iter_mut() {
            if attr.name.local == "class" {
                attr.value = "item active".to_string();
            }
        }
    }

    let full_after_counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let full_after_tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &full_after_counters, &registry, false, None,
    );

    let node_index = restyle_node_index(&doc, &sheet);
    let dirty_roots =
        restyle_root_set_for_node_change(&doc, [(a, NodeChange::Attr("class"))], &node_index);
    let delta = RestyleDelta {
        prev_styles: baseline_counters.styles().clone(),
        dirty_roots,
        content_dirty: crate::counters::ContentDirty::Untracked,
    };
    set_incremental_restyle(true);
    let incr_counters = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, delta);
    set_incremental_restyle(false);

    super::super::set_incremental_box_build(true);
    let _ = super::super::take_box_build_stats();
    let mut incr_tree = super::super::incremental_build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &incr_counters, &registry, false, &mut prev_tree,
    );
    let reused = super::super::take_box_build_stats().reused;
    super::super::set_incremental_box_build(false);

    // See the hover-transition test above for why geometry (not `Debug`
    // string equality) is the correct comparison — `custom_props` HashMap
    // iteration order isn't guaranteed equal across independent cascades.
    let mut full_after_tree = full_after_tree;
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);
    super::super::lay_out(&mut incr_tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);
    super::super::lay_out(&mut full_after_tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);

    fn collect_rects(b: &super::super::LayoutBox, out: &mut Vec<(lumen_dom::NodeId, super::super::Rect)>) {
        out.push((b.node, b.rect));
        for c in &b.children {
            collect_rects(c, out);
        }
    }
    let mut ra = Vec::new();
    let mut rb = Vec::new();
    collect_rects(&incr_tree, &mut ra);
    collect_rects(&full_after_tree, &mut rb);
    assert_eq!(ra.len(), rb.len(), "box count must match a full rebuild");
    for ((na, xa), (nb, xb)) in ra.iter().zip(rb.iter()) {
        assert_eq!(na, nb, "node order must match a full rebuild");
        assert!(
            (xa.x - xb.x).abs() < 0.5 && (xa.y - xb.y).abs() < 0.5
                && (xa.width - xb.width).abs() < 0.5 && (xa.height - xb.height).abs() < 0.5,
            "rect mismatch for {na:?}: incremental {xa:?} vs full {xb:?}",
        );
    }
    assert_eq!(
        reused, 0,
        "a DOM class mutation (ContentDirty::Untracked) must never reuse a \
         box subtree — got {reused} reuses",
    );
}

// ── BUG-341 S33: grid probe-reuse tests ─────────────────────────────────────
//
// S32's general `(node, constraints)`-keyed cache was removed (see
// `CV_AUTO_TOUCHED`'s doc comment for the full history); `lay_out_grid`'s
// Step 4/5 now reuse a non-subgrid item's probe result directly instead of
// laying it out twice, unconditionally (no thread-local toggle to drive
// an "on"/"off" differential the way S32's tests did). These tests instead
// construct scenarios where a *wrong* reuse (bad translate, or serving a
// probe computed at the wrong scroll-relative position) would produce a
// visibly wrong result, and assert the correct one.

#[test]
fn grid_probe_reuse_repositions_second_row_correctly() {
    // Two auto-placed rows, the first with an explicit height. If the
    // second item's reused probe subtree (laid out at Step 4's temporary
    // y=0) were translated by the wrong delta, its final `rect.y` would
    // land somewhere other than exactly `row1 height + row-gap`.
    use lumen_dom::build_flat_tree;
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    let html = r#"<div class="grid"><div class="tall">first</div><div class="cell">second</div></div>"#;
    let css = r#"
            .grid { display: grid; grid-template-columns: 1fr; row-gap: 10px; width: 400px; }
            .tall { height: 200px; }
        "#;
    let vp = Size::new(800.0, 600.0);
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);
    let mut b = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    super::super::lay_out(&mut b, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);

    fn find_by_class<'a>(b: &'a super::super::LayoutBox, doc: &lumen_dom::Document, class: &str) -> Option<&'a super::super::LayoutBox> {
        if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
            && attrs.iter().any(|a| a.name.local == "class" && a.value.split_whitespace().any(|c| c == class))
        {
            return Some(b);
        }
        b.children.iter().find_map(|c| find_by_class(c, doc, class))
    }
    let second = find_by_class(&b, &doc, "cell").expect("second row item must exist");
    let first = find_by_class(&b, &doc, "tall").expect("first row item must exist");
    // Relative to the first row, not an absolute y — the default UA body
    // margin (8px) offsets everything and isn't this test's concern.
    let gap = second.rect.y - (first.rect.y + first.rect.height);
    assert!(
        (gap - 10.0).abs() < 0.5,
        "second row item must sit exactly row-gap (10px) below the first row, got {gap}",
    );
}

#[test]
fn grid_probe_reuse_refuses_a_content_visibility_auto_item() {
    // A grid item with `content-visibility: auto` sits in the second row,
    // far enough down to be skipped by `cv_should_skip` at its *real*
    // position — but not at Step 4's temporary probe position (y=0, always
    // "relevant"). If the probe result were wrongly reused for Step 5
    // instead of recomputed, `cv_should_skip` would never run at the real
    // y and nothing would be recorded as skipped.
    use lumen_dom::build_flat_tree;
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    crate::content_visibility::set_cv_scroll(0.0, 0.0);
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
    let html = r#"<div class="grid"><div class="tall">first</div><div class="cv">second</div></div>"#;
    let css = r#"
            .grid { display: grid; grid-template-columns: 1fr; width: 400px; }
            .tall { height: 1000px; }
            .cv { content-visibility: auto; }
        "#;
    // viewport 600 ⇒ bottom_bound = 600 * 1.5 = 900; row2 starts at 1000 > 900 ⇒ must skip.
    let vp = Size::new(800.0, 600.0);
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);
    let mut b = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    super::super::lay_out(&mut b, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);
    let skipped = crate::content_visibility::take_cv_skipped();
    assert!(
        !skipped.is_empty(),
        "the .cv item must be recomputed (not served from a y=0 probe) and skipped at its real position",
    );
}

#[test]
fn grid_fr_rows_fill_definite_container_height_not_just_content_leftover() {
    // BUG-277 срез 18: a `1fr 1fr` row template on a grid with a definite
    // content-box height must split that FULL height evenly between the
    // two rows — the fr tracks' own probed content height (used only as a
    // fallback for *indefinite*-height containers) must not be subtracted
    // from the available space before distributing it. Before the fix,
    // each row got only `(height - 2*content_height) / 2`, leaving a gap
    // of `2*content_height` unaccounted for at the container's bottom.
    use lumen_dom::build_flat_tree;
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    let html = r#"<div class="grid"><div class="cell">a</div><div class="cell">b</div></div>"#;
    let css = r#"
            .grid { display: grid; grid-template-columns: 1fr; grid-template-rows: 1fr 1fr; height: 220px; width: 400px; }
            .cell { display: flex; align-items: center; justify-content: center; }
        "#;
    let vp = Size::new(800.0, 600.0);
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);
    let mut b = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    super::super::lay_out(&mut b, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);

    fn find_all_by_class<'a>(b: &'a super::super::LayoutBox, doc: &lumen_dom::Document, class: &str, out: &mut Vec<&'a super::super::LayoutBox>) {
        if let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(b.node).data
            && attrs.iter().any(|a| a.name.local == "class" && a.value.split_whitespace().any(|c| c == class))
        {
            out.push(b);
        }
        for c in &b.children {
            find_all_by_class(c, doc, class, out);
        }
    }
    let mut cells = Vec::new();
    find_all_by_class(&b, &doc, "cell", &mut cells);
    assert_eq!(cells.len(), 2, "expected exactly 2 `.cell` grid items");
    for cell in &cells {
        assert!(
            (cell.rect.height - 110.0).abs() < 1.0,
            "each 1fr row of a 220px-tall grid must be ~110px, got {}",
            cell.rect.height,
        );
    }
    let gap = cells[1].rect.y - (cells[0].rect.y + cells[0].rect.height);
    assert!(
        gap.abs() < 1.0,
        "the two fr rows must be contiguous (no unaccounted leftover space), got gap {gap}",
    );
}

/// BUG-341 S16 gate: a text-only mutation must cost exactly the subtree that
/// contains it, and must not go stale.
///
/// Before S16 the whole document was declared content-unstable by a single
/// boolean, so this cycle rebuilt every box. The mechanism replacing it is
/// the *risky* half of the slice: if the mutation source under-reports, a
/// reused subtree keeps the old text — visible corruption, not a slow frame.
/// Hence both halves are asserted here, and in this order:
///
/// 1. **Correctness first** — every `InlineSegment`'s text in the
///    incremental tree equals a full rebuild's. This is what fails if
///    `ContentDirty` stops propagating up from the mutated text node.
/// 2. **Then the counter** — the untouched sibling subtree is cloned, and
///    the mutated node's own chain is not. A mechanism that reuses nothing
///    passes (1) perfectly and is simply the pre-S16 behaviour (S8's
///    lesson), so only a count can tell the two apart.
#[test]
fn box_build_text_mutation_reuses_everything_but_the_mutated_chain() {
    use lumen_dom::{build_flat_tree, NodeData};
    use crate::counters::{
        build_counter_style_registry, incremental_precompute_counters, precompute_counters,
        set_incremental_restyle, ContentDirty, RestyleDelta,
    };
    use crate::style::ComputedStyle;

    let html = r#"<div id="host"><p id="line">short</p></div>
        <div id="unrelated"><p>x</p><p>y</p><p>z</p></div>"#;
    let mut doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("p { color: black; }");
    let vp = Size::new(800.0, 600.0);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let registry = build_counter_style_registry(&sheet);

    let baseline_counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let mut prev_tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &baseline_counters, &registry, false, None,
    );

    // The mutation: text data only. No attribute, no structure, no style —
    // precisely the case the cascade is blind to.
    let line = doc.find_by_id("line").expect("#line must exist");
    let text_node = *doc
        .get(line)
        .children
        .first()
        .expect("#line must have a text child");
    match &mut doc.get_mut(text_node).data {
        NodeData::Text(s) => *s = "a considerably longer replacement string".to_owned(),
        other => panic!("expected a text node, got {other:?}"),
    }

    let full_after_counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let full_after_tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &full_after_counters, &registry, false, None,
    );

    // A complete content report: exactly the mutated text node. No cascade
    // dirty root at all — text cannot change selector matching.
    let content = std::collections::HashSet::from([text_node]);
    let delta = RestyleDelta {
        prev_styles: baseline_counters.styles().clone(),
        dirty_roots: std::collections::HashSet::new(),
        content_dirty: ContentDirty::Nodes(&content),
    };
    set_incremental_restyle(true);
    let incr_counters = incremental_precompute_counters(&doc, &sheet, vp, &flat, false, delta);
    set_incremental_restyle(false);

    super::super::set_incremental_box_build(true);
    let _ = super::super::take_box_build_stats();
    let incr_tree = super::super::incremental_build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &incr_counters, &registry, false, &mut prev_tree,
    );
    let bb = super::super::take_box_build_stats();
    super::super::set_incremental_box_build(false);

    fn collect_text(b: &super::super::LayoutBox, out: &mut Vec<String>) {
        if let super::super::BoxKind::InlineRun { segments, .. } = &b.kind {
            for seg in segments {
                out.push(seg.text.clone());
            }
        }
        for c in &b.children {
            collect_text(c, out);
        }
    }
    let mut incr_text = Vec::new();
    let mut full_text = Vec::new();
    collect_text(&incr_tree, &mut incr_text);
    collect_text(&full_after_tree, &mut full_text);
    assert_eq!(
        incr_text, full_text,
        "the incremental tree carries stale text — a subtree containing the mutated text \
         node was reused. `ContentDirty` must propagate from the text node up through every \
         ancestor's `children_clean`.",
    );
    assert!(
        incr_text.iter().any(|t| t.contains("considerably longer")),
            "sanity: the mutation must be visible in the rebuilt tree at all, got {incr_text:?}",
    );

    // The counter half: #unrelated is untouched, so it must come across as
    // a clone; the mutated chain must not.
    assert!(
        bb.reused > 0,
        "a text-only mutation must still let untouched subtrees (#unrelated) be cloned — \
         got {bb:?}. This is the whole point of S16: pre-S16 the document-wide flag made \
         this 0.",
    );
    assert!(
        !incr_counters.clean_subtrees().contains(&line),
        "#line contains the mutated text node and must not be marked clean",
    );
    assert!(
        !incr_counters.clean_subtrees().contains(&text_node),
        "the mutated text node itself must not be marked clean",
    );
}

// ── BUG-341 S36: layout-result cache differential tests ────────────────────
//
// Same mechanism S32 built and S33 removed (net-negative at the time),
// resurrected with `used_size_override` folded into the key (S34/S35
// established the precondition it needs — see `LayoutResultKey`'s own doc
// comment). Mirrors S32's own five differential tests, plus one new test
// for the override-collision hazard S36 exists to close.

/// Lays out `html`/`css` once with the cache enabled and once with it off;
/// asserts every box's rect matches within 0.5px and returns the cache
/// stats from the cached run.
fn cached_vs_uncached_geometry(html: &str, css: &str, vp: Size) -> super::super::LayoutResultCacheStats {
    use lumen_dom::build_flat_tree;
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);

    let mut uncached = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    super::super::lay_out(&mut uncached, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);

    let mut cached = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    super::super::set_layout_result_cache(true);
    super::super::lay_out(&mut cached, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false);
    let stats = super::super::take_layout_result_cache_stats();
    super::super::set_layout_result_cache(false);

    fn collect_rects(b: &super::super::LayoutBox, out: &mut Vec<(lumen_dom::NodeId, super::super::Rect)>) {
        out.push((b.node, b.rect));
        for c in &b.children {
            collect_rects(c, out);
        }
    }
    let mut ra = Vec::new();
    let mut rb = Vec::new();
    collect_rects(&cached, &mut ra);
    collect_rects(&uncached, &mut rb);
    assert_eq!(ra.len(), rb.len(), "box count must match the uncached pass");
    for ((na, xa), (nb, xb)) in ra.iter().zip(rb.iter()) {
        assert_eq!(na, nb, "node order must match the uncached pass");
        assert!(
            (xa.x - xb.x).abs() < 0.5 && (xa.y - xb.y).abs() < 0.5
                && (xa.width - xb.width).abs() < 0.5 && (xa.height - xb.height).abs() < 0.5,
            "rect mismatch for {na:?}: cached {xa:?} vs uncached {xb:?}",
        );
    }
    stats
}

#[test]
fn layout_result_cache_matches_uncached_on_nested_column_flex() {
    let html = r#"<div class="outer"><div class="mid"><div class="inner">
            some reasonably long text content so intrinsic sizing has real work to do
        </div></div></div>"#;
    let css = r#"
            .outer { display: flex; flex-direction: column; width: 400px; }
            .mid { display: flex; flex-direction: column; }
            .inner { display: flex; flex-direction: column; }
        "#;
    let stats = cached_vs_uncached_geometry(html, css, Size::new(800.0, 600.0));
    assert_eq!(stats.poisoned, 0, "no content-visibility:auto in this fixture, should never poison");
}

#[test]
fn layout_result_cache_matches_uncached_on_grid_probe_pass() {
    // S32 found this fixture (CSS Grid's "layout at temporary position to
    // get intrinsic height" probe, `probe_x`/`probe_y` above) was the one
    // natural case where the generic cache's `ptr_eq` guard held across
    // two calls to the same node. S33 subsequently gave that exact case
    // its own zero-overhead point-fix (`probe_reuse` above): the final
    // placement pass now repositions the probe's already-computed
    // `LayoutBox` directly instead of calling `lay_out` a second time, so
    // there is no second call left for *this* generic cache to intercept
    // — `hits == 0` here is S33's fix working as intended, not a S36
    // regression. Kept as a geometry-parity check (cache on vs. off must
    // still agree) rather than deleted, since it is still the closest
    // thing to a grid-side regression guard this cache has.
    let html = r#"<div class="grid"><div class="cell">
            some reasonably long text content so intrinsic sizing has real work to do
        </div></div>"#;
    let css = r#"
            .grid { display: grid; grid-template-columns: 1fr; width: 400px; }
        "#;
    let stats = cached_vs_uncached_geometry(html, css, Size::new(800.0, 600.0));
    assert_eq!(stats.poisoned, 0, "no content-visibility:auto in this fixture, should never poison");
}

#[test]
fn layout_result_cache_hits_on_verbatim_repeat_call() {
    // S33's grid point-fix means neither of the two fixtures above
    // exercises an actual hit (flex items never repeat the same style
    // `Arc` within one `lay_out_flex` call per S32; grid's own probe/final
    // pair is now short-circuited before it reaches this cache at all).
    // Proves the hit path itself — `Arc::ptr_eq` match,
    // `translate_subtree` repositioning — is not simply dead code: two
    // back-to-back `lay_out` calls on the very same box with unchanged
    // style/constraints must be a hit, and the repositioned result must
    // land at the second call's `(start_x, start_y)`, not the first's.
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    use lumen_dom::build_flat_tree;

    let html = "<div>hello there</div>";
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("div { width: 200px; }");
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let vp = Size::new(800.0, 600.0);
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);

    let mut tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    let div = tree.children.last_mut().expect("html > body > div");
    let div = div.children.last_mut().expect("body > div");

    // Independent baseline: what an uncached call directly at (30, 40)
    // produces (may not itself be (30, 40) — the div's own margin/border
    // offsets the border-box origin from the call's `start_x`/`start_y`).
    let mut baseline_tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    let baseline_div = baseline_tree.children.last_mut().expect("html > body > div");
    let baseline_div = baseline_div.children.last_mut().expect("body > div");
    super::super::lay_out(baseline_div, 30.0, 40.0, 200.0, None, None, vp, init_pcb, &null_hp, false);

    super::super::set_layout_result_cache(true);
    super::super::lay_out(div, 0.0, 0.0, 200.0, None, None, vp, init_pcb, &null_hp, false);
    super::super::lay_out(div, 30.0, 40.0, 200.0, None, None, vp, init_pcb, &null_hp, false);
    let stats = super::super::take_layout_result_cache_stats();
    super::super::set_layout_result_cache(false);

    assert_eq!(stats, super::super::LayoutResultCacheStats { hits: 1, misses: 1, poisoned: 0 });
    assert!(
        (div.rect.x - baseline_div.rect.x).abs() < 0.5
            && (div.rect.y - baseline_div.rect.y).abs() < 0.5
            && (div.rect.width - baseline_div.rect.width).abs() < 0.5
            && (div.rect.height - baseline_div.rect.height).abs() < 0.5,
        "a hit must translate the cached subtree to match an uncached call at the same origin: \
         cached {:?} vs baseline {:?}",
        div.rect, baseline_div.rect,
    );
}

#[test]
fn layout_result_cache_matches_uncached_when_style_mutates_between_calls() {
    let html = r#"<div class="row"><div class="item">hello there</div></div>"#;
    let css = r#"
            .row { display: flex; width: 400px; }
            .item { flex-basis: auto; width: 120px; }
        "#;
    let stats = cached_vs_uncached_geometry(html, css, Size::new(800.0, 600.0));
    assert_eq!(stats.poisoned, 0, "no content-visibility:auto in this fixture, should never poison");
}

#[test]
fn layout_result_cache_refuses_to_cache_a_content_visibility_auto_subtree() {
    crate::content_visibility::set_cv_scroll(0.0, 0.0);
    crate::content_visibility::set_cv_relevant(std::collections::HashSet::new());
    let html = r#"<div class="outer"><div class="cv">short text</div></div>"#;
    let css = r#"
            .outer { display: flex; flex-direction: column; width: 400px; }
            .cv { content-visibility: auto; }
        "#;
    let stats = cached_vs_uncached_geometry(html, css, Size::new(800.0, 600.0));
    assert_eq!(stats.hits, 0, "a content-visibility:auto subtree must never be served from cache");
    assert!(stats.poisoned > 0, "the .cv node's own call must be recorded as poisoned, not silently cached");
    crate::content_visibility::take_cv_skipped();
}

#[test]
fn layout_result_cache_refuses_when_subgrid_context_is_active() {
    use crate::subgrid::{SubgridContext, SubgridContextGuard};
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    use lumen_dom::build_flat_tree;
    let html = "<div id=\"a\"></div>";
    let doc = lumen_html_parser::parse(html);
    let flat = build_flat_tree(&doc);
    let sheet = lumen_css_parser::parse("");
    let root_style = ComputedStyle::root();
    let vp = Size::new(800.0, 600.0);
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let b = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    let _guard = SubgridContextGuard::set(
        Some(SubgridContext::from_parent_tracks(&[100.0], 0.0)),
        None,
    );
    assert!(
        !super::super::cacheable_for_layout_result_cache(&b),
        "must refuse to cache while a subgrid context is active",
    );
}

#[test]
fn layout_result_cache_key_distinguishes_used_size_override_from_plain_probe() {
    // BUG-341 S36's whole reason to exist: `lay_out`'s Step-1 probe and
    // `lay_out_with_used_size`'s final pass can land on the identical
    // `(node, width, height)` key with the identical style `Arc` (S34) —
    // the key must still treat them as different cache slots, or the
    // final pass would silently be served the probe's un-overridden
    // geometry. Drives both wrappers directly on the same node/width so
    // every other key field matches too; `#item`'s declared `width: 400px`
    // makes the plain-probe result deterministic without relying on
    // block-auto-width resolution semantics.
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    use lumen_dom::build_flat_tree;

    let html = r#"<div id="item">hello there</div>"#;
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("#item { width: 400px; }");
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let vp = Size::new(800.0, 600.0);
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);
    let item_node = doc.find_by_id("item").expect("#item exists");

    fn find_mut(b: &mut super::super::LayoutBox, node: lumen_dom::NodeId) -> &mut super::super::LayoutBox {
        fn contains(b: &super::super::LayoutBox, node: lumen_dom::NodeId) -> bool {
            b.node == node || b.children.iter().any(|c| contains(c, node))
        }
        if b.node == node {
            return b;
        }
        for c in &mut b.children {
            if contains(c, node) {
                return find_mut(c, node);
            }
        }
        panic!("node not found in tree");
    }

    let mut tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );

    super::super::set_layout_result_cache(true);
    super::super::lay_out(
        find_mut(&mut tree, item_node),
        0.0, 0.0, 400.0, None, None, vp, init_pcb, &null_hp, false,
    );
    super::super::lay_out_with_used_size(
        find_mut(&mut tree, item_node),
        0.0, 0.0, 400.0, None, None, vp, init_pcb, &null_hp, false,
        super::super::UsedSizeOverride { width: Some(250.0), height: None, box_sizing: None },
    );
    let stats = super::super::take_layout_result_cache_stats();
    super::super::set_layout_result_cache(false);

    assert_eq!(
        stats.hits, 0,
        "the override call must never be served from the plain probe's cache entry \
         (same node/width/height/style, different used_size_override) — got {stats:?}",
    );
    assert_eq!(stats.misses, 2, "both calls must independently compute — got {stats:?}");

    let item = find_mut(&mut tree, item_node);
    assert!(
        (item.rect.width - 250.0).abs() < 0.5,
        "the used-size override must actually apply, not fall through to the probe's cached \
         un-overridden width — got {:?}",
        item.rect,
    );
}

/// BUG-341 S37 — S36 left one question unattempted: "profiling which piece
/// of the per-call fixed cost dominates (key construction vs. HashMap
/// lookup vs. clone-on-insert)". Isolated microbench per
/// `docs/perf-method.md`'s "ask price from a separate microbench before
/// operating the whole path" rule, instead of re-running the whole
/// `lay_out_cache_checked` A/B (already done three times, S32/S33/S36) to
/// answer a question about one internal piece of it.
///
/// Run: `cargo test -p lumen-layout --profile dev-release
/// box_tree::tests::bug341_differential::bug341_s37_cache_fixed_cost_breakdown
/// -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf measurement (BUG-341 S37) — see doc comment for run command"]
fn bug341_s37_cache_fixed_cost_breakdown() {
    use crate::counters::{build_counter_style_registry, precompute_counters};
    use crate::style::ComputedStyle;
    use lumen_dom::build_flat_tree;
    use std::collections::HashMap;
    use std::hint::black_box;
    use std::sync::Arc;
    use std::time::Instant;

    // A moderately nested flex row of 20 items, each with two text-bearing
    // spans — representative subtree size for one `lay_out_flex` item (not
    // the whole ~2300-node chrome document S36 measured the wall-clock A/B
    // on), so the clone benchmark below pays a realistic depth/fan-out, not
    // a single leaf box.
    let mut html = String::from(r#"<div class="row">"#);
    for i in 0..20 {
        html.push_str(&format!(
            r#"<div class="item"><span class="a">label {i}</span><span class="b">value {i}</span></div>"#
        ));
    }
    html.push_str("</div>");
    let css = r#"
        .row { display: flex; }
        .item { display: flex; flex-direction: column; padding: 4px; }
    "#;
    let vp = Size::new(800.0, 600.0);
    let doc = lumen_html_parser::parse(&html);
    let sheet = lumen_css_parser::parse(css);
    let flat = build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let counters = precompute_counters(&doc, &sheet, vp, &flat, false);
    let registry = build_counter_style_registry(&sheet);
    let null_hp = lumen_core::ext::NullHyphenationProvider;
    let init_pcb = super::super::Rect::new(0.0, 0.0, vp.width, vp.height);

    let mut tree = super::super::build_box(
        &doc, &sheet, doc.root(), &root_style, vp, &flat, &counters, &registry, false, None,
    );
    super::super::lay_out(
        &mut tree, 0.0, 0.0, vp.width, Some(vp.height), None, vp, init_pcb, &null_hp, false,
    );
    let body = tree.children.last().expect("html > body");
    let row = body.children.last().expect("body > .row").clone();

    fn count_nodes(b: &super::super::LayoutBox) -> usize {
        1 + b.children.iter().map(count_nodes).sum::<usize>()
    }
    let subtree_nodes = count_nodes(&row);
    let style = Arc::clone(&row.style);

    const N: usize = 50_000;

    // (a) key construction alone: 11 fields, 8 of them `f32::to_bits`.
    let t = Instant::now();
    let mut acc = 0u64;
    for i in 0..N {
        let key = super::super::LayoutResultKey {
            node: row.node,
            width_bits: (i as f32).to_bits(),
            height_bits: Some((i as f32).to_bits()),
            viewport_w_bits: vp.width.to_bits(),
            viewport_h_bits: vp.height.to_bits(),
            pcb_x_bits: 0.0f32.to_bits(),
            pcb_y_bits: 0.0f32.to_bits(),
            pcb_w_bits: vp.width.to_bits(),
            pcb_h_bits: vp.height.to_bits(),
            in_block_flow: false,
            measurer_ptr: 0,
            hp_ptr: i,
            used_size_override: super::super::UsedSizeOverrideBits::from(None),
        };
        acc = acc.wrapping_add(black_box(key).width_bits as u64);
    }
    black_box(acc);
    let key_ns = t.elapsed().as_nanos() as f64;

    // (b) HashMap lookup against a realistically populated map (200 distinct
    // keys — same order of magnitude as a handful of hundred live entries,
    // well under the ~4 260 S36 measured on the full chrome document but
    // enough to exercise real hashing/bucket-walk, not an empty map),
    // alternating hit/miss.
    let mut map: HashMap<super::super::LayoutResultKey, super::super::LayoutResultEntry> =
        HashMap::new();
    let mut sample_keys = Vec::new();
    for i in 0..200u32 {
        let key = super::super::LayoutResultKey {
            node: row.node,
            width_bits: (i as f32).to_bits(),
            height_bits: None,
            viewport_w_bits: vp.width.to_bits(),
            viewport_h_bits: vp.height.to_bits(),
            pcb_x_bits: 0.0f32.to_bits(),
            pcb_y_bits: 0.0f32.to_bits(),
            pcb_w_bits: vp.width.to_bits(),
            pcb_h_bits: vp.height.to_bits(),
            in_block_flow: false,
            measurer_ptr: 0,
            hp_ptr: 0,
            used_size_override: super::super::UsedSizeOverrideBits::from(None),
        };
        map.insert(
            key,
            super::super::LayoutResultEntry {
                style: Arc::clone(&style),
                start_x: 0.0,
                start_y: 0.0,
                result: row.clone(),
            },
        );
        sample_keys.push(key);
    }
    let miss_key = super::super::LayoutResultKey { width_bits: 999_999, ..sample_keys[0] };
    let t = Instant::now();
    let mut found = 0u64;
    for i in 0..N {
        let k = if i % 2 == 0 { &sample_keys[i % sample_keys.len()] } else { &miss_key };
        if black_box(map.get(k)).is_some() {
            found += 1;
        }
    }
    black_box(found);
    let lookup_ns = t.elapsed().as_nanos() as f64;

    // (c) clone-on-insert: the two clones `lay_out_cache_checked` pays on
    // every miss it decides to cache — `Arc::clone(&b.style)` (refcount
    // bump) and `b.clone()` (the whole `LayoutBox` subtree, recursing
    // through every descendant's `Vec<LayoutBox>`).
    let t = Instant::now();
    for _ in 0..N {
        black_box(Arc::clone(&style));
    }
    let arc_clone_ns = t.elapsed().as_nanos() as f64;

    const N_SUBTREE: usize = 5_000; // fewer reps: this arm is the expensive one
    let t = Instant::now();
    for _ in 0..N_SUBTREE {
        black_box(row.clone());
    }
    let subtree_clone_ns = t.elapsed().as_nanos() as f64;

    eprintln!(
        "[s37] subtree_nodes={subtree_nodes} N={N} N_SUBTREE={N_SUBTREE}\n\
         [s37] key_construction   {:>8.1} ns/call\n\
         [s37] hashmap_lookup     {:>8.1} ns/call (200-entry map, 50% hit)\n\
         [s37] arc_style_clone    {:>8.1} ns/call\n\
         [s37] subtree_clone      {:>8.1} ns/call  ({:.2} ns/node)",
        key_ns / N as f64,
        lookup_ns / N as f64,
        arc_clone_ns / N as f64,
        subtree_clone_ns / N_SUBTREE as f64,
        subtree_clone_ns / N_SUBTREE as f64 / subtree_nodes as f64,
    );
}

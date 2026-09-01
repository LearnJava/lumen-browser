//! Перерасчёт chrome-документа: CC-9 salvage, CC-12 и срезы BUG-341,
//! проверяющие, что взаимодействие переиспользует боксы, а не строит заново.

use super::*;

// в”Ђв”Ђ CC-9: #contentArea pruning salvages #findBar/#downloadsPanel в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

#[test]
fn take_content_area_salvages_find_bar_and_downloads_panel() {
    let html = concat!(
        "<html><body>",
        "<div id=\"before\"></div>",
        "<div id=\"contentArea\">",
        "<div id=\"placeholder\">demo</div>",
        "<div id=\"findBar\">find</div>",
        "<div id=\"downloadsPanel\">downloads</div>",
        "</div>",
        "<div id=\"after\"></div>",
        "</body></html>",
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let mut layout = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let content_area = doc.find_by_id("contentArea").expect("fixture has #contentArea");

    let pruned = take_content_area(&mut layout, content_area, &["findBar", "downloadsPanel"], &doc);
    assert!(pruned.is_some(), "must return #contentArea's own rect");

    assert!(
        lumen_layout::find_box_by_node(&layout, content_area).is_none(),
        "#contentArea's own box must be gone"
    );
    let placeholder = doc.find_by_id("placeholder").expect("fixture has #placeholder");
    assert!(
        lumen_layout::find_box_by_node(&layout, placeholder).is_none(),
        "non-salvaged descendants of #contentArea must be discarded"
    );

    let find_bar = doc.find_by_id("findBar").expect("fixture has #findBar");
    let downloads_panel = doc.find_by_id("downloadsPanel").expect("fixture has #downloadsPanel");
    assert!(lumen_layout::find_box_by_node(&layout, find_bar).is_some(), "#findBar must be salvaged");
    assert!(
        lumen_layout::find_box_by_node(&layout, downloads_panel).is_some(),
        "#downloadsPanel must be salvaged"
    );

    // Salvaged boxes must land at #contentArea's former slot, in document
    // order, as direct children of #contentArea's former parent (<body>)
    // вЂ” not nested under some other unrelated box.
    let body_box = lumen_layout::find_box_by_node(&layout, doc.body().expect("fixture has <body>"))
        .expect("<body> must have a layout box");
    let before = doc.find_by_id("before").expect("fixture has #before");
    let after = doc.find_by_id("after").expect("fixture has #after");
    let order: Vec<NodeId> = body_box.children.iter().map(|b| b.node).collect();
    assert_eq!(order, vec![before, find_bar, downloads_panel, after]);
}

// в”Ђв”Ђ CC-11: chrome document gets its own Animation/Transition scheduler в”Ђв”Ђ

#[test]
fn chrome_transition_scheduler_stays_independent_of_page_scheduler_for_same_node_id() {
    // chrome_doc and the page Document each number NodeIds from 0
    // independently (see Lumen::chrome_animation_scheduler's doc
    // comment) вЂ” this proves two separate TransitionScheduler instances
    // don't let one tree's transition state leak into the other's frame
    // when driven with a colliding NodeId. A single shared scheduler
    // would fail this test (the page's transition would also appear in
    // the chrome frame).
    let node = lumen_dom::NodeId::from_index(3usize);

    let mut page_sched = TransitionScheduler::new();
    let mut chrome_sched = TransitionScheduler::new();

    let mut old = lumen_layout::ComputedStyle::root();
    old.opacity = 0.0;
    old.transition_properties = vec!["opacity".to_string()];
    old.transition_durations = vec![1.0];
    old.transition_timing_functions = vec![lumen_layout::TimingFunction::Linear];
    let mut new = old.clone();
    new.opacity = 1.0;

    // Only the page's #box transitions opacity 0 в†’ 1 at t=0; chrome's own
    // node with the same NodeId is never synced (nothing changed there).
    page_sched.sync(node, &old, &new, 0.0);
    let chrome_frame = chrome_sched.tick(0.5);
    assert!(
        !chrome_frame.overrides.contains_key(&node),
        "chrome scheduler must not see the page's transition for the same NodeId"
    );

    let page_frame = page_sched.tick(0.5);
    let op = page_frame.overrides[&node]
        .opacity
        .expect("page transition must be active at t=0.5");
    assert!((op - 0.5).abs() < 0.01, "expected ~0.5 midpoint, got {op}");
}

/// CC-12 (docs/tasks/p1-css-chrome.md): perf gate for the chrome
/// document's full restyle-on-every-interaction cost. Deliberately
/// headless/CPU-only rather than driven through the live `LUMEN_BENCH`
/// harness (`bench_frames.rs`) вЂ” that harness measures a whole GPU frame,
/// but the brief's "РјСѓС‚Р°С†РёСЏ в†’ СЂРµСЃС‚Р°Р№Р» в†’ СЂРµР»СЌР№Р°СѓС‚ в†’ paint" cycle is
/// exactly `Lumen::relayout_chrome_host`'s body, which never touches the
/// GPU (paint here means display-list build, not rasterization). Timing
/// it directly gives a deterministic, GPU-independent number instead of
/// bench_frames.rs's own cautionary tale (its doc comment: a whole-frame
/// sample folded an unrelated cost into the number it was aimed at).
///
/// `#[ignore]`d like `LUMEN_BENCH` itself is opt-in вЂ” a wall-clock budget
/// assert doesn't belong in the default `cargo test -p lumen-shell` gate
/// (shared-runner contention would make it flaky there); run explicitly:
/// `cargo test -p lumen-shell --profile dev-release cc12_chrome_perf_gate -- --ignored --nocapture`.
///
/// Was red (measured p50 в‰€ 580-630ms, ~300Г— over the 2ms budget) before
/// BUG-341's fixes вЂ” see [BUG-341](../../../bugs/BUG-341-OPEN.md) for the
/// full history: `lay_out_flex`'s double layout pass (fixed, ~86ms),
/// `bind_model`'s list rebuilds churning NodeIds every call (fixed), the
/// S3 incremental cascade + S5 pipeline wiring below (this bench now
/// takes the same `layout_mutation_incremental_restyle` path
/// `relayout_chrome_host` does). CC12_HOVER's own SIDEBAR/`None` toggle
/// is a documented S3 worst case (`:hover` invalidates every ancestor of
/// both the old and new target on a "nothing hovered" transition, which
/// this fixture hits every other cycle) вЂ” see BUG-341 "S3" for why a
/// representative sibling-to-sibling hover move fares much better, and
/// `bug341_s5_incremental_pipeline_share` below for that number. Still
/// red at S5 вЂ” see BUG-341 "S5" for the re-measured numbers.
#[test]
#[ignore = "manual perf gate (CC-12) вЂ” see BUG-341; doc comment has the run command"]
fn cc12_chrome_perf_gate_hover_and_keystroke_cycles() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let hover_target = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    const WARMUP: usize = 10;
    const SAMPLES: usize = 60;
    const BUDGET_MS: f32 = 2.0;

    let mut hover_stats = lumen_paint::FrameStats::new();
    let mut hover_state = Cc12IncrementalState::default();
    for i in 0..WARMUP + SAMPLES {
        let hover = if i % 2 == 0 { hover_target } else { None };
        let model = cc12_bench_model("");
        let (ms, _) =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, hover, &mut hover_state);
        if i >= WARMUP {
            hover_stats.record(ms as f32);
        }
    }
    let hover_summary = hover_stats.summary().expect("samples collected");
    eprintln!("{}", hover_summary.display_with("CC12_HOVER"));

    let mut key_stats = lumen_paint::FrameStats::new();
    let mut key_state = Cc12IncrementalState::default();
    let mut typed = String::new();
    for i in 0..WARMUP + SAMPLES {
        typed.push('a');
        if typed.len() > 40 {
            typed.clear();
        }
        let model = cc12_bench_model(&typed);
        let (ms, _) =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, None, &mut key_state);
        if i >= WARMUP {
            key_stats.record(ms as f32);
        }
    }
    let key_summary = key_stats.summary().expect("samples collected");
    eprintln!("{}", key_summary.display_with("CC12_KEY"));

    assert!(
        hover_summary.p95_ms < BUDGET_MS,
        "hover-flip p95 {:.3}ms exceeds {BUDGET_MS}ms budget вЂ” see BUG-341 \"S5\" for \
             the current numbers and the open follow-up (S6) needed to close this",
        hover_summary.p95_ms,
    );
    assert!(
        key_summary.p95_ms < BUDGET_MS,
        "keystroke p95 {:.3}ms exceeds {BUDGET_MS}ms budget вЂ” see BUG-341 \"S5\" for \
             the current numbers and the open follow-up (S6) needed to close this",
        key_summary.p95_ms,
    );
}

/// BUG-341 S8 regression gate: `graft_geometry` must reuse the **whole**
/// chrome box tree when two consecutive layouts see an identical document.
///
/// This is the test whose absence let two independent defects sit in the
/// incremental-layout path unnoticed through slices S1-S7, while every
/// differential test stayed green (they assert `incremental == full`
/// *output*, which a graft that reuses nothing also satisfies вЂ” just
/// slowly). Both are described in BUG-341 "S8": `kind_layout_eq` was
/// missing 6 of `BoxKind`'s 20 variants (every SVG kind among them, and
/// chrome is built out of SVG icons), and `graft_geometry` returned before
/// recursing whenever one node's style differed вЂ” which the root box does
/// on every single cycle, because `lay_out` writes the used viewport
/// `height` back into it. Either one alone drove reuse to zero.
///
/// Asserting the *count* rather than the output is the point: a reuse
/// regression is invisible in geometry and only shows up as wall-clock,
/// where machine noise (В±10-15% on this project's reference machine) hides
/// it. Keep this test exact вЂ” "most boxes clean" is not a useful contract.
#[test]
fn graft_geometry_reuses_whole_chrome_tree_when_nothing_changed() {
    use lumen_layout::incremental::{graft_geometry, mark_subtree_dirty, DirtyBits};

    fn count(b: &lumen_layout::box_tree::LayoutBox, clean: &mut usize, total: &mut usize) {
        *total += 1;
        if b.dirty == DirtyBits::CLEAN {
            *clean += 1;
        }
        for c in &b.children {
            count(c, clean, total);
        }
    }

    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    lumen_chrome::bind_model(&mut doc, &cc12_bench_model(""));

    let (prev, _) =
        lumen_layout::layout_measured_hyp_with_counters(&doc, &sheet, viewport, &measurer, &hyp, false);
    let (mut next, _) =
        lumen_layout::layout_measured_hyp_with_counters(&doc, &sheet, viewport, &measurer, &hyp, false);

    mark_subtree_dirty(&mut next);
    let all_clean = graft_geometry(&mut next, &prev);
    let (mut clean, mut total) = (0usize, 0usize);
    count(&next, &mut clean, &mut total);

    assert!(total > 100, "chrome document should produce a non-trivial box tree, got {total}");
    assert_eq!(
        clean, total,
        "graft_geometry reused only {clean}/{total} boxes of an unchanged chrome document вЂ”              every box must be reusable when nothing changed (BUG-341 S8). A drop here means some              `BoxKind` variant is missing from `kind_layout_eq`, or a layout pass writes a used              value back into `ComputedStyle` that the freshly-built tree cannot match.",
    );
    assert!(all_clean, "graft_geometry must report the whole tree clean when nothing changed");
}

/// BUG-341 S13 regression gate: a hover flip must not force boxes back
/// through layout merely because the *previous* pass wrote its own used
/// values into their styles.
///
/// `prev` is a laid-out tree: `lay_out_flex` overwrites each flex item's
/// `width`/`height`/`box_sizing` with the resolved used value, and the
/// post-layout passes rewrite more. The freshly-built tree carries none of
/// that, so a naive style comparison called 81 of this document's 318 boxes
/// "changed" вЂ” every one of them differing *only* in those fields вЂ” and
/// dragged 41 ancestors along, because a graft reject propagates upwards.
/// 122 boxes re-laid-out per interaction, none of them actually changed.
///
/// Gated on the **count**, like its S8 predecessor above and for the same
/// reason: geometry is identical either way, so only wall-clock would show
/// this, and machine noise (В±10-15%) hides it. The
/// `reject_style_used_value_only` assert is the load-bearing one вЂ” it fails
/// the moment a layout pass starts writing a used value the graft cannot
/// account for, whatever the stylesheet happens to contain.
#[test]
fn bug341_s13_hover_flip_reuses_boxes_the_layout_pass_only_wrote_used_values_into() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let model = cc12_bench_model("");
    lumen_chrome::bind_model(&mut doc, &model);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    lumen_layout::incremental::set_graft_diagnostics(true);
    // BUG-341 S18: with whole-subtree box reuse on, the graft is handed the
    // document as one `REUSED_SUBTREE` claim and honours it without
    // comparing a single box вЂ” correct, and exactly what S18 is for, but it
    // would turn this census into "1 box visited, 1 reused". Turning reuse
    // off keeps this gate measuring what it was written to measure: whether
    // the *comparison*, when it does run, is fooled by the used values the
    // previous layout pass wrote back into the styles.
    let mut state = Cc12IncrementalState { box_reuse_off: true, ..Default::default() };
    let mut last = lumen_layout::incremental::GraftStats::default();
    for i in 0..4 {
        let hover = if i % 2 == 0 { sidebar } else { None };
        let _ = cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, hover, &mut state);
        last = lumen_layout::incremental::take_graft_stats();
    }
    lumen_layout::incremental::set_graft_diagnostics(false);

    assert!(last.visited > 100, "chrome document should produce a non-trivial box tree: {last:?}");
    assert_eq!(
        last.reject_style_used_value_only, 0,
        "{} boxes were refused reuse purely because the previous layout pass wrote used \
             values back into their styles (BUG-341 S13) вЂ” the graft must compare against the \
             cascade result, not against the laid-out tree's polluted styles. Full census: {last:?}",
        last.reject_style_used_value_only,
    );
    assert_eq!(
        last.reused_clean, last.visited,
        "a hover flip that changes no computed style must leave the whole chrome document \
             reusable, got {}/{} вЂ” census: {last:?}. If chrome.html gains a rule that really does \
             restyle on `#sidebar:hover`, this number legitimately drops: replace the equality \
             with the count that rule accounts for, do not loosen it to a percentage.",
        last.reused_clean,
        last.visited,
    );
}

/// BUG-341 S15 regression gate: a hover flip that re-cascades nothing must
/// not rebuild the box tree either.
///
/// After S13/S14 the chrome document's dirty set on a `#sidebar`/`None`
/// toggle is empty and `graft_geometry` reuses all 318 boxes вЂ” yet the tree
/// was still built from scratch every cycle, only to be grafted straight
/// back onto the previous geometry (`build_box` was 2.2-2.5 ms of a
/// ~3.7 ms cycle, the largest item left). S4's `clean_subtrees` mechanism
/// existed for exactly this and had been switched off since S4's own
/// measurement rejected it.
///
/// The gate is the **count**, not wall-clock: cloning versus rebuilding
/// produces the identical tree, so nothing but a counter can tell the two
/// apart, and the 15% machine noise this fixture sits in hides the whole
/// effect. `built` at single digits means the whole document came across in
/// one subtree clone; a regression (e.g. the reuse flag stops reaching the
/// rayon workers that build large flex containers, which is precisely what
/// happened before S15) sends it back into the hundreds.
#[test]
fn bug341_s15_hover_flip_reuses_the_box_tree_instead_of_rebuilding_it() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let model = cc12_bench_model("");
    lumen_chrome::bind_model(&mut doc, &model);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    let mut state = Cc12IncrementalState::default();
    let mut first_full_built = 0;
    let mut last = lumen_layout::box_tree::BoxBuildStats::default();
    for i in 0..4 {
        let hover = if i % 2 == 0 { sidebar } else { None };
        let (_, bb) =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, hover, &mut state);
        // Cycle 0 has no `prev` вЂ” it is the full-rebuild reference.
        if i == 0 {
            first_full_built = bb.built;
        }
        last = bb;
    }

    assert!(
        first_full_built > 100,
        "the first (full) cycle should build a non-trivial chrome tree, got {first_full_built}",
    );
    assert!(
        last.reused >= 1,
        "a hover flip nothing can react to must clone the document's box subtree from the \
             previous cycle, got {last:?}",
    );
    assert!(
        last.built < 10,
        "{} boxes were rebuilt on a cycle whose cascade dirty set is empty (full rebuild is \
             {first_full_built}) вЂ” the S4 `clean_subtrees` reuse is not reaching them. Census: \
             {last:?}",
        last.built,
    );
}

/// BUG-341 S16 regression gate: a keystroke must cost the omnibox's own
/// chain, not the whole document's box tree.
///
/// This is `CC12_KEY`: one typed character changes the `#omniInput`
/// `value` attribute and nothing else. S15 made hover frames reuse all 318
/// boxes, but reuse was licensed by a single document-wide
/// `dom_content_stable` boolean, so this cycle вЂ” the one real content
/// change in the fixture вЂ” still rebuilt every box, `build_box` being
/// 1.9-2.3 ms of a ~3.1-3.8 ms cycle. The boolean is now a per-node
/// `ContentDirty::Nodes` set fed by `bind_model_tracked`.
///
/// The gate is the **count**: rebuilding and cloning produce identical
/// trees, so nothing but a counter distinguishes them (S8's lesson), and
/// the fixture's machine noise is wider than the whole effect. The
/// correctness side of this mechanism is gated separately and closer to the
/// code, in `lumen-layout`'s
/// `box_build_text_mutation_reuses_everything_but_the_mutated_chain` (stale
/// text is what an under-reporting tracker produces) and in
/// `lumen-chrome`'s `every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive`
/// (which is what keeps the tracker from under-reporting in the first
/// place).
#[test]
fn bug341_s16_keystroke_rebuilds_only_the_omnibox_chain() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    let mut first_full_built = 0;
    let mut last = lumen_layout::box_tree::BoxBuildStats::default();
    for i in 0..4 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        let (_, bb) =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, None, &mut state);
        if i == 0 {
            first_full_built = bb.built;
        }
        last = bb;
    }

    assert!(
        first_full_built > 100,
        "the first (full) cycle should build a non-trivial chrome tree, got {first_full_built}",
    );
    assert!(
        last.reused > 0,
        "a keystroke must let the ~99% of the document it never came near be cloned from the \
             previous cycle, got {last:?} вЂ” this is exactly what the document-wide \
             `dom_content_stable` boolean prevented up to S15",
    );
    assert!(
        last.built * 4 < first_full_built,
        "{} of {first_full_built} boxes were rebuilt for a one-character omnibox change вЂ” \
             the per-node content-dirty set is not narrowing anything. Census: {last:?}. If \
             chrome.html gains a rule that genuinely restyles a large region on `#omniInput`, \
             raise this to the count that rule accounts for; do not turn it into a percentage \
             of whatever the code currently does.",
        last.built,
    );
}

/// BUG-341 S19 regression gate: finding the reusable subtrees must cost the
/// spine above them, not a walk of the whole previous tree.
///
/// The reuse unit became a *move* in S19 (the subtrees are taken out of the
/// previous tree instead of deep-copied out of it вЂ” see
/// `lumen-layout`'s `bug341_s19_reuse_takes_the_subtree_out_of_prev_
/// instead_of_copying_it` for that half), and carving the index that way
/// stops at each reusable subtree's root instead of hashing every box the
/// way S4's `index_by_node` did. This is the production-document counter for
/// it: on a keystroke, chrome's 318 boxes are found through ~19.
///
/// A counter, not wall-clock вЂ” an index that walks the whole tree again
/// produces the identical result, and 0.02 ms on this fixture is far inside
/// the machine noise the whole cycle sits in.
#[test]
fn bug341_s19_reuse_index_walks_the_spine_not_the_previous_tree() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    let mut first_full_built = 0;
    let mut last = lumen_layout::box_tree::BoxBuildStats::default();
    for i in 0..4 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        let (_, bb) =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, None, &mut state);
        if i == 0 {
            first_full_built = bb.built;
        }
        last = bb;
    }

    assert!(
        first_full_built > 100,
        "the first (full) cycle should build a non-trivial chrome tree, got {first_full_built}",
    );
    assert!(
        last.reused >= 5,
        "a keystroke must still take whole subtrees out of the previous tree вЂ” {last:?}",
    );
    assert!(
        last.prev_index_visited < 50,
        "{} boxes of the previous tree were walked to find {} reusable subtrees on a keystroke \
             that rebuilds ~28 of 318 вЂ” the index is being built over the whole tree again \
             (S4's `index_by_node`), not carved out of the spine. Census: {last:?}. If \
             chrome.html gains structure that genuinely rebuilds a large region on every \
             keystroke, record the count that structure accounts for; do not turn this into a \
             percentage of whatever the code currently does.",
        last.prev_index_visited,
        last.reused,
    );
}

/// BUG-341 S20 regression gate: a keystroke must not dispatch rayon workers
/// to carry out moves.
///
/// ADR-016 M4.1 fans a flex/grid container's children onto rayon once there
/// are eight or more of them, sized against a full pass where each child
/// costs a cascade and a box build. On the incremental path since S15/S19 a
/// child that is in the reuse index costs a `Mutex` lock and a move, and on
/// a chrome interaction nearly every one of them is вЂ” so the threshold, read
/// off `dom_children.len()`, was dispatching a worker per subtree the pass
/// was about to move in O(1). Measured at ~1 ms of a 2.5 ms keystroke cycle
/// (BUG-341 "S20"), spread across `body`, `.main-col` and `.omnibox-wrap`,
/// whose own work is ~4 Вµs each.
///
/// Both arms are asserted, and the full-pass one matters as much as the
/// incremental one: the cheapest way to make this test's headline number
/// pass is to stop parallelising altogether, which would cost the full pass
/// the parallel selector matching M4.1 exists for.
///
/// A counter, not wall-clock: the fan-out produces the identical tree, so
/// every differential test in the track passes either way (S8's lesson), and
/// thread-pool overhead is exactly the kind of cost machine noise hides.
#[test]
fn bug341_s20_keystroke_moves_subtrees_without_dispatching_workers() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    let mut first_full = lumen_layout::box_tree::BoxBuildStats::default();
    let mut last = lumen_layout::box_tree::BoxBuildStats::default();
    for i in 0..4 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        let (_, bb) =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, None, &mut state);
        if i == 0 {
            first_full = bb;
        }
        last = bb;
    }

    assert!(
        first_full.fanouts > 0,
        "the first (full) cycle must still parallelise its large containers вЂ” M4.1's \
             parallel selector matching is what the threshold exists for, and narrowing the \
             *incremental* estimate must not have reached the full path. Census: {first_full:?}",
    );
    assert!(
        last.reused >= 5,
        "a keystroke must still take whole subtrees out of the previous tree, otherwise this \
             test is asserting about a cycle that has no moves to skip dispatching for вЂ” {last:?}",
    );
    assert_eq!(
        last.fanouts, 0,
        "a keystroke dispatched {} rayon fan-out(s) while reusing {} whole subtrees вЂ” the \
             threshold is counting children the pass will move rather than children it will \
             build. Census: {last:?}. If chrome.html ever gains a container that genuinely \
             rebuilds eight or more element children on a keystroke, record that container and \
             its count here; do not relax this to `<= whatever the code currently does`.",
        last.fanouts, last.reused,
    );
}

/// BUG-341 S21 regression gate: an interaction must not re-index the sheet.
///
/// The cascade's `CascadeIndex` вЂ” the top-level `RuleIndex` plus one per
/// `@layer`/`@media`/`@supports` block plus two sheet-wide predicate scans вЂ”
/// was keyed by the stylesheet's **address**, which the allocator hands back
/// the moment a sheet is freed. To keep that key honest every layout pass
/// dropped the cache before its first `compute_style`, and so did every
/// rayon worker's `StyleEnvSnapshot::install`, which reduced a cross-pass
/// cache to a within-pass one: the S21 census measured one rebuild per
/// incremental cycle (0.14-0.22 ms, 7-19% of a cycle that had got down to
/// 0.74-3.0 ms) and 33 per full pass across the worker pool. Keyed by
/// `Stylesheet::revision`, which is minted per sheet and never recycled,
/// nothing needs dropping.
///
/// A counter, not wall-clock: an index rebuilt from scratch every frame
/// yields byte-identical styles, so no differential test in this track can
/// see it, and 0.2 ms is well inside machine noise.
///
/// Both arms. The cold pass must still build one вЂ” a cache that is never
/// populated also never rebuilds, and would serve an empty index (i.e. no
/// rule matches anything, which the `dirty_roots` assertion below and every
/// pixel test would catch, but not this counter).
#[test]
fn bug341_s21_interaction_cycles_do_not_reindex_the_stylesheet() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    lumen_layout::style::clear_rule_idx_cache();
    let _ = lumen_layout::style::take_cascade_index_stats();

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    typed.push('a');
    let _ = cc12_bench_cycle(
        &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None, &mut state,
    );
    let cold = lumen_layout::style::take_cascade_index_stats();
    assert!(
        cold.builds >= 1,
        "the first pass must index the sheet вЂ” a cache that stays empty would hand every \
             node an index that matches nothing. Census: {cold:?}",
    );

    // Both interaction shapes CC-12 measures: a keystroke (DOM mutation)
    // and a hover flip. Neither touches the stylesheet.
    for i in 0..4 {
        typed.push('a');
        let _ = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None,
            &mut state,
        );
        let hover = if i % 2 == 0 { sidebar } else { None };
        let _ = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, hover,
            &mut state,
        );
    }

    let warm = lumen_layout::style::take_cascade_index_stats();
    assert_eq!(
        warm.builds, 0,
        "eight interaction cycles re-indexed the stylesheet {} time(s) ({:.3} ms) without a \
             single rule changing. The index is keyed by `Stylesheet::revision`; something is \
             either dropping the cache on the layout path or minting a revision for a sheet that \
             was not mutated. Census: {warm:?}",
        warm.builds,
        warm.build_ns as f64 / 1e6,
    );
}

/// BUG-341 S24 regression gate: interaction cycles must *carry* the cascade
/// cache, not rebuild it вЂ” and must not sweep it either.
///
/// A hover cycle reused all 828 of the previous pass's styles and then
/// inserted all 828 into a fresh map, which the pipeline cloned wholesale so
/// it could be the next cycle's `prev_styles`. Since S24 the map is moved
/// into the pass and back out of it. Nothing about the *output* changes, so
/// this has to be a counter gate (S8's lesson): `passes_lived` is the number
/// of passes that have written into the map the pipeline is holding, and a
/// pipeline that went back to handing each pass a fresh one would report 1
/// for ever while every differential test stayed green.
///
/// Both arms, and the second is the load-bearing one. The cheapest way to
/// satisfy "never rebuild" is to never evict either вЂ” and an entry that
/// outlives the pass that wrote it breaks the property the whole reuse rule
/// rests on: *an entry exists iff the immediately preceding pass visited
/// that node*. Absence is what forces a recompute for a node that was
/// detached and re-attached, or moved to a new parent, whose style was
/// computed under a different inherited chain. So the gate also asserts that
/// the steady state never sweeps (the sweep is O(document) вЂ” putting it back
/// on every pass would undo the slice while keeping every test green) and
/// that a pass which really does drop nodes evicts exactly them.
#[test]
fn bug341_s24_interaction_cycles_carry_the_cascade_cache_instead_of_rebuilding_it() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    // Cold pass: a full cascade, which legitimately builds its map from
    // scratch and has lived through no incremental pass at all.
    let _ = cc12_bench_cycle(
        &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None, &mut state,
    );
    assert_eq!(
        state.prev_cascade_styles.passes_lived(),
        0,
        "the first pass is a full cascade вЂ” it has no cache to carry",
    );
    let cold_len = state.prev_cascade_styles.len();
    assert!(cold_len > 100, "chrome must cascade a non-trivial node count, got {cold_len}");

    // Eight interaction cycles of both shapes CC-12 measures: a keystroke
    // (DOM mutation) and a hover flip. Neither adds or removes a node.
    for i in 0..4 {
        typed.push('a');
        let _ = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None,
            &mut state,
        );
        assert!(
            !state.prev_cascade_styles.swept_last_pass(),
            "keystroke cycle {i} swept the cascade cache вЂ” a full scan of {} entries for a \
                 pass that removed nothing puts back exactly the per-pass O(document) work this \
                 slice removed. Either `visited` is not counting one visit per element, or the \
                 flat tree really does reach a node twice (then this gate needs the count, not a \
                 flat `false`).",
            state.prev_cascade_styles.len(),
        );
        let hover = if i % 2 == 0 { sidebar } else { None };
        let _ = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, hover,
            &mut state,
        );
        assert!(
            !state.prev_cascade_styles.swept_last_pass(),
            "hover cycle {i} swept the cascade cache вЂ” see the keystroke assertion above",
        );
    }
    assert_eq!(
        state.prev_cascade_styles.passes_lived(),
        4,
        "the interaction cycles that really cascade must have written into one and the same \
             map. A lower number means some pass handed the pipeline a freshly built map instead \
             of the one it was given вЂ” byte-identical styles at the cost S24 removed. Four and \
             not eight since BUG-341 S26: the four hover cycles present an empty delta, and a \
             pass that skips its walk deliberately leaves the ordinal alone вЂ” it visited nobody, \
             so 'the immediately preceding pass to visit this node' is still the keystroke \
             before it, which is exactly what keeps the entries reusable. \
             `bug341_s26_hover_cycles_do_not_re_walk_the_cascade` gates that half.",
    );
    assert_eq!(
        state.prev_cascade_styles.len(),
        cold_len,
        "no cycle added or removed a node, so the carried map must still hold exactly the \
             elements the cold pass cascaded",
    );

    // Arm two: a cycle that really does drop nodes must evict them. The
    // sidebar's tab list is rebuilt from the model, so a shorter model
    // detaches rows вЂ” their entries must not survive into the next pass.
    let before_removal = state.prev_cascade_styles.len();
    let _ = cc12_bench_cycle(
        &mut doc, &sheet, &cc12_bench_shrunk_model(&typed), viewport, &measurer, &hyp, None,
        &mut state,
    );
    assert!(
        state.prev_cascade_styles.swept_last_pass(),
        "a cycle that detached rows left the cache un-swept: every detached node's entry is \
             still there, and a node re-attached under a different parent would reuse a style \
             cascaded under the old inherited chain",
    );
    assert!(
        state.prev_cascade_styles.len() < before_removal,
        "the sweep kept all {before_removal} entries although the model lost rows",
    );
    for (nid, _) in state.prev_cascade_styles.iter() {
        assert!(
            doc.get(*nid).parent.is_some() || *nid == doc.root(),
            "the carried cache still holds {nid:?}, which is detached from the document",
        );
    }
}

/// BUG-341 S27 regression gate: a cycle whose delta names one node must
/// walk that node's chain, not the document.
///
/// S26 removed the traversal for the cycle whose delta names *nobody*, and
/// left the general case open: a keystroke names one dirty root and one
/// content-mutated node, yet the walk still entered all 1 740 nodes of the
/// chrome document to re-cascade one of them. Everything it did to the
/// other 1 739 was a restatement of its input вЂ” reuse the carried style,
/// report clean, record a `clean_subtrees` entry вЂ” all of which the delta
/// already implies for any subtree holding neither of those two nodes.
///
/// A counter gate, not wall-clock and not differential, for the S8 reason:
/// walking the whole document produces byte-identical output, so only a
/// count separates the two shapes.
///
/// Four arms, and the last three are the load-bearing ones:
///
/// * the cold pass must still walk everything,
/// * the keystroke must still re-cascade the node it changed вЂ” driving the
///   visit count to zero by never cascading passes arm two and renders a
///   stale document,
/// * every element inside a skipped subtree must be found in the carried
///   cache (`confirm_misses`); a miss means the spine under-approximated
///   the delta and a node was left with no style at all,
/// * the cache must survive the skipping: the S24 ordinal contract is "an
///   entry exists iff the immediately preceding pass visited that node", so
///   a skip that walks away without restamping gets its whole subtree swept
///   at `finish_pass` and re-cascaded next cycle вЂ” same output, one
///   document-sized recompute per interaction.
#[test]
fn bug341_s27_a_keystroke_walks_its_own_chain_not_the_document() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    // Cold pass вЂ” a full cascade, which must walk the whole document.
    let _ = cc12_bench_cycle(
        &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None, &mut state,
    );
    let cold = lumen_layout::counters::take_cascade_stats();
    assert!(
        cold.visited > 1000,
        "the cold pass must walk the whole chrome document, got visited={}",
        cold.visited,
    );
    let elements = state.prev_cascade_styles.len();

    for i in 0..4 {
        typed.push('a');
        let _ = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None,
            &mut state,
        );
        let key = lumen_layout::counters::take_cascade_stats();

        assert!(
            key.visited * 4 < cold.visited,
            "keystroke cycle {i} entered {} of the document's {} nodes. Its delta names one \
                 dirty root and one content-mutated node, so nothing outside their ancestor \
                 chains can be reached вЂ” every other node was entered only to hand back what the \
                 delta already said about it.",
            key.visited,
            cold.visited,
        );
        assert!(
            key.skipped_subtrees > 0,
            "keystroke cycle {i} skipped no subtree at all вЂ” the spine was built and never \
                 consulted",
        );
        assert!(
            key.recomputed > 0,
            "keystroke cycle {i} re-cascaded nothing (visited={}). The omnibox's own `value` \
                 attribute changed, so a node really does need re-cascading; zeroing the visit \
                 counter by never cascading passes the arm above and renders a stale document.",
            key.visited,
        );
        assert_eq!(
            key.confirm_misses, 0,
            "keystroke cycle {i}: {} element(s) inside a skipped subtree had no entry in the \
                 carried cache. The skip rests on the claim that the previous pass cascaded every \
                 one of them вЂ” a miss means a node was left with no style, which is a rebuilt box \
                 at best and a wrong inherited chain at worst.",
            key.confirm_misses,
        );
        assert!(
            !state.prev_cascade_styles.swept_last_pass(),
            "keystroke cycle {i} swept the cache although no node left the document. A \
                 skipped subtree still owes the S24 pass ordinal; skipping without restamping \
                 makes `finish_pass` read the whole subtree as gone, and the next cycle \
                 re-cascades it вЂ” identical output, one document-sized recompute per keystroke.",
        );
        assert_eq!(
            state.prev_cascade_styles.len(),
            elements,
            "keystroke cycle {i} left the carried cache holding {} entries instead of the \
                 {elements} elements the cold pass cascaded",
            state.prev_cascade_styles.len(),
        );
    }
}

/// BUG-341 S26 regression gate: an interaction cycle whose delta says
/// nothing changed must not walk the document to find that out.
///
/// The census that opened this slice split the incremental pass by stage for
/// the first time and found the cascade *traversal* вЂ” not `compute_style`,
/// which by then ran once per keystroke and never on a hover вЂ” to be 50-75 %
/// of the whole pass on both CC-12 scenarios. On a hover flip, S14 had
/// already emptied the root set and S16 reported no content mutation, so the
/// walk entered all 1 740 nodes of the chrome document, reused all 828
/// styles, declared all 828 elements clean, and handed back the map it was
/// given. Every one of those answers was already in the delta.
///
/// A counter, not wall-clock, for the usual reason (the S8 lesson): a walk
/// that reuses everything produces byte-identical output, so no differential
/// test in this track can tell it from not walking at all.
///
/// Both arms, and the second is load-bearing: the cheapest way to drive the
/// visit count to zero is to stop cascading altogether, which arm one alone
/// would happily accept.
#[test]
fn bug341_s26_hover_cycles_do_not_re_walk_the_cascade() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    // Cold pass вЂ” a full cascade, which must walk everything.
    let _ = cc12_bench_cycle(
        &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None, &mut state,
    );
    let cold = lumen_layout::counters::take_cascade_stats();
    assert!(
        cold.visited > 100,
        "the cold pass must walk the whole document, got visited={}",
        cold.visited,
    );

    for i in 0..4 {
        // Arm 2 вЂ” a keystroke really mutates the omnibox, so the cascade
        // stage must still run.
        typed.push('a');
        let _ = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None,
            &mut state,
        );
        let key = lumen_layout::counters::take_cascade_stats();
        assert!(
            key.visited > 0 && key.recomputed > 0,
            "keystroke cycle {i} skipped the cascade entirely (visited={} recomputed={}) вЂ” \
                 the omnibox's own value attribute changed, so a node really does need \
                 re-cascading. Zeroing the visit counter by never cascading passes arm one and \
                 renders a stale document.",
            key.visited,
            key.recomputed,
        );

        // Arm 1 вЂ” a hover flip on the sidebar: S14 leaves the root set
        // empty and the model is unchanged, so the delta states outright
        // that nothing changed.
        let hover = if i % 2 == 0 { sidebar } else { None };
        let _ = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, hover,
            &mut state,
        );
        let hov = lumen_layout::counters::take_cascade_stats();
        assert_eq!(
            hov.visited, 0,
            "hover cycle {i} walked {} node(s) although its delta named no dirty root and no \
                 content mutation. Every element would take the reuse branch and every node \
                 report itself clean вЂ” the stage's whole output on such a cycle is a restatement \
                 of its input, and it was the largest single item in the pass.",
            hov.visited,
        );
    }
}

/// BUG-341 S25 regression gate: the box-build stage must read a child's
/// `display` off the cascade cache, not cascade the child again.
///
/// Deciding which formatting context a child joins asked `compute_style`
/// up to three times per element child вЂ” `is_inline_content`,
/// `is_inline_block` and the `display:none` re-probe inside the
/// inline-collect loop each ran a full cascade. `precompute_counters` had
/// already cascaded every one of those nodes against the same parent style,
/// and `build_box_inner` builds the child's box out of *that* entry
/// whatever the probe answers, so the probes were pure re-derivation: 14
/// per keystroke cycle and 2 per hover cycle, 0.21-0.25 ms of a 0.63 ms
/// keystroke and 0.07-0.08 ms of a 0.29 ms hover. Two of chrome's most
/// expensive cascades sat in that count вЂ” `<html>`, which carries the
/// whole design system's custom properties, was re-cascaded on every hover
/// frame purely to be told it is not inline.
///
/// A counter, not wall-clock: the answer is identical either way, so no
/// differential test in this track can see it (the S8 lesson).
///
/// Both arms, and the second is load-bearing: the cheapest way to drive
/// "cascades" to zero is to stop asking about `display` at all, which puts
/// every child in the wrong formatting context вЂ” a wrong tree, not a slow
/// frame. `display_probes` is the count of questions asked, and it must
/// stay above zero. What the answers must *be* is gated in `lumen-layout`
/// (`bug341_s25_display_probes_read_the_cascade_instead_of_re_running_it`),
/// on a fixture that exercises all three probes at once.
#[test]
fn bug341_s25_the_box_build_stage_reads_display_off_the_cascade_cache() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    // Cold pass: a full cascade populates the cache for the whole document,
    // so even here no probe has an excuse to run one of its own.
    let (_, cold) = cc12_bench_cycle(
        &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None, &mut state,
    );
    assert!(
        cold.display_probes > 0,
        "the full pass must still ask which formatting context each element child joins. \
             Census: {cold:?}",
    );
    assert_eq!(
        cold.display_probe_cascades, 0,
        "the full pass re-cascaded {} node(s) only to read their `display`, although \
             `precompute_counters` had just cascaded every one of them. Census: {cold:?}",
        cold.display_probe_cascades,
    );

    // Both interaction shapes CC-12 measures: a keystroke (DOM mutation)
    // and a hover flip.
    for i in 0..4 {
        typed.push('a');
        let (_, key) = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, None,
            &mut state,
        );
        assert_eq!(
            key.display_probe_cascades, 0,
            "keystroke cycle {i} re-cascaded {} node(s) to read their `display`. The \
                 incremental pass carries the cascade cache (S24), so every element the box \
                 build walks has an entry in it; a non-zero count means either the probe stopped \
                 consulting the cache or the carried map lost entries the pass still needs. \
                 Census: {key:?}",
            key.display_probe_cascades,
        );
        assert!(key.display_probes > 0, "cycle {i} asked nothing. Census: {key:?}");

        let hover = if i % 2 == 0 { sidebar } else { None };
        let (_, hov) = cc12_bench_cycle(
            &mut doc, &sheet, &cc12_bench_model(&typed), viewport, &measurer, &hyp, hover,
            &mut state,
        );
        assert_eq!(
            hov.display_probe_cascades, 0,
            "hover cycle {i} вЂ” see the keystroke assertion above. Census: {hov:?}",
        );
        assert!(hov.display_probes > 0, "hover cycle {i} asked nothing. Census: {hov:?}");
    }
}

/// BUG-341 S14 regression gate: a hover flip no rule in the sheet can
/// react to must re-cascade nothing at all.
///
/// This is CC-12's own `#sidebar`/`None` toggle вЂ” the shape S3 documented
/// as its worst case and every slice since then worked around. `:hover`
/// genuinely does flip on every ancestor of `#sidebar` up to the document
/// root (CSS Selectors L4 В§4.3), so the pre-S14 root-set contained the root
/// and forced a whole-document re-cascade вЂ” 6.8-8.4 ms of a ~12 ms cycle,
/// producing byte-identical styles for all 318 boxes (S13's census proved
/// the "identical" half).
///
/// Two asserts, in this order on purpose. The first is the *ground truth*:
/// the two cascades really are equal, independently of any narrowing code.
/// The second is the **count gate** вЂ” the root-set is empty вЂ” which is the
/// only thing that can fail if the narrowing silently stops narrowing: a
/// mechanism that reuses nothing still reproduces the full cascade exactly
/// (S8's lesson), just slowly. If `chrome.html` ever gains a rule that
/// really does restyle on `#sidebar:hover` or on one of its ancestors, both
/// asserts flip together and the fix is to point this test at a different
/// node, not to loosen it.
#[test]
fn bug341_s14_hover_flip_no_rule_can_react_to_recascades_nothing() {
    use lumen_layout::counters::{
        incremental_precompute_counters, precompute_counters, set_incremental_restyle, RestyleDelta,
    };
    use lumen_layout::style::{restyle_root_set_for_state_change, restyle_state_index};

    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    lumen_chrome::bind_model(&mut doc, &cc12_bench_model(""));
    let viewport = Size::new(1280.0, 800.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let sidebar = doc
        .find_by_id(lumen_chrome::ids::SIDEBAR)
        .expect("chrome preview must have #sidebar");

    let index = restyle_state_index(&doc, &sheet);
    assert!(
        !index.is_conservative(),
        "chrome.html has no dynamic `:has()` and the chrome document has no shadow roots вЂ” \
             if either changes, the per-node narrowing turns itself off and CC-12's hover cycle \
             silently returns to a whole-document re-cascade",
    );
    assert!(
        index.state_compound_count() > 10,
        "chrome.html has dozens of `:hover` rules; scanning found only {} вЂ” the narrowing \
             would be trivially (and uselessly) correct on an empty compound list",
        index.state_compound_count(),
    );

    // Ground truth, computed without any incremental machinery: nothing
    // hovered vs `#sidebar` hovered produce the same cascade.
    lumen_layout::set_interactive_state(None, None, None);
    let none_map = precompute_counters(&doc, &sheet, viewport, &flat, false);
    lumen_layout::set_interactive_state(Some(sidebar), None, None);
    let hovered_map = precompute_counters(&doc, &sheet, viewport, &flat, false);
    lumen_layout::clear_interactive_state();
    assert!(none_map.styles().len() > 100, "chrome document should cascade a non-trivial node count");
    assert_eq!(
        none_map.styles(),
        hovered_map.styles(),
        "no rule in chrome.html reacts to hovering #sidebar, so both full cascades must agree",
    );

    // The count gate: the narrowed root-set for that transition is empty.
    let dirty_roots = restyle_root_set_for_state_change(&doc, None, Some(sidebar), &index);
    assert!(
        dirty_roots.is_empty(),
        "hovering #sidebar flips `:hover` on {} node(s) the restyle root-set still keeps, but \
             no selector in chrome.html can observe any of them (asserted above) вЂ” BUG-341 S14",
        dirty_roots.len(),
    );

    // And the incremental cascade run under that empty root-set still
    // reproduces the full post-transition cascade bit-for-bit.
    lumen_layout::set_interactive_state(Some(sidebar), None, None);
    let delta = RestyleDelta {
        prev_styles: none_map.styles().clone(),
        dirty_roots,
        content_dirty: lumen_layout::counters::ContentDirty::Nothing,
    };
    set_incremental_restyle(true);
    let incr = incremental_precompute_counters(&doc, &sheet, viewport, &flat, false, delta);
    set_incremental_restyle(false);
    lumen_layout::clear_interactive_state();
    assert_eq!(
        incr.styles(),
        hovered_map.styles(),
        "incremental cascade with an empty root-set must equal the full post-transition cascade",
    );
}


/// BUG-341 S17 regression gate: a keystroke must re-cascade the omnibox
/// input, not the omnibox.
///
/// This is the S14 argument applied to DOM mutations. Typing one character
/// writes `#omniInput`'s `value` attribute; the pre-S17 root-set answered
/// that by invalidating the *parent's* whole subtree, because a sibling
/// combinator (`X + Y`) is the one shape that reaches outside the changed
/// node's own subtree. The census (`bug341_s17_keystroke_restyle_census`)
/// found that cost 12 re-cascaded elements, all 12 producing a
/// byte-identical `ComputedStyle`, and each losing its box on the way
/// (`must_recompute` в‡’ not in `clean_subtrees`).
///
/// Three asserts, in this order on purpose. First the *ground truth*,
/// computed with no incremental machinery at all: the two full cascades
/// really are equal, so nothing in `chrome.html` reacts to the `value`
/// write. Then the **count gate** вЂ” the root-set is `{#omniInput}` and the
/// cascade recomputes exactly one element вЂ” which is the only thing that
/// can fail silently: a mechanism that narrows nothing still reproduces the
/// full cascade (S8's lesson), just slowly. Last, that the incremental
/// cascade run under the narrowed root-set equals the full one.
///
/// If `chrome.html` ever gains a sibling rule that can match `#omniInput`,
/// the count assert flips and the honest fix is to record the number that
/// rule accounts for вЂ” not to loosen this into a percentage.
#[test]
fn bug341_s17_keystroke_recascades_the_input_not_the_omnibox() {
    use lumen_layout::counters::{
        incremental_precompute_counters, precompute_counters, set_incremental_restyle,
        take_cascade_stats, RestyleDelta,
    };
    use lumen_layout::style::{restyle_node_index, restyle_root_set_for_node_change};

    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let viewport = Size::new(1280.0, 800.0);
    lumen_chrome::bind_model(&mut doc, &cc12_bench_model("a"));
    let flat = lumen_dom::build_flat_tree(&doc);
    let omni = doc
        .find_by_id(lumen_chrome::ids::OMNI_INPUT)
        .expect("chrome preview must have #omniInput");
    let omnibox = doc.get(omni).parent.expect("#omniInput must have a parent");

    // Ground truth: cascade before and after one more typed character.
    let before = precompute_counters(&doc, &sheet, viewport, &flat, false);
    let touched = lumen_chrome::bind_model_tracked(&mut doc, &cc12_bench_model("ab"));
    let after = precompute_counters(&doc, &sheet, viewport, &flat, false);
    assert!(before.styles().len() > 100, "chrome document should cascade a non-trivial node count");
    assert_eq!(
        before.styles(),
        after.styles(),
        "no rule in chrome.html reacts to `#omniInput`'s `value`, so both full cascades must agree",
    );

    // The mutation report itself: exactly one node, exactly one attribute.
    assert_eq!(
        touched.selector.keys().copied().collect::<std::collections::HashSet<_>>(),
        [omni].into_iter().collect::<std::collections::HashSet<_>>(),
        "typing must report only #omniInput as selector-touched: {touched:?}",
    );
    assert_eq!(
        touched.selector[&omni].attrs.iter().map(String::as_str).collect::<Vec<_>>(),
        ["value"],
        "typing writes exactly the `value` attribute",
    );

    // The count gate: the narrowed root-set is the input itself, not the
    // `.omnibox` wrapper whose 12-element subtree used to re-cascade.
    let node_index = restyle_node_index(&doc, &sheet);
    assert!(
        !node_index.is_conservative(),
        "chrome.html has no `:has()`/`:nth-child(of вЂ¦)` and the chrome document has no shadow \
             roots вЂ” if any of that changes, the per-node narrowing turns itself off and CC-12's \
             keystroke cycle silently returns to re-cascading the whole `.omnibox`",
    );
    let dirty_roots =
        restyle_root_set_for_node_change(&doc, chrome_node_changes(&touched), &node_index);
    assert_eq!(
        dirty_roots,
        [omni].into_iter().collect::<std::collections::HashSet<_>>(),
        "a `value` write must invalidate #omniInput alone; {:?} would be the pre-S17 \
             widen-to-parent answer",
        [omnibox],
    );

    let _ = take_cascade_stats();
    let delta = RestyleDelta {
        prev_styles: before.styles().clone(),
        dirty_roots,
        content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
    };
    set_incremental_restyle(true);
    let incr = incremental_precompute_counters(&doc, &sheet, viewport, &flat, false, delta);
    set_incremental_restyle(false);
    let stats = take_cascade_stats();
    assert_eq!(
        stats.recomputed, 1,
        "exactly one element (#omniInput) may re-run `compute_style` for a one-character \
             omnibox change; got {stats:?}",
    );
    assert_eq!(
        incr.styles(),
        after.styles(),
        "the incremental cascade under the narrowed root-set must equal the full one",
    );
}

// в”Ђв”Ђ BUG-405 slice 49: does `predict_same` really predict `chrome_dl` byte identity в”Ђв”Ђ

/// BUG-405 slice 48's census (`scripts/chrome_dl_repeat_census.py`, driven over
/// MCP) could only ever produce `predict=false`: `new_tab`/`navigate` are the
/// only automation events that reach `relayout_chrome_host`, and both always
/// touch `bind_model`'s content, so `touched.is_empty()` is never true in that
/// sample. The one shape that *would* hit `predict=true` in real use в€’ two
/// consecutive `relayout_chrome_host` passes with the exact same hover target
/// (a mouse jittering inside a still-hovered button, or any call reached for
/// an unrelated reason while hover happens not to have moved) в€’ needs a real
/// `CursorMoved` (`crates/shell/src/lumen/cursor_moved.rs`) or a real chrome
/// click (`dispatch_chrome_action`), neither reachable through MCP's
/// hit-test-free `click` (see CLAUDE.md's "hovered/active nid ... cannot be
/// exercised by any automation surface" gotcha).
///
/// `relayout_chrome_host` itself cannot be unit-tested directly either вЂ” it
/// early-returns without a real `self.renderer`, and building a `Lumen` (246
/// fields: JS runtime, network stack, tab manager, вЂ¦) for one diagnostic test
/// is out of proportion to what this slice can safely reach. So this
/// reproduces the *identical* four-input formula
/// (`touched.is_empty() && interactive_stable && viewport_stable &&
/// forced_colors_stable`) and the *identical* building blocks
/// `relayout_chrome_host`/`cc12_bench_cycle` use
/// (`bind_model_tracked`/`set_interactive_state`/`layout_measured_hyp`/
/// `paint_ordered`/`hash_display_list`) at the doc/sheet level в€’ the same
/// level every other `bug341_s*` gate in this file already tests at в€’ instead
/// of going through `Lumen`.
#[test]
fn bug405_slice49_chrome_dl_predict_same_holds_for_a_steady_state_hover_repeat() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let model = cc12_bench_model("");
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR).expect("chrome preview must have #sidebar");

    // Cycle 0 (cold): establishes the bound model and gives cycle 1 a
    // predecessor `dl` to compare against, exactly like the first call to
    // `relayout_chrome_host` in a live session.
    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    lumen_layout::set_interactive_state(Some(sidebar), None, None);
    let layout0 = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let dl0 = paint_ordered(&layout0);
    let hash0 = lumen_paint::hash_display_list(&[], &dl0, 0.0, 0.0, 0, 0);
    lumen_layout::clear_interactive_state();

    // Cycle 1 reproduces exactly the shape срез 48's census could not reach:
    // `relayout_chrome_host` invoked again with the SAME hover target and an
    // unchanged model. `touched` stays empty (nothing in `model` changed
    // since cycle 0) and `new_interactive == chrome_prev_interactive` holds
    // because the hover target is identical в€’ both are the real preconditions
    // `predict_same` checks, not stand-ins for them.
    let touched1 = lumen_chrome::bind_model_tracked(&mut doc, &model);
    lumen_layout::set_interactive_state(Some(sidebar), None, None);
    let layout1 = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
    let dl1 = paint_ordered(&layout1);
    let hash1 = lumen_paint::hash_display_list(&[], &dl1, 0.0, 0.0, 0, 0);
    lumen_layout::clear_interactive_state();

    // `viewport_stable`/`forced_colors_stable` are trivially true here в€’
    // neither viewport nor Forced-Colors Mode is touched by this fixture,
    // which mirrors the real precondition: either one flipping already
    // forces `relayout_chrome_host`'s full-layout fallback, a separate path
    // this slice does not need to reproduce.
    let interactive_stable = true; // hover target identical across both cycles
    let predict_same = touched1.is_empty() && interactive_stable;
    assert!(
        predict_same,
        "reproduction must land on predict=true в€’ the exact case срез 48's MCP census (0/55 \
             predict=true calls) could not reach; touched1={touched1:?}",
    );

    let actual_same = hash0 == hash1;
    assert!(
        actual_same,
        "BUG-405 Рї.85: predict_same=true but chrome_dl bytes differ (hash0={hash0} hash1={hash1}) \
             в€’ this is the dangerous direction (predict=true, actual=false) a content_epoch \
             skip-path for chrome_dl would need to rule out on the ONE shape срез 48's census left \
             unmeasured. If this ever fails, the skip-path proposed in `bugs/BUG-405-OPEN.md`'s \
             'Остаток' would have shown stale chrome on screen for a plain hover-hold.",
    );
}

// -- BUG-405 srez 50: ChromeOverlayFrameCache reuse-or-build decision --

/// A cache HIT must return bytes identical to a fresh rebuild, and each of
/// the four key inputs (generation/host/viewport/caret) must, on its own,
/// force a MISS -- the correctness gate chrome_overlay_segment's doc comment
/// promises. Pure-function test, no Lumen/renderer needed (srez 49 found
/// that disproportionate for one gate).
#[test]
fn bug405_slice50_chrome_overlay_cache_hit_matches_fresh_build_and_key_changes_miss() {
    let chrome_dl = vec![lumen_paint::DisplayCommand::FillRect {
        rect: Rect::new(0.0, 0.0, 40.0, 20.0),
        color: lumen_layout::Color { r: 10, g: 20, b: 30, a: 255 },
    }];
    let host = Rect::new(200.0, 40.0, 800.0, 700.0);
    let (win_w, win_h) = (1024.0_f32, 768.0_f32);
    let caret = Some((
        Rect::new(300.0, 10.0, 2.0, 20.0),
        lumen_layout::Color { r: 0, g: 120, b: 220, a: 220 },
    ));

    // Cold: no cache yet -- must build fresh and hand back something to
    // remember.
    let (framed0, strips0, _digests0, cache0) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, None);
    let cache0 = cache0.expect("cold call must produce a cache to remember");

    // Same generation/host/viewport/caret, cache present -- must be a HIT:
    // no new cache (nothing changed to remember), bytes identical to cycle 0.
    let (framed1, strips1, _digests1, cache1) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, Some(&cache0));
    assert!(cache1.is_none(), "unchanged key must reuse the existing cache, not rebuild one");
    assert_eq!(framed1, framed0, "HIT must be byte-identical to the fresh build it reuses");
    assert_eq!(strips1, strips0);

    // Each key field, changed alone, must force a rebuild (a MISS) -- this
    // is what makes the cache SAFE: a stale entry is invalidated, not reused.
    let cases = [
        ("generation", 2, host, (win_w, win_h), caret),
        ("host", 1, Rect::new(210.0, 40.0, 800.0, 700.0), (win_w, win_h), caret),
        ("viewport", 1, host, (1025.0, 768.0), caret),
        ("caret", 1, host, (win_w, win_h), None),
    ];
    for (label, gen_, host2, vp2, caret2) in cases {
        let (_, _, _, new_cache) =
            chrome_overlay_segment(&chrome_dl, host2, vp2.0, vp2.1, caret2, gen_, true, Some(&cache0));
        assert!(
            new_cache.is_some(),
            "changing only `{label}` must miss the cache and rebuild -- a false HIT here would \
             show stale chrome pixels on screen",
        );
    }

    // cache_enabled=false must never consult (or update) the cache, even
    // when one that would otherwise match is passed in -- the
    // LUMEN_NO_CHROME_OVERLAY_CACHE=1 A/B lever's whole point.
    let (framed_disabled, _, _, cache_disabled) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, false, Some(&cache0));
    assert_eq!(framed_disabled, framed0, "disabled arm must still build the correct bytes");
    assert!(cache_disabled.is_none(), "disabled arm must not remember a cache either");
}

// -- BUG-405 срез 51: re-measure the net win at strips_used=4 --

/// Срез 50's "Остаток" promised a re-measurement on a multi-band layout
/// (sidebar + narrow window, all four overlay strips active) вЂ” the
/// `chrome_mix` multiplier its `bench-text-scroll.html` stand never
/// exercised (window maximized there в†’ only the top strip is
/// non-degenerate, `strips_used=[1]`).
///
/// A live-window census for that layout turns out to be unreachable through
/// the existing MCP automation surface: `AutomationCommand::Click` resolves
/// its `Target` (`Point`/`NodeId`/`Selector`, `crates/shell/src/lumen/
/// automation.rs::resolve_automation_target`) purely against
/// `self.layout_box`/`self.layout_source` вЂ” the PAGE document вЂ” and then
/// calls `Lumen::handle_click_at` directly. That is a different path from a
/// real winit `MouseInput` event, which checks `self.point_over_chrome`
/// FIRST and dispatches to `chrome_hit_test`/`dispatch_chrome_action`
/// before ever reaching page hit-testing
/// (`crates/shell/src/app/window_event/mouse_input.rs`). No chrome control
/// вЂ” the vertical-tabs toggle (`Ctrl+B`, no chrome UI equivalent either),
/// `data-action="open-web-sidebar"`, `data-action="open-ai-sidebar"` вЂ” is
/// reachable from a census script driving the live window over MCP, and
/// none of these toggles persist across a fresh launch to be pre-seeded via
/// config. So this slice measures the same pure function directly instead
/// вЂ” no live window, GPU or MCP needed at all, same reasoning as срез 49's
/// "a whole `Lumen` for one diagnostic is disproportionate".
///
/// Host/window numbers are copied from `bug405_slice50_...`'s correctness
/// fixture, which already happens to leave a margin on all four sides
/// (`strips_used == 4`, asserted below) вЂ” that test just never timed
/// anything.
///
/// `#[ignore]`d like срез 50's sibling gates вЂ” run explicitly:
/// `cargo test -p lumen-shell --profile dev-release bug405_slice51 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 51) вЂ” doc comment has the run command"]
fn bug405_slice51_chrome_overlay_cache_net_win_at_four_active_strips() {
    // ~130 commands в€’ `build: chrome` census (срез 50) logged `cmds=130` at
    // strips_used=1 on the real chrome document, so this fixture's `chrome_dl`
    // matches that order of magnitude instead of the single-`FillRect` toy
    // срез 50's correctness test used (fine for byte-equality, too small for
    // a stable timing signal here).
    let chrome_dl: Vec<lumen_paint::DisplayCommand> = (0..130)
        .map(|i| lumen_paint::DisplayCommand::FillRect {
            rect: Rect::new(i as f32, 0.0, 40.0, 20.0),
            color: lumen_layout::Color { r: 10, g: 20, b: 30, a: 255 },
        })
        .collect();
    let host = Rect::new(200.0, 40.0, 800.0, 700.0);
    let (win_w, win_h) = (1024.0_f32, 768.0_f32);
    let caret = Some((
        Rect::new(300.0, 10.0, 2.0, 20.0),
        lumen_layout::Color { r: 0, g: 120, b: 220, a: 220 },
    ));

    let (_, strips_used, _, cache) =
        chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, None);
    assert_eq!(
        strips_used, 4,
        "fixture must exercise all four strips вЂ” the chrome_mix multiplier this slice measures",
    );
    let cache = cache.expect("cold build must produce a cache to remember");

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;

    let mut hit_stats = lumen_paint::FrameStats::new();
    let mut rebuild_stats = lumen_paint::FrameStats::new();
    // Interleaved HIT-then-rebuild each round (docs/perf-method.md) вЂ” both
    // arms see the same cache/allocator warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let (framed, _, _, new_cache) =
            chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, true, Some(&cache));
        let hit_ms = t0.elapsed().as_secs_f32() * 1000.0;
        assert!(new_cache.is_none(), "must stay a HIT for the whole loop");
        std::hint::black_box(&framed);

        let t1 = std::time::Instant::now();
        let (framed2, strips2, _, _) =
            chrome_overlay_segment(&chrome_dl, host, win_w, win_h, caret, 1, false, None);
        let rebuild_ms = t1.elapsed().as_secs_f32() * 1000.0;
        assert_eq!(strips2, 4);
        std::hint::black_box(&framed2);

        if i >= WARMUP {
            hit_stats.record(hit_ms);
            rebuild_stats.record(rebuild_ms);
        }
    }

    let hit_summary = hit_stats.summary().expect("samples collected");
    let rebuild_summary = rebuild_stats.summary().expect("samples collected");
    eprintln!("{}", hit_summary.display_with("BUG405_S51_HIT_4STRIP"));
    eprintln!("{}", rebuild_summary.display_with("BUG405_S51_REBUILD_4STRIP"));
    let saved = (1.0 - hit_summary.min_ms / rebuild_summary.min_ms) * 100.0;
    // срез 50 measured -20% on the live stand at strips_used=1 (one strip's
    // worth of `chrome_dl` copied either way) вЂ” compare this number against
    // that baseline, not against 0%.
    eprintln!("cache-on saves {saved:.1}% at strips_used=4 (by min of {SAMPLES} interleaved samples)");
}

// -- BUG-405 срез 52: is strips_used=4 even reachable on the real chrome layout? --

/// Builds the real chrome `(host_rect, chrome_dl)` pair `relayout_chrome_host`
/// itself would produce вЂ” срез 49's level (no live `Lumen`/renderer needed):
/// the actual `(doc, sheet)` asset, a `ChromeModel` with every panel that can
/// widen a strip turned on, run through the same `layout_measured_hyp`/
/// `take_content_area` pair production uses, `dl` built AFTER pruning like
/// `relayout_chrome_host` does. Shared by both srez-52 tests below so the
/// timing gate does not duplicate the fixture-building the correctness gate
/// already covers.
fn bug405_slice52_real_chrome_overlay_fixture() -> (Rect, lumen_paint::DisplayList, (f32, f32)) {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    // Every panel that can widen a strip, turned on at once вЂ” the real-UI
    // analogue of срез 51's "all four margins" intent: vertical sidebar
    // (left), `#rightSidebar` (right). `#findBar`/`#downloadsPanel` are
    // turned on too, but CC-9's own doc comment on `relayout_chrome_host`
    // says both are salvaged out of `#contentArea` BEFORE `page_host_rect` is
    // computed, i.e. they overlay the content area rather than resizing it,
    // so they should not be able to add a strip even in principle вЂ” the
    // `strips_used` assertion below exists to confirm that reading holds,
    // not to assume it.
    let mut model = cc12_bench_model("");
    model.layout_vertical = true;
    model.sidebar_collapsed = false;
    model.right_sidebar.open = true;
    model.find.open = true;
    model.downloads_open = true;

    let _ = lumen_chrome::bind_model_tracked(&mut doc, &model);
    let mut layout = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);

    let content_area = doc
        .find_by_id(lumen_chrome::ids::CONTENT_AREA)
        .expect("chrome preview must have #contentArea");
    let (host_rect, _detached) = take_content_area(
        &mut layout,
        content_area,
        &[
            lumen_chrome::ids::FIND_BAR,
            lumen_chrome::ids::DOWNLOADS_PANEL,
            lumen_chrome::ids::CP_OVERLAY,
            lumen_chrome::ids::CERT_OVERLAY,
            lumen_chrome::ids::PRINT_OVERLAY,
        ],
        &doc,
    )
    .expect("#contentArea must have a box to prune");

    // Same order as `relayout_chrome_host`: `dl` is built from `layout`
    // AFTER `#contentArea` is pruned out of it вЂ” this is the real
    // `chrome_dl`, not срез 51's synthetic 130-`FillRect` stand-in.
    let chrome_dl = paint_ordered(&layout);
    (host_rect, chrome_dl, (viewport.width, viewport.height))
}

/// Срез 51's own remainder flagged this: it measured the `chrome_mix`
/// multiplier at `strips_used=4` on a HAND-PICKED `Rect` (copied from срез
/// 50's correctness fixture, which "just happens" to leave a margin on all
/// four sides) вЂ” not on any host rect the real chrome layout ever produces.
///
/// Reading `assets/chrome/chrome.html`'s own CSS answers this ahead of the
/// test running: `.body-row{flex:1}` (the flex row holding `#contentArea` +
/// `#rightSidebar`) fills 100% of the height left under `.toolbar`, and
/// nothing downstream of it is shorter than its container вЂ” there is no
/// element anywhere in the asset that could leave a gap between
/// `#contentArea`'s bottom edge and the window's. `strips_used` should
/// therefore cap at 3 (top toolbar + left vertical-sidebar + right
/// `#rightSidebar`), never reach 4 вЂ” срез 51's `chrome_mix` scenario names a
/// shape the real UI cannot produce.
#[test]
fn bug405_slice52_real_chrome_layout_caps_at_three_active_strips() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (_, strips_used, _, _) = chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
    eprintln!(
        "BUG405_S52 host_rect={host_rect:?} viewport=({win_w}, {win_h}) strips_used={strips_used} \
         cmds={}",
        chrome_dl.len(),
    );
    assert_eq!(
        strips_used, 3,
        "the asset's CSS (`.body-row{{flex:1}}` leaves no vertical gap below #contentArea) predicts a \
         structural ceiling of 3 non-degenerate strips (top/left/right) on the real chrome layout, never \
         срез 51's 4 вЂ” if this fails, either the asset changed to add a bottom-shrinking element (a new \
         strip is genuinely reachable, срез 51's number is back in play) or this reasoning was wrong \
         (investigate before trusting either arm's percentage)",
    );
}

/// The net win at the real `strips_used` ceiling (срез 52's correctness gate
/// above measures it at 3, not срез 51's synthetic 4) вЂ” real `host_rect` and
/// real `chrome_dl` (actual command count from the real chrome layout, not
/// срез 51's 130-`FillRect` stand-in). `#[ignore]`d like срез 51's sibling
/// gate вЂ” run explicitly:
/// `cargo test -p lumen-shell --profile dev-release bug405_slice52 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 52) вЂ” doc comment has the run command"]
fn bug405_slice52_chrome_overlay_cache_net_win_on_real_layout() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (_, strips_used, _, cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, None);
    let cache = cache.expect("cold build must produce a cache to remember");

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;

    let mut hit_stats = lumen_paint::FrameStats::new();
    let mut rebuild_stats = lumen_paint::FrameStats::new();
    // Same interleaved-per-round shape as срез 51 (docs/perf-method.md) вЂ” both
    // arms see the same cache/allocator warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let (framed, _, _, new_cache) =
            chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, Some(&cache));
        let hit_ms = t0.elapsed().as_secs_f32() * 1000.0;
        assert!(new_cache.is_none(), "must stay a HIT for the whole loop");
        std::hint::black_box(&framed);

        let t1 = std::time::Instant::now();
        let (framed2, strips2, _, _) =
            chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
        let rebuild_ms = t1.elapsed().as_secs_f32() * 1000.0;
        assert_eq!(strips2, strips_used);
        std::hint::black_box(&framed2);

        if i >= WARMUP {
            hit_stats.record(hit_ms);
            rebuild_stats.record(rebuild_ms);
        }
    }

    let hit_summary = hit_stats.summary().expect("samples collected");
    let rebuild_summary = rebuild_stats.summary().expect("samples collected");
    eprintln!("{}", hit_summary.display_with("BUG405_S52_HIT_REAL"));
    eprintln!("{}", rebuild_summary.display_with("BUG405_S52_REBUILD_REAL"));
    let saved = (1.0 - hit_summary.min_ms / rebuild_summary.min_ms) * 100.0;
    eprintln!(
        "cache-on saves {saved:.1}% at strips_used={strips_used} on the REAL chrome layout \
         (cmds={}, by min of {SAMPLES} interleaved samples)",
        chrome_dl.len(),
    );
}

// -- BUG-405 срез 55: how much of fold_overlay's cost is the chrome-segment rehash? --

/// Срез 54's census (`bugs/BUG-405-OPEN.md` "Остаток", вариант (б)) narrowed
/// п.85 to one concrete question: `fold_overlay` (`display_list.rs:1970`)
/// hashes EVERY overlay command every frame with `hash_one_command`,
/// including the `chrome_dl` segment a `ChromeOverlayFrameCache` HIT already
/// proves byte-identical to every earlier HIT since the cache was built — but
/// is that rehash actually a measurable share of `fold_overlay`'s own cost,
/// or (as срез 47's numbers suggest — "послекэша" 0.14→0.02мс after the
/// double-hash was deduplicated) too small already to justify the
/// `content_epoch`-style plumbing a real fix would need? Measured directly
/// (docs/perf-method.md: counter/identity, not wall-clock feel) — no engine
/// code touched, `fold_overlay` runs exactly as it stands on `main` today.
///
/// Fixture: срез 52's REAL chrome segment (cmds=292 chrome_dl → framed at
/// strips_used=3, not a synthetic stand-in) plus the real scrollbar overlay
/// (`scrollbar::build_scrollbar_overlay`, exactly 2 `FillRect`s) — the two
/// overlay sources срез 54's read of `redraw_requested.rs` confirmed are
/// actually present every frame on the hot scrolling path (the other 7
/// builders return an empty `Vec` on a plain page).
///
/// `cargo test -p lumen-shell --profile dev-release bug405_slice55 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 55) — doc comment has the run command"]
fn bug405_slice55_fold_overlay_cost_is_mostly_chrome_segment() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, _, _) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    assert_eq!(scrollbar_cmds.len(), 2, "срез 54's read: exactly track+thumb");

    let mut full_overlay = chrome_segment.clone();
    full_overlay.extend(scrollbar_cmds.iter().cloned());
    let chrome_len = chrome_segment.len();

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;
    let mut full_stats = lumen_paint::FrameStats::new();
    let mut tail_only_stats = lumen_paint::FrameStats::new();
    // Interleaved each round (docs/perf-method.md) — both arms see the same
    // allocator/cache warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let digests_full = lumen_paint::display_list::fold_overlay(&full_overlay);
        let full_ms = t0.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_full);

        let t1 = std::time::Instant::now();
        // The best case a chrome-digest-reuse fix could reach: only the tail
        // (scrollbar) actually gets hashed, the chrome prefix's digests come
        // from `ChromeOverlayFrameCache` instead of `hash_one_command`.
        let digests_tail = lumen_paint::display_list::fold_overlay(&full_overlay[chrome_len..]);
        let tail_ms = t1.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_tail);

        if i >= WARMUP {
            full_stats.record(full_ms);
            tail_only_stats.record(tail_ms);
        }
    }

    let full_summary = full_stats.summary().expect("samples collected");
    let tail_summary = tail_only_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG405_S55_FOLD_FULL"));
    eprintln!("{}", tail_summary.display_with("BUG405_S55_FOLD_TAIL_ONLY"));
    let attributable = (1.0 - tail_summary.min_ms / full_summary.min_ms) * 100.0;
    eprintln!(
        "chrome-segment rehash accounts for {attributable:.1}% of fold_overlay's cost \
         ({chrome_len} chrome cmds vs {} scrollbar cmds, min of {SAMPLES} interleaved samples, \
         full={:.4}ms tail={:.4}ms)",
        full_overlay.len() - chrome_len,
        full_summary.min_ms,
        tail_summary.min_ms,
    );
}

// -- BUG-405 срез 56: срез 55's fixture put chrome first — real order is scrollbar first --

/// Reading `redraw_requested.rs`'s Step 6 top-down (not just the caret/cache
/// doc comments срезы 50/54/55 already read) shows the common live-scrolling
/// case — no find-bar, no validation tooltip, no color/date picker, no
/// `<dialog>`, no view transition, no hint overlay, i.e. every block between
/// the chrome step and the scrollbar step is a conditional `if let`/`if` that
/// is false on a plain page — builds `overlay_buf` with exactly two
/// `Vec::append` calls:
///
/// 1. chrome step: `framed.append(&mut overlay_buf)` while `overlay_buf` is
///    still empty, then `overlay_buf = framed` — chrome only, so far.
/// 2. scrollbar step: `combined = scrollbar_cmds; combined.append(&mut
///    overlay_buf)`, then `overlay_buf = combined`.
///
/// `Vec::append(&mut other)` keeps `self`'s elements first and moves
/// `other`'s elements after them, so step 2 puts the **scrollbar first,
/// chrome second** — the reverse of срез 55's `full_overlay = chrome_segment;
/// extend(scrollbar_cmds)` fixture. Every later overlay builder in the same
/// function (tooltip/pickers/dialog/view-transition) uses the identical
/// `X.append(&mut overlay_buf); overlay_buf = X;` prepend shape, so any of
/// them firing that frame also lands in front of chrome — chrome's start
/// offset inside `overlay_buf` is not a fixed prefix length at all, it moves
/// with whichever of those builders ran, and even the bare scrollbar-only
/// frame this test measures puts chrome at offset 2, not 0.
///
/// This does not move срез 55's headline number (rehashing the ~292-command
/// chrome segment dominates `fold_overlay`'s cost regardless of which end of
/// the array it sits at — the command count `hash_one_command` walks is
/// invariant to order), but it does invalidate the "cache by prefix length"
/// framing срез 54 suggested: a real digest-reuse mechanism cannot ask the
/// `Renderer` to "reuse the first K digests", because the K commands at the
/// front are frequently NOT the cached chrome segment. It needs a
/// caller-declared `(start, len)` RANGE, recomputed every frame from the
/// actual composition (`scrollbar_cmds.len()` when the scrollbar drew, `0`
/// otherwise, before any prepending overlay builder ran) — a fixed prefix or
/// suffix slot is the wrong shape for this cache's key. No engine code
/// touched, `fold_overlay` runs exactly as it stands on `main` today.
///
/// `cargo test -p lumen-shell --profile dev-release bug405_slice56 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 56) — doc comment has the run command"]
fn bug405_slice56_fold_overlay_cost_holds_with_real_command_order() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, _, _) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, false, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    assert_eq!(scrollbar_cmds.len(), 2, "срез 54's read: exactly track+thumb");

    // Real `redraw_requested.rs` order on the common live-scrolling frame:
    // scrollbar (volatile, changes with `scroll_y` every frame) FIRST,
    // chrome (stable across a `chrome_layout_generation`) SECOND — the
    // reverse of срез 55's fixture.
    let mut full_overlay = scrollbar_cmds.clone();
    full_overlay.extend(chrome_segment.iter().cloned());
    let scrollbar_len = scrollbar_cmds.len();

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;
    let mut full_stats = lumen_paint::FrameStats::new();
    let mut volatile_only_stats = lumen_paint::FrameStats::new();
    // Interleaved each round (docs/perf-method.md) — both arms see the same
    // allocator/cache warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let digests_full = lumen_paint::display_list::fold_overlay(&full_overlay);
        let full_ms = t0.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_full);

        let t1 = std::time::Instant::now();
        // Best case a range-aware chrome-digest-reuse fix could reach: only
        // the volatile prefix (scrollbar) gets hashed, chrome's digests are
        // assumed supplied by `ChromeOverlayFrameCache` at whatever offset it
        // actually starts at this frame.
        let digests_volatile =
            lumen_paint::display_list::fold_overlay(&full_overlay[..scrollbar_len]);
        let volatile_ms = t1.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_volatile);

        if i >= WARMUP {
            full_stats.record(full_ms);
            volatile_only_stats.record(volatile_ms);
        }
    }

    let full_summary = full_stats.summary().expect("samples collected");
    let volatile_summary = volatile_only_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG405_S56_FOLD_FULL"));
    eprintln!("{}", volatile_summary.display_with("BUG405_S56_FOLD_VOLATILE_ONLY"));
    let attributable = (1.0 - volatile_summary.min_ms / full_summary.min_ms) * 100.0;
    eprintln!(
        "chrome-segment rehash accounts for {attributable:.1}% of fold_overlay's cost with the \
         REAL command order (scrollbar first, chrome second) — confirms срез 55's number under \
         the corrected fixture ({} chrome cmds vs {scrollbar_len} scrollbar cmds, min of \
         {SAMPLES} interleaved samples, full={:.4}ms volatile-only={:.4}ms)",
        chrome_segment.len(),
        full_summary.min_ms,
        volatile_summary.min_ms,
    );
}

// -- BUG-405 срез 57: thread ChromeOverlayFrameCache's digest into fold_overlay --

/// Срез 56 fixed the reuse mechanism's shape (a `(start, len)` range
/// recomputed from the actual composition, not a fixed prefix/suffix slot).
/// This slice implements it: `ChromeOverlayFrameCache` now also remembers
/// `fold_overlay(&framed)` (`chrome_ui.rs`'s `digests` field), and
/// `fold_overlay_with_reuse` (`lumen_paint::display_list`) hashes only the
/// commands OUTSIDE the declared range, splicing in the cached tail
/// unchanged.
///
/// Correctness gate: on the real command order (scrollbar first, chrome
/// second — срез 56), reusing the chrome segment's cached digest must give
/// BIT-IDENTICAL output to a full `fold_overlay` — a false hit here means a
/// wrong pixel comparison downstream (`overlay_cache_step`/the frame hash),
/// not just a slower frame.
#[test]
fn bug405_slice57_fold_overlay_with_reuse_matches_full_fold_on_real_order() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, digests0, cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");
    let cache = cache.expect("cold build must produce a cache to remember");

    // Second call, same key -- must be a HIT, and its digest must equal the
    // cold build's own fold (not just same length).
    let (chrome_segment2, _, chrome_digests, new_cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, Some(&cache));
    assert!(new_cache.is_none(), "unchanged key must stay a HIT");
    assert_eq!(chrome_segment2, chrome_segment, "HIT must reuse the exact same bytes");
    assert_eq!(chrome_digests, digests0, "HIT digest must equal the cold build's own fold");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    let mut full_overlay = scrollbar_cmds.clone();
    full_overlay.extend(chrome_segment2.iter().cloned());
    let chrome_start = scrollbar_cmds.len();

    let expected = lumen_paint::display_list::fold_overlay(&full_overlay);
    let actual = lumen_paint::display_list::fold_overlay_with_reuse(
        &full_overlay,
        Some(&(chrome_start, chrome_digests)),
    );
    assert_eq!(
        actual, expected,
        "reused digest must be bit-identical to a full recompute -- a mismatch here would silently \
         feed a wrong per-command hash into overlay_cache_step/the frame hash, i.e. a false cache HIT \
         (wrong pixel forever, not just a slow frame)",
    );
}

/// A `(start, digests)` whose length does not fit `overlay.len()` -- a stale
/// hint from a shorter/longer buffer than the one it was computed for,
/// exactly what `overlay_len_after_prepend_phase` in `redraw_requested.rs`
/// guards against by construction, but this is the last line of defence
/// inside `fold_overlay_with_reuse` itself -- must fall back to a full
/// recompute, not panic or silently misalign.
#[test]
fn bug405_slice57_fold_overlay_with_reuse_falls_back_on_length_mismatch() {
    let overlay = vec![
        lumen_paint::DisplayCommand::FillRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: lumen_layout::Color { r: 1, g: 2, b: 3, a: 255 },
        },
        lumen_paint::DisplayCommand::FillRect {
            rect: Rect::new(10.0, 0.0, 10.0, 10.0),
            color: lumen_layout::Color { r: 4, g: 5, b: 6, a: 255 },
        },
    ];
    let expected = lumen_paint::display_list::fold_overlay(&overlay);

    // Stale hint: claims a tail of 5 digests, buffer only has 2 commands.
    let stale = (0usize, vec![1u64, 2, 3, 4, 5]);
    let actual = lumen_paint::display_list::fold_overlay_with_reuse(&overlay, Some(&stale));
    assert_eq!(actual, expected, "length mismatch must fall back to a full recompute");
}

/// End-to-end perf gate: the actual win `fold_overlay_with_reuse` gives on
/// the real command order, chrome digest supplied the way
/// `redraw_requested.rs` now supplies it (a `ChromeOverlayFrameCache` HIT).
/// Comparable to срезы 55/56's headline number, but measuring the REAL
/// entry point instead of the "best case" `full_overlay[chrome_len..]`
/// slice those two used as a stand-in before this mechanism existed.
///
/// `cargo test -p lumen-shell --profile dev-release bug405_slice57 -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf gate (BUG-405 срез 57) — doc comment has the run command"]
fn bug405_slice57_fold_overlay_with_reuse_net_win_on_real_order() {
    let (host_rect, chrome_dl, (win_w, win_h)) = bug405_slice52_real_chrome_overlay_fixture();
    let (chrome_segment, strips_used, _, cache) =
        chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, None);
    assert_eq!(strips_used, 3, "must match срез 52's real-layout ceiling");
    let cache = cache.expect("cold build must produce a cache to remember");

    let scrollbar_cmds = scrollbar::build_scrollbar_overlay(400.0, 4000.0, win_w, win_h);
    let mut full_overlay = scrollbar_cmds.clone();
    full_overlay.extend(chrome_segment.iter().cloned());
    let chrome_start = scrollbar_cmds.len();

    const WARMUP: usize = 20;
    const SAMPLES: usize = 500;
    let mut full_stats = lumen_paint::FrameStats::new();
    let mut reuse_stats = lumen_paint::FrameStats::new();
    // Interleaved each round (docs/perf-method.md) — both arms see the same
    // allocator/cache warmth instead of one racing first.
    for i in 0..WARMUP + SAMPLES {
        let t0 = std::time::Instant::now();
        let digests_full = lumen_paint::display_list::fold_overlay(&full_overlay);
        let full_ms = t0.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_full);

        // The real entry point: a fresh HIT lookup (as `redraw_requested.rs`
        // does every frame) feeds its digest into `fold_overlay_with_reuse`.
        let t1 = std::time::Instant::now();
        let (_, _, chrome_digests, new_cache) =
            chrome_overlay_segment(&chrome_dl, host_rect, win_w, win_h, None, 1, true, Some(&cache));
        assert!(new_cache.is_none(), "must stay a HIT for the whole loop");
        let digests_reused = lumen_paint::display_list::fold_overlay_with_reuse(
            &full_overlay,
            Some(&(chrome_start, chrome_digests)),
        );
        let reuse_ms = t1.elapsed().as_secs_f32() * 1000.0;
        std::hint::black_box(&digests_reused);

        if i >= WARMUP {
            full_stats.record(full_ms);
            reuse_stats.record(reuse_ms);
        }
    }

    let full_summary = full_stats.summary().expect("samples collected");
    let reuse_summary = reuse_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG405_S57_FOLD_FULL"));
    eprintln!("{}", reuse_summary.display_with("BUG405_S57_FOLD_REUSE"));
    let saved = (1.0 - reuse_summary.min_ms / full_summary.min_ms) * 100.0;
    eprintln!(
        "chrome-digest reuse saves {saved:.1}% of fold_overlay's cost on the real entry point \
         ({} chrome cmds vs {} scrollbar cmds, min of {SAMPLES} interleaved samples, \
         full={:.4}ms reuse={:.4}ms)",
        chrome_segment.len(),
        scrollbar_cmds.len(),
        full_summary.min_ms,
        reuse_summary.min_ms,
    );
}

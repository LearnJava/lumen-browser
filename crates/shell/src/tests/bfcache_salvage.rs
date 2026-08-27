//! BUG-341 S22: отсоединение и восстановление поддеревьев `#contentArea`.

use super::*;

#[test]
fn take_content_area_with_no_salvage_ids_behaves_like_a_plain_prune() {
    let html = concat!(
        "<html><body>",
        "<div id=\"contentArea\"><div id=\"placeholder\">demo</div></div>",
        "</body></html>",
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let mut layout = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let content_area = doc.find_by_id("contentArea").expect("fixture has #contentArea");

    assert!(take_content_area(&mut layout, content_area, &[], &doc).is_some());
    assert!(lumen_layout::find_box_by_node(&layout, content_area).is_none());
    let placeholder = doc.find_by_id("placeholder").expect("fixture has #placeholder");
    assert!(lumen_layout::find_box_by_node(&layout, placeholder).is_none());
}

// в”Ђв”Ђ BUG-341 S22: the pruning is reversible в”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђв”Ђ

/// The salvage list `relayout_chrome_host` passes to `take_content_area`.
const S22_SALVAGE_IDS: [&str; 5] = [
    lumen_chrome::ids::FIND_BAR,
    lumen_chrome::ids::DOWNLOADS_PANEL,
    lumen_chrome::ids::CP_OVERLAY,
    lumen_chrome::ids::CERT_OVERLAY,
    lumen_chrome::ids::PRINT_OVERLAY,
];

/// Box-for-box identity of two chrome trees, for BUG-341 S22's round-trip
/// gate. Style is compared by `Arc` **identity**, not by value: the
/// restored tree must hold the very boxes that were detached, not
/// equivalent ones.
fn s22_assert_identical(
    a: &lumen_layout::LayoutBox,
    b: &lumen_layout::LayoutBox,
    path: &mut Vec<String>,
) {
    let at = || path.join(">");
    assert_eq!(a.node, b.node, "{}: node id", at());
    assert_eq!(a.rect, b.rect, "{}: rect", at());
    assert_eq!(
        std::mem::discriminant(&a.kind),
        std::mem::discriminant(&b.kind),
        "{}: BoxKind discriminant",
        at(),
    );
    assert!(std::sync::Arc::ptr_eq(&a.style, &b.style), "{}: style is a different Arc", at());
    assert_eq!(a.children.len(), b.children.len(), "{}: children.len()", at());
    for (i, (ca, cb)) in a.children.iter().zip(b.children.iter()).enumerate() {
        path.push(format!("child[{i}] node={:?}", ca.node));
        s22_assert_identical(ca, cb, path);
        path.pop();
    }
}

/// BUG-341 S22 soundness gate: `restore_content_area` reproduces the
/// pre-pruning tree exactly, on the real chrome document.
///
/// Exactness is the contract, not a nicety. The restored tree becomes the
/// next pass's `prev` basis, and `incremental_build_box` moves whole clean
/// subtrees across from it вЂ” so whatever the basis is missing or has in the
/// wrong slot, the produced document is missing or has in the wrong slot
/// too (`bug341_s22_a_restored_basis_carries_the_whole_document_forward`
/// measures exactly that failure). Comparing `style` by `Arc` identity is
/// part of the point: the restore must hand back the boxes it took, not
/// equivalent ones.
///
/// The box-count assert is the other arm: without it the test would pass
/// just as well if `take_content_area` had quietly stopped pruning.
#[test]
fn bug341_s22_restoring_a_detachment_reproduces_the_pristine_tree() {
    let (doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let (mut layout, _counters) = lumen_layout::layout_measured_hyp_with_counters(
        &doc, &sheet, viewport, &measurer, &hyp, false,
    );
    let pristine = layout.clone();
    let content_area = doc
        .find_by_id(lumen_chrome::ids::CONTENT_AREA)
        .expect("chrome document has #contentArea");

    let (_rect, detached) = take_content_area(&mut layout, content_area, &S22_SALVAGE_IDS, &doc)
        .expect("#contentArea has a layout box");
    assert!(
        census_count_boxes(&layout) < census_count_boxes(&pristine),
        "pruning must actually remove boxes, otherwise the round-trip below is a no-op",
    );

    assert!(restore_content_area(&mut layout, detached), "restore must find every recorded path");
    s22_assert_identical(&layout, &pristine, &mut vec!["root".to_owned()]);
}

/// BUG-341 S22: the same round-trip over the salvage path, which the real
/// chrome document cannot exercise at rest.
///
/// Every salvageable popover (`#findBar`, `#downloadsPanel`, the CC-10
/// modals) is `display:none` until opened, so it has no box and
/// `take_content_area` salvages nothing вЂ” the gate above therefore runs
/// with an empty `salvage_paths` and would not notice if the salvage half
/// of the restore were broken. This fixture nests one popover inside
/// another element so the recorded paths are more than one level deep and
/// their removal order matters: restoring in the wrong order, or against
/// the wrong parent, misplaces a box and `s22_assert_identical` says so.
#[test]
fn bug341_s22_restoring_puts_salvaged_popovers_back_where_they_came_from() {
    let html = concat!(
        "<html><body>",
        "<div id=\"before\"></div>",
        "<div id=\"contentArea\">",
        "<div id=\"placeholder\">demo<div id=\"downloadsPanel\">downloads</div></div>",
        "<div id=\"findBar\">find</div>",
        "</div>",
        "<div id=\"after\"></div>",
        "</body></html>",
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("");
    let mut layout = lumen_layout::layout(&doc, &sheet, Size::new(800.0, 600.0));
    let pristine = layout.clone();
    let content_area = doc.find_by_id("contentArea").expect("fixture has #contentArea");

    let (_rect, detached) =
        take_content_area(&mut layout, content_area, &["findBar", "downloadsPanel"], &doc)
            .expect("#contentArea has a layout box");
    assert_eq!(detached.salvage_paths.len(), 2, "both popovers must be salvaged");
    assert!(
        detached.salvage_paths.iter().any(|p| p.len() > 1),
        "the nested popover's path must be deeper than one level, or this fixture is not \
             exercising what it was written for",
    );

    assert!(restore_content_area(&mut layout, detached), "restore must find every recorded path");
    s22_assert_identical(&layout, &pristine, &mut vec!["root".to_owned()]);
}

/// BUG-341 S22 counter gate: three production-shaped chrome cycles,
/// returning the last cycle's `(built, reused, boxes_produced)`.
///
/// `restore` picks the arm: `true` is production (undo the pruning, hand
/// the pristine tree on), `false` feeds the pruned tree straight back as
/// the basis вЂ” the mistake this slice's design makes possible.
fn s22_pipeline_cycles(restore: bool) -> (u32, u32, u64) {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter РЅРµ РїР°СЂСЃРёС‚СЃСЏ");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer РёР· bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let mut live: Option<(lumen_layout::LayoutBox, Option<ContentAreaDetachment>)> = None;
    let mut prev_cascade_styles = lumen_layout::CascadeStyles::default();
    let mut typed = String::new();
    let mut last = (0, 0);
    let mut last_boxes = 0;
    for _ in 0..3 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        let touched = lumen_chrome::bind_model_tracked(&mut doc, &model);
        lumen_layout::set_interactive_state(None, None, None);
        let basis = live.take().and_then(|(mut lb, detached)| match detached {
            Some(d) if restore => restore_content_area(&mut lb, d).then_some(lb),
            _ => Some(lb),
        });
        let (mut layout, counters) = match basis {
            Some(prev) => {
                let node_index = lumen_layout::style::restyle_node_index(&doc, &sheet);
                let dirty_roots: std::collections::HashSet<_> =
                    lumen_layout::style::restyle_root_set_for_node_change(
                        &doc,
                        chrome_node_changes(&touched),
                        &node_index,
                    )
                    .into_iter()
                    .collect();
                let delta = lumen_layout::counters::RestyleDelta {
                    prev_styles: std::mem::take(&mut prev_cascade_styles),
                    dirty_roots,
                    content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                };
                lumen_layout::counters::set_incremental_restyle(true);
                lumen_layout::box_tree::set_incremental_box_build(true);
                let result = lumen_layout::box_tree::layout_mutation_incremental_restyle(
                    &doc, &sheet, viewport, &measurer, &hyp, false, prev, delta,
                );
                lumen_layout::box_tree::set_incremental_box_build(false);
                lumen_layout::counters::set_incremental_restyle(false);
                result
            }
            None => lumen_layout::layout_measured_hyp_with_counters(
                &doc, &sheet, viewport, &measurer, &hyp, false,
            ),
        };
        let bb = lumen_layout::box_tree::take_box_build_stats();
        last = (bb.built, bb.reused);
        last_boxes = census_count_boxes(&layout);
        let detached = doc
            .find_by_id(lumen_chrome::ids::CONTENT_AREA)
            .and_then(|id| take_content_area(&mut layout, id, &S22_SALVAGE_IDS, &doc))
            .map(|(_rect, d)| d);
        live = Some((layout, detached));
        prev_cascade_styles = counters.into_styles();
        lumen_layout::clear_interactive_state();
    }
    (last.0, last.1, last_boxes)
}

/// BUG-341 S22 gate, both arms: what a chrome cycle produces when its
/// basis was restored, versus when the pruned tree is fed straight back.
///
/// The measured answer, and the reason this slice needs a gate of its own:
/// a pruned basis does not merely cost a rebuild, it produces a **wrong
/// tree**. `#contentArea`'s parent is clean on an interaction cycle, so
/// `incremental_build_box` moves that whole subtree over from `prev` in
/// O(1) вЂ” and a `prev` missing `#contentArea` therefore yields a document
/// missing `#contentArea`, permanently, 155 boxes instead of 318. It never
/// recovers, because the next cycle's basis is that same tree.
///
/// Wall-clock cannot gate this and neither can the differential suite: the
/// chrome host paints the real page over `#contentArea`'s rect anyway, so
/// the visible frame of a wrong-basis run is very nearly the right one.
/// Box counts are the observable, which is the lesson S8 wrote for the
/// whole track.
#[test]
fn bug341_s22_a_restored_basis_carries_the_whole_document_forward() {
    let (built_restored, reused_restored, boxes_restored) = s22_pipeline_cycles(true);
    let (built_pruned, reused_pruned, boxes_pruned) = s22_pipeline_cycles(false);
    eprintln!(
        "[s22-gate] restored built={built_restored} reused={reused_restored} \
             boxes={boxes_restored} | pruned-basis built={built_pruned} \
             reused={reused_pruned} boxes={boxes_pruned}"
    );
    assert!(
        boxes_restored > boxes_pruned,
        "a restored basis must carry the whole document forward, a pruned one must not \
             ({boxes_restored} vs {boxes_pruned}) вЂ” equal counts mean either the restore is a \
             no-op or the pruning is, and this gate is then measuring nothing",
    );
    // The steady-state cycle of the production arm must still be an
    // *incremental* one: a restore that quietly failed would fall back to
    // a full layout, which also yields the whole document вЂ” and would be
    // the slow frame this slice exists to avoid.
    assert!(
        built_restored < 100 && reused_restored > 0,
        "the restored arm's steady-state cycle must stay incremental \
             (built={built_restored} reused={reused_restored}) вЂ” a full-layout fallback \
             builds every one of the document's boxes",
    );
}

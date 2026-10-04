//! Ручные переписи и замеры BUG-341 — все под `#[ignore]`.
//!
//! Это диагностика, а не гейт: каждая печатает распределение и запускается
//! поимённо, команда — в док-комментарии самого теста.

use super::*;

use super::bug341_census_b::census_subtree_elements;

/// BUG-341 S13 diagnostic: census of *why* `graft_geometry` refuses boxes
/// on the two CC-12 interaction shapes.
///
/// S12's detail scopes established that a hover flip touching one subtree
/// still re-lays-out ~1700 of ~3100 boxes, and that both `build_box` and
/// `lay_out` are close to linear in that number — but not whether those
/// boxes genuinely changed. This prints the partition
/// (`reused_clean` / identity / style / child-count / descendant) plus the
/// share of style rejects that vanish once the used-value writeback fields
/// are discounted. Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s13_graft_reject_census -- --ignored --nocapture`.
#[test]
#[ignore = "manual diagnostic (BUG-341 S13) — see doc comment for run command"]
fn bug341_s13_graft_reject_census() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let model = cc12_bench_model("");
    lumen_chrome::bind_model(&mut doc, &model);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);
    let tabs_container = doc
        .find_by_id(lumen_chrome::ids::SB_TABS)
        .expect("chrome preview must have #sbTabs");
    let tab_rows = doc.get(tabs_container).children.clone();
    let (tab_a, tab_b) = (Some(tab_rows[0]), Some(tab_rows[1]));

    lumen_layout::incremental::set_graft_diagnostics(true);

    for (label, targets) in [
        ("CC12_HOVER(sidebar/none)", [sidebar, None]),
        ("SIBLING_HOVER(tabA/tabB)", [tab_a, tab_b]),
    ] {
        // Reuse off for the same reason as the S13 gate above: this census
        // is about the per-box comparison, which S18's O(1) reuse claim
        // (rightly) skips in production.
        let mut state = Cc12IncrementalState { box_reuse_off: true, ..Default::default() };
        for i in 0..6 {
            let _ = cc12_bench_cycle(
                &mut doc,
                &sheet,
                &model,
                viewport,
                &measurer,
                &hyp,
                targets[i % 2],
                &mut state,
            );
            let s = lumen_layout::incremental::take_graft_stats();
            if i >= 2 {
                eprintln!(
                    "[s13-census] {label} cycle={i} visited={} clean={} \
                         rej_identity={} rej_style={} (used_value_only={}, no_cascade={}, \
                         cascade_differs={}) rej_child_count={} rej_descendant={}",
                    s.visited,
                    s.reused_clean,
                    s.reject_identity,
                    s.reject_style,
                    s.reject_style_used_value_only,
                    s.reject_style_no_cascade_entry,
                    s.reject_style_cascade_differs,
                    s.reject_child_count,
                    s.reject_descendant,
                );
            }
        }
    }
    lumen_layout::incremental::set_graft_diagnostics(false);
}

/// Short human-readable identification of a DOM node, for the census
/// diagnostics below (`div#omniInput.foo`, `text"abc"`).
pub(super) fn census_describe(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> String {
    match &doc.get(id).data {
        lumen_dom::NodeData::Element { name, attrs } => {
            let mut s = name.local.to_string();
            for a in attrs {
                match a.name.local.as_str() {
                    "id" => s.push_str(&format!("#{}", a.value)),
                    "class" => {
                        for c in a.value.split_whitespace() {
                            s.push_str(&format!(".{c}"));
                        }
                    }
                    _ => {}
                }
            }
            s
        }
        lumen_dom::NodeData::Text(t) => {
            format!("text{:?}", t.chars().take(20).collect::<String>())
        }
        other => format!("{other:?}").chars().take(24).collect(),
    }
}

/// BUG-341 S17 diagnostic: census of *why* a keystroke re-cascades and
/// rebuilds what it does.
///
/// S16 left `build_box` as the largest stage on `CC12_KEY` with the census
/// "38 boxes built for one typed character, against 3 on a hover frame".
/// That number is downstream of the cascade: every re-cascaded node loses
/// its box (`must_recompute` ⇒ not in `clean_subtrees`). This prints, for
/// the keystroke cycle, the mutation the tracker reported, the restyle
/// root-set derived from it, how many nodes that root-set re-cascaded, and
/// — the load-bearing column — how many of those re-cascaded nodes ended up
/// with a **different** `ComputedStyle` than the one they already had.
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s17_keystroke_restyle_census -- --ignored --nocapture`.
#[test]
#[ignore = "manual diagnostic (BUG-341 S17) — see doc comment for run command"]
fn bug341_s17_keystroke_restyle_census() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    for i in 0..4 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        let touched = lumen_chrome::bind_model_tracked(&mut doc, &model);
        lumen_layout::set_interactive_state(None, None, None);

        let (layout, counters) = match state.prev_pristine_layout.take() {
            Some(prev) => {
                let node_index = lumen_layout::style::restyle_node_index(&doc, &sheet);
                let dirty_roots = lumen_layout::style::restyle_root_set_for_node_change(
                    &doc,
                    chrome_node_changes(&touched),
                    &node_index,
                );
                if i == 3 {
                    for (n, t) in &touched.selector {
                        eprintln!(
                            "[s17-census] selector-touched: {} attrs={:?} structural={}",
                            census_describe(&doc, *n),
                            t.attrs,
                            t.structural,
                        );
                    }
                    for n in &touched.content {
                        eprintln!("[s17-census] content-touched:  {}", census_describe(&doc, *n));
                    }
                    for n in &dirty_roots {
                        eprintln!(
                            "[s17-census] dirty root: {} (subtree {} elements)",
                            census_describe(&doc, *n),
                            census_subtree_elements(&doc, *n),
                        );
                    }
                    // The chain that cannot be cloned: every ancestor of a
                    // content-dirty node is itself rebuilt, and each rebuild
                    // costs its own box plus one `build_box_or_reuse` call
                    // per child. This is what `boxes_built` is made of once
                    // the cascade root-set is down to one node.
                    let mut chain = Vec::new();
                    let mut cur = touched.content.iter().copied().next();
                    while let Some(n) = cur {
                        chain.push(n);
                        cur = doc.get(n).parent;
                    }
                    for n in chain.iter().rev() {
                        eprintln!(
                            "[s17-census] rebuilt chain: {} ({} children)",
                            census_describe(&doc, *n),
                            doc.get(*n).children.len(),
                        );
                    }
                }
                let delta = lumen_layout::counters::RestyleDelta {
                    prev_styles: std::mem::take(&mut state.prev_cascade_styles),
                    dirty_roots,
                    content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                    shallow_roots: Default::default(), point_roots: Default::default(),
                };
                lumen_layout::counters::set_incremental_restyle(true);
                lumen_layout::box_tree::set_incremental_box_build(true);
                let _ = lumen_layout::counters::take_cascade_stats();
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
        let cs = lumen_layout::counters::take_cascade_stats();
        let bb = lumen_layout::box_tree::take_box_build_stats();

        // Ground truth: of the nodes that re-cascaded, how many actually
        // ended up with a different `ComputedStyle`?
        //
        // BUG-341 S24: read off the displaced-entry record rather than by
        // diffing this pass's map against the previous one — the two are
        // now the same map, and `replaced_styles` holds exactly the
        // "recomputed, and here is what it had before" pairs this census
        // used to reconstruct. Nodes with no previous entry (freshly
        // inserted) are absent from both, as before.
        let mut changed = Vec::new();
        let mut identical = Vec::new();
        for (nid, prev) in counters.replaced_styles() {
            let style = &counters.styles()[nid];
            if **prev == **style {
                identical.push(*nid);
            } else {
                changed.push(*nid);
            }
        }
        if i == 3 {
            for n in &changed {
                eprintln!("[s17-census] style REALLY changed: {}", census_describe(&doc, *n));
            }
            eprintln!(
                "[s17-census] recascaded-but-identical: {}",
                identical
                    .iter()
                    .map(|n| census_describe(&doc, *n))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        eprintln!(
            "[s17-census] cycle={i} cascade_recomputed={} cascade_reused={} \
                 really_changed={} recascaded_identical={} boxes_built={} boxes_reused={}",
            cs.recomputed,
            cs.reused,
            changed.len(),
            identical.len(),
            bb.built,
            bb.reused,
        );

        state.prev_pristine_layout = Some(layout.clone());
        state.prev_cascade_styles = counters.into_styles();
        lumen_layout::clear_interactive_state();
    }
}

/// BUG-341 S18 regression gate: a subtree the box-build stage cloned out of
/// the previous tree must not be walked again by the two stages that follow.
///
/// S15 made a hover frame clone the whole chrome document in one
/// `build_box_or_reuse` call, and S16 made a keystroke clone all of it but
/// the omnibox chain. Both then handed the copy to `mark_subtree_dirty`,
/// which marked all 318 boxes dirty, and to `graft_geometry`, which compared
/// each of them against the very box it had just been copied from and
/// cleared the bit again — two full walks per frame to re-derive a fact the
/// box-build stage already knew (`graft_geometry` was 0.4-1.0 ms of a ~2.6 ms
/// keystroke cycle).
///
/// The gate is the **count**: honouring the claim in O(1) and re-deriving it
/// in O(n) produce the identical tree, so only a counter can tell them apart
/// (S8's lesson), and this fixture's machine noise is wider than the whole
/// effect. `visited` is the number of boxes the graft really compared; it
/// must collapse to the chain the keystroke rebuilt, not the document.
/// Correctness — that the skipped subtrees really are identical, and that a
/// skipped claim never reaches `lay_out` looking clean when it should not —
/// is gated separately in `lumen-layout`'s
/// `mutation_incremental_restyle_*_matches_full` differential tests, which
/// compare geometry against a full pass.
#[test]
fn bug341_s18_reused_subtrees_are_not_re_walked_by_the_graft() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    let mut state = Cc12IncrementalState::default();
    let mut last = lumen_layout::incremental::GraftStats::default();
    for i in 0..4 {
        let hover = if i % 2 == 0 { sidebar } else { None };
        let model = cc12_bench_model("");
        let _ = cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, hover, &mut state);
        last = lumen_layout::incremental::take_graft_stats();
    }
    assert_eq!(
        last.reused_wholesale, 1,
        "a hover flip nothing can react to must reach the graft as a single whole-document \
             reuse claim, got {last:?}",
    );
    // Four: the three boxes S15's gate records as still built on this flip,
    // plus the one claim that stands for the other 314.
    assert!(
        last.visited <= 5,
        "{} boxes were compared against their own copies on a hover flip whose box tree came \
             wholesale out of the previous cycle — before S18 this was the whole 318-box \
             document, and it must now be only the handful the box-build stage really rebuilt. \
             Census: {last:?}",
        last.visited,
    );

    // The keystroke shape: the omnibox chain is rebuilt, everything else is
    // claimed. The graft must visit the chain only.
    let mut key_state = Cc12IncrementalState::default();
    let mut typed = String::new();
    let mut key_last = lumen_layout::incremental::GraftStats::default();
    for _ in 0..4 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        let _ =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, None, &mut key_state);
        key_last = lumen_layout::incremental::take_graft_stats();
    }
    assert!(
        key_last.reused_wholesale >= 5,
        "the boxes a keystroke never came near must reach the graft as reuse claims, got \
             {key_last:?}",
    );
    assert!(
        key_last.visited < 100,
        "{} boxes were compared on a keystroke cycle that rebuilds ~28 of 318 — the graft is \
             still walking subtrees the box-build stage copied verbatim. Census: {key_last:?}. If \
             chrome.html gains structure that genuinely rebuilds a large region on every \
             keystroke, record the count that structure accounts for; do not turn this into a \
             percentage of whatever the code currently does.",
        key_last.visited,
    );
}

/// BUG-341 S18 diagnostic: census of *what* the 28 boxes a keystroke
/// rebuilds actually are.
///
/// S17 drove the cascade down to a single recomputed element, yet the box
/// tree still rebuilds ~28 boxes — so the residual is no longer about which
/// nodes are invalidated but about the **unit of reuse**. `clean_subtrees`
/// licenses cloning a whole element subtree, and a subtree containing one
/// content-dirty node is not clonable, so every ancestor of `#omniInput`
/// rebuilds — and each rebuild re-walks all of its own children.
///
/// This prints the built list itself (per-node, classified) plus the
/// per-ancestor breakdown: how many child slots each rebuilt ancestor has,
/// how many of them came across as whole-subtree clones, and how many were
/// rebuilt for want of a finer-grained unit. Run: `cargo test -p lumen-shell
/// --profile dev-release bug341_s18_keystroke_box_build_census --
/// --ignored --nocapture`.
#[test]
#[ignore = "manual diagnostic (BUG-341 S18) — see doc comment for run command"]
fn bug341_s18_keystroke_box_build_census() {
    use std::collections::HashSet;

    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    let mut state = Cc12IncrementalState::default();
    let mut typed = String::new();
    for i in 0..4 {
        typed.push('a');
        let model = cc12_bench_model(&typed);
        let touched = lumen_chrome::bind_model_tracked(&mut doc, &model);
        lumen_layout::set_interactive_state(None, None, None);
        let last = i == 3;

        lumen_layout::box_tree::set_box_build_diagnostics(last);
        let _ = lumen_layout::counters::take_cascade_stats();
        let _ = lumen_layout::style::take_compute_style_calls();
        let (layout, counters) = match state.prev_pristine_layout.take() {
            Some(prev) => {
                let node_index = lumen_layout::style::restyle_node_index(&doc, &sheet);
                let dirty_roots = lumen_layout::style::restyle_root_set_for_node_change(
                    &doc,
                    chrome_node_changes(&touched),
                    &node_index,
                );
                let delta = lumen_layout::counters::RestyleDelta {
                    prev_styles: std::mem::take(&mut state.prev_cascade_styles),
                    dirty_roots,
                    content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                    shallow_roots: Default::default(), point_roots: Default::default(),
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
        let built = lumen_layout::box_tree::take_box_build_log();
        lumen_layout::box_tree::set_box_build_diagnostics(false);
        let bb = lumen_layout::box_tree::take_box_build_stats();
        let cs = lumen_layout::counters::take_cascade_stats();
        let full_cascades = lumen_layout::style::take_compute_style_calls();
        let copy = lumen_layout::box_tree::take_box_copy_stats();
        eprintln!(
            "[s18-census] cycle={i} cascade_recomputed={} cascade_reused={} \
                 boxes_built={} boxes_reused={} compute_style_calls={full_cascades} \
                 subtree_reuse={:.3}ms over {} boxes; prev_index={:.3}ms over {} boxes",
            cs.recomputed,
            cs.reused,
            bb.built,
            bb.reused,
            copy.reuse_ns as f64 / 1e6,
            copy.reuse_boxes,
            copy.index_ns as f64 / 1e6,
            copy.index_boxes,
        );

        if last {
            // The chain that cannot be cloned: every ancestor of a
            // content-dirty node, plus the dirty node itself.
            let mut chain: HashSet<lumen_dom::NodeId> = HashSet::new();
            for &n in &touched.content {
                let mut cur = Some(n);
                while let Some(c) = cur {
                    chain.insert(c);
                    cur = doc.get(c).parent;
                }
            }
            let clean = counters.clean_subtrees();
            let built_set: HashSet<lumen_dom::NodeId> = built.iter().copied().collect();

            eprintln!("[s18-census] content-dirty nodes: {}", touched.content.len());
            for &n in &touched.content {
                eprintln!("[s18-census]   {}", census_describe(&doc, n));
            }
            eprintln!(
                "[s18-census] built={} (distinct nodes {}) reused={} chain_len={}",
                bb.built,
                built_set.len(),
                bb.reused,
                chain.len(),
            );

            // Partition the built list: which of them are on the
            // un-clonable chain, which are non-elements (never eligible —
            // `clean_subtrees` records elements only), which are elements
            // the cascade re-ran, and which are left unexplained.
            let (mut on_chain, mut non_elem, mut recascaded, mut other) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for &n in &built {
                let is_elem = matches!(doc.get(n).data, lumen_dom::NodeData::Element { .. });
                if chain.contains(&n) {
                    on_chain.push(n);
                } else if !is_elem {
                    non_elem.push(n);
                } else if !clean.contains(&n) {
                    recascaded.push(n);
                } else {
                    other.push(n);
                }
            }
            for (label, list) in [
                ("on-chain", &on_chain),
                ("non-element", &non_elem),
                ("elem-not-clean", &recascaded),
                ("clean-but-built", &other),
            ] {
                eprintln!(
                    "[s18-census] {label}: {} — {}",
                    list.len(),
                    list.iter()
                        .map(|&n| census_describe(&doc, n))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }

            // Per-ancestor breakdown: the child slots each rebuilt ancestor
            // re-walked, split by what happened to each child.
            let mut chain_ordered: Vec<lumen_dom::NodeId> = chain.iter().copied().collect();
            chain_ordered.sort_by_key(|&n| {
                let mut d = 0usize;
                let mut cur = doc.get(n).parent;
                while let Some(c) = cur {
                    d += 1;
                    cur = doc.get(c).parent;
                }
                d
            });
            let (mut slots, mut slots_reused, mut slots_built) = (0usize, 0usize, 0usize);
            for &anc in &chain_ordered {
                let kids = doc.get(anc).children.clone();
                let reused_kids = kids
                    .iter()
                    .filter(|&&k| clean.contains(&k) && !built_set.contains(&k))
                    .count();
                let built_kids = kids.iter().filter(|&&k| built_set.contains(&k)).count();
                slots += kids.len();
                slots_reused += reused_kids;
                slots_built += built_kids;
                eprintln!(
                    "[s18-census] ancestor {} children={} reused={} built={} \
                         (elem children {}, text/comment {})",
                    census_describe(&doc, anc),
                    kids.len(),
                    reused_kids,
                    built_kids,
                    kids.iter()
                        .filter(|&&k| matches!(
                            doc.get(k).data,
                            lumen_dom::NodeData::Element { .. }
                        ))
                        .count(),
                    kids.iter()
                        .filter(|&&k| !matches!(
                            doc.get(k).data,
                            lumen_dom::NodeData::Element { .. }
                        ))
                        .count(),
                );
            }
            eprintln!(
                "[s18-census] chain child slots total={slots} reused={slots_reused} \
                     built={slots_built} neither={}",
                slots - slots_reused - slots_built,
            );
        }

        state.prev_pristine_layout = Some(layout.clone());
        state.prev_cascade_styles = counters.into_styles();
        lumen_layout::clear_interactive_state();
    }
}

/// BUG-341 S19 diagnostic: census of the whole-tree **copies** an
/// incremental cycle makes, as opposed to the boxes it builds.
///
/// S18 drove the graft down to O(1) per reused subtree and left the copies
/// as the largest remaining items. This prints, per cycle and per scenario,
/// the three the queue names: the reuse copy taken out of `prev` inside
/// `build_box_or_reuse`, the index walk over `prev` that feeds it, and the
/// pipeline's own `layout.clone()` that persists the next cycle's `prev`
/// — each with the number of boxes it touched, so a later run can tell
/// "the copy got cheaper" from "the region got smaller".
///
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s19_copy_census -- --ignored --nocapture`.
#[test]
#[ignore = "manual diagnostic (BUG-341 S19) — see doc comment for run command"]
fn bug341_s19_copy_census() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    for scenario in ["KEY", "HOVER"] {
        let mut state = Cc12IncrementalState::default();
        let mut typed = String::new();
        for i in 0..6 {
            let (model, hover) = if scenario == "KEY" {
                typed.push('a');
                (cc12_bench_model(&typed), None)
            } else {
                (cc12_bench_model(""), if i % 2 == 0 { sidebar } else { None })
            };
            // Diagnostics on for the last two cycles only: the census
            // traversals they add are themselves measurable, and the first
            // cycles are the cold full-layout ones anyway.
            lumen_layout::box_tree::set_box_build_diagnostics(i >= 4);
            let prev_boxes =
                state.prev_pristine_layout.as_ref().map_or(0, census_count_boxes);
            let touched = lumen_chrome::bind_model_tracked(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);
            let (layout, counters) = match state.prev_pristine_layout.take() {
                Some(prev) => {
                    let (prev_hover, prev_focus, prev_active) = state.prev_interactive;
                    let state_index = lumen_layout::style::restyle_state_index(&doc, &sheet);
                    let mut dirty_roots = std::collections::HashSet::new();
                    for (was, now) in [
                        (prev_hover, hover),
                        (prev_focus, None),
                        (prev_active, None),
                    ] {
                        dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                            &doc, was, now, &state_index,
                        ));
                    }
                    let node_index = lumen_layout::style::restyle_node_index(&doc, &sheet);
                    dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                        &doc,
                        chrome_node_changes(&touched),
                        &node_index,
                    ));
                    let delta = lumen_layout::counters::RestyleDelta {
                        prev_styles: std::mem::take(&mut state.prev_cascade_styles),
                        dirty_roots,
                        content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                        shallow_roots: Default::default(), point_roots: Default::default(),
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
            let copy = lumen_layout::box_tree::take_box_copy_stats();
            let t = std::time::Instant::now();
            let persisted = layout.clone();
            let clone_tree_ns = t.elapsed().as_nanos() as u64;
            let clone_tree_boxes = census_count_boxes(&persisted);
            let cs = lumen_layout::counters::take_cascade_stats();
            if i >= 4 {
                eprintln!(
                    "[s19-census] {scenario} cycle={i} prev_boxes={prev_boxes} \
                         cascade_recomputed={} boxes_built={} boxes_reused={} | \
                         subtree_reuse={:.3}ms/{} boxes | prev_index={:.3}ms/{} boxes | \
                         clone_tree={:.3}ms/{clone_tree_boxes} boxes",
                    cs.recomputed,
                    bb.built,
                    bb.reused,
                    copy.reuse_ns as f64 / 1e6,
                    copy.reuse_boxes,
                    copy.index_ns as f64 / 1e6,
                    copy.index_boxes,
                    clone_tree_ns as f64 / 1e6,
                );
            }
            lumen_layout::box_tree::set_box_build_diagnostics(false);
            state.prev_pristine_layout = Some(persisted);
            state.prev_cascade_styles = counters.into_styles();
            state.prev_interactive = (hover, None, None);
            lumen_layout::clear_interactive_state();
        }
    }
}

/// BUG-341 S20 diagnostic: where an incremental cycle's time actually goes,
/// stage by stage, and inside the two stages that dominate.
///
/// The queue named two items for this slice (the pipeline's `layout.clone()`
/// and `precompute_counters` rebuilding its `CounterMap` from scratch). This
/// census exists to check that claim before a line is changed — the sixth
/// slice in a row to do so, and the fifth where the planned premise was not
/// where the time was. It prints, per scenario and per cycle:
///
/// - the whole pass's wall-clock, and after it the tree copy **this harness**
///   makes to keep a `prev` for the next cycle. That column stopped
///   describing production at S22, which replaced the pipeline's per-frame
///   `layout.clone()` with a reversible prune, so read it as harness
///   overhead and not as part of the cycle;
/// - the `CascadeIndex` rebuild the pass forces. When this census was
///   written every pass forced one, because the cache was keyed by the
///   sheet's address and had to be dropped at the top of each pass; S21
///   keyed it by `Stylesheet::revision` and the column now reads zero on a
///   warm thread. See `bug341_s21_cascade_index_census` for the split;
/// - the `CounterMap` the cascade stage produced, by size, plus a replay of
///   rebuilding those three collections so the map's own construction cost
///   can be told from the traversal that fills it. Replayed rather than
///   timed in place: per-node timers around ~2500 hash operations would cost
///   a sizeable fraction of the stage they are measuring. **Since S24 the
///   `styles` replay is a measure of removed cost, not incurred cost** — the
///   pass carries that map rather than filling it, which the `carried=`
///   column reports (passes lived through, whether the pass had to sweep,
///   and how many entries it displaced);
/// - every box the build stage really built, with its **inclusive** cost,
///   and a self-time column derived by subtracting the descendants that are
///   themselves in the log.
///
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s20_stage_census -- --ignored --nocapture`. Add
/// `LUMEN_PROFILE_TREE=1` for the engine's own stage split alongside it.
#[test]
#[ignore = "manual diagnostic (BUG-341 S20) — see doc comment for run command"]
fn bug341_s20_stage_census() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    for scenario in ["KEY", "HOVER"] {
        let mut state = Cc12IncrementalState::default();
        let mut typed = String::new();
        for i in 0..6 {
            let (model, hover) = if scenario == "KEY" {
                typed.push('a');
                (cc12_bench_model(&typed), None)
            } else {
                (cc12_bench_model(""), if i % 2 == 0 { sidebar } else { None })
            };
            let report = i >= 4;
            // S20's own gate, not S18/S19's: the copy census walks every
            // reused subtree from inside the parent's `build_box`, which
            // would show up as that parent's build cost.
            lumen_layout::box_tree::set_box_time_diagnostics(report);
            lumen_layout::style::set_pseudo_cascade_diagnostics(report);
            let touched = lumen_chrome::bind_model_tracked(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);
            let _ = lumen_layout::style::take_cascade_index_stats();
            let _ = lumen_layout::style::take_pseudo_cascade_stats();
            let _ = lumen_layout::style::take_pseudo_cascade_sites();
            let t_pass = std::time::Instant::now();
            let (layout, counters) = match state.prev_pristine_layout.take() {
                Some(prev) => {
                    let (prev_hover, prev_focus, prev_active) = state.prev_interactive;
                    let state_index = lumen_layout::style::restyle_state_index(&doc, &sheet);
                    let mut dirty_roots = std::collections::HashSet::new();
                    for (was, now) in [
                        (prev_hover, hover),
                        (prev_focus, None),
                        (prev_active, None),
                    ] {
                        dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                            &doc, was, now, &state_index,
                        ));
                    }
                    let node_index = lumen_layout::style::restyle_node_index(&doc, &sheet);
                    dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                        &doc,
                        chrome_node_changes(&touched),
                        &node_index,
                    ));
                    let delta = lumen_layout::counters::RestyleDelta {
                        prev_styles: std::mem::take(&mut state.prev_cascade_styles),
                        dirty_roots,
                        content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                        shallow_roots: Default::default(), point_roots: Default::default(),
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
            let pass_ns = t_pass.elapsed().as_nanos() as u64;
            let idx_stats = lumen_layout::style::take_cascade_index_stats();
            let ps_stats = lumen_layout::style::take_pseudo_cascade_stats();
            let ps_sites = lumen_layout::style::take_pseudo_cascade_sites();
            let cs = lumen_layout::counters::take_cascade_stats();
            let bb = lumen_layout::box_tree::take_box_build_stats();
            let (probe_ns, miss_ns) = lumen_layout::box_tree::take_box_probe_ns();
            let times = lumen_layout::box_tree::take_box_build_time_log();
            let t = std::time::Instant::now();
            let persisted = layout.clone();
            let clone_tree_ns = t.elapsed().as_nanos() as u64;

            if report {
                eprintln!(
                    "[s20-census] {scenario} cycle={i} pass={:.3}ms clone_tree={:.3}ms | \
                         cascade_index rebuilds={} {:.3}ms | pseudo={} hits={} {:.3}ms | \
                         cascade recomputed={} reused={} visited={} clean_inserts={} | \
                         boxes built={} reused={} fanouts={} | \
                         display_probes={} cascaded={} {:.3}ms style_misses={} {:.3}ms",
                    pass_ns as f64 / 1e6,
                    clone_tree_ns as f64 / 1e6,
                    idx_stats.builds,
                    idx_stats.build_ns as f64 / 1e6,
                    ps_stats.calls,
                    ps_stats.hits,
                    ps_stats.ns as f64 / 1e6,
                    cs.recomputed,
                    cs.reused,
                    cs.visited,
                    cs.clean_inserts,
                    bb.built,
                    bb.reused,
                    bb.fanouts,
                    bb.display_probes,
                    bb.display_probe_cascades,
                    probe_ns as f64 / 1e6,
                    bb.style_misses,
                    miss_ns as f64 / 1e6,
                );
                let mut sites: Vec<_> = ps_sites.into_iter().collect();
                sites.sort_by_key(|(_, st)| std::cmp::Reverse(st.ns));
                for (name, st) in &sites {
                    eprintln!(
                        "[s20-census]   pseudo ::{name} calls={} hits={} {:.3}ms",
                        st.calls,
                        st.hits,
                        st.ns as f64 / 1e6,
                    );
                }
                census_report_counter_map(&counters);
                census_report_built_boxes(&doc, &times);
            }

            lumen_layout::box_tree::set_box_time_diagnostics(false);
            lumen_layout::style::set_pseudo_cascade_diagnostics(false);
            state.prev_pristine_layout = Some(persisted);
            state.prev_cascade_styles = counters.into_styles();
            state.prev_interactive = (hover, None, None);
            lumen_layout::clear_interactive_state();
        }
    }
}

/// BUG-341 S20 census helper: the `CounterMap` a cycle produced, by size,
/// with a replay of rebuilding its collections.
///
/// The replay reproduces exactly the inserts `counters::walk` performs: an
/// `Arc` clone plus insert per element into `styles`, a counter-stack
/// snapshot plus insert per element into `nodes`, and one insert per clean
/// node into `clean_subtrees` — over the real sizes, so "the map costs X of
/// the stage's Y" is an honest attribution rather than a guess. `nodes` is
/// not exposed, but it holds one entry per element, which is `styles`' size.
fn census_report_counter_map(counters: &lumen_layout::CounterMap) {
    let styles = counters.styles();
    let clean = counters.clean_subtrees();

    // BUG-341 S23: the replays reserve capacity because production now does
    // (`CounterMap::with_capacity`). A replay that still grew from zero
    // would keep reporting the rehashing this slice removed.
    let t = std::time::Instant::now();
    let mut styles_replay: HashMap<lumen_dom::NodeId, std::sync::Arc<lumen_layout::style::ComputedStyle>> =
        HashMap::with_capacity(styles.len());
    for (&id, style) in styles {
        styles_replay.insert(id, std::sync::Arc::clone(style));
    }
    let styles_ns = t.elapsed().as_nanos() as u64;

    // BUG-341 S23: only nodes with a counter actually in scope store a
    // snapshot, so this replays the map's real size — zero on `chrome.html`,
    // which declares no counters. Before S23 it was one empty-map clone per
    // element, and reading the count off `styles` hid exactly that.
    let snapshots = counters.counter_snapshot_count();
    let t = std::time::Instant::now();
    let mut nodes_replay: HashMap<lumen_dom::NodeId, lumen_layout::counters::CounterSnapshot> =
        HashMap::new();
    let stacks: lumen_layout::counters::CounterSnapshot = HashMap::new();
    for &id in styles.keys().take(snapshots) {
        nodes_replay.insert(id, stacks.clone());
    }
    let nodes_ns = t.elapsed().as_nanos() as u64;

    let t = std::time::Instant::now();
    let mut clean_replay: std::collections::HashSet<lumen_dom::NodeId> =
        std::collections::HashSet::with_capacity(styles.len());
    for &id in clean {
        clean_replay.insert(id);
    }
    let clean_ns = t.elapsed().as_nanos() as u64;

    // BUG-341 S26: the reuse path itself. One hash lookup plus an `Arc`
    // clone per element is what a pass that recomputes *nothing* still
    // pays, and no earlier census separated it from `compute_style`. Uses
    // the same map, so the probe sequence and load factor are production's.
    // BUG-341 S26: the reuse path, split into the two things it does per
    // element — the hash lookup that finds the entry, and the `Arc::clone`
    // that takes it. The first attempt at this replay measured them together
    // and read as "the hash barely matters": `Arc::clone` touches the
    // refcount word of a 3.2 KB `ComputedStyle`, one cold cache line per
    // element and 2.6 MB of working set per pass, which swamped the hash on
    // both sides of the comparison being made. Split, it is the clone that
    // costs 2-4× the lookup — and the walk never reads the style it is
    // counting a reference to.
    let ids: Vec<lumen_dom::NodeId> = styles.keys().copied().collect();
    let t = std::time::Instant::now();
    let mut sink = 0usize;
    for id in &ids {
        if let Some(s) = styles.get(id) {
            sink ^= std::sync::Arc::as_ptr(s) as usize;
        }
    }
    let lookup_ns = t.elapsed().as_nanos() as u64;
    assert!(sink != 0 || ids.is_empty(), "lookup replay must not be optimised away");

    // The refcount half on its own: clone every entry, then drop the lot.
    let t = std::time::Instant::now();
    let clones: Vec<std::sync::Arc<lumen_layout::style::ComputedStyle>> =
        ids.iter().filter_map(|id| styles.get(id).map(std::sync::Arc::clone)).collect();
    let arc_ns = t.elapsed().as_nanos() as u64;
    assert_eq!(clones.len(), ids.len(), "arc replay must clone every entry");
    drop(clones);

    eprintln!(
        "[s20-census]   CounterMap: carried passes={} swept={} displaced={} | \
             styles={} ({:.3}ms replay AVOIDED) snapshots={} ({:.3}ms replay) \
             clean_subtrees={} ({:.3}ms replay) reuse_lookups={} \
             (lookup {:.3}ms / arc_clone {:.3}ms) — total replay {:.3}ms",
        styles.passes_lived(),
        styles.swept_last_pass(),
        counters.replaced_styles().len(),
        styles_replay.len(),
        styles_ns as f64 / 1e6,
        snapshots,
        nodes_ns as f64 / 1e6,
        clean_replay.len(),
        clean_ns as f64 / 1e6,
        ids.len(),
        lookup_ns as f64 / 1e6,
        arc_ns as f64 / 1e6,
        (styles_ns + nodes_ns + clean_ns) as f64 / 1e6,
    );
}

/// BUG-341 S20 census helper: every box the build stage really built, most
/// expensive first, with inclusive and self time.
///
/// Self time subtracts the *direct* descendants that are themselves in the
/// log — a built box's children are either built (logged, subtracted here)
/// or moved in wholesale (O(1), nothing to subtract). Inclusive time on a
/// container that rayon fanned out also covers the join wait, so a container
/// whose self time is large is worth a second look before it is believed.
fn census_report_built_boxes(doc: &lumen_dom::Document, times: &[(lumen_dom::NodeId, u64)]) {
    let incl: HashMap<lumen_dom::NodeId, u64> = times.iter().copied().collect();
    let mut rows: Vec<(lumen_dom::NodeId, u64, i64)> = times
        .iter()
        .map(|&(id, ns)| {
            let kids: i64 = doc
                .get(id)
                .children
                .iter()
                .filter_map(|c| incl.get(c))
                .map(|&n| n as i64)
                .sum();
            (id, ns, ns as i64 - kids)
        })
        .collect();
    rows.sort_by_key(|&(_, ns, _)| std::cmp::Reverse(ns));
    let total: u64 = times.iter().map(|&(_, ns)| ns).sum();
    let self_total: i64 = rows.iter().map(|&(_, _, s)| s).sum();
    eprintln!(
        "[s20-census]   built {} boxes, Σinclusive={:.3}ms Σself={:.3}ms",
        times.len(),
        total as f64 / 1e6,
        self_total as f64 / 1e6,
    );
    for &(id, ns, self_ns) in rows.iter().take(10) {
        eprintln!(
            "[s20-census]     {:>8.3}ms incl {:>8.3}ms self  {}",
            ns as f64 / 1e6,
            self_ns as f64 / 1e6,
            census_describe(doc, id),
        );
    }
}

/// BUG-341 S21 diagnostic: how often the `CascadeIndex` is really rebuilt
/// per incremental pass, by whom, and how the rebuild's time splits.
///
/// The queue named `CascadeIndex::build` the largest remaining item of the
/// S20 census (0.12-0.21ms every pass, on both scenarios) — but that number
/// came from `take_cascade_index_stats`, which was **thread-local** while
/// the code it counts is not: `build_box` fans flex/grid containers out over
/// rayon workers, and every worker's `StyleEnvSnapshot::install` drops the
/// per-thread index cache before doing style work. The counter is
/// process-wide as of this slice, so this census is the first honest count.
/// It prints, per scenario and per cycle:
///
/// - the whole pass's wall-clock, so a rebuild can be read as a share of it;
/// - rebuild count and total nanoseconds, split into the four phases of
///   `CascadeIndex::build` (top-level `RuleIndex`, the per-block indexes,
///   the `@media`/`@supports` activity evaluation, the two sheet-wide
///   predicate scans) — so "re-index the sheet" can be told apart from
///   "re-evaluate the media queries";
/// - the same figures for a pass run with the box-build fan-out suppressed
///   (`prev_index`-driven, so simply an incremental pass) versus a full
///   pass, which is the only way to attribute rebuilds to workers;
/// - the sheet's shape (rule and block counts), because the rebuild is
///   linear in it and `chrome.html` is not a large sheet.
///
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s21_cascade_index_census -- --ignored --nocapture`.
#[test]
#[ignore = "manual diagnostic (BUG-341 S21) — see doc comment for run command"]
fn bug341_s21_cascade_index_census() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    eprintln!(
        "[s21-census] sheet: rules={} media_blocks={} (Σ{} rules) layers={} supports={} scope={}",
        sheet.rules.len(),
        sheet.media_rules.len(),
        sheet.media_rules.iter().map(|m| m.rules.len()).sum::<usize>(),
        sheet.layers.len(),
        sheet.supports_rules.len(),
        sheet.scope_rules.len(),
    );

    for scenario in ["KEY", "HOVER"] {
        let mut state = Cc12IncrementalState::default();
        let mut typed = String::new();
        for i in 0..6 {
            let (model, hover) = if scenario == "KEY" {
                typed.push('a');
                (cc12_bench_model(&typed), None)
            } else {
                (cc12_bench_model(""), if i % 2 == 0 { sidebar } else { None })
            };
            let report = i >= 4;
            let touched = lumen_chrome::bind_model_tracked(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);
            let _ = lumen_layout::style::take_cascade_index_stats();
            let t_pass = std::time::Instant::now();
            let (layout, counters) = match state.prev_pristine_layout.take() {
                Some(prev) => {
                    let (prev_hover, prev_focus, prev_active) = state.prev_interactive;
                    let state_index = lumen_layout::style::restyle_state_index(&doc, &sheet);
                    let mut dirty_roots = std::collections::HashSet::new();
                    for (was, now) in [
                        (prev_hover, hover),
                        (prev_focus, None),
                        (prev_active, None),
                    ] {
                        dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                            &doc, was, now, &state_index,
                        ));
                    }
                    let node_index = lumen_layout::style::restyle_node_index(&doc, &sheet);
                    dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                        &doc,
                        chrome_node_changes(&touched),
                        &node_index,
                    ));
                    let delta = lumen_layout::counters::RestyleDelta {
                        prev_styles: std::mem::take(&mut state.prev_cascade_styles),
                        dirty_roots,
                        content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
                        shallow_roots: Default::default(), point_roots: Default::default(),
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
            let pass_ns = t_pass.elapsed().as_nanos() as u64;
            let idx = lumen_layout::style::take_cascade_index_stats();
            let bb = lumen_layout::box_tree::take_box_build_stats();

            if report {
                eprintln!(
                    "[s21-census] {scenario} cycle={i} pass={:.3}ms | index rebuilds={} \
                         {:.3}ms ({:.1}% of pass) = rules {:.3} + blocks {:.3} + active {:.3} \
                         + predicates {:.3} | fanouts={} built={} reused={}",
                    pass_ns as f64 / 1e6,
                    idx.builds,
                    idx.build_ns as f64 / 1e6,
                    100.0 * idx.build_ns as f64 / pass_ns.max(1) as f64,
                    idx.rules_ns as f64 / 1e6,
                    idx.blocks_ns as f64 / 1e6,
                    idx.active_ns as f64 / 1e6,
                    idx.predicates_ns as f64 / 1e6,
                    bb.fanouts,
                    bb.built,
                    bb.reused,
                );
            }

            state.prev_pristine_layout = Some(layout);
            state.prev_cascade_styles = counters.into_styles();
            state.prev_interactive = (hover, None, None);
            lumen_layout::clear_interactive_state();
        }
    }

    // A full pass for contrast: it fans out over rayon (M4.1), so its
    // rebuild count is the worker count plus one, and it is the number the
    // thread-local counter could never see.
    let _ = lumen_layout::style::take_cascade_index_stats();
    let t = std::time::Instant::now();
    let _ = lumen_layout::layout_measured_hyp_with_counters(
        &doc, &sheet, viewport, &measurer, &hyp, false,
    );
    let full_ns = t.elapsed().as_nanos() as u64;
    let idx = lumen_layout::style::take_cascade_index_stats();
    eprintln!(
        "[s21-census] FULL pass={:.3}ms | index rebuilds={} {:.3}ms (Σ over all threads)",
        full_ns as f64 / 1e6,
        idx.builds,
        idx.build_ns as f64 / 1e6,
    );
}

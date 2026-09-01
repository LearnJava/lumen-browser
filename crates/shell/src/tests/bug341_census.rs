//! Ручные переписи и замеры BUG-341 — все под `#[ignore]`.
//!
//! Это диагностика, а не гейт: каждая печатает распределение и запускается
//! поимённо, команда — в док-комментарии самого теста.

use super::*;

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
fn census_describe(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> String {
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

/// BUG-341 S27 census: the nodes a walk would have to enter if it only
/// followed the *spine* — every ancestor of a dirty root or a
/// content-mutated node, plus those nodes' own subtrees.
///
/// This is the exact size of the traversal the slice proposes, computed
/// from the same two inputs the delta carries, so the census can say what
/// fraction of the real traversal is removable before a line of it is
/// written (the S17/S19 rule: measure the note, do not trust it).
fn census_spine_size(
    doc: &lumen_dom::Document,
    dirty_roots: &std::collections::HashSet<lumen_dom::NodeId>,
    content: &std::collections::HashSet<lumen_dom::NodeId>,
) -> (usize, usize) {
    let mut spine: std::collections::HashSet<lumen_dom::NodeId> = std::collections::HashSet::new();
    for &seed in dirty_roots.iter().chain(content.iter()) {
        let mut cur = Some(seed);
        while let Some(id) = cur {
            if !spine.insert(id) {
                break;
            }
            cur = doc.get(id).parent;
        }
    }
    // A dirty root re-cascades its whole subtree, so those nodes are
    // entered too — they are the part of the walk the slice cannot remove.
    let mut forced = 0usize;
    for &root in dirty_roots {
        forced += census_subtree_nodes(doc, root);
    }
    (spine.len(), forced)
}

/// Number of nodes (of any kind) in `id`'s subtree, inclusive.
fn census_subtree_nodes(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> usize {
    1 + doc.get(id).children.iter().map(|&c| census_subtree_nodes(doc, c)).sum::<usize>()
}

/// BUG-341 S27 census replay: the recursion alone, with no per-node work.
/// The floor under any shape that still visits the document.
fn census_bare_traversal(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> usize {
    let mut n = 1;
    for &c in &doc.get(id).children {
        n += census_bare_traversal(doc, c);
    }
    n
}

/// BUG-341 S27 census replay: the recursion plus one map restamp per
/// element — the cheapest traversal that can still keep the S24 pass
/// ordinal exact, and therefore the candidate that changes no invariant.
fn census_restamp_traversal(
    doc: &lumen_dom::Document,
    id: lumen_dom::NodeId,
    map: &mut HashMap<lumen_dom::NodeId, (std::sync::Arc<lumen_layout::style::ComputedStyle>, u64)>,
    pass: u64,
) -> usize {
    let mut n = 0;
    if let Some(e) = map.get_mut(&id) {
        e.1 = pass;
        n += 1;
    }
    for &c in &doc.get(id).children {
        n += census_restamp_traversal(doc, c, map, pass);
    }
    n
}

/// BUG-341 S27 census: how much of the cascade stage is the traversal
/// itself, and how much of that traversal the spine would keep.
///
/// S26 proved the traversal is the stage (50-70% of the pass) and removed
/// it for the cycle whose delta names nobody. This asks the general
/// question — on a cycle that *does* name somebody, how many of the nodes
/// it enters could no dirty root and no content mutation possibly reach.
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s27_walk_census -- --ignored --nocapture`.
#[test]
#[ignore = "manual census (BUG-341 S27) — see doc comment for run command"]
fn bug341_s27_walk_census() {
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
            let touched = lumen_chrome::bind_model_tracked(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);
            let structural = touched.selector.values().filter(|t| t.structural).count();
            let t_pass = std::time::Instant::now();
            let (layout, counters) = match state.prev_pristine_layout.take() {
                Some(prev) => {
                    let (prev_hover, prev_focus, prev_active) = state.prev_interactive;
                    let state_index = lumen_layout::style::restyle_state_index(&doc, &sheet);
                    let mut dirty_roots = std::collections::HashSet::new();
                    for (was, now) in [(prev_hover, hover), (prev_focus, None), (prev_active, None)] {
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
                    let (spine, forced) = census_spine_size(&doc, &dirty_roots, &touched.content);
                    eprintln!(
                        "[s27-census] {scenario} cycle={i} dirty_roots={} content={} \
                             structural={structural} spine={spine} forced_subtrees={forced}",
                        dirty_roots.len(),
                        touched.content.len(),
                    );
                    let delta = lumen_layout::counters::RestyleDelta {
                        prev_styles: std::mem::take(&mut state.prev_cascade_styles),
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
            let pass_ns = t_pass.elapsed().as_nanos() as u64;
            let cs = lumen_layout::counters::take_cascade_stats();
            // The two candidate shapes for the traversal, replayed over the
            // real document and a map of the real size. Split by operation
            // (the S26 lesson): a replay that times the recursion together
            // with the restamp cannot tell "walking the document is the
            // cost" from "touching the map is".
            let mut replay: HashMap<lumen_dom::NodeId, (std::sync::Arc<lumen_layout::style::ComputedStyle>, u64)> =
                HashMap::with_capacity(counters.styles().len());
            for (nid, style) in counters.styles().iter() {
                replay.insert(*nid, (std::sync::Arc::clone(style), 0));
            }
            let t = std::time::Instant::now();
            let bare = census_bare_traversal(&doc, doc.root());
            let bare_ns = t.elapsed().as_nanos() as u64;
            let t = std::time::Instant::now();
            let stamped = census_restamp_traversal(&doc, doc.root(), &mut replay, 1);
            let restamp_ns = t.elapsed().as_nanos() as u64;
            eprintln!(
                "[s27-census] {scenario} cycle={i} replay bare={:.3}ms ({bare} nodes) \
                     restamp={:.3}ms ({stamped} entries)",
                bare_ns as f64 / 1e6,
                restamp_ns as f64 / 1e6,
            );
            eprintln!(
                "[s27-census] {scenario} cycle={i} pass={:.3}ms walk={:.3}ms ({:.0}%) \
                     visited={} recomputed={} reused={} clean_inserts={} skipped={} entries={} \
                     confirmed={} confirm_misses={}",
                pass_ns as f64 / 1e6,
                cs.walk_ns as f64 / 1e6,
                100.0 * cs.walk_ns as f64 / pass_ns as f64,
                cs.visited,
                cs.recomputed,
                cs.reused,
                cs.clean_inserts,
                cs.skipped_subtrees,
                counters.styles().len(),
                // BUG-341 S28: the S27 restamp count, not printed by this census
                // before now — the S26 dense-keying note priced ~4200 NodeId lookups
                // per pass; `visited` + `confirmed` is what that traversal costs today.
                cs.confirmed,
                cs.confirm_misses,
            );
            state.prev_pristine_layout = Some(layout.clone());
            state.prev_cascade_styles = counters.into_styles();
            state.prev_interactive = (hover, None, None);
            lumen_layout::clear_interactive_state();
        }
    }
}

/// BUG-341 S30 census: how much of `lay_out_flex`'s residual double-layout
/// (the "Fix scope note"/layout-result-cache idea, still unimplemented after
/// S1-S29 closed the *cascade* gap) is real redundant work a `(node,
/// constraints)`-keyed memoization cache could actually remove — measured
/// *before* building that cache, per `docs/perf-method.md` §1.
///
/// Runs a full (non-incremental) `layout_measured_hyp` pass per cycle —
/// the S8 lesson: profile the path you'd actually change. `lay_out_flex`'s
/// Step-1 probe and final placement pass are both inside this path, and the
/// incremental cascade S3-S29 built does not touch `lay_out` itself once a
/// subtree is dirty.
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s30_flex_key_census -- --ignored --nocapture`.
#[test]
#[ignore = "manual census (BUG-341 S30) — see doc comment for run command"]
fn bug341_s30_flex_key_census() {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);

    for scenario in ["KEY", "HOVER"] {
        let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
        let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);
        let mut typed = String::new();
        for i in 0..5 {
            let (model, hover) = if scenario == "KEY" {
                typed.push('a');
                (cc12_bench_model(&typed), None)
            } else {
                (cc12_bench_model(""), if i % 2 == 0 { sidebar } else { None })
            };
            lumen_chrome::bind_model(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);
            lumen_layout::box_tree::set_layout_key_census(true);
            let (_layout, _counters) = lumen_layout::layout_measured_hyp_with_counters(
                &doc, &sheet, viewport, &measurer, &hyp, false,
            );
            let census = lumen_layout::box_tree::take_layout_key_census();
            eprintln!(
                "[s30-census] {scenario} cycle={i} calls={} repeat_key_calls={} ({:.1}%) repeat_key_same_style={} ({:.1}% of repeats) repeat_key_same_style_and_override={} ({:.1}% of repeats, {:.1}% of same_style)",
                census.calls,
                census.repeat_key_calls,
                100.0 * census.repeat_key_calls as f64 / census.calls.max(1) as f64,
                census.repeat_key_same_style,
                100.0 * census.repeat_key_same_style as f64 / census.repeat_key_calls.max(1) as f64,
                census.repeat_key_same_style_and_override,
                100.0 * census.repeat_key_same_style_and_override as f64 / census.repeat_key_calls.max(1) as f64,
                100.0 * census.repeat_key_same_style_and_override as f64 / census.repeat_key_same_style.max(1) as f64,
            );
            lumen_layout::clear_interactive_state();
        }
    }
}

// BUG-341 S32 built a general `(node, constraints)`-keyed layout-result
// cache and measured its real wall-clock effect here
// (`bug341_s32_layout_result_cache_share`, since removed along with the
// mechanism it measured — see `lumen_layout::box_tree`'s `CV_AUTO_TOUCHED`
// doc comment for the full history and numbers). S33 replaced the general
// cache with a targeted, zero-overhead probe-reuse fix scoped to
// `lay_out_grid` and confirmed via `grep` that `crates/chrome/` contains
// no `display: grid` anywhere — so neither the removed general cache nor
// the new targeted fix was ever reachable from *this* fixture, and this
// A/B harness had nothing left to measure a difference on at the time.
// S34 then removed `SavedItemSizing` (the style-mutation dance this
// comment pointed at as "the real next lever") in favor of
// `UsedSizeOverride`, restoring flex-item style-`Arc` stability across a
// Step-1 probe and final placement pass for 77.5% of repeat-key calls
// (S35's honest, override-aware re-measurement of S34's number) — the
// precondition S32's cache needed but did not have. S36 resurrects the
// general cache with `UsedSizeOverride` folded into its key (see
// `LayoutResultKey`'s own doc comment) and re-measures below.

/// BUG-341 S36: real wall-clock effect of the resurrected layout-result
/// cache (`lumen_layout::box_tree::set_layout_result_cache`) on the same
/// real chrome document/fixture S30-S35 used to measure the cache's
/// premise, reported honestly per `docs/perf-method.md` §1 ("key match is
/// not the same as wall-clock win — measure the real thing before
/// trusting the count"). S35 established a 77.5%-of-repeats ceiling for
/// this fixture; this measures whether that ceiling is now enough to make
/// the general cache net-positive, or whether clone cost still eats the
/// win the way it did at S32's 8.3% ceiling.
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s36_layout_result_cache_share -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf measurement (BUG-341 S36) — see doc comment for run command"]
fn bug341_s36_layout_result_cache_share() {
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    const WARMUP: usize = 10;
    const SAMPLES: usize = 60;

    for scenario in ["KEY", "HOVER"] {
        let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
        let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);
        let mut typed = String::new();

        let mut off_stats = lumen_paint::FrameStats::new();
        let mut on_stats = lumen_paint::FrameStats::new();
        let mut total_hits = 0u64;
        let mut total_misses = 0u64;
        let mut total_poisoned = 0u64;

        for i in 0..WARMUP + SAMPLES {
            let (model, hover) = if scenario == "KEY" {
                typed.push('a');
                (cc12_bench_model(&typed), None)
            } else {
                (cc12_bench_model(""), if i % 2 == 0 { sidebar } else { None })
            };
            lumen_chrome::bind_model(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);

            // Cache OFF, then cache ON, on the *same* document state this
            // cycle — an A/B pair per cycle (docs/perf-method.md's own
            // "interleaved A/B compared on min" rule), not two separate
            // sequential blocks that would also capture drift/contention
            // as if it were the cache's own effect.
            let t = std::time::Instant::now();
            let _ = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
            let off_ns = t.elapsed().as_nanos();

            lumen_layout::box_tree::set_layout_result_cache(true);
            let t = std::time::Instant::now();
            let _ = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
            let on_ns = t.elapsed().as_nanos();
            let stats = lumen_layout::box_tree::take_layout_result_cache_stats();
            lumen_layout::box_tree::set_layout_result_cache(false);

            if i >= WARMUP {
                off_stats.record(off_ns as f32 / 1e6);
                on_stats.record(on_ns as f32 / 1e6);
                total_hits += stats.hits as u64;
                total_misses += stats.misses as u64;
                total_poisoned += stats.poisoned as u64;
            }
            lumen_layout::clear_interactive_state();
        }
        let off_summary = off_stats.summary().expect("samples collected");
        let on_summary = on_stats.summary().expect("samples collected");
        eprintln!("{}", off_summary.display_with(&format!("BUG341_S36_{scenario}_CACHE_OFF")));
        eprintln!("{}", on_summary.display_with(&format!("BUG341_S36_{scenario}_CACHE_ON")));
        eprintln!(
            "[s36-cache] {scenario} hits={total_hits} misses={total_misses} poisoned={total_poisoned} \
                 hit_rate={:.1}%",
            100.0 * total_hits as f64 / (total_hits + total_misses).max(1) as f64,
        );
    }
}

/// Number of element nodes in `id`'s subtree, inclusive — the "how wide is
/// this dirty root" column of the S17 census.
fn census_subtree_elements(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> usize {
    let node = doc.get(id);
    let own = usize::from(matches!(node.data, lumen_dom::NodeData::Element { .. }));
    own + node.children.iter().map(|&c| census_subtree_elements(doc, c)).sum::<usize>()
}

/// BUG-341 S5: like `cc12_chrome_perf_gate_hover_and_keystroke_cycles`'s
/// `CC12_HOVER` scenario, but hover moves between two sibling tab rows
/// (`#sbTabs`' first two children) instead of toggling `SIDEBAR`/`None`.
/// S3 documented the toggle as a conservative-invalidation worst case
/// (`:hover` invalidates every ancestor of both the old and new target
/// when transitioning from "nothing hovered" — see BUG-341 "S3"); this is
/// the representative case real mouse movement over already-hovered
/// chrome looks like, and where the incremental cascade is expected to
/// pay off the most. Not a pass/fail gate (no separate budget exists for
/// this shape yet) — a recorded measurement, run alongside CC-12's own
/// number for comparison. Run: `cargo test -p lumen-shell --profile
/// dev-release bug341_s5_incremental_pipeline_share -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf measurement (BUG-341 S5) — see doc comment for run command"]
fn bug341_s5_incremental_pipeline_share() {
    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let model = cc12_bench_model("");
    lumen_chrome::bind_model(&mut doc, &model);
    let tabs_container = doc
        .find_by_id(lumen_chrome::ids::SB_TABS)
        .expect("chrome preview must have #sbTabs");
    let tab_rows = doc.get(tabs_container).children.clone();
    assert!(tab_rows.len() >= 2, "bind_model must have populated at least 2 tab rows");
    let tab_a = tab_rows[0];
    let tab_b = tab_rows[1];

    const WARMUP: usize = 10;
    const SAMPLES: usize = 60;

    let mut stats = lumen_paint::FrameStats::new();
    let mut state = Cc12IncrementalState::default();
    for i in 0..WARMUP + SAMPLES {
        let hover = if i % 2 == 0 { Some(tab_a) } else { Some(tab_b) };
        let (ms, _) = cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, hover, &mut state);
        if i >= WARMUP {
            stats.record(ms as f32);
        }
    }
    let summary = stats.summary().expect("samples collected");
    eprintln!("{}", summary.display_with("BUG341_S5_SIBLING_HOVER"));
}

/// BUG-341 S3: standalone measurement of the incremental cascade's
/// `precompute_counters` wall-time saving on the real chrome document, for
/// a *representative* hover interaction — the pointer moving between two
/// sibling tab rows (`sbTabs`' first two children after `bind_model`
/// populates 6 tabs, same fixture CC-12 uses). Not wired into
/// `layout_measured_hyp`/`layout_mutation_incremental` yet (that pipeline
/// wiring is S5), so this calls
/// `lumen_layout::counters::{precompute_counters, incremental_precompute_counters}`
/// directly rather than going through CC-12's full `relayout_chrome_host`
/// cycle.
///
/// Deliberately does **not** mirror CC-12's own hover fixture
/// (`SIDEBAR`/`None` toggle each cycle): `restyle_root_set_for_state_change`
/// treats a transition where nothing was previously hovered as "every
/// ancestor of the new target flipped its `:hover` boolean" (correct per
/// CSS Selectors L4 §4.3 — `:hover` matches ancestors too), which forces a
/// conservative full-subtree invalidation from close to the document root.
/// That is real, correct behaviour of the v1 model (brief §4 explicitly
/// allows v1 to over-approximate), but it means CC-12's specific
/// on/off-toggle interaction shape is close to a worst case for this
/// model, not the common case — sibling-to-sibling hover motion (this
/// test) is what most real mouse movement over already-hovered chrome
/// looks like, and is where the model is supposed to pay off. Recorded
/// here as the honest, representative number; see BUG-341 for the
/// SIDEBAR/None-toggle number as a documented worst case instead of a
/// silently-omitted one.
///
/// `#[ignore]`d like CC-12 itself — a wall-clock number isn't a pass/fail
/// gate here (no pipeline consumes the incremental path yet), it is a
/// recorded measurement (brief §5 S3: "measure `precompute_counters` share
/// drop"). Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s3_incremental_cascade_precompute_share -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf measurement (BUG-341 S3) — see doc comment for run command"]
fn bug341_s3_incremental_cascade_precompute_share() {
    use lumen_layout::counters::{
        incremental_precompute_counters, precompute_counters, set_incremental_restyle, RestyleDelta,
    };
    use lumen_layout::style::restyle_root_set_for_state_change;

    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let model = cc12_bench_model("");
    lumen_chrome::bind_model(&mut doc, &model);
    let viewport = Size::new(1280.0, 800.0);
    let flat = lumen_dom::build_flat_tree(&doc);

    let tabs_container = doc
        .find_by_id(lumen_chrome::ids::SB_TABS)
        .expect("chrome preview must have #sbTabs");
    let tab_rows = doc.get(tabs_container).children.clone();
    assert!(tab_rows.len() >= 2, "bind_model must have populated at least 2 tab rows");
    let tab_a = tab_rows[0];
    let tab_b = tab_rows[1];

    const WARMUP: usize = 10;
    const SAMPLES: usize = 60;

    // Baseline snapshot: tab_a hovered (steady state before the move).
    lumen_layout::set_interactive_state(Some(tab_a), None, None);
    let baseline = precompute_counters(&doc, &sheet, viewport, &flat, false);
    let total_nodes = baseline.styles().len();

    let mut full_stats = lumen_paint::FrameStats::new();
    for i in 0..WARMUP + SAMPLES {
        lumen_layout::set_interactive_state(Some(tab_b), None, None);
        let t0 = std::time::Instant::now();
        let map = precompute_counters(&doc, &sheet, viewport, &flat, false);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if i >= WARMUP {
            full_stats.record(ms as f32);
        }
        std::hint::black_box(&map);
    }
    let full_summary = full_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG341_S3_FULL_PRECOMPUTE"));

    let state_index = lumen_layout::style::restyle_state_index(&doc, &sheet);
    let dirty_roots = restyle_root_set_for_state_change(&doc, Some(tab_a), Some(tab_b), &state_index);
    let dirty_count = dirty_roots.len();
    set_incremental_restyle(true);
    let mut incr_stats = lumen_paint::FrameStats::new();
    let mut last_incr_map = None;
    for i in 0..WARMUP + SAMPLES {
        lumen_layout::set_interactive_state(Some(tab_b), None, None);
        // BUG-341 S24: the cache is consumed by the pass, so each sample
        // gets its own copy of the same baseline — built outside the timed
        // region, exactly like the `prev` tree copy the S4 bench below makes.
        let delta = RestyleDelta {
            prev_styles: baseline.styles().clone(),
            dirty_roots: dirty_roots.clone(),
            content_dirty: lumen_layout::counters::ContentDirty::Nothing,
        };
        let t0 = std::time::Instant::now();
        let map = incremental_precompute_counters(&doc, &sheet, viewport, &flat, false, delta);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if i >= WARMUP {
            incr_stats.record(ms as f32);
        }
        last_incr_map = Some(map);
    }
    set_incremental_restyle(false);
    lumen_layout::clear_interactive_state();
    let incr_summary = incr_stats.summary().expect("samples collected");
    eprintln!("{}", incr_summary.display_with("BUG341_S3_INCREMENTAL_PRECOMPUTE"));

    // Correctness: same hover target, must match the full cascade exactly
    // regardless of the wall-time saving (brief §4 correctness gate).
    lumen_layout::set_interactive_state(Some(tab_b), None, None);
    let full_after = precompute_counters(&doc, &sheet, viewport, &flat, false);
    lumen_layout::clear_interactive_state();
    assert_eq!(
        last_incr_map.expect("at least one sample").styles(),
        full_after.styles(),
        "incremental cascade must reproduce the full cascade exactly on the chrome doc",
    );

    eprintln!(
        "BUG341_S3: {total_nodes} nodes, dirty_roots={dirty_count}; full_precompute \
             p50={:.4}ms p95={:.4}ms; incremental_precompute p50={:.4}ms p95={:.4}ms; drop={:.1}%",
        full_summary.p50_ms,
        full_summary.p95_ms,
        incr_summary.p50_ms,
        incr_summary.p95_ms,
        (1.0 - incr_summary.p50_ms as f64 / full_summary.p50_ms as f64) * 100.0,
    );
}

/// BUG-341 S4 — real-machine measurement companion to the S3 test above:
/// wall-clock `build_box` (full rebuild every call) vs
/// `incremental_build_box` (whole-subtree reuse for the untouched region)
/// on the same CC-12 chrome-preview hover transition. Feeds the S4
/// recorded measurement (brief §5 S4: "measure `build_box` share drop").
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s4_incremental_box_build_share -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf measurement (BUG-341 S4) — see doc comment for run command"]
fn bug341_s4_incremental_box_build_share() {
    use lumen_layout::box_tree::{incremental_build_box, set_incremental_box_build};
    use lumen_layout::counters::{
        build_counter_style_registry, incremental_precompute_counters, precompute_counters,
        set_incremental_restyle, RestyleDelta,
    };
    use lumen_layout::style::{restyle_root_set_for_state_change, ComputedStyle};

    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let model = cc12_bench_model("");
    lumen_chrome::bind_model(&mut doc, &model);
    let viewport = Size::new(1280.0, 800.0);
    let flat = lumen_dom::build_flat_tree(&doc);
    let root_style = ComputedStyle::root();
    let registry = build_counter_style_registry(&sheet);

    let tabs_container = doc
        .find_by_id(lumen_chrome::ids::SB_TABS)
        .expect("chrome preview must have #sbTabs");
    let tab_rows = doc.get(tabs_container).children.clone();
    assert!(tab_rows.len() >= 2, "bind_model must have populated at least 2 tab rows");
    let tab_a = tab_rows[0];
    let tab_b = tab_rows[1];

    const WARMUP: usize = 10;
    const SAMPLES: usize = 60;

    // Baseline snapshot + box tree: tab_a hovered (the "prev" for reuse).
    // `incremental_build_box` with the flag off degrades to a plain full
    // `build_box` (private to `lumen_layout`, not reachable from here) —
    // used here as the full-rebuild reference/timing throughout. The very
    // first call has no real "prev" yet, so pass an unused placeholder
    // (never consulted while the flag is off).
    set_incremental_box_build(false);
    let mut unused_placeholder = lumen_layout::LayoutBox {
        node: doc.root(),
        rect: Rect::ZERO,
        style: std::sync::Arc::new(root_style.clone()),
        kind: lumen_layout::BoxKind::Skip,
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: Default::default(),
        origin: lumen_layout::BoxOrigin { node: None, role: lumen_layout::BoxRole::Placeholder },
    };
    lumen_layout::set_interactive_state(Some(tab_a), None, None);
    let baseline = precompute_counters(&doc, &sheet, viewport, &flat, false);
    let prev_tree = incremental_build_box(
        &doc, &sheet, doc.root(), &root_style, viewport, &flat, &baseline, &registry, false, &mut unused_placeholder,
    );

    let mut full_stats = lumen_paint::FrameStats::new();
    for i in 0..WARMUP + SAMPLES {
        lumen_layout::set_interactive_state(Some(tab_b), None, None);
        let map = precompute_counters(&doc, &sheet, viewport, &flat, false);
        // BUG-341 S19: each iteration gets its own `prev` — the incremental
        // path moves the reusable subtrees out of it, so a shared one would
        // be empty from the second sample on. The copy is outside the timed
        // region, exactly like the cascade above it.
        let mut prev_copy = prev_tree.clone();
        let t0 = std::time::Instant::now();
        let tree = incremental_build_box(
            &doc, &sheet, doc.root(), &root_style, viewport, &flat, &map, &registry, false, &mut prev_copy,
        );
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if i >= WARMUP {
            full_stats.record(ms as f32);
        }
        std::hint::black_box(&tree);
    }
    let full_summary = full_stats.summary().expect("samples collected");
    eprintln!("{}", full_summary.display_with("BUG341_S4_FULL_BUILD_BOX"));

    let state_index = lumen_layout::style::restyle_state_index(&doc, &sheet);
    let dirty_roots = restyle_root_set_for_state_change(&doc, Some(tab_a), Some(tab_b), &state_index);
    set_incremental_restyle(true);
    set_incremental_box_build(true);
    let mut incr_stats = lumen_paint::FrameStats::new();
    let mut last_incr_tree = None;
    for i in 0..WARMUP + SAMPLES {
        lumen_layout::set_interactive_state(Some(tab_b), None, None);
        // BUG-341 S24: one fresh cache per sample, like the `prev` copy below.
        let delta = RestyleDelta {
            prev_styles: baseline.styles().clone(),
            dirty_roots: dirty_roots.clone(),
            content_dirty: lumen_layout::counters::ContentDirty::Nothing,
        };
        let map = incremental_precompute_counters(&doc, &sheet, viewport, &flat, false, delta);
        // See the full-rebuild loop above: one fresh `prev` per sample.
        let mut prev_copy = prev_tree.clone();
        let t0 = std::time::Instant::now();
        let tree = incremental_build_box(
            &doc, &sheet, doc.root(), &root_style, viewport, &flat, &map, &registry, false, &mut prev_copy,
        );
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if i >= WARMUP {
            incr_stats.record(ms as f32);
        }
        last_incr_tree = Some(tree);
    }
    set_incremental_restyle(false);
    set_incremental_box_build(false);
    lumen_layout::clear_interactive_state();
    let incr_summary = incr_stats.summary().expect("samples collected");
    eprintln!("{}", incr_summary.display_with("BUG341_S4_INCREMENTAL_BUILD_BOX"));

    // Correctness: same hover target, must match a full rebuild exactly
    // regardless of the wall-time saving (brief §4 correctness gate).
    lumen_layout::set_interactive_state(Some(tab_b), None, None);
    let full_after_map = precompute_counters(&doc, &sheet, viewport, &flat, false);
    let full_after_tree = incremental_build_box(
        &doc, &sheet, doc.root(), &root_style, viewport, &flat, &full_after_map, &registry, false, &mut prev_tree.clone(),
    );
    lumen_layout::clear_interactive_state();
    let incr_tree = last_incr_tree.expect("at least one sample");

    // Structural sanity check only (node id + `BoxKind` discriminant +
    // child count) — NOT full field/`Debug` equality: `ComputedStyle`
    // carries a `custom_props: HashMap<String, String>` (CSS custom
    // properties, heavily used by this design-system chrome doc), and
    // `HashMap`'s `Debug` prints entries in iteration order, which two
    // independently-computed (but content-equal) cascades need not share.
    // `lay_out`/`collect_rects`-based bit-for-bit verification (the real
    // BUG-341 S4 correctness gate) lives in `lumen_layout`'s own
    // differential tests (`box_build_*` in `box_tree.rs`), which have
    // access to the crate-private `lay_out` this test does not.
    fn assert_same_shape(a: &lumen_layout::LayoutBox, b: &lumen_layout::LayoutBox, path: &mut Vec<String>) {
        assert_eq!(a.node, b.node, "{}: node id mismatch", path.join(">"));
        assert_eq!(
            std::mem::discriminant(&a.kind), std::mem::discriminant(&b.kind),
            "{}: BoxKind discriminant mismatch a={:?} b={:?}", path.join(">"), a.kind, b.kind,
        );
        assert_eq!(
            a.children.len(), b.children.len(),
            "{}: children.len() mismatch", path.join(">"),
        );
        for (i, (ca, cb)) in a.children.iter().zip(b.children.iter()).enumerate() {
            path.push(format!("child[{i}] node={:?}", ca.node));
            assert_same_shape(ca, cb, path);
            path.pop();
        }
    }
    assert_same_shape(&incr_tree, &full_after_tree, &mut vec!["root".to_string()]);

    eprintln!(
        "BUG341_S4: full_build_box p50={:.4}ms p95={:.4}ms; incremental_build_box \
             p50={:.4}ms p95={:.4}ms; drop={:.1}%",
        full_summary.p50_ms,
        full_summary.p95_ms,
        incr_summary.p50_ms,
        incr_summary.p95_ms,
        (1.0 - incr_summary.p50_ms as f64 / full_summary.p50_ms as f64) * 100.0,
    );
}

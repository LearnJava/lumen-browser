//! Вторая половина ручных переписей BUG-341 (S27 и позже) — все под `#[ignore]`.
//! Первая половина — [`super::bug341_census`].

use super::*;

use super::bug341_census::census_describe;

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
            // BUG-341 S40: this census counts *real* `lay_out_inner` calls, and
            // S40's in-place reuse removes some of them. Pinned off here so the
            // numbers stay comparable with S30-S39's, which were all measured
            // before the mechanism existed; S40's own effect is measured by
            // `bug341_s40_in_place_reuse_share` instead.
            lumen_layout::box_tree::set_layout_in_place_reuse(false);
            lumen_layout::box_tree::set_layout_key_census(true);
            lumen_layout::box_tree::set_flex_column_census(true);
            let (_layout, _counters) = lumen_layout::layout_measured_hyp_with_counters(
                &doc, &sheet, viewport, &measurer, &hyp, false,
            );
            let census = lumen_layout::box_tree::take_layout_key_census();
            // BUG-341 S41: the same pass, counted from the other end — not "how
            // many calls could a cache have served" (S30-S40, answered and
            // closed at 3.1%) but "how many second calls are still made, and
            // why". `double` is the residual the bug is named for; its split
            // says whether the remaining path S40's "Not attempted" names
            // (stop calling full `lay_out` for an intrinsic height) is worth
            // building at all.
            let fc = lumen_layout::box_tree::take_flex_column_census();
            eprintln!(
                "[s41-double] {scenario} cycle={i} needed={} memo_served={} probed={} \
                 replayed={} double={} ({:.1}% of probed) dirty={} size={} (grew={}) cross={}",
                fc.needed,
                fc.memo_served,
                fc.probed,
                fc.replayed,
                fc.double,
                100.0 * fc.double as f64 / fc.probed.max(1) as f64,
                fc.double_dirty,
                fc.double_size,
                fc.double_size_grew,
                fc.double_cross,
            );
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
            // BUG-341 S39: of the repeats already safe by style+override, how
            // many also land at the *identical* `(start_x, start_y)`. A
            // `LayoutBox` stores absolute rects, so serving a shared (not
            // cloned) subtree at a different origin means mutating every
            // descendant — copy-on-write, i.e. the deep clone S38's
            // recommended redesign (ii) exists to avoid. Only the share
            // counted here is free for a share-don't-clone mechanism.
            eprintln!(
                "[s39-origin] {scenario} cycle={i} same_style_and_override={} same_origin_too={} ({:.1}% of same_style_and_override, {:.1}% of repeats)",
                census.repeat_key_same_style_and_override,
                census.repeat_key_same_style_override_and_origin,
                100.0 * census.repeat_key_same_style_override_and_origin as f64
                    / census.repeat_key_same_style_and_override.max(1) as f64,
                100.0 * census.repeat_key_same_style_override_and_origin as f64
                    / census.repeat_key_calls.max(1) as f64,
            );
            // BUG-341 S38: per-key occurrence-count histogram — S33's "clone
            // only on second sighting" policy was rejected for the CSS-Grid
            // probe/final case because that case's only repeats are exactly
            // two occurrences; this answers the same question for the
            // flex-item redundancy this fixture actually exercises, per
            // docs/perf-method.md's rule not to let that finding transfer
            // unchecked to a different target.
            let distinct_keys = census.keys_seen_once + census.keys_seen_twice + census.keys_seen_three_plus;
            eprintln!(
                "[s38-shape] {scenario} cycle={i} distinct_keys={distinct_keys} once={} ({:.1}%) twice={} ({:.1}%) three_plus={} ({:.1}%)",
                census.keys_seen_once,
                100.0 * census.keys_seen_once as f64 / distinct_keys.max(1) as f64,
                census.keys_seen_twice,
                100.0 * census.keys_seen_twice as f64 / distinct_keys.max(1) as f64,
                census.keys_seen_three_plus,
                100.0 * census.keys_seen_three_plus as f64 / distinct_keys.max(1) as f64,
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

/// BUG-341 S38: same real chrome document/protocol as S36's own
/// `bug341_s36_layout_result_cache_share`, extended to a third arm — the
/// `Lazy` insertion policy (`set_layout_result_cache_lazy`) that defers the
/// subtree clone S37 isolated as the per-miss fixed cost's dominant term to
/// a key's confirmed second sighting, instead of `Eager`'s (S36)
/// unconditional clone-on-every-miss. S38's own key-occurrence census found
/// this fixture's real repeat shape is dominated by 3+-occurrence keys
/// (86.3% of distinct keys, vs. 4.4% exactly-twice) — the shape `Lazy`
/// needs to recoup most of `Eager`'s hits while skipping the clone on the
/// 9.2% of keys never seen again at all.
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s38_layout_result_cache_lazy_share -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf measurement (BUG-341 S38) — see doc comment for run command"]
fn bug341_s38_layout_result_cache_lazy_share() {
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
        let mut eager_stats = lumen_paint::FrameStats::new();
        let mut lazy_stats = lumen_paint::FrameStats::new();
        let mut eager_hits = 0u64;
        let mut eager_misses = 0u64;
        let mut lazy_hits = 0u64;
        let mut lazy_misses = 0u64;
        let mut lazy_deferred = 0u64;

        for i in 0..WARMUP + SAMPLES {
            let (model, hover) = if scenario == "KEY" {
                typed.push('a');
                (cc12_bench_model(&typed), None)
            } else {
                (cc12_bench_model(""), if i % 2 == 0 { sidebar } else { None })
            };
            lumen_chrome::bind_model(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);

            // Three-arm interleaved A/B/C per cycle on the *same* document
            // state (docs/perf-method.md's "interleaved A/B compared on
            // min" rule, extended to three arms rather than two) — OFF,
            // EAGER (S36), LAZY (S38), same order every cycle since the
            // rotation rule (docs/perf-method.md §4) is about eliminating a
            // *fixed-position* bias across repeated circles, and all three
            // arms already share the same position across all 70 circles.
            let t = std::time::Instant::now();
            let _ = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
            let off_ns = t.elapsed().as_nanos();

            lumen_layout::box_tree::set_layout_result_cache(true);
            let t = std::time::Instant::now();
            let _ = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
            let eager_ns = t.elapsed().as_nanos();
            let e_stats = lumen_layout::box_tree::take_layout_result_cache_stats();
            lumen_layout::box_tree::set_layout_result_cache(false);

            lumen_layout::box_tree::set_layout_result_cache_lazy(true);
            let t = std::time::Instant::now();
            let _ = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
            let lazy_ns = t.elapsed().as_nanos();
            let l_stats = lumen_layout::box_tree::take_layout_result_cache_stats();
            lumen_layout::box_tree::set_layout_result_cache_lazy(false);

            if i >= WARMUP {
                off_stats.record(off_ns as f32 / 1e6);
                eager_stats.record(eager_ns as f32 / 1e6);
                lazy_stats.record(lazy_ns as f32 / 1e6);
                eager_hits += e_stats.hits as u64;
                eager_misses += e_stats.misses as u64;
                lazy_hits += l_stats.hits as u64;
                lazy_misses += l_stats.misses as u64;
                lazy_deferred += l_stats.deferred as u64;
            }
            lumen_layout::clear_interactive_state();
        }
        let off_summary = off_stats.summary().expect("samples collected");
        let eager_summary = eager_stats.summary().expect("samples collected");
        let lazy_summary = lazy_stats.summary().expect("samples collected");
        eprintln!("{}", off_summary.display_with(&format!("BUG341_S38_{scenario}_CACHE_OFF")));
        eprintln!("{}", eager_summary.display_with(&format!("BUG341_S38_{scenario}_CACHE_EAGER")));
        eprintln!("{}", lazy_summary.display_with(&format!("BUG341_S38_{scenario}_CACHE_LAZY")));
        eprintln!(
            "[s38-cache] {scenario} eager hits={eager_hits} misses={eager_misses} hit_rate={:.1}%",
            100.0 * eager_hits as f64 / (eager_hits + eager_misses).max(1) as f64,
        );
        eprintln!(
            "[s38-cache] {scenario} lazy hits={lazy_hits} misses={lazy_misses} deferred={lazy_deferred} \
                 hit_rate={:.1}% (of misses, deferred={:.1}%)",
            100.0 * lazy_hits as f64 / (lazy_hits + lazy_misses).max(1) as f64,
            100.0 * lazy_deferred as f64 / lazy_misses.max(1) as f64,
        );
    }
}

/// Number of element nodes in `id`'s subtree, inclusive — the "how wide is
/// this dirty root" column of the S17 census.
pub(super) fn census_subtree_elements(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> usize {
    let node = doc.get(id);
    let own = usize::from(matches!(node.data, lumen_dom::NodeData::Element { .. }));
    own + node.children.iter().map(|&c| census_subtree_elements(doc, c)).sum::<usize>()
}

/// BUG-341 S40: real wall-clock effect of in-place layout reuse
/// (`lumen_layout::box_tree::set_layout_in_place_reuse`, **on by default**) on
/// the same real chrome document/fixture S30-S39 measured, with the same
/// interleaved-per-cycle A/B protocol S36/S38 used, so the number is
/// comparable with theirs rather than with a fresh harness.
///
/// Unlike the S32/S36/S38 layout-result cache this replaces in role, a hit
/// here copies nothing: the box already holds the result, so the hit is a
/// `return`. See `LayoutInPlaceKey`'s doc comment for why that is a different
/// mechanism and not another insertion-policy variant.
/// Run: `cargo test -p lumen-shell --profile dev-release
/// bug341_s40_in_place_reuse_share -- --ignored --nocapture`.
#[test]
#[ignore = "manual perf measurement (BUG-341 S40) — see doc comment for run command"]
fn bug341_s40_in_place_reuse_share() {
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
        let mut hits = 0u64;
        let mut recorded = 0u64;
        let mut refused = 0u64;

        for i in 0..WARMUP + SAMPLES {
            let (model, hover) = if scenario == "KEY" {
                typed.push('a');
                (cc12_bench_model(&typed), None)
            } else {
                (cc12_bench_model(""), if i % 2 == 0 { sidebar } else { None })
            };
            lumen_chrome::bind_model(&mut doc, &model);
            lumen_layout::set_interactive_state(hover, None, None);

            lumen_layout::box_tree::set_layout_in_place_reuse(false);
            let t = std::time::Instant::now();
            let _ = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
            let off_ns = t.elapsed().as_nanos();

            lumen_layout::box_tree::set_layout_in_place_reuse(true);
            let _ = lumen_layout::box_tree::take_layout_in_place_stats();
            let t = std::time::Instant::now();
            let _ = lumen_layout::layout_measured_hyp(&doc, &sheet, viewport, &measurer, &hyp, false);
            let on_ns = t.elapsed().as_nanos();
            let s = lumen_layout::box_tree::take_layout_in_place_stats();

            if i >= WARMUP {
                off_stats.record(off_ns as f32 / 1e6);
                on_stats.record(on_ns as f32 / 1e6);
                hits += s.hits as u64;
                recorded += s.recorded as u64;
                refused += s.refused as u64;
            }
            lumen_layout::clear_interactive_state();
        }
        let off_summary = off_stats.summary().expect("samples collected");
        let on_summary = on_stats.summary().expect("samples collected");
        eprintln!("{}", off_summary.display_with(&format!("BUG341_S40_{scenario}_REUSE_OFF")));
        eprintln!("{}", on_summary.display_with(&format!("BUG341_S40_{scenario}_REUSE_ON")));
        eprintln!(
            "[s40-reuse] {scenario} hits={hits} recorded={recorded} refused={refused} \
             hit_rate={:.1}% of computed-or-served",
            100.0 * hits as f64 / (hits + recorded + refused).max(1) as f64,
        );
    }
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
            shallow_roots: Default::default(), point_roots: Default::default(),
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
        used_line_height: root_style.font_size * root_style.line_height,
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
            shallow_roots: Default::default(), point_roots: Default::default(),
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

/// BUG-341 S45 diagnostic: *which* boxes a hover flip rebuilds (S43 counted
/// 31 built / 11 reused; nothing named them). Prints the built list with each
/// node's role in the cycle — dirty root, ancestor of one, recascaded — and
/// the number of distinct nodes, which answers whether "31" is one box per
/// node or the same node built more than once. Run: `cargo test -p lumen-shell
/// --profile dev-release bug341_s45_hover_built_census -- --ignored --nocapture`.
#[test]
#[ignore = "manual diagnostic (BUG-341 S45) — see doc comment for run command"]
fn bug341_s45_hover_built_census() {
    use std::collections::HashSet;

    let (mut doc, sheet) = lumen_chrome::parse_document(chrome_preview::HTML);
    let font = lumen_font::Font::parse(INTER_FONT).expect("bundled Inter не парсится");
    let measurer = lumen_paint::FontMeasurer::new(&font).expect("FontMeasurer из bundled Inter");
    let hyp = KnuthLiangHyphenation::new();
    let viewport = Size::new(1280.0, 800.0);
    let model = cc12_bench_model("");
    lumen_chrome::bind_model(&mut doc, &model);
    let sidebar = doc.find_by_id(lumen_chrome::ids::SIDEBAR);

    let mut state = Cc12IncrementalState::default();
    for i in 0..6 {
        let hover = if i % 2 == 0 { sidebar } else { None };
        let last = i >= 4;
        let prev_hover = state.prev_interactive.0;
        lumen_layout::box_tree::set_box_build_diagnostics(last);
        let (_, bb) =
            cc12_bench_cycle(&mut doc, &sheet, &model, viewport, &measurer, &hyp, hover, &mut state);
        let built = lumen_layout::box_tree::take_box_build_log();
        lumen_layout::box_tree::set_box_build_diagnostics(false);
        if !last {
            continue;
        }
        let distinct: HashSet<_> = built.iter().copied().collect();
        eprintln!(
            "[s45-census] built={} distinct_nodes={} reused={} hover_from={:?} hover_to={:?}",
            bb.built,
            distinct.len(),
            bb.reused,
            prev_hover.map(|n| census_describe(&doc, n)),
            hover.map(|n| census_describe(&doc, n)),
        );
        for &n in &built {
            let depth = {
                let (mut d, mut c) = (0, doc.get(n).parent);
                while let Some(p) = c {
                    d += 1;
                    c = doc.get(p).parent;
                }
                d
            };
            eprintln!(
                "[s45-census]   depth={depth:2} {} ({} children)",
                census_describe(&doc, n),
                doc.get(n).children.len()
            );
        }
    }
}

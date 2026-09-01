//! Тесты `lumen-shell`, вынесенные из `main.rs` (дорожка SPLIT, батч SH-4b).
//!
//! Здесь остаются мелкие тесты, не образующие темы, и обвязка, общая для
//! нескольких подмодулей: стенд CC-12 (`cc12_bench_*`, `Cc12IncrementalState`),
//! `census_count_boxes` и `args`. Приватные элементы этого модуля видны его
//! потомкам, поэтому каждому подмодулю хватает `use super::*;`.

use super::*;

mod bfcache_salvage;
mod bug341_census;
mod chrome_incremental;
mod cli;
mod page_pipeline;
mod page_resources;
mod scripts_and_frames;

// ── BUG-436: typed characters reach the JS dispatch intact ───────────────

#[test]
fn escaped_char_is_safe_inside_single_quoted_js_literal() {
    // Every call site interpolates into `'...'`, so an apostrophe must not
    // close the literal — it used to, and the whole dispatch script was
    // dropped as a syntax error.
    assert_eq!(escape_js_string_char('\''), r"\'");
    assert_eq!(escape_js_string_char('\\'), r"\\");
    assert_eq!(escape_js_string_char('a'), "a");
}

#[test]
fn escaped_string_escapes_every_quote() {
    assert_eq!(escape_js_string("it's"), r"it\'s");
    // Non-ASCII goes over as a `\uXXXX` escape, not a raw character.
    assert_eq!(escape_js_string("\u{444}"), r"\u0444");
}

// \u2500\u2500 BUG-829: a traversal hands JS the entry's state as JSON *text* \u2500\u2500\u2500\u2500\u2500\u2500\u2500

/// The shim's `_lumen_deliver_popstate` runs `JSON.parse` on its first
/// argument, so a bare `{"n":1}` reached it as an object literal, threw
/// inside that parse and delivered `state: null` \u2014 for every state but
/// `null`, which is the one value that survived the confusion unchanged.
#[cfg(feature = "v8")]
#[test]
fn popstate_state_is_passed_as_a_json_string_not_an_object_literal() {
    assert_eq!(
        popstate_eval_source(r#"{"n":1}"#, "https://example.com/p?a=1"),
        r#"_lumen_deliver_popstate("{\"n\":1}", 'https://example.com/p?a=1')"#
    );
    // A string state is the case the confusion could not have survived at
    // all: bare, `"hi"` arrived as the JS string `hi` and `JSON.parse`
    // rejected it.
    assert_eq!(
        popstate_eval_source(r#""hi""#, ""),
        r#"_lumen_deliver_popstate("\"hi\"", '')"#
    );
    assert_eq!(
        popstate_eval_source("null", ""),
        r#"_lumen_deliver_popstate("null", '')"#
    );
}

/// The URL keeps its own single-quote encoding, and a state carrying a
/// quote or a backslash must not be able to break out of either literal.
#[cfg(feature = "v8")]
#[test]
fn popstate_eval_source_escapes_both_arguments() {
    let src = popstate_eval_source(r#"{"s":"a'b\\c"}"#, "https://e.example/it's\\x");
    assert!(src.contains(r#"'https://e.example/it\'s\\x'"#), "url: {src}");
    // The state literal is double-quoted, so an apostrophe inside it is
    // harmless; the backslash and the inner quotes are the ones escaped.
    assert!(src.contains(r#""{\"s\":\"a'b\\\\c\"}""#), "state: {src}");
}

// ── ADR-023: engine-thread default flip + rollback precedence ────────────

#[test]
fn engine_thread_on_by_default_when_no_vars_set() {
    assert!(engine_thread_enabled_from(None, None));
}

#[test]
fn engine_thread_opt_out_disables() {
    assert!(!engine_thread_enabled_from(Some("1"), None));
}

#[test]
fn engine_thread_opt_out_beats_explicit_legacy_on() {
    assert!(!engine_thread_enabled_from(Some("1"), Some("1")));
}

#[test]
fn engine_thread_legacy_zero_disables() {
    assert!(!engine_thread_enabled_from(None, Some("0")));
}

#[test]
fn engine_thread_legacy_one_still_enables() {
    assert!(engine_thread_enabled_from(None, Some("1")));
}

#[test]
fn engine_thread_opt_out_only_honours_exact_one() {
    // Anything other than "1" is not the documented opt-out spelling.
    assert!(engine_thread_enabled_from(Some("0"), None));
    assert!(engine_thread_enabled_from(Some(""), None));
}

// ── DS-1: design-token generator output sanity ───────────────────────────

#[test]
fn theme_tokens_radius_lg_matches_prototype() {
    assert_eq!(crate::theme_tokens::radius::LG, 6.0);
}

#[test]
fn theme_tokens_profile_anonymous_matches_prototype() {
    assert_eq!(
        crate::theme_tokens::profile::ANONYMOUS,
        lumen_layout::Color {
            r: 255,
            g: 59,
            b: 48,
            a: 255,
        }
    );
}

// ── CC-12: chrome perf gate — mutate → restyle → relayout → paint cycle ─

/// Builds a populated `ChromeModel` (6 tabs, 3 workspaces) so the bound
/// document approaches the ~400-node ballpark CC-12's brief measures
/// against, rather than the near-empty `ChromeModel::default()` a real
/// session never actually renders.
fn cc12_bench_model(omnibox_value: &str) -> lumen_chrome::ChromeModel {
    let tabs = (0..6usize)
        .map(|i| lumen_chrome::ChromeTabModel {
            id: i,
            title: format!("Tab {i}"),
            active: i == 0,
            sleeping: false,
            is_child: false,
            container_color: None,
            group: None,
        })
        .collect();
    let workspaces = (0..3i64)
        .map(|i| lumen_chrome::ChromeWorkspaceModel {
            id: i,
            name: format!("Workspace {i}"),
            active: i == 0,
            color: "#3b82f6".to_owned(),
        })
        .collect();
    lumen_chrome::ChromeModel {
        tabs,
        workspaces,
        omnibox: lumen_chrome::OmniboxModel { value: omnibox_value.to_owned(), ..Default::default() },
        ..Default::default()
    }
}

/// BUG-341 S24: [`cc12_bench_model`] with half its tabs and workspaces
/// gone, so a cycle really detaches DOM nodes instead of only rewriting
/// attributes. The eviction arm of the S24 gate needs a pass whose element
/// set genuinely shrinks.
fn cc12_bench_shrunk_model(omnibox_value: &str) -> lumen_chrome::ChromeModel {
    let mut model = cc12_bench_model(omnibox_value);
    model.tabs.truncate(2);
    model.workspaces.truncate(1);
    model
}

/// BUG-341 S5: persisted state `cc12_bench_cycle` carries across
/// iterations, mirroring exactly what `Lumen::relayout_chrome_host`
/// itself now persists (`chrome_prev_*` fields) — a standalone struct
/// here since this bench has no `Lumen` instance to hang them on.
#[derive(Default)]
struct Cc12IncrementalState {
    prev_pristine_layout: Option<lumen_layout::LayoutBox>,
    prev_cascade_styles: lumen_layout::CascadeStyles,
    prev_interactive: (Option<lumen_dom::NodeId>, Option<lumen_dom::NodeId>, Option<lumen_dom::NodeId>),
    /// BUG-341 S18: run the cycle with whole-subtree box reuse (S15) off,
    /// so the box tree is rebuilt and `graft_geometry` really compares it
    /// against its predecessor box by box. The S13 gate needs that: with
    /// reuse on, the graft is now handed a subtree it knows is a copy of
    /// `prev` and honours it in O(1), which is the right production
    /// behaviour but leaves nothing for a per-box reject census to measure.
    box_reuse_off: bool,
}

/// One `relayout_chrome_host`-equivalent pass, timed exactly like the
/// mutate (`bind_model`) → restyle+relayout → persist-for-next-cycle →
/// paint (`paint_ordered`, display-list build — CC-12's "paint" stops
/// here, same as the rest of the engine's layout→paint terminology; GPU
/// submit/present is a separate stage this cycle never reaches) sequence
/// `Lumen::relayout_chrome_host` runs on every chrome interaction.
///
/// BUG-341 S6: mirrors `relayout_chrome_host`'s own eligibility check —
/// the incremental path is taken whenever a previous pristine tree/
/// cascade cache exists, deriving its cascade dirty-root-set from
/// `bind_model_tracked`'s real diff (unioned with the interactive-state
/// root-set) instead of requiring the whole `ChromeModel` to be
/// bit-identical (S5's limit — that's what kept `CC12_KEY`'s per-cycle
/// omnibox-text change on the full-layout path). Falls back to
/// `layout_measured_hyp_with_counters` only on the first cycle. `state`'s
/// persist of the fresh tree/cascade cache is deliberately inside the timed
/// region — that per-cycle cost is real production overhead, not a
/// benchmark artifact. BUG-341 S22 turned the tree half of it from a
/// whole-tree `clone()` into a move.
#[allow(clippy::too_many_arguments)]
fn cc12_bench_cycle(
    doc: &mut lumen_dom::Document,
    sheet: &lumen_css_parser::Stylesheet,
    model: &lumen_chrome::ChromeModel,
    viewport: Size,
    measurer: &lumen_paint::FontMeasurer,
    hyp: &KnuthLiangHyphenation,
    hover: Option<lumen_dom::NodeId>,
    state: &mut Cc12IncrementalState,
) -> (f64, lumen_layout::box_tree::BoxBuildStats) {
    let touched = lumen_chrome::bind_model_tracked(doc, model);
    lumen_layout::set_interactive_state(hover, None, None);
    let new_interactive = (hover, None, None);

    let t0 = std::time::Instant::now();
    let (layout, counters) = match state.prev_pristine_layout.take() {
        Some(prev) => {
            let (prev_hover, prev_focus, prev_active) = state.prev_interactive;
            let state_index = lumen_layout::style::restyle_state_index(doc, sheet);
            let mut dirty_roots = std::collections::HashSet::new();
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                doc, prev_hover, new_interactive.0, &state_index,
            ));
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                doc, prev_focus, new_interactive.1, &state_index,
            ));
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_state_change(
                doc, prev_active, new_interactive.2, &state_index,
            ));
            let node_index = lumen_layout::style::restyle_node_index(doc, sheet);
            dirty_roots.extend(lumen_layout::style::restyle_root_set_for_node_change(
                doc,
                chrome_node_changes(&touched),
                &node_index,
            ));
            let delta = lumen_layout::counters::RestyleDelta {
                prev_styles: std::mem::take(&mut state.prev_cascade_styles),
                dirty_roots,
                // BUG-341 S16 — mirrors `relayout_chrome_host` exactly, so
                // the bench measures the production reuse decision.
                content_dirty: lumen_layout::counters::ContentDirty::Nodes(&touched.content),
            };
            lumen_layout::counters::set_incremental_restyle(true);
            lumen_layout::box_tree::set_incremental_box_build(!state.box_reuse_off);
            let result = lumen_layout::box_tree::layout_mutation_incremental_restyle(
                doc, sheet, viewport, measurer, hyp, false, prev, delta,
            );
            lumen_layout::box_tree::set_incremental_box_build(false);
            lumen_layout::counters::set_incremental_restyle(false);
            result
        }
        None => lumen_layout::layout_measured_hyp_with_counters(doc, sheet, viewport, measurer, hyp, false),
    };
    // BUG-341 S8: split the timed region so the incremental design's own
    // per-cycle bookkeeping (two deep clones of the whole document) is
    // visible separately from the layout work it is meant to be saving —
    // the S8 re-analysis found that bookkeeping to be ~12-13ms of the
    // cycle, which no stage profile inside `lumen-layout` can show.
    let t_layout = t0.elapsed().as_secs_f64() * 1000.0;
    let t2 = std::time::Instant::now();
    state.prev_cascade_styles = counters.into_styles();
    let t_clone_styles = t2.elapsed().as_secs_f64() * 1000.0;
    let t3 = std::time::Instant::now();
    let _dl = paint_ordered(&layout);
    let t_paint = t3.elapsed().as_secs_f64() * 1000.0;
    // BUG-341 S22: a **move**, not a `clone()`. Production stopped copying
    // the tree here too — `relayout_chrome_host` hands the next pass its
    // own live tree with `take_content_area`'s removals undone
    // (`restore_content_area`). This bench has never modelled the pruning,
    // so the move is the whole of the change here; production additionally
    // pays the restore, which is one insert per salvaged popover plus a
    // path walk and does not scale with the tree.
    let t1 = std::time::Instant::now();
    state.prev_pristine_layout = Some(layout);
    let t_persist_tree = t1.elapsed().as_secs_f64() * 1000.0;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let bb = lumen_layout::box_tree::take_box_build_stats();
    eprintln!(
        "[cc12-split] total={ms:.2} layout={t_layout:.2} persist_tree={t_persist_tree:.2} \
             clone_styles={t_clone_styles:.2} paint={t_paint:.2} \
             boxes_built={} boxes_reused={}",
        bb.built, bb.reused
    );
    lumen_layout::clear_interactive_state();
    state.prev_interactive = new_interactive;
    (ms, bb)
}

/// Boxes in `b`'s subtree, inclusive — census only.
fn census_count_boxes(b: &lumen_layout::LayoutBox) -> u64 {
    1 + b.children.iter().map(census_count_boxes).sum::<u64>()
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

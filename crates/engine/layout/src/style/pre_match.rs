//! LOAD-6 — parallel selector matching for the initial cascade.
//!
//! The THREAD-4 census ([`ROADMAP.md`] LOAD-6) measured github.com's layout pass at
//! 6.5–8.0 s, of which `precompute_counters` is 83–89 % — almost all `compute_style`,
//! and inside it `cs_match` (selector matching) 82–92 %. The walk itself
//! ([`crate::counters::walk`]) cannot be fanned out: a child's `ComputedStyle`
//! inherits from the parent's, so nodes have to be cascaded in tree order.
//!
//! *Which rules match a node*, however, does not depend on inherited style at all:
//! the candidates come from the read-only [`crate::style::RuleIndex`], the answer is
//! per node, and the cascade's source-order keys are per rule. So a full pass first
//! matches every element in parallel — one [`compute_style_shareable`] per node on a
//! throwaway inherited style, keeping only the matched-rule record it produces
//! ([`MatchedRule`], the same record an incremental pass carries) — and the sequential
//! walk then applies those declarations instead of re-matching them.
//!
//! The record is exactly `compute_style_restricted`'s input with nothing invalidated,
//! so the walk re-applies the declarations through the already-proven restricted road:
//! same declarations, same sort keys, same result — only the candidate scan is gone.
//! `@scope` rules and shadow-tree rules are not part of the record; they keep matching
//! inside `compute_style` itself, for every node, as they did before. For a pre-matched
//! node that is the only matching left in the walk.
//!
//! `LUMEN_NO_PRE_MATCH=1` — A/B switch and the way back if a page ever shows a stale
//! style; read once per process, like the other rollback switches of this track.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lumen_core::geom::Size;
use lumen_css_parser::{Stylesheet, StylesheetRevision};
use lumen_dom::{Document, FlatTree, NodeData, NodeId};

use super::cascade::compute_style_shareable;
use super::invalidation::MatchedRule;
use super::ComputedStyle;

/// Below this many elements the rayon fan-out costs more than the matching it spreads.
/// The measured pages (github.com, the `bug935_cascade_bench` BEM page) are 2 000+.
const MIN_ELEMENTS: usize = 64;

/// One element's pre-matched document-sheet rules plus whether the cascade may memoise
/// a style built from them (the same `shareable` [`compute_style_shareable`] reports).
struct Entry {
    /// `None` for pseudo-only entries — subset passes cover every element for
    /// pseudo records but only `needed` for element rules; `lookup` must not
    /// serve an empty rule record to the cascade.
    rules: Option<Arc<[MatchedRule]>>,
    shareable: bool,
    /// PERF-PSEUDO-PRE — per pseudo-element name, the node's matched pseudo
    /// declarations ([`super::pseudo::PseudoMatch`]). Empty when the sheet
    /// targets no pseudo-elements; a name the worker skipped simply misses the
    /// lookup and the pseudo probe matches sequentially, as before.
    pseudo: Vec<(Box<str>, Arc<[super::pseudo::PseudoMatch]>)>,
    /// PERF-PSEUDO-STORE: at least one record was computed this pass rather
    /// than served from `pseudo_snapshot` — only those need `store_matches_bulk`
    /// (a snapshot hit is already in the cache under the same pass key).
    pseudo_dirty: bool,
}

struct Table {
    revision: StylesheetRevision,
    viewport: (u32, u32),
    dark_mode: bool,
    nodes: HashMap<NodeId, Entry>,
}

thread_local! {
    /// Installed by [`install`] for the duration of one full cascade and cleared by the
    /// returned guard. Thread-local because it is one pass's scratch state: the walk that
    /// consumes it runs on the thread that installed it.
    static TABLE: RefCell<Option<Table>> = const { RefCell::new(None) };
}

/// `LUMEN_NO_PRE_MATCH=1` — see the module doc.
fn disabled() -> bool {
    #[cfg(test)]
    if TEST_OFF.with(std::cell::Cell::get) {
        return true;
    }
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("LUMEN_NO_PRE_MATCH").is_some_and(|v| v != "0"))
}

#[cfg(test)]
thread_local! {
    /// Lets one test drive both roads in one process — the env switch is read once.
    static TEST_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn set_disabled_for_test(off: bool) {
    TEST_OFF.with(|c| c.set(off));
}

#[cfg(test)]
thread_local! {
    /// Gate for the differential tests: proves the walk actually consumed the table instead
    /// of quietly falling back to matching (which would make them pass vacuously).
    static HITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Returns the pre-matched lookups served since the last call (test-only, see [`HITS`]).
#[cfg(test)]
pub(crate) fn take_hits() -> u64 {
    HITS.with(|c| c.replace(0))
}

/// Clears the table when the pass ends, including on an unwind.
pub(crate) struct Guard;

impl Guard {
    /// PERF-PSEUDO-PRE — drains the per-node pseudo match records out of the
    /// table for bulk-loading into the pass's `PseudoCache` (box build runs
    /// after this guard is gone). The table is dead the moment the cascade
    /// walk ends — both callers drop the guard right after — so taking it
    /// moves the records instead of cloning every `Box<str>` name and bumping
    /// every `Arc` (~15k allocations saved per flush on real pages).
    /// Snapshot-served records are already in the pass's `PseudoCache` —
    /// re-storing them is pure write amplification, so `pseudo_dirty` only.
    pub(crate) fn pseudo_records(&self) -> super::pseudo::PseudoRecords {
        TABLE.with(|t| {
            t.borrow_mut()
                .take()
                .map(|table| {
                    table
                        .nodes
                        .into_iter()
                        .filter(|(_, e)| e.pseudo_dirty)
                        .map(|(node, e)| (node, e.pseudo))
                        .collect()
                })
                .unwrap_or_default()
        })
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        TABLE.with(|t| t.borrow_mut().take());
    }
}

/// Matches every element of `flat` in parallel and installs the records for the walk that
/// follows. A document too small to amortise the fan-out, or a build with the switch on,
/// leaves the table empty — every node then matches itself inside `compute_style`, exactly
/// as before.
pub(crate) fn install(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    flat: &FlatTree,
) -> Guard {
    install_impl(doc, sheet, viewport, dark_mode, flat, None, None)
}

/// Incremental-cascade variant: matches only the elements in `needed` — the
/// nodes the scoped walk can recompute — instead of the whole document. A node
/// outside the set simply falls back to matching inside `compute_style`, so an
/// under-approximation costs time, never correctness. `pseudo_cache` lets
/// workers reuse pseudo match records the previous pass left behind — the
/// caller must have dropped the entries of every node this pass recomputes
/// first, or a stale record would be replayed.
pub(crate) fn install_subset(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    flat: &FlatTree,
    needed: &HashSet<NodeId>,
    pseudo_cache: Option<&std::sync::Arc<crate::counters::PseudoCache>>,
) -> Guard {
    install_impl(doc, sheet, viewport, dark_mode, flat, Some(needed), pseudo_cache)
}

fn install_impl(
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
    flat: &FlatTree,
    needed: Option<&HashSet<NodeId>>,
    pseudo_cache: Option<&std::sync::Arc<crate::counters::PseudoCache>>,
) -> Guard {
    let _prof = lumen_core::profile::scope("pre_match");
    TABLE.with(|t| t.borrow_mut().take());
    // `node_count` bounds the element count from above — this early exit keeps tiny documents
    // (almost every unit-test fixture) from even walking the tree.
    if disabled() || doc.node_count() < MIN_ELEMENTS {
        return Guard;
    }
    let mut nodes = Vec::new();
    collect_elements(doc, flat, doc.root(), needed, &mut nodes);
    // PERF-PSEUDO-PRE: the pseudo names this sheet can target, matched per
    // node in the same worker — `compute_pseudo_element_style`'s candidate
    // scan depends only on DOM state, exactly like the element match above.
    let pseudo_names = super::pseudo::pre_match_pseudo_names(sheet, viewport, dark_mode);
    // A subset pass with a tiny `needed` set used to bail here — but box
    // build still probes pseudo styles across the rebuilt subtree, and with
    // no table those probes all take the sequential candidate scan. The
    // pseudo-only coverage below is cheap enough to run anyway.
    let pseudo_fallback = needed.is_some() && !pseudo_names.is_empty();
    if nodes.len() < MIN_ELEMENTS && !pseudo_fallback {
        return Guard;
    }
    // Workers start with default style thread-locals: `:hover`/`:focus`/`:focus-visible`
    // would otherwise match nothing (and shadow sheets disappear) on every node they take.
    let snap = super::StyleEnvSnapshot::capture();
    let inherited = ComputedStyle::root();
    // PERF-PSEUDO-PRE-SUBSET: box build probes pseudo styles of nodes the
    // scoped walk never recomputes (layout-dirty is not style-dirty), so a
    // subset pass that recorded pseudo matches only for `needed` would leave
    // those probes to rescan serially. The pseudo scan reads DOM state only —
    // covering every element costs a fraction of the element match it rides
    // with, and the records stay valid for the whole pass.
    let mut pseudo_nodes: Vec<NodeId> = Vec::new();
    if needed.is_some() && !pseudo_names.is_empty() {
        collect_elements(doc, flat, doc.root(), None, &mut pseudo_nodes);
    }
    let node_set: HashSet<NodeId> = nodes.iter().copied().collect();
    let work: Vec<NodeId> = if pseudo_nodes.is_empty() {
        nodes.clone()
    } else {
        nodes
            .iter()
            .copied()
            .chain(pseudo_nodes.into_iter().filter(|n| !node_set.contains(n)))
            .collect()
    };
    let pseudo_key = pseudo_cache
        .map(|_| crate::counters::pseudo_pass_key(doc, sheet, viewport, dark_mode));
    // One locked clone beats per-lookup mutex traffic across the worker pool.
    let pseudo_snapshot = pseudo_key
        .and_then(|k| pseudo_cache.map(|c| c.match_snapshot(k)));
    let entries: Vec<(NodeId, Entry)> = {
        use rayon::prelude::*;
        work
            .par_iter()
            .map(|&node| {
                snap.install();
                let in_scope = needed.is_none_or(|s| s.contains(&node));
                let (rules, shareable) = if in_scope {
                    let mut rules = Vec::new();
                    // The style itself is discarded: inheritance is what the sequential walk
                    // is for. Only the record of what matched survives.
                    let (_, shareable) = with_ancestor_frames(doc, flat, node, || {
                        compute_style_shareable(
                            doc, node, sheet, &inherited, viewport, dark_mode, None, &mut rules, true,
                        )
                    });
                    // The restricted road binary-searches `prev` by (block, rule).
                    rules.sort_unstable_by_key(|m| (m.block, m.rule));
                    (Some(Arc::from(rules)), shareable)
                } else {
                    (None, false)
                };
                // Reuse last pass's pseudo match records where the caller
                // vouches they are not stale (subset passes pre-drop the
                // recomputed set); a miss recomputes — the bulk store after
                // the walk persists it for the next pass. `pseudo_dirty`
                // marks entries holding at least one computed record: a
                // pure snapshot hit needs no re-store (PERF-PSEUDO-STORE).
                let mut pseudo_dirty = false;
                let pseudo = pseudo_names
                    .iter()
                    .filter_map(|name| {
                        if let Some(rec) =
                            pseudo_snapshot.as_ref().and_then(|s| s.get(node, name))
                        {
                            return Some((name.clone().into_boxed_str(), rec));
                        }
                        pseudo_dirty = true;
                        super::pseudo::match_pseudo_owned(
                            doc, node, name, sheet, viewport, dark_mode,
                        )
                        .map(|rec| (name.clone().into_boxed_str(), rec))
                    })
                    .collect();
                (node, Entry { rules, shareable, pseudo, pseudo_dirty })
            })
            .collect()
    };
    TABLE.with(|t| {
        *t.borrow_mut() = Some(Table {
            revision: sheet.revision(),
            viewport: (viewport.width.to_bits(), viewport.height.to_bits()),
            dark_mode,
            nodes: entries.into_iter().collect(),
        });
    });
    Guard
}

/// This node's pre-matched rules and shareability, when the table belongs to this sheet and
/// media state. `None` falls back to matching inside the cascade.
pub(crate) fn lookup(
    node: NodeId,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) -> Option<(Arc<[MatchedRule]>, bool)> {
    // PERF-PRE-MATCH-HIT: count only lookups that found a live table — calls
    // after the guard dropped (build_box/post passes) would otherwise read as
    // misses and swamp the census the walk actually produces.
    TABLE.with(|t| {
        let t = t.borrow();
        let table = t.as_ref()?;
        if table.revision != sheet.revision()
            || table.dark_mode != dark_mode
            || table.viewport != (viewport.width.to_bits(), viewport.height.to_bits())
        {
            // Stale table — count as absent, not as a coverage miss.
            crate::counters::note_pre_match_lookup_kind(1);
            return None;
        }
        let entry = table.nodes.get(&node);
        let res = entry.and_then(|e| {
            e.rules.as_ref().map(|rules| {
                #[cfg(test)]
                HITS.with(|c| c.set(c.get() + 1));
                (Arc::clone(rules), e.shareable)
            })
        });
        crate::counters::note_pre_match_lookup_kind(match (entry, res.is_some()) {
            (_, true) => 0,
            (None, _) => 1,
            (Some(_), _) => 2,
        });
        res
    })
}

/// PERF-PSEUDO-PRE — this node's pre-matched pseudo declarations for `pseudo`
/// (the bare name, without `::` or a functional argument), when the table
/// belongs to this sheet and media state. `None` falls back to matching inside
/// `compute_pseudo_element_style`, exactly like [`lookup`]'s contract.
pub(in crate::style) fn pseudo_lookup(
    node: NodeId,
    pseudo: &str,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) -> Option<Arc<[super::pseudo::PseudoMatch]>> {
    TABLE.with(|t| {
        let t = t.borrow();
        let table = t.as_ref()?;
        if table.revision != sheet.revision()
            || table.dark_mode != dark_mode
            || table.viewport != (viewport.width.to_bits(), viewport.height.to_bits())
        {
            return None;
        }
        table
            .nodes
            .get(&node)?
            .pseudo
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(pseudo))
            .map(|(_, rec)| Arc::clone(rec))
    })
}

/// Elements of the composed tree in document order — the set [`crate::counters::walk`]
/// re-cascades on a full pass. Text and other non-elements carry no style of their own.
fn collect_elements(
    doc: &Document,
    flat: &FlatTree,
    root: NodeId,
    needed: Option<&HashSet<NodeId>>,
    out: &mut Vec<NodeId>,
) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if matches!(doc.get(id).data, NodeData::Element { .. })
            && needed.is_none_or(|s| s.contains(&id))
        {
            out.push(id);
        }
        stack.extend(flat.children_of(doc, id).iter().rev().copied());
    }
}

/// Runs `f` with the ancestor-bloom of [`crate::counters::walk`] reproduced for `node`.
///
/// [`crate::style::ancestor_filter::prefix_may_match_descendant`] only refutes a prefix when
/// the frame on top of its stack is the node being matched, and the frame stack is built by
/// the sequential walk. Without it every descendant-combinator selector would walk the whole
/// ancestor chain — BUG-1800 slice 5 measured that bloom at 2.6–2.9× the matching cost. The
/// path is rebuilt from [`FlatTree::parent_of`], the same composed parent the walk passes to
/// `push_frame`, so poisoning across a shadow/slot boundary lands on the same nodes.
fn with_ancestor_frames<R>(
    doc: &Document,
    flat: &FlatTree,
    node: NodeId,
    f: impl FnOnce() -> R,
) -> R {
    let mut chain = Vec::new();
    let mut cur = Some(node);
    while let Some(id) = cur {
        if matches!(doc.get(id).data, NodeData::Element { .. }) {
            chain.push(id);
        }
        if id == doc.root() {
            break;
        }
        cur = flat.parent_of(doc, id);
    }
    chain.reverse();
    let guards: Vec<_> = chain
        .into_iter()
        .map(|id| {
            let composed_parent = flat.parent_of(doc, id).unwrap_or(doc.root());
            super::ancestor_filter::push_frame(doc, id, composed_parent)
        })
        .collect();
    let out = f();
    drop(guards);
    out
}


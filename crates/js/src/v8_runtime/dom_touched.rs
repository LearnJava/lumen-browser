//! Трекер DOM-мутаций страницы: [`DomTouched`] (BUG-341 S7), [`DomChanges`]
//! и журнал старых значений `class`/`id` ([`LoggedAttr`], [`ValueLog`], BUG-935).
//!
//! Выделено из `v8_runtime/runtime.rs` без изменений поведения (file-size gate,
//! docs/lint-policy.md §5.1).

use super::*;

/// BUG-341 S7 — outcome of draining the page-side DOM-mutation tracker since
/// the last [`V8JsRuntime::take_dom_touched`] call.
///
/// Feeds [`lumen_layout::style::restyle_root_set_for_node_change`] so the
/// ADR-016 M4 page pipeline (`Lumen::try_relayout_raf_incremental`) can take
/// the incremental-cascade path (`layout_mutation_incremental_restyle`)
/// instead of a full cascade for JS DOM mutations, mirroring
/// `lumen_chrome::bind_model_tracked` (BUG-341 S6) on the chrome side.
#[derive(Debug, Default, Clone)]
pub struct DomTouched {
    /// Nodes whose selector-relevant attribute/class/style value or child
    /// list actually changed via a tracked primitive (`setAttribute`,
    /// `removeAttribute`, `className`/`classList`, inline `style`,
    /// `appendChild`/`removeChild`/`insertBefore`, `textContent`/`innerHTML`).
    pub nodes: HashSet<NodeId>,
    /// `true` when a mutation happened this cycle through a primitive whose
    /// effect on which nodes' selector-relevant state changed cannot be
    /// attributed precisely (`execCommand`, contenteditable text editing,
    /// Selection-driven range deletes, Shadow DOM attachment). When `true`,
    /// `nodes` alone is **not** a safe restyle root-set — the caller must
    /// fall back to a full cascade for this cycle.
    pub unattributed: bool,
    /// BUG-493 (`.sheet` после `appendChild`): монотонный счётчик мутаций DOM
    /// через любой отслеживаемый примитив. В отличие от `nodes`/`unattributed`
    /// не сбрасывается [`V8JsRuntime::take_dom_touched`] — по нему реестр
    /// листов (`sheet_sync::SheetSync`) понимает, что DOM сдвинулся с его
    /// последней сверки, не заводя собственного флага в двадцати трёх нативах.
    pub(crate) epoch: u64,
    /// BUG-1211: per-node touch generation — `node → epoch` at the node's
    /// most recent tracked mutation, populated alongside `nodes` but, like
    /// `epoch` itself, **never cleared** by [`V8JsRuntime::take_dom_touched`]
    /// (a `HashMap` rather than `HashSet` specifically so repeat touches to
    /// the same node update its value instead of being no-ops against a set
    /// that already contains it). Lets a consumer that keeps its own
    /// "basis" watermark (the engine thread's same-tick flush,
    /// `crates/js/src/v8_runtime/style_flush.rs`) ask "what changed since
    /// epoch N" without needing to drain — the drain here is reserved for
    /// the page pipeline's own per-rAF-cycle cadence and draining it a
    /// second time from the flush thread would blind that cycle to
    /// mutations the flush already folded in (see `FlushHandles::
    /// dom_touched`'s doc comment). Without this, a same-tick flush that
    /// only peeks at the ever-growing `nodes` set re-widens its dirty-root
    /// set to everything touched since the page loaded on every single
    /// flush, defeating the whole point of the incremental path for the
    /// read-after-mutate-in-a-loop pattern BUG-1211 is about.
    pub(crate) touch_gen: HashMap<NodeId, u64>,
    /// BUG-1211: `node → epoch` of the node's latest touch that is **not** a
    /// plain attribute write (child-list change, `textContent`, dirty
    /// value/checked…). Such a touch can reach siblings through
    /// `:nth-child`/`:empty`/sibling combinators, so the same-tick flush
    /// widens it to the parent. Never cleared, like [`Self::touch_gen`].
    pub(crate) structural_gen: HashMap<NodeId, u64>,
    /// BUG-935 срез 60: `node → epoch` of the node's latest touch that changed *only its
    /// child list* (`appendChild`/`removeChild`/`insertBefore` on it, `textContent`,
    /// `innerHTML`). Such a touch is not in [`Self::structural_gen`]: the flush restyles
    /// the node and its direct children instead of the parent's whole subtree
    /// (`NodeChange::ChildList`). Never cleared, like [`Self::touch_gen`].
    pub(crate) child_list_gen: HashMap<NodeId, u64>,
    /// BUG-935 срез 89: `node → epoch` of the node's latest child-list touch that no edit
    /// record describes (`innerHTML`, `textContent`, a fragment, a log that outgrew
    /// [`CHILD_EDIT_CAP`]). One after the basis reports the plain `ChildList`. Never cleared.
    pub(crate) child_any_gen: HashMap<NodeId, u64>,
    /// BUG-935 срез 89: `node → [(epoch, front, back)]`, one entry per insertion or removal of
    /// an *element* child through `appendChild`/`insertBefore`/`removeChild`; `front`/`back` —
    /// no element sibling stood before/after the edited node, so the edit could change which
    /// element is first/last. Text nodes are not recorded: no `:first-child` counts them.
    /// Lets the flush say which children a child-list change can have given a different
    /// `:first-child`/`:last-child` answer ([`lumen_layout::style::NodeChange::ChildListEnds`]).
    pub(crate) child_edits: HashMap<NodeId, Vec<(u64, bool, bool)>>,
    /// BUG-1211: `node → attribute name → epoch` of the latest write to that
    /// attribute through `setAttribute`/`removeAttribute`/inline `style`.
    /// Lets the flush ask `restyle_root_set_for_node_change` which selectors
    /// could react to *this* attribute (a `style` write rarely widens at all)
    /// instead of treating every touch as `Unattributed`. Never cleared.
    pub(crate) attr_gen: HashMap<NodeId, HashMap<Box<str>, u64>>,
    /// BUG-935 срез 68: the values `class`/`id` had *before* each recent write, per node —
    /// what lets the flush name the tokens that changed since its basis
    /// ([`Self::value_before`]). Never cleared, like [`Self::attr_gen`]; bounded per
    /// attribute by [`ValueLog`].
    pub(crate) value_log: HashMap<(NodeId, LoggedAttr), ValueLog>,
    /// BUG-935 срез 68: the epoch of the last drain ([`V8JsRuntime::take_dom_touched`]),
    /// which empties [`Self::value_log`] along with the other per-node maps. A basis older
    /// than this has writes the log no longer holds, so [`Self::value_before`] declines.
    pub(crate) log_floor: u64,
    /// BUG-935 срез 81: the epoch of the latest untracked mutation ([`Self::unattributed`]'s
    /// moment). Unlike the flag it survives the drain and is never cleared, so a consumer
    /// with its own watermark asks "was there one since epoch N" — the flag alone, which
    /// nobody drains under the engine thread, stayed set for the page's whole life.
    pub(crate) unattributed_gen: u64,
}

/// BUG-935 срез 89: child-list edits kept per node ([`DomTouched::child_edits`]); a node edited
/// more often than this between two drains falls back to the plain `ChildList`.
pub(crate) const CHILD_EDIT_CAP: usize = 32;

/// BUG-935 срез 81: what [`V8JsRuntime::dom_changes_reader`] reports since its last call.
#[derive(Debug, Default, Clone)]
pub struct DomChanges {
    /// Every node touched since the last read, with what the root-set may assume.
    pub changes: Vec<(NodeId, lumen_layout::style::OwnedNodeChange)>,
    /// An untracked mutation happened since the last read (or this is the first read) —
    /// `changes` is not a safe root-set, the caller recascades everything.
    pub unattributed: bool,
}

/// BUG-935 срез 68: the attributes whose old value [`DomTouched::value_log`] keeps — the
/// two a selector reads by token (`.a`, `#a`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LoggedAttr {
    Class,
    Id,
}

impl LoggedAttr {
    pub(crate) fn of(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("class") {
            Some(Self::Class)
        } else if name.eq_ignore_ascii_case("id") {
            Some(Self::Id)
        } else {
            None
        }
    }
}

/// BUG-935 срез 68: the last few writes to one attribute of one node, each with the value
/// it replaced.
#[derive(Debug, Default, Clone)]
pub(crate) struct ValueLog {
    /// `(epoch of the write, value before it)`, oldest first. `""` stands for "absent":
    /// for a class list or an id the two mean the same thing to a selector.
    writes: Vec<(u64, Box<str>)>,
    /// The newest epoch whose entry was dropped for room. A question about a basis older
    /// than this has lost the entry that would have answered it.
    lost_through: u64,
}

impl ValueLog {
    /// Entries kept per attribute: a node toggled in a loop keeps the last few writes, and
    /// a flush whose basis predates them falls back to the deep path.
    const CAP: usize = 4;

    pub(crate) fn push(&mut self, epoch: u64, before: &str) {
        if self.writes.len() == Self::CAP {
            self.lost_through = self.writes.remove(0).0;
        }
        self.writes.push((epoch, before.into()));
    }

    /// The value at `basis_epoch`: what the first write after it replaced. `None` when no
    /// write after the basis is on record, or when the entry for it was dropped.
    pub(crate) fn value_at(&self, basis_epoch: u64) -> Option<&str> {
        if basis_epoch < self.lost_through {
            return None;
        }
        self.writes.iter().find(|(e, _)| *e > basis_epoch).map(|(_, v)| &**v)
    }

    /// BUG-935 срез 81: every value the attribute held after `basis_epoch` other than the
    /// current one — the values each write after it replaced. `None` when an entry for it
    /// was dropped. For a consumer whose real basis may lie anywhere between `basis_epoch`
    /// and now: the style it holds was computed from one of these values or the current one.
    pub(crate) fn values_since(&self, basis_epoch: u64) -> Option<impl Iterator<Item = &str>> {
        (basis_epoch >= self.lost_through)
            .then(|| self.writes.iter().filter(move |(e, _)| *e > basis_epoch).map(|(_, v)| &**v))
    }
}

impl DomTouched {
    /// BUG-935 срез 68: the value `name` (`class`/`id`) of `node` had when the epoch was
    /// `basis_epoch`, if the log can say.
    pub(crate) fn value_before(&self, node: NodeId, name: &str, basis_epoch: u64) -> Option<&str> {
        if basis_epoch < self.log_floor {
            return None;
        }
        self.value_log.get(&(node, LoggedAttr::of(name)?))?.value_at(basis_epoch)
    }

    /// BUG-935 срез 81: every node touched after `basis_epoch`, with what the root-set may
    /// assume about the touch. Shared by the same-tick flush and the UI thread's on-thread
    /// restyle (which used to report every node as `Unattributed`, so a `class` write on
    /// `<html>` restyled the whole document there while the flush restyled a handful of
    /// nodes). `child_list_narrowing` off reports a child-list touch as `Unattributed`.
    ///
    /// `exact_basis` — the styles being updated were computed at exactly `basis_epoch` (the
    /// flush: its basis is taken under the document lock). Then a `class`/`id` write is
    /// reported once, with its value at the basis. Otherwise (the UI thread reads before the
    /// layout takes the document lock, and an off-thread commit's snapshot is later still) the
    /// real basis lies somewhere after `basis_epoch`, and one `AttrFrom` per value held since
    /// then is reported: a write toggled back (`"" → "a" → ""`) has an empty difference
    /// against its first old value, yet the styles may have been computed with `a`.
    pub fn changes_since(
        &self,
        basis_epoch: u64,
        child_list_narrowing: bool,
        exact_basis: bool,
    ) -> Vec<(NodeId, lumen_layout::style::OwnedNodeChange)> {
        let mut changes = Vec::new();
        for (&n, &touch) in &self.touch_gen {
            if touch <= basis_epoch {
                continue;
            }
            let structural = self.structural_gen.get(&n).copied().unwrap_or(0) > basis_epoch;
            // BUG-935 срез 68: a `class`/`id` write whose value at the basis is on record is
            // reported with it, so the root-set can name the tokens that changed.
            let mut named: Vec<lumen_layout::style::OwnedNodeChange> = Vec::new();
            for (name, _) in self.attr_gen.get(&n).into_iter().flatten().filter(|&(_, &g)| g > basis_epoch) {
                let olds: Option<Vec<&str>> = if exact_basis {
                    self.value_before(n, name, basis_epoch).map(|old| vec![old])
                } else {
                    self.values_since(n, name, basis_epoch)
                };
                match olds {
                    Some(olds) => {
                        let mut seen = HashSet::new();
                        named.extend(olds.into_iter().filter(|old| seen.insert(*old)).map(|old| {
                            lumen_layout::style::OwnedNodeChange::AttrFrom { name: name.clone(), old: old.into() }
                        }));
                    }
                    None => named.push(lumen_layout::style::OwnedNodeChange::Attr(name.clone())),
                }
            }
            // BUG-935 срез 60: a touch that changed only the child list is its own kind of
            // change — the node and its direct children are restyled, not the parent's subtree.
            let child_list = self.child_list_gen.get(&n).copied().unwrap_or(0) > basis_epoch;
            let child_list_any = self.child_any_gen.get(&n).copied().unwrap_or(0) > basis_epoch;
            if structural || (child_list && !child_list_narrowing) || (named.is_empty() && !child_list) {
                changes.push((n, lumen_layout::style::OwnedNodeChange::Unattributed));
            } else {
                if child_list && child_list_any {
                    changes.push((n, lumen_layout::style::OwnedNodeChange::ChildList));
                } else if child_list {
                    let (mut front, mut back, mut edits) = (false, false, 0u32);
                    for &(_, f, b) in self.child_edits.get(&n).into_iter().flatten().filter(|e| e.0 > basis_epoch) {
                        front |= f;
                        back |= b;
                        edits += 1;
                    }
                    changes.push((n, lumen_layout::style::OwnedNodeChange::ChildListEnds { front, back, edits }));
                }
                changes.extend(named.into_iter().map(|c| (n, c)));
            }
        }
        changes
    }

    /// BUG-935 срез 81: [`ValueLog::values_since`] for `name` (`class`/`id`) of `node`.
    fn values_since(&self, node: NodeId, name: &str, basis_epoch: u64) -> Option<Vec<&str>> {
        if basis_epoch < self.log_floor {
            return None;
        }
        let vals: Vec<&str> = self.value_log.get(&(node, LoggedAttr::of(name)?))?.values_since(basis_epoch)?.collect();
        (!vals.is_empty()).then_some(vals)
    }

    /// BUG-935 срез 81: whether an untracked mutation happened after `basis_epoch`.
    pub fn unattributed_since(&self, basis_epoch: u64) -> bool {
        self.unattributed_gen > basis_epoch
    }

    /// The epoch of the drain that started this tracker — for a drained tracker, the basis
    /// of everything it holds.
    pub fn log_floor(&self) -> u64 {
        self.log_floor
    }
}

#[cfg(test)]
mod value_log_tests {
    use super::*;

    fn touched_with(node: NodeId, writes: &[(u64, &str)]) -> DomTouched {
        let mut t = DomTouched::default();
        for &(epoch, before) in writes {
            t.value_log.entry((node, LoggedAttr::Class)).or_default().push(epoch, before);
        }
        t
    }

    #[test]
    fn the_value_at_a_basis_is_what_the_first_later_write_replaced() {
        let n = NodeId::from_raw(3);
        let t = touched_with(n, &[(5, "a"), (9, "a b"), (12, "b")]);
        assert_eq!(t.value_before(n, "class", 4), Some("a"), "basis before every write");
        assert_eq!(t.value_before(n, "class", 5), Some("a b"), "the write at the basis is already in it");
        assert_eq!(t.value_before(n, "class", 10), Some("b"));
        assert_eq!(t.value_before(n, "class", 12), None, "no write after the basis");
        assert_eq!(t.value_before(n, "CLASS", 4), Some("a"), "attribute names compare case-insensitively");
        assert_eq!(t.value_before(n, "style", 4), None, "only class and id are logged");
    }

    #[test]
    fn a_basis_older_than_the_dropped_entries_gets_no_answer() {
        let n = NodeId::from_raw(1);
        let writes: Vec<(u64, &str)> = (1..=6).map(|e| (e * 10, "x")).collect();
        let t = touched_with(n, &writes);
        // Entries for epochs 10 and 20 were dropped for room.
        assert_eq!(t.value_before(n, "class", 5), None);
        assert_eq!(t.value_before(n, "class", 19), None);
        assert_eq!(t.value_before(n, "class", 20), Some("x"));
        assert_eq!(t.value_before(n, "class", 59), Some("x"));
    }

    /// BUG-935 срез 81: a basis that may lie later than the mark gets every value since it.
    #[test]
    fn every_value_since_a_mark_is_listed_unless_one_was_dropped() {
        let n = NodeId::from_raw(4);
        let t = touched_with(n, &[(5, "a"), (9, "a b"), (12, "b")]);
        assert_eq!(t.values_since(n, "class", 4), Some(vec!["a", "a b", "b"]));
        assert_eq!(t.values_since(n, "class", 9), Some(vec!["b"]));
        assert_eq!(t.values_since(n, "class", 12), None, "no write after the mark");
        let writes: Vec<(u64, &str)> = (1..=6).map(|e| (e * 10, "x")).collect();
        let t = touched_with(n, &writes);
        assert_eq!(t.values_since(n, "class", 19), None, "the entry for epoch 20 was dropped");
        assert_eq!(t.values_since(n, "class", 20).map(|v| v.len()), Some(4));
    }

    #[test]
    fn a_drain_makes_older_bases_unknown() {
        let n = NodeId::from_raw(2);
        let mut t = touched_with(n, &[(7, "a")]);
        t.log_floor = 6;
        assert_eq!(t.value_before(n, "class", 5), None, "the basis predates the drain");
        assert_eq!(t.value_before(n, "class", 6), Some("a"));
    }
}

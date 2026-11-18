# ADR-036: Incremental restyle keeps invalidation sets and carried subtrees — the wide road is the boundary, not a gap

## Status

Accepted

## Date

2026-11-17

## Context

BUG-935 closed 101 micro-slices on the same symptom without moving it: a
same-tick flush on a large page re-cascades narrow node sets, but every node in
them pays the full candidate list of the rule index (ebay: 610 k candidates and
1.34 M `matches_complex` per flush; github: 3.77 M candidates for a 1 805–2 075
node pass), and a subtree whose styles differ only in inherited custom
properties is rebuilt box by box. The task [PERF-INVALIDATION](../tasks/PERF-INVALIDATION.md)
replaced the micro-slices with three slices on one mechanism (bug card
[BUG-935](../../bugs/BUG-935-OPEN.md) keeps the history).

Both slice gates are taken on local stands:

- **S1 — invalidation sets.** `style/invalidation.rs` indexes the sheet both
  ways (key → rules, and per-node matched set on `CascadeStyles`); a flush
  recomputes a node from its previous matched set ∪ the rules its changes can
  flip. Stand `flush_walk_cost` (BEM page, 2 591 elements, 1 504 rules):
  candidates 6 464 → 18 per flush (359×), best flush 0.92 → 0.31 ms (34 % of
  HEAD), `recomputed` unchanged.
- **S2 — carried subtrees reach their consumers.** A subtree classified
  `CustomOnly` by the cascade is moved into the new tree (BUG-935 s102) and the
  collector plan now has a licence for it (`custom_only_subtrees`), so it is not
  walked box by box; `ChangedNodes` reads the cascade's classification instead of
  re-comparing full `ComputedStyle`s. Stand `v8_bug935_s101_stand`: 300-flush
  sum 19.4/18.1 → 13.3/10.7 s (59–69 %, gate ≤ 70 %), collectors 17.7 → 1.6 ms
  per flush.

Live confirmation (github.com, dev-release, `--maximized`, `LUMEN_NO_ADBLOCK=1`,
`compat_crawl --only github`, 2026-11-17; numbers in
[journal](../perf/journal.md)): status 200, 0 exceptions, text 3 390–3 420
characters, pixel diff 4.4–6.0 %, 31 flushes / 64.9 s per session (inside the
19–28 flushes / 60–80 s base band of [BUG-1794](../../bugs/BUG-1794-DUPLICATE.md)
slice 3), and under `LUMEN_VERIFY_INCR_CASCADE=1` 11 incremental cascades of
2 080–2 278 styles, **all equal to a full cascade**, 0 divergences.

What the live run also showed, and what this ADR has to state plainly: **the S1
narrow hand is inert on github.** `NodeRestyleIndex::invalidated_rules` returns
`None` when the document has an author shadow root
(`crates/engine/layout/src/style/restyle.rs:1846`), because the index scans the
document sheet only — shadow-tree sheets are not in it. github ships Primer
components, so every flush takes the wide road and re-matches the full candidate
list; its cost is the cascade itself (`CustomFlow` cloning `ComputedStyle` per
descendant, 21–25 ms under load) and deep root sets from the `:has()`-reach and
shadow guard, both gated classes that were rolled back twice in BUG-1800
(slices 12/13). The S1 win is real where the licence is issued: no author shadow
root, attribute deltas with an old value, no sheet change.

## Decision

Keep both mechanisms as part of the default incremental path, and treat the
**wide road as the contract's boundary**, not as an unfinished branch:

1. A flush recomputes a node from its recorded matched set ∪ invalidation sets
   when the document/sheet pair licenses it; the matched set is a superset of
   what the matcher can return, so the style is byte-identical by construction.
2. A `CustomOnly` subtree is carried whole, and every consumer of the previous
   pass's output reads the cascade's classification instead of re-deriving it
   (plan licences, change record). This is now a rule of the incremental path,
   not a property of one collector.
3. The wide road is taken whenever the licence cannot be established:
   structural and unattributed changes, `class`/`id` writes without an old
   value, a changed stylesheet revision, deep/fanned roots, and any document
   with an author shadow root. It stays byte-for-byte the pre-S1 path.
4. Nothing new is built for the github/shadow-DOM class without a fresh
   decision. Its remaining levers are named, gated and measured as empty or
   gated: `has()`-reach differential and lifting the conservative shadow mode
   (BUG-1800 slices 12/13/16), and the cascade's per-descendant clone
   (`CustomFlow`) — the largest single item left on the s101 stand.

BUG-935 stays `OPEN (DEBTOR)`: this mechanism removes the *rule-list* and
*collector* costs it measured, but the shadow-DOM/deep-root storm that makes
github slow is untouched. The bug card carries the explicit boundary.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep slicing BUG-935 for another lever | 101 slices moved the flush cost but never the symptom; the remaining items are architectural (this task) or gated two-rollback classes |
| License the narrow hand in shadow documents by scanning `:host` as "possible" | The index would have to be re-built per shadow sheet and per revision; a sound superset is not expressible without scanning them, and an unsound one is visible corruption |
| Force deep roots narrow (remove `conservative`) | Rolled back twice before the mechanism existed (BUG-1794 slice 5, BUG-1800 slices 12/13); the `:has()` flip question makes the reuse licence refuse under every candidate anyway (BUG-1800 slice 16) |
| Make the S1 path unconditional (drop the shadow-revision guard) | Stale rules against a new sheet are wrong styles, not slow frames; `sheet_revision` is what makes the set a superset |
| Publish `--*` from the cascade instead of refreshing carried box styles (S2's other option) | Already decided by BUG-935 s102: refreshing the carried styles is one pass over the carried boxes and keeps one source of truth |

## Consequences

- **Positive:** the flush pays for what its delta can flip and for the boxes it
  actually rebuilt; the wide road keeps every unlicensed case exactly as before,
  so the worst case is the pre-slice path. New collectors and post-build passes
  have a stated rule to follow instead of each re-deriving freshness.
- **Negative / trade-offs:** a per-node `Arc<[MatchedRule]>` lives next to every
  cascaded node (16 bytes per matched rule, rules are shared), and the index is
  built once per sheet revision. The mechanism is invisible on pages with author
  shadow roots, which today are the heaviest pages (github, cnn) — the reported
  readiness on those sites is unchanged, and the ADR says so rather than
  implying a fix.
- **Future:** revisit when a measured live scenario is blocked by the boundary —
  the ready-made trigger is scanning author shadow-root sheets into the
  invalidation index (allowed if it stays a sound superset, e.g. `:host`/
  `::slotted` compounds indexed per sheet revision), or a new class for the
  cascade's per-descendant `ComputedStyle` clone. Either starts as a new task
  with its own stand and a fresh user decision; BUG-935 is closed only when the
  github/cnn readiness symptom moves. That task is
  [PERF-WIDE-ROAD](../tasks/PERF-WIDE-ROAD.md) (P6, 2026-11-18): it owns the
  remainder, and its S1 must name one of the two candidates with numbers or
  close the class as WONTFIX.

## Addendum (2026-11-18): both candidates measured and closed by number — the class is WONTFIX (Phase 3+)

PERF-WIDE-ROAD S1 ran the census this ADR demanded (github ×4 live compat-crawl
runs, cnn trace-nav/dump-layout with a stack sampler; numbers —
[journal](../perf/journal.md) §2026-11-18, [BUG-1800](../../bugs/BUG-1800-OPEN.md)
§Срез 18). Neither candidate is a dominant article of the flush:

- **The per-descendant clone class is dead by number.** `CustomFlow` never runs
  on shadow-root pages at all (`inherited = 0` in every run — the
  `has_author_shadow_roots` guard, `custom_flow.rs:171`), so the "clone per
  descendant" this ADR attributed to github existed only on the s101 stand.
  The generalized per-node construction (`ComputedStyle::inheriting`, `cs_init`)
  measures **0.5–1.2 %** of the flush on both sites.
- **Shadow-sheet indexing addresses a sub-dominant article**: document-sheet
  wide matching is ≈ 25–31 % of the github flush (honest, uninstrumented) and
  15.6 % on cnn — zeroing it entirely leaves ≥ 60 % of the cost.
- The largest measured articles are *outside* both candidates: github's
  `flush.custom_props` collector at 20–31 % (the scoped licence requires an
  empty `cq_boxless`, and github uses `@container`), cnn's per-element
  `::before`/`::after` probe (`quote_pseudos`) at ~38 % (detail-inflated).
  The shared amplifier on both sites is the recompute volume itself
  (`forced_same` 95.6–99.4 % — document-wide roots), the class §Alternatives
  rejected with two rollbacks.

Per the task's gate, no mechanism is built; BUG-935, BUG-1778 and BUG-1800 move
to `WONTFIX (Phase 3+)` with these numbers. The wide road remains the
contract's boundary; reopening takes a Phase 3+ decision, and the census names
where the next percent actually lives (the pseudo-element probe, the
`custom_props` scoped licence under non-empty `cq_boxless`).

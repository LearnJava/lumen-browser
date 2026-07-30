# ADR-025: identity propagation — `BoxOrigin` through layout, display list and compositor

## Status

Accepted (2026-07-30). This is a technical contract, not a policy question — it
needed review rather than a user decision, and leaving it `Proposed` would stall
`DEVX-7` on a review with no reviewer. It must land **before** `DEVX-7`, because
`DEVX-7` builds an index keyed on identity and the current key is wrong.

**Scope of what is fixed here.** Three rules are binding and changing any of them
needs a new ADR:

1. absence of a DOM origin is expressed by `Option`, never by a sentinel value;
2. identity is the pair `(node, role)`, never `node` alone;
3. provenance lives in a side index, not as a field on `DisplayCommand`.

Everything else — the exact `BoxRole` variants, field names, the span struct's
layout — may be refined while implementing `DEVX-7` without a new ADR, as long as
the three rules hold. Record such refinements in the `DEVX-7` commit body.

## Date

2026-07-30

## Context

Every diagnostic capability discussed for the `DEVX-7…16` track — `explain_element`,
`explain_page`, structural tree diff, subtree-scoped control, and any future
"which element caused this repaint" answer — reduces to one question:

> given a paint command, which thing produced it?

Today that question is unanswerable, and the reason is not a missing field. It is
that **identity in the layout tree is already overloaded three ways**, so a naive
provenance index would silently attribute paint to the wrong element rather than
report "unknown". Verified against the code on 2026-07-30:

1. **`LayoutBox.node` is not optional.** `crates/engine/layout/src/box_tree.rs:2006`
   declares `pub struct LayoutBox { pub node: NodeId, … }`. Every box must name a
   DOM node, including boxes that have none.

2. **Anonymous boxes reuse the parent's id.** `anon_inline_run(node: NodeId, …)`
   (`box_tree.rs:3691`) and `anon_inline_block_row(node: NodeId, …)`
   (`box_tree.rs:3817`) take an id from their caller, and the call site passes
   `node: parent_id` (`box_tree.rs:4164`). Paint emitted by an anonymous box is
   therefore indistinguishable from paint emitted by its parent element.

3. **The "no DOM origin" sentinel collides with the document root.** Generated
   content is tagged `source_node == NodeId::from_index(0)`, documented as
   *"no DOM origin"* at `box_tree.rs:1900-1906` and `box_tree.rs:4308`. But index
   0 **is** the document root — stated in a test comment in the paint crate:
   *"document root is 0; img elements get a non-zero id"*
   (`crates/engine/paint/src/display_list.rs:10979`). "Synthetic" and "the root
   element" are the same value.

4. **Pseudo-elements have no identity at all.** `::first-letter` is a flag on an
   inline segment — `pub pseudo_kind: PseudoKind` (`box_tree.rs:2078-2082`) — not
   a distinct id. Its paint is attributed to the text node's box.

5. **The display list carries almost no identity to begin with.** Of the ~40
   `DisplayCommand` variants (`display_list.rs:330`) only `LazyImageSlot` and the
   canvas slots carry `node_id` (`display_list.rs:460-464`, `6990`, `7842`), and
   on the inline path even that is a placeholder with an explicit admission in the
   code: *"node_id unavailable in InlineFrag (no box reference); use 0 as sentinel"*
   (`display_list.rs:3623-3627`).

On top of the overloading, the relation is many-to-one twice over: one DOM node
produces many boxes (anonymous, pseudo, list markers, table fixup), and one box
produces many fragments (line boxes, column and page breaks). So
`command → node` is not a function, and identity cannot be a scalar — it is a
tree of origins.

This is why the earlier framing ("write down what `NodeId`/`BoxId` mean") is not
enough. There is no existing consistent convention to write down; the convention
has to be fixed first.

## Decision

### 1. Identity is a pair, and absence is expressible

```rust
/// Where a layout box came from. Replaces the bare `NodeId` as the identity of
/// a box for all introspection purposes.
pub struct BoxOrigin {
    /// The DOM node this box belongs to, or `None` for boxes with no DOM
    /// origin (anonymous boxes, generated content). Never a sentinel value.
    pub node: Option<NodeId>,
    /// Why this box exists — disambiguates the many boxes one node can produce.
    pub role: BoxRole,
}

pub enum BoxRole {
    /// The principal box of an element.
    Element,
    /// Anonymous block wrapper (CSS 2.1 §9.2.1.1). `node` is the *containing*
    /// element, and this role is what makes it distinguishable from it.
    AnonymousBlock,
    /// Anonymous inline run (`anon_inline_run`).
    AnonymousInlineRun,
    /// Pseudo-element box or segment (`::before`, `::after`, `::first-letter`,
    /// `::first-line`, `::marker`).
    Pseudo(PseudoKind),
    /// List marker box.
    ListMarker,
    /// `content:` generated content with no DOM text node.
    GeneratedContent,
    /// Box synthesised by table fixup (CSS 2.1 §17.2.1).
    TableFixup,
}
```

Rules:

- **`Option<NodeId>` replaces the `NodeId::from_index(0)` sentinel.** Absence
  becomes a type-level fact instead of a magic value that aliases the root.
- **`(node, role)` is the identity, never `node` alone.** Any introspection
  output that names an element must carry both, so an anonymous wrapper is never
  reported as its parent.
- `LayoutBox.node` stays for the hot layout paths that legitimately want "which
  element's style applies here"; `BoxOrigin` is what introspection reads.

### 2. Who creates identity, who must preserve it, when it may vanish

- **Created** in `box_tree.rs` at box construction — the only place boxes are
  made, including every `anon_*` helper and every fixup path.
- **Preserved** by every pass that clones, grafts or reorders boxes. The one that
  matters today is `incremental::graft_geometry` (`crates/engine/layout/src/incremental.rs`,
  2976 lines, wired into `box_tree.rs`): a grafted box keeps the origin of the
  node it represents, not of the box it was copied from.
- **May be absent** only as `node: None` with a role that explains why. There is
  no other legal way to not know.
- **Fragments inherit** their box's origin plus a fragment index. One box → many
  fragments is expected and must not be flattened.

### 3. Provenance in the display list is a side index, not a field

```rust
pub struct ProvenanceIndex {
    spans: Vec<ProvenanceSpan>,
}

pub struct ProvenanceSpan {
    /// Half-open range into `DisplayList::commands`.
    pub range: Range<usize>,
    pub origin: BoxOrigin,
    /// Fragment index within the box (line box / column / page break).
    pub fragment: u32,
    /// Depth in the clip stack at span open — pairs with the existing
    /// `PushClipRect`/`PushClipRoundedRect`/`PushClipPath` → `PopClip` markers.
    pub clip_depth: u16,
}
```

- `DisplayCommand` is **not** modified. Its size and layout stay exactly as they
  are, because the display list is rebuilt every frame and the enum already has
  ~40 variants.
- Spans are opened and closed during emission by the same push/pop discipline the
  clip stack already uses (`display_list.rs:548-572`), so the mechanism is not new
  to the emitter.
- The existing per-variant `node_id` on `LazyImageSlot` / canvas slots stays as
  is — it serves the shell's bitmap registry (`canvas:{node_id}`), which is a
  different consumer with a different lifetime. It is not the provenance path and
  must not be conflated with it.

### 4. Invariants (checked in debug builds; the `DEVX-8b` half of the invariant layer)

- Every emitted command falls inside exactly one span.
- Every span's `origin` resolves: `node: Some(id)` refers to a live DOM node;
  `node: None` has a role that permits absence.
- Span nesting is balanced, and its depth agrees with the clip stack depth.
- Every layout box with a visible background or border has at least one span.
- Command order within a stacking context agrees with the stacking tree.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Add a `node_id` field to `DisplayCommand` | ~40 variants, list rebuilt every frame; inflates the hot path for a diagnostic need. It also cannot express fragments or roles — it would reproduce exactly the ambiguity this ADR exists to remove. |
| A new `RenderArtifact` IR between layout and paint (proposed in the 2026-07-30 discussion) | Conceptually cleaner, but it means rewriting the recursive emitter inside an 18605-line file, with a mandatory ~20-minute graphic run plus CPU-snapshot regeneration on every iteration. The side index gets the same decoupling — `DisplayCommand` untouched, layout not coupled to paint — at a fraction of the risk. Revisit only if a second consumer needs a real IR. |
| Keep the parent-id reuse and document it as a known caveat | Provenance would be confidently wrong on almost every page: anonymous boxes are ubiquitous. A diagnostic that lies is worse than one that abstains, and the whole point of the track is to stop guessing. |
| `NodeId` + a separate `is_anonymous: bool` | Insufficient: it distinguishes two cases where there are at least seven, and still cannot express pseudo-elements or table fixup. |
| Assign fresh synthetic `NodeId`s to anonymous boxes | Makes them addressable but breaks the "id ↔ DOM node" meaning everywhere else in the engine, including selector matching and event targeting. A separate role field keeps the DOM id honest. |

## Consequences

- **Positive:** `explain_element` can answer the paint half of its chain at all,
  and answer it correctly on pages with anonymous boxes. Structural diff can say
  "`div.news-feed` stopped painting" instead of "command #417 changed".
- **Positive:** the `0`-sentinel collision with the document root is removed —
  a latent correctness hazard independent of introspection.
- **Negative:** this touches `box_tree.rs` construction paths, so **it can move
  pixels** (anonymous boxes, `::first-letter`). The full graphic-test run plus
  deterministic CPU-snapshot regeneration is mandatory in the same commit — see
  `CLAUDE.md` §Graphic tests. Unlike `DEVX-7` itself, display-list neutrality
  cannot be claimed here.
- **Negative:** two ways to ask "which node" (`LayoutBox.node` for style,
  `BoxOrigin` for identity) until the former is audited. That audit is
  deliberately out of scope — doing it here would couple an identity contract to
  a refactor of the hot layout path.
- **Future:** revisit if a real intermediate IR becomes justified by a second
  consumer, or if BUG-341 (paused) resumes and changes the grafting rules.

## Related

- [ADR-024](ADR-024-introspection-api-levels.md) — the levels that decide what
  part of this becomes wire-visible and when.
- [`docs/tasks/p1-introspection-track.md`](../tasks/p1-introspection-track.md) —
  `DEVX-7` (provenance) and `DEVX-8b` (paint invariants) implement this ADR.
- BUG-341 (paused by user decision 2026-07-28) — `incremental.rs` is the pass
  with the strongest preservation obligation; this ADR does not resume it.

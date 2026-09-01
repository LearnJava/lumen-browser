# ADR-028: Vendoring applies per sub-decision, not per subsystem

## Status

Accepted

## Date

2026-09-01

## Context

[ADR-027](ADR-027-own-vs-vendored-boundary.md) (2026-08-31) replaced a
name-based never-take list with a decision-ownership test — *if we implement
this ourselves and disagree with the reference implementation, are we wrong by
definition?* — and predicted, in its own Consequences section, that applying
the test to text shaping, SVG and URL would make three things "reachable":
OpenType features via `rustybuzz`, "SVG gradients, masks and filters" via
`resvg`/`usvg`, and WHATWG-conformant URL parsing via `url`.

The `LIB` track executed all three replacements between 2026-08-31 and
2026-09-01 (`LIB-0`…`LIB-10`, see `ROADMAP.md`). Text shaping landed exactly as
ADR-027 anticipated: `LIB-1` added `RustybuzzShaper` behind the existing
`TextShaper` trait-anchor, `LIB-2` flipped the production default, `LIB-3`
deleted the own `gsub.rs`/`gpos.rs`/`otlayout.rs`/`shape.rs` (~1350 lines)
outright. One implementation, no residue.

SVG and URL did not. Both tasks found, only after starting implementation,
that "take the vendored crate" was not a single atomic swap for the whole
subsystem — each subsystem split into a part a committee's spec genuinely
owns and a part that is a Lumen product decision, and only the first part
could be vendored:

- **SVG (`LIB-4`, `LIB-5`, `LIB-9`).** `resvg`/`usvg` 0.44 fully replaced the
  own SVG-to-DisplayList path, but **only** for the external-image entry point
  (`<img src=*.svg>`, `background-image: url(*.svg)` — `lumen_image::decode_svg`).
  Inline, DOM-live `<svg>` could not use `usvg` at all: its public types
  (`Tree`, `Path`, `Fill`, `Stroke`, …) are `pub(crate)` outside `Tree::from_str`
  on raw XML text, which is unusable for a tree that must stay wired to Lumen's
  own DOM, cascade and hit-testing. `LIB-5` and `LIB-9` instead hand-built a
  *parallel own-code* paint model (`SvgPaint::Gradient`/`SvgGradientDef`,
  `resolve_svg_mask` in `box_tree/svg.rs`) that borrows the SVG spec's
  *concepts* — it is not vendored code, and it covers less than ADR-027's
  "gradients, masks and filters become reachable" implied: only
  `linearGradient`/`radialGradient` for inline fill/stroke (`LIB-5`) and
  `<mask>` (`LIB-9`); `clipPath`, `pattern`, `marker` and `filter` on inline
  SVG remain entirely unimplemented, not merely unreached.
- **URL (`LIB-6`).** `lumen_core::url::Url` now parses scheme/host/path/query/
  fragment through the `url` crate (conformance against `urltestdata.json`:
  41.95% → 81.86%). But `Url::host()` was **deliberately kept as own code
  permanently** — the address bar's IDN-spoof guard
  (`crates/shell/src/address_bar.rs::guard_display_text`) needs the string the
  user actually typed, not a WHATWG-normalized one, and that is a decision
  about what Lumen shows the user, which ADR-027's own criterion assigns to
  "ours."

In both cases the ADR-027 test was applied correctly — it is just that it
applies to a narrower unit than "SVG rendering" or "URL parsing." Neither
ADR-027's worked examples nor its Consequences section say this; a reader
applying it subsystem-by-subsystem, as ADR-027 itself is phrased, would expect
a full swap and be surprised by what actually shipped.

A second, unrelated discovery from the same track: `LIB-9` needed
`PushMaskLayer`/`PopMaskLayer`, infrastructure a prior investigation
([`subsystems/paint.md`](../../subsystems/paint.md), BUG-272 slice 15,
2026-07-19) had read as already implemented in all three paint backends. It
was not — femtovg's handler was scissor-only and `cpu_raster.rs` had no
handler at all (`_ => {}`); only the wgpu path had a real (if never-exercised)
implementation, which turned out to have its own live bug once actually
driven ([BUG-936](../../bugs/BUG-936-OPEN.md)). "Infrastructure already
exists" was a premise nobody had verified by running it end to end.

## Decision

Extend ADR-027's test with two operating rules, both learned from `LIB-5`,
`LIB-6` and `LIB-9`, not from theory:

1. **Apply the decision-ownership test per sub-decision, not per subsystem.**
   Before starting a vendoring task, enumerate the subsystem's sub-decisions
   (parse vs. display, external vs. inline, kernel vs. policy) and run the
   ADR-027 test on each separately. Expect — do not treat it as a scope
   failure — that some sub-decisions vendor cleanly and others stay ours in
   the same task. Record the split explicitly in the task's ROADMAP.md row and
   in `docs/plan/tech-stack.md` §5, rather than letting the row imply a full
   subsystem swap it did not do.
2. **A "this infrastructure already exists" premise requires a runtime check,
   not a source read, before it can gate or unblock a task.** `LIB-9` cost an
   extra investigation cycle because slice 15's 2026-07-19 finding was true of
   the code as *written* (a real `MaskLayerComposite` existed in
   `renderer.rs`) but false of the code as *exercised* (nothing had ever
   called it, so its two sibling backends' stub status and its own latent bug
   were both invisible to static reading). Before relying on "X already
   works" to scope a task down, run X, even in the crudest probe.

This does not change ADR-027's criterion itself, which held up: `host()`
staying own code is the criterion applied correctly to a narrower unit, not an
exception to it.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Treat the SVG/URL partial replacements as ADR-027 failures and look for an all-or-nothing vendoring path (e.g. fork `usvg` to expose its tree, or vendor `host()` too) | Forking `usvg` trades a public-API problem for a maintenance burden ADR-027 exists to avoid; vendoring `host()` would let a WHATWG-normalized host diverge from user-typed text, reopening the exact spoofing surface `guard_display_text` was written to close — the split is the correct outcome, not a workaround |
| Leave ADR-027 as the only record and fold this into the LIB task rows | The task rows (`LIB-5`, `LIB-6`, `LIB-9` in `ROADMAP.md`) record *what* shipped; this ADR records the *reusable rule* — that a subsystem-shaped vendoring task should be expected to decompose, and that "already implemented" claims need a runtime check — which the next vendoring task (icu4x, per §5's provisional-accelerator note) will need again |
| Roll this into a revision of ADR-027 itself | ADR-027's criterion is unchanged and still correct; this is a refinement of how to *apply* it, discovered by executing the very tasks ADR-027 authorized. Keeping it a separate ADR preserves ADR-027's original reasoning and measurements as they were verified at the time |

## Consequences

- **Positive:** the next vendoring candidate (Bidi/line-breaking/segmentation/
  normalization via `icu4x`, flagged provisional in `docs/plan/tech-stack.md`
  §5) can be scoped correctly up front instead of rediscovering the
  per-sub-decision split mid-task.
- **Positive:** `docs/plan/tech-stack.md` §5's "Покинули этот список" table now
  distinguishes what actually vendored (external SVG, URL parse/serialize)
  from what stayed own code by decision, not by omission (`host()`, inline SVG
  filter/pattern/marker/clipPath).
- **Negative / trade-off:** a "done" vendoring task's ROADMAP row is longer and
  more qualified than a clean subsystem swap would be — accepted, since the
  alternative is a row that overclaims coverage (as `LIB-5`'s original scope
  read before this ADR, per `subsystems/font.md`/`CAPABILITIES.md`'s LIB-8
  documentation pass).
- **Future:** if a later task manages to vendor `usvg`'s tree for inline SVG
  (e.g. a future `usvg` release exposes a public construction API) or finds a
  display-safe way to normalize `host()`, that closes the gap this ADR
  documents — re-open the relevant LIB row rather than filing a new ADR.

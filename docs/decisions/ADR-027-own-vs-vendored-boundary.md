# ADR-027: The own-vs-vendored boundary is decision ownership, not subsystem name

## Status

Accepted

## Date

2026-08-31

## Context

[`docs/plan/tech-stack.md`](../plan/tech-stack.md) §5 draws the boundary between
our code and third-party crates by **naming subsystems**: a fixed list of things
we always write ourselves, and a fixed list of crates we never take. That list
was written at the start of the project, before most of the engine existed.

Three things have since made it unusable as a decision rule.

**1. The list is factually wrong in three places.** It is not a policy that is
being violated — it is a description of the code that no longer matches it:

| §5 claims | Reality |
|---|---|
| «PNG-декодер … готов в `lumen-image` + свой DEFLATE» | [`crates/engine/image/src/png/mod.rs`](../../crates/engine/image/src/png/mod.rs) is a 125-line wrapper over `zune_png` |
| «свой DEFLATE, переиспользуемый для HTTP gzip/deflate» | [`crates/network/src/flate.rs`](../../crates/network/src/flate.rs) is `flate2`; WOFF1 uses `miniz_oxide` |
| «~~`tiny-skia`~~ → свой 2D-растеризатор» (never-take list) | `tiny-skia = "0.11"` is a dependency of `lumen-paint`, backing the `cpu-render` backend |

JPEG, WebP and AVIF are wrappers too. The migration this ADR describes has
already happened for image codecs — quietly, with no ill effect, and without
anyone noticing that the policy said otherwise.

**2. Where the list held, it produced stubs that do not advance.** Measured
against the code on 2026-08-31:

| Subsystem | State | Comparable crate |
|---|---|---|
| Text shaping ([`shape.rs`](../../crates/engine/font/src/shape.rs), [`gsub.rs`](../../crates/engine/font/src/gsub.rs), [`gpos.rs`](../../crates/engine/font/src/gpos.rs), [`otlayout.rs`](../../crates/engine/font/src/otlayout.rs)) | ~1 350 lines. GSUB lookup types **1 and 4 of 8**; GPOS **1 and 2 of 9**. No GPOS 4 (mark-to-base) → diacritics do not attach. No contextual lookups. The module doc states outright: «complex scripts are out of scope» | `rustybuzz` ≈ 25–30k lines, pure Rust, no C |
| SVG render path ([`box_tree/svg.rs`](../../crates/engine/layout/src/box_tree/svg.rs), [`svg_path.rs`](../../crates/engine/paint/src/svg_path.rs)) | 14 elements. **Zero** occurrences of `linearGradient`, `radialGradient` or `url(#…)` — gradients exist only as DOM objects in the JS layer and are never painted. No `clipPath`, `mask`, `filter`, `pattern`, `marker`, `<image>` | `resvg`/`usvg` — static SVG 1.1 near-complete |
| URL ([`core/src/url.rs`](../../crates/core/src/url.rs)) | ~370 lines of implementation. No WHATWG state machine, no percent-encoding sets | `url` ≈ 10k lines, validated against `urltestdata.json` |

Shaping is not unfinished work in progress: `U-2` («шейпинг текста GSUB/GPOS»)
is marked **done** in `ROADMAP.md`. The list said «write it ourselves», the task
closed, and the result covers Latin and Cyrillic ligatures plus kerning.

**3. The project's largest vendoring decision was correct.** V8 replaced the
JS engine wholesale ([ADR-018](ADR-018-v8-cutover.md)); nobody proposes
reversing it. `wgpu`, `rustls`, SQLite, `icu4x` are the same shape. The rule
that would have forbidden them is the rule §5 states.

The failure mode of a name-based list is that it cannot answer the question it
is asked. «Is a shaper ours?» has no principled answer. «Who decides what a
correct answer is here?» does.

## Decision

Replace the name-based boundary with a criterion about **who owns the
decision**:

> **Ours** — wherever *we* decide what correct means: how to lay out a box, when
> to invalidate a style, what to paint and in what order, how a nested browsing
> context behaves, what the browser shows the user, what it refuses to send.
>
> **Vendored** — wherever a committee already decided and wrote it down: a file
> format, a Unicode table, an OpenType lookup, a URL parsing state machine, a
> compression algorithm, a cryptographic primitive.

The test is not «is this hard» or «is this core». It is: *if we implement this
ourselves and disagree with the reference implementation, are we right or are we
wrong?* If we are wrong by definition, the spec owns the decision and we should
take the implementation.

Consequences of applying the criterion to the current never-take list:

**Leaves the list (may now be vendored):** `rustybuzz` (and `ttf-parser`, which
it carries), `resvg`/`usvg`, `url`. Each still requires a trait-anchor in
`lumen-core::ext` and per-dependency justification in the commit body — those
rules are unchanged.

**Stays on the list permanently — this is the engine:** `html5ever`,
`cssparser` + `selectors`, `stylo`, `taffy`, `hyper`, `hickory-resolver`,
`encoding_rs`, `adblock`, `readability`, `tokio`, `egui`/`iced`/`Slint`. These
are places where Lumen decides, and taking them is what would make the project a
wrapper around someone else's engine.

The rewritten §5 is the normative text; this ADR is the reasoning behind it.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep §5 as written | It is already false in three places and is being cited to block decisions on a premise the code has disproved. [`docs/tasks/rp5-external-svg-images.md`](../tasks/rp5-external-svg-images.md) rejected `resvg` on the grounds that «движок уже рисует SVG-контуры/градиенты/`<use>`» — the gradient half of that sentence was never true |
| Keep the boundary but move individual crates case by case | That is what has been happening: PNG, DEFLATE and `tiny-skia` moved without the list being updated. Without a criterion the list drifts out of sync again |
| Write everything ourselves, including a full shaper | Measured cost: a completed task (`U-2`) yielded 2 of 8 GSUB and 2 of 9 GPOS lookup types. Reaching parity is years of work on data tables that carry no Lumen-specific decision |
| Take a whole engine (Servo components, Chromium) | Discards ~420k lines of working own pipeline — network, HTML parser, DOM, CSS cascade, layout, paint — and with it the reason the project exists |

## Consequences

- **Positive:** the three measured stubs can be replaced with implementations
  that are complete by construction. Diacritics, Arabic, Indic scripts and
  OpenType features become reachable; SVG gradients, masks and filters become
  reachable; URL conformance becomes measurable against the official corpus.
- **Positive:** the policy stops contradicting the code, so it can be cited
  again without first checking whether it is true.
- **Negative:** `rustybuzz` brings its own font parser (`ttf-parser`), so the
  tree carries two. Accepted deliberately; the cost is table-parsing memory,
  measured in `LIB-1`.
- **Negative:** replacing the shaper moves glyphs by sub-pixel amounts on every
  page with text, invalidating three independent golden sets (graphic tests,
  deterministic CPU snapshots, textual display-list `.snap`). `LIB-2` exists
  solely to pay that cost in one place.
- **Negative:** dependency surface and compile time grow. Both are bounded:
  `tiny-skia` (which `resvg` renders through) is already in the tree.
- **Future:** the annual provisional review in §5 stays. A vendored crate
  returns to «ours» only when it starts blocking a decision we need to own —
  not because it became feasible to reimplement.

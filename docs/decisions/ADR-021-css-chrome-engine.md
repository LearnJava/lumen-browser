# ADR-021: Browser chrome rendered by Lumen's own engine (Variant A of ADR-015), no JS bridge

## Status

Accepted

## Date

2026-07-24

## Context

ADR-015 introduced the `ChromeView` boundary and deliberately deferred the native-vs-web choice for
the browser chrome, listing "collect evidence during Phase 3, then pick one / keep both / go hybrid"
as the path forward. That evidence surfaced earlier than planned, from a different direction: the
design-system track (DS-1…DS-19, `docs/tasks/p1-design-v3.md`) spent 19 slices hand-translating a
single HTML/CSS reference mock-up (`docs/design/lumen-v3_3.html`, ~700 lines CSS / ~350–400 DOM
elements) into Rust — `toolbar.rs`/`tabs/strip.rs`/`panels/*` building `Vec<DisplayCommand>` by hand,
colours mirrored from the mock-up's CSS custom properties into a parallel Rust `Palette`
([panels/themes.rs:184](../../crates/shell/src/panels/themes.rs)), hit-test as separate coordinate
math off `CHROME_H` ([toolbar.rs:39](../../crates/shell/src/toolbar.rs)). Every future mock-up
revision repeats this translation tax, and the two representations (CSS source of truth, Rust
implementation) can silently drift.

Since ADR-015, `lumen-layout`/`lumen-paint` gained essentially everything the mock-up needs: flex,
grid (`repeat()`/`minmax()`/`auto-fill`), `var()`, `calc()`, transitions, `@keyframes`, `transform`,
`backdrop-filter`, `box-shadow`, gradients, `::before/::after` with `attr()`,
`:hover/:focus/:active/:focus-within`, an incremental relayout path
(`layout_mutation_incremental`, [box_tree.rs:2728](../../crates/engine/layout/src/box_tree.rs)), and a
generic hit-test returning node/ancestor-path/cursor/`user-select`
([hit_test.rs:77](../../crates/engine/paint/src/hit_test.rs)). Full gap analysis in
[docs/tasks/p1-css-chrome.md](../tasks/p1-css-chrome.md) §2 found only cosmetic gaps
(non-standard `::-webkit-scrollbar*`, `resize: vertical` drag-UI, sticky scroll-follow) — closed by
this track's own CSS slices rather than blocking it.

This makes running the mock-up's actual HTML/CSS through the real
`html-parser → css-parser → layout → paint` pipeline — instead of transcribing it — both newly
feasible and directly dogfooding-valuable: every rendering gap the chrome hits is a real engine bug a
page author would hit too.

## Decision

Adopt **Variant A of ADR-015 ("Native chrome")**: the browser chrome is not a hand-built Rust display
list *or* a JS-driven bundle — it is `assets/chrome/{chrome.html,chrome.css}` (produced from the
frozen mock-up by `scripts/gen_chrome_assets.py`) parsed and painted by Lumen's own engine, with a
**Rust-only, JS-free** binding layer (`ChromeModel` mutates the parsed chrome DOM's text/attributes/
template clones directly — no privileged JS↔Rust bridge, no JS runtime involved in chrome rendering
at all). This resolves ADR-015's deferred A-vs-B choice for the currently-shipping mock-up-driven
interface: **A**, not B — dogfooding the engine does not require a JS bridge, and skipping one removes
ADR-015's named attack surface and per-frame-JS-execution cost entirely. ADR-015's `ChromeView`
trait/`BrowserController` contract, and the option for plugins or a future literal `WebChrome` (JS
bundle) backend to reuse it, are unaffected — this decision picks the shipping default, not a final
closure of the trait design space.

Compile-time / runtime split (full design: [docs/tasks/p1-css-chrome.md](../tasks/p1-css-chrome.md)):
`lumen-chrome`'s `build.rs` parse-gates the assets (bad CSS/unknown property = build error, `-webkit-*`
allowlisted) and codegens typed id/`ChromeAction`/template-registry lookups so stringly-typed chrome
references fail at compile time, not at runtime. At runtime the chrome `Document`+`Stylesheet` are
parsed once at startup; each frame/mutation drives the same restyle → (incremental) relayout → paint
pipeline pages use.

### Flag strategy (modelled on ADR-018's V8 cutover)

| Stage | Mechanism |
|---|---|
| Opt-in development (CC-1…CC-13) | `LUMEN_CSS_CHROME=1` env var; default window behaviour unchanged pixel-for-pixel; engine chrome and legacy panels may co-render (painter's order in `overlay_buf` already supports layering) |
| Parity checklist (CC-14) | Explicit day-to-day scenario list (navigation, tabs, panels, themes, DPI/zoom, split view) verified under the flag |
| Default flip (CC-14) | `LUMEN_CSS_CHROME` default flips to on; `LUMEN_LEGACY_CHROME=1` becomes the rollback opt-out — same shape as ADR-018's `--features quickjs` rollback window |
| Legacy deletion (CC-15) | Slice-by-slice removal of `toolbar.rs`'s builder, `tabs/strip.rs`'s renderer, `panels/*`'s `DisplayCommand` code, and `Palette` constants not covered by the mock-up (chrome surfaces the mock-up doesn't reach — reader view, source view, split view, real DevTools panels, ~30 panels total — stay legacy overlays indefinitely per the brief's risk #5), by the same pattern S12b used to delete `rquickjs` after ADR-018 |

### Fate of the DS track

DS-1…DS-19 (`docs/tasks/p1-design-v3.md`) is **complete and closed** (2026-07-23). It fully served its
purpose: a working, on-target chrome exists today, and its slices are what produced the gap analysis
this ADR relies on. **No new manual DS-numbered tasks are to be filed** — any further mock-up-driven UI
work happens through the CC track (CSS asset regeneration) once CC-4 lands, or as legacy-overlay fixes
in the interim. The DS-built legacy code is not deleted now: it keeps shipping as the default renderer
until CC-14 flips the default, then is removed slice-by-slice in CC-15 (leaving only mock-up-uncovered
surfaces, per risk #5 above).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Keep translating the mock-up into Rust by hand (status quo, DS-style) | Confirmed real by 19 DS slices: repeats per mock-up revision, two sources of truth can drift, gets none of the engine's own CSS coverage as dogfooding signal |
| ADR-015 Variant B as originally scoped (JS bundle + privileged bridge) | The chrome's actual need is "parse and render the mock-up's CSS", not "run untrusted-shaped JS" — a bridge's attack surface and per-frame JS execution cost buy nothing here; `ChromeModel` mutating the DOM from Rust is simpler and sufficient |
| Delete the DS-built legacy chrome immediately | No parity yet (CC-4 through CC-13 not built); would leave the browser without a working chrome mid-migration |
| Defer this decision further, keep evaluating both ADR-015 backends before picking | The dogfooding value and near-total CSS coverage are already evidenced by the DS track + gap analysis in `docs/tasks/p1-css-chrome.md` §2; further deferral only delays removing the DS-track's per-revision translation tax |

## Consequences

- **Positive:** the mock-up's HTML/CSS becomes the chrome's actual source of truth instead of a
  translation target; every chrome-rendering gap is a real, dogfooded engine bug (`BUG-NNN`) instead of
  a silent hand-coded workaround; future mock-up revisions regenerate assets instead of re-deriving
  Rust; resolves ADR-015's deferred choice without introducing its named JS-bridge attack surface.
- **Negative / trade-offs:** full restyle-per-hover/keystroke on a ~400-node chrome document is a new
  per-frame cost gated by CC-12 (≤2 ms budget, incremental-layout fallback); two chrome implementations
  (legacy Rust + engine-rendered) coexist under a flag from CC-4 through CC-15, doubling review surface
  during that window; text metrics for chrome UI move from femtovg's manual layout to the engine's text
  measurer, so pixel-level differences from legacy are expected and accepted (parity with the mock-up
  matters more than parity with legacy).
- **Future:** CC-12's perf gate is the concrete revisit trigger — if the ≤2 ms budget cannot be met
  even after `layout_mutation_incremental` and targeted optimization, the default flip (CC-14) is
  blocked and this decision must be revisited (e.g. fall back to a narrower engine-rendered scope,
  or reopen ADR-015's Variant B evaluation). `docs/tasks/p1-css-chrome.md` is the living execution
  log for CC-1…CC-17.

## Relationships

- **Resolves the deferred choice from** [ADR-015](ADR-015-swappable-chrome-view.md) in favour of
  Variant A (native/engine-rendered), without its JS-bridge Variant B.
- **Same flag-strategy idiom as** [ADR-018](ADR-018-v8-cutover.md) — opt-in flag → parity checklist →
  default flip → rollback flag → slice-by-slice legacy deletion.
- **Builds on** [ADR-009](ADR-009-shell-panel-system.md)'s `overlay_buf` painter's-order compositing,
  which already lets the engine-rendered chrome and legacy panels co-render during the flagged window.
- **Full execution plan:** [docs/tasks/p1-css-chrome.md](../tasks/p1-css-chrome.md).

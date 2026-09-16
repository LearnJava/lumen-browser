# BUG-536: CSS Transitions/Animations produce no JS-observable effect — no `Animation`/`CSSTransition`/`CSSAnimation` object, no lifecycle events, no live interpolated value

**Статус:** OPEN (ДОРАБОТКА → [GAP-CSSANIM](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-CSSANIM` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Дата:** 2026-08-03
**Компонент:** js/layout (CSS Transitions/Animations runtime — no obvious owner file yet,
see "Что нужно" below)
**Найден:** WPT-RUN-3 срез 27 (`ROADMAP.md`) — массовый прогон `css/css-transitions`
(120 testharness id, `run_report.py --all --root css/css-transitions --recursive --processes=6`);
confirmed to extend to CSS Animations (not just Transitions) срез 29 — массовый
прогон `css/css-animations`

## Симптом

Three independent JS-visible surfaces of a running CSS transition are all
silent, even though the corresponding `transition-*` longhands parse and
apply (the box does end up at its target style eventually per real
rendering — this is a JS-introspection gap, not necessarily a rendering
one):

**1. `document.getAnimations()` / `element.getAnimations()` never return the
transition.** Every test that indexes into the result (`document
.getAnimations()[0]`) gets `undefined`, cascading into
`TypeError: Cannot read properties of undefined (reading 'effect'|'ready'|
'finish'|'cancel'|'pending'|'currentTime'|'startTime'|'timeline'|
'transitionProperty'|'appendChild')` on the very next line. Count-based
assertions confirm it's not a filtering issue: `assert_equals: getAnimations
returns two running CSS Transitions expected 2 but got 0`.

**2. Transition lifecycle events never dispatch.** `transitionrun`/
`transitionstart`/`transitioncancel`/`transitionend` listeners never fire:
`events-001/003/005/006/008.html` and `before-load-001.html`/
`disconnected-element-001.html` all TIMEOUT waiting for one; `event-
dispatch.tentative.html` (30/30 subtests) times out on every state-machine
transition ("Timed out waiting for transitionrun[, transitionstart]");
`transitionevent-interface.html` (12/12) can't construct/inspect a
`TransitionEvent` instance because none is ever delivered live.

**3. `getComputedStyle()` never reflects a value mid-transition.** The
`properties-value-*.html` files (a self-contained legacy harness,
`support/{properties,generalParallelTest,runParallelAsyncHarness}.js`) set
`transition-property` on ~30 longhands each, change the target value, and
assert the *initial* read differs from the *target* read — both come back
`""` (`assert_not_equals: initial and target values may not match got
disallowed value ""`), and the paired "did `transitionend` fire" check
reports the same empty string. This is **not** the already-documented
`getComputedStyle` gaps ([BUG-472](BUG-472-OPEN.md) missing-longhand map,
[BUG-483](BUG-483-OPEN.md) missing `has` trap) — those properties
(`width`, `height`, `top`/`left`/`right`/`bottom`, `margin-*`, `padding-*`,
`border-*-width`) resolve correctly via `getPropertyValue` everywhere else
in the corpus; the `""` appears specifically when the read happens in the
context of an active transition. Simpler, non-harness-driven single-file
tests hit the identical `""` mid-transition pattern
(`changing-while-transition-00{1,2,3,4}.html`, `currentcolor-animation-001
.html`, `starting-of-transitions-001.html`, `shadow-root-insertion.html`,
`starting-style-*.html`), so it is not an artifact of the legacy harness
either.

## Масштаб находки

99/120 testharness files ran to completion (`harness OK`) in this slice, but
only 650/3211 subtests passed overall — the vast majority of the failing
2561 subtests trace to one of the three symptoms above. Representative
counts: `properties-value-001.html` (560/560 failing), `properties-value
-inherit-001.html`/`-002.html` (560/560 each), `properties-value-003.html`
(122/122), `properties-value-implicit-001.html`/`-inherit-003.html` (60/60
each), `event-dispatch.tentative.html` (30/30), `all-interpolates-same-as
-explicit-property.html` (32/32), plus every `CSSTransition-*.tentative
.html`/`KeyframeEffect-*.tentative.html`/`Document|Element-getAnimations
.tentative.html` file (getAnimations cluster, ~15 files). 7 files
(`events-001/003/005/006/008`, `before-load-001`, `disconnected-element
-001`) TIMEOUT at the harness level waiting on an event that never comes.

Not exclusive to this category: [BUG-533](BUG-533-FIXED.md) (`css-highlight
-api`, срез 26) already noted one file whose harness hung on a related
throw-during-cleanup path; this is the first slice to isolate the CSS
Transitions-specific JS surface as a whole.

## Расширение (срез 29, `css/css-animations`)

Identical mechanism, confirmed for the CSS Animations spec, not just CSS
Transitions: `Element.getAnimations()`/`document.getAnimations()` return
nothing for a running CSS (declarative, `animation-name`-triggered)
animation, so every `CSSAnimation-*.tentative.html`/`KeyframeEffect-*
.tentative.html`/`AnimationEffect-*.tentative.html` test that indexes into
the result cascades into the same `TypeError: Cannot read properties of
undefined (reading 'effect'|'ready'|'pending'|'finish'|'cancel'|'finished'|
'play'|'playState'|'pause'|'reverse'|'timeline')` family — **162 subtests**
this slice (78 `.effect`, 36 `.ready`, 17 `.pending`, 10 `.finish`, 6
`.cancel`, plus smaller counts for the rest). Same root cause as the
Transitions case: CSS-triggered animations (as opposed to JS-authored
`element.animate()`, which is the separate, already-tracked BUG-463 gap)
never get an `Animation`/`CSSAnimation` object wired into the
`getAnimations()` result.

Also affects `animation-range`/`animation-range-start`/`animation-range-end`/
`animation-name`/`animation` "doesn't seem to be supported in the computed
style" failures **only insofar as those tests also call `getAnimations()`**
— most of that specific message pattern is instead [BUG-539](BUG-539-OPEN.md)
(computed-style `has`-trap), not this bug; the two are easy to conflate
because both surface on the same test files. Kept separate in this slice's
`.ini` attribution by matching on the actual thrown message, not the test
file.

## Что нужно

Needs an engine-side investigation to locate where CSS-triggered transitions
are (or aren't) represented as `Animation`/`CSSTransition` instances and
wired into `Document`/`Element.getAnimations()`, and where `transitionrun`/
`transitionstart`/`transitionend`/`transitioncancel` are meant to be
dispatched — grep for `CSSTransition`/`getAnimations`/`transitionrun` across
`crates/js/src/dom.rs` and the layout/paint animation-tick path as a
starting point. Scope this as its own implementation task; this bug records
the WPT-visible symptom, not the fix.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-transitions/` for all
attributable files this slice (see `docs/wpt-status.md` → row `css` for the
full breakdown alongside BUG-483/484/463/384's shares of the same run).

## Срез 30 (spread across 10 categories, 2026-08-03) — first confirmation for Web Animations interpolation tests, not just CSS Transitions

43 files / ~2500 subtests, dominated by a new message shape:
`assert_true: Web Animations should be supported expected true got false` —
every `*-interpolation*.html`/`animation/*.html` file across `css-values`
(10), `filter-effects` (10), `css-shapes` (5), `css-anchor-position` (4),
`css-align` (4), `css-text-decor` (3), `css-break` (3), `css-color` (2),
`css-tables` (1), `css-contain` (1) gates its per-property interpolation
assertions on a `document.timeline`-backed support check before running —
same root as CSS Transitions (no JS-observable `Animation` object), just
reached via the Web Animations API entry point instead of a CSS-Transitions-
triggered one. Confirms the bug's title ("CSS Transitions/**Animations**")
rather than being a distinct API gap — not re-scoped, no new bug filed.
`.ini` under each category's own `tests/wpt/metadata/css/<category>/`.

## Срез 1 (GAP-CSSANIM, 2026-09-16, `p1-gap-cssanim-srez1`) — CSS Transitions lifecycle events now dispatch

Closed symptom 2 for CSS Transitions specifically (not yet CSS Animations,
symptom 1 `getAnimations()`, or symptom 3 `getComputedStyle()` mid-transition —
those stay open). `TransitionScheduler` (`crates/engine/layout/src/
animation.rs`) tracked interpolated values only, with no notion of JS or
events; it now also returns `TransitionEventInfo` (`Run`/`Start`/`End`/
`Cancel`) from `sync()`/`tick()`:

- `Run` fires in `sync()` the instant the transition is generated (even while
  still in its delay), matching CSS Transitions L1 §3.
- `Start` fires once in `tick()` when the transition leaves its delay period
  (`started_fired` guards against re-firing every subsequent frame).
- `End` fires once on completion (`completed` guards a `fill-mode: forwards/
  both` entry, which stays in `active` afterward to keep applying the end
  value, from re-firing `transitionend` on every following frame).
- `Cancel` fires in `sync()` for a still-running (not yet completed) entry
  that a newer transition supersedes before it could finish — reusing the
  `interrupted_value` detection the scheduler already had.

The shell collects these into `Lumen::transition_events` (same
cross-producer/single-delivery-point shape as `cv_events`, BUG-852: fed from
`apply_relayout_result`'s `sync()` calls and `RedrawRequested`'s `tick()`,
drained once per frame by `Lumen::deliver_transition_events` once a JS context
exists) and pushes them into JS via a new `PersistentJs::deliver_transition_events`
→ `_lumen_deliver_transition_events()` (`web_api_shim_mid_b.js`), which
dispatches real `TransitionEvent`s (`transitionrun`/`transitionstart`/
`transitionend`/`transitioncancel`) using the constructor that already existed
but nothing fired autonomously (`web_api_shim_mid.js`).

Live confirmation: `verify_event_delivery_gaps.py --variant css-transition`
(dev-release) now prints `transitionrun, transitionstart, transitionend`
(was "— nothing"). `--variant css-animation` still prints nothing, as
expected — `AnimationScheduler`/`@keyframes` events are a separate, not yet
wired path (BUG-503's part of this gap). Not in this slice: `getAnimations()`
returning a `CSSTransition` object (still `undefined`, so `.target`/`.effect`
reads on the dispatched event's `target` element still won't chain into a
Web Animations `Animation`), `getComputedStyle()` mid-transition, and
`transitioncancel` on node removal (`remove_node()` does not emit events).
8 new unit tests in `crates/engine/layout/src/animation.rs`
(`sync_fires_run_event_for_new_transition` and siblings).

## Срез 3 (GAP-CSSANIM, 2026-09-16, `p1-gap-cssanim-srez3`) — `getComputedStyle()` now reflects the live interpolated `opacity`/`transform`

Closed symptom 3 for the two properties the schedulers already track for
compositor offload — `opacity` and `transform`. `getComputedStyle()` used
to answer from `V8JsRuntime::computed_styles`, a snapshot refreshed only on
a real relayout or a lazy same-tick flush (`FlushHandles::maybe_flush`,
CSSOM-4/BUG-493) — neither fires just because an already-running CSS
transition/animation ticks its clock, so a page polling
`getComputedStyle(el).opacity` mid-animation always read the pre-animation
(or post-relayout) value. `AnimationFrame` (`crates/engine/layout/src/
animation.rs`) now has `to_computed_style_patches()`, serializing its
per-node `opacity`/`transform` overrides through the same
`opacity_to_css`/`transform_list_to_css` helpers `computed_style_to_map`
uses (factored out of `selector_query.rs` for this reuse, so a live value
reads back byte-identical to a static one). The shell calls this once per
frame, right after the schedulers tick (`crates/shell/src/lumen/
animated_computed_style.rs`, same single-delivery-point shape as
`deliver_transition_events`/`deliver_animation_events`), and merges the
result straight into `computed_styles` via a new `V8JsRuntime::
patch_animated_computed_styles` — no JS event dispatch involved, since
`getComputedStyle()` reads the snapshot synchronously and has no listener
to notify. A subsequent real relayout's `update_computed_styles` naturally
overwrites these entries with the settled value once the animation stops
ticking.

Live confirmation: `verify_event_delivery_gaps.py --variant
css-animation-progress` (dev-release) now prints `opacity 1, opacity
0.709377, opacity 0.55937105, …` (was static `opacity 1`); a manual
`translateX` probe over the same harness showed `transform: none` →
`translateX(28.98px)` → `translateX(44.00px)` → `translateX(59.00px)`.
`--variant css-animation`/`css-transition` (event dispatch, срезы 1/2)
unaffected. Not in this slice: `color`/`background-color`/`height`
overrides (tracked by the same schedulers but not wired into this
snapshot), `getAnimations()` still returning nothing for a CSS-triggered
animation/transition (separate, architecturally larger gap — no
`Animation`/`CSSAnimation` object exists to return `.effect`/`.currentTime`
from), and `getBoundingClientRect()`/other geometry reads mid-animation
(untouched — `transform` is compositor-offloaded by design and does not
affect layout geometry; a layout-affecting animated property like
`margin-left` already goes through a real relayout each tick and was not
re-verified as broken here). 4 new unit tests in `crates/engine/layout/src/
animation.rs` (`computed_style_patches_*`).

# BUG-536: CSS Transitions/Animations produce no JS-observable effect — no `Animation`/`CSSTransition`/`CSSAnimation` object, no lifecycle events, no live interpolated value

**Статус:** OPEN
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

Not exclusive to this category: [BUG-533](BUG-533-OPEN.md) (`css-highlight
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

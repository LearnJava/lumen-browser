# BUG-503: `animationend` never fires for a real (engine-driven, non-scripted)
CSS animation — `async_test`s waiting on it TIMEOUT

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js/engine boundary (animation event dispatch — exact site not
isolated this slice; `AnimationEvent` constructor exists in
`crates/js/src/dom.rs:3555` and can be manually constructed/dispatched, but
nothing was found that fires one autonomously when a scheduled CSS animation
completes)
**Найден:** WPT-RUN-3 срез 10 (`ROADMAP.md`) — массовый прогон `css/css-variables`

## Механизм

Not root-caused to a specific line this slice (flagged as an observation,
same as BUG-488/BUG-493 were on first sighting) — the mechanism is inferred
from behaviour, not confirmed via source read of the dispatch path. Six
files in this slice follow the same pattern: a `@keyframes` rule with
`animation-duration` in the ~1s range, started via
`element.style.animationPlayState = "running"` (or already running from
page load), with an `async_test` registering an `'animationend'` listener
via `addEventListener` and calling `step_func_done()` inside it. Every one
of these `async_test`s times out — the listener callback never fires, so
`done()` is never called. This is independent of
[BUG-499](BUG-499-OPEN.md)/[BUG-493](BUG-493-OPEN.md) (which affect the
*synchronous* "before animation" assertions in the same files, a separate
symptom) — the manually-constructible `AnimationEvent` (confirmed present,
`dom.rs:3555`, used in an existing unit test that manually dispatches one)
shows the *type* exists; what's missing is the engine autonomously firing
one when a real, scheduled animation's active duration elapses.

## Симптом

```
[TIMEOUT] Verify color after animation -- Test timed out
[NOTRUN] Verify CSS variable value after animation --
```

`variable-animation-from-to.html`, `-over-transition.html`, `-to-only.html`
(NOTRUN — the `async_test` registered but its containing file's other tests
never let the harness reach a state where it's scheduled) and
`variable-animation-substitute-into-keyframe.html`/`-into-keyframe-shorthand.html`/
`-into-keyframe-transform.html`/`-within-keyframe.html`/`-within-keyframe-fallback.html`/
`-within-keyframe-multiple.html` (explicit TIMEOUT) all hang the same way.
Also relevant to `variable-transitions-transition-property-all-before-value.html`/
`-value-before-transition-property-all.html`, which wait on `'transitionend'`
instead (NOTRUN for their "after" checks) — plausibly the same underlying
gap (`transitionend`/`animationend` sharing a dispatch mechanism), not
independently confirmed.

## Масштаб находки

9 files this slice (`css/css-variables`), all through the
`animationend`/`transitionend`-wait idiom. Not surveyed beyond this slice —
inferred to affect any WPT test anywhere using this idiom, but unconfirmed
against, e.g., `css-animations`/`css-transitions` categories directly (not
yet vendored/run at time of writing).

## .ini

Committed `.ini` marking the "after animation"/"after transition"
subtests `expected: TIMEOUT` (or `NOTRUN` where the harness itself reports
that status) in each of the 9 files above, header citing BUG-503. The
"before" subtests in the same files are attributed to BUG-499/BUG-493
instead (see those files' `.ini` headers, which cite both).

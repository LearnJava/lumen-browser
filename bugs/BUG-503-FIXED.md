# BUG-503: `animationend` never fires for a real (engine-driven, non-scripted)
CSS animation — `async_test`s waiting on it TIMEOUT

**Статус:** OPEN (ДОРАБОТКА → [GAP-CSSANIM](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-CSSANIM` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
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

---

## Эскалация 2026-08-21 (P2, WPT-RUN-6 срез 15): не только событие — CSS-анимация невидима для JS целиком

Механизм, который срез 10 WPT-RUN-3 вывел из поведения, теперь подтверждён
прямым замером и чтением кода, и оказался шире записанного выше.

**Диспетчера нет вовсе.** `grep -rn "animationstart" crates/` даёт ровно два
совпадения — комментарий над конструктором `AnimationEvent`
(`crates/js/src/dom.rs:610`) и имя атрибута в списке `on*`-свойств
(`dom.rs:998`); ни одной строки, которая бы событие *отправляла*, в
воркспейсе нет. То же для `transitionend`/`transitionrun`/`transitioncancel`
(смыкается с [BUG-536](BUG-536-FIXED.md), где то же сказано про переходы).

**Замер** — `tests/wpt/verify_event_delivery_gaps.py` (живое окно, http,
улики из stderr; dev-release, Linux, коммит `a7ee9468f`):

| проба | получено |
|---|---|
| `animation: fade 100ms linear 2` + слушатели всех четырёх `animation*` | ни одного события за 8 с, страница жива |
| `transition: opacity 100ms` + смена `style.opacity` + слушатели всех четырёх `transition*` | ни одного события |
| WAAPI `element.animate(...).onfinish` на той же странице | **срабатывает** — дефект в CSS-driven пути, не в событиях вообще |
| `getComputedStyle(el).opacity` каждые 300 мс на 2-секундной анимации `opacity: 1 → 0` | `1` всё время |
| `el.getBoundingClientRect().left` на 2-секундной `margin-left: 0 → 300px` | `8` всё время |

То есть ждать события — не единственный способ повиснуть: тест, который
следит за геометрией через `ResizeObserver` или читает `getComputedStyle`,
тоже не дождётся ничего. Именно так висят `css/css-anchor-position/
transform-01x.html`: у них два выхода — колбэк `ResizeObserver` (нужен
реальный сдвиг вёрстки) и `onanimationend` (нужно событие), и закрыты оба.

**Масштаб по корпусу.** Механизм `css-animation-events` в
`tests/wpt/timeout_audit.py` забирает 61 id остатка снимка WPT-RUN-5:
`css/css-transitions` 14, `css/css-variables` 11, `css/css-animations` 10,
`dom/nodes/moveBefore` 10 (там ждут `animationstart`, чтобы проверить, что
`moveBefore` не перезапускает анимацию), `css/css-anchor-position` 7,
остальное поштучно. Это остаток *после* всех прочих механизмов — файлов,
завязанных на идиому, в корпусе больше.

**Проверка фикса:** `verify_event_delivery_gaps.py --variant css-animation
--variant css-transition --variant css-animation-progress
--variant css-animation-layout` — первые две печатают события, третья
показывает падающий `opacity`, четвёртая — растущий `left`.

---

## Срез 1 (GAP-CSSANIM, 2026-09-16, `p1-gap-cssanim-srez1`) — `transitionend` (CSS Transitions half) закрыт, `animationend` (CSS Animations half) остаётся

Живой замер этого среза (`verify_event_delivery_gaps.py --variant
css-transition`) подтверждает: `transitionrun`/`transitionstart`/
`transitionend` теперь диспатчатся автономно — см. подробности механизма в
[BUG-536](BUG-536-FIXED.md#срез-1-gap-cssanim-2026-09-16-p1-gap-cssanim-srez1--css-transitions-lifecycle-events-now-dispatch).
Файлы этого бага, ждущие `'transitionend'` (см. «Симптом» выше), закрыты этим
срезом; файлы, ждущие `'animationend'` от `@keyframes`-анимации, — нет:
`--variant css-animation` по-прежнему печатает «— nothing», `AnimationScheduler`
не тронут. Остаток бага — именно эта половина.

## Срез 2 (GAP-CSSANIM, 2026-09-16, `p1-gap-cssanim-srez2`) — `animationend`/`animationstart`/`animationiteration` for CSS Animations now dispatch

Closed the remaining half. The shell's `AnimationScheduler`
(`crates/shell/src/animation_scheduler.rs`, distinct from the unused
`lumen_layout::animation::AnimationScheduler` — the tree-walking one wired
into `RedrawRequested` is the one that matters) tracked interpolated
keyframe values per `(node, animation-index)` in a `RunState`, with no
notion of JS or lifecycle events. `RunState` now also carries
`started_fired`/`iterations_completed`/`completed`/`last_local_time_s`, and
`tick()` returns `Vec<AnimationEventInfo>` alongside the render frame:

- `Start` fires the first tick where the local time (after delay) is `>= 0`.
- `Iteration` fires once per completed loop boundary crossed since the last
  tick, except the final one (whose completion is `End`, not `Iteration`,
  per CSS Animations L1 §4.5.1).
- `End` fires once when the active period completes (`local_time_s >=
  duration * iteration_count`), independent of `fill-mode` — an entry with
  `fill-mode: none` still logically ends even though `compute_t` stops
  returning an override for it that frame.
- `Cancel` fires for any instance still tracked but not visited by the
  current frame's tree walk (`animation-name` changed away, node removed) —
  detected via a per-tick visited-set diff against `self.running`, skipped
  for instances that already fired `End` (a re-render after completion must
  not look like a cancellation).

The shell collects these into `Lumen::animation_events` (same
cross-producer/single-delivery-point shape as `transition_events`, срез 1)
and pushes them into JS via `PersistentJs::deliver_animation_events` →
`_lumen_deliver_animation_events()` (`web_api_shim_mid_b.js`), which
dispatches real `AnimationEvent`s using the constructor that already existed
(`web_api_shim_mid.js`) but nothing fired autonomously.

Live confirmation: `verify_event_delivery_gaps.py --variant css-animation`
(dev-release) now prints `animationstart, animationiteration, animationend`
(was «— nothing»); `--variant css-transition` unaffected (still `run+start+
end`). Not in this slice: `document.getAnimations()`/`element.getAnimations()`
still return nothing for a CSS-triggered animation (no `Animation`/
`CSSAnimation` object exists to return — `.target`/`.effect` reads chained
off the dispatched event's `target` still won't reach a Web Animations
object), and `getComputedStyle()` mid-animation still doesn't reflect the
interpolated value (`--variant css-animation-progress` still reports a
static `opacity 1`, not falling — same gap symptom 3 documented for
transitions in [BUG-536](BUG-536-FIXED.md)). 6 new unit tests in
`crates/shell/src/animation_scheduler.rs`
(`tick_fires_start_on_first_active_frame` and siblings).

## Срез 3 (GAP-CSSANIM, 2026-09-16, `p1-gap-cssanim-srez3`)

`getComputedStyle()` mid-animation now reflects the live `opacity`/
`transform` — `--variant css-animation-progress` prints `opacity 1, opacity
0.709377, …` instead of a static `1`. Full mechanism documented in
[BUG-536](BUG-536-FIXED.md#срез-3-gap-cssanim-2026-09-16-p1-gap-cssanim-srez3--getcomputedstyle-now-reflects-the-live-interpolated-opacitytransform)
(same fix, both bugs share this symptom). `getAnimations()` still returns
nothing for a CSS-triggered animation — untouched by this slice.

## Срез 4 (GAP-CSSANIM, 2026-09-17, `p1-gap-cssanim-srez4`)

`getComputedStyle()` now also reflects live `color`/`background-color`/
`height` during a transition (not `@keyframes` animations — at the time,
that scheduler never interpolated `height`). Full mechanism documented in
[BUG-536](BUG-536-FIXED.md#срез-4-2026-09-17-p1-gap-cssanim-srez4). `getAnimations()`
still returns nothing for a CSS-triggered animation/transition — untouched
by this slice.

## Срез 5 (GAP-CSSANIM, 2026-09-17, `p1-gap-cssanim-srez5`)

`@keyframes height` now interpolates too — `AnimationScheduler` (the
`@keyframes` ticker) gained the same `height` plumbing `TransitionScheduler`
already had. Full mechanism in
[BUG-536](BUG-536-FIXED.md#срез-5-2026-09-17-p1-gap-cssanim-srez5). Also:
`getAnimations()` turned out to already be fully implemented in the JS shim
(`_wa_animations`/`Animation`/`CSSAnimation`) — the actual gap is that
CSS-driven transitions/animations never register into that registry, so it
returns `[]` for them specifically, narrower than "still returns nothing"
implied above. `getBoundingClientRect()`/geometry mid-animation confirmed
broken for layout-affecting properties (height/margin/width) — the
per-frame compositor path skips relayout by design.

## Срез 6 (GAP-CSSANIM, 2026-09-17, `p1-gap-cssanim-srez6`)

`getAnimations()` now returns something for a running `@keyframes` animation.
Full mechanism in [BUG-536](BUG-536-FIXED.md#срез-6-2026-09-17-p1-gap-cssanim-srez6)
— `_lumen_deliver_animation_events` (`web_api_shim_mid_b.js`) registers a
shadow `Animation` into `_wa_animations` on `animationstart`, keeps it there
(`playState: 'finished'`) past `animationend` per CSS Animations L1 §4.5.1,
and drops it on `animationcancel`. `getBoundingClientRect()`/geometry
mid-animation remains open — untouched by this slice.

## Срез 8 (GAP-CSSANIM, 2026-09-17, `p1-gap-cssanim-srez8`)

Correction to срез 5: `@keyframes height` did **not** actually reach
`getComputedStyle()` — срез 5 fixed/tested the wrong (unused) `AnimationScheduler`
type. Now fixed in the live one (`crates/shell/src/animation_scheduler.rs`).
Full mechanism and the remaining, architecturally larger geometry gap
(`getBoundingClientRect()` during height/margin/width animation, which needs a
real relayout pass fed an animated override — not present anywhere in the
layout pipeline today) in
[BUG-536](BUG-536-FIXED.md#срез-8-gap-cssanim-2026-09-17-p1-gap-cssanim-srez8--correction-keyframes-height-did-not-reach-getcomputedstyle-in-the-live-path-now-it-does-geometry-mid-animation-remains-open-and-is-architecturally-larger-than-a-slice).

## Срез 9 (GAP-CSSANIM, 2026-09-17, `p1-gap-cssanim-srez9`) — geometry mid-animation closed; `GAP-CSSANIM` fully closed

`getBoundingClientRect()`/`getClientRects()` now track an animated `height`
(thread-local `ANIMATED_HEIGHTS` patched into the layout tree before the one
"full" layout pass, plus a forced relayout each tick a height animation runs
without a DOM mutation). Full mechanism in
[BUG-536](BUG-536-FIXED.md#срез-9-gap-cssanim-2026-09-17-p1-gap-cssanim-srez9--getboundingclientrectgeometry-now-tracks-an-animated-height-task-closed).
This was the last open item under `GAP-CSSANIM` — the task (and this bug) is
now closed.

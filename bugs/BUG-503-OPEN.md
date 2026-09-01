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
(смыкается с [BUG-536](BUG-536-OPEN.md), где то же сказано про переходы).

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

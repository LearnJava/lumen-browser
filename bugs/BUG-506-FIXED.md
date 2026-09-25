# BUG-506: `<script src="../other-category/support/helper.js">` (cross-directory relative external script) never executes before dependent inline code under the wptrunner-driven pipeline

**Статус:** FIXED 2026-09-24 (P3)
**Дата:** 2026-08-02
**Компонент:** unclear — likely shell script-loading order (`crates/shell/src/main.rs`) or a
wptrunner/BiDi-navigation-specific timing gap; not root-caused to a single file/line this slice
**Найден:** WPT-RUN-3 срез 12 (`ROADMAP.md`) — массовый прогон `css/css-logical`

## Симптом

`animation-001.html`/`animation-002.html`/`animation-003.tentative.html`/`animation-004.html`
and `animations/logical-shorthand-relative-prioritization-by-number-of-components.tentative.html`
all include, in document order:

```html
<script src="/resources/testharness.js"></script>
<script src="/resources/testharnessreport.js"></script>
<script src="../css-animations/support/testcommon.js"></script>
<script>
'use strict';
test(t => {
  const div = addDiv(t);   // ReferenceError: addDiv is not defined
  ...
```

`../css-animations/support/testcommon.js` defines `addDiv`/`addStyle`
(`tests/wpt/css/css-animations/support/testcommon.js:125`/`139`) — a
helper shared across `css/css-animations` and (via this relative path)
`css/css-logical`. Every `test()` body in these 5 files throws
`ReferenceError: addDiv is not defined` (or `addStyle is not defined`),
i.e. the third external `<script src>` never ran (or its top-level
`function addDiv(...)` declaration never landed on the global scope)
before the fourth, inline `<script>` block executed — **reproducibly**,
confirmed by re-running `run_report.py --root css/css-logical --limit 1`
against just this one file: same `addDiv is not defined` failures every
time.

## Investigation — not a simple "helper never loads"

A manual `--mcp-live-port` probe of the exact same URL (served the same
way, `python -m http.server`), navigating and then — in a **separate**
`eval()` call issued after `navigate()` returned — querying
`typeof window.addDiv`, returns `"function"`:

```js
JSON.stringify({addDiv: typeof window.addDiv, addStyle: typeof window.addStyle,
                 test: typeof window.test, ready: document.readyState})
// → {"addDiv":"function","addStyle":"function","test":"function","ready":"complete"}
```

The shell's own stdout for that same manual load additionally shows the
three external scripts fetched out of network-arrival order (`testharness.js`,
then `testcommon.js`, then `testharnessreport.js` — NOT the order they were
requested in) but **executed** in the correct document order (`Загружен
скрипт: testharness.js` → `testharnessreport.js` → `testcommon.js`), which
is the spec-correct blocking-classic-script behaviour. So a plain
navigate-then-inspect probe shows nothing wrong.

Yet the actual wptrunner/BiDi test-execution pipeline (`tools/wptrunner`,
the same mechanism used for every WPT-RUN-3 slice) reproducibly observes
`addDiv is not defined` for these exact files, every run. The discrepancy
between "works under a simple navigate+eval probe" and "fails
deterministically under the real wptrunner harness" was not resolved this
slice — candidate mechanisms not yet ruled in/out:

- wptrunner's own executor may start polling/evaluating page state (or
  the page's own inline script may begin executing) before
  `browsingContext.navigate`'s readiness wait has actually let all three
  external scripts finish running, specifically when their relative URLs
  cross a directory boundary (`../css-animations/support/...` — every
  other helper script this track has exercised so far lives under
  `/resources/` or `css/support/`, both same-or-shallower-depth paths;
  this is the first slice to hit a `../`-crossing relative `<script src>`
  under wptrunner).
- The raw, non-normalized request URL observed in the shell log
  (`http://.../css/css-logical/../css-animations/support/testcommon.js`,
  `..` not collapsed) could interact with wptrunner's own `wptserve`
  routing/caching differently than with a plain `http.server`.

## Масштаб находки

5 files / 57 subtests this slice, all sharing the identical
`../css-animations/support/testcommon.js` dependency:
`animation-001.html` (24), `animation-002.html` (15),
`animation-003.tentative.html` (1), `animation-004.html` (16),
`animations/logical-shorthand-relative-prioritization-by-number-of-components.tentative.html`
(1). Not surveyed beyond `css/css-logical` this slice — the same helper
is referenced (unexercised so far) by `css/css-view-transitions`
(`pseudo-element-animations.html`/`-rerun.html`), which would be the next
place to confirm whether this is specific to `css-logical`'s particular
`../`-crossing path or a general "external script from a differently-
rooted relative path" gap.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-logical/` for all 5
files, `expected: FAIL` per subtest (harness itself completes `OK`, only
individual subtests fail).

## Срез P3 2026-09-04: the original mechanism no longer reproduces — real blocker is CSSOM-4/BUG-493's shell gap

Re-measured all 5 files against the exact real wptrunner pipeline (not a
reimplementation): `verify_bug506_cross_dir_script.py` (modeled on
`verify_bug961_orchestration.py`) builds the same `TestEnvironment`
(real `wptserve`, real `.any.js`/`AnyHtmlHandler` routing) and drives
`LumenTestharnessExecutor._run_testharness` — imported unmodified from
`executorlumen.py` — against a freshly built `dev-release` binary, no
`TestRunnerManager` involved. Ran twice per file: variant A is the real
executor path, variant B a bare `BidiSession.navigate(wait="complete")`
followed by a *separate* `script.evaluate`, isolating whether the
executor's own poll loop matters at all.

**`addDiv is not defined`/`addStyle is not defined` does not occur on any
of the 5 files, in either variant.** `typeof window.addDiv` reads back
`"function"` and the harness itself completes `status=0` (OK) every time
— the helper fetch itself was also confirmed directly (`urllib.request`
against the exact cross-directory URL `wptserve` serves it at,
`/css/css-animations/support/testcommon.js`, 7511 bytes, contains
`function addDiv`). Whatever produced the original symptom (WPT-RUN-3
срез 12, 2026-08-02) has been resolved as a side effect of unrelated work
in the ~1 month since — the shell's script-loading/prefetch pipeline has
seen multiple slices since (BUG-171 prefetch cache, streaming pipeline
changes) that plausibly closed the race window; not worth archaeology now
that it's gone.

**All 5 files still fail today, but every failure is now attributable to
a single, already-tracked, different defect: CSSOM-4/BUG-493's shell
coverage gap.** `bugs/BUG-493-FIXED.md`'s CSSOM-4 slice (2026-09-03)
made `FlushHandles::maybe_flush` force a synchronous style+layout
recompute before `_lumen_get_computed_style`/`_lumen_get_bounding_rect`/
`_lumen_get_custom_property` read their snapshot maps — **but only for
`InProcessSession`** (headless/`--mcp-port`); the doc comment on
`crates/js/src/v8_runtime/style_flush.rs` says outright "the interactive
shell never calls `update_stylesheet`, so `FlushHandles::stylesheet`
stays `None` there and `maybe_flush` is a no-op". Every real wptrunner
run launches Lumen via `--bidi-port` (`tools/wptrunner/wptrunner/
browsers/lumen.py::make_command`) — the live/interactive path, not
`InProcessSession` — so this gap is live on literally every WPT test
Lumen runs, not just this file's 5. [BUG-977](BUG-977-FIXED.md)/CSSOM-7
already tracks one narrow instance of it (`_lumen_request_scroll`'s
`is_clip` check); what this slice adds is that the *general* case —
plain `getComputedStyle()` and `HTMLStyleElement.sheet` on a
freshly-mutated node, no scroll/clip involved — hits the exact same
no-op `maybe_flush` wall:

- `animation-001.html` (24 subtests) and the `logical-shorthand-…
  .tentative.html` file (1 subtest): `addDiv(t)` appends a new `<div>`
  in the same synchronous script turn `getComputedStyle(div).<prop>`
  reads it back — `computed_styles` has no entry yet for that node id
  (no relayout has run since it was inserted), so every property reads
  back `""` instead of its real computed value. This reclassifies one
  subtest, `Logical properties are NOT stored as physical properties`
  (checks `KeyframeEffect.getKeyframes()` structure only, no
  `getComputedStyle` call) — it does **not** hit the gap and now
  genuinely `PASS`es; the committed `.ini` incorrectly still listed it
  `expected: FAIL` and has been corrected.
- `animation-002.html`/`animation-003.tentative.html`/`animation-004.html`
  (15+1+16 subtests): `addStyle(t, rules)` (`support/testcommon.js:139`)
  does `document.head.appendChild(extraStyle); … extraStyle.sheet
  .insertRule(...)` in the same synchronous turn — `.sheet` is still
  `null` (the `CSSStyleSheet` object HTML LS §"create a stylesheet"
  associates with a connected `<style>` element hasn't been created yet,
  same missing-synchronous-recompute cause), so every subtest throws
  `TypeError: Cannot read properties of null (reading 'insertRule')`
  instead of `ReferenceError: addDiv is not defined`. Subtest names/count
  are unchanged from the original filing — only the failure message
  differs — so the existing `.ini`s for these 3 files needed no `[…]`
  block changes, only the stale root-cause comment.

**Blocker reattributed from "unclear" to CSSOM-4/BUG-493** (owner: the
P1-owned CSSOM roadmap track). This is not a point fix for P3 — it is
the same architectural hole BUG-977/CSSOM-7 already exists for, just
observed through two different native entry points
(`_lumen_get_computed_style`/`HTMLStyleElement.sheet` vs
`_lumen_request_scroll`). Extending `FlushHandles`/an equivalent
synchronous recompute to the live shell (`crates/shell/src/relayout.rs`)
is what unblocks all 57 subtests across these 5 files at once, plus an
unknown amount of the rest of the WPT corpus that this session did not
survey (any test that mutates the DOM/style then reads it back
synchronously, in the same script turn, under a *live-window* run — which
is every real Lumen WPT run, not a `--mcp-port` one).

Probe script kept as `tests/wpt/verify_bug506_cross_dir_script.py` for
re-verification once CSSOM-4/BUG-493's shell coverage lands.

## Закрытие P3 2026-09-24: блокер снят CSSOM-7, найден и исправлен последний собственный дефект

Перемер тем же `verify_bug506_cross_dir_script.py` на свежем `dev-release`
(все 5 файлов, вариант A — настоящий `LumenTestharnessExecutor`):

- Исходный симптом (`addDiv is not defined`) по-прежнему не воспроизводится,
  `typeof addDiv === "function"` в обоих вариантах.
- Блокер, на который баг был переатрибутирован 2026-09-04 (same-tick
  `getComputedStyle()` в живом `--bidi-port`-окне), закрыт CSSOM-7
  (`44baf2b10`): шелл теперь пушит каскадный лист, `maybe_flush` не no-op.
  В `animation-001.html` 5 подтестов, которые под ним стояли, уже зелёные на
  `main` (blockSize через object/array notation, physical-wins-over-logical ×3).
- **Найден отдельный точечный дефект:** `computed_style_to_map`
  (`crates/engine/layout/src/selector_query.rs`) вообще не сериализовал
  `writing-mode`, `direction`, `unicode-bidi`, `text-orientation`. Поля в
  `ComputedStyle` каскадились и использовались layout/bidi, но
  `getComputedStyle(el).writingMode`/`.direction` возвращали `""`. Отсюда
  падали `writing-mode is not animatable` / `direction is not animatable`
  (ожидают `horizontal-tb`/`ltr`), а заодно все
  `css/css-writing-modes/parsing/*-computed.html`.

### Правка

Четыре ключа добавлены в `computed_style_to_map`, значения — «as specified»
(CSS Writing Modes L4 §2.1/§2.2/§3.1/§5.1). Юнит-тест
`selector_query::tests::writing_mode_family_is_serialised` проверяет значение
по умолчанию и нестандартное значение каждого свойства.

### Замер (wptrunner, `run_report.py --check`, до/после на одной машине)

| Каталог | `main` | ветка |
|---|---|---|
| `css/css-writing-modes/parsing` | 2 unexpected PASS | **16** unexpected PASS, 0 регрессий |
| `css/css-logical` | 183 unexpected PASS | **185** (+2 `…is not animatable`); множество REGRESSION побитово то же |

14 новых PASS в writing-modes: `writing-mode-computed` 3/3,
`direction-computed` 2/2, `unicode-bidi-computed` 6/6,
`text-orientation-computed` 3/3 (2 `direction-invalid` — уже на `main`).
`.ini` для этих 5 файлов удалены (все перечисленные подтесты теперь PASS).
Из `css-logical/animation-001.html.ini` сняты 7 устаревших записей FAIL.
Шапки остальных `.ini` переписаны под текущие причины.

### Что осталось — вне этого бага

- `animation-002/003/004` (32 подтеста): `HTMLStyleElement.sheet === null`
  сразу после `appendChild` — остаток [BUG-493](BUG-493-FIXED.md) (передан P6
  2026-09-24, ломает styled-components на реальных сайтах).
- `animation-001` (17 подтестов) и `logical-shorthand-…tentative`: логические
  свойства в keyframes Web Animations не учитывают `writing-mode`/`direction`
  элемента (задокументированный LTR-only объём CSS Logical L1 в
  `CSS-SPECS.md`), плюс приоритет shorthand/longhand внутри одного keyframe;
  точечно не локализовано.
- `run_report.py --check --root css/css-logical` на чистом `main` даёт 534
  REGRESSION. Это устаревший baseline, а не регресс движка: `.ini` для
  `logical-box-*`/`inheritance`/`*-interpolation` ждут харнесс-`TIMEOUT`
  (тогда неперечисленные подтесты не считались), а харнесс теперь
  завершается `OK`, и каждый непройденный подтест читается как «expected
  PASS regressed». Нужен `--update-expected` по каталогу; здесь не делался.

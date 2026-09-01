# BUG-622: `document.defaultView` is missing entirely (should return `window`)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — live `document` object literal, ~`dom.rs:6989+`)
**Найден:** P2, WPT-VENDOR-inert, 2026-08-04

## Симптом

Confirmed live (`--mcp-live-port`):

```js
typeof document.defaultView       // → "undefined" (should be "object")
document.defaultView === window   // → false        (should be true)
```

`grep -c defaultView crates/js/src/dom.rs` is 0 — not a broken getter,
the property doesn't exist at all on the live `document` object (same
diagnostic pattern as [[reference_shim_dual_document_split]]: check
`'prop' in document`, not `document.prop !== undefined`, to tell "no
property" from "property that evaluates to undefined").

## Масштаб

Found via `inert-does-not-match-disabled-selector.html`:
`document.defaultView.getComputedStyle(button)` throws `Cannot read
properties of undefined (reading 'getComputedStyle')` — the test uses
`document.defaultView.getComputedStyle` instead of the equivalent global
`getComputedStyle`, a common defensive-coding idiom in WPT tests (works
in an iframe/detached-document context where the global `getComputedStyle`
isn't guaranteed to resolve to the right window). 1 file directly hit in
this category; likely a wider-impact gap since `defaultView` is one of the
most basic `Document` properties (WHATWG DOM §3.5) and a common pattern in
test helper libraries (`elem.ownerDocument.defaultView`) for reaching
"the window a node belongs to" without assuming `window` is in scope.

Fix shape: add `get defaultView() { return window; }` to the live
`document` object literal — same one-line pattern as other single-value
accessors on that object.

## Реконфирмация 2026-08-05 (WPT-VENDOR-pointerevents)

The predicted "wider-impact" played out: after fixing the separate
vendoring gap [BUG-654](BUG-654-FIXED.md) (`test_driver.Actions` was
undefined, masking everything downstream of it), this became the single
largest failure cluster in the corrected `pointerevents` run — dozens of
subtests across `setPointerCapture`/boundary-event/predicted-list tests,
each surfacing as `Error: Browsing context for element was detached`
(misleading text, same root cause) from
`tools/wptrunner/wptrunner/testdriver-extra.js::get_context`'s
`element.ownerDocument.defaultView` check. Confirms this is worth
prioritizing — it silently degrades signal quality for every WPT category
whose tests route through `test_driver`'s element-targeted helpers
(`send_keys`, `action_sequence`, `get_computed_label`, etc.), not just the
one file already on record.

## Реконфирмация 2026-08-21 (WPT-RUN-6 срез 3): единственная причина 307 TIMEOUT в `editing/`

`WPT-RUN-6` (разбор массовых TIMEOUT по Windows-снимку `WPT-RUN-5`) взял
`editing` — четвёртая по размеру нераспознанная категория (307 TIMEOUT из
429 прогнанных, 71.6 %). Дедупликация по базовому файлу (без `?variant`)
даёт всего **32 уникальных файла** — то есть один и тот же дефект
размножен `<meta name="variant">`-развёрткой ~10×.

Живой прогон одного файла (`editing/other/cefalse-boundaries-deletion.html`
— **не использующего** `test_driver` вовсе, только `document.execCommand`)
подтверждает механизм напрямую:

```
script error: JS runtime error: Cannot read properties of undefined (reading 'test_driver')
...
TEST_END: TIMEOUT, expected OK
```

Причина — `editing/include/editor-test-utils.js`, `EditorTestUtils`
constructor:

```js
constructor(aEditingHost, aHarnessWindow = window) {
  this.editingHost = aEditingHost;
  if (aHarnessWindow != this.window && this.window.test_driver) {
```

`this.window` — геттер `get window() { return this.document.defaultView; }`
— всегда `undefined` (этот баг). `aHarnessWindow != this.window` истинно
(реальный `window` не равен `undefined`), поэтому JS безусловно вычисляет
`this.window.test_driver` и падает синхронно **в конструкторе**, ещё до
регистрации хотя бы одного `test()`/`promise_test()` — testharness.js не
получает ни одного теста и никогда не публикует `harness_status`, отсюда
TIMEOUT, а не FAIL. Это бьёт КАЖДЫЙ файл, инстанциирующий
`EditorTestUtils` — включая те, что вообще не используют `test_driver`
(как в репро), потому что проверка безусловна.

**Масштаб:** `editor-test-utils.js` подключают **82 файла** в `editing/`
(шире, чем 32 уже попавших в TIMEOUT этого прогона — часть остальных 50,
видимо, падает по другим, ранее объясненным причинам раньше, чем
доходит до этой строки, или ещё не прогонялась). В целом по корпусу
`.defaultView` встречается в 42 файлах напрямую, из них 11 — `html/`,
7 — `custom-elements/`, 4 — `dom/`, 4 — `css/`, что подтверждает
предсказание 2026-08-05 «across every category routing through
`test_driver`'s element-targeted helpers» — сюда добавляется целый класс
«безусловная проверка в конструкторе хелпера», не завязанный на
`test_driver` вовсе.

Отдельно в этом же срезе подтверждена **не новая** причина:
`css/css-grid/alignment` (162 TIMEOUT, 74.7 %) — это целиком уже известный
[BUG-564](BUG-564-FIXED.md) (`document.fonts.ready` не резолвится):
`<body onload="document.fonts.ready.then(() => { checkLayout('.grid'); })">`
в каждом файле категории.

Обновлённая доля объяснённых TIMEOUT по срезу 1 WPT-RUN-6 (2296/6205, 37 %)
растёт минимум на 307 — `editing/` не входил в срезы 1-2. `css-grid/alignment`
не добавляется отдельно — он уже внутри 384/452 по идиоме `fonts.ready`,
посчитанной по всему корпусу в срезе 1.


## Замер 2026-08-23 (WPT-RUN-6, срез 25): `defaultView` действительно отсутствует, но падает не он первым

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant testdriver-click-path`
(dev-release, Linux, `main` = `530d0a444`) подтверждает `typeof
document.defaultView === 'undefined'` — баг открыт и актуален. Но тот же
замер показывает, что элемент-адресованный `test_driver`-экшен до
`testdriver-extra.js::get_context` **не доходит**: `resources/testdriver.js::click`
раньше зовёт `element.getClientRects()`, которого нет
([BUG-478](BUG-478-OPEN.md)/[BUG-551](BUG-551-DUPLICATE.md)/[BUG-580](BUG-580-DUPLICATE.md)),
и бросает синхронно:

```
tdc-api getClientRects=undefined elementsFromPoint=false elementFromPoint=false
        defaultView=undefined contains=true
tdc-throws TypeError: el.getClientRects is not a function
```

Практический вывод: починка одного `defaultView` не разблокирует ни одного
`test_driver.click`-теста — нужны все три звена цепочки
(`getClientRects` → `elementsFromPoint` → `defaultView`).

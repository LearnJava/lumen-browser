# BUG-478: `Element.prototype.getClientRects()`/`getBoxQuads()` missing (only `Range` has `getClientRects`)

**Статус:** OPEN (ДОРАБОТКА → [GAP-GEOM](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-GEOM` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

```
FAIL sub element in a child inline box should not be included
  target.getClientRects is not a function
FAIL Element getClientRects()
  document.getElementById(...).getClientRects is not a function
```

## Причина

`grep -n "getClientRects" crates/js/src/dom.rs` finds it defined exactly
once, on the `Range` object literal built by `_lumen_make_range`
(`dom.rs:6947`: `getClientRects: function() { return [this.getBoundingClientRect()]; }`).
The live `Element`/`Node` wrapper has no `getClientRects` at all — `'x' in
document.body` for that name is `false`, not a broken getter.

## Масштаб находки

12 subtests of `getClientRects` (`getClientRects-br-htb-ltr.html`,
`getClientRects-inline-inline-child.html`, `getClientRects-zoom.html`,
`DOMRectList.html`, `cssom-getClientRects.html`,
`cssom-getClientRects-002.html`) plus harness-level TIMEOUT on
`getClientRects-inline-atomic-child.html`; separately, `getBoxQuads()`
(CSSOM View §6, a distinct but structurally identical gap — same "is not a
function" symptom, same missing-entirely root cause) accounts for 7 more
subtests in `cssom-getBoxQuads-001.html`/`cssom-getBoxQuads-002.html`.

## Что нужно

Add `getClientRects()` to the live `Element`/`Range`/`Text` wrapper(s),
returning a `DOMRectList`-like array — for elements, one rect per CSS box the
element generates (a single-rect `[getBoundingClientRect()]` fallback is
spec-incomplete for multi-fragment inlines but would already unblock the
`is not a function` failures; a correct multi-fragment answer needs per-line
fragment rects from layout, which the box tree already has — see the
`InlineRun`/`frag[]` structure `--dump-layout` prints).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: FAIL`/`TIMEOUT` per the actual
run.


## Замер 2026-08-23 (WPT-RUN-6, срез 25): это блокирует `test_driver.click`, а с ним весь кластер `focus-visible-*`

`tests/wpt/verify_focus_mutation_animation_gaps.py --variant testdriver-click-path`
дословно повторяет то, что `resources/testdriver.js::click` делает **в
странице** до того, как дело дойдёт до исполнителя, и печатает первый
отказавший шаг (dev-release, Linux, `main` = `530d0a444`):

```
tdc-api getClientRects=undefined elementsFromPoint=false elementFromPoint=false
        defaultView=undefined contains=true scrollIntoView=function
tdc-throws TypeError: el.getClientRects is not a function
```

`click(element)` начинается с `inView(element)` →
`getPointerInteractablePaintTree(element)` → `element.getClientRects()`
(`resources/testdriver.js:52`). То есть вызов бросает **синхронно**, promise
не создаётся вовсе, `.then(() => done())` теста не выполняется, и в снимке
WPT-RUN-5 (до фикса BUG-591/716) исключение никуда не долетало — файл уходил
в TIMEOUT с пустым логом.

Важная поправка к готче в `CLAUDE.md`: элемент-адресованный `test_driver`-экшен
падает **не** на `document.defaultView` ([BUG-622](BUG-622-OPEN.md), это
следующий по порядку отказ, `testdriver-extra.js::get_context`), а раньше — на
этом баге. `elementsFromPoint`/`elementFromPoint` ([BUG-464](BUG-464-FIXED.md),
[BUG-477](BUG-477-DUPLICATE.md)) — третье звено той же цепочки: даже с
`getClientRects` дерево не построится.

**Масштаб этой грани:** механизм `testdriver-click-preconditions` в
`tests/wpt/timeout_audit.py` — 14 id остатка снимка WPT-RUN-5, из них 12 это
весь кластер `css/selectors/focus-visible-*`, плюс `focus/scroll-matches-focus.html`
и `html/semantics/forms/the-label-element/forward-focus-to-associated-element.html`.

## Слияние дубликатов 2026-09-02 (ре-триаж пула WPT-RUN-5/6)

[BUG-551](BUG-551-DUPLICATE.md) и [BUG-580](BUG-580-DUPLICATE.md) — тот же
отсутствующий метод, заведённый двумя другими срезами. Формально сведены сюда:
выживает первый по дате (эта запись, 2026-08-02), у тех статус
`DUPLICATE → BUG-478`, файлы переименованы в `-DUPLICATE.md`, восемь входящих
ссылок поправлены. Три записи на один `grep`-ноль — это и есть иллюстрация
правила [docs/probe-method.md §8](../docs/probe-method.md): пробел, заведённый
багом, переоткрывается на каждом следующем прогоне.

Уникальные замеры, перенесённые из них:

- **из BUG-551** (WPT-RUN-3 срез 33, `css/css-sizing`, 2026-08-03):
  `contain-intrinsic-size/auto-010.html` — 1 файл / 2 сабтеста, «Last remembered
  size» на многофрагментном элементе, то есть случай, где однорректный фолбэк
  `[getBoundingClientRect()]` заведомо не годится. Там же уточнение по коду:
  третье вхождение `getClientRects` в файле — заглушка `_CaretPosition`
  (`dom.rs:10516`), не `Element`.
- **из BUG-580** (WPT-VENDOR-html-semantics-misc, 2026-08-04): **76 сабтестов**,
  подавляющая часть — через хелпер `isVisible()` в
  `popovers/resources/popover-utils.js`. Сопутствующие пробелы, замеренные тем же
  проходом: `Document.prototype.elementFromPoint` (8 обращений) — с тех пор
  реализован ([BUG-464](BUG-464-FIXED.md)/[BUG-477](BUG-477-DUPLICATE.md),
  2026-09-01), и `Node.prototype.getRootNode()` (6 обращений) —
  [BUG-599](BUG-599-OPEN.md), остаётся багом: это один метод, а не семейство.

Суммарный масштаб после слияния: 12 сабтестов `cssom-view` + 7 `getBoxQuads` +
76 `html/semantics` + 2 `css-sizing`, плюс 14 id `testdriver-click-preconditions`,
которые упираются в него раньше всего остального.

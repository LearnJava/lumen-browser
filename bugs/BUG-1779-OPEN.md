# BUG-1779 — `Object.prototype.toString.call(<HTML-элемент>)` возвращает `[object Object]`

**Статус:** OPEN
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js (обёртки DOM-узлов, `Symbol.toStringTag`/классификация хост-объектов)

## Источник: сайты

mirtesen-ru (вес 3, `text_len` 2 при 9812 у Chrome), aviasales-ru (вес 2, 1 при 9026) — **2 сайта, вес кластера 5**. Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28.

Живой стек mirtesen: `_lumen_sel` ← `HTMLDocument.querySelectorAll` ← `Fc` (tippy.js, `packages-B_Rt485v.js:12`) ← `initTooltip` ← `componentDidMount` → `SyntaxError: [object Object] is not a valid selector`, React-приложение не монтируется, страница пуста.

## Причина (подтверждена)

tippy.js определяет «это элемент» через `{}.toString.call(x)` и ищет подстроку `Element]`. В Lumen для `div/span/input/html/head/Text/Comment/DocumentFragment/canvas/video` результат — `[object Object]` (SVG-элементы дают верное `[object SVGSVGElement]`, `document` — `[object HTMLDocument]`, `NodeList` — `[object NodeList]`). Элемент не узнаётся и уходит в `document.querySelectorAll(<элемент>)`.

## Минимальная репродукция

```html
<!doctype html><div id=a></div><pre id=out></pre>
<script>out.textContent = {}.toString.call(a) + ' ' + {}.toString.call(document.head) + ' ' + {}.toString.call(document.createTextNode(''));</script>
```

`lumen --dump-layout` такого файла печатает `[object Object]` для каждого элемента.

## Ожидание (Chrome)

`[object HTMLDivElement] [object HTMLHeadElement] [object Text]` (`Symbol.toStringTag` на прототипах интерфейсов: `HTMLDivElement`, `Text`, `Comment`, `DocumentFragment`, `HTMLHeadElement`, …).

## Где искать

Прототипы обёрток DOM-узлов в `crates/js` (V8-обёртки и shim `web_api_shim_*.js`): у SVG-элементов тег уже есть — свести остальные к той же схеме.

## Как проверить

Репродукция выводит `[object HTMLDivElement] …`; mirtesen.ru и aviasales.ru в живом окне не пишут `is not a valid selector`.

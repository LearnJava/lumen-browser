# BUG-1558 — `getClientRects`/`getBoundingClientRect`: пустой inline-элемент — нулевой прямоугольник, `Range.getClientRects` одна рамка на многострочный текст, `DOMRectList` не `[object DOMRectList]`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout/js (`crates/engine/layout/src/lib.rs::collect_layout_rects`, `_lumen_get_bounding_rect`; `Range.getClientRects`)

## Симптом

Пустой `<span>` в строке абзаца: `getBoundingClientRect()` — `[0,0,0,0]`, `getClientRects().length` — `0` (CSSOM View §«getClientRects»: один пустой `DOMRect` у позиции в строке, высота — высота строки); `offsetLeft` пустого inline — `0` (тест ждёт `16`/`37.27`/`60.44`); `Range.selectNodeContents(span).getClientRects().length` — `1` при `span.getClientRects().length` `2` на двухстрочном тексте, `<br>` — нулевой прямоугольник, `offsetParent` у `position: fixed` и `body`/`html`. 33 id (69 из 91 сабтеста).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<p style="font-size:20px">abc <span id=s></span> def</p>` — `s.getBoundingClientRect()`, `s.getClientRects().length` | `[0,0,0,0]`, 0 | x≈44, высота строки, 1 |
| двухстрочный `<span>` — `getClientRects().length` и `Range.selectNodeContents(span).getClientRects().length` | 2 и 1 | 2 и 2 |
| `Object.prototype.toString.call(el.getClientRects())` | `[object Object]` | `[object DOMRectList]` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom-view/{getClientRects-*,getBoundingClientRect-*,offsetTopLeft-*,offsetTop-offsetLeft-*,offsetParent*,range-*,DOMRectList}.html` (33 id).

## Что делать

Фрагменты inline-боксов для пустых элементов и `<br>`, `Range.getClientRects` по фрагментам текста, `DOMRectList`, `offsetParent` по спецификации.

## Как проверить

`css/cssom-view/getBoundingClientRect-empty-inline.html`, `getClientRects-inline.html`.

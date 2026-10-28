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

## Срез 1 (2026-10-28, P3) — влит

Сделано:

- `DOMRectList`/`DOMRect`/`DOMRectReadOnly`/`DOMPoint*`/`DOMMatrix*`/`DOMQuad` — `@@toStringTag` (`[object DOMRectList]`).
- Пустой inline-элемент (`<span></span>`) получает место: layout оставляет нулевой сегмент-маркер (`InlineSegment::is_empty_inline_marker`), `for_each_empty_inline_element` ставит нулевой по ширине прямоугольник высотой в строку после предыдущего текста (за схлопнутым пробелом — в начале следующего фрагмента) или в начале следующего текста; `getBoundingClientRect`/`getClientRects`/`offsetLeft`/`offsetTop` больше не нули. Фазовый случай (рана нет) не строится.
- `Range.getClientRects()`/`getBoundingClientRect()` по спецификации: прямоугольники содержащихся элементов (`display: contents` — через детей) и выделенных частей строк каждого Text (`_lumen_text_frag_rects`; мягкий перенос без ширины, границы внутри суррогатной пары сдвигаются наружу); таблица фрагментов теперь включает поддеревья `content-visibility: hidden`.
- `offsetParent`: любой `<body>` и безрамочный элемент (`display: none`-поддерево) — `null`; `position: fixed` — ближайший предок, образующий containing block (`transform`/`perspective`/`filter`/`backdrop-filter`/`transform-style: preserve-3d`/`contain: paint|layout`/`will-change`), иначе `null`; `perspective` и `transform-style` опубликованы в computed-style.

WPT `css/cssom-view` (файлы `DOMRectList`, `getBoundingClientRect-*`, `getClientRects-*`, `offset*`, `range-*`, `cssom-get*Rect*`): +25 сабтестов к базе того же main (`offsetTopLeft-empty-inline*`, `offsetParent-body-and-html`, `offsetParent-fixed`, `DOMRectList`, три `range-*`, `getBoundingClientRect-newline`). Display list 194 страниц корпуса (`graphic_tests`, `samples`) — без изменений; `css/CSS2/linebox`, `css/CSS2/normal-flow`, `css/css-display` — без регрессий.

Остаток:

- Пустой inline в фазовой строке (`<span></span>` единственный в блоке): `getBoundingClientRect-empty-inline-002` ждёт `(8, 8, 0, 0)`, ран не строится — нужен ран без строк и без влияния на схлопывание полей.
- `getBoundingClientRect-empty-inline` — пустой `<span>` рядом с `inline-block` на одной строке (базовая линия, `y` 92); раскладка строк однородной высоты.
- `<br>`: прямоугольник в rtl/вертикальных режимах (`getClientRects-br-*-rtl`), `getClientRects-inline-atomic-child`/`-inline-child`/`-with-block-child`.
- Ширина части фрагмента пропорциональна длине (нет посимвольных advance) — `getBoundingClientRect-shy`, `getBoundingClientRect-svg` (`<tspan>`), `*-zoom`.
- `offsetParent_element_test` (`<svg>` без `offsetParent`), `offsetTopLeft-trailing-space-inline`, `offsetTop-offsetLeft-nested-offsetParents`/`-with-zoom`.

# BUG-1542 — Размытие от `filter: blur()` выходит за границу `overflow:hidden` предка

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs` — `PushFilter` с `blur` внутри `PushClipRect` предка с `overflow:hidden`)

## Симптом

`overflow:hidden` блок 100×100 с потомком `filter:blur(20px)` 100×100: пиксели размытия справа и слева от клипа закрашены `(155,155,255)` и `(125,125,255)` (ожидается фон страницы — клип обрезает отфильтрованный результат). 3 reftest; `will-change-blur-filter-under-clip` и `fixed-pos-filter-clip-002` отнесены сюда по именам и тексту (клип + `blur`), пробой не проверены.

## Проба

Проба (`--screenshot`, `<div style="position:absolute;top:10px;left:10px;width:100px;height:100px;overflow:hidden;background:red"><div style="width:100px;height:100px;background:blue;filter:blur(20px)">`):

| точка | у нас | ожидается |
|---|---|---|
| `(5,50)` — вне клипа слева | `(125,125,255)` | `(255,255,255)` |
| `(115,50)` — вне клипа справа | `(155,155,255)` | `(255,255,255)` |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects/blur-clip-stacking-context-001` (проба), `fixed-pos-filter-clip-002`, `will-change-blur-filter-under-clip`.

## Что делать

Применять клип предка к результату фильтра (область рисования слоя фильтра = пересечение с текущим клипом).

## Как проверить

`css/filter-effects/blur-clip-stacking-context-001.html`.

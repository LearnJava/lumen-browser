# BUG-1543 — `filter` на `<span>` не применяется к вложенному float/блоку (блок-в-инлайне)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout/paint (`filter` на `display:inline` элементе с блочным/плавающим потомком)

## Симптом

`<span style="filter:blur(2px)"><div style="float:left;width:100px;height:100px;background:green">`: край float остаётся резким (пиксель `(98,50)` — `(0,128,0)`, `(104,50)` — белый), хотя фильтр должен размыть его (CSS Filter Effects 1 §«filter»: `filter` применяется к inline-боксу и его потомкам, включая float/блок). 2 reftest (`filtered-inline-applies-to-float`, `filtered-inline-is-container`).

## Проба

Проба (`--screenshot`, `blur(2px)`, пиксели строки `y=50` на `x = 96, 99, 100, 101, 104`):

| разметка | у нас | ожидается |
|---|---|---|
| `<div style="filter:blur(2px)">` + float | `(20,138,20) (108,182,108) (147,201,147) (184,220,184) (247,251,247)` | то же |
| `<span style="filter:blur(2px)">` + тот же float | `(0,128,0) (0,128,0) (255,255,255) (255,255,255) (255,255,255)` | как в строке выше |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects/filtered-inline-applies-to-float.html`, `filtered-inline-is-container.html`. Родственный [BUG-1472](BUG-1472-OPEN.md) (`position:relative` на inline с блоком внутри).

## Что делать

Строить для inline-бокса с блочным потомком фрагменты-слои фильтра по каждому фрагменту (блок-в-инлайне) либо считать такой inline контейнером для `filter`.

## Как проверить

`css/filter-effects/filtered-inline-applies-to-float.html`.

# BUG-1485 — Якорь с `transform` берётся по непреобразованному прямоугольнику: `anchor()` и `position-area` не следуют за `translate`/`scale`/`rotate` якоря

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/anchor.rs` — `collect_anchors`: геометрия якоря не учитывает его `transform`)

## Симптом

CSS Anchor Positioning 1 §2.2: якорь с трансформацией даёт границы по преобразованному border box (ограничивающий прямоугольник), а не по раскладочному. У нас цель `left:anchor(--a right); top:anchor(--a bottom)` стоит в `(90,80)` при любом `transform` якоря (`none`, `translateX(100px)`, `scale(2)`, `rotate(45deg)`), хотя `getBoundingClientRect` якоря уже преобразован (`(150,50)`, `(50,50,80,60)`, `(28.8,50,49.5,49.5)`). 17 id `css-anchor-position/transform-001…017.html`: 9 reftest `thick` и 8 TIMEOUT (`transform-010…017` ждут `ResizeObserver` на цели, чей размер при анимации `transform` якоря никогда не меняется — следствие того же).

## Проба

Проба (`--mcp`, контейнер 400×300, якорь `left:50;top:50;40×30`, `transform-origin:0 0`):

| `transform` якоря | якорь `getBoundingClientRect` | цель у нас | ожидается (правый-нижний угол якоря) |
|---|---|---|---|
| `none` | `(50,50,40,30)` | `(90,80)` | `(90,80)` |
| `translateX(100px)` | `(150,50,40,30)` | `(90,80)` | `(190,80)` |
| `scale(2)` | `(50,50,80,60)` | `(90,80)` | `(130,110)` |
| `rotate(45deg)` | `(28.8,50,49.5,49.5)` | `(90,80)` | `(78.3,99.5)` |

## Как найдено

WPT-RUN-14 срез 21: `css-anchor-position/transform-001.html`…`transform-017.html`.

## Что делать

В `collect_anchors` брать для якоря с `transform` ограничивающий прямоугольник преобразованного border box (та же матрица, что у `getBoundingClientRect`/paint), пересчитывать при анимации `transform`.

## Как проверить

Таблица выше; `css/css-anchor-position/transform-001.html`.

# BUG-1343 — Плитка `background-image` на полупиксельной позиции (`center`, `bottom` при нечётной разности) рисуется со швами по 1 px между плитками

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 11, `css/CSS2` (backgrounds + borders))
**Область:** paint (`crates/engine/paint/src/cpu_raster.rs` — `DrawBackgroundImage`, повтор плитки; позиция плитки не привязана к целым пикселям)

## Симптом

Фон `url(blue15x15.png) repeat-x` в блоке 200 px шириной, `background-position`:

| позиция | пиксели по x в строке картинки |
|---|---|
| `10px bottom`, `92px bottom` | сплошная синяя полоса |
| `center bottom` (x = 92.5) | на каждой границе плитки (x = 2, 17, 32, …) `rgb(63,63,255)` — шов |
| `92.5px bottom` | то же |

Плитка 15 px на дробном `x` растрескивается с полупрозрачной кромкой, соседние плитки не перекрываются.

## Как найдено

WPT-RUN-14 срез 11: 30 id `backgrounds/background-{043,048,055,…,196}.xht` — `background: url(blue15x15.png) repeat-x bottom` (то есть `center bottom`), расхождение 596 px — 14 швов. Эталон — одна картинка, растянутая на ширину.

## Что делать

Привязать начало плитки и шаг к целым device-пикселям (CSS Backgrounds §3.9 «Background Painting»: при `repeat` пиксельно-точная плитка `background-position` округляется). Тот же приём, что BUG-1337.

## Как проверить

`css/CSS2/backgrounds/background-043.xht`.

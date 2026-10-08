# BUG-1427 — Градиент с одной цветовой остановкой (`linear-gradient(green)`, `radial-gradient(green)`, `repeating-linear-gradient(green)`) не рисуется

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images + css/css-values + css/css-color`)
**Область:** layout/paint (разбор градиентов с одной остановкой)

## Симптом

`--screenshot`, `background-image:<значение>` в бокс 100×10, пиксель x=5:

| значение | пиксель |
|---|---|
| `linear-gradient(green)` | `(255,255,255)` |
| `linear-gradient(to right, green 50%)` | `(255,255,255)` |
| `radial-gradient(green)` | `(255,255,255)` |
| `repeating-linear-gradient(green)` | `(255,255,255)` |
| `conic-gradient(green)` | `(0,128,0)` — верно |
| `linear-gradient(green, green)` (контроль) | `(0,128,0)` |

В `gradient-single-stop-001.html` под тестовым `div` лежит красный `div` — остаётся виден: `red 10000 px` в снимке вместо 0.

## Как найдено

WPT-RUN-14 срез 19: `gradient-single-stop-001…005.html` — FAIL (`thick`); подтверждено пробой по трём функциям.

## Что делать

Разрешить `<color-stop-list>` из одного элемента (сейчас нужно ≥ 2) для линейного, радиального и повторяющихся вариантов;
конический уже допускает.

## Как проверить

`css/css-images/gradient/gradient-single-stop-001.html` … `005.html`.

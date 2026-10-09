# BUG-1587 — `offset-distance: <percentage>` считается от диагонали самого бокса, а не от длины пути; замкнутый путь не оборачивается

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** layout (`resolve_motion_transform` в `crates/engine/layout/src/style/` — `property_trees`; база процентов у `offset-distance`)

## Симптом

Процентное `offset-distance` пересчитывается не от длины `offset-path`, а от размера самого элемента: смещение вдоль `path()` на `100%` равно длине диагонали бокса. Для `ray()` проценты считаются от размера содержащего блока (`ray-size`), а у нас — тоже от бокса. Абсолютные значения (`px`) работают: `offset-distance: 100px` → `x=100`.

## Проба

`--dump-layout` + `console.log(getBoundingClientRect())`, `.t{position:absolute;left:0;top:0;width:10px;height:10px;transform-origin:0 0}`, `offset-rotate:0deg`:

| `offset-path`, `offset-distance` | `x, y` у нас | ожидается |
|---|---|---|
| `path("M0 0 h 200")`, `50%` | `7.1, 0` | `100, 0` |
| `path("M0 0 h 200")`, `100%` | `14.1, 0` | `200, 0` |
| `path("M0 0 h 400")`, `100%` | `14.1, 0` (то же — длина пути не участвует) | `400, 0` |
| `path("M0 0 h 200")`, `100px` | `100, 0` | `100, 0` |
| `path("m 0 0 h 200 v 150 z")`, `601px` (периметр 600) | `0, 0` | `1, 0` |
| `path("m 0 0 h 200 v 150 z")`, `120%` у бокса 100×40 | `129.2, 0` | периметр 200+150+250=600 → 720 px → `120, 0` |
| `ray(90deg closest-side)`, `left:100px;top:100px`, `offset-position:auto`, `100%` | `114.1, 100` (то же `√(10²+10²)` поверх `left:100px`) | сдвиг на длину луча до границы по `closest-side` |

Из 25 reftest `offset-path-ray-*` 24 используют `offset-distance: N%` (остальной — `ray-011`, пиксельный `offset-distance`); проверено только, что процент считается от бокса. Есть ли отдельный дефект у `ray-size`/`contain`/`at`, станет видно после правки — сейчас они закрыты этим.

## Как найдено

WPT-RUN-14 срез 27: `css/motion/offset-path-ray-001…018`, `offset-path-ray-contain-001…005`, `offset-distance-001…008`.

## Что делать

Привести базу процента к CSS Motion 1 §«offset-distance»: для `path()`/basic-shape/`url()` — длина пути, для `ray()` — длина луча до границы по `ray-size`; для замкнутого пути — `distance mod length`. После правки перемерить остальные `ray-*` (там может остаться дефект `ray-size`/`contain`).

## Как проверить

`css/motion/offset-distance-002.html`, `offset-path-ray-006.html`, `offset-path-ray-contain-004.html`.

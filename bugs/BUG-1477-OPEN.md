# BUG-1477 — Якорь — разорванный inline (несколько строк / колонок) или внутри multicol: берётся один прямоугольник, а не объединение фрагментов

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/anchor.rs` — `collect_anchors`, геометрия якоря-`inline` со многими боксами/строками)

## Симптом

`anchor-name` на `inline` элементе, обёрнутом на несколько строк или колонок, должен давать якорю ограничивающий прямоугольник всех его фрагментов (CSS Anchor Positioning 1 §2.2; WPT `anchor-name-inline-001.html`: ожидается ширина 30, у нас 0). Внутри `multicol` якорь и цель живут в разных колонках, раскладка цели считается от неверной колонки. 32 id среза: 22 `anchor-position-multicol-*`/`anchor-name-multicol-*`/`anchor-position-multicol-colspan-*` (23 `thick` + 9 testharness с 25 из 35 сабтестов) и 10 `anchor-position-inline-*`/`anchor-name-inline-*`. Для inline-якоря проба показала: `<span id=anc style="anchor-name:--a">YYY</span>` даёт `anc` в `(51.8,0,32.6,16)` и цель `left:anchor(--a left)` в `(0,16,5,5)` — то есть левая сторона якоря берётся как `0`, а не `51.8`; для `inline-block` (51,0) и для блока (30,0) верно. Для `multicol` проба не проводилась — отнесено по имени.

## Проба

Проба (`--mcp`, Ahem 16 px, `padding-left:30px` у строки): цель `position:absolute;left:anchor(--a left);top:anchor(--a bottom)`:

| якорь | `anc` | цель у нас | ожидается |
|---|---|---|---|
| `<span style="anchor-name:--a">YYY</span>` | `(51.8,0,32.6,16)` | `(0,16)` | `(51.8,16)` |
| `<div style="anchor-name:--a;display:inline">` | `(51.8,0,32.6,16)` | `(0,16)` | `(51.8,16)` |
| `<span style="anchor-name:--a;display:inline-block">` | `(51,0,32.6,16)` | `(51,16)` | `(51,16)` |
| блок `margin-left:30px;width:40px;height:20px` | `(30,0,40,20)` | `(30,20)` | `(30,20)` |

## Как найдено

WPT-RUN-14 срез 21: `css-anchor-position/anchor-name-inline-001.html`, `anchor-position-inline-002.html`, `anchor-position-multicol-001.html`.

## Что делать

Брать для inline-якоря объединённый прямоугольник строковых фрагментов (как `getBoundingClientRect`); для multicol — фрагмент, содержащий цель, по правилам §2.3.

## Как проверить

Таблица выше; `css/css-anchor-position/anchor-name-inline-001.html`.

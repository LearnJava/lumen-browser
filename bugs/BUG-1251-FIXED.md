# BUG-1251 — Статическая позиция `position:absolute` ребёнка flex-контейнера (CSS Flexbox L1 §4.1) не учитывает `justify-content`/`align-items`/`align-self`/`align

**Статус:** FIXED 2026-10-04 (P1, FLEX-VWM; остаток — BUG-1265)
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** layout (`crates/engine/layout/src/box_tree/flex.rs` — статическая позиция абсолютно позиционированного ребёнка flex-контейнера)

## Симптом

Статическая позиция `position:absolute` ребёнка flex-контейнера (CSS Flexbox L1 §4.1) не учитывает `justify-content`/`align-items`/`align-self`/`align-content` контейнера: бокс всегда лежит в углу content-box. Пример: контейнер `100×60 display:flex; justify-content:center; align-items:center`, ребёнок `10×10` — `rect=(0,0,…)` вместо `(45,25)`; с `flex-end` — тоже `(0,0)`. WPT-RUN-14-S1: 32 id `css/css-flexbox/abspos/flex-abspos-staticpos-*` (514 упавших сабтестов, `OK`-тесты с `checkLayout`) плюс `-rtl`/`-vertWM` варианты.

## Описание

Проба `--dump-layout`:

```html
<div style="display:flex;position:relative;width:100px;height:60px;justify-content:center;align-items:center"><div style="position:absolute;width:10px;height:10px"></div></div>
```

ребёнок `rect=(0,0,10,10)`, ожидается `(45,25,10,10)`.

## Как найдено

WPT-RUN-14 срез 1, кластер «abspos static position» (32 id, 514 сабтестов — крупнейший по сабтестам среди layout-кластеров). Вертикальные writing-mode варианты зависят ещё и от BUG-1258.

## Как проверить

`run_report.py --root css/css-flexbox/abspos --recursive`.

## Исправлено

FLEX-VWM (2026-10-04): `flex_trampoline::align_abs_static_positions` двигает abspos-ребёнка, у которого инсеты по оси `auto` с обеих сторон, из угла
content box туда, куда `justify-content` (главная ось) / `align-self` (поперечная; `auto` → `align-items`) поставили бы единственный item: `start`/`center`/`end`,
`space-around`/`space-evenly` как `center`, `safe` с откатом к writing-mode start, `baseline`/`last baseline` как `start`/`end`; оси физические — через `flex_axes`
(в том числе `-rtl`/`-vertWM`). Для `safe` переполнение считается против containing block ребёнка (внешний positioned-предок, если сам flex им не является).
WPT `css/css-flexbox/abspos`: 367 → 1382 из 1400 сабтестов.

Остаток 18 сабтестов — `align-self: self-start | self-end` (относительно writing-mode самого ребёнка) в `*-rtl-004`/`*-vertWM-002/004`, и ещё `abspos-descendent-001`,
`position-absolute-002`, `dynamic-grid-flex-abspos` — см. BUG-1265.

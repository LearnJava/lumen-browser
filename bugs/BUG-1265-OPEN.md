# BUG-1265 — `self-start`/`self-end` и `safe` у выравнивания самих flex-items и статической позиции абсолютных детей

**Статус:** OPEN
**Заведён:** 2026-10-04 (P1, FLEX-VWM)
**Область:** layout (`crates/engine/layout/src/style/values/flexgrid.rs` — `AlignValue::parse`; `box_tree/flex_trampoline.rs`)

## Симптом

`AlignValue` склеивает `start` / `flex-start` / `self-start` в один `Start`, а `end` / `flex-end` / `self-end` в один `End`. FLEX-VWM разделил `start`/`end` (writing-mode контейнера) и `flex-*`
для `justify-content`/`align-content`/`align-self`/`align-items` через флаги `ContentAlignExtra`, но:

1. `self-start`/`self-end` берутся относительно writing-mode и `direction` **самого item** (у ортогонального ребёнка — относительно его осей), а не контейнера. Сейчас они считаются как `start`/`end` контейнера.
   Падает в `abspos/flex-abspos-staticpos-align-self-rtl-004.html` и `-vertWM-002/004` (по 4 сабтеста).
2. `safe` у `align-items`/`align-self` учитывается только в статической позиции abspos-ребёнка; у обычных flex-items (`finish_line`, `column_item_cross_shift`) `safe center`/`safe end` при переполнении не откатываются к start.
3. `place-content`/`place-items`/`place-self` разбирают токены по одному и не знают `safe`, `left`, `right`.

## Описание

Мелочь по числу тестов (≈ 18 сабтестов в `abspos/`), но флаги `…_wm`/`…_safe` — костыль: честнее хранить в `AlignValue` вариант `{ keyword, overflow, relative_to }`.

## Как найдено

FLEX-VWM: остаток после замера `css/css-flexbox/abspos` (1382 из 1400 сабтестов).

## Как проверить

`flex-abspos-staticpos-align-self-rtl-004.html`, `-vertWM-002.html`, `-vertWM-004.html` (`check-layout-th`) — все сабтесты `PASS`.

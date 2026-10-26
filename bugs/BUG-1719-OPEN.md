# BUG-1719 — остаток BUG-1528: статическая позиция abspos в inline-элементе и в вертикальных режимах письма, `anchor-center`

**Статус:** OPEN
**Заведён:** 2026-10-26 (P3, остаток [BUG-1528](BUG-1528-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/abs_static_align.rs`, `inline_build.rs`, `vertical_trampoline.rs`; парсер `align-self: anchor-center` — [BUG-1524](BUG-1524-OPEN.md))

## Симптом

`css/css-align/abspos/`: `align-self-static-position-002.html` (вертикальные режимы письма, было 28750 px, стало 15000), `-003.html`, `-004.html`, `-007.html` (абсолютный бокс внутри `display:inline`; 19300–20000 px) и `-008.html` / `justify-self-static-position-001.html` (остаётся один бокс `anchor-center`, 2500 / 1500 px).

## Что известно

1. Бокс внутри inline-элемента сборка выносит в `children` блока-контейнера рядом с `InlineRun` — по дереву не отличить от прямого ребёнка. Прямоугольник статической позиции у него — точка в строке по обеим осям (`end` → `left: -50px` относительно линии начала), а не content box; `abs_static_align::align_static_positions` поэтому пропускает контейнеры со строчным содержимым (стартовая позиция, как раньше). Нужна пометка бокса при выносе (`build.rs`, цикл по `row_items`) или `BoxOrigin`-роль.
2. `vertical_trampoline.rs:302` складывает `abs_deferred` по тому же формату, но выравнивание там не вызывается; ось строки — y.
3. `anchor-center` без якоря равен `center`; значение пока не разбирается (`BUG-1524`).

## Как проверить

`align-self-static-position-002/-003/-004/-007/-008.html`, `justify-self-static-position-001.html` через `tests/wpt/reftest_pixdiff.classify`.

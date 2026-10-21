# BUG-1664 — `cq*`-единицы в свойствах, резолвимых при каскаде (`tab-size`, `font-size`, `letter-spacing`), не видят контейнера

**Статус:** OPEN
**Заведён:** 2026-10-21 (P3, остаток [BUG-1438](BUG-1438-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/container_anchor.rs` — `apply_container_styles`, `re_style_subtree`; `style/apply/text.rs` — ветка `tab-size`; `style/math_value.rs` — `leaf`)

## Симптом

`tab-size: calc(10 + (sign(2cqw - 10px) * 5))` внутри `container-type: inline-size; width: 100px` должен вычислиться в `5`, `calc(10px + (sign(2cqw - 10px) * 5px))` — в `5px`. Получаем `8` (начальное значение): при каскаде `CONTAINER_CQ` не установлен, `cqw` не резолвится, объявление отбрасывается.

WPT: `css/css-text/parsing/tab-size-computed.html` — 2 сабтеста.

## Причина

`cq*`-контекст ставится только на время перекладки контейнера (`set_cq_context` в `apply_container_inner`), а сам проход `apply_container_styles` выходит рано, если в таблице стилей нет `@container`-правил, и перестилизует потомков только по `@container`-правилам. Свойства, которые резолвятся в px при каскаде (а не хранят `Length` до раскладки, как `width`), контейнера не знают.

## Что делать

Запоминать в стиле исходное значение объявления с `cq*` (отложенный резолв), а в проходе по контейнерам — ставить `set_cq_context` и перерезолвить его (для этого проход не должен выходить рано, когда в дереве есть такие стили). Та же причина у `font-size: 5cqw` вне `@container`.

## Как проверить

`run_report.py --all --root css/css-text/parsing` (`tab-size-computed`).

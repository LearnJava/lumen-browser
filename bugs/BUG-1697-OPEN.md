# BUG-1697 — `getComputedStyle`: `min-*`/`max-*` размеры в viewport-единицах не абсолютизируются

**Статус:** OPEN
**Заведён:** 2026-10-23 (P3, при BUG-1505)
**Область:** layout (`crates/engine/layout/src/selector_query.rs` — `sizing_computed_css`, `length_to_css`)

## Симптом

`getComputedStyle(el).minHeight` при `min-height:13vw` возвращает `13vw`; `height:13vw` даёт `133.12px`. Computed-значение длины с `vw`/`vh`/`vmin`/`vmax` — абсолютная длина. `sizing_computed_css` резолвит `em` и `calc()` с `Size::ZERO` вместо viewport, а прочие viewport-единицы печатает как есть.

## Проба

`t.style='min-height:13vw'; getComputedStyle(t).minHeight` → `13vw` (ожидается `133.12px` при окне 1024 px); то же для `min-width`; `height:13vw` → `133.12px`.

## Как найдено

`css/css-properties-values-api/registered-property-computation.html`: эталон `length_ref('13vw')` читает `min-height`.

## Что делать

Пробросить viewport в сборщик computed-значений (`collect_computed_styles`) и резолвить `Vw/Vh/Vmin/Vmax` (и в `calc()`) в `sizing_computed_css`.

## Как проверить

Проба выше; три подтеста `registered-property-computation.html` с `vw`/`vmin`/`vmax`.

# BUG-1664 — `cq*`-единицы в свойствах, резолвимых при каскаде (`tab-size`, `letter-spacing`, `word-spacing`), не видят контейнера

**Статус:** FIXED 2026-11-15
**Заведён:** 2026-10-21 (P3, остаток [BUG-1438](BUG-1438-FIXED.md))
**Область:** layout (`crates/engine/layout/src/box_tree/container_anchor.rs` — `apply_container_styles`, `re_style_subtree`; `style/apply/text.rs`; `style/apply.rs`; `style/computed.rs`)

## Симптом

`tab-size: calc(10 + (sign(2cqw - 10px) * 5))` внутри `container-type: inline-size; width: 100px` должен вычислиться в `5`, `calc(10px + (sign(2cqw - 10px) * 5px))` — в `5px`. Получаем `15`/`15px`: `cqw` разрешался по viewport (1024 px → 20,48), потому что `CONTAINER_CQ` при каскаде не установлен.

WPT: `css/css-text/parsing/tab-size-computed.html` — 2 сабтеста.

## Причина

`cq*`-контекст ставится только на время перекладки контейнера (`set_cq_axes` в `apply_container_inner`), а сам проход `apply_container_styles` выходил рано, если в таблице стилей нет `@container`-правил, и перестилизовывал потомков только по `@container`-правилам. Свойства, которые резолвятся в px при каскаде (а не хранят `Length` до раскладки, как `width`), контейнера не знают. Отдельная грань: `substitute_numeric_math` (BUG-1430) вычислял числовой `calc()` в литерал ещё до разбора свойства, так что от `cq*` оставался только viewport-fallback — и `tab-size` принимал его за `<number>`.

## Что сделано

- `ComputedStyle::cq_lengths` — исходные `Length` объявлений с `cq*` для `tab-size`/`letter-spacing`/`word-spacing` (третий элемент — значение числовой размерности). Заполняется в `apply_declaration` (`note_cq_length`), не наследуется: px родителя абсолютен.
- `ComputedStyle::resolve_cq_lengths` перерезолвит их с явной парой осей контейнера (`re_style_subtree`, `StyleSweep::run`, `restyle_boxless`), не трогая ambient `CONTAINER_CQ` — он восстанавливается через `cq_axes`/`restore_cq_axes`.
- `cq_axes_of` — базис `cq*` из цепочки `ContainerContext`: ширина ближайшего контейнера, содержащего inline-размер, высота — ближайшего, содержащего block-размер.
- `apply_container_inner` ставит `set_cq_axes` до рестайла (теперь `cq*` внутри `@container`-объявлений тоже считается по контейнеру), рестайл и `restyle_boxless` идут всегда, даже без `@container`-правил.
- `substitute_numeric_math` не подставляет литерал для `tab-size` со значением, текстово содержащим `cq`-единицу (`contains_cq_unit_text`); размерность (число или длина) определяет `math_value::numeric_math_is_number`.

`font-size: 5cqw` вне `@container` этой правкой не покрыт: вычисленный `font-size` наследуется и участвует в раскладке потомков, его перерезолв требует перекаскада поддерева — отдельная задача.

## Как проверить

`run_report.py --all --root css/css-text/parsing` (`tab-size-computed`).
Юнит-тесты: `cargo test -p lumen-layout --lib container_units` — `cascade_resolved_props_see_the_container` (`tab-size: 5`, `letter-spacing: 2px`), `tab_size_length_with_cq_resolves_against_container` (`5px`).

**Проверено:** `cargo test -p lumen-layout --lib` 6025 passed; `cargo clippy -p lumen-layout --all-targets -- -D warnings` чисто. Graphic goldens не затронуты: ни один golden не использует `cq*`-единицы (`graphic_tests/29-container-queries.html` — только `container-type`).

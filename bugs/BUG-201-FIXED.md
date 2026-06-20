# BUG-201

**Статус:** FIXED 2026-06-20
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs`)
**Тест:** TEST-82 (5.00% → 2.38% → KNOWN_DEBTORS BUG-128)

## Описание

SVG `<use>`: clone shapes/groups/symbols из `<defs>`, x/y offset, xlink:href,
nested chains. Несколько строк теста рендерились неверно: пропадали клоны,
звёзды-symbol не рисовались, масштабированный ряд уезжал по вертикали.

## Корни (три независимых дефекта)

1. **`<polygon>`/`<polyline>` не имели ветки рендера** в `process_svg_node` —
   попадали в `_ =>` (scan children, no shape). Звезда `<symbol>` (ряд 2b)
   состоит из `<polygon>` → не рисовалась вовсе. Фикс: новая ветка строит
   path `d`-строку из `points` (`parse_svg_points` + `points_to_path_d`) и
   рендерит через существующий `<path>`-пайплайн (polygon закрывает контур `Z`,
   polyline — нет).

2. **HTML5-парсер не самозакрывает `<use/>`** (это не void-элемент), поэтому
   соседние `<use>` после первого вкладывались как его DOM-дети. Ветка `<use>`
   рекурсировала в *target*, но не в собственные DOM-дети → рендерился только
   ПЕРВЫЙ клон каждого target (ряд 1: 1 из 2, ряд 3: 1 из 10 и т.д.). Фикс:
   ветка `<use>` (и `<polygon>`/`<polyline>`) теперь сканирует mis-nested
   siblings в `out`, как уже делали `rect`/`circle`. `<defs>`/`<symbol>` при
   прямом обходе больше не рендерятся (раньше `_ =>` рисовал их содержимое).

3. **element-transform применялся к doc-координатам.** В
   `lay_out_svg_element_position` origin вьюпорта `(ox,oy)` запекался в bbox
   ПЕРЕД применением composed-трансформа → `scale(0.75)` масштабировал и origin.
   Масштабированные клоны ряда 3 (`transform="scale(0.75)"`) уезжали с y≈347 на
   y≈260 (0.75·origin_y). Фикс: трансформ применяется в user-space, ПОТОМ
   маппинг user→document `(ox + x·sx, oy + y·sy)`. Для translate результат
   идентичен (translate коммутирует со сдвигом origin), для scale/rotate —
   корректен. Все существующие svg-transform тесты используют svg в (0,0), где
   старая и новая формулы совпадают.

## Итог

5.00% → 2.38%. Все клоны/группы/symbol/polygon/nested-chain + x/y offset +
scale совпадают с Edge по позиции/размеру/заливке (см. diff: остаются только
AA-периметры фигур, rule 2, и текст меток `.label`). Остаток 2.38% = font-parity
(Inter vs Edge, rule 3) → TEST-82 в `KNOWN_DEBTORS` (ref BUG-128).

Регресс-тесты (`box_tree.rs`): `parse_svg_points_handles_commas_and_spaces`,
`points_to_path_d_closes_polygon_but_not_polyline`,
`svg_polygon_renders_as_path_shape`, `svg_polygon_inside_symbol_renders_via_use`,
`svg_defs_children_do_not_render_directly`, `svg_use_multiple_siblings_all_clone`,
`svg_use_scale_transform_does_not_scale_viewport_origin`.

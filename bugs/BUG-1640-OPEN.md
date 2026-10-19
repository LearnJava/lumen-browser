# BUG-1640 — `perspective`: `inherit`, `perspective-origin: 0 0` и перспектива через inline-посредника

**Статус:** OPEN
**Заведён:** 2026-10-19 (P3, при закрытии [BUG-1290](BUG-1290-FIXED.md))
**Область:** layout (`crates/engine/layout/src/style/apply/motion.rs`, `crates/engine/paint/src/display_list/walk.rs::emit_push_perspective`)

## Симптом

Три WPT-reftest `css/css-transforms`, которые проходили, пока CPU-растр не рисовал перспективу вовсе (обе картинки были
одинаково неверны), и стали красными после BUG-1290:

1. `transform3d-perspective-002` — `perspective: inherit` на внутреннем `div` не даёт обёртку проекции:
   `--dump-display-list` показывает только `PushTransform` ребёнка. Эталон (`transform3d-perspective-001-ref`) —
   трапеция, получаем квадрат 100×100.
2. `transform3d-perspective-origin-001` — `perspective-origin: 0 0` рисуется иначе, чем `top left`
   (ограничивающий прямоугольник 0…104 против 0…114 при одинаковой разметке); дамп display list обе матрицы печатает
   одинаково (только 2D-часть). Подозрение: `parse_position_component` не принимает безразмерный `0` либо
   порядок ключевых слов `top left`.
3. `perspective-children-only-inline` — `perspective: 100px` на контейнере не должна действовать на внука, лежащего
   через inline-`div#middle`; обёртка охватывает всё поддерево, и `translateZ(-100px)` внука масштабируется вдвое, хотя
   по спеку он не ребёнок контейнера.

## Как проверить

`python .tmp/ref_cmp.py`-подобный прогон: снимок теста и эталона через `lumen --screenshot`, попиксельное сравнение.
Или `--dump-layout` + проба матрицы `perspective_matrix` в `property_trees.rs`.

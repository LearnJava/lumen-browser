# BUG-179 — flex auto flex-basis использует ширину контейнера вместо max-content

**Status:** FIXED 2026-06-17
**Crate:** lumen-layout
**File:** `crates/engine/layout/src/box_tree.rs:3932`

## Симптом

В TEST-46 (individual-transforms) второй столбец уезжал примерно на 160px правее ожидаемой позиции.

## Корень

Flex-item с `flex-basis: auto` и без явной `width` определял размер базы через предварительный
layout-проход: `item.rect.width`. Блочный элемент в предварительном проходе растягивается до
ширины контейнера, поэтому вся ширина контейнера становилась flex base size.

Пример: контейнер 600px, item A (`min-width: 200px`, без `width`), item B (`width: 100px`):
- Старый код: A.base = 600px → total_hyp = 700px > 600px → shrink → A.width ≈ 514px (неверно)
- Правильно: A.base = max-content (0px для пустого div) → clamped to min-width 200px → total_hyp = 300px < 600px → no shrink → A.width = 200px

## Фикс

Новая функция `flex_auto_base_main_width` (CSS Flexbox §9.2/§9.7):
1. Вычисляет `max_content_outer_width` элемента (без measurer текст = 0px)
2. Ограничивает `max-width` сверху
3. Ограничивает `min-width` снизу

Заменяет старую аппроксимацию через поиск дочернего элемента с явной шириной.

## Тест

`box_tree::tests::flex_auto_basis_item_with_min_width_uses_min_not_container_width`

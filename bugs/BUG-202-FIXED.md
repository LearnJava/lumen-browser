# BUG-202

**Статус:** FIXED 2026-06-17
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs`)
**Тест:** TEST-83 (14.02% → 7.88% → KNOWN_DEBTORS)

## Описание

TEST-83 («scroll-behavior») падал на 14%. Реальная причина — не scroll-behavior,
а **text-only inline-block без shrink-to-fit**.

`.pill { display: inline-block; padding: 4px 12px; }` со строковым содержимым
рисовался во всю ширину контейнера и складывался в столбик, а не обтягивал текст
и не тёк в ряд (как в Edge). Из-за раздувшейся `.root-demo` весь контент ниже
(`.api-demo`) уезжал вниз → диф во всю высоту страницы.

## Корень

`preferred_inline_block_width` (shrink-to-fit ширина inline-block) рекурсивно
измеряла только дочерние **боксы**. Текст `InlineRun` хранится в поле `segments`,
а не в `children`, поэтому для чисто-текстового inline-block функция возвращала
`content_w = 0` → `None` → в `lay_out` ветка shrink-to-fit (box_tree.rs:4817) не
срабатывала → бокс оставлял доступную (полную) ширину.

## Фикс

Добавлена ветка `BoxKind::InlineRun` в `preferred_inline_block_width`
(box_tree.rs:3732): preferred = max-content ширина текста (сумма ширин сегментов
без переноса), зеркало ветки `InlineRun` в `max_content_outer_width`. Теперь
text-only inline-block обтягивает текст, `InlineBlockRow` корректно течёт в ряд.

Регресс-тест `text_only_inline_block_shrinks_to_fit`.

## Остаток

7.88% = font-parity (Inter vs Edge, BUG-128, правило 3 graphic_tests) по всему
тексту страницы + faint overlay scrollbar. TEST-83 → `KNOWN_DEBTORS` (BUG-128, 7.88).

## Связанное

При разборе обнаружен отдельный дефект: scroll-контейнер в ordered
(stacking-context) пути не рисует scrollbar (`box_layer_ops` без `DrawScrollbar`).
Заведён как **BUG-220**.

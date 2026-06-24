# BUG-194

**Статус:** FIXED 2026-06-24 (DEBTOR — KNOWN_DEBTORS BUG-128)
**Компонент:** layout
**Тест:** TEST-65 (1.33% → 1.40%, KNOWN_DEBTOR)

## Описание

Заявлено как «Flexbox `align-content` multi-line deviation». Расследование показало,
что геометрия `align-content` во всех 7 кейсах (flex-start / flex-end / center /
space-between / space-around / space-evenly / stretch) **пиксель-в-пиксель совпадает
с Edge** (diff: заливки боксов чёрные). Все 6 элементов помещаются в одну flex-строку
(контент-ширина ≈472px > 6×50+5×4=320px), align-content применяется к single-line
контейнеру — корректно.

Настоящий дефект: **текст — прямой потомок flex-item** (`.item { display: flex }` с
текстом «1»…«6») — терялся. Белые цифры в Edge центрированы внутри голубых боксов;
в Lumen боксы были пустыми.

## Корень

1. `build_box` для `NodeData::Text` возвращает `BoxKind::Skip`; в ветке
   `is_item_container` (flex/grid/table) такой бокс отбрасывался → текст исчезал.
   По CSS Flexbox §4 / Grid §6 непрерывный текст-ран — прямой потомок flex/grid
   контейнера — должен оборачиваться в анонимный (blockified) item.
2. Кросс-выравнивание `align-items: center`/`end` двигало только `item.rect.y`, а
   поддерево item'а (его `InlineRun`) уже было позиционировано в абсолютных
   координатах и оставалось у кросс-старта (верх бокса). Цифра прилипала к верху.

## Фикс

- `build_anon_text_item` (box_tree.rs): оборачивает текст-ран прямого потомка
  flex/grid-контейнера в анонимный `Block`-item с `InlineRun`; whitespace/control-only
  раны item не порождают. Только для flex/grid (таблицы сохраняют свои anon-cell
  правила).
- Кросс-выравнивание (box_tree.rs ветки `Center`/`End`): сдвиг всего поддерева через
  `shift_y_box(item, new_y - item.rect.y)` вместо присваивания `item.rect.y` (та же
  логика, что BUG-165 в align-content).
- Регресс-тест `flex_text_child_is_wrapped_and_centered`.

## Остаток (DEBTOR)

TEST-65 1.40% = font-parity цифр + 7 подписей (`center`/`space-between`/… Arial vs
Inter, rule 3) + border-radius edge-AA. Класс BUG-128, в `KNOWN_DEBTORS`.

## Воспроизведение

`python graphic_tests/run.py --only 65` (после `--only 00` для калибровки).

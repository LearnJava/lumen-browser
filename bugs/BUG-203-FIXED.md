# BUG-203

**Статус:** FIXED 2026-06-20 (engine-дефект устранён; остаток TEST-84 → KNOWN_DEBTORS BUG-128, font-parity)
**Компонент:** paint
**Тест:** TEST-84 (8.20% → 6.02%)

## Описание

`text-decoration-skip-ink`: auto/none/all — underline gaps over glyph descenders.

## Корень

`emit_decoration_line_skip_ink` (`crates/engine/paint/src/display_list.rs`) строил
gap на **всю ширину ячейки глифа** (`char_w`) плюс margin `thickness + 1` с обеих
сторон. Для ряда последовательных descender'ов («gjpqy» во всех 4 рядах теста)
соседние gap'ы пересекались и сливались в один огромный gap, **стирая линию
целиком**:

- skip-ink: all — gap каждой ячейки покрывал всё → подчёркивание не рисовалось
  вовсе (на скриншоте Lumen — пустота);
- skip-ink: auto — descender-run «gjpqy» + большинство descender'ов в «Typography»
  съедали линию почти полностью; видны были лишь короткие огрызки.

## Фикс

Gap теперь клирит только **центральную ink-зону ячейки**, а не всю ячейку:
центр в середине advance, полуширина `char_w * 0.28 + thickness * 0.5`, кап
`char_w * 0.45` (≈56% advance). Между соседними скип-глифами остаётся видимый
сегмент линии — как в Edge. Подтверждено скриншотом: все 4 ряда подчёркиваний
(auto/none/all/thick) совпадают с эталоном, ряд «all» снова рисует штрихи под
каждым глифом.

Регресс-тесты (display_list.rs):
- `skip_ink_consecutive_descenders_keep_line_visible` — «gjpqy» не стирает линию;
- `skip_ink_all_does_not_erase_line` — skip-ink:all рисует сегменты.

## Остаток (DEBTOR)

6.02% = font-parity: Edge рендерит дефолтный serif (Times), Lumen — Inter sans;
48px-глифы расходятся по всей странице (CLAUDE.md rule 3 — текст не трекается).
TEST-84 → `KNOWN_DEBTORS` (BUG-128, baseline 6.02%).

## Воспроизведение

`python graphic_tests/run.py --only 84`

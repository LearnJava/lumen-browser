# Задача: Gap Decorations — спековые `column-rule-*`/`row-rule-*` для flex/grid

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:113` (BUG-553) · **Размер:** M · **Крейты:** `lumen-layout`, `lumen-paint`

## Цель
Flex/grid-ветка рисует по нестандартным `gap_rule_*`. Нужны имена из css-gaps-1.

## Точка входа
`crates/engine/paint/src/display_list/walk.rs:603`, `crates/engine/paint/src/gap_decorations.rs`, `crates/engine/layout/src/style/computed.rs:490-503`.

## Что сделать
`column-rule-*` (поля multicol уже есть) → вертикальные сегменты flex/grid; новые `row-rule-width/-style/-color` + шортхенды `row-rule`, `rule` → горизонтальные сегменты. Без `<gap-rule-list>`/`repeat()`.

## Готово, когда
Юнит в paint: grid 2×2, `row-rule: 2px solid red` → один горизонтальный `GapSegment`; `column-rule` → один вертикальный. Эталон `graphic_tests/73-gap-rule.html` перегенерирован в том же коммите.

## Гейт
`cargo clippy -p lumen-layout -p lumen-paint --all-targets -- -D warnings`; полный `python graphic_tests/run.py --continue-on-fail` + эталоны.

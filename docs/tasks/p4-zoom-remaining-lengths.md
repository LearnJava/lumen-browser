# Задача: `zoom` — немасштабируемые длины

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:188` · **Размер:** S–M · **Крейты:** `lumen-layout`

## Цель
`apply_zoom_to_lengths` не масштабирует `border-*-radius`, `outline-width`/`-offset`, смещения/blur/spread `box-shadow`, `flex-basis` (px); `border-width` в `em` масштабируется дважды.

## Точка входа
`crates/engine/layout/src/style/cascade.rs:177` (`apply_zoom_to_lengths`).

## Готово, когда
Юнит: `zoom:2; border-radius:4px; flex-basis:50px; box-shadow:2px 2px 4px` → 8 / 100 / (4,4,8); `border:1em` при `font-size:10px` и `zoom:2` → 20px, а не 40.

## Попутно
Строка CSS-SPECS:188 утверждает, что `getComputedStyle` не снимает zoom; по коду (`selector_query.rs:1193-1197`) снимает — поправить текст строки.

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings`; двигает пиксели → полный `python graphic_tests/run.py --continue-on-fail` + эталоны.

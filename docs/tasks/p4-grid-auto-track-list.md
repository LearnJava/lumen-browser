# Задача: `grid-auto-columns`/`grid-auto-rows` со списком треков

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:496` · **Размер:** S · **Крейты:** `lumen-layout`

## Цель
`parse_single` принимает один трек, `grid_track` берёт `auto_track` скаляром.

## Точка входа
`crates/engine/layout/src/style/apply/layout.rs:218`, `crates/engine/layout/src/box_tree/grid.rs:629`, места вызова в `grid_trampoline.rs:362-407`.

## Что сделать
`Vec` треков и циклический выбор по индексу неявного трека (`100px 200px` → 100, 200, 100…); для неявных треков перед явными — цикл с конца списка.

## Готово, когда
Юнит layout: `grid-auto-rows: 100px 200px`, 3 неявные строки → высоты 100/200/100.

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings`; полный `python graphic_tests/run.py --continue-on-fail` + эталоны, если что-то сдвинулось.

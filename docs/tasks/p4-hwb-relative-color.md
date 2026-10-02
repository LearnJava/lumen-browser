# Задача: относительный цвет `hwb(from …)`

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:43` · **Размер:** S · **Крейты:** `lumen-layout`

## Цель
`parse_hwb_body` явно отвергает `from`, в `relative_origin_channels` нет HWB.

## Точка входа
`crates/engine/layout/src/style/parse/color.rs:511`, `crates/engine/layout/src/color_mix.rs:718`.

## Готово, когда
Юнит: `hwb(from red h w b)` = red; `hwb(from red h 50% b)` ≈ rgb(255,128,128); `calc()` над каналами работает как у `hsl(from …)`.

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings` + `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`.

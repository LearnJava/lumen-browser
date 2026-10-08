# Задача: лонгхенды `font-variant-numeric` / `-ligatures` / `-position`

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:234` · **Размер:** M · **Крейты:** `lumen-layout`

## Цель
Лонгхенды не существуют нигде (grep даёт 0), шортхенд `font-variant` их только сбрасывает. Вывод в рендер уже есть (`DrawText.font_features`).

## Точка входа
`crates/engine/layout/src/style/values/typography.rs:627`, `crates/engine/layout/src/style/apply/text.rs:272` (`text_font_features`).

## Что сделать
Поля в `ComputedStyle` (наследуемые), парсинг, компоненты шортхенда `font-variant`, теги в `text_font_features`: `tnum`/`lnum`/`onum`/`pnum`/`frac`/`afrc`/`zero`/`ordn`; `liga`/`clig`/`dlig`/`hlig`/`calt` = 0/1; `sups`/`subs`. `font-feature-settings` остаётся последним (перекрывает).

## Не трогать
`font-variant-east-asian`/`-alternates` и `@font-feature-values` — отдельная задача.

## Готово, когда
Юнит: `font-variant-numeric: tabular-nums slashed-zero` → `(b"tnum",1)` и `(b"zero",1)` в `text_font_features`; `font-variant-ligatures: none` → `liga`/`clig`/`calt` = 0; `font-feature-settings: "tnum" 0` перекрывает `tabular-nums`.

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings` + `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`.

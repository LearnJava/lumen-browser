# Задача: шортхенды `grid-template` / `grid` целиком

**Developer:** P4 · **Указатель:** `CSS-SPECS.md:495` · **Размер:** M · **Крейты:** `lumen-layout`

## Цель
Сейчас поддержана только форма `rows / cols`.

## Точка входа
`crates/engine/layout/src/style/apply/layout.rs:238`, `:259`.

## Что сделать
- ASCII-форма `"a a" 50px "b c" auto / 1fr 2fr` → areas + rows + cols.
- Сброс незаданных лонгхендов в начальные значения.
- `grid: auto-flow [dense]? <auto-rows> / <cols>` и обратная форма `<rows> / auto-flow [dense]? <auto-cols>`; `none`.

## Готово, когда
Юниты в `style/tests`: areas = [["a","a"],["b","c"]], rows = [50px, auto], cols = [1fr, 2fr]; `grid: auto-flow 100px / 1fr` → flow=row, auto_rows=100px, cols=[1fr].

## Гейт
`cargo clippy -p lumen-layout --all-targets -- -D warnings` + `scripts/scoped-test.sh` + `python graphic_tests/dump_golden.py`.

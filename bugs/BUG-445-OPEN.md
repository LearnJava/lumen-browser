# BUG-445

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/tests/snapshots/flex_*.txt`) + гейт
`scripts/scoped-test.sh`
**Файл:** `crates/engine/layout/tests/cases/snapshot_tests.rs`

## Описание

Шесть flex-снапшотов интеграционного набора `lumen-layout --test all` красные на
чистом `main`:

```
flex_row_equal_children
flex_row_explicit_basis
flex_column_children
flex_column_gap
flex_gap_with_grow
flex_wrap_grow_per_line
```

Геометрия в них **совпадает** — расходится только хвостовая аннотация `w=`/`h=`:

```
--- expected ---
    Block rect=(0.00, 0.00, 300.00, 50.00) bg=#ff0000ff display=flex w=300.00 h=50.00
--- actual ---
    Block rect=(0.00, 0.00, 300.00, 50.00) bg=#ff0000ff display=flex w=900.00 h=50.00
```

Причина — смена семантики дампа в BUG-333/BUG-343: `w=`/`h=` печатаются из
**стиля**, а не из `rect=`. В `flex_row_equal_children` селектор `div` из
`div { display:flex; width:900px }` матчит и детей тоже, поэтому у них
специфицированная `width: 900px`, а использованная (после `flex-grow: 1`) — 300px.
Эталоны записаны под старую семантику и с тех пор не перегенерированы.

## Почему это не поймали раньше

Гейт `scripts/scoped-test.sh` до этих тестов **не доходит**: он падает раньше на
`cargo test -p lumen-layout --lib`, где красные `ch_approximated_as_half_em` /
`ex_approximated_as_half_em` ([BUG-339](BUG-339-OPEN.md), тоже
предсуществующие), и обрывает прогон пакета целиком. То есть один известный
красный маскирует другой: интеграционные тесты `lumen-layout` в гейте
фактически не исполняются.

## Что сделать

1. Решить, какая семантика `w=`/`h=` в `--dump-layout` правильная (вопрос к
   владельцу BUG-333/343: печатать специфицированное значение, использованное,
   или оба), и перегенерировать эталоны под неё.
2. Отдельно — не дать одному красному крейту глотать остаток прогона: в
   `scoped-test.sh` нет `--no-fail-fast`.

## Как нашли

P1, 2026-07-29, при финальном гейте BUG-433. Проверено прямым прогоном
`cargo test -p lumen-layout --test all` на сборке `main` — те же шесть тестов
падают идентично, к BUG-433 отношения не имеют.

# BUG-445

**Статус:** FIXED 2026-07-30
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

## Фикс

Семантика уже была решена в самом BUG-333/343 (владелец — тот же P1): после
фикса `w=`/`h=` печатает **специфицированное** значение стиля (то, что реально
стоит в `style.width`/`style.height` после restore), не used-размер, который
flex временно писал в стиль во время раскладки. Шесть golden-файлов были
записаны до этого фикса и с тех пор не перегенерированы — отсюда расхождение,
не регрессия. Во всех шести случаях `rect=` (геометрия) совпадал побайтово,
менялась только хвостовая аннотация; в двух случаях (`flex_column_children`,
`flex_column_gap`) заодно ушла аннотация `box-sizing=border-box` — она была
побочным следом старого «пишем и не откатываем» пути, устранённого тем же
фиксом.

Перегенерировано точечно (`UPDATE_SNAPSHOTS=1 cargo test -p lumen-layout
--test all -- flex_row_equal_children flex_row_explicit_basis
flex_column_children flex_column_gap flex_gap_with_grow
flex_wrap_grow_per_line`) — задело ровно 6 файлов. Полный `cargo test -p
lumen-layout --test all --no-fail-fast`: 71/71 passed.

Отдельно — `scripts/scoped-test.sh` теперь запускает `cargo test` с
`--no-fail-fast`, чтобы один известный красный крейт (сейчас —
[BUG-339](BUG-339-OPEN.md)) не глотал остаток прогона пакета и не маскировал
другие красные тесты того же крейта.

## Как нашли

P1, 2026-07-29, при финальном гейте BUG-433. Проверено прямым прогоном
`cargo test -p lumen-layout --test all` на сборке `main` — те же шесть тестов
падали идентично, к BUG-433 отношения не имели.

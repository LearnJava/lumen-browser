# BUG-1804 — 16 unit-тестов lumen-layout красные на чистом main: семья border-collapse / border-spacing / табличных гридов

**Статус:** FIXED
**Открыт:** 2026-11-13 (P3, попутно к гейту BUG-1800 срез 2)
**Исправлен:** 2026-11-13 (P3)
**Компонент:** layout (`crates/engine/layout/src` — стили таблиц; виновный merge не бисектился)

## Симптом

`cargo test -p lumen-layout --lib` на чистом main `860aaab0d` (проверено stash-экспериментом
2026-11-13, до/после переписи BUG-1800 срез 2 — множество красных идентично, 5981 passed /
16 failed в обоих случаях):

- `style::tests::svg_transform_and_misc::border_spacing_zero_by_default`
- `style::tests::svg_transform_and_misc::separate_mode_keeps_full_cell_borders`
- `style::tests::table_anon::display_contents_row_cells_join_the_table`
- `style::tests::table_anon::display_contents_row_group_rows_join_the_table`
- `style::tests::anchor_positioning_tests::p4_border_collapse_inherited_by_cells` —
  `td` получает `Separate` вместо унаследованного от таблицы `Collapse`; при этом сама
  таблица стиль получает (ассерт на 602 зелёный) — ломается наследование на ячейке
- `style::tests::shadow_dom_selectors::document_normal_rule_beats_host_rule_of_shadow_sheet`
- `style::tests::shadow_dom_selectors::document_rule_does_not_reach_into_shadow_tree`
- `style::tests::shadow_dom_selectors::normal_declaration_of_outer_context_beats_inner_one`
- `style::tests::ua::cellspacing_hint_negative_ignored`
- `table_grid_presentational::table_cells_layout_horizontally`
- `table_grid_presentational::table_rows_stack_vertically`
- `table_grid_presentational::table_rowspan2_second_row_skips_occupied_column`
- `table_grid_presentational::table_cell_after_colspan2_x_position`
- `table_grid_presentational::table_colspan2_cell_width`
- `table_grid_presentational::table_rowspan2_cell_height_spans_two_rows`
- `table_grid_presentational::table_with_tbody_rows_stack_vertically`

Все красные — одна семья: наследование/значения `border-collapse`/`border-spacing` и
раскладка табличных гридов. Похоже на один недавний merge (бисект по мержам между
последним зелёным гейтом и `860aaab0d` — первый шаг фикса).

## Попутно замечено на том же main (отдельные дефекты, не часть этого бага)

- `cargo test -p lumen-js --lib` **не компилируется**: `crates/js/src/dom/tests/v8_ux_selection_api.rs`
  — `use of undeclared type QualName/NodeId` (76 errors) — в тестовый файл попала правка без импортов.
- `cargo test -p lumen-driver --test all`: `bug1191_boxless_computed_style` (уже задокументирован
  в [BUG-1784](BUG-1784-OPEN.md)), `bug489_display_contents_computed_style`, `snapshot_cpu::cpu_snapshots_match_references`.
- `cargo test -p lumen-paint --lib`: `display_list::text_and_images::empty_cells_hide_ignored_under_border_collapse`
  (та же border-collapse семья).
- `cargo test -p lumen-media-native --lib`: `hevc_hardware_matches_software` (аппаратно-зависимый ассерт).
- Флака в полных параллельных прогонах lumen-layout:
  `box_tree::tests::block_flow_trampoline::deep_chain_with_margins_collapses_through_the_trampoline`
  — изолированно зелёный всегда (3/3, до и после правок среза), в полном прогоне падает
  через раз (17/16 failed в трёх подряд). Прецедент — [BUG-1787](BUG-1787-OPEN.md).

## Репро

```
cargo test -p lumen-layout --lib
```

## Что сделать

Бисект по merge-коммитам main до последнего зелёного гейта → виновник → фикс или откат
куска. До починки гейт `scoped-test.sh` по layout-цепочке не бывает полностью зелёным —
сравнивать множества красных с baseline, а не требовать ноль (см. срез 2 BUG-1800).

## Исправление

**Диагноз.** Бисект по merge-границам не нужен: коммиты виновника стоят прямо на
first-parent цепочке main (потому «merge не бисектился»). Все 16 красных — два
последствия BUG-1777:

1. **Реальная регрессия каскада** (`ebf06fb24`, «исправления для таблиц,
   shadow-cascade, label-click») — три красных `shadow_dom_selectors`:
   - encapsulation-context sort перевёрнут («inner wins для normal, outer для
     `!important`») против CSS Scoping L1 / Cascade L5 §6.1 — нормальные
     декларации выигрывает ВНЕШНЕЕ дерево, `!important` — внутреннее; мотивация
     коммита «должно помочь disabled keyboard» — спекулятивная, тестовых
     ожиданий под перевёрнутый порядок никто не заводил;
   - удалён `matched.clear()` для узлов shadow-дерева — document-правила стали
     течь внутрь shadow-дерева; это scoping-запрет (CSS Scoping L1 §3.1), а не
     каскадный вопрос, ключом сортировки он не выражается. UA-фаза (единственное
     верное наблюдение коммита — UA-правила применяются к shadow-деревьям) и до
     отката шила отдельно от `matched`.
2. **Устаревшие ожидания тестов** к осознанно изменившемуся поведению
   (`7f2b54104` UA-стили `<table>`: `border-spacing: 2px` — паритет с Chrome;
   `39aedf889` border-collapse/table-layout не наследуются — CSS 2.1 §17.6):
   13 тестов табличной геометрии/стилей писались под spacing 0 и наследуемый
   border-collapse; сообщения («первая строка y=0, got 2», «x=80, got 84»,
   «height 70, got 72») — ровно сдвиг 2px UA-дефолта.

**Правки.** `style/cascade.rs`: возвращены оба куска (sort `cx` и `matched.clear()`
с исходным комментарием). Тесты: `table_grid_presentational.rs` — в общую фикстуру
добавлен авторский `border-spacing:0` (2 варианта replaceAll, грид-математика
BUG-006 сохранена); `table_anon.rs` — `border-spacing:0` в sheet обоих хелперов
(`lay`, `measured_cell_width`); `separate_mode_keeps_full_cell_borders` —
`border-spacing:0` в фикстуру (старый строгий gap=0); `border_spacing_zero_by_default`
→ `border_spacing_ua_default_2px_and_zero_overridable` — гвард нового дефолта
(bare `<table>` gap=2) и его перекрытия авторским 0; `cellspacing_hint_negative_ignored`
— проигнорированный hint оставляет UA-дефолт 2.0 (сработавший дал бы −5);
`p4_border_collapse_inherited_by_cells` → `p4_border_collapse_not_inherited_by_cells`
— ассерт перевёрнут по спецификации: td держит initial Separate, модель схлопнутых
границ гонится border-collapse самой таблицы при раскладке.

**Гейт:** `cargo test -p lumen-layout --lib` 6005 passed / 0 failed (было 16 failed);
clippy `-p lumen-layout --all-targets -D warnings` чистый; graphic-тесты — см. строку
в BUGS-FIXED.md. Попутные дефекты карточки (несборка `lumen-js --lib`,
driver/paint/media-native, флака deep_chain) — не часть этого бага, оставлены.

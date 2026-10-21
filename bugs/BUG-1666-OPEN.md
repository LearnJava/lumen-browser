# BUG-1666 — красный `main`: 15 тестов paint/snapshot/shell и 5 goldens падают без связи с текущими задачами

**Статус:** OPEN
**Заведён:** 2026-10-21 (Chat; найден гейтом `scripts/scoped-test.sh` при слиянии BUG-1665, падения подтверждены на чистом `main` 40088de85 без коммита BUG-1665)
**Область:** paint / shell / tests (`crates/engine/paint/src/display_list/tests/text_and_images.rs`, `crates/engine/paint/src/hit_test.rs:401`, `crates/engine/paint/tests/cases/snapshot_tests.rs:37`, `crates/shell/src/tests/page_pipeline.rs:201,280`, goldens `graphic_tests/`)
**Владелец:** P6 (неотложное: красный `main` валит гейт `/lumen-task-finish` у всех ролей)

## Симптом

Один и тот же набор падений на `main` и на любой ветке от него; к сети и JS не относится (`lumen-paint` не зависит от `lumen-network`).

| крейт / бинарь | тест | что видно |
|---|---|---|
| `lumen-paint` (lib) | `display_list::text_and_images::underline_positioned_below_baseline`, `overline_positioned_above_text`, `line_through_positioned_above_baseline`, `wrapped_lines_have_correct_y_offset`, `inline_run_wrap_y_offset`, `color_input_paints_value_swatch` | вертикальные позиции текста/декораций не совпадают (`text_and_images.rs:983`, `:1125`) |
| `lumen-paint` (lib) | `hit_test::tests::hit_inside_simple_block_returns_node` | hit на `<p>` даёт `NodeId(1)` вместо узла `<p>` (`hit_test.rs:401`) |
| `lumen-paint` (`tests/all.rs`) | `cases::snapshot_tests::{single_paragraph, cyrillic_text, line_wrap_two_lines, nested_blocks_paint_order, paragraph_with_background}` | снапшот display list расходится (`snapshot_tests.rs:37`) |
| `lumen-shell` (`--bin lumen`) | `tests::page_pipeline::{bug269_no_attrs_uses_raw_intrinsic, bug969_density_descriptor_unaffected, bug969_nw_descriptor_density_corrects_raw_decode}` | ожидаемый размер картинки — `None` (`page_pipeline.rs:201`, `:280`) |
| `graphic_tests/dump_golden.py` | `samples/test-06-layout.html`, `graphic_tests/25-table-layout.html`, `35-grid-named-areas.html`, `106-transform-zindex.html` (`--dump-display-list`), `65-flex-align-content.html` (`--dump-layout`) | например `PushClipRect (1,1,1022,718) -> (0,0,1024,720)`, `FillRect -> PushClipRect` — клип окна по краю сменился |

Гипотеза (не проверена): одна общая причина в layout/paint — сдвиг текста по вертикали + смена клипа корня похожи на одно изменение в недавних слияниях layout (`contain:size`, multicol, `:is/:where`…). Не исключено и окружение (шрифты этой машины) — bisect это различит.

**Вне этого бага, уже заведены** (не чинить здесь, только не путать): `v8_gap_uashadowslot::*` — [BUG-1237](BUG-1237-OPEN.md); `chrome_incremental::bug341_s13/s20/s21` — [BUG-1651](BUG-1651-OPEN.md); `page_resources::frame_subresources_fetch_links_and_imgs_with_outcomes` — [BUG-480](BUG-480-OPEN.md).

**Нестабильные (flaky), в отдельный баг:** `lumen-js` `dom::tests::v8_idle_message_clipboard::navigator_clipboard_read_asks_and_returns_items`, `v8_runtime::tests::perf9_code_cache_full_still_stores_new_entry`, `perf9_code_cache_hit_preserves_semantics` — падают в полном параллельном прогоне `scoped-test.sh`, по отдельности проходят и на `main`, и на ветке. Вероятно, общее состояние (каталог code cache, буфер обмена) между тестами.

## Проба

```
cargo test -p lumen-paint --lib -- text_and_images hit_inside_simple_block
cargo test -p lumen-paint --test all -- snapshot_tests
cargo test -p lumen-shell --bin lumen -- bug269_no_attrs bug969_
python graphic_tests/dump_golden.py
```

## Что делать

1. Воспроизвести пробой выше на свежем `main`.
2. `git bisect` по `main` (первым кандидатом взять коммит несколькими днями раньше, где эти тесты зелёные; шаг — `cargo test -p lumen-paint --lib -- underline_positioned_below_baseline`). Найти первый плохой коммит; проверить, объясняет ли он все группы из таблицы (paint, snapshot, shell `page_pipeline`, goldens) — если нет, bisect для оставшейся группы отдельно.
3. Если причина — дефект в коде: починить в этой ветке (`p6-bug-1666`) с регрессией; если изменение поведения было намеренным и тесты/goldens устарели — обновить ожидания и goldens **в том же коммите** с обоснованием в теле коммита (какой коммит и почему поменял результат).
4. Если причина — окружение машины (шрифты и т.п.), а не код: описать в этом файле, как это проверено, и закрыть баг как WONTFIX/ДОРАБОТКА по `docs/doc-sync.md`.
5. Если починка выходит за один срез: завести по каждой группе свой `BUG-NNN-OPEN.md` с найденным коммитом-виновником, поставить указатели в `STATUS-P3.MD` и закрыть этот баг ссылками.
6. Flaky-тройку завести отдельным багом `OPEN` (следующий номер) с наблюдениями выше, указатель — в `STATUS-P3.MD`.

## Как проверить

- `bash scripts/scoped-test.sh` на ветке: в выводе нет ни одного теста из таблицы «Симптом» (падения из BUG-1237/BUG-1651/BUG-480 допустимы, они вне этого бага).
- `python graphic_tests/dump_golden.py` — 0 несовпадений.
- `cargo clippy -p <затронутый крейт> --all-targets -- -D warnings` чисто.

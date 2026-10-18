# GRID-ABSPOS — заметка передачи (ветка `p1-grid-abspos`, работать в основном checkout)

## Сделано (код написан, clippy `-p lumen-layout --all-targets` чист, 11 юнит-тестов `tests/grid_abspos.rs` зелёные)

* `grid.rs`: `resolve_abs_axis` (линии области: `auto`/`span`-без-якоря/линия за сеткой → `None` = край padding box);
  `GridInit.col_explicit/row_explicit`.
* Новый `box_tree/grid_abspos.rs`: `lay_out_abs(b, s, init: Option<&GridInit>, is_positioned, own_pcb, content: Rect, …)` —
  CB-область, статическая позиция = область, выравнивание `justify-self/align-self` по осям с обеими вставками `auto`,
  физические оси (rtl / vertical-rl / sideways-lr). Старый `grid_trampoline::lay_out_abs` удалён; вызовы в
  `grid_trampoline::finish_frame` ×2, `layout_dispatch.rs`, `grid_vertical.rs` обновлены.
* `multicol_abspos.rs`: `lay_out_abs_children_with` (переопределение CB на ребёнка), несколько `fn` стали `pub(super)`.
* WPT `css/css-grid/abspos` (`run_report.py … --update-expected`, `--processes 4`, 8 процессов роняет bidi-handshake):
  метаданные `tests/wpt/metadata/css/css-grid/abspos/` обновлены (в коммите). Против сборки до правок: 58 → 27 регрессий
  к старой метадате в первых 40 тестах, остальные не хуже. Прогон полного каталога — ~12 мин (TIMEOUT-тесты), чанки
  `--offset/--limit`.

## Осталось

1. **Регрессия:** `positioned-grid-items-negative-indices-002.html` — на сборке до правок PASS, сейчас FAIL (в метадате
   записан `FAIL`). Разобрать (отрицательные линии `-1` и т.п. в `resolve_abs_axis`/`GridAxis::numbered` — вероятно
   отрицательная линия считается от `last_line()` явной сетки, а нужна неявная; сверить с тестом), поправить,
   перегенерировать ini этого теста (`--offset`/`--limit` подобрать так, чтобы попал один файл, или `--root` не нужен —
   можно прогнать целиком).
2. Известные нерешённые (были до правок, не входят в задачу): `descendant-static-position-001/002/004` (abspos — потомок
   grid-item, статическая позиция внутри item), `empty-grid-001` подтесты, `abspos-in-flexbox-in-grid-crash` TIMEOUT.
3. Гейты: `scripts/scoped-test.sh` (lumen-layout), `python graphic_tests/dump_golden.py`; расходится ли пиксель — полный
   `python graphic_tests/run.py --continue-on-fail` + голдены в том же коммите (docs/graphic-tests.md). Вынуждены ли
   новые графтесты — по желанию 1 (abspos в grid с выравниванием).
4. Доки в том же коммите: `CAPABILITIES.md` (grid abspos), `ROADMAP.md:1045` → done, `docs/wpt-vendor-notes/css.md`
   (строки про abspos css-grid), удалить эту заметку, удалить указатель `ROADMAP.md:1045` из `STATUS-P1.md`.
5. `merge --no-ff` в main, push (в рабочем каталоге есть чужие незакоммиченные файлы — не стейджить: `.claude-manager/*`,
   `graphic_tests/results/*`, `crates/js/src/{bluetooth,serial,webhid,webrtc_stub,webusb}.rs`, `shell/src/panel_layout.rs`,
   `tests/wpt/metadata/compat/webkit-scrollbar-parse.html.ini`).

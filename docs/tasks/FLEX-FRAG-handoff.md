# FLEX-FRAG — заметка передачи (WIP, ветка `p1-flex-frag`, слот `p1-work`)

## Сделано

- `crates/engine/layout/src/box_tree/multicol_flow.rs`:
  - `is_fragmentable` принимает `Display::Flex` (контейнер режется как блок, дети — flex-элементы);
  - `walk_item` для flex сортирует детей по `rect.y` (`order`/`*-reverse`) и вызывает новый
    `walk_flex_lines`: элементы с перекрывающимися блочными протяжённостями — одна flex-линия;
    между линиями — кандидат разрыва (`break-*` любого элемента линии), внутри элементов линии —
    их собственные кандидаты (параллельные потоки, разрыв режет все элементы по геометрии);
  - `Window.extent` (высота колонки, `INFINITY` без высоты) — фрагмент flex-контейнера с
    разрывом внизу (`bottom_cut`) достаёт до низа колонки (Flexbox L1 §12);
  - `Free.resume` + `Model::free_at` — верхнее поле первого элемента выше колонки режется первым
    разрывом, следующая колонка начинается у элемента (flex-container-fragmentation-006).
- `multicol_trampoline.rs`: `Window { extent: h_of(c) }`.
- Тесты: `box_tree/tests/multicol_flex.rs` (7 шт., зарегистрирован в `tests/mod.rs`) — зелёные;
  `cargo test -p lumen-layout multicol` — 105 зелёных.

## Не сделано / дальше

1. WPT `css/css-break/flexbox` (330 id): после первой правки 118 PASS / 211 FAIL (`.tmp/flex-after.log`).
   Повторного прогона с `extent`/leading-margin ещё нет (бинарь `target/dev-release/lumen.exe`
   собран с ними). ~112 падений — тесты с `position: absolute` (`.abs`-накладки прямо в multicol →
   `has_out_of_flow` отключает нарезку) = задача MULTICOL-FRAG-OOF, не эта.
2. Остаются без OOF: `contain: size`-элементы (41, `contain:size` у вложенных блоков внутри flex-элемента),
   inline-block в строках разной высоты (`ItemLines` режет строки равной высотой `height/l`),
   проценты высоты от фрагментированного размера (single-line-row-026/047), vertical writing-mode,
   «элемент растёт из-за фрагментации» (single-line-column-007..014), overflow содержимого элемента
   (058), multi-line row/column (путь `emit_grid_fragments`, не затронут).
3. `break-before/after` первого/последнего элемента flex не пробрасывается контейнеру (019/020) —
   проверить `breaks_before/after` в `multicol_break.rs`.
4. Не сделано по ТЗ: докс-синхронизация (ROADMAP.md строка FLEX-FRAG → статус, CAPABILITIES.md,
   `docs/wpt-vendor-notes/css.md` §css-break, `subsystems/layout.md` если есть), полный
   `python graphic_tests/run.py --continue-on-fail` (flex в multicol двигает пиксели), clippy
   `cargo clippy -p lumen-layout --all-targets -- -D warnings`, закрывающий коммит + merge --no-ff.
5. Побочно: `css/css-break/flexbox/flexbox-fragmentation-layout-001-crash.html` зависает (60 с) и на
   базовом бинаре — после него wptrunner иногда обрывает прогон (остаётся 39 тестов). Завести баг;
   исключать из прогона можно `--exclude-prefix` только с `MSYS2_ARG_CONV_EXCL='*'`
   (иначе Git Bash превращает `/css/...` в `C:/Program Files/Git/css/...`).

## Как проверять

- `cargo test -p lumen-layout multicol` (≈3 мин на холодную сборку тестов).
- Сборка бинаря: `cargo build --profile dev-release --bin lumen` (6 мин холодно, 3 мин инкрементально).
- WPT: `PY=D:/RustProjects/lumen-browser/.tmp/venv-wpt/Scripts/python.exe;
  LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='*' $PY tests/wpt/run_report.py --binary <lumen.exe>
  --all --root css/css-break/flexbox --recursive --processes 8 --exclude-prefix /css/css-break/flexbox/flexbox-fragmentation-layout
  --out .tmp/x.html --log-raw .tmp/x.log`; сводка — `.tmp/wptsum.py <log> [<log2>]`
  (`.tmp/` не в git: скрипт печатает статусы `test_end` из raw-лога; при потере — 15 строк на Python).
- Визуальный разбор: `.tmp/shot.py <out.png> <lumen.exe> <тесты...>` — сетка «тест | референс» (PIL из venv-wpt).

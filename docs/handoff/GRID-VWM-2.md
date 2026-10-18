# Заметка передачи: GRID-VWM-2 (ветка `p1-grid-vwm-2`)

## Сделано (закоммичено в ветке, в main НЕ влито)

Базовая линия WPT (31 файл задачи): 118 / 687 сабтестов → **437 / 687**.

1. `grid_trampoline.rs`: строки `minmax(...)` растут по содержимому (`row_grows_with_content`, `maximize_bounded_rows`:
   Grid L1 §11.5–11.6; `minmax(_, fr)` не трогаем — GRID-FR-ROWS). Тесты `minmax_rows_*` в `tests/grid_vwm.rs`.
2. `grid_trampoline.rs`: `aspect-ratio` + определённая высота + не растягиваемый `justify-self` → ширина из пропорции
   (`ratio_width_for_height`). Тест `aspect_ratio_gives_width_to_a_non_stretched_item`.
3. `grid_vertical.rs` + `intrinsic.rs`: ортогональный item горизонтальной сетки — при замере обтягивает содержимое даже при
   `stretch` (`orthogonal_fit_height(.., probe)`); `orthogonal_fit_content_height(.., add_margins)` — для grid без полей
   (в flex/block поведение прежнее); `final_args` вычитает поля из `cell_block` ортогонального item'а.
   `grid-items-minimum-height-orthogonal-001` 46/46.

Проверено: `cargo clippy -p lumen-layout --all-targets -- -D warnings` чисто; `cargo test -p lumen-layout -- --skip deep_` зелёный
(`deep_*` падают STACK_OVERFLOW и на чистом main — BUG-1266); `graphic_tests/dump_golden.py` 12/12; `--dump-layout` всех 170
`graphic_tests/*.html` старым и новым бинарём идентичен (пиксели не двигаются). Живой `graphic_tests/run.py` в этой среде не
работает (TEST-00: «automation command timed out», захват экрана) — пиксельный прогон не выполнен.

## Осталось (перед слиянием)

- Документы в том же коммите слияния: `CAPABILITIES.md:77` (строки «Not done» — убрать minmax-строки/ортогональные
  min-height, оставить остаток), `ROADMAP.md:1042` (GRID-VWM-2 → done; остаток → новая строка GRID-VWM-3, затем
  `python scripts/gen_roadmap.py`), `docs/wpt-vendor-notes/css.md` (строки 318/374/379/2138: ссылки на GRID-VWM-2),
  убрать указатель `ROADMAP.md:1042` из `STATUS-P1.md`, удалить этот файл при слиянии.
- Остаток для GRID-VWM-3 (измерено):
  a) `min-width`/`max-width` не входят в `max_content_outer_width` (`intrinsic.rs:730`): inline-grid/auto-столбец с
     `min-width:60px` item'ом даёт 10 вместо 70 (`grid-items-minimum-width-orthogonal-002`, 0/44 + в харнессе ширина 1008);
  b) автоминимум столбца `minmax(auto, 0px|25px)` не зажимается максимумом (`grid-minimum-size-grid-items-022/024`);
  c) `grid-intrinsic-size-with-orthogonal-items` (0/7), `grid-align-justify-stretch-with-orthogonal-flows` (0/8, Ahem/BUG-1306),
     `grid-item-aspect-ratio-justify-self-001` ортогональная группа (ширина 396), `normal`/`stretch` + ratio (transferred min-height);
  d) `column-rule` в вертикальной сетке: `grid_vertical.rs` `b.subgrid_tracks = None` — нужен физический разворот дорожек и
     правка paint;
  e) baseline `text-orientation: sideways` (`grid-align-baseline-vertical` 5/11, `grid-minimum-contribution-baseline-shim-vertical-*` 0/6);
  f) `%`-поля/padding (`grid-items-percentage-*-vertical-*`, упирается в BUG-1306), `margin:auto` в авто-колонках вертикали (0/4×2).

## Стенд WPT

В `.tmp/` основного чекаута: `wpt_run.sh <ids> <out.json> [бинарь]`, `ids_all.txt`, `wpt_sum.py <json> [v]`, `wpt_msg.py <json> <подстрока> <n>`.
Бинарь собирать с локальным обходом BUG-1597: в `crates/shell/src/chrome_ui.rs:410` `loading: false,`
(`cargo build -p lumen-shell`, копия в `.tmp/lumen-wpt.exe`), перед коммитом `git checkout crates/shell/src/chrome_ui.rs`.
Python venv: `tests/wpt/.venv/Scripts/python.exe`. Пробы: `.tmp/p2..p6.html`, `lumen-wpt.exe --dump-layout file:///...`.

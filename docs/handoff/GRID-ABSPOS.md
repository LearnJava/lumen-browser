# GRID-ABSPOS — заметка передачи (ветка `p1-grid-abspos`, слот `p1-work`)

Кода ещё нет — сделан разбор и проект. Задача: ROADMAP.md:1045. Работать в `.claude/worktrees/p1-work/`.

## Что выяснено (по WPT `css/css-grid/abspos`, `css/support/grid.css`)

* **Когда применяется область.** Только если контейнер — containing block потомка (`is_positioned` для `absolute`;
  для `fixed` — `multicol_abspos::fixed_cb_rect(b).is_some()`, функцию сделать `pub(super)`). Иначе CB — `own_pcb`,
  статическая позиция — content box контейнера (WPT `grid-abspos-staticpos-*-large-border-padding`: выравнивание по
  центру *content box*).
* **Область.** `grid-column/row-start/end` разрешаются как у items, но: `auto` → край padding box контейнера;
  несуществующая линия (за `n_cols+1`/`n_rows+1`, т.е. за явной+неявной сеткой по items) → `auto`; `span` без якоря
  (`span/auto`, `auto/span`, `auto/auto`) → оба края auto; `auto / N` → старт = padding edge (НЕ `N-1`, как у items:
  WPT `grid-positioned-items-implicit-grid-001`: `grid-column-end:3` при 1 столбце → весь padding box).
  Линия-старт = начало дорожки `l` (`offsets[l-1]`, если `l = n+1` — конец последней), линия-конец = конец дорожки
  `l-1` (`offsets[l-2]+sizes[l-2]`; `l=1` → `offsets[0]`): зазоры на краях области не входят
  (WPT `grid-positioned-items-gaps-001`: `2/auto` → x=165=15+100+50, `auto/2` → ширина 115).
* **Статическая позиция при контейнере-CB** — начало *области* (padding box при auto/auto: x=0, не content box), а не
  content box (WPT `positioned-grid-items-001`: ref — обычные items с `place-items:start`). Выравнивание
  `justify-self/align-self` (auto → `justify-items/align-items` контейнера) работает внутри static-rect по осям, где
  обе вставки `auto`; `baseline`→`start`, `last baseline`→`end`, `safe`, `self-start/end` (`resolve_own`), `left/right`
  (`resolve_side`). Образцы кода выравнивания: `grid_trampoline.rs` блок `rtl` в `lay_out_abs` (его заменить) и
  `grid_vertical::place_item`.
* Сейчас `grid_trampoline::lay_out_abs` ставит всех на начало content box и зеркалит только rtl.

## План (один новый файл + правки)

1. `grid.rs`: `pub(super) fn resolve_abs_axis(start, end, axis: &GridAxis, n_tracks_total) -> (Option<u32>, Option<u32>)`
   рядом с `resolve_grid_axis` (использует приватные `resolve_edge`, `span_forward/backward`).
2. `GridInit`: поля `col_explicit: u32`, `row_explicit: u32` (= `n_tracks` осей `col_axis/row_axis` из
   `build_grid_init`, grid.rs ~382) — нужны для `GridAxis` (`names: &init.col_names`, `areas: &s.grid_template_areas`).
3. Новый `box_tree/grid_abspos.rs` (`mod grid_abspos;` в box_tree.rs рядом с `mod grid_vertical;`): перенести туда
   `lay_out_abs` с новой сигнатурой `(b, s, init: Option<&GridInit>, is_positioned, own_pcb, content_box: Rect,
   measurer, viewport, hp)`. Физические оси: горизонтально — x←столбцы (rev = `direction: rtl`), y←строки;
   вертикально — x←строки (rev = `rl`), y←столбцы (rev = `inline_rev = (dir==Rtl) != (wm==SidewaysLr)`);
   `content_box` считают вызывающие (горизонт.: высота = `b.rect.y+b.rect.height - border_bottom - padding_bottom -
   content_y`; вертикаль.: ширина = `b.rect.width - geom.frame_horiz`, высота = `content_inline`). Диапазон по оси:
   не rev → `c_lo+off`; rev → `c_hi-off`; auto-края → padding box (`my_pcb`). `init == None` (нет дорожек) → пустые
   массивы, линия 1 = начало content box, остальные → auto.
4. `multicol_abspos.rs`: `lay_out_abs_children_with(parent, impl IntoIterator<Item=(usize,f32,f32,Option<Rect>)>, …)`
   с переопределением CB на ребёнка; `lay_out_abs_children` делегирует. Вызывает приватный `place_abs_child`.
5. После размещения — сдвиг по статическим осям (`left/right` или `top/bottom` оба auto, нет anchor-полей):
   `off = grid_vertical::axis_offset(align, cell=hi-lo, size, m_s, m_e, false, false)`; `target = rev ? hi-off-size :
   lo+off`; `translate_subtree(c, target - pos)`. Поля — как в `place_abs_child` (по `cb.width` / `cb.height`).
6. Заменить вызовы `grid_trampoline::lay_out_abs` (finish_frame ×2, layout_dispatch.rs ~1308, grid_vertical.rs ~528).

## Проверка

* Базовый замер WPT `css/css-grid/abspos` (332 теста) запущен на `.tmp/lumen-base.exe` (копия сборки до правок):
  `.tmp/wpt-base.log`, `.tmp/wpt-base.html`, `.tmp/wpt-base.raw` (если прогон не дошёл до конца — повторить:
  `tests/wpt/.venv/Scripts/python.exe` лежит в **основном** checkout; `LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'`;
  `run_report.py --binary <exe> --out … --all --root css/css-grid/abspos --processes 4`).
* После правок: `cargo build --profile dev-release -p lumen-shell`, тот же прогон, сравнить; затем
  `run_report.py --update-expected` для `css/css-grid/abspos` (метаданные `tests/wpt/metadata/css/css-grid/abspos/`).
* Юнит-тесты: `crates/engine/layout/src/box_tree/tests/` (образец — `grid_rtl.rs`, подключить в `tests/mod.rs`).
* Дальше: clippy `-p lumen-layout --all-targets -D warnings`, `scripts/scoped-test.sh`, `graphic_tests/dump_golden.py`,
  доки (CAPABILITIES.md, ROADMAP.md:1045 → done, `docs/wpt-vendor-notes/css.md` строки 317/318/320/375,
  удалить эту заметку), удалить указатель `ROADMAP.md:1045` из `STATUS-P1.md`, merge --no-ff в main, push.
* Не мерять через `positioned-grid-descendants-*` до BUG-1306 (Ahem-снимок).

# Заметка передачи: GRID-RTL (ветка `p1-grid-rtl`, слот `p1-work`)

Удалить этот файл при слиянии ветки.

## Сделано (код готов, clippy `-p lumen-layout --all-targets` чистый, 7 новых тестов `box_tree/tests/grid_rtl.rs` зелёные)

- `GridInit.rtl` (горизонтальная сетка + `direction: rtl`); `cell_origin` зеркалит ячейку по x.
- `justify-content: left|right` в rtl — физическая сторона (`grid.rs`, через `resolve_side`).
- `post_final_item`: start/end/stretch/авто-поля/`self-start`/`left|right` с учётом rtl; `relative_shift` берёт `right` при обоих inset'ах в rtl и считает `top/bottom %` от высоты области.
- `lay_out_abs` (+ параметр `content_width`): статическая позиция abspos в rtl-сетке у правого края, `justify-self` (start/end/center) учтён.
- WPT: `grid-align`, `grid-align-justify-stretch`, `grid-items-relative-offsets-001/002`, `grid-item-direction-rtl-001`, `staticpos-justify-self-rtl-003/004` зелёные; регрессий среди 25 rtl-тестов нет. Стенд: `.tmp/mk_wpt.sh` (собирает бинарь с локальным обходом BUG-1597 и откатывает `chrome_ui.rs`), `.tmp/wpt_run.sh .tmp/ids_rtl.txt out.json .tmp/lumen-wpt.exe`, `.tmp/wpt_sum.py`.

## Осталось

1. Docs: `CAPABILITIES.md` (строка про GRID-VWM, «Not done … `direction: rtl` in a horizontal grid») -> готово GRID-RTL; шапка `grid_vertical.rs` («Не реализовано: direction: rtl…»); `ROADMAP.md` строка GRID-RTL `planned`->`done` + `python scripts/gen_roadmap.py`; `docs/wpt-vendor-notes/css.md` (строка `direction: rtl`).
2. Гейт пикселей: `python graphic_tests/run.py --continue-on-fail`, перегенерировать голдены в том же коммите (если есть сдвиги); `scripts/scoped-test.sh`.
3. Проверка `column-rule` в rtl-сетке (paint берёт `rtl` по шаблону, `record_own_tracks` отдаёт логические смещения — возможно, надо зеркалить для subgrid).
4. `/lumen-task-finish`: merge `--no-ff` в main, push, удалить указатель `ROADMAP.md:1044` из `STATUS-P1.md`.

Не коммитить обход `loading: false` в `crates/shell/src/chrome_ui.rs`.

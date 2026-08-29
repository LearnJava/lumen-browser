# BUG-801 — бесконечный цикл в auto-placement CSS Grid: элемент, не помещающийся в явные колонки, вешает layout намертво

**Статус:** FIXED 2026-08-29
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 9 — разбор TIMEOUT рефтестов)
**Область:** `crates/engine/layout/src/box_tree.rs` — пасс 2 auto-placement, row-flow и column-flow `loop` (CSS Grid L1 §8.5)
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи. Исправлен P3, 2026-08-29.

## Симптом

Процесс не завершается вообще — ни таймаута, ни паники, ни сообщения:

```bash
cat > /tmp/g.html <<'EOF'
<!DOCTYPE html><style>.g { display: grid; grid-template-columns: 50px 50px; }</style>
<div class="g"><div style="grid-column: 3;">x</div></div>
EOF
timeout 60 lumen --dump-layout /tmp/g.html   # rc=124, никакого вывода
```

Виснет именно layout: `--dump-layout` (без paint, без JS) достаточно, поэтому
живое окно на такой странице тоже застынет насмерть — это не «медленно», а
цикл без выхода. CPU при этом на 100 %.

## Условия (измерены перебором, 2026-08-21)

Виснет, когда **все три** верны:

1. явных колонок **две и более** (`grid-template-columns: 50px 50px`,
   `1fr 1fr`, `auto auto`, `repeat(auto-fill, 50px)` — важен итоговый счёт
   треков, а не форма записи);
2. элементу нужна колонка **за последней явной** — `grid-column: 3`,
   `grid-column: 5`, `grid-column: 3 / 4`, `grid-column: span 3`,
   `grid-column: 9 / span 2`;
3. по второй оси позиция **авто** (`grid-row` не задан).

Не виснет: одна явная колонка или `grid-template-columns: none`
(случайный аварийный выход, см. ниже); полностью явное размещение
(`grid-column: 3; grid-row: 1` — такой элемент до цикла не доходит);
отрицательные номера линий (`grid-column: -5` резолвится внутрь явной сетки).

Зеркальный дефект по второй оси: `grid-auto-flow: column` +
`grid-template-rows: 50px 50px` + `grid-row: 3` — тоже виснет.
`grid-auto-flow: row dense` — тоже (dense меняет только точку старта скана).

## Причина (локализована чтением кода)

`box_tree.rs:11817-11821`, пасс 2 auto-placement:

```rust
// Bounds: item must fit within explicit column count (or 1-col fallback).
let fits = (try_ce_val - 1) <= n_explicit_cols as u32 || n_explicit_cols == 1;
let cell_free = fits && (try_c..try_ce_val)
    .all(|c| (scan_r..scan_r + row_span).all(|r| !occupied.contains(&(c, r))));
```

`fits` зависит **только** от колонок элемента, а скан двигает строку
(`scan_r += 1`) — то есть если элемент не помещается в явные колонки, `fits`
ложно на каждой итерации навсегда, `cell_free` никогда не становится
истинным, и `loop` (11813) не имеет ни одного выхода: `break` стоит только
внутри ветки `cell_free`. Ограничения на число просмотренных строк нет.

Единственное, что спасает узкую сетку — дизъюнкт `|| n_explicit_cols == 1`:
он делает `fits` истинным всегда, поэтому однотрековые сетки и
`grid-template-columns: none` (там `.max(1)`) не виснут. Это не защита от
цикла, а частный случай, случайно его закрывающий.

По спецификации выхода из цикла быть и не должно: CSS Grid L1 §8.5 требует
**наращивать неявную сетку** под элемент, который не помещается в явную
(«increase the number of columns in the implicit grid»), а не искать ему
место внутри явных колонок. То есть `fits` в этой форме — неверная проверка,
а не недостающий guard: правильная реализация создаёт неявные колонки и
размещает элемент, и цикл завершается на первой же итерации.

## Цена (WPT-корпус, снимок 2026-08-20 Linux, 479/479 шардов)

**709 рефтестов TIMEOUT** из 715 — и это не «709 тяжёлых страниц»: 99 %
таймаутов идут **подряд** внутри одного процесса браузера (гистограмма длин
серий: 134, 66, 65, 65, 64, 63, 63, 59, 51, 33, …; одиночных всего 4).
Механизм: `LumenRefTestExecutor` переиспользует один процесс
`lumen --ipc-server` на все тесты шарда (`executorlumen.py`), первая же
зависшая страница вешает его навсегда, и каждый следующий тест шарда
получает `Timed out rendering …` по таймауту сокета — до конца шарда.
Одна страница с этим дефектом стоит десятки чужих вердиктов.

Категории, где это доминирует: `css/css-grid/grid-lanes` (206 TIMEOUT),
`css/css-grid/subgrid` (46), `css/css-transforms/*` (≈60 суммарно),
`css/css-flexbox/intrinsic-size` (17) — везде виноваты **референсные**
страницы (`*-ref.html`), которые как раз и раскладывают элементы по явным
колонкам с выходом за их край.

**Уточнение 2026-08-21 (WPT-RUN-6, срез 11).** Две из перечисленных категорий
этому багу не принадлежат — они вешаются своими дефектами, найденными и
измеренными отдельно: `css/css-transforms` — [BUG-803](BUG-803-OPEN.md)
(вечный цикл в `parse_svg_transform`, одна страница `2d-rotate-notref.html`
на 133 вердикта), `css/css-flexbox` — [BUG-802](BUG-802-FIXED.md)
(экспоненциальный layout вложенного колоночного flex, пять `.xhtml`-страниц
на 318 вердиктов). Строку выше читать вместе с этим абзацем.

За самим BUG-801 по тому же снимку остаются **10 из 16 повисших процессов**
(≈244 коллатеральных TIMEOUT), все в `css/css-grid`. Проверено поштучно под
`--dump-layout`: виснут `grid-definition/grid-auto-repeat-multiple-values-002.html`,
`grid-lanes/subgrid/.../line-names/column-line-names-012.html`,
`subgrid/line-names-008.html`, `subgrid/line-names-012.html`,
`subgrid/parent-repeat-auto-fit-001.html`, а у остальных пяти виснет не тест,
а его **эталон** (`column-auto-repeat-021-ref.html`, `line-names-010-ref.html`,
`line-names-005-ref.html`, `column-subgrid-grid-gap-003-ref.html`,
`column-subgrid-writing-direction-001-ref.html`) — сам тест в одиночку
проходит за 0.13 с. Отсюда практическое правило для триажа: страница-эталон
рефтеста — полноправный источник зависания, и по id теста её не видно.

Повторено вживую независимо от снимка:
`run_report.py --all --root css/css-grid/grid-lanes/track-sizing/auto-repeat
--recursive --processes 2` → 129 TIMEOUT из 169; первые 40 тестов проходят за
15 с, дальше **все** подряд по 14 с (таймаут сокета), оба процесса
браузера — до конца прогона.

## Как проверить фикс

```bash
timeout 30 lumen --dump-layout /tmp/g.html          # должен вернуть rc=0
```

плюс те же пять вариантов из «Условий» (span, `3 / 4`, `auto auto`,
`grid-auto-flow: column` по строкам, `row dense`) и прогон
`run_report.py --all --root css/css-grid/grid-lanes --recursive` — TIMEOUT
должен исчезнуть как класс (вердикты станут PASS/FAIL, что уже про
корректность раскладки, а не про зависание).

Регрессионный тест логично положить рядом с существующими grid-тестами
`crates/engine/layout/src/lib.rs` (`grid_explicit_placement`,
`grid_three_column_auto_placement`) — сейчас ни один из них не ставит
элемент за край явной сетки, поэтому дефект и дожил до корпусного прогона.

## Уточнение WPT-RUN-6 срезом 23 (2026-08-22)

Механизм получил **измеренный** список id: 22 теста остатка снимка WPT-RUN-5
(`grid-implicit-track-loop`, таблица `MEASURED` в
`tests/wpt/verify_layout_hangs.py`, читается `tests/wpt/timeout_audit.py`).
Правило по исходнику здесь невозможно в принципе: выходит ли элемент за
последнюю явную линию, зависит от **разрешённого** числа дорожек, а
`repeat(auto-fill, …)` делает его функцией ширины контейнера — поэтому список
получен прогоном каждой страницы под `--dump-layout` с таймаутом, а не грепом.
Из 22 — 10 виновники `hung-browser` с 244 чужими TIMEOUT-ами.

Строки после мержей сместились: `fits` сегодня — `box_tree.rs:11901` (row-flow)
и `:11948` (column-flow).

Минимальные репро живут в `REPROS` пробы и проверяются одной командой
(`verify_layout_hangs.py --repros`): `grid-line-past-explicit`,
`grid-span-past-explicit`, `grid-subgrid-nested` — виснут (обрыв на 15 с);
контроли `grid-inside-explicit` и `grid-single-track` — 0,01 с. После фикса
все пять строк должны показать `ok`.

## Фикс (P3, 2026-08-29)

Причина подтвердилась ровно так, как описана выше: `fits` — неверная
проверка, не недостающий guard. Два независимых исправления, по одному на
форму дефекта, обе в row-flow и column-flow ветках Пасса 2:

1. **Явно заданная колонка/строка** (`fixed_cs`/`fixed_rs != 0`, например
   `grid-column: 9 / span 2`) — позиция уже не поисковая: `try_ce_val`/
   `try_re_val` константны, поэтому граничная проверка была ложной на каждой
   итерации без единого шанса на успех. Теперь для этого случая `fits`
   снята целиком — решает только занятость ячеек.
2. **Авто-размещаемый элемент со спаном шире явной сетки**
   (`grid-column: span 3` при 2 явных треках) — граница вычисляется как
   `col_bound = max(n_explicit_cols, col_span)` (по строкам аналогично
   `row_bound`). Это гарантирует, что колонка/строка 1 всегда проходит
   `fits` (`try_ce_val - 1 == col_span <= col_bound`), поэтому цикл сходится
   за конечное число шагов, ограниченное числом уже занятых ячеек — ровно
   то поведение, которого требует CSS Grid L1 §7.1 (наращивание неявной
   сетки под элемент, которому не хватает места).

Дизъюнкт `|| n_explicit_cols == 1`, ранее случайно спасавший однотрековые
сетки от зависания (полностью отключая проверку `fits` для них), убран —
он больше не нужен: `col_bound`/`row_bound` при `n_explicit_cols == 1`
корректно раскрывается до `col_span`, если тот больше 1.

**Проверено:**
- Три висящих репро из `REPROS` (`grid-line-past-explicit`,
  `grid-span-past-explicit`, `grid-subgrid-nested`) и два контроля
  (`grid-inside-explicit`, `grid-single-track`) — все пять `--dump-layout`
  завершаются rc=0 за <1.2 с (dev-release, вручную через bash `timeout`,
  без `verify_layout_hangs.py`, см. ниже).
- Точный репро из «Симптома» (`grid-template-columns: 50px 50px` +
  `grid-column: 3`) — rc=0.
- 4 новых регрессионных теста в `crates/engine/layout/src/lib.rs`:
  `grid_column_start_beyond_explicit_grid_terminates`,
  `grid_span_wider_than_explicit_grid_terminates`,
  `grid_explicit_start_and_span_beyond_grid_terminates`,
  `grid_row_span_wider_than_explicit_grid_terminates` (последний —
  зеркало по column-flow/`grid-auto-flow: column`). Все 85 grid-тестов
  `lumen-layout` зелёные, регрессий нет.
- `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист.

**`verify_layout_hangs.py --repros` не запускается на этой Windows-машине**
(`args = ["timeout", str(limit), binary]` резолвит `timeout` как встроенный
`cmd.exe`/`timeout.exe`, а не POSIX-обёртку скрипта — все пять записей
отвечают `got=died t=0.02s` одинаково, включая контроли, что выдаёт
инструментальную проблему, а не поведение браузера). Не чинилось здесь —
инструмент вне зоны ответственности P3 (`docs/automation.md`/P2). Проверка
сделана вручную: те же пять тел страниц сохранены как файлы и прогнаны
`timeout 15 ./target/dev-release/lumen.exe --dump-layout <file>` из git-bash
(коретилз `timeout`, не встроенный cmd) — воспроизводит тот же сигнал, что
ждёт `verify_layout_hangs.py`, минус HTTP-обвязка.

**Как проверить фикс на живом WPT-корпусе (не выполнялось здесь):**
`run_report.py --all --root css/css-grid/grid-lanes --recursive` — TIMEOUT
по механизму `grid-implicit-track-loop` должен исчезнуть как класс.

# BUG-801 — бесконечный цикл в auto-placement CSS Grid: элемент, не помещающийся в явные колонки, вешает layout намертво

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 9 — разбор TIMEOUT рефтестов)
**Область:** `crates/engine/layout/src/box_tree.rs:11813` (row-flow `loop`) и `:11861` (зеркальный column-flow `loop`) — пасс 2 auto-placement, CSS Grid L1 §8.5
**Владелец:** P1/P3 (движок). Заведён P2 в ходе WPT-задачи, здесь не чинится.

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
на 133 вердикта), `css/css-flexbox` — [BUG-802](BUG-802-OPEN.md)
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

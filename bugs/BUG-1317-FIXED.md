# BUG-1317 — shrink-to-fit grid-контейнера ломается, если элемент размещён по имени линии или области

**Статус:** FIXED 2026-10-07 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 8, `css/css-grid`, вторая половина)
**Область:** layout (`crates/engine/layout/src/box_tree/grid.rs`, `intrinsic.rs` — расчёт max-content ширины контейнера по дорожкам; путь с именованным размещением отличается от числового)

## Симптом

`<div class=g>` с `.g{display:inline-grid;grid-template-columns:[x] 25px [y] 25px}`, дети `height:20px`, `--dump-layout`, ширина `.g`:

| дети | получено | ожидается |
|---|---|---|
| `grid-column:1` и `grid-column:2` | 50 | 50 |
| без размещения | 50 | 50 |
| `grid-column:x` и `grid-column:y` | **1024** | 50 |
| один ребёнок `grid-column:x` | **1024** | 50 |
| `grid-column:x/y` + обычный | **1024** | 50 |
| `grid-template-areas:"a b"` + `grid-area:a`, `grid-area:b` | **1024** | 50 |
| то же, `display:grid; width:max-content` | **0** | 50 |
| то же, `float:left` / `position:absolute` | **1024** | 50 |

Дорожки заданы в px, поэтому ширина контейнера не зависит от размещения элементов; при именованном — зависит. Ту же ошибку даёт `grid-column: C` с неоднозначным именем (`[C] 25px [C] 25px`).

## Как найдено

WPT-RUN-14 срез 8: `placement/grid-placement-using-named-grid-lines-001/002/003/006/007/008/009` (reftest `ref-filled-green-100px-square`: контейнер 1008×100 вместо 100×100, `--dump-layout`), `grid-template-areas-must-keep-named-columns-order-001`, `grid-container-change-named-grid-recompute-child-positions-001` (4 сабтеста) — 10 id в этом кластере. Поиск приёма «именованное размещение + shrink-to-fit» по исходникам остальных не зелёных id нашёл ещё 23 (кластеры «subgrid», `*-shrinkwrap-*`, `getComputedStyle`); часть из них, вероятно, падает по этой же причине — пробой не проверено.

## Что делать

Найти, почему ветка расчёта intrinsic-ширины при наличии именованного размещения уходит в «заполнить доступную ширину»: вероятно, элемент с неразрешённым (по числу) номером линии пропускается при вкладе в дорожки, и контейнер считает сетку «неопределённой». Разрешать имена линий/областей до расчёта вклада (как делает размещение).

## Как проверить

`css/css-grid/placement/grid-placement-using-named-grid-lines-001.html`, `grid-template-areas-must-keep-named-columns-order-001.html`.

## Решение (2026-10-07, P6)

Причина: `column_placement_in_explicit_grid` (`intrinsic.rs`) принимала только числовые `GridLine::{Auto,Line,Span}`, а `Named`/`NamedLine`/`SpanNamed` отвечала «не внутри явной сетки» — быстрый путь «сумма px-дорожек» отключался, и контейнер падал в правило «заполнить доступную ширину» (inline-grid/float/abspos — 1024, `max-content` — 0).

Исправление: функция теперь вызывает ту же `resolve_grid_axis`, что и размещение (`grid.rs`, `GridAxis`/`resolve_grid_axis` стали `pub(super)`), так что числа, `span`, имена линий и имена областей (`grid-area: a` → `a-start`/`a-end`) разрешаются одинаково с раскладкой. Неизвестное имя даёт неявную линию за явной сеткой — быстрый путь по-прежнему не применяется. Добавлена защита: `grid-template-areas` шире шаблона колонок добавляет авто-колонки, которых сумма px не знает, — тогда откат.

Тесты: `bug1317_*` в `box_tree/tests/intrinsic_and_wrap.rs`. Пиксельный гейт (`graphic_tests/run.py`) в сессии не выполнен — TEST-00 не находит маркер (захват экрана недоступен); `dump_golden.py` — 12/12 совпадают. Ожидания WPT-`.ini` не пересчитывались.

# BUG-1399 — multicol: `columns: 1` не контейнер колонок; `column-wrap: wrap` без `column-height` не открывает второй ряд

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** layout (`crates/engine/layout/src/box_tree/multicol_trampoline.rs`, `multicol_abspos.rs`)

## Симптом

`--dump-layout`, `body{margin:0}`:

| проба | получено | ожидается |
|---|---|---|
| `columns:1;column-fill:auto;width:50px;column-gap:0;height:100px`, ребёнок `height:200px` | один бокс 50×200 | два фрагмента по 100 px, второй правее (`x=50`) |
| то же с `columns:2;width:100px` | два фрагмента 50×100 | (верно) |
| `columns:2;height:50px;gap:10px 0;column-wrap:wrap`, ребёнок `height:180px` | колонки `x=0,50,100,150`, все на `y=0`, последняя 50×30 | колонки идут рядами высотой 50 px, ряды через `row-gap` (`column-height:auto` берёт определённую высоту контейнера, CSS Multicol L2 §`column-height`) |
| `columns:1` + `column-span:all` в середине | три блока подряд | (верно) |

`CSS-SPECS.md` пишет: «`auto` = `wrap`, когда задан `column-height`» — это работает; без `column-height` и без явного `column-wrap` результат верный (`nowrap`); не работает явное `column-wrap:wrap` при `column-height:auto`.

## Как найдено

WPT-RUN-14 срез 17: 16 id с `columns: 1` (из них `multicol-span-float-002…004`, `multicol-span-all-list-item-*`, `multicol-fill-balance-020/029`, `multicol-rule-nested-balancing-004` — ещё и по другим причинам, число id по правилу «в исходнике есть `columns:1`») и 3 id `column-wrap` без `column-height`.

## Что делать

`multicol_trampoline.rs`: считать контейнер multicol при `column_count == Some(1)`; в ветке `wrap` брать `column-height:auto` = высота контейнера, если она определённая.

## Как проверить

`css/css-multicol/column-height-011.html`, `column-height-003.html`.

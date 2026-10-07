# BUG-1397 — `contain: size` у замещаемых и grid, `contain: inline-size` в раскладке не применяются

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** layout (`crates/engine/layout/src/box_tree/layout_dispatch.rs:512`, `grid_vertical.rs:356`, `intrinsic.rs`; `ContainFlags::INLINE_SIZE` в раскладке не читается)

## Симптом

`--dump-layout`, `body{margin:0;font:20px/1 Ahem}`:

| проба | получено | ожидается (CSS Containment L1 §3.1, L3 `inline-size`) |
|---|---|---|
| `<img src="60x60-red.png" style="contain:size;padding:50px;background:green">` | 160×160 (как без `contain`) | 100×100 (внутренний размер 0 + padding) |
| `<canvas width=50 height=50 style="contain:size;padding:10px">` | 70×70 (как без `contain`) | 20×20 |
| `<div style="display:inline-grid;contain:size;grid:50px / 100px;background:green"><div style="height:80px"></div></div>` | 0×0 | 100×50 (явные треки задают размер) |
| то же без `contain` | 100×50 | 100×50 |
| `<div style="display:inline-block;contain:inline-size;background:green"><div style="width:300px;height:30px"></div></div>` | ширина 300 | 0 по inline-оси, высота 30 (то же у `inline-flex`) |
| `<div style="contain:inline-size">` (блок) | 800 | 800 |

Для блока, `inline-block` и `flex` с `contain:size` (высота 0, ширина по CSS) раскладка верна.

`ContainFlags::INLINE_SIZE` разбирается (`style/apply/layout.rs:911`) и читается только в `getComputedStyle` (`selector_query.rs:2078`); `grep ContainFlags::INLINE_SIZE crates/engine/layout/src/box_tree/` — пусто.

## Как найдено

WPT-RUN-14 срез 17: `contain-size-013/041/042`, `contain-size-replaced-001…007`, `contain-size-grid-001…006` (3 testharness, 68 из 103 сабтестов), `contain-inline-size-*` (14 id; `-flex`, `-grid`, `-table`, `-replaced`, `-fieldset`, `-legend`, `-multicol`, `-vertical-rl-`, `-bfc-floats-*`).

## Что делать

Заменяемые: при `SIZE` игнорировать собственный размер (натуральные width/height/ratio). Grid: при `SIZE` размер контейнера по явным трекам, содержимое не участвует. `INLINE_SIZE`: применять ту же ветку, что `size_contained`, только по inline-оси (`layout_dispatch.rs:512`).

## Как проверить

`css/css-contain/contain-size-013.html`, `contain-size-grid-003.html`, `contain-inline-size-flex.html`.

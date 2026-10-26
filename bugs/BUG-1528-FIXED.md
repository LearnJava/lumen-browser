# BUG-1528 — `align-self`/`justify-self`/`align-items` не влияют на статическую позицию абсолютного бокса в блочном контейнере; во flex-контейнере работает только ось блока

**Статус:** FIXED 2026-10-26 (P3); остаток — [BUG-1721](BUG-1721-OPEN.md)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — статическая позиция `position:absolute` бокса в блочном и flex-контейнере)

## Симптом

Абсолютный бокс 20×20 без инсетов (`top/left:auto`) в блоке 100×100 с `align-self: center; justify-self: center` остаётся в `(0,0)` (ожидается `(40,40)`). Во flex-контейнере `align-self:end; justify-self:end` — `y=80` верно, `x` остаётся 0 (ожидается 80). 9 reftest.

## Проба

Проба (`--mcp`, контейнер `position:relative;width:100px;height:100px`, цель `position:absolute;width:20px;height:20px`):

| контейнер | стили цели | `(x,y)` у нас | ожидается |
|---|---|---|---|
| `block` | `align-self:center; justify-self:center` | `(0,0)` | `(40,40)` |
| `flex` | `align-self:end; justify-self:end` | `(0,80)` | `(80,80)` |


## Как найдено

WPT-RUN-14 срез 24: `css-align/abspos/align-items-static-position`, `align-self-static-position-001…008` (кроме 005), `justify-self-static-position-001`. Родственный [BUG-1470](BUG-1470-OPEN.md) (статическая позиция внутри inline-контекста) и [BUG-1481](BUG-1481-FIXED.md) (бокс с обоими инсетами).

## Что делать

При определении статической позиции применять `align-self`/`justify-self` (и `align-items`/`justify-items` контейнера как умолчание) к свободному месту в содержащем блоке.

## Как проверить

`css/css-align/abspos/align-self-static-position-001.html`, `abspos/justify-self-static-position-001.html`.

## Исправлено

`box_tree/abs_static_align.rs` (вызов из `block_flow_trampoline::finish_frame` после `finish_after_match`): у абсолютного ребёнка блочного контейнера (`block`/`flow-root`/`inline-block`/`list-item`, `horizontal-tb`) по оси с обоими `auto`-инсетами `justify-self` (для `auto` — `justify-items` контейнера) и `align-self` выравнивают бокс в прямоугольнике статической позиции: по x это content box контейнера, по y — точка потока без высоты (поэтому `end` уводит бокс выше точки на его высоту, `center` — на половину), без hull-коррекции. Попутно `reposition_abs_child` (второй проход для потомков за непозиционированной обёрткой) теперь применяет `justify-self`/`align-self` между двумя инсетами, как `place_abs_child` (общая `aligned_between_insets`). Проба: `align-self-static-position-001`, `-006`, `align-self-with-flex-grid-parent` — попиксельно; `-002`, `-008`, `justify-self-static-position-001` — лучше (было 28750→15000, 5000→2500, 3000→1500 px).

Не вошло: прямоугольник-точка внутри inline-элемента, вертикальные режимы письма, `anchor-center` — см. [BUG-1721](BUG-1721-OPEN.md). Ось строки flex-контейнера берёт `justify-content`, а не `justify-self` — так в Flexbox §4.1, оставлено как есть.

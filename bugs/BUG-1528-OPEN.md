# BUG-1528 — `align-self`/`justify-self`/`align-items` не влияют на статическую позицию абсолютного бокса в блочном контейнере; во flex-контейнере работает только ось блока

**Статус:** OPEN
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

WPT-RUN-14 срез 24: `css-align/abspos/align-items-static-position`, `align-self-static-position-001…008` (кроме 005), `justify-self-static-position-001`. Родственный [BUG-1470](BUG-1470-OPEN.md) (статическая позиция внутри inline-контекста) и [BUG-1481](BUG-1481-OPEN.md) (бокс с обоими инсетами).

## Что делать

При определении статической позиции применять `align-self`/`justify-self` (и `align-items`/`justify-items` контейнера как умолчание) к свободному месту в содержащем блоке.

## Как проверить

`css/css-align/abspos/align-self-static-position-001.html`, `abspos/justify-self-static-position-001.html`.

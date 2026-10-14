# BUG-1544 — `filter: grayscale(300%)` и `sepia(300%)` не ограничиваются 100 %: цвет получается неверным

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** layout/paint (`crates/engine/layout/src/style/parse/transform.rs:236` `parse_filter_fn` — `grayscale`/`sepia` не ограничены 1)

## Симптом

Filter Effects 1: значения `grayscale()`, `sepia()`, `invert()`, `opacity()` выше 100 % приравниваются к 100 %. `grayscale(300%)` над красным даёт `(0,163,163)`, `grayscale(100%)` — `(54,54,54)`; `sepia(300%)` — `(0,255,208)` против `(100,89,69)` у `sepia(100%)`. `invert(300%)` и `opacity(300%)` совпадают со 100 % (ограничены). 1 id (`filter-grayscale-005.html`).

## Проба

Проба (`--screenshot`, красный блок 50×50, цвет `(25,25)`):

| фильтр | у нас | ожидается |
|---|---|---|
| `grayscale(100%)` | `(54,54,54)` | `(54,54,54)` |
| `grayscale(300%)` | `(0,163,163)` | `(54,54,54)` |
| `sepia(100%)` | `(100,89,69)` | `(100,89,69)` |
| `sepia(300%)` | `(0,255,208)` | `(100,89,69)` |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects/filter-grayscale-005.html`.

## Что делать

Ограничивать аргумент `grayscale()`/`sepia()` единицей при разборе или применении.

## Как проверить

`css/filter-effects/filter-grayscale-005.html`.

# BUG-1448 — `display: initial` и `display: unset` дают `block` вместо `inline` для любого элемента

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** layout (`crates/engine/layout/src/style/` — начальное значение `display` для `initial`/`unset`)

## Симптом

Начальное значение `display` — `inline` (CSS Display 3 §2.1), `unset` для неунаследуемого свойства то же. Мы возвращаем `block`: `<span style="display:initial">`, `<b style="display:unset">`, `<li style="display:initial">`, `<table style="display:unset">`, `.f{display:inline-block;display:unset}` на `<span>` → `getComputedStyle(…).display` = `block`. Явное `display:inline` и `display:revert` дают верное. Страдает `css-cascade/unset-val-002.html` (1 reftest); в `css/` 12 файлов используют `display: initial|unset|revert`.

## Проба

| разметка | `getComputedStyle(x).display` | ожидается |
|---|---|---|
| `<span id=x style="display:initial">` | `block` | `inline` |
| `<b id=x style="display:unset">` | `block` | `inline` |
| `<li id=x style="display:initial">` | `block` | `inline` |
| `<table id=x style="display:unset">` | `block` | `inline` |
| `<div id=x style="display:initial">` | `block` | `inline` |
| `<div style="display:inline">` | `inline` | верно |
| `span{display:block}` + `<span style="display:revert">` | `inline` | верно |

## Как найдено

WPT-RUN-14 срез 20: `css-cascade/unset-val-002.html` (красный `inline-block` + `display: unset` перекрывает зелёный квадрат).

## Что делать

Сопоставить ключевые слова `initial`/`unset` для `display` с `Display::Inline`, а не с блочным значением конструктора `ComputedStyle`.

## Как проверить

Таблица выше; `css/css-cascade/unset-val-002.html`.

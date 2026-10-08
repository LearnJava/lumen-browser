# BUG-1449 — `revert-layer`: из атрибута `style` и с `!important` не откатывает слой (11 reftest); `margin-block-start: revert` не возвращает UA-значение

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** layout (каскад: `revert-layer` в атрибуте `style` и с `!important`; `revert` логических `margin-*`)

## Симптом

(1) `<div id=t style="background-color:red; background-color:revert-layer">` при правиле `#t{background-color:green}` (без слоя) — фона нет совсем, ожидается зелёный (`revert-layer-009`, `-012` и др.); то же для `!important` в атрибуте. (2) Три слоя: `#t{…}`, `@layer{#t{background-color:green}}`, `@layer{#t{background-color:red; background-color:red !important; background-color:revert-layer !important}}`, `@layer{#t{background-color:red; background-color:red !important}}` — получаем красный, ожидается зелёный (`revert-layer-005`). Без `!important` и внутри слоёв (`@layer a{green}` + `@layer b{red;revert-layer}`) работает. (3) `margin:0; margin-block-start: revert` на `<h1>` → `0px`, ожидается `21.44px` (UA), хотя физическое `margin-top: revert` и `margin: revert` верны (`revert-val-005`, `-006`, `-007`: 3 сабтеста). 11 reftest `revert-layer-0NN` (10 `thick`) и 3 testharness.

## Проба

| проверка | у нас | ожидается |
|---|---|---|
| `#t{w/h:100px;bg:green}` + `style="bg:red;bg:revert-layer"` | фона нет | зелёный |
| то же с `!important` в обоих местах | фона нет | зелёный |
| `@layer a{#t{bg:green}}` + `style="bg:red;bg:revert-layer"` | зелёный | верно |
| три слоя с `revert-layer !important` (см. выше) | **красный** | зелёный |
| `h1{margin:0;margin-top:revert}` | `21.44px` | верно |
| `h1{margin:0;margin-block-start:revert}` | **`0px`** | `21.44px` |
| `h1{margin:0px;margin:revert}` | `21.44px` | верно |

## Как найдено

WPT-RUN-14 срез 20: `css-cascade/revert-layer-005.html`, `-009`, `-012`; `revert-val-005.html` (+ `-006`, `-007`).

## Что делать

(1) Атрибут `style` — отдельный шаг каскада: `revert-layer` из него откатывает к стилям листов, а не к «ничему». (2) Для `!important` порядок слоёв обратный — откат считать в обратном порядке. (3) `revert` для логических свойств — через сопоставление с физическим (по `writing-mode`/`direction`), как для остальных значений.

## Как проверить

Таблица выше; `css/css-cascade/revert-layer-009.html`, `css/css-cascade/revert-val-005.html`.

# BUG-1467 — `display: run-in` не реализован: значение читается как `block`, run-in-блок не вливается в следующий блок

**Статус:** OPEN (ДОРАБОТКА → RUNIN-1)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** css-parser/layout (`display: run-in` — нет ни значения, ни раскладки; `crates/engine/layout/src/style/`, `crates/engine/layout/src/box_tree/`)

## Симптом

`display: run-in` (CSS 2.1 §9.2.3, CSS Display 3 §5) разбирается как неизвестное значение и даёт `block`: `getComputedStyle(x).display` — `block`, а `CSS.supports('display','run-in')` — `true` (см. BUG-1475: проверка значения не выполняется вовсе). Заголовок-run-in остаётся отдельной строкой, а не вливается в начало следующего блока; `counter-increment`/`counter-reset`/`quotes`/`letter-spacing` на таком элементе не проверяются, потому что элемент остаётся блоком. 81 reftest `css-display/run-in/` (77 `run-in-*.xht` — раскладка, 4 `*-applies-to-*.xht` — свойства на run-in-боксе), все `thick`. Движков с поддержкой нет (Blink и WebKit убрали много лет назад, [csswg-drafts#9784](https://github.com/w3c/csswg-drafts/issues/9784) предлагает убрать из спецификации) — решение «делать ли» остаётся за пользователем.

## Проба

Проба (`--mcp`, `<div id=a style="display:run-in">`):

| проверка | у нас | ожидается |
|---|---|---|
| `getComputedStyle(a).display` | `block` | `run-in` |
| `<div style="display:run-in">hello</div><div>block</div>` — высота тела | 2 строки (58,08 px) | 1 строка: run-in вливается в блок |
| `<div id=a>` с `counter-increment:test 5; display:run-in` и `a:before{content:counter(test)}` | пусто (проверено ещё и для `display:block` — дело в BUG-1366: `:before` с одним двоеточием) | число `5` |
| `inline-block` для сравнения | `inline-block`, 36,13 px | — |

Снимок `counter-increment-applies-to-011.xht` на `--screenshot --viewport 800x600 --ahem`: одна строка «Test passes if…», ссылка рисует ещё `5` под ней.

## Как найдено

WPT-RUN-14 срез 21: 81 reftest `css-display/run-in/` (`run-in-basic-010.xht`, `counter-increment-applies-to-011.xht`, `run-in-run-in-between-005.xht`).

## Что делать

Решение по существу: делать ли функцию, которой нет ни в одном браузерном движке. Если делать — задача RUNIN-1: значение `run-in` в `Display`, перестройка дерева боксов (§5: вливание в следующий блок, не создающий BFC; иначе анонимный блок), `::first-letter` через run-in. Если нет — закрыть запись как «не будем» и вывести `css-display/run-in/` (237 файлов, 133 automatable) из знаменателя в `tests/wpt/metadata`.

## Как проверить

Таблица выше; `css/css-display/run-in/run-in-basic-010.xht`.

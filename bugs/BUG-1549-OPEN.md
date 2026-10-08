# BUG-1549 — Пустой `<li>` с `list-style-position: inside` имеет высоту 0: маркеры соседних `li` ложатся в одну точку

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout (`crates/engine/layout/src/box_tree/` — высота строки для `inside`-маркера у `li` без содержимого)

## Симптом

`<ol style="list-style-position:inside"><li></li><li></li></ol>`: у каждого `li` `rect.height = 0`, оба маркера — в `y=8`. По CSS Lists 3 §2.4 `inside`-маркер — inline-бокс в строке `li`, значит строка непустая и `li` высотой в строку. В эталонах WPT — по `<div>` на каждый маркер. 29 reftest `@counter-style` (`counter-style-at-rule/*`, `<li><li><li><li><li>`) и часть `start`-тестов.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `<ol style="list-style-position:inside"><li></li><li></li></ol>` — `Marker rect` первого и второго `li` | `(8, 8)`, `(8, 8)`; высоты `li` 0 | `y` 8 и 26; высота `li` по строке |
| A/B (`&#x200B;` в каждый пустой `li` в 43 тестах `@counter-style`) | 42 `thick`, 1 `no-match-ref` | часть `identical` |

## Как найдено

WPT-RUN-14 срез 25: `css/css-counter-styles/counter-style-at-rule/{access-from-shadow-dom,descriptor-*,system-*,symbols-function*}.html`. A/B с ZWSP не делает ни один id зелёным (у части — `\2023`-escape: BUG-1572 для `content`, BUG-1380 для значений; у части — BUG-1547/1566).

## Что делать

Строить для `li` с `inside`-маркером первую строку даже при отсутствии текста (маркер даёт содержимое строки), высота — высота строки маркера.

## Как проверить

`css/css-counter-styles/counter-style-at-rule/system-cyclic.html` после BUG-1547, BUG-1566, BUG-1572.

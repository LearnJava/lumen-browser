# BUG-1452 — `appendChild`/`insertBefore` в родителя с тысячами детей стоит O(n) за вызов: 8 000 вставок — 4,3 с, 16 000 — 17,3 с (квадратичный рост)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `appendChild` `:8612`, `insertBefore`, `firstChild`; не локализовано)

## Симптом

Время `for (i<N) parent.appendChild(document.createElement('span'))` растёт ×4 при удвоении N: N=2 000 — 321 мс, 4 000 — 1 119 мс, 8 000 — 4 269 мс, 16 000 — 17 316 мс. `insertBefore(x, null)` — 4,7 с на 8 000; сборка фрагмента — 4,35 с и вставка фрагмента — 4,35 с; присвоение `innerHTML` на 8 000 `<span>` — 81 мс. Одиночный вызов при 6 000 детях: `appendChild(x)` (перенос) — 946 мкс, `d.firstChild` — 7,3 мс. Отдельные внутренние помощники измерены и не объясняют цену: `_lumen_native_append_child` 4 мс на 4 000 вызовов, `_lumen_adopt_detached`, `_lumen_node_contains`, `_lumen_ce_*`, `_lumen_fire_slotchange`, `_lumen_make_element` — каждый ≤ 12 мкс на вызов при 6 000 братьях; `_lumen_get_children` на 4 000 детях — 560 мкс. Сумма измеренных частей — порядка 30 мкс против 946 мкс на вызов; недостающее не найдено (кандидат — копирование списка детей в `firstChild`/`childNodes`/`children`, 7,3 мс на `firstChild` — самая дорогая из измеренных операций). После серии из 50 переносов одного узла в родителя с 6 000 детей следующий `childNodes.length` бросил `DOM node limit exceeded` — как и почему, не исследовано. Последствие: `selectors/invalidation/has-complexity.html` (25 000 вставок) — TIMEOUT («browser did not answer a BiDi call within 20s»).

## Проба

| N | `appendChild` N новых `<span>` в `#c` | на удвоение |
|---|---|---|
| 2 000 | 321 мс | — |
| 4 000 | 1 119 мс | ×3,5 |
| 8 000 | 4 269 мс | ×3,8 |
| 16 000 | 17 316 мс | ×4,1 |
| 8 000 через `innerHTML` | 81 мс | — |

Проба (`--mcp`, `eval`, `performance.now()`): цикл из таблицы. Для сравнения 8 000 вызовов `_lumen_native_append_child` напрямую — 7 мс.

## Как найдено

WPT-RUN-14 срез 20: `selectors/invalidation/has-complexity.html` — TIMEOUT `browser did not answer a BiDi call within 20s`.

## Что делать

Профилировать `appendChild` на родителе с большим числом детей (счётчик `_lumen_get_children`/копий массива на вызов); искать линейный по числу братьев проход, не покрытый измеренными помощниками.

## Как проверить

Проба из таблицы (цикл по N); `css/selectors/invalidation/has-complexity.html`.

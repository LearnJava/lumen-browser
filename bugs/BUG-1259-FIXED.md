# BUG-1259 — `flex-wrap: balance` и `flex-line-count` (CSS Flexbox L2, `#algo-balance`) не реализованы: `enum FlexWrap` знает только `nowrap`/`wrap`/`wrap-reverse`

**Статус:** FIXED 2026-10-23 (P4)
**Заведён:** 2026-10-03 (P2, WPT-RUN-14 срез 1, `css/css-flexbox`)
**Область:** css-parser/layout (`crates/engine/layout/src/style/values/flexgrid.rs` — `FlexWrap`)

## Симптом

`flex-wrap: balance` и `flex-line-count` (CSS Flexbox L2, `#algo-balance`) не реализованы: `enum FlexWrap` знает только `nowrap`/`wrap`/`wrap-reverse` (слово `balance` есть лишь у `text-wrap-style`). WPT-RUN-14-S1: 40 id `css/css-flexbox/balance/*` (reftest'ы с `flex-wrap: balance` и `getComputedStyle`-тесты, 23 сабтеста).

## Описание

ДОРАБОТКА → `CSS-SPECS.md` (P4): свойство и значение отсутствуют. Строка добавлена в таблицу Flexbox `CSS-SPECS.md`.

## Как найдено

WPT-RUN-14 срез 1.

## Исправление

`flex-wrap: nowrap | [wrap | wrap-reverse] || balance` (`FlexWrap::parse_balance`; флаг `ComputedStyle::flex_wrap_balance`, `wrap` остаётся базой) и `flex-line-count: <integer [1,∞]>` (initial 1; `calc()`/`round()` через `math_value::integer_property`). Раскладка — `box_tree/flex_balance.rs`: число строк = жадное (или `flex-line-count`, не больше числа элементов); при этом числе выбирается разбиение на смежные серии с минимальной суммой квадратов остатка `(avail − ширина строки)²`, при равенстве — больше элементов в первой строке, затем во второй…; внешний размер элемента для разбиения не ниже 0. Авто-высота колонки: доступный размер = сумма всего, строки режет только `flex-line-count`; элементы с `width:auto` меряются в доле контейнера на одну строку. Intrinsic: max-content ряда = минимум по разбиениям на N строк максимальной строки, колонки — сумма самых широких элементов строк.

Остаток (не отдельные баги): `balance-available-size-002` (элементы с `writing-mode: vertical-rl` в ряду — доля поперечного размера по высоте); `balance-negative-margin-002` div 3 — распределение свободного места при отрицательных margin (общий flex-путь, не balance); `flex-wrap-valid`/`-invalid`, `flex-line-count-invalid` — канонизация/валидация сеттера `element.style` (BUG-484).

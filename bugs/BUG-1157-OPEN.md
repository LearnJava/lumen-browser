# BUG-1157 — `getComputedStyle(el).transform` отдаёт заданное значение, а не `matrix(…)`

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-1121](BUG-1121-FIXED.md))
**Область:** layout — `crates/engine/layout/src/selector_query.rs:1591`
(`computed_style_to_map`: `transform_list_to_css(&style.transform)` сериализует список функций
как задан).

## Симптом

Видимое окно, `--maximized`, без блокировщика, Chrome 153:

| `transform` | Lumen | Chrome |
|---|---|---|
| `translateX(10px)` | `translateX(10px)` | `matrix(1, 0, 0, 1, 10, 0)` |
| `scale(2) rotate(0deg)` | `scale(2, 2) rotate(0deg)` | `matrix(2, 0, 0, 2, 0, 0)` |
| `translate3d(1px,2px,3px)` | `translate3d(1px, 2px, 3px)` | `matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 3, 1)` |
| `translateX(5px) translateY(3px)` (через `style`) | как задано | `matrix(1, 0, 0, 1, 5, 3)` |
| нет | `none` | `none` |

CSSOM §9 «resolved value»: для `transform` это `matrix()` или (для 3D) `matrix3d()`,
посчитанные из списка функций, а не сам список.

## Реальный сайт

imgur: после [BUG-1121](BUG-1121-FIXED.md) первая ошибка верхнего уровня —
`TypeError: Cannot read properties of null (reading '1')` в `transformPose` → `NgSM.t.setPose`
(popmotion-pose). Чтение позы:
`var a=i.match(/^matrix3d\((.+)\)$/);return a?Kr(a[1],t):Kr(i.match(/^matrix\((.+)\)$/)[1],e)`,
где `i = getComputedStyle(o).transform`. На `translateX(…)` оба `match` дают `null`. Страница —
106 узлов против 1231 в Chrome. Что эта ошибка — единственная причина пустой страницы, не
доказано: это следующий барьер после `document.referrer`.

## Что сделать

Сериализовать resolved value: перемножить функции списка в матрицу 4×4 (проценты в
`translate` — от border box, как в paint) и вывести `matrix(a, b, c, d, e, f)`, если матрица 2D,
иначе `matrix3d(…)`; `none` — как есть. Проверить, кто ещё читает эту карту (`CSSStyleDeclaration`
computed-объект, Typed OM `computedStyleMap`): у Typed OM своя сериализация, её не трогать.
Критерий: таблица выше совпадает с Chrome; на imgur нет ошибки `reading '1'`.

# BUG-1847 — `getComputedStyle().transform` после завершённого transition показывает значение закрытого состояния

**Статус:** OPEN
**Заведён:** 2026-11-26 (ZCode, portal.nextera.ru, сравнением с Chrome 158.0.8127.37)
**Область:** layout ([`resolved_geometry.rs:424`](../crates/engine/layout/src/resolved_geometry.rs) — `resolved_transform`, computed-значения)

## Симптом

`aside#ppSidePanel` портала, `transition: transform 0.22s, visibility 0.22s`; открытие панели
(закреплённый режим `pp-left-pin`, ширина 300) убирает `translateX(-300px)`. Через ≥1 с после клика:

| | Chrome 158 | Lumen a0666c3564+патч |
|---|---|---|
| `getBoundingClientRect().x` панели | `0` | `0` (панель на месте, `elementFromPoint` внутри неё попадает в элементы) |
| `getComputedStyle(panel).transform` | `none` | `matrix(1, 0, 0, 1, -300, 0)` |

Повторено дважды на разных значениях: `−360` (неподзакреплённая панель) и `−300` (закреплённая) —
computed-стиль всегда показывает значение закрытого состояния, раскладка при этом переключилась.

Замер синтетическим transition в offscreen-пробе недостоверный (offscreen-контейнеры в Lumen не
считаются вовсе — ловушка метода), свидетельство — только живая страница.

Пиксельная отрисовка панели в этом состоянии не проверялась (окно перекрыто,
`x-scope-screenshot` для живого окна не реализован): неизвестно, рисует ли paint по тому же
устаревшему transform или только computed-стиль врёт.

## Что делать

Computed-значение transitioned-свойства после завершения перехода — конечное значение (`none`),
а не застрявший кадр. Сверять `resolved_transform` с состоянием раскладки, из которой он берётся.

## Как проверить

Живая страница портала: открыть/закрепить панель, через 1 с `getComputedStyle().transform` —
`none`, как в Chrome; `rect.x` и computed согласованы. Смежные: BUG-1248 (SMIL `animVal` — другой
механизм), BUG-1293/BUG-536 (FIXED — интерполяция computed `transform`).

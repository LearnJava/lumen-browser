# BUG-1435 — `getComputedStyle()` не возвращает `rotate`, `scale`, `translate`, `tab-size`, `interpolate-size`, `image-orientation`, `object-view-box`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 19, `css/css-images` + `css/css-values` + `css/css-color`)
**Область:** layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`)

## Симптом

`el.style.cssText = prop:value; getComputedStyle(el).getPropertyValue(prop)`:

| свойство | получено | ожидается |
|---|---|---|
| `rotate:45deg` | `` | `45deg` |
| `scale:2` | `` | `2` |
| `translate:5px` | `` | `5px` |
| `tab-size:calc(1 + 2)` | `` | `3` |
| `transform:rotate(2deg)` (контроль) | `matrix(0.99939084, 0.034899496, …)` | так же |
| `z-index:3` (контроль) | `3` | так же |

`--dump-display-list` для `rotate:90deg`: `PushTransform [-0.000 1.000 -1.000 -0.000 55.000 -45.000]` — значение хранится и применяется,
в карту `computed_style_to_map` его просто нет.

## Как найдено

WPT-RUN-14 срез 19: почти все `*-computed.html` в `css-values` проверяют числовое вычисление через `scale`/`rotate`:
`assert_true: scale doesn't seem to be supported in the computed style` (`random-computed.tentative.html` 143 из 152),
`assert_not_equals: round(10,10) isn't valid in 'scale'; got the default value instead` (`round-mod-rem-computed` 176/243),
`minmax-number-computed`, `acos-asin-atan-atan2-computed` (`rotate`), `progress-computed`, `calc-mix-computed`. Без записи этих
свойств в computed тесты не могут проверить вычисление функций вообще — [BUG-1430](BUG-1430-OPEN.md) и этот баг вместе закрывают
большую часть `css-values`.

## Что делать

Добавить `rotate`/`scale`/`translate`/`tab-size` (и `interpolate-size`, `image-orientation`, `object-view-box` после их разбора) в
`computed_style_to_map` в канонической форме (`none` или значение, `calc()` вычислен).

## Как проверить

Страница из таблицы; `css/css-values/round-mod-rem-computed.html`, `random-computed.tentative.html`.

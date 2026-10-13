# BUG-1589 — Motion Path в CSSOM: `getComputedStyle` не отдаёт ни одного `offset-*`, `offset-position` не разбирается, невалидное принимается, сериализация неканонична

**Статус:** OPEN
**Заведён:** 2026-10-09 (P2, WPT-RUN-14 срез 27, `css/css-ruby` + `css-layout-api` + `css-box` + `motion` + `css-highlight-api` + `css-paint-api` + `css-viewport` + `mediaqueries`)
**Область:** layout/js (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; разбор `offset-position`; сериализация `offset-*`)

## Симптом

Три независимые поломки в одном CSSOM-кластере:

1. **`getComputedStyle()`** не знает `offset`, `offset-path`, `offset-distance`, `offset-rotate`, `offset-anchor`, `offset-position` — тест `*-computed.html` падает на `assert_true: … doesn't seem to be supported in the computed style` во всех сабтестах (сабтестов: 6, 5, 14, 15, 65, 12 в шести файлах `parsing/*-computed.html`).
2. **`offset-position`** как свойство не существует: `CSS.supports("offset-position","normal")` — `false`; тесты `animation/offset-position-composition.html` (56 сабтестов) и `offset-position-interpolation.html` (312) падают на `'from' value should be supported`. `ray-012`/`ray-013` (`offset-position: 10px 90%`) — тоже.
3. **Разбор и сериализация**: недопустимые значения принимаются, допустимые не приводятся к канону.

## Проба

`--dump-layout` + `console.log`, `d.style.setProperty(prop, v); d.style.getPropertyValue(prop)`:

| вызов | у нас | ожидается |
|---|---|---|
| `offset-distance: foo`, `offset-distance: 10px 20px` | `foo`, `10px 20px` | `""`, `""` |
| `offset-path: path("M 20 30 A 60 70 80")` (неполная дуга) | как записано | `""` |
| `offset-rotate: 5turn auto` | `5turn auto` | `auto 5turn` |
| `offset-distance: 0` | `0` | `0px` |
| `offset-path-parsing-valid`: сериализация `ray(…)` (сообщение теста) | `ray(0rad closest-side)` | `ray(0rad)` |
| `offset: 10px 30px`, `offset-position: auto` | принимаются | допустимо; в `computed` — `""` |
| `getComputedStyle(e).getPropertyValue(k)` для `offset-path`, `offset-distance`, `offset-rotate`, `offset-anchor`, `offset-position`, `offset` | `""` ×6 | значения |
| `offset-path: shape(from 0px 0px, line to 10px 10px)` | пробелы сохранены как в исходнике (`shape(  from 0px    0px, …`) | единичные пробелы |

## Как найдено

WPT-RUN-14 срез 27: `css/motion/parsing/*` (21 id), `motion/inheritance.html`, `motion/animation/offset-position-*`, `motion/offset-supports-calc.html`.

## Что делать

Добавить шесть `offset-*` и шорткод `offset` в `computed_style_to_map` (канонические значения: длины в `px`, углы, `normal`/`auto`); завести `offset-position` в `ComputedStyle` и разбор (используется в `ray()`-расчёте как стартовая точка); проверка допустимости в `style.setProperty` — как у остальных свойств; канонизация порядка `offset-rotate`. Не путать с BUG-1050/BUG-1278/BUG-1515 — другие свойства, тот же файл.

## Как проверить

`css/motion/parsing/offset-distance-computed.html`, `offset-rotate-parsing-valid.html`, `animation/offset-position-interpolation.html`.

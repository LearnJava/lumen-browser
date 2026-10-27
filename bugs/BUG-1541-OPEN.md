# BUG-1541 — CSSOM `filter`/`backdrop-filter`: `backdrop-filter` нет в `getComputedStyle`; недопустимое принимается; `blur(0)` не канонизируется; `flood-*`/`lighting-color`/`color-interpolation-filters` не разбираются

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** css-parser/layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; `style/parse/transform.rs` — разбор `filter`/`backdrop-filter`)

## Симптом

`getComputedStyle(el).backdropFilter` — `""` (`'backdrop-filter' in cs` — `false`). `filter = "blur(5px 6px)"`, `"auto"`, `blur(-1px)` принимаются (должны отвергаться); сериализация `blur(0)` не приводится к `blur(0px)`; `filter: url(#x)` и `drop-shadow(…)` в computed — `none`; `backdrop-filter: !important` читается с суффиксом `!important`. `flood-color`, `flood-opacity`, `lighting-color`, `color-interpolation-filters`: `CSS.supports` `false` (9+9+2+5 сабтестов «doesn't seem to be supported in the computed style»). WAAPI: `el.animate({filter:["blur(0px)","blur(20px)"]})` + `pause()` + `currentTime=2000` — `getComputedStyle().filter` — `blur(20px)` (ожидается `blur(10px)`, интерполяция `filter` не работает — [BUG-1234](BUG-1234-OPEN.md)). 24 id, 560 из 877 сабтестов (+ 7 id / 672 из 954 — `animation/*-interpolation*`).

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `getComputedStyle(t).backdropFilter` для `blur(5px)` | `""` | `blur(5px)` |
| `style.filter="blur(5px 6px)"` → computed | `none` (в `style` — записано) | отвергнуто |
| `style.filter="auto"` → `style.filter` | `auto` | `""` |
| `style.filter="blur(0)"` → `style.filter` | `blur(0)` | `blur(0px)` |
| `style.filter="url(#x)"` → computed | `none` | `url("#x")` |
| `CSS.supports("flood-color","red")` | `false` | `true` |


## Как найдено

WPT-RUN-14 срез 24: `css/filter-effects/parsing/{filter,backdrop-filter}-{computed,parsing-valid,parsing-invalid}`, `parsing/{flood-color,flood-opacity,lighting-color,color-interpolation-filters}-*`, `inheritance.html`, `backdrop-filter-important`, `filter-sign-function`, `animation/*`, `drop-shadow-currentcolor-inheritance`, `idlharness.any.html` (306 из 485: `SVGFilterElement` и интерфейсы `SVGFE*` перечислимы/отсутствуют — отдельная причина, SVG-FILTER).

## Что делать

Добавить `backdrop-filter` в карту; отвергать недопустимые аргументы/`auto`/отрицательные `blur`; канонизировать `blur(0)`; разбор и карта для `flood-color`/`flood-opacity`/`lighting-color`/`color-interpolation-filters`; `url()` в computed — после SVG-FILTER.

## Как проверить

`css/filter-effects/parsing/backdrop-filter-computed.html`, `parsing/filter-parsing-valid.html`.

## Срез 1 (2026-10-27, P3)

Сделано: `backdrop-filter` в `computed_style_to_map`; строгий разбор `filter`/`backdrop-filter` (`parse_filter_list_strict`: неизвестная функция, `auto`, `none` + функция, безразмерное `blur(10)`/`hue-rotate(90)`, отрицательные количества, процентные длины — объявление отвергается); пустые скобки дают значение по умолчанию; `grayscale/invert/opacity/sepia` зажаты единицей; `calc()` в аргументах (число/процент через `math_value`, длина через `parse_length`; отрицательный результат math зажимается в 0); JS-сеттер `style.filter/backdropFilter` — `_lumen_css_canonical_filter` (канон `blur(0)`→`blur(0px)`, `drop-shadow` с цветом первым, отказ недопустимого). Тесты: `computed_map_backdrop_filter_and_filter_canonical_forms`, `computed_map_filter_invalid_value_is_dropped`, `v8_bug1541_filter_cssom`. WPT `css/filter-effects/parsing` (замер до добавления `calc()`): 258/339; `filter`/`backdrop-filter` `parsing-valid` 87/87 и 37/37, `parsing-invalid` 25/25 и 25/25. Файлы `.ini` в `tests/wpt/metadata/css/filter-effects/parsing/` не перегенерированы (порты WPT заняла чужая сессия) — `run_report.py --update-expected --root css/filter-effects/parsing`.

Остаток: `flood-color`/`flood-opacity`/`lighting-color`/`color-interpolation-filters` как CSS-свойства (поля ComputedStyle, наследование `color-interpolation-filters`, canonical в computed; сейчас читаются только из атрибутов в `box_tree/svg_filter.rs`); внешний `url(file.svg#id)`; `cqw` в math внутри filter; WAAPI — BUG-1234.

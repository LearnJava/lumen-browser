# BUG-1529 — CSSOM свойств CSS Inline: `baseline-shift` нет в `getComputedStyle`; `vertical-align`/`line-height` принимают `auto`, `0` не канонизируется, `line-height: normal` читается как `1.2`

**Статус:** FIXED 2026-10-26 (P3); остаток — [BUG-1724](BUG-1724-OPEN.md)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 24, `css/filter-effects` + `css-inline` + `css-tables` + `css-align`)
**Область:** css-parser/layout (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`; `style/parse/` — `vertical-align`, `line-height`, `baseline-shift`)

## Симптом

`'baseline-shift' in getComputedStyle(el)` — `false`, хотя `CSS.supports('baseline-shift','top')` — `true`. `line-height: normal` в computed — `1.2` (должно быть `normal`); `line-height: auto`, `-1` принимаются (недопустимы). `vertical-align: auto` принимается и читается как `baseline`. `initial-letter`: значение хранится (`style.initialLetter = "3 2"` читается), но `CSS.supports('initial-letter','3 2')` — `false`, в `getComputedStyle` нет. 11 id, 79 из 127 сабтестов (+ 5 id / 69 из 75 для `initial-letter`, кластер «CSSOM `initial-letter`»).

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `"baseline-shift" in getComputedStyle(t)` | `false` | `true` |
| `style.lineHeight="normal"` → computed | `1.2` | `normal` |
| `style.lineHeight="auto"` / `"-1"` | принято | отвергнуто |
| `style.verticalAlign="auto"` → computed | `baseline` | отвергнуто (`""`) |
| `CSS.supports("initial-letter","3 2")` | `false` | `true` |


## Как найдено

WPT-RUN-14 срез 24: `css-inline/parsing/{baseline-shift,line-height,vertical-align}-{computed,valid,invalid}`, `inheritance.html`, `baseline-shift/baseline-shift-sub-super*`, `initial-letter/initial-letter-{computed,valid,invalid,sign-function}`. `alignment-baseline`, `dominant-baseline`, `baseline-source`, `text-box*` в разбор не входят вовсе — это строки `CSS-SPECS.md`, не дефект.

## Что делать

Добавить `baseline-shift` в карту; `line-height: normal` отдавать как `normal`; отвергать `auto`/отрицательные у `line-height` и `auto` у `vertical-align`; включить `initial-letter` в `CSS.supports` и карту (resolved: `normal` или `<number> <integer>`).

## Как проверить

`css/css-inline/parsing/baseline-shift-computed.html`, `parsing/line-height-computed.html`, `initial-letter/initial-letter-computed.html`.


## Что сделано

- `element.style` (`style/values/text_cssom.rs`, маршрут `_LUMEN_TEXT_PROPERTIES` в шиме): грамматика и каноническая запись `line-height` (`normal | <number ≥ 0> | <length-percentage ≥ 0>`), `baseline-shift` (`<length-percentage> | sub | super | top | center | bottom`; `0` → `0px`, число без единицы отвергается), `vertical-align` по CSS Inline L3 (`[first | last] || <alignment-baseline> || <baseline-shift>`: `auto`, `none`, повтор части, два сдвига отвергаются; начальные части опускаются), `initial-letter` (`normal | n m | n && [drop | raise]?`).
- `getComputedStyle` (`selector_query.rs::computed_style_to_map`): `line-height` — `normal` или длина в px (число умножается на `font-size`), `baseline-shift` и `initial-letter` появились в карте.
- Каскад: отрицательный `line-height` отбрасывается, `%` внутри `calc()` считается от `font-size`, отрицательная сумма зажимается в `0`; `SvgBaselineShift` получил `top`/`center`/`bottom` (SVG-текст их не сдвигает); `initial-letter` понимает `drop`/`raise` (`ComputedStyle::initial_letter_keyword`); `initial-letter` добавлен в `SUPPORTED_PROPERTIES`.
- WPT `css/css-inline/parsing`: 71 → 120 из 159 сабтестов; `initial-letter/*` testharness: 33 из 33.
- Тесты: `selector_query::tests::computed_map_inline_properties_bug1529`, `style::tests::initial_letter_parse`, `js::dom::tests::v8_bug1529_inline_cssom`.

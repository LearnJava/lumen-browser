# BUG-1529 — CSSOM свойств CSS Inline: `baseline-shift` нет в `getComputedStyle`; `vertical-align`/`line-height` принимают `auto`, `0` не канонизируется, `line-height: normal` читается как `1.2`

**Статус:** OPEN
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

# BUG-1325 — CSSOM свойств CSS Text: `getComputedStyle()` отдаёт `""`, невалидное значение принимается, сериализация не каноническая

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 9, `css/css-text`, первая половина)
**Область:** js/layout (`crates/layout/src/selector_query.rs::computed_style_to_map` — нет строк для перечисленных ниже свойств; `crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand` — для них нет грамматики; тот же класс, что [BUG-1254](BUG-1254-OPEN.md) для flex, [BUG-1278](BUG-1278-OPEN.md) для CSS UI, [BUG-1307](BUG-1307-OPEN.md) для grid)

## Симптом

Проба (`run_report.py --all`, временный `test()`; `d` — `<div>`):

| свойство | `CSS.supports` | `d.style.<p> = v` читается | `getComputedStyle(d).<p>` |
|---|---|---|---|
| `tab-size`, `text-wrap`, `text-wrap-mode`, `text-wrap-style`, `line-break`, `word-break`, `overflow-wrap`, `hyphens`, `text-align-last` | true | `Ss` — значение сохраняется как есть | **`""`** |
| `white-space-collapse` | false | да | да (единственное с обеими) |
| `letter-spacing`, `text-indent`, `text-align`, `text-transform` | true | да | да, но значение неверно: `letter-spacing: normal` → `0px`; `text-indent: 10px each-line` → `calc(10px - 0.5em)`/`100%`; `text-align: justify` → `center`; `text-transform: full-width` → `none` |
| `word-wrap` (алиас `overflow-wrap`) | true | да | `""`; в `style.cssText` сериализуется как `word-wrap`, а не как `overflow-wrap` |

Невалидное значение не отклоняется: `lineBreak = "none"`, `tabSize = "-10px"`, `letterSpacing = "auto"`, `hyphens = "normal"`, `overflowWrap = "auto"`, `textAlignLast = "none"`, `wordBreak = "auto"` читаются обратно как записано (проба: `parsing/*-invalid.html`, «expected "" but got …»). Каноническая форма не строится: `hyphenate-limit-chars: auto auto auto` → не `auto`, `text-wrap: wrap` → не `wrap`, `white-space: normal` → не канон.

## Как найдено

WPT-RUN-14 срез 9, `css/css-text/parsing/` (86 id, 64 не зелёных, 483 сабтеста) и `inheritance.html`, `tab-size/tab-size-computed-value-001.html`, `overflow-wrap/word-wrap-alias.html`, `line-break/line-break-var-substitution.html`, `text-align/text-align-webkit-match-parent.html` (5 id / 44 сабтеста). Разложение parsing: реализованные свойства — 41 id / 276 сабтестов (`*-computed` 35, `*-invalid` 11, `*-valid` 8: `hyphens`, `line-break`, `tab-size`, `text-align*`, `letter-spacing`, `overflow-wrap`, `text-wrap*`, `text-indent`, `text-transform`, `white-space*`, `word-break`, `word-spacing`, `word-wrap`); отсутствующие свойства — 23 id / 207 сабтестов (см. CSS-SPECS.md:76: `hanging-punctuation`, `hyphenate-character`, `hyphenate-limit-chars`, `text-autospace`, `text-fit`, `text-group-align`, `text-justify`, `text-spacing`, `text-spacing-trim`, `word-space-transform`) — не дефект, а нереализованные свойства.

## Что делать

Строки в `computed_style_to_map` для реализованных свойств (указанное/вычисленное значение по спецификации свойства, `normal` у `letter-spacing`/`word-spacing`), грамматика в `_lumen_canonicalize_longhand` (валидатор значений и каноническая сериализация — по таблице свойств CSS Text), `word-wrap` как алиас `overflow-wrap` в `style` и `cssText`. Для отсутствующих свойств — P4 (CSS-SPECS.md:76).

## Как проверить

`css/css-text/parsing/tab-size-computed.html`, `line-break-invalid.html`, `hyphens-computed.html`, `text-wrap-valid.html`; проба одной строкой `getComputedStyle(document.body).tabSize !== ''`.

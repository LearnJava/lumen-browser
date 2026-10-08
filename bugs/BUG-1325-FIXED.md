# BUG-1325 — CSSOM свойств CSS Text: `getComputedStyle()` отдаёт `""`, невалидное значение принимается, сериализация не каноническая

**Статус:** FIXED 2026-10-08 (P6)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 9, `css/css-text`, первая половина)
**Область:** js/layout (`crates/layout/src/selector_query.rs::computed_style_to_map` — нет строк для перечисленных ниже свойств; `crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand` — для них нет грамматики; тот же класс, что [BUG-1254](BUG-1254-FIXED.md) для flex, [BUG-1278](BUG-1278-OPEN.md) для CSS UI, [BUG-1307](BUG-1307-OPEN.md) для grid)

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

## Исправление

Измерение — `run_report.py --all --root css/css-text/parsing`: **271 → 532 из 754 сабтестов**. Из оставшихся 222 свойств, которых в движке нет вовсе (`hanging-punctuation`, `hyphenate-*`, `text-autospace`, `text-spacing*`, `text-fit`, `text-justify`, `text-group-align`, `word-space-transform`, `text-align-all` — 212, это P4, CSS-SPECS.md:76), и 10 сабтестов, вынесенных в [BUG-1438](BUG-1438-OPEN.md) (проценты в `letter-spacing`/`word-spacing`, числовой `calc()` в `tab-size`).

- **`computed_style_to_map`** (`selector_query.rs`): добавлены `tab-size`, `text-wrap`, `text-wrap-mode`, `text-wrap-style`, `line-break`, `word-break`, `overflow-wrap` + `word-wrap`, `hyphens`, `text-align-last`. `letter-spacing: 0` → `normal`; `white-space` собирается из пары (`white-space-collapse`, `text-wrap-mode`), `preserve-breaks nowrap` пишется длинно; `text-transform` и `text-indent` несут `full-width`/`full-size-kana`/`math-auto` и `hanging`/`each-line`; `text-indent: calc(10px + 0.5em)` сворачивается в px.
- **Каскад** (`style/apply/text.rs`): `text-align: justify | justify-all | match-parent` (`match-parent` резолвится в конце `compute_style`, у корня — `start`), `text-align-last: match-parent`, `word-break: auto-phrase`, `white-space: <collapse> || <wrap-mode>`, `tab-size` числом (`TextCssomExtra::tab_size_number`), `text-indent` с модификаторами, `text-transform` с новыми компонентами. Layout этих компонент не читает — они лежат в `ComputedStyle::text_extra` только для CSSOM.
- **JS-шим** (`web_api_shim_mid.js`): ключевые слова вынесены в `_LUMEN_KEYWORD_PROPERTIES`; `word-wrap` — псевдоним `overflow-wrap` (`_lumen_camel_to_kebab`, `_lumen_parse_style`); шорткоды `white-space` и `text-wrap` раскладываются на лонгхенды и собираются обратно (общий `text-wrap-mode`; в `cssText` его забирает `white-space`); пустое присваивание шорткоду и `removeProperty` снимают оба лонгхенда; длины и составные грамматики идут через native `_lumen_css_canonical_text` → `style::values::text_cssom` (сумма `calc()` упорядочена по CSS Values 4 §10.10: `calc(2ch - 30%)` → `calc(-30% + 2ch)`).
- `CSS.supports` знает `white-space-collapse` и `word-wrap` (`SUPPORTED_PROPERTIES`).

Тесты: `style::tests::text_cssom_tests`, `selector_query::tests::css_text_longhands_are_serialised`, `text_align_match_parent_resolution`, `dom::tests::v8_bug1325_css_text_cssom` (lumen-js, `--features v8-backend`).

**Что показало сравнение с прежним бинарём.** `css/css-text` целиком: +288 / −281 сабтестов. «Минус» — не регресс функции: `interpolation-testcommon.js` сравнивает анимированный элемент с эталонным, у несуществующего в карте свойства оба значения были `""`, и такие тесты проходили вхолостую. Теперь они честно красные — см. [BUG-1437](BUG-1437-OPEN.md). `css/cssom`, `css-overflow`, `css-ui`, `css-inline`, `css-writing-modes` — без регрессий (в `cssom`/`css-overflow` +9). Метод — `docs/probe-method.md` §4.

**Ожидания WPT** (`tests/wpt/metadata`) обновлены `--update-expected --processes 7` только для корней, где правка что-то меняла: `css/css-text/{parsing,animations,text-align,tab-size}` и верхний уровень `css/css-text`; `css-pseudo/first-line-allowed-properties.html.ini` дополнен тремя `wordBreak` вручную.

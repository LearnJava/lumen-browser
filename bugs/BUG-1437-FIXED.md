# BUG-1437 — анимации и переходы свойств CSS Text (`tab-size`, `text-indent hanging/each-line`, `text-align-last`, `line-break`, `hyphens`) и `::first-line` отдают неверные computed-значения

**Статус:** FIXED 2026-10-21 (P3)
**Заведён:** 2026-10-08 (P6, побочная находка BUG-1325)
**Область:** layout (`crates/engine/layout/src/animation.rs` — интерполяция/дискретные переходы этих свойств; `style/pseudo.rs` — `::first-line` не фильтрует неразрешённые свойства)

## Симптом

Против прежнего бинаря (`css/css-text`, `--log-raw`): после [BUG-1325](BUG-1325-FIXED.md) свойства появились в `getComputedStyle()`, и тесты, проходившие вхолостую, честно красные:

| тест | сабтестов | сообщение |
|---|---|---|
| `animations/tab-size-interpolation.html` | 149 | `expected "0" but got "5"` — число в `@keyframes` не интерполируется, значение стоит на середине |
| `animations/text-indent-interpolation.html` | 53 | `expected "0px each-line" but got "10px"` — `hanging`/`each-line` дискретны (CSS Text L3 §7.1), переход между ними не отрабатывается |
| `text-align/text-align-last-interpolation.html` | 49 | дискретный переход `auto → start` (`transition-behavior: allow-discrete`) |
| `animations/text-indent-composition.html` | 10 | `add`/`accumulate` с модификаторами |
| `animations/line-break-no-interpolation.html`, `hyphen-no-interpolation.html` | по 10 | дискретный переход |
| `css-pseudo/first-line-allowed-properties.html` | 3 | `getComputedStyle(el, '::first-line').wordBreak` → `break-all`, ожидается `normal` (`word-break` не входит в разрешённые свойства `::first-line`); для `display: flex`/`inline` — пустая строка |

## Почему раньше «проходило»

`interpolation-testcommon.js` сравнивает значение анимированного элемента с эталонным; у свойства, которого нет в карте, оба `""`.

## Что делать

Подключить свойства к `animation.rs` (число — интерполяция, остальные — дискретно с `allow-discrete`), модификаторы `text-indent` — дискретная часть. `::first-line`: применять к computed только разрешённые свойства (CSS Pseudo L4 §first-line-styling).

## Как проверить

`run_report.py --all --root css/css-text/animations --recursive`; `css/css-pseudo/first-line-allowed-properties.html`.

## Исправление

Замер до правки (`css/css-text/animations`, 10 файлов): 733/1136 сабтестов. После: 956/1136; без `hyphenate-character`/`hyphenate-limit-chars` (нереализованные свойства, ниже) — все файлы списка зелёные: `tab-size-interpolation` 210/210, `text-indent-interpolation` 252/252, `text-indent-composition` 62/62, `line-break-no-interpolation` 42/42, `letter-spacing-interpolation` 144/144, `word-spacing-interpolation` 144/144.

Корни — четыре, ни один не в `animation.rs` (там живёт только планировщик кадров для `opacity`/`transform`/цветов/`height`; остальные свойства анимирует JS-оверлей BUG-1293):

1. **CSS-wide keyword в ключевом кадре.** `from { tab-size: initial }`, `element.animate({textIndent: ['inherit', '20px']})` интерполировали слово. `_wa_wide_resolve` (`web_api_shim_tail_b.js`) разворачивает `initial` (нативный `_lumen_css_initial_value` — карта корневого стиля), `inherit` (вычисленное значение родителя), `unset` (по таблице наследуемых `is_inherited_property`, `css_wide_value.rs`). Работает для всех свойств, не только текстовых.
2. **Каскад.** `apply_css_wide_keyword` не знал `tab-size`, `hyphens`, а у `text-indent` не сбрасывал `hanging`/`each-line`, у `text-transform` — `full-width`/`full-size-kana`; переход «из `initial`» стартовал с неверного значения.
3. **Значения.** `text-indent` приводится к канонической записи (`50px each-line hanging` → `50px hanging each-line`) до сравнения; модификаторы различаются — дискретный переход; сумма `%` + длина — `calc(100% + 50px)` (интерполяция и `add`/`accumulate`); `tab-size` не уходит в минус; число и длина (`1` и `10px`) не смешиваются; `normal` у `letter-spacing`/`word-spacing` — `0` между концами (у `letter-spacing` на прогрессе 0 остаётся словом).
4. **`::first-line`.** `compute_pseudo_element_style` применял к нему любую декларацию; `margin`, `position`, `word-break`, `transform`… попадали в computed. Теперь действует список CSS Pseudo-Elements L4 §3.2 (`first_line_property_applies`), как `marker_property_applies` для `::marker`.

Тесты: `v8_bug1437_text_motion.rs` (6), `css_wide_keywords_reset_text_longhands`, `first_line_computed_style_keeps_only_applicable_properties`, `css_wide_value::tests`.

## Остаток

- `hyphenate-character`, `hyphenate-limit-chars` не разбираются вовсе («'from' value should be supported»): `hyphen-no-interpolation` 42/126, `hyphen-limit-chars-interpolation` 0/96 — нереализованные свойства, не дефект (feature gap).
- `getComputedStyle(el, '::first-line')` отдаёт `""` для `display: inline`/`flex` у `el` (сегмента первой строки нет — вычисленного стиля псевдоэлемента тоже) и для `font-feature-settings`, `font-kerning`, `font-size-adjust`, `font-variation-settings`, `text-decoration`, `text-underline-position` (нет в `computed_style_to_map`, это BUG-1407): `css-pseudo/first-line-allowed-properties` 38/112.

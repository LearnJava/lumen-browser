# BUG-1401 — CSSOM: `contain`, `container-type`, `content-visibility` — каноническая запись, недопустимые значения, `!important`

**Статус:** FIXED 2026-10-22 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** js/layout (`crates/js/src/shim/web_api_shim_mid.js` — валидация `element.style`; `crates/engine/layout/src/selector_query.rs:2060-2090` — `contain` в computed)

## Симптом

`--dump-layout`, скрипт, `d = <div>`:

| присваивание / чтение | получено | ожидается |
|---|---|---|
| `d.style.contain='strict'; getComputedStyle(d).contain` | `size layout style paint` | `strict` |
| `d.style.contain='content'; …` | `layout style paint` | `content` |
| `d.style.contain='layout size'; d.style.contain` | `layout size` | `size layout` |
| `d.style.contain='strict layout'; d.style.contain` | `strict layout` | `""` (отброшено) |
| `d.style.contentVisibility='bogus'` / `'auto hidden'` | сохраняются | `""` |
| `d.style.cssText='container-type:size !important'; d.style.containerType` | `size !important` | `size` |
| `'container-type' in getComputedStyle(d)` | `false` | `true` |

## Как найдено

WPT-RUN-14 срез 17: `css-contain/parsing/{contain-computed,contain-valid,contain-invalid}.html` (23 из 43 сабтестов), `container-type-important.html`, `content-visibility/parsing/content-visibility-invalid.html`, `content-visibility-interpolation.html` (32 из 130; анимируется дискретно, `hidden` на 0 < t < 1 выдаётся неверно).

## Что делать

Общая таблица канонических сокращений (`strict`, `content`) и порядка флагов для `contain` в `selector_query.rs` и в JS-валидаторе; отбрасывать `strict`/`content` вместе с другими значениями; `!important` не должен попадать в значение longhand у `style.<prop>`.

## Как проверить

`css/css-contain/parsing/contain-computed.html`, `contain-valid.html`, `contain-invalid.html`.

## Как исправлено

- `contain` в `getComputedStyle`: полные наборы `size layout style paint` / `layout style paint` сериализуются как `strict` / `content` (`contain_to_css`, `selector_query.rs`); разбор значения в каскаде отбрасывает недопустимые комбинации (`parse_contain`, `style/apply/layout.rs`).
- `element.style.contain` / `containerType` / `contentVisibility`: канонический порядок флагов, отказ от `strict`/`content`/`none` с соседями, повторов и `size`+`inline-size`, от значений вне грамматики (`_lumen_css_canonical_contain`, `_lumen_css_canonical_container_type`, запись в `_LUMEN_KEYWORD_PROPERTIES`).
- `!important` срезается с конца значения в `_lumen_parse_style` — в `style.<prop>` он не попадает (приоритет по-прежнему не отслеживается).
- Тесты: `dom/tests/v8_bug1401_contain_cssom.rs`, `computed_map_contain_*` в `selector_query.rs`. WPT: `css-contain/parsing` 43/43, `content-visibility/parsing` 11/11.

## Остаток

`content-visibility-interpolation.html` (дискретная анимация `content-visibility` через Web Animations) не относится к CSSOM-разбору — это BUG-536 (нет наблюдаемого объекта Animation), baseline `.ini` остаётся.

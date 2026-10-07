# BUG-1401 — CSSOM: `contain`, `container-type`, `content-visibility` — каноническая запись, недопустимые значения, `!important`

**Статус:** OPEN
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

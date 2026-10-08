# BUG-1475 — `CSS.supports(prop, value)` проверяет только имя свойства: любое значение известного свойства — «поддерживается»

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** js/css-parser (`crates/js/src/v8_runtime/install/platform.rs::_lumen_css_supports_prop`, `_lumen_css_supports_cond`)

## Симптом

`CSS.supports('display','bogus')`, `('color','bogus')`, `('width','-5xyz')`, `('position','bogus')`, `('overflow','bogus')` и однопараметрная форма `CSS.supports('(display: bogus)')` возвращают `true`; только неизвестное имя даёт `false`. Это ломает тесты, которые перебирают значения (`css-display/parsing/display-invalid.html` — 55 из 55 сабтестов «should not set the property value», `css-position/parsing/*-invalid.html`), и проверки `assert_implements_optional` в каждом файле, начинающемся с «поддерживается ли свойство» (`anchor-name doesn't seem to be supported in the computed style` — 633 сабтеста `position-area-computed.html`). Смежно с [BUG-501](BUG-501-FIXED.md) (форма с одним аргументом и пользовательские свойства — исправлено 2026-09-03; значение осталось неразобранным), [BUG-495](BUG-495-OPEN.md) («только имя свойства по списку»), [BUG-1377](BUG-1377-OPEN.md) (невалидное значение не отбрасывается в каскаде).

## Проба

Проба (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `CSS.supports('display','bogus')` | `true` | `false` |
| `CSS.supports('color','bogus')` | `true` | `false` |
| `CSS.supports('width','-5xyz')` | `true` | `false` |
| `CSS.supports('(display: bogus)')` | `true` | `false` |
| `style.display = 'bogus'; style.display` | `bogus` | `` |
| `CSS.supports('nonexistent-prop','1')` | `false` | `false` |

## Как найдено

WPT-RUN-14 срез 21: `css-display/parsing/display-invalid.html` (55 сабтестов), `css-position/parsing/inset-invalid.html`, пробы `CSS.supports`.

## Что делать

Вызывать реальный разбор значения свойства (`apply_declaration` в режиме «только проверить») вместо проверки имени по списку; тот же разбор отклоняет значение в `element.style`.

## Как проверить

Таблица выше; `css/css-display/parsing/display-invalid.html`.

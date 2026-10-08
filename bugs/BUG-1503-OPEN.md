# BUG-1503 — CSS-wide слова у пользовательских свойств (`initial`/`inherit`/`unset`) читаются как литерал; запасное значение `var(--a, x)` не используется при цикле и после `initial`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/style/substitute.rs`, `crates/engine/layout/src/style/cascade.rs` — CSS-wide ключевые слова у пользовательских свойств и циклы)

## Симптом

`--a:initial`, `--a:inherit`, `--a:unset` на пользовательском свойстве раскрываются не по CSS Cascade 4 §7.3: значение остаётся словом (`getPropertyValue('--x')` вернёт `initial`), а `var(--a)` после `--a:inherit` при родителе `--a:green` — не `green`. Запасное значение `var(--a, green)` не срабатывает, когда `--a` — guaranteed-invalid (после `initial`) или участвует в цикле (`--a:var(--b);--b:var(--a);color:var(--a,green)` — ожидается `green`; у нас `black`). `revert`/`revert-layer`/`revert-rule` в запасном значении (`var(--x, revert)`) читаются буквально.

## Проба

Проба (`--mcp`, цвет `p`, `body{color:green}`-контекст не используется, ожидается `green` из запасного значения):

| декларации | у нас | ожидается |
|---|---|---|
| `--a:initial;color:var(--a,green)` | `black` | `green` |
| `--a:inherit;color:var(--a,green)` (корень) | `black` | `green` |
| `--a:unset;color:var(--a,green)` (корень) | `black` | `green` |
| `body{--a:green}p{--a:inherit;color:var(--a)}` | `black` | `green` |
| `body{--a:green}p{--a:unset;color:var(--a)}` | `black` | `green` |
| `body{--a:crimson}p{--a:initial;color:var(--a,green)}` | `black` | `green` |
| `--a:var(--b);--b:var(--a);color:var(--a,green)` | `black` | `green` |
| `--a:crimson;--a:var(--a);color:var(--a,green)` | `black` | `green` |
| `--x:initial` и `getComputedStyle(p).getPropertyValue('--x')` | `initial` | пусто |

## Как найдено

WPT-RUN-14 срез 22: `css-variables/variable-declaration-{30,43,44,45,46,47,51,52,56,57,58}.html`, `wide-keyword-fallback-002`, `variable-css-wide-keywords*`, `revert-*-in-fallback`, `variable-cycles`, `variable-definition-keywords` — 20 id.

## Что делать

Раскрывать CSS-wide ключевые слова у `--*` в каскаде (до подстановки); при guaranteed-invalid и при цикле выбирать запасное значение.

## Как проверить

Таблица выше; `css/css-variables/variable-declaration-43.html`, `variable-css-wide-keywords.html`.

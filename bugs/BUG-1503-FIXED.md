# BUG-1503 — CSS-wide слова у пользовательских свойств (`initial`/`inherit`/`unset`) читаются как литерал; запасное значение `var(--a, x)` не используется при цикле и после `initial`

**Статус:** FIXED 2026-10-23 (P3)
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

## Исправление

Два механизма. (1) Каскад ([`cascade.rs`](../crates/engine/layout/src/style/cascade.rs), пасс custom properties): `initial` удаляет запись (guaranteed-invalid; у зарегистрированного свойства `apply_property_initial_values` возвращает `initial-value`), `inherit` берёт значение родителя, `unset` — как `inherit` для наследуемого и как `initial` для `inherits: false`; слово больше не хранится литералом, `getPropertyValue('--x')` после `--x:initial` пуст. (2) Подстановка ([`substitute.rs`](../crates/engine/layout/src/style/substitute.rs), `expand_vars_tracked`): свойство, на которое ссылается `var()`, считается guaranteed-invalid и при отсутствии, и когда его собственное значение не подставляется (ссылка на несуществующее), и при цикле — тогда применяется запасное значение. Цикл ищется стеком раскрываемых имён, а не глубиной: запасное значение внутри цикла (`--a:var(--b,x);--b:var(--a,x)`) цикл не спасает, поглощается он только там, где замкнулся (у `var(--a, green)`), а зависящее от цикла свойство тоже недействительно. Тесты — `tests/var_css_wide.rs`.

Остаток: `revert`/`revert-layer`/`revert-rule` в запасном значении у **пользовательского** свойства (`--x: var(--unknown, revert)` → `getPropertyValue` пуст) по-прежнему читаются буквально; для типизированных свойств (`margin: var(--unknown, revert)`) откат к UA уже работает.

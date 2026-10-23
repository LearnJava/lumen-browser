# BUG-1507 — `@function`: типизированные параметры (`--a <length>`) и `@media` в теле функции не работают — вызов недопустим

**Статус:** FIXED 2026-10-23 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** layout (`crates/engine/layout/src/style/substitute.rs` — `expand_custom_functions`, `FunctionRule`)

## Симптом

Вызов `--f()` функции с типизированным параметром (`@function --f(--a <length>:3px) returns <length>{result:calc(var(--a)*2)}`) — недопустимое значение (`width` падает в `auto` = `1008px`), а то же без типа (`--f(--a:3px)`) — `6px`. Функция с `returns <length>` без типизированных параметров работает (`7px`). Условные группы в теле (`@media all{result:5px}`) не применяются (`1px` вместо `5px`); `if()` в `result` не вычисляется (`CSS-SPECS.md` — `if()` ⬜). Прямая рекурсия `--f(){result:--f()}` недопустима верно.

## Проба

Проба (`--mcp`, `#t{width:--f();padding:0}`, ожидается `getComputedStyle(t).width`):

| определение | у нас | ожидается |
|---|---|---|
| `@function --f(--a:3px){result:calc(var(--a)*2)}` | `6px` | `6px` |
| `@function --f(--a <length>:3px) returns <length>{result:calc(var(--a)*2)}` | `1008px` | `6px` |
| `@function --f(--a <length>){result:var(--a)}`, `--f(5px)` | `1008px` | `5px` |
| `@function --f() returns <length>{result:7px}` | `7px` | `7px` |
| `@function --f(){result:1px;@media all{result:5px}}` | `1px` | `5px` |
| `@function --f(--a){result:if(style(--a:1): 7px; else: 9px)}`, `--f(1)` | `1008px` | `7px` |

## Как найдено

WPT-RUN-14 срез 22: `css-mixins/functions/*` — 21 id, 267 из 390 сабтестов (`at-function-cssom` 34/34, `at-function-parsing` 48/86, `dashed-function-eval` 57/89, `dashed-function-parsing` 28/49, `function-attr` 6/19, `function-conditionals`, `function-container-*`, `function-layer`).

## Что делать

Типы параметров и `returns` проверять и приводить значение; условные группы (`@media`/`@supports`/`@container`) в теле `@function`; `if()`/`attr()`/`inherit()` в `result` (смежно с `CSS-SPECS.md` — `if()`); CSSOM `CSSFunctionRule` — CSSOM-10.

## Как проверить

Таблица выше; `css/css-mixins/functions/dashed-function-eval.html`, `function-conditionals.html`.

## Исправление

`crates/engine/css-parser/src/parser/at_rules.rs`: `FunctionParameter.syntax` (тип `<length>` / `type(…)` отделён от имени — раньше имя было `--a <length>`, поэтому `var(--a)` не находил параметр), `FunctionRule::returns_syntax`, тело — `Vec<FunctionBodyItem>` с вложенными `@media`/`@supports`. `crates/engine/layout/src/style/substitute.rs`: `coerce_to_syntax` (проверка по сопоставителю `@property` + вычисленный вид), аргумент с ошибкой/несовпадением типа → значение по умолчанию, иначе параметр не задан; значения по умолчанию считаются в области предыдущих параметров; локальная `--x:` типизированного параметра сохраняет тип; активные условные группы разворачиваются в порядке источника. Тесты `css_function_*` (layout) и `function_typed_parameters_and_returns` (css-parser).

Остаток (функциональные пробелы, не дефекты): `@container` в теле функции, `if()`/`attr()`/`inherit()` в `result`, CSSOM `CSSFunctionRule` (CSSOM-10).

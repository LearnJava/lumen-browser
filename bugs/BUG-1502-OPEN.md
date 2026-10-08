# BUG-1502 — Имя пользовательского свойства с CSS-экранированием (`--\30`, `--\fffd`, `--a\27 d`) не совпадает с тем же именем без экранирования

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/declarations.rs` — имя пользовательского свойства с экранированием)

## Симптом

Имена `--0` и `--\30` — одно и то же свойство (CSS Syntax 3 §4.3.7: экранирование раскрывается при разборе идентификатора), как и `--\d800`/`--\fffd` (суррогат заменяется на U+FFFD). У нас имя хранится как написано: `--0:green; color:var(--\30)` и `--\30:green; color:var(--\30)` не находят свойство. Неэкранированные не-ASCII имена (`--длинное-имя`) работают.

## Проба

Проба (`--mcp`, `p{…}`, цвет `p`):

| декларации | у нас | ожидается |
|---|---|---|
| `--0:green;color:var(--\30)` | `black` | `green` |
| `--\30:green;color:var(--\30)` | `black` | `green` |
| `--\d800:green;color:var(--\fffd)` | `black` | `green` |
| `--\d800:green;color:var(--\d800)` | `black` | `green` |
| `--длинное-имя:green;color:var(--длинное-имя)` | `green` | `green` |

## Как найдено

WPT-RUN-14 срез 22: `css-variables/variable-declaration-{31,32,34,35,36,42}.html`, `variable-reference-{21,22,23,24}.html` — 10 reftest.

## Что делать

Раскрывать CSS-экранирование в имени пользовательского свойства при разборе (и при `getPropertyValue`/`setProperty`), одной функцией со всеми остальными идентификаторами.

## Как проверить

Таблица выше; `css/css-variables/variable-declaration-32.html`.

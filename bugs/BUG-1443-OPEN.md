# BUG-1443 — Сериализация `:nth-child()` неканонична (`1n` вместо `n`, `-1n+3` вместо `-n+3`), а число за пределами `i32` обнуляет аргумент: `:nth-child(99999999999)` → `:nth-child`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/selectors.rs` — `nth_to_css_str` `:649`, разбор An+B)

## Симптом

`CSSStyleRule.selectorText` для `:nth-child(n)` отдаёт `:nth-child(1n)`, для `-n+3` — `-1n+3`, для `+n` — `1n`, для `n+2` — `1n+2` (канон CSSOM: `n`, `-n+3`, `n`, `n+2`). Числа, не помещающиеся в `i32`, не клампятся к `2147483647`: `:nth-child(99999999999)` и `:nth-child(-n+99999999999)` сериализуются как `:nth-child` без аргумента. `parse-anplusb.html` — 108 из 112 сабтестов падают (все на `1n` против `n`), `nth-child-large-anplusb-clamp.html` — 3 из 3. Для `a = 1` `nth_to_css_str` печатает `{a}n`.

## Проба

`insertRule(q + "{}")` → `cssRules[0].selectorText`:

| q | у нас | ожидается |
|---|---|---|
| `:nth-child(n)` | `:nth-child(1n)` | `:nth-child(n)` |
| `:nth-child(+n)` | `:nth-child(1n)` | `:nth-child(n)` |
| `:nth-child(-n+3)` | `:nth-child(-1n+3)` | `:nth-child(-n+3)` |
| `:nth-child(n+2)` | `:nth-child(1n+2)` | `:nth-child(n+2)` |
| `:nth-child(2n+1)`, `odd` | `:nth-child(2n+1)` | верно |
| `:nth-child(2n)`, `even` | `:nth-child(2n)` | верно |
| `:nth-child(2147483647)` | `:nth-child(2147483647)` | верно |
| `:nth-child(99999999999)` | **`:nth-child`** | `:nth-child(2147483647)` |
| `:nth-child(-n+99999999999)` | **`:nth-child`** | `:nth-child(-n+2147483647)` |

## Как найдено

WPT-RUN-14 срез 20: `parse-anplusb.html` (108/112), `nth-child-large-anplusb-clamp.html` (3/3).

## Что делать

`nth_to_css_str`: `a == 1` → `n`, `a == -1` → `-n`; при разборе клампить к `i32::MAX`/`i32::MIN` (насыщающее преобразование).

## Как проверить

Таблица выше; `css/selectors/parsing/parse-anplusb.html`, `css/selectors/nth-child-large-anplusb-clamp.html`.

# BUG-1501 — `var(--a)var(--b)` без пробела между ссылками склеивается в два токена и принимается; по спецификации склейка двух подстановок без разделителя даёт недопустимое значение

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser/layout (`crates/engine/layout/src/style/substitute.rs` — склейка подставленных `var()`)

## Симптом

`color:var(--a)var(--b)` при `--a:orange;--b:red` у нас — `orangered` (`rgb(255,69,0)`), то есть значения склеены в `orangered`; по CSS Variables 1 §3 подстановка вставляет токены, а два идентификатора подряд — не один токен: декларация недопустима во время вычисления и значение — наследуемое/начальное. Тот же дефект для `--a:var(--b)var(--c)` внутри пользовательского свойства. Контроль: `var(--c)var(--b)` с `--c:orange` ведёт себя верно.

## Проба

Проба (`--mcp`, `p{color:crimson;--a:orange;--b:red;color:var(--a)var(--b)}`):

| декларации | у нас | ожидается |
|---|---|---|
| `color:var(--a)var(--b)` (`orange`, `red`) | `rgb(255, 69, 0)` | наследуемое |
| `--a:var(--b)var(--c);--b:orange;--c:red;color:var(--a)` | `rgb(255, 69, 0)` | наследуемое |
| `--b:green;--a:var(--c)var(--b);--c:orange;color:var(--a)` | `black` | `black` |

## Как найдено

WPT-RUN-14 срез 22: `css-variables/variable-declaration-{14,53,54,55}.html`, `variable-reference-15.html` — 5 reftest.

## Что делать

Подставлять `var()` токенами, а не строками: между двумя подстановками без пробела вставлять `/**/`-эквивалент (разделитель токенов) или проверять склейку.

## Как проверить

Таблица выше; `css/css-variables/variable-declaration-53.html`.

# BUG-1501 — `var(--a)var(--b)` без пробела между ссылками склеивается в два токена и принимается; по спецификации склейка двух подстановок без разделителя даёт недопустимое значение

**Статус:** FIXED 2026-10-23 (P3)
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

## Исправление

- `layout/style/substitute.rs` (`expand_vars` + `token_boundary_sep`): на стыке префикса/подстановки/хвоста, где прямая склейка слила бы два токена (имя/число/размерность: `orange`+`red`, `10`+`px`, `1`+`em`), вставляется пробел; границы с разделителем (`(`, `,`, `*`, пробел) не трогаются. Работает и для `var()` внутри custom property (подстановка идёт тем же `expand_vars`). Побочно: `var(--n)px` при `--n:10` теперь тоже недопустимо (как в Chrome) — раньше принималось как `10px`.
- Тесты: `style::tests::restyle::expand_vars_adjacent_substitutions_stay_separate_tokens`, `adjacent_var_substitution_is_invalid_at_computed_value_time`.
- Reftest: `variable-declaration-14/53/54/55`, `variable-reference-15` — `thick` (898 px) → `identical`.

# BUG-1500 — Декларации `var()` с недопустимым телом (`!`, `;`, `!important` в запасном значении, лишняя `)`, `{ [ var() ] }`, не закрытый `var(` в конце листа) не отбрасываются или отбрасываются лишние

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/declarations.rs` — декларации со значением из `var()`; разбор токенов custom property)

## Симптом

CSS Variables 1 §2.1/§3: декларация со значением, не разбирающимся как `<declaration-value>`, отбрасывается целиком, а прежняя декларация остаётся в силе; незакрытая функция/строка/комментарий в конце листа закрывается автоматически. У нас: `--a:var(--b,!)`, `--a:var(--b,;)`, `--a:var(--b,!important)` принимаются и подменяют `--a` (получаем `--b`, ожидается прежнее `--a`); `--a:red)` (лишняя `)`) — наоборот, пропадает целиком вместе с предыдущим `--a`; `color:{ [ var(--a) ] }` (блок `{}` с `var()` внутри) принимается, а должно быть недопустимо; `color:var(--a` в самом конце листа (а также `var(--a /* …`, `var(--a, "`) не закрывается и объявление пропадает.

## Проба

Проба (`--mcp`, `body{color:green}` кроме отмеченных; `p{…}`, цвет `p`):

| декларации | у нас | ожидается |
|---|---|---|
| `--a:green;--b:crimson;--a:var(--b,!);color:var(--a)` | `crimson` | `green` |
| то же с `var(--b,;)` и `var(--b,!important)` | `crimson` | `green` |
| `--a:green;--a:red);color:var(--a)` | `black` (всё пропало) | `green` |
| `color:red;color:{ [ var(--a) ] }` | `red` | наследуемое (блок недопустим) |
| `color:red;--a:green;color:var(--a` (конец листа), `…var(--a /* x`, `…var(--a, "` | `red` | `green` (автозакрытие) |
| `--a:var(--b) !important !important` | прежнее значение | прежнее значение |
| `@supports (color: var(--a)) and (not (color: var(--a,!)))` и ещё 12 вариантов | ложно | истинно |

## Как найдено

WPT-RUN-14 срез 22: 29 reftest `css-variables` (`variable-declaration-11/12/13/23/59`, `variable-reference-07/08/18/20/25…29/33/35`, `variable-supports-09/10/17/21/23/30/41/42/49/53/55/58/64`), все `thick`; `variable-supports-*` ещё зависят от BUG-1475.

## Что делать

Привести разбор значения декларации и `var()` к `<declaration-value>`: недопустимые токены (`!`, `;` на верхнем уровне, `{}`-блок, несбалансированная `)`) — отбрасывают декларацию; EOF автоматически закрывает открытые скобки, строки и комментарии.

## Как проверить

Таблица выше; `css/css-variables/variable-declaration-11.html`, `variable-reference-25.html`.

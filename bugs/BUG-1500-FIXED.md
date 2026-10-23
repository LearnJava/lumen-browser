# BUG-1500 — Декларации `var()` с недопустимым телом (`!`, `;`, `!important` в запасном значении, лишняя `)`, `{ [ var() ] }`, не закрытый `var(` в конце листа) не отбрасываются или отбрасываются лишние

**Статус:** FIXED 2026-10-23 (P3)
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

## Исправление

Часть пунктов из пробы к моменту работы уже не воспроизводилась (EOF-закрытие `var(--a` и `/*`, `!important !important` в обычном свойстве); пробу пришлось переснять с `body { color: blue }`, иначе зелёный эталон маскировал выброшенное правило.

- `css-parser/declarations.rs` (`parse_value_checked`): имя функции `VAR(` приводится к `var(` (имя ASCII-регистронезависимо, дальше код ищет строчное); непарная `)`/`]` в значении custom property делает объявление недопустимым (`--a: red)`) — для обычных свойств остаётся как раньше (matching-brackets-002).
- `css-parser/nesting_amp.rs` (`ident_led_block_ahead`): `ident : {…}` — объявление, а не вложенное правило (`color: { [ var(--a) ] }` целиком значение, CSS Syntax §5.4.4); `a:hover {…}` по-прежнему правило.
- `layout/style/substitute.rs` (`fallback_declaration_value_ok`): `!` и `;` на верхнем уровне fallback-а `var()`/`env()` — ошибка разбора (`var(--b,!)`, `var(--b,;)`, `var(--b,!important)`); `<!--` — один CDO-токен, а не `!`.
- `layout/style/cascade.rs`: custom property со значением из `var()`/`env()` проходит ту же parse-time проверку, что и обычные свойства (раньше не проходила вовсе — `--a: var(--b) <!--;` и `--a: var(--b,!)` подменяли прежнее `--a`).
- `css-parser/supports.rs` (`parse_declaration`): объявление в `@supports` с недопустимым `var()` (`!`/`;` в fallback-е, голова не `--ident`) — `Unknown` (ложь); имя `--` зарезервировано; `\`-escape в именах (`--\d800`) допустимы; `<!--` не считается `!`.
- `shell/doc_extract.rs`: строка, не закрытая в конце `<style>`, закрывается кавычкой до разделителя `
` — иначе разделитель превращал её в bad-string и объявление `color: var(--a, "` пропадало.
- Тесты: `layout/tests/var_invalid_declaration_value.rs` (8), `supports_declaration_with_invalid_var_is_unsupported`, `property_with_braced_value_stays_a_declaration`, `extract_style_blocks_closes_a_string_open_at_eof`.
- Reftest `css-variables` (165 `variable-declaration|reference|supports-*`): все 29 из заявки `identical`; остаток 18 (`variable-declaration-14/30/42…58`, `variable-reference-15/39`) — другие кластеры, на том же бинаре без правки красные.

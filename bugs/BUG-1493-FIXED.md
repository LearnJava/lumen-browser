# BUG-1493 — Грамматика условия `@supports`: `not(`/`or(` как функции, смесь `and`/`or` без скобок, `;`, пустое значение и приоритеты не отвергаются как недопустимые

**Статус:** FIXED 2026-10-23 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** css-parser (`crates/engine/css-parser/src/parser/supports.rs` — разбор условия `@supports`)

## Симптом

Условие, которое по CSS Conditional 3 §2 недопустимо или ложно, принимается, и блок применяется. Значение декларации не проверяется вовсе (общая причина — BUG-1475). Но есть и отдельные грамматические пропуски: функциональный токен `not(` и `or(` без пробела разбирается как `not (`/`or (`; `and`/`or` в одном уровне скобок разрешены вместе; `;` и пустое значение внутри скобок не делают условие недопустимым; `!important` разрешён только один раз, с хвостовыми токенами и с `!bogus` условие должно быть ложным; Отдельно — восстановление после ошибки: прелюдия `@supports` с лишней `)` (`(margin: 2px) )`, `(width: 0))`, `(margin: 0)) ;`) съедает и следующее за ней правильное правило. `:compute(…)`-подобные функции с пробелом внутри (`width:compute( 2px + 2px )`) — условие должно считаться ложным и `not (…)` истинным, у нас наоборот.

## Проба

Проба (`--mcp`, `#t{color:red}` + `@supports ⟨условие⟩{#t{color:green}}`, «применяется» = `green`):

| условие | у нас | ожидается |
|---|---|---|
| `(margin: )` | применяется | нет |
| `(margin:0;)` | применяется | нет |
| `(margin:0 or padding:0)`, `(margin:0 and padding:0)` | применяется | нет |
| `not(foo: baz)` | применяется | нет |
| `(color:green) or(color:red)` | применяется | нет |
| `((margin:0) and (display:inline) or (width:1em))` | применяется | нет |
| `( ( background-color: red ) or( background-color: green ) )` | применяется | нет |
| `(color:red !bogus)`, `(color:red !important !important)`, `(color:red !important x)` | применяется | нет |
| `(not (width:compute( 2px + 2px )))` | **нет** | применяется |
| `not (color:red !important !important)` | **нет** | применяется |
| `@supports (margin: 2px) ) {…}`, `(width: 0)) {}`, `(margin: 0)) ;` — и сразу за ними `@supports (margin:0){div{color:green}}` | следующий правильный `@supports` **не применяется** (разбор не восстанавливается после лишней `)`) | применяется |
| `(margin: 0]) {…}`, `(margin: ) {…}` и следующий правильный `@supports` | применяется | применяется |
| `(margin:0)`, `(color:red) or (foo:bar)`, `(( background-color: red ) or ( background-color: green ))` | применяется | применяется |

Значение декларации (`(color:bogus)`, `(width:-5xyz)`, `(display:bogus)`, `(width: calc(1px+2px))`) тоже принимается — это BUG-1475.

## Как найдено

WPT-RUN-14 срез 22: 32 reftest `css-conditional` (`at-supports-014/016/018/019/020/026/028/031/034…039/043`, `css-supports-005…045.xht`), все `thick`. Часть id зависит ещё и от BUG-1475 и от BUG-1460.

## Что делать

Разбор условия по грамматике Conditional 3: `<supports-in-parens>`, строгое `not`/`and`/`or` как идентификаторы, отвергнуть `not(`/`or(` как функции, один уровень — один тип связки, объявление через разбор значения свойства (BUG-1475), приоритет `!important` не более одного раза и без хвоста.

## Как проверить

Таблица выше; `css/css-conditional/at-supports-016.html`, `at-supports-019.html`, `css-supports-013.xht`.

## Исправление

- `crates/engine/css-parser/src/parser/supports.rs` — условие разбирается по грамматике Conditional 3 §2 (`parse_condition` / `parse_in_parens` / `parse_paren_content` / `parse_declaration`) вместо «первая связка на уровне»: `not`/`and`/`or` — идентификаторы с обязательным пробелом после (`not(`/`or(` — функции, `<general-enclosed>` → ложь), `and` и `or` на одном уровне не смешиваются, хвостовые токены и голый идентификатор делают всё условие `Unknown`. Скобки, не разобравшиеся ни как условие, ни как объявление, — `<general-enclosed>` (ложь, но `not (…)` над ними истинно).
- Структура объявления: у обычного свойства значение не пусто и без `:`/`;` на верхнем уровне; `!important` — не более одного раза, в конце и ровно этим словом (`!bogus`, `!important x`, `!important !important` → ложь); `--*` допускает пустое значение и `:`. Имя свойства — валидный ident.
- `parse_supports_rule` (`supports.rs`) и вложенный `@supports` (`nested_rules.rs`): лишняя `)` в прелюдии не уводит глубину в минус — `{`/`;` за ней снова завершают прелюдию, следующее правило не съедается.
- Тесты: `at_supports_grammar_rejects_invalid_conditions`, `at_supports_grammar_accepts_valid_conditions`, `at_supports_stray_close_paren_recovers`.
- Остаток: строка таблицы `(not (width:compute( 2px + 2px )))` (значение с неизвестной функцией считается валидным) и значения вроде `(color:bogus)` — это проверка значения свойства, [BUG-1475](BUG-1475-OPEN.md); WPT-прогон `css-conditional` после него даст полную картину.

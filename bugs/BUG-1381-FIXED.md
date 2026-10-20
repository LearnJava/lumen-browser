# BUG-1381 — восстановление после синтаксической ошибки: at-rule, скобки, строки, EOF

**Статус:** FIXED 2026-10-20 (P6)
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** css-parser (`crates/engine/css-parser/src/parser.rs` — восстановление после ошибки: `@ import`, незакрытые `(`/`[`, строки, EOF)

## Симптом

CSS 2.1 §4.2 (и CSS Syntax): при невалидном at-rule пропускается всё до `;` или блока `{…}` с учётом парных скобок/кавычек; незакрытые скобки/строки закрываются в конце файла.

| CSS | результат | ожидается |
|---|---|---|
| `@ import "x.css"; div{color:red} * {color:green}` (`@` + пробел — не at-keyword, а `<delim-token>`) | `div` красный | зелёный: `@ import "x.css"; div` — один невалидный селектор до `{…}`, правило `div{color:red}` отбрасывается целиком; Lumen обрывает на `;` и применяет `div{color:red}` |
| `p{color:red;color:green` (EOF, без `}`) | зелёный | верно |
| `p{color:rgb(0,128,0` (одно объявление, EOF) | чёрный | зелёный (EOF закрывает скобку, `rgb(0,128,0)` валидно) |
| `@foo; div{color:red}` | красный | верно |

Без пробы остались: `matching-brackets-001…003`, `square-brackets-001`, `strings-000`, `unterminated-string-001`, `blocks-and-strings-001`, `core-syntax-006`, `declarations-009`, `at-rule-007/011/012/013` — содержимое тестов читалось, причина по одному не отделена.

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/syntax/{at-rule,eof,matching-brackets,square-brackets,strings,unterminated-string,blocks-and-strings,declarations,core-syntax}-*` — 17 id.

## Что делать

Привести `parse_rule`/`parse_declaration_block` к алгоритму «consume a component value» с отслеживанием парных `(` `[` `{` и строк; на EOF закрывать открытые конструкции и принимать валидное значение.

## Как проверить

`css/CSS2/syntax/eof-002.xht`, `eof-005.xht`, `at-rule-001.xht`.

## Исправление (2026-10-20, P6)

Один сканер компонент-значений `Parser::scan_component_values` (`parser.rs`) вместо четырёх посимвольных циклов (`recover_to_block_end`, `recover_to_decl_boundary`, `skip_at_rule`/`skip_until_block_end`, `skip_block`): строки, комментарии и escape атомарны, `(`/`[`/`{` открывают блоки со стеком закрывающих, несовпадающий закрыватель — обычный токен.

- `@` без идентификатора после него — delim, правило квалифицированное (`@ import "x.css"; div{…}` — один отброшенный prelude); `;` в prelude невалидного top-level-правила не обрывает его (`recover_qualified_rule`, prelude пересканируется с начала).
- `parse_value_checked` (declarations.rs): стек закрывателей вместо счётчика (`{…}` в значении — блок), EOF закрывает открытые `(`/`[`/`{` и строку, строка, оборванная неэкранированным переводом строки (bad-string), делает декларацию невалидной.
- `@import "…` без закрывающей кавычки на EOF принимается; невалидный `@import` доходит до своего `;`.

Тесты: `parser/tests/recovery.rs` (BUG-1381, 11 штук, по материалам `css/CSS2/syntax/*`).

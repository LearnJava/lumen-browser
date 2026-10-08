# BUG-1378 — имя свойства в CSS регистрозависимо в `parse_declaration`

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** css-parser (`crates/engine/css-parser/src/parser/declarations.rs::parse_declaration` — имя свойства не приводится к нижнему регистру)

## Симптом

`parse_declaration` (`declarations.rs:89`) берёт `parse_ident()` как есть. `--dump-display-list`:

| CSS | цвет текста | ожидается |
|---|---|---|
| `p{color:green}` | зелёный | зелёный |
| `p{COLOR:green}` | чёрный | зелёный |
| `p{Color:green}` | чёрный | зелёный |
| `p{bACkGRounD:green}` | фон не нарисован | зелёный фон |
| `p{color:gREen}` | зелёный | верно (значение регистронезависимо) |
| `P{COLOR:green}` | чёрный | зелёный (селектор `P` верен, дело в свойстве) |
| `<p style="COLOR:green">` | зелёный | верно (inline-стиль приводится) |

Inline-стиль нормализуется, а таблица стилей — нет.

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/syntax/case-sensitive-000.xht`, `-001.xht`, `-003.xht` (+ `case-sensitive-006.html`; `cascade`-подкаталог — не пересекается). 5 id.

## Что делать

В `parse_declaration` вернуть `property.to_ascii_lowercase()` (кроме custom properties `--*`, которые регистрозависимы).

## Как проверить

`css/CSS2/syntax/case-sensitive-000.xht`, `-001.xht`.

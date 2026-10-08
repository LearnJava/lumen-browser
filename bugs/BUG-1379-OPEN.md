# BUG-1379 — `/* … */` внутри значения объявления делает значение невалидным

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** css-parser (`crates/engine/css-parser/src/parser/declarations.rs::parse_value_until_terminator` — комментарий `/* */` внутри значения не вырезается)

## Симптом

`parse_value_until_terminator` копирует символы как есть, не вырезая комментарии. `--dump-display-list`, `p{…}`:

| значение | цвет | ожидается |
|---|---|---|
| `/*C*/p{color:green}` | зелёный | верно |
| `p/*C*/{color:green}` | зелёный | верно |
| `p{color/*C*/:green}` | зелёный | верно |
| `p{color:/*C*/green}` | чёрный | зелёный |
| `p{color:green/*C*/;}` | чёрный | зелёный |
| `p{color:gr/*c*/een}` | чёрный | **чёрный верно**: `gr` + `een` — два токена (CSS Syntax §4: комментарий не склеивает токены) |
| `p{margin-top:/*c*/10px;background:green}` | фон есть, отступа нет | отступ 10px |
| `p{height:1/*c*/0px}` | высота 0 | **невалидно** — два токена `1` `0px` — верно (см. BUG-1377) |

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/syntax/comments-001.xht` (комментарии вокруг каждого токена); `signed-numbers-001.xht` — отдельная причина. 1 id.

## Что делать

Заменять комментарий внутри значения одним пробелом при сборке строки в `parse_value_until_terminator` (учитывая строки и `url(`).

## Как проверить

`css/CSS2/syntax/comments-001.xht`.

# BUG-1380 — escape-последовательности в значении свойства и в `url()` не раскрываются

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 15, `css/CSS2` (остальные каталоги: selectors, css1, syntax, box-display, visufx, visudet, …))
**Область:** css-parser/layout (`crates/engine/css-parser/src/parser.rs::parse_ident` и разбор значений — escape `\HH ` в значениях и `url()`)

## Симптом

Селектор и имя свойства с escape разбираются: `\64\69\76 {color:green}` → зелёный; `div{\63 \6F \6C \6F \72 :green}` → зелёный; `\64 iv{…}`, `\000064iv{…}` — зелёный. А в **значении** — нет:

| CSS | цвет | ожидается |
|---|---|---|
| `div{color:\67 \72 \65 \65 \6E ;}` | чёрный | зелёный |
| `p{color:\67reen}` | чёрный | зелёный |
| `p{color:g\72een}` | чёрный | зелёный |
| `p{background:\75rl(x.png) green}` | фон `green` применён (но `\75rl` не распознан как `url`) | `url(x.png)` — фон = картинка |
| `p{font-family:\41hem}` | `Ahem` | верно |

В `url()`: `background:url(support/\'green\ block.png)` — путь с экранированной кавычкой и пробелом не раскрывается.

## Как найдено

WPT-RUN-14 срез 15: `css/CSS2/syntax/escapes-001…014` (9), `escaped-ident-spaces-001…007` (7), `escaped-ident-001`, `escaped-url-001`, `ident-003`, `ident-018`, `uri-005/012/013/015/016/017/018` (7), `characters-0080-009F-001`: 29 id.

## Что делать

Раскрывать escape (`\` + 1–6 hex + необязательный пробел; `\` + любой не-hex) на уровне токенизации значения (`parse_value_until_terminator`) и ключевых слов, а также в `url()`. `font-family` уже раскрывается отдельно.

## Как проверить

`css/CSS2/syntax/escapes-002.xht`, `escaped-ident-spaces-001.xht`, `uri-005.xht`.

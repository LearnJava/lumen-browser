# BUG-1670 — Quirks-режим: таблица сбрасывает `font-family` к `serif` вместо наследования

**Статус:** OPEN
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), news.ycombinator.com)
**Область:** layout/UA-стили (quirks-правило для `table`)

## Симптом

```html
<body style="font-family:Verdana;font-size:10px"><table><tr><td id=t>x</td></tr></table>
```

(без `<!doctype>` — quirks). `getComputedStyle(td).fontFamily`: Chrome `Verdana`, Lumen `serif`
(у `table` — тоже `serif`). `font-size` сбрасывается к 16px в обоих — это верно. С `<!doctype html>`
Lumen отдаёт `Verdana`.

HN (quirks): `font-family: verdana → serif` у 66 узлов (`table`, `tbody`, `tr`, `td`), весь текст
ленты другим шрифтом и с другими метриками.

## Что делать

Quirks-правило HTML §15.3.7: `table { font-weight: initial; font-style: initial; font-variant: initial;
font-size: initial; line-height: initial; white-space: initial; text-align: initial; }` — `font-family`
в списке нет. Похоже, у нас правило сбрасывает шорткат `font` целиком (или `font-family: initial`).
Убрать `font-family` из сброса.

## Как проверить

Репро выше: `Verdana`. `python scripts/geom_diff.py --only hn` — нет строки `font-family: verdana → serif`.
WPT: `quirks/table-font-*` (если есть), `html/rendering/non-replaced-elements/tables/table-quirks*`.

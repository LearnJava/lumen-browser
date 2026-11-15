# BUG-1670 — Quirks-режим: таблица сбрасывала `font-family` к `serif` вместо наследования

**Статус:** FIXED 2026-11-15 (P3)
**Заведён:** 2026-10-21 (P2, TEST-11 — [geom-diff-2026-10-21](../docs/quality/geom-diff-2026-10-21.md), news.ycombinator.com)
**Область:** layout/UA-стили (quirks-правило для `table`)

## Симптом

```html
<body style="font-family:Verdana;font-size:10px"><table><tr><td id=t>x</td></tr></table>
```

(без `<!doctype>` — quirks). `getComputedStyle(td).fontFamily`: Chrome `Verdana`, Lumen `serif`
(у `table` — тоже `serif`). `font-size` сбрасывается к 16px в обоих — это верно. С `<!doctype html>`
Lumen отдавал `Verdana`.

HN (quirks): `font-family: verdana → serif` у 66 узлов (`table`, `tbody`, `tr`, `td`), весь текст
ленты другим шрифтом и с другими метриками.

## Причина и фикс

`style::quirks::apply_quirks_table_reset` сбрасывал `font_family` к дефолту документа. HTML §15.3.7
в quirks-правиле для `table` перечисляет `font-weight`, `font-style`, `font-variant`, `font-size`,
`line-height`, `white-space`, `text-align` — `font-family` в списке нет, он наследуется. Сброс
`font_family` убран, doc-комментарий правила приведён к спецификации.

## Как проверить

Репро выше: `Verdana`. `python scripts/geom_diff.py --only hn` — нет строки
`font-family: verdana → serif`. Регрессия: `quirks_table_font_family_inherits` в
`crates/engine/layout/src/tests/background_alignment_quirks.rs`.

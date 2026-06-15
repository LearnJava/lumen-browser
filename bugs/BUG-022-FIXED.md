# BUG-022

**Статус:** FIXED 2026-05-22
**Компонент:** css-parser

## Описание

Quirks-mode hashless hex colors not parsed

## Детали

TEST-20: `bgcolor="44aa66"` не распознаётся как `#44aa66` в quirks-mode.

**Компонент:** `lumen-css-parser`

# BUG-021

**Статус:** FIXED 2026-05-22
**Компонент:** html-parser

## Описание

HTML bgcolor attribute ignored

## Детали

TEST-20: `<body bgcolor="#1a2030">` даёт белый фон вместо тёмно-синего.

**Компонент:** `lumen-html-parser` (presentational hints)

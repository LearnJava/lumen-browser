# BUG-216

**Статус:** OPEN
**Компонент:** css-parser/layout
**Тест:** TEST-117 (diff 2.28%)

## Описание

CSS Generated Content `quotes` + `open-quote`/`close-quote` — auto curly, вложенные `<q>`, custom

## Воспроизведение

`python graphic_tests/run.py --only 117` → FAIL 2.28%

## Как чинить

Реализовать quotes property и open-quote/close-quote counter в generated content — поддержка вложенных уровней.

# BUG-196

**Статус:** OPEN
**Компонент:** css-parser/layout
**Тест:** TEST-67 (diff 16.41%)

## Описание

CSS Values L4 `attr()` typed substitution — `content:attr(data-label)` генерирует `::before` labels

## Воспроизведение

`python graphic_tests/run.py --only 67` → FAIL 16.41%

## Как чинить

Реализовать attr() typed substitution в css-parser; пробросить значение в generated content ::before/::after.

# BUG-206

**Статус:** FIXED 2026-06-24
**Компонент:** layout
**Тест:** TEST-87 (diff 1.98%)

## Описание

`inset-area: none none` — якорь не влияет на позицию при none keywords (стаб)

## Воспроизведение

`python graphic_tests/run.py --only 87` → FAIL 1.98%

## Как чинить

Реализовать парсинг inset-area keywords — при none/none позиция anchor не применяется к inset.

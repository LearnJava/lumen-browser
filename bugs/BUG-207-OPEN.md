# BUG-207

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-88 (diff 1.98%)

## Описание

`anchor-name` в вложенных элементах — иерархия DOM, поиск якорей (стаб)

## Воспроизведение

`python graphic_tests/run.py --only 88` → FAIL 1.98%

## Как чинить

Реализовать поиск anchor по имени в DOM-дереве — scoped lookup по containing block hierarchy.

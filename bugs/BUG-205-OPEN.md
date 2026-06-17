# BUG-205

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-86 (diff 2.12%)

## Описание

`position-anchor: --foo` — fallback позиция без inset-area отличается (стаб)

## Воспроизведение

`python graphic_tests/run.py --only 86` → FAIL 2.12%

## Как чинить

Реализовать position-anchor lookup — positioned element должен наследовать позицию anchor-элемента.

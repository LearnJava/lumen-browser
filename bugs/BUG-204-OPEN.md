# BUG-204

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-85 (diff 1.98%)

## Описание

`anchor-name: --foo` базовое объявление — визуализация элемента отличается от Edge (стаб)

## Воспроизведение

`python graphic_tests/run.py --only 85` → FAIL 1.98%

## Как чинить

Реализовать базовую регистрацию anchor-name в layout — хотя бы статический positioned fallback.

# BUG-185

**Статус:** OPEN
**Компонент:** layout/paint
**Тест:** TEST-32 (diff 3.75%)

## Описание

list markers: `::marker` box geometry, outside/inside, disc/decimal/alpha/roman

## Воспроизведение

`python graphic_tests/run.py --only 32` → FAIL 3.75%

## Как чинить

Расследовать геометрию маркерного бокса `::marker` — позицию outside/inside и типы маркеров.

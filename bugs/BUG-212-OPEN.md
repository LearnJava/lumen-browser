# BUG-212

**Статус:** OPEN
**Компонент:** font/layout
**Тест:** TEST-95 (diff 3.39%)

## Описание

CSS Fonts L5 `font-size-adjust` — масштабирование x-height; used-size = size × adjust/aspect

## Воспроизведение

`python graphic_tests/run.py --only 95` → FAIL 3.39%

## Как чинить

Реализовать font-size-adjust: получить aspect ratio шрифта (x-height/em) и скорректировать font-size.

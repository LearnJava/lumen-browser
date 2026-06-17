# BUG-210

**Статус:** OPEN
**Компонент:** css-parser/paint
**Тест:** TEST-92 (diff 15.59%)

## Описание

CSS Color 4 §6.2 system colors: Canvas/ButtonFace/Highlight/GrayText/AccentColor и др.

## Воспроизведение

`python graphic_tests/run.py --only 92` → FAIL 15.59%

## Как чинить

Реализовать разрешение CSS system color keywords в css-parser → реальные OS/UA цвета (или разумные дефолты).

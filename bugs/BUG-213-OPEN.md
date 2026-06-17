# BUG-213

**Статус:** OPEN
**Компонент:** css-parser/layout
**Тест:** TEST-97 (diff 2.78%)

## Описание

CSS Lists `counter-set` — порядок reset→increment→set; set перекрывает increment

## Воспроизведение

`python graphic_tests/run.py --only 97` → FAIL 2.78%

## Как чинить

Исправить порядок применения counter-reset/increment/set в cascade — set должен перекрывать increment (CSS Lists L3 §6.4).

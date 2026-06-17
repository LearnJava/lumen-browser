# BUG-199

**Статус:** OPEN
**Компонент:** layout
**Тест:** TEST-71 (diff 7.03%)

## Описание

`@starting-style`: static rendering двух цветных блоков отличается от Edge

## Воспроизведение

`python graphic_tests/run.py --only 71` → FAIL 7.03%

## Как чинить

Расследовать обработку @starting-style в каскаде — initial paint значения до transition.

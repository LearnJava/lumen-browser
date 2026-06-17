# BUG-181

**Статус:** OPEN
**Компонент:** layout/paint
**Тест:** TEST-19 (diff 9.05%)

## Описание

`object-fit` basic — fill/contain/cover/none/scale-down для `<img>`

## Воспроизведение

`python graphic_tests/run.py --only 19` → FAIL 9.05%

## Как чинить

Проверить реализацию всех режимов object-fit в femtovg_backend.rs / layout image sizing.

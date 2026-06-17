# BUG-209

**Статус:** OPEN
**Компонент:** image
**Тест:** TEST-90 (diff 2.75%)

## Описание

AVIF image decoder — `<picture>` с AVIF source + PNG fallback, прямой `<img src=".avif">`

## Воспроизведение

`python graphic_tests/run.py --only 90` → FAIL 2.75%

## Как чинить

Добавить AVIF декодер (libavif / dav1d через crate) или fallback на PNG при неподдерживаемом формате.

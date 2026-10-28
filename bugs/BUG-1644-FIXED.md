# BUG-1644 — `clientWidth`/`scrollWidth` у `<html>` больше окна при `innerWidth` = 1920

**Статус:** OPEN
**Заведён:** 2026-10-19 (живой прогон https://otpravka.pochta.ru/specification#/enums-base-mail-type)
**Область:** layout/js (геометрия корневого элемента для CSSOM View)

## Симптом

Живое окно `--maximized`, `devicePixelRatio = 1`, `innerWidth = 1920`, `innerHeight = 945`:

| Свойство | Lumen | Ожидается |
|---|---|---|
| `document.documentElement.clientWidth` | 2216 | ≈ 1920 (ширина viewport-а без полосы) |
| `document.documentElement.scrollWidth` | 2256 | 1920 (горизонтального переполнения нет — `div.page { overflow:hidden }`) |
| `document.body.scrollWidth` | 2256 | 1920 |
| `header.header-v2` (`width:1200px`) `scrollWidth` | 2880 | 1200 (абсолютный `::before` шириной `150vw` обрезан `overflow:hidden` предка) |

При этом `scrollTo(300,0)` оставляет `scrollX = 0`. Ни один элемент не выходит за `clientWidth`
по `getBoundingClientRect().right`.

## Что сделать

- `clientWidth` корня — по спеке CSSOM View равен ширине viewport-а за вычетом полосы; найти, откуда
  берётся 2216.
- `scrollWidth` не должен включать переполнение, обрезанное `overflow:hidden`/`clip` потомка
  (тот же корень, что [BUG-1642](BUG-1642-FIXED.md) п. 1, но в layout/JS-геометрии).
  Смежно: [BUG-1389](BUG-1389-FIXED.md), [BUG-1415](BUG-1415-FIXED.md).
- Репро: локальная страница с `div{overflow:hidden}` > `section{position:relative}` >
  `::before{position:absolute;left:-50vw;width:150vw}`; сначала проверить, воспроизводится ли
  2216 без сайта.

# BUG-1644 — `clientWidth`/`scrollWidth` у `<html>` больше окна при `innerWidth` = 1920

**Статус:** FIXED 2026-10-28 (P3)
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

## Причина и исправление

Живой замер (1920×945): `clientWidth`/`scrollWidth` корня и `body` уже давали 1920 (корень-скроллер
берёт размер viewport-а, переполнение под `overflow:hidden` считает `scroll_rollup`); 2216 был у
`documentElement.getBoundingClientRect().width` при `left = -296`. Источник — боковое меню
`.pochta-service-menu` (`left:-390px`) со ссылками `display:flex`: текст flex-контейнера оборачивается в
анонимный элемент, `InlineRun` которого ключуется **текстовым узлом** (`build_anon_text_item`).
`inline_element_ancestors` (`crates/engine/layout/src/clickable.rs`) шёл от узла текста вверх до
`stop_at`, которого среди DOM-предков нет, — до корня — и `collect_layout_rects_box` через `and_modify`
растягивал ссылку, `html` и `body` фрагментами за экраном. Теперь для `stop_at` = текстовый узел
список пуст. Тест `anonymous_flex_text_item_does_not_widen_ancestors`; репро без сайта —
`position:absolute;left:-390px` > `a{display:flex}` с текстом (раньше `html` = −358…1920).
Заодно исправлен `getClientRects()` у `html`/`body` (13 прямоугольников вместо одного).


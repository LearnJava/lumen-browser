# BUG-1297 — CSSOM `background-*` / `border-*` / `box-shadow` / `border-radius`: принимает невалидное, не канонизирует, `getComputedStyle()` отдаёт `""` или не то

**Статус:** OPEN
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 6, `css/css-backgrounds`)
**Область:** js + layout (`element.style` для background/border — `crates/js/src/shim/web_api_shim_mid.js::_lumen_canonicalize_longhand`; `crates/engine/layout/src/selector_query.rs::computed_style_to_map`)


## Симптом

Проба (`d.style[p] = v; d.style[p]`), всё должно быть `""` (значение отброшено):

| присваивание | получено |
|---|---|
| `backgroundSize = '-1px'` | `-1px` |
| `backgroundRepeat = 'repeat repeat-x'` | `repeat repeat-x` |
| `backgroundAttachment = 'auto'` | `auto` |
| `backgroundImage = 'red'` | `red` |
| `backgroundPosition = 'left right'` | `left right` |
| `backgroundClip = 'fill-box'` | `fill-box` |
| `boxShadow = 'auto'`, `borderRadius = 'auto'` | `auto` |
| `borderImageSlice = 'fill'`, `borderImageRepeat = 'space'` | принято (свойства парсером не разобраны) |

`getComputedStyle(div)` на элементе с `background-color:green`: `backgroundPosition` — `""` (должно `0% 0%`), `borderImageSource` — `""` (`none`), `borderRadius` — `""` (`0px`); у `background-image:none,none; background-repeat:space round` `backgroundRepeat` = `repeat, repeat` (теряет `space`/`round` и число слоёв). Сериализация: `no-repeat url(/favicon.ico)` вместо `url("/favicon.ico") no-repeat`, `fill 1 2% 3 4%` вместо `1 2% 3 4% fill`, `center left` вместо `left center`, `1px` вместо `1px auto`.

Механизм тот же, что [BUG-484](BUG-484-FIXED.md) (валидаторы по семействам свойств) и [BUG-472](BUG-472-OPEN.md)/[BUG-1278](BUG-1278-OPEN.md) (рукописная карта computed): для `background-*`, `border-*`, `box-shadow`, `border-radius` валидаторов и строк карты нет.

## Как найдено

WPT-RUN-14 срез 6: `css/css-backgrounds/parsing/*` и `inheritance.sub.html` — 45 файлов, 463 упавших сабтеста: `*-computed` 16, `*-invalid` 13, `*-valid` 12, прочее 4. Не входят `border-image-*` (они в BUG-492: свойства не разобраны вовсе).

## Что делать

Валидаторы и канонизаторы по грамматике свойства (как в CSSOM-2 для остальных семейств) и строки `background-position/-size/-repeat/-origin/-clip/-attachment`, `border-radius`, `border-*-color/-style`, `box-shadow` в `computed_style_to_map` с полным числом слоёв.

## Как проверить

`css/css-backgrounds/parsing/{background,border,box-shadow,border-radius}-*.html`, `inheritance.sub.html`.

# BUG-1764 — `cssText` не сворачивает `outline`/`list-style`, `-webkit-line-clamp` не псевдоним, значения `border-image` не разворачиваются

**Статус:** OPEN
**Заведён:** 2026-10-27 (P3, остаток [BUG-1552](BUG-1552-FIXED.md))
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_serialize_style`, `_lumen_expand_border_shorthand`)

## Симптом

`css/cssom/shorthand-values.html`, 3 из 21 красные:

| `cssText =` | у нас | ожидается |
|---|---|---|
| `outline-width: 2px; outline-style: dotted; outline-color: blue;` | три длинных свойства | `outline: blue dotted 2px;` |
| `list-style-type: circle; list-style-position: inside; list-style-image: none;` | три длинных свойства | `list-style: inside circle;` |
| `-webkit-line-clamp: none;` | `-webkit-line-clamp: none;` | `line-clamp: none;` |

Кроме того, `border-image` со значением, отличным от `none`/CSS-wide (`url(a.png) 30 / 10px round`), хранится одним сырым ключом: `style.borderImageSource` пуст, `border: 1px` поверх него не сбрасывает его в начальное значение так, как сбрасывает `border-image-*`.

## Что делать

Добавить `outline` (`<color> || <style> || <width>`, порядок сериализации colour style width) и `list-style` (`type || position || image`) в таблицы шорткодов `_lumen_serialize_style`/`setProperty`/`getPropertyValue` по образцу `_lumen_border_*`; `-webkit-line-clamp` — в `_LUMEN_COMPAT_ALIASES`; `border-image` — разбор в пять длинных свойств.

## Как проверить

`css/cssom/shorthand-values.html` — 21/21, `.ini` снять.

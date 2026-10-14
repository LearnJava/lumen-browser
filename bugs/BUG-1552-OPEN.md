# BUG-1552 — `style.cssFloat = "left"` пишет `css-float: left;`; `style.length` — строка; `item()` нет; `border: 1px` из длинных свойств не сворачивается

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_make_style`, сериализация шорткодов)

## Симптом

`CSSStyleDeclaration`: (а) `style.cssFloat = "left"` записывает декларацию `css-float: left;` (имя свойства получено заменой заглавной), `style.float` остаётся `""` и `getComputedStyle(el).float` — `none`: элемент не обтекается; (б) `style.length` — `string` (`typeof`) и `""`, `style.item` не функция, `style[0]` — `""`; (в) сериализация шорткода: `border: 1px; border-top: 1px` читается `border: 1px; border-top: 1px;` (ожидается `border: 1px;`), `cssom-setProperty-shorthand.html` — 12 упавших из 76 (удаление `border-color`/`border-style` длинным свойством), `shorthand-values.html` — 11 упавших из 21. Итерация (`[...style]`) и `getPropertyPriority` — BUG-1506/BUG-1459.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `el.style.cssFloat="left"` → `el.style.cssText` / `el.style.float` / `getComputedStyle(el).float` | `css-float: left;` / `""` / `none` | `float: left;` / `left` / `left` |
| `typeof el.style.length` / `el.style.length` для `style="color:red;margin:1px"` | `string` / `""` | `number` / 2 |
| `typeof el.style.item`, `el.style.item(0)` | `string`; `TypeError` | `function`; `color` |
| `border:1px; border-top:1px` через `cssText` | `border: 1px; border-top: 1px;` | `border: 1px;` |

## Как найдено

WPT-RUN-14 срез 25: `css/cssom/{cssstyledeclaration-csstext,css-style-declaration-modifications,inline-style-001,shorthand-values,cssom-setProperty-shorthand,shorthand-serialization,property-accessors,css-style-attr-decl-block}.html` (353 из 1 094 сабтестов кластера `om-serialization` вместе с BUG-1551).

## Что делать

Алиас `cssFloat` → `float` в именах свойств; `length`/`item()`/индексы — настоящие числа и методы (`Symbol.iterator` — BUG-1506); сворачивание `border`/`border-*`/`margin`/`padding` по алгоритму «serialize a CSS declaration block» (CSSOM §6.7.2).

## Как проверить

`css/cssom/shorthand-values.html`, `cssstyledeclaration-csstext.html`; `el.style.cssFloat="left"` обтекает.

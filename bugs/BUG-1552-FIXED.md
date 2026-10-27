# BUG-1552 — `style.cssFloat = "left"` пишет `css-float: left;`; `style.length` — строка; `item()` нет; `border: 1px` из длинных свойств не сворачивается

**Статус:** FIXED 2026-10-27 (P3; остаток — [BUG-1764](BUG-1764-OPEN.md))
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

## Исправление

`style.length`/`item()`/индексы к моменту взятия уже были исправлены в BUG-1506; осталось два симптома.

- `cssFloat` — IDL-имя `float`: `_lumen_camel_to_kebab('cssFloat')` возвращает `float` (раньше `css-float`), `'cssFloat' in style` истинно.
- `border`, `border-{top,right,bottom,left}`, `border-image: none` разворачиваются в длинные свойства (12 боковых + 5 `border-image-*`), хранение и чтение — `_lumen_border_*` рядом с `_lumen_serialize_style`. Сериализация по CSSOM §6.7.2: `border` (все 17 длинных свойств одной важности, `border-image-*` в начальном значении) → `border-width/-style/-color` → `border-<side>` → `border-image`; опущенные начальные значения (`border: 1px`, `border: none`).
- Разбор блока объявлений оставляет последнее из дубликатов на его месте (`_lumen_style_merge_decl`, как Blink): `border: 1px; border-top: 1px !important` → `border-top` в конце.
- `removeProperty`/`setProperty(p, '')` у TRBL-шорткодов (`margin`, `padding`, `border-width/-style/-color`) убирали только ключ шорткода и оставляли длинные свойства и их `!important` — теперь убирают всё.

Тесты: `crates/js/src/dom/tests/v8_bug1552_style_float_shorthand.rs` (в том числе 8 случаев `shorthand-values.html` и все `border*`/`margin`/`padding`/`overflow` из `cssom-setProperty-shorthand.html`). Ожидания `.ini` сняты по результату этих тестов; полный прогон `css/cssom` под `run_report.py` не выполнялся (порты WPT заняты чужой сессией).

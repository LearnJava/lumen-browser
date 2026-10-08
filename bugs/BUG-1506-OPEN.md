# BUG-1506 — `CSSRuleList`, `StyleSheetList` и `CSSStyleDeclaration` не итерируются (`[...sheet.cssRules]`, `for…of`, `[...el.style]`), нет `getPropertyPriority`

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 22, `css/css-view-transitions` + `css-conditional` + `css-variables` + `css-properties-values-api` + `css-mixins`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `CSSRuleList`, `StyleSheetList`, `CSSStyleDeclaration`)

## Симптом

`Array.from(sheet.cssRules)` работает, `[...sheet.cssRules]` — `TypeError: sheet.cssRules is not iterable` (нет `Symbol.iterator` у `cssRules`, `document.styleSheets`, `CSSStyleDeclaration`, у `style[Symbol.iterator]` — `typeof` `string`, потому что объект стиля отдаёт любое имя как свойство). `querySelectorAll`-список и `classList` итерируются. У `element.style` и `rule.style` нет метода `getPropertyPriority` (`is not a function`); у `getComputedStyle(el)` — есть и возвращает `` (см. BUG-1459).

## Проба

Проба (`--mcp`):

| выражение | у нас | ожидается |
|---|---|---|
| `[...s.sheet.cssRules].length` | `TypeError: … is not iterable` | `2` |
| `[...document.styleSheets].length` | `TypeError` | `1` |
| `[...t.style].length` | `TypeError` | число свойств |
| `typeof s.sheet.cssRules[Symbol.iterator]` | `undefined` | `function` |
| `Array.from(s.sheet.cssRules).length` | `2` | `2` |
| `typeof t.style.getPropertyPriority` | `TypeError` на вызов | `function` |

## Как найдено

WPT-RUN-14 срез 22: `css-properties-values-api/at-property-cssom.html` (39 из 39 сабтестов: `document.styleSheets[0].cssRules is not iterable`), `css-variables/variable-invalidation.html` (`getPropertyPriority is not a function`).

## Что делать

Добавить `[Symbol.iterator]` у трёх коллекций (итерация индексов; у `CSSStyleDeclaration` — имена свойств) и `getPropertyPriority` у `CSSStyleDeclaration`; `Symbol.iterator` у объекта стиля не должен отдавать строку.

## Как проверить

Таблица выше; `css/css-properties-values-api/at-property-cssom.html`.

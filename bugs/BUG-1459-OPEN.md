# BUG-1459 — Приоритет `!important` в инлайновом `element.style` теряется: `cssText = 'c: v !important'` и `el.style = '…'` отбрасывают декларацию, `setProperty(…, 'important')` не пишет приоритет, `getPropertyPriority` отсутствует

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_make_style`, `cssText`/`setProperty`/`getPropertyPriority`)

## Симптом

`el.style.cssText = "color: green !important"` (и `el.style = "color: green !important"`) не применяет декларацию вовсе и оставляет `cssText` пустым; `el.setAttribute('style', 'color: green !important')` и статический атрибут `style="color: green !important"` применяют её (цвет зелёный), но `el.style.cssText` их не показывает (пусто / без `color`). `el.style.setProperty('color','green','important')` пишет `color: green;` без `!important` — декларация перебивает обычное правило листа, но проигрывает `!important` листа (`#target{color:red !important}` побеждает). `typeof el.style.getPropertyPriority` — `string` (функции нет, имя разбирается как обычное свойство). Последствие — `css-cascade/layer-vs-inline-style.html` (2 из 4 сабтестов: `target.style = 'background-color: green !important'` при `@layer{#target{background-color:red}}`), связанные `layer-stylesheet-sharing-important`, `important-prop` и тесты на `!important` внутри атрибута; смежно с [BUG-1401](BUG-1401-OPEN.md) (`cssText="container-type:size !important"` читается как значение `size !important`).

## Проба

`<style>#target{width:10px;color:red}</style><div id=target>`, `getComputedStyle(target).color`:

| действие | цвет | `style.cssText` после | ожидается |
|---|---|---|---|
| `style.cssText = "color: green !important"` | **красный** | `` | зелёный, `color: green !important;` |
| `style = "color: green !important"` | **красный** | `` | зелёный |
| `setAttribute("style", "color: green !important")` | зелёный | `` (**пусто**) | `color: green !important;` |
| `<div style="color: green !important; width: 5px">` | зелёный | `width: 5px;` (**без color**) | `color: green !important; width: 5px;` |
| `style.setProperty("color", "green", "important")` | зелёный | `color: green;` | `color: green !important;` |
| то же при листовом `color: red !important` | **красный** | — | зелёный |
| `typeof style.getPropertyPriority` | `string` | — | `function` |

## Как найдено

WPT-RUN-14 срез 20: `css-cascade/layer-vs-inline-style.html` (сабтесты «Important inline style > normal layered style» и «… > important layered style»).

## Что делать

Хранить приоритет декларации в `CSSStyleDeclaration` (как значение и флаг), разбирать `!important` в `cssText`-сеттере и в `el.style = …`, отдавать его в `cssText`, `getPropertyPriority`, `setProperty(…, 'important')` и передавать каскаду как инлайновое `!important`.

## Как проверить

Таблица выше; `css/css-cascade/layer-vs-inline-style.html`.

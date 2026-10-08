# BUG-1565 — Typed OM: `StylePropertyMap.clear` нет, `get/has/delete/getAll("lemon")` не бросают `TypeError`, `set` недопустимого значения не бросает, `new CSSUnitValue(0,"lemon")` и `new CSSKeywordValue("")` не бросают

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/typed_om_api.rs` — `StylePropertyMap`, `CSSUnitValue`, `CSSKeywordValue`)

## Симптом

Проверки типов и имён из Typed OM L1 §3 отсутствуют: `attributeStyleMap.clear` — `undefined`; `get("lemon")` возвращает `undefined` вместо `TypeError`; `set("color", CSS.px(1))` принимается; `computedStyleMap().get("lemon")` — `undefined`; `new CSSUnitValue(0,"lemon")` и `new CSSKeywordValue("")` создаются; `CSSStyleValue.parse("","auto")` не бросает; `CSSKeywordValue.value = ""` и `CSSUnparsedValue[3] = "foo"` не бросают. 32 id `the-stylepropertymap` (127 из 176) и 9 id `stylevalue-*` (17 из 65).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `el.attributeStyleMap.get("lemon")` | `undefined` | `TypeError` |
| `el.attributeStyleMap.set("color", CSS.px(1))` | не бросает | `TypeError` |
| `typeof el.attributeStyleMap.clear` | `undefined` | `function` |
| `new CSSUnitValue(0,"lemon")` | не бросает | `TypeError` |
| `new CSSKeywordValue("")` | не бросает | `TypeError` |
| `el.computedStyleMap().size` | 299 | число свойств, включая шорткоды (`size` = все поддержанные) |

## Как найдено

WPT-RUN-14 срез 25: `css/css-typed-om/the-stylepropertymap/{inline,declared,computed}/*.html` (32), `stylevalue-objects/parse*-invalid.html`, `stylevalue-subclasses/{cssKeywordValue-*,cssUnparsedValue-*,cssVariableReferenceValue-variable}.html`.

## Что делать

Добавить проверки (`TypeError`), `clear()`, нормализацию шорткодов в `get`/`set`, валидацию единиц у `CSSUnitValue`.

## Как проверить

`css/css-typed-om/the-stylepropertymap/inline/get-invalid.html`, `clear.html`.

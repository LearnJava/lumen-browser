# BUG-1564 — Typed OM: нет `CSSNumericValue.parse`, `CSSColorValue.parse`, `CSSMathClamp`, `CSSImageValue`, `CSSNumericArray`, `CSS.Q/Hz/kHz/cap/svw/lvh/dvh/cqw/cqi/flex`; `value.type` — свойство, а не метод

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/typed_om_api.rs`)

## Симптом

Отсутствуют интерфейсы и фабрики: `typeof CSSMathClamp`, `CSSImageValue`, `CSSNumericArray`, `CSSPositionValue`, `CSSNumericType` — `undefined`; `CSSNumericValue.parse`, `CSSColorValue.parse` — `undefined`; `CSS.Q`, `CSS.Hz`, `CSS.kHz`, `CSS.cap`, `CSS.svw`, `CSS.lvh`, `CSS.dvh`, `CSS.cqw`, `CSS.cqi`, `CSS.flex` — `undefined` (остальные 33 фабрики есть). `numericValue.type` в WebIDL — метод `type()`, у нас — не функция (`a.type is not a function`). Конструкторы цвета (`CSSHSL`, `CSSHWB`, `CSSLab`, `CSSLCH`, `CSSOKLab`, `CSSOKLCH`) отвергают допустимые значения каналов и читают процент как число. `CSSStyleValue.parse("transform","rotate(1deg)")` — `CSSStyleValue`, а не `CSSTransformValue`; `new CSSKeywordValue("a b").toString()` — `a b` (тест ждёт `\ Hello\ World`); `CSSUnparsedValue` теряет `/**/`. 40 id, 65+56+40+11 сабтестов (`idlharness` — 380 из 544).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `typeof CSSMathClamp + typeof CSSImageValue + typeof CSSNumericArray + typeof CSSPositionValue` | `undefined` ×4 | `function` ×4 |
| `typeof CSSNumericValue.parse`, `typeof CSSColorValue.parse` | `undefined`, `undefined` | `function`, `function` |
| `["Q","Hz","kHz","cap","svw","lvh","dvh","cqw","cqi","flex"].map(u => typeof CSS[u])` | `undefined` ×10 | `function` ×10 |
| `CSS.px(1).type` / `typeof CSS.px(1).type` | свойство / не функция | `type()` — функция |
| `CSSStyleValue.parse("transform","rotate(1deg)").constructor.name` | `CSSStyleValue` | `CSSTransformValue` |

## Как найдено

WPT-RUN-14 срез 25: `css/css-typed-om/{idlharness,factory-*,stylevalue-subclasses/*,stylevalue-normalization/*,stylevalue-serialization/*}.html` (кластеры `to-numparse`, `to-colorvalue`, `to-factory`, `to-mathclamp`, `to-other`).

## Что делать

Реализовать недостающие интерфейсы и методы Typed OM L1 (§5–§6), переименовать свойство `type` в метод, добавить недостающие единицы в `CSS.*`.

## Как проверить

`css/css-typed-om/factory-frequency.html`, `stylevalue-subclasses/numeric-objects/parse.tentative.html`.

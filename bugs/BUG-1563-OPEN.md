# BUG-1563 — Typed OM: `new CSSMathSum(a, b)`, `CSSMathProduct`, `CSSMathMin`, `CSSMathMax` принимают массив вместо перечисления аргументов — 243 теста падают в `ERROR` на загрузке

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** js (`crates/js/src/typed_om_api.rs:169-213` — `function CSSMathSum(values)` и т. д.)

## Симптом

WebIDL: `CSSMathSum(CSSNumberish... args)` — переменное число аргументов. У нас `function CSSMathSum(values) { … values.map(…) }`: `new CSSMathSum(CSS.px(1), CSS.em(2))` — `TypeError: values.map is not a function`. `resources/testsuite.js` (общий для 244 файлов `the-stylepropertymap/properties/*`) строит `new CSSMathSum(new CSSUnitValue(0,"px"), …)` на верхнем уровне, поэтому все 243 файла — `ERROR` без единого сабтеста (`values.map is not a function`). A/B: обёртка конструкторов в `testsuite.js` (не закоммичена) → 239 `OK` и 5 `ERROR`, но 1 146 из 11 311 сабтестов зелёные (7 561 «did not throw», 853 «Computed value must be a CSSStyleValue», 712 `var()`, 569 `.type is not a function` — BUG-1564/1565).

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `new CSSMathSum(CSS.px(1), CSS.em(2))` | `TypeError: values.map is not a function` | объект `CSSMathSum` |
| `new CSSMathSum([CSS.px(1), CSS.em(2)])` | работает | `TypeError`/объект с одним элементом-массивом |
| `css-typed-om/the-stylepropertymap/properties/*.html` — статус | 243 × `ERROR` | `OK` с сабтестами |
| A/B с обёрткой конструкторов (`testsuite.js` временно) — сабтестов зелёных | 1 146 из 11 311 | все |

## Как найдено

WPT-RUN-14 срез 25: `css/css-typed-om/the-stylepropertymap/properties/*.html` (243), `stylevalue-subclasses/{cssRGB,cssPerspective,cssRotate,cssScale,cssSkew,cssSkewX,cssSkewY,cssTranslate}.html`, `stylevalue-serialization/cssMathValue.tentative.html`, `parse-calc-expressions.html` (263 id кластера `to-mathctor`).

## Что делать

Принимать `...args` (и плоский массив как один аргумент), проверять тип (`CSSNumberish`), не ломать внутренние вызовы `flattenSameOperator`.

## Как проверить

`css/css-typed-om/the-stylepropertymap/properties/accent-color.html` — не `ERROR`; `stylevalue-serialization/cssMathValue.tentative.html`.

# BUG-601: `DOMTokenList` constructor not exposed on `window` — `instanceof DOMTokenList` throws `ReferenceError`, even though the object itself works correctly

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `// ── DOMTokenList (classList) ──` section around line 1191)
**Найден:** P2, WPT-VENDOR-html-interaction, 2026-08-04

## Симптом

```
FAIL focusGroup IDL attribute is a DOMTokenList - DOMTokenList is not defined
FAIL focusGroup DOMTokenList and focusGroupStart are exposed on SVGElement via the HTMLOrSVGOrMathMLElement mixin - DOMTokenList is not defined
FAIL focusGroup DOMTokenList and focusGroupStart are exposed on MathMLElement via the HTMLOrSVGOrMathMLElement mixin - DOMTokenList is not defined
```
(`focusgroup/tentative/idl-reflection.html`, 3 of 20 subtests — every other
subtest in the same file, exercising `.value`, `.contains()`, `.add()`,
`.remove()`, `.toggle()`, `.supports()`, `[SameObject]` identity and
`PutForwards` on the very same `element.focusGroup` object, **passes**)

## Причина

`element.focusGroup`/`classList` are backed by a real, spec-correct
`DOMTokenList`-shaped object internally (17/20 subtests in this file
confirm every method and the `[SameObject]` contract work), but the
constructor is never assigned as a global — `typeof window.DOMTokenList` is
`"undefined"`. Any test or page script that does `x instanceof DOMTokenList`
or `new DOMTokenList(...)` fails, even though the object it's checking is
functionally correct.

## Масштаб

Narrow and mechanical: add `window.DOMTokenList = <the internal
constructor/class>` (or an equivalent global binding) next to the existing
`DOMTokenList (classList)` implementation. Only the identity-check subtests
fail; no functional regression in `classList`/`focusGroup` themselves.

## Ревизия P3 2026-09-16

Заявка описывала состояние до BUG-715 (2026-09-05), который уже дал
`DOMTokenList` реальный глобальный конструктор/прототип
(`crates/js/src/shim/web_api_shim_mid.js:1303-1365`,
`globalThis.DOMTokenList = DOMTokenList`) — сама заявленная причина закрыта
чужим срезом, но перепроверка живым прогоном
(`tests/wpt/run_report.py --binary <bin> --all --root
html/interaction/focus/focusgroup/tentative --recursive --offset 13 --limit
1`) на `focusgroup/tentative/idl-reflection.html` даёт **1/21** сабтестов,
не «17/20 проходят кроме identity-проверок», как утверждала заявка. `grep`
по `focusGroup`/`focusgroup` (регистронезависимо) в `crates/js/src` и во
всех JS-шимах — **ноль совпадений**: `element.focusGroup`/`.focusGroupStart`
не заведены вовсе, `classList`/`relList` — единственные существующие
IDL-члены на базе `DOMTokenList`. Заявленный «функционально корректный,
просто без глобального конструктора» объект никогда не существовал для
`focusGroup` — это не идентификационный дефект, а целиком отсутствующая
IDL-рефлексия HTML LS `focusgroup`/`focusgroupstart` content-атрибутов
(new-element-content-attribute reflection + сам `DOMTokenList`-объект
над атрибутом `focusgroup`, PutForwards-семантика, `supports()` со списком
из 12 стандартных токенов). Переквалифицировано в ДОРАБОТКУ →
[GAP-FOCUSGROUP](../ROADMAP.md). Указатель убран из `STATUS-P3.md`.

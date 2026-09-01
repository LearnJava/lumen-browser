# BUG-522: CSS/DOM Geometry Interfaces (`DOMMatrix`, `DOMPoint`, `DOMRect`, `DOMQuad`, `WebKitCSSMatrix`) don't exist as globals

**Статус:** OPEN (ДОРАБОТКА → [GAP-GEOM](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — ведётся как задача `GAP-GEOM` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт. Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6: срезы заводили багом всё подряд, потому что правила заведения ([docs/probe-method.md §8](../docs/probe-method.md)) тогда ещё не было. Файл сохраняет номер и путь — на него ссылаются CLAUDE.md, STATUS-файлы и python-тулинг, а запись наблюдений остаётся полезной там, где лежит.
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs` — no `DOMMatrix`/`DOMPoint`/
`DOMRect`/`DOMQuad`/`WebKitCSSMatrix` constructor anywhere;
`grep -n "DOMMatrix\|DOMPoint\|DOMRect\|DOMQuad" crates/js/src/dom.rs`
returns zero hits)
**Найден:** WPT-RUN-3 срез 23 (`ROADMAP.md`) — массовый прогон `css/geometry`

## Механизм

The Geometry Interfaces Module (`DOMMatrixReadOnly`/`DOMMatrix`,
`DOMPointReadOnly`/`DOMPoint`, `DOMRectReadOnly`/`DOMRect`,
`DOMRectList`, `DOMQuad`, and the legacy `WebKitCSSMatrix` alias) is not
implemented at all — none of these names exist on `window`. This is the
biggest single gap the WPT-RUN-3 track has found to date by subtest count:
`window.DOMMatrix`/`DOMPoint`/`DOMRect`/`DOMQuad`/`WebKitCSSMatrix` are all
`undefined`, so `new DOMMatrix()` and friends throw `TypeError: self[constr]
is not a constructor` (the WPT idiom that iterates a list of constructor
names and does `new self[name]`) or plain `ReferenceError` for any test
that references the bare identifier.

The gap has two secondary, dependent symptoms:

1. **`Element.prototype.getClientRects` is missing entirely** — unlike
   `getBoundingClientRect()` (which exists but returns a plain object
   literal, not a `DOMRect` instance — the same shape as the
   `innerHTML`/`getBoundingClientRect` "Phase 0 stub" class documented in
   [BUG-368](BUG-368-OPEN.md)/CAPABILITIES.md), `getClientRects()` has no
   binding on the live element path at all: `document.getElementById(id)
   .getClientRects is not a function`.

2. **Canvas 2D's `setTransform()`/`getTransform()` are blocked by the same
   gap.** `CanvasRenderingContext2D.prototype.setTransform`
   (`dom.rs:5230`) only accepts the 6-number positional form
   (`setTransform(a,b,c,d,e,f)`, coercing each argument with `+a` —
   silently `NaN` for a dict argument instead of validating or throwing)
   and has no overload for the `DOMMatrix2DInit`/`DOMMatrixInit` dictionary
   form used by `ctx.setTransform({a:1, m11:2, ...})`.
   `CanvasRenderingContext2D.prototype.getTransform` does not exist at
   all (`grep -n "getTransform" crates/js/src/canvas2d.rs crates/js/
   src/dom.rs` — zero hits) — it would need to construct and return a
   `DOMMatrix`, which doesn't exist.

## Симптом

```
FAIL new DOMMatrix() - self[constr] is not a constructor
FAIL new DOMMatrix(new DOMMatrix()) - self[constr] is not a constructor
FAIL DOMRect constructor without parameter - constructor is not a constructor
FAIL matrixTransform - DOMPoint is not defined
FAIL DOMQuad irregular - DOMQuad is not defined
FAIL Equivalence test - assert_true: WebKitCSSMatrix should exist expected true got false
ERROR DOMRectList.html - TypeError: document.getElementById(...).getClientRects is not a function
FAIL setTransform({a: 1, m11: 2}) (invalid) - assert_throws_js: function
  "() => ctx.setTransform(dict)" did not throw
FAIL setTransform (Sanity check without dictionary) - ctx.getTransform is not a function
```

Three files time out at the wptrunner level instead of failing fast
(`DOMMatrix-001.html`, `DOMRect-001.html`, `DOMRect-nan.html` — same root
cause, harness never reaches its own internal per-subtest timeout path
cleanly; not isolated further this slice).

## Масштаб находки

23/27 harness-OK files in `css/geometry`, essentially all of them failing
on this one root cause (720 of the category's 784 subtests). The two
`DOMMatrix*-validate-fixup.html` files additionally exercise Canvas 2D's
`setTransform`, contributing 64 more subtests there. `idlharness.any.html`
is the standard un-vendored `/resources/idlharness.js` infra gap (same
class as BUG-367/374/392/397), not attributed here.

## Что нужно

Implement the Geometry Interfaces Module as real global constructors with
proper prototypes (`DOMMatrixReadOnly`/`DOMMatrix`, `DOMPointReadOnly`/
`DOMPoint`, `DOMRectReadOnly`/`DOMRect`, `DOMRectList`, `DOMQuad`,
`WebKitCSSMatrix` as an alias). Once `DOMRect`/`DOMMatrix` exist,
`getBoundingClientRect()`/`getClientRects()`/`Canvas2D.getTransform()`
should return real instances instead of plain objects, and
`setTransform()` should gain the dictionary overload with the spec's
NaN/Infinity validation.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/geometry/` for all 23
harness-OK files plus the 3 TIMEOUT files (`expected: TIMEOUT`) and
`DOMRectList.html` (`expected: ERROR`). `idlharness.any.html` left without
`.ini` (infra gap, not attributed).

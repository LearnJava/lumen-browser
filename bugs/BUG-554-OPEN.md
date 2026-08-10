# BUG-554: CSS Typed OM value-type hierarchy and unit-factory functions almost entirely missing — only a narrow base slice exists

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** js (`crates/js/src/typed_om_api.rs`)
**Найден:** WPT-RUN-3 срез 37 (`ROADMAP.md`) — массовый прогон `css/css-typed-om`

## Механизм

`typed_om_api.rs` defines only a thin slice of the CSS Typed OM spec
(<https://www.w3.org/TR/css-typed-om-1/>): `CSSStyleValue` (base class, only
a `toString`), `CSSUnitValue`, `CSSKeywordValue`, a partial `CSSNumericValue`
(a `parse`-like helper, not the real numeric-value base class), and
the two property maps. Confirmed by grep — no other Typed OM identifier
appears anywhere in `crates/js/`:

> Update 2026-08-10: the maps themselves are no longer part of this report.
> [BUG-387](BUG-387-FIXED.md) rebuilt them — `StylePropertyMapReadOnly`
> (base, what `computedStyleMap()` returns) + `StylePropertyMap` (mutable),
> each with its own source, plus `getAll`/`has`/`size`/`entries`/`keys`/
> `values`/`forEach`/`@@iterator`. The name `ComputedStylePropertyMap`, used
> below and in the original text of this report, no longer exists. What is
> listed below — the value-type hierarchy and the unit factories — is
> untouched and still the whole of this bug.

- **Unit factory functions** — the `CSS` namespace has no `CSS.px()`,
  `CSS.em()`, `CSS.number()`, `CSS.deg()`, `CSS.rad()`, `CSS.grad()`,
  `CSS.cm()`/`.mm()`/`.Q()`/`.in()`/`.pt()`/`.pc()`, etc. — every one of
  `CSSNumericFactory`'s ~30 methods is undefined; `CSS.deg is not a
  function` and siblings dominate the `stylevalue-subclasses/` failures.
- **`CSSMathValue` hierarchy** — `CSSMathSum`/`CSSMathProduct`/
  `CSSMathNegate`/`CSSMathInvert`/`CSSMathMin`/`CSSMathMax`/`CSSMathClamp`
  are all undefined; arithmetic operators on numeric values
  (`CSSUnitValue.prototype.add`/`.mul`/etc., which would need to return
  these) don't exist either.
- **`CSSTransformValue`** and its component subclasses (`CSSTranslate`,
  `CSSRotate`, `CSSScale`, `CSSSkew`, `CSSSkewX`, `CSSSkewY`,
  `CSSPerspective`, `CSSMatrixComponent`) are all undefined — every
  `stylevalue-subclasses/css{Translate,Rotate,Scale,Skew*,Perspective,
  MatrixComponent}*.html` file throws at its own top-level test-data setup
  before a single `test()` registers.
- **`CSSColorValue`** and its subclasses (`CSSRGB`, `CSSHSL`, `CSSHWB`,
  `CSSLab`, `CSSLCH`, `CSSOKLab`, `CSSOKLCH`) are all undefined — same
  top-level-throw pattern as the transform subclasses.
- **`CSSUnparsedValue`** and **`CSSVariableReferenceValue`** (the
  `var()`-token representation) are undefined.
- **`CSSStyleValue.parse()`/`.parseAll()`** static factory methods (the
  spec's primary "parse an arbitrary CSS value string" entry point,
  independent of any `StylePropertyMap`) don't exist on the base class.
- **`StylePropertyMap` mutation** — `.set()`/`.append()`/`.delete()` on
  both the declared (`CSSStyleRule.styleMap`) and inline
  (`Element.attributeStyleMap`) flavors are absent; only the read side
  (`.get()`, itself scoped by BUG-387) exists.

## Симптом

`css/css-typed-om` mass run (WPT-RUN-3 slice 37): 326/359 harness OK,
28/1364 subtests passed. Of the 1336 failing subtests
plus 33 zero-subtest harness TIMEOUTs, **185 subtest failures and 32 of the
33 TIMEOUTs** are this bug (the remaining 1151 subtest failures are a
*different*, already-documented issue — [BUG-346](BUG-346-OPEN.md)'s
`Url::resolve()` dot-segment gap 404s the category's own
`../resources/testhelper.js` on 333 of 374 files, and the 1 remaining
TIMEOUT is `idlharness.html`, whose `/resources/idlharness.js` dependency
is simply unvendored — same non-engine pattern seen in every prior
category). Representative direct hits: `CSS.deg is not a function`
(`stylevalue-subclasses/cssHSL.html`, top-level, 0 subtests ever register),
`CSSMathSum is not defined` (17), `CSSUnparsedValue is not defined` (16),
`CSSStyleValue.parse is not a function` (11), `CSSRGB is not defined` (10).

## Масштаб находки

Second-largest cluster in `css-typed-om` after the BUG-346 masking (which
this bug's own extension note already flagged as "the next layer of
failure" once BUG-346 is fixed — see `BUG-346-OPEN.md`, "Срез 30" section).
`CSS-SPECS.md:138` already correctly scopes CSS Typed OM as unclaimed
("JS API; P3 territory"), so no doc-status correction is needed — this
entry exists so the P3 backlog has a concrete, itemized starting point
instead of a bare "not done" line. Fixing in priority order by WPT weight:
(1) `CSS.<unit>()` factory functions — unblocks the largest single class of
top-level throws across `stylevalue-subclasses/`; (2) `CSSStyleValue.parse`/
`.parseAll` — unblocks `stylevalue-normalization/` once BUG-346 is fixed;
(3) the `CSSTransformValue`/`CSSColorValue` subclass families; (4)
`CSSMathValue` arithmetic; (5) `StylePropertyMap` mutation methods.

# BUG-484: Inline `style` setter never parses/validates values — no rejection, no shorthand expansion, no canonicalization

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:4264-4302` — `_lumen_make_style`)
**Найден:** WPT-RUN-3 срез 5 (`ROADMAP.md`) — массовый прогон `css/css-box`

## Механизм

`_lumen_make_style(nid)` (`dom.rs:4264`) implements `CSSStyleDeclaration` for
inline `element.style` entirely in JS, without ever routing the value
through `css-parser`'s real grammar:

```js
setProperty: function(prop, val) {
    var obj = getParsed();
    obj[_lumen_camel_to_kebab(String(prop))] = String(val);   // dom.rs:4276
    setParsed(obj);
},
```

and the bracket/dot-property `set` trap (`dom.rs:4296-4299`) calls the same
`setProperty` for any property not already on the handler object. Both paths
do exactly one thing: stringify the value and store it verbatim as the
property's serialized form (`prop: val` joined into the `style="…"`
attribute by `_lumen_serialize_style`, `dom.rs:4257`). There is no per-property
grammar table, no rejection of a value that doesn't parse, no expansion of a
shorthand into its longhands, and no re-serialization into canonical form.

## Симптом

Three distinct assertion shapes in `css/support/parsing-testcommon.js`-based
tests, all traceable to the same missing step ("run the value through the
real parser"):

**1. Invalid values are accepted instead of rejected** (CSSOM §Change a
computed value: if the value doesn't match the property's grammar, the
`[[declarations]]` entry must not be updated — reading the property back
should give `""`):

```
FAIL e.style['width'] = "10px border-box" should not set the property value
  - assert_equals: expected "" but got "10px border-box"
FAIL e.style['overflow'] = "visible hidden scroll" should not set the property value
  - assert_equals: expected "" but got "visible hidden scroll"
FAIL e.style['margin-top'] = "60" should not set the property value  (unitless length)
```

**2. Shorthand properties are not expanded into longhands** — setting
`e.style.margin = "1px 2px 3px 4px"` stores the literal string under the key
`"margin"` and never touches `margin-top`/`-right`/`-bottom`/`-left`, so
reading any of the four longhands back gives `""`:

```
FAIL e.style['margin'] = "1px 2px 3px 4px" should set margin-top
  - assert_equals: margin-top should be canonical expected "1px" but got ""
```

**3. Multi-token/`calc()` values are stored raw, not canonicalized** — e.g.
`margin-trim: "block-start block-end"` should serialize back as the
canonical `"block"` (CSS Box §margin-trim serialization), and
`calc(2em + 3%)` should reorder operands to `calc(3% + 2em)` per the CSSOM
`<calc-sum>` serialization algorithm; both come back byte-identical to the
input instead.

## Масштаб находки

14 files / 133 subtests in this slice (`css/css-box`), all through the same
code path — no other defect masked underneath (each failing subtest's
message is one of the three shapes above, nothing else):

- **Rejection (shape 1)**: `parsing/clear-invalid.html` (2),
  `parsing/float-invalid.html` (3), `parsing/height-invalid.html` (10),
  `parsing/margin-invalid.html` (7), `parsing/max-height-invalid.html` (12),
  `parsing/max-width-invalid.html` (12), `parsing/overflow-invalid.html` (6),
  `parsing/padding-invalid.html` (10), `parsing/visibility-invalid.html` (2),
  `parsing/width-invalid.html` (13) — 77 subtests.
- **Shorthand expansion (shape 2)**: `parsing/margin-shorthand.html` (16),
  `parsing/padding-shorthand.html` (16) — 32 subtests.
- **Canonicalization (shape 3)**: `parsing/margin-trim.html` — 23 of its 34
  subtests (10 canonicalization + 13 rejection of malformed `margin-trim`
  values, e.g. duplicate keywords, mixing `block`/`block-start`, `auto`,
  `left` — same shape-1 rejection gap, just against a property with no
  parser support at all so *everything* about it is unvalidated);
  `parsing/padding-valid.html` — 1 of its 11 subtests (`calc()` operand
  order not canonicalized).

Not css-box-specific — same `_lumen_make_style` code path backs every
`element.style` mutation in the engine, so this will recur in every future
WPT-RUN-3 slice that exercises `*-invalid.html`/`*-shorthand.html`/
`*-valid.html` parsing tests (the `css/support/parsing-testcommon.js`
pattern is as widely used across `css/` as `computed-testcommon.js`, see
[BUG-483](BUG-483-OPEN.md)).

## Что нужно

Route `setProperty`/the bracket `set` trap through `css-parser`'s actual
value-grammar validation before storing anything: reject (no-op) on parse
failure, expand registered shorthands into their longhand components, and
serialize accepted values through the canonical serializer instead of
storing the raw input string. This is a substantial CSSOM-correctness gap,
likely proportional in effort to the `css-parser` crate's existing per-property
grammar coverage — worth scoping as its own implementation task rather than
a quick patch.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-box/` for the 12
attributed files (both `-invalid.html` and `-shorthand.html`, plus
`margin-trim.html`/`padding-valid.html`), `expected: FAIL` per the actual
run.

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

**WPT-RUN-3 срез 6 (`css/css-cascade`, 2026-08-02)** confirmed the
prediction: `parsing/all-invalid.html` (7 subtests, `e.style['all'] =
"..."` for various malformed `all`-shorthand values) fails on exactly shape
1 (rejection) — `assert_equals: expected "" but got "<the invalid value>"`
for each. Committed `.ini` under `tests/wpt/metadata/css/css-cascade/`.

**WPT-RUN-3 срез 9 (`css/css-backgrounds`, 2026-08-02)** — largest
extension yet: 34 files / ~300 subtests, dominating the `parsing/`
subdirectory (`background-{attachment,clip,color,image,origin,position,
position-x,position-y,repeat,size}-{valid,invalid}.html`,
`border-{color,style,width}-shorthand.html`, `border-shorthand.html`,
`background-shorthand-serialization.html`'s `assert_in_array` cases,
`webkit-border-radius-valid.html`). Two new observations, both the same
underlying `_lumen_make_style` Proxy: (1) `box-shadow-invalid.html`
(40 subtests) and `background-image-invalid.html` (12, `cross-fade()`/
`radial-gradient()` malformed-argument cases) confirm shape 1 extends to
function-value grammars, not just simple tokens; (2)
`webkit-border-radius-valid.html` additionally throws `"style is not
iterable"` on 2 subtests that spread/iterate `element.style` — the same
Proxy is also missing `Symbol.iterator`/`length`, the identical
missing-trap class already documented on the *other* style-like Proxy in
[BUG-483](BUG-483-OPEN.md) (`getComputedStyle`'s). Not a new bug, just
confirms the class recurs on both Proxies independently.

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
run. Срез 9 added `.ini` under `tests/wpt/metadata/css/css-backgrounds/`
for 34 more files (several shared with BUG-463/472/492/495 — one `.ini`
header per file references every bug that owns a subtest in it).

## Срез 10 (`css/css-variables`, 2026-08-02) — a fourth symptom shape: `cssText`
getter is a raw attribute passthrough, not derived from the parsed model at all

`cssText`'s getter (`dom.rs:4287`) is even more literal than `setProperty`'s
storage: `get: function() { return _lumen_get_attr(nid, 'style') || ''; }` —
it returns the **raw `style="…"` HTML-attribute text verbatim**, bypassing
`getParsed()`/`_lumen_parse_style` entirely. This compounds the already-
documented "no real declaration-block model" gap with three additional,
directly-observable defects, all confirmed via WPT this slice:

- **No per-property deduplication.** `style="background: var(--prop);
  background: green;"` (two declarations for the same property, written
  directly in markup — a real `CSSStyleDeclaration` keeps only the *last*
  one per the CSS declaration-block model) round-trips through `cssText`
  with both still present, instead of collapsing to `"background:
  green;"`.
- **No comment stripping.** `/* comment */` sequences inside a declaration
  value are preserved verbatim instead of being stripped at parse time.
- **No spacing/`!important` normalization.** `style="--var3:red"` (no space
  after `:`, source as typed) reads back as `"--var3:red"` instead of the
  canonical `"--var3: red;"` (space after `:`, trailing `;`); `style="--var4:red!important"`
  similarly misses the space before `!important`.

`variable-cssText.html`'s 6 failing subtests (`target6`…`target11`) are all
instances of the first two defects. `variable-invalidation.html`'s "inline
style test"/"inline style test important" (2 of its 4 subtests — the other
2 are [BUG-471](BUG-471-OPEN.md)) are the third. `variable-reference-shorthands.html`'s
5 failing subtests (`target1`/`target2`/`target3` `margin`/`margin-top`)
and part of `variable-reference.html`'s failures are the **shorthand-not-
expanded** symptom already documented above (setting `margin-top` after
`margin: var(--prop)` never invalidates `margin`'s serializability — the
flat key-value map has no concept of one declaration superseding another
across shorthand/longhand boundaries). `var-parsing.html` (5 subtests) and
8 of `variable-reference.html`'s subtests are the **invalid value accepted
instead of rejected** symptom, specifically for malformed `var()` argument
syntax (`var(--x ())`, `var(prop)` without `--`, `var(--prop 20px)`, `var(20px)`,
…) — the inline-style setter never validates that a `var()` reference's
argument list is even syntactically well-formed before storing it verbatim.

## .ini (срез 10)

Committed `.ini` under `tests/wpt/metadata/css/css-variables/` for
`variable-cssText.html`, `var-parsing.html`, `variable-reference.html`
(BUG-484 subtests only — the `.sheet.cssRules` one is BUG-471),
`variable-reference-shorthands.html`, and `variable-invalidation.html`
(2 of its 4 subtests — the other 2 cite BUG-471).

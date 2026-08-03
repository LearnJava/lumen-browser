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

## Срез 12 (`css/css-logical`, 2026-08-02) — largest extension yet, all three shapes recur for CSS Logical Properties

32 files / ~280 subtests, dominating the `parsing/` subdirectory of
`css/css-logical` end to end. **Rejection (shape 1)**: every
`*-invalid.html` file in the category (`block-size`, `inline-size`,
`max-/min-block-/inline-size`, `border-block-/inline-{color,style,width}`,
`inset`, `inset-block-inline`, `margin-block-inline`, `padding-block-inline`
— 16 files) — invalid values like `"10px border-box"`, unitless lengths,
malformed multi-token forms are all stored verbatim instead of rejected.
**Shorthand expansion (shape 2)**: `inset-shorthand.html`,
`margin-block-inline-shorthand.html`, `padding-block-inline-shorthand.html`,
`inset-block-inline-shorthand.html` (4 files) — setting `e.style.inset =
"1px 2px 3px 4px"` (or `margin-inline`/`padding-block`/`inset-block`/
`inset-inline`) never populates the four (or two) physical/flow-relative
longhands, all read back `""`. **Canonicalization (shape 3)**: every
`*-valid.html` file (`border-block-/inline-{color,style,width}-valid`,
`border-block-/inline-valid`, `inset-valid`, `inset-block-inline-valid`,
`margin-block-inline-valid`, `padding-block-inline-valid` — 12 files) —
`calc()` operand reordering, hex-to-`rgb()` color canonicalization,
duplicate-value collapsing (`"hidden hidden"` → `"hidden"`, `"auto auto"`
→ `"auto"`) all come back byte-identical to the input. `.ini` under
`tests/wpt/metadata/css/css-logical/` for all 32 files, `expected: FAIL`
per subtest.

## Срез 14 (`css/css-color-hdr`, 2026-08-02) — rejection shape on a property that has zero implementation, not just zero validation

`parsing.html` (1 file, 16 of its 29 subtests): every malformed
`dynamic-range-limit`/`dynamic-range-limit-mix()` value (missing/negative/
out-of-range percentages, invalid keywords like `"none"`/`"hdr"`/`"sdr"`,
space-separated instead of comma-separated mix args) is stored verbatim
instead of rejected — same shape-1 signature as every prior slice. The
other 13 of 29 subtests in this file pass (their value happens to
round-trip regardless of validation); `computed.html`/`inheritance.html`/
`interpolation.html` fail on a distinct, more fundamental gap — the
property isn't recognized *at all* (not merely unvalidated) — filed
separately as [BUG-508](BUG-508-OPEN.md). `.ini` under
`tests/wpt/metadata/css/css-color-hdr/parsing.html.ini`, `expected: FAIL`
per subtest (all 16 attributable to this bug).

## Срез 15 (`css/css-easing`, 2026-08-02) — both shapes recur for easing-function values, plus a fourth canonicalization variant (keyword-to-function aliasing)

**Rejection (shape 1)**: `timing-functions-syntax-invalid.html` (13/13
subtests, whole file) — `"auto"`, `"ease-in ease-out"` (two values where one
is expected), out-of-arity `cubic-bezier(1, 2, 3)`/`cubic-bezier(1, 2, 3, 4,
5)`, out-of-range `cubic-bezier(-0.1, 0.1, 0.5, 0.9)` (`x1`/`x3` must be in
`[0, 1]` per spec), a list with one invalid member (`"initial,
cubic-bezier(0, -2, 1, 3)"`), and invalid `steps()` argument combinations
(`steps(1, jump-none)` — `jump-none` requires `n >= 2`; `steps(-100, ...)`/
`steps(0, ...)` — `n` must be `>= 1` for `start`/`end`, `>= 2` for
`jump-none`) are all stored verbatim instead of rejected. `step-timing-
functions-syntax.html` (5 of 12) and `linear-timing-functions-syntax.html`
(8 of ~24) contribute more instances of the identical shape (`steps(0,
*)`, `linear()`/`linear(0)`/`linear(100%)` — arity-zero or single-stop
`linear()` is invalid per spec, needs `>= 2` stops). **Canonicalization
(shape 3)**: `timing-functions-syntax-valid.html` (3/21) —
`cubic-bezier(calc(-2), calc(0.7 / 2), ...)` not simplified to `calc(0.35)`,
`steps(calc(5 / 2), start)` not simplified to `steps(calc(2.5), start)`;
`linear-timing-functions-syntax.html` (3 more) — whitespace inside
`linear(...)` not stripped, `calc(50% - 50%)` not simplified to `calc(0%)`,
`calc(0/0)` not evaluated. **New shape 4, keyword-to-function aliasing**:
CSS Easing L1 defines `step-start`/`step-end` as pure serialization aliases
of `steps(1, start)`/`steps(1)`, and `steps(N, end)`/`steps(N, jump-end)` as
aliases of the shorter canonical `steps(N)` (the `end`/`jump-end` keyword is
the *default* jump-position and must not round-trip) — none of this alias
folding happens, so `e.style['animation-timing-function'] = "step-start"`
serializes back as the literal input `"step-start"` instead of `"steps(1,
start)"`, and `"steps(2, end)"`/`"steps(2, jump-end)"` stay as typed instead
of collapsing to `"steps(2)"`. 6 occurrences across `step-timing-functions-
syntax.html` (2) and both `timing-functions-syntax-valid.html`/
`linear-timing-functions-syntax.html` combined with the `steps(2, jump-end)`
instances already counted above. Root cause is the same one line for all
four shapes: `_lumen_make_style`'s `setProperty` (`dom.rs:4264-4302`)
stringifies and stores the raw input, never routing it through
`css-parser`'s real `<easing-function>` grammar (which already exists and is
correct for parsing — confirmed via `style.rs`'s passing
`animation_timing_functions` unit tests covering `TimingFunction::Steps`/
`Linear`/`CubicBezier` — but has no corresponding canonical-serializer
counterpart anywhere in the layout crate, `grep -n "impl.*Display.*for
TimingFunction\|fn.*timing.*to_css"` returns zero hits, so even a correctly
parsed value has nowhere to be re-serialized from). 40 subtests / 4 files
this slice. `.ini` under `tests/wpt/metadata/css/css-easing/` for all
4 files, `expected: FAIL` per subtest.

## Срез 21 (`css/css-link-params`+`css-forced-color-adjust`+`css-size-adjust`+`css-env`+`css-overscroll-behavior`, 2026-08-03)

9 files, 52 subtests, both established shapes at once and no new ones —
**rejection (shape "should not set the property value")**:
`css-link-params/link-parameters-invalid.html` (12, malformed `param(...)`
accepted verbatim), `css-forced-color-adjust/parsing/forced-color-adjust-invalid.html`
(6), `css-size-adjust/parsing/text-size-adjust-invalid.html` (4),
`css-env/env-parsing.html` (5, malformed `env(name (), )` accepted),
`css-env/indexed-env.tentative.html` (4, `env(test1 test2, green)`/`env(test
-1, green)` — env()'s *indexed* second-argument grammar not validated,
same underlying line as the rest), `css-overscroll-behavior/parsing/overscroll-behavior-invalid.html`
(15, all four of `overscroll-behavior`/`-x`/`-y` plus the newly-unrecognized
`-block`/`-inline` — see [BUG-516](BUG-516-OPEN.md) — accept `"normal"`/`"0"`/
space-repeated keyword lists that must be rejected). **Canonicalization**:
`css-size-adjust/parsing/text-size-adjust-valid.html` (1, `calc(10% + 5%)`
not simplified to `calc(15%)`), `css-overscroll-behavior/parsing/overscroll-behavior-valid.html`
(4, `"contain contain"`/`"none none"`/`"auto auto"`/`"chain chain"` not
collapsed to the single-keyword canonical form). One instance is worth
flagging separately: `css-env/seralization-round-tripping.tentative.html`
(1) sets a *valid* `env(test)`, then calls `setProperty(..., "env()")` (an
explicitly-invalid empty-argument form per spec) expecting the setter to
reject it and keep the old value — instead the invalid call succeeds and
clobbers the valid one, the same rejection-shape bug but observed through a
round-trip assertion rather than a direct empty-string check. `.ini` under
each category's own `tests/wpt/metadata/css/<category>/` for all 9 files,
`expected: FAIL` per subtest.

## Срез 22 (`css/css-rhythm`+`css/css-mixins`, 2026-08-03)

9 files, ~55 subtests, both established shapes, no new ones — driven by two
different underlying "property doesn't exist" bugs
([BUG-517](BUG-517-OPEN.md) for `block-step*`, [BUG-518](BUG-518-OPEN.md)
for `@mixin`) rather than a partial-validation gap this time: since the
property/at-rule isn't recognized at all, the generic passthrough is the
*only* code path reached. **Rejection**: `css-rhythm/parsing/block-step-
align-invalid.html` (12), `block-step-insert-invalid.html` (12),
`block-step-invalid.html` (7), `block-step-round-invalid.html` (16),
`block-step-size-invalid.html` (5), `css-mixins/functions/dashed-function-
parsing.html` (29, malformed `--func(...)` argument-list syntax accepted
verbatim), `dashed-function-named-arg.tentative.html` (6, malformed named-
argument syntax `--func(--myident:)` accepted). **Canonicalization**:
`css-rhythm/parsing/block-step-valid.html` (23 — every valid `block-step`
shorthand serializes as the raw input instead of the spec's longhand
order), `block-step-size-valid.html` (1, `"0"` not canonicalized to
`"0px"`). `.ini` under `tests/wpt/metadata/css/css-rhythm/` and
`tests/wpt/metadata/css/css-mixins/` for all 9 files, `expected: FAIL` per
subtest.

## Срез 24 (`css/css-scroll-anchoring` + `css/css-content`, 2026-08-03)

Two more forms: `overflow-anchor`'s invalid-value rejection (2 subtests,
`css-scroll-anchoring/parsing/overflow-anchor-invalid.html`) and `content`'s
own invalid-value rejection (70 subtests, `content-invalid.html` — the
largest single-file contribution to this bug's invalid-value form to date)
plus a new canonicalization gap: `content: counter(name, DECIMAL)` (a valid
but non-lowercase `<counter-style>` keyword) round-trips verbatim instead of
serializing as `counter(name)` per the spec's implied-default omission rule
(8 subtests, `content-valid.html`). `.ini` under
`tests/wpt/metadata/css/css-scroll-anchoring/parsing/` and
`tests/wpt/metadata/css/css-content/parsing/`.

## Срез 24 (`css/compositing`, 2026-08-03)

Three more invalid-value files (7 subtests total):
`background-blend-mode-invalid.html` (2), `isolation-invalid.html` (2),
`mix-blend-mode-invalid.html` (3). `.ini` under
`tests/wpt/metadata/css/compositing/parsing/`.

## Срез 24 (`css/css-scrollbars`, 2026-08-03)

Two more files (16 subtests): `scrollbar-color-parsing.html` (5, plus a new
canonicalization gap -- `#FF0000` doesn't serialize to `rgb(255, 0, 0)`) and
`scrollbar-width-parsing.html` (11, invalid-value rejection only). `.ini`
under `tests/wpt/metadata/css/css-scrollbars/`.

## Срез 26 (`css/css-ruby` + `css/css-page`, 2026-08-03)

Same shape, two categories: `css-ruby/parsing/ruby-align-invalid.html` (4),
`ruby-merge-invalid.html` (6), `ruby-overhang-invalid.html` (11) +
`ruby-overhang-valid.html` (1, canonicalization: `"none"` should serialize
back as `"spaces"`), `ruby-position-invalid.html` (5) — none of the four
`ruby-*` longhands reject any string; `css-page/page-rule-declarations-002.html`
(4), `parsing/page-invalid.html` (5), `parsing/size-invalid.html` (14),
`parsing/page-orientation-invalid.tentative.html` (4). `.ini` under
`tests/wpt/metadata/css/css-ruby/` and `tests/wpt/metadata/css/css-page/`.

## Срез 27 (`css/css-transitions`, 2026-08-03)

Six files, ~48 subtests, both established shapes plus a new alias-folding
instance: **rejection** — `parsing/transition-behavior.html` (10 of its 25
failing subtests — `allow-discrete`-related `transition`/`transition
-behavior` setter values accepted verbatim), `parsing/transition-delay
-invalid.html` (5), `parsing/transition-duration-invalid.html` (5),
`parsing/transition-property-valid.html` (1, a rejection case embedded in
an otherwise-valid file). **Canonicalization** — `parsing/transition-valid
.html` (3, shorthand component order not canonicalized: `"1s -3s cubic
-bezier(...) top"` should serialize as `"top 1s cubic-bezier(...) -3s"`);
`parsing/transition-timing-function-valid.html` (4, the same `step-start`/
`step-end`/`steps(N, end)`/`steps(N, jump-end)` keyword-to-function alias
gap already documented for `animation-timing-function` in срез 15 — same
missing serializer, different shorthand). `.ini` under
`tests/wpt/metadata/css/css-transitions/parsing/`.

## Срез 29 (`css/css-scroll-snap` + `css/css-animations`, 2026-08-03)

Same canonicalization gap, confirmed via `parsing/scroll-margin-valid.html`
(`test_valid_value("scroll-margin-top", "0", "0px")` — verbatim-stored `"0"`
never canonicalizes to `"0px"`, 18 subtests across the file's
`scroll-margin-*` longhands) and `parsing/animation-range-shorthand.html`/
`animation-range-{start,end}-computed.html` (`"normal"`/`"normal normal"`
not expanded into the two-longhand canonical form, 34 subtests). `.ini`
under `tests/wpt/metadata/css/css-scroll-snap/parsing/` and
`tests/wpt/metadata/css/css-animations/parsing/`.

## Срез 30 (`css/css-align` + `css/css-anchor-position` + `css/css-color` + others, 2026-08-03)

Largest extension yet — 99 files / ~2300 subtests, `css/css-align`
dominating with the clearest shape-2 (shorthand-not-expanded) evidence to
date: every `place-content`/`place-items`/`place-self` shorthand getter
returns `""` instead of the expected expanded longhand pair
(`align-content`+`justify-content`, etc — `place-content-shorthand-002.html`
alone: 216 subtests; `place-items-shorthand-002.html`: 374;
`place-self-shorthand-002.html`: 360), and every `parse-{align,justify}-
{content,items,self}-00{1..5}.html` file across `content-distribution/`,
`default-alignment/`, `self-alignment/` fails its "computed style is not
what is should"/"specified value is not what it should" assertions the
same way (shape 1 rejection + shape 3 canonicalization combined, ~700
subtests). `css/css-anchor-position` (3 files, `container-type`/`position-
try-fallbacks`/`position-visibility` parsing), `css/css-color` (`color()`/
`hsl()`/alpha-color valid-value canonicalization, several hundred subtests
across `color-valid-*.html`), and smaller shares in `css/css-contain`,
`css/css-shapes`, `css/css-tables`, `css/css-values`, `css/css-view-
transitions`, `css/filter-effects` round out the slice — same three shapes
throughout, no new ones. `.ini` under each category's own
`tests/wpt/metadata/css/<category>/`.

## Срез 31 (`css-fonts`/`css-transforms`/`css-text`/`css-flexbox`/`css-ui`, 2026-08-03)

Shape-3 (serialization not canonicalized) confirmed on 42 more files/~560
subtests: `css-fonts` (13 files/363 subtests — `font`/`font-family`/
`font-weight`/`font-stretch` shorthand+longhand round-tripping), `css-
transforms` (8/84 — `transform`/`scale`/`rotate`/`translate` value
serialization), `css-text` (11/78 — `text-decoration`/`letter-spacing`/
`white-space` shorthand), `css-flexbox` (5/27 — `flex` shorthand), `css-ui`
(5/10). Same mechanism as every prior slice (`_lumen_make_style` stores the
raw string instead of parsing+canonicalizing). `.ini` under each category's
own `tests/wpt/metadata/css/<category>/`.

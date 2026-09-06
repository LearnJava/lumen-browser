# BUG-518: CSS Mixins `@mixin`/`@apply`/`@contents` rules not implemented at all

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser (`grep -rn "\"mixin\"\|\"apply\"\|\"contents\"\|MixinRule\|
ApplyRule\|ContentsRule" crates/engine/css-parser/src/*.rs` — zero hits.
Contrast with `@function` (CSS-SPECS.md: 🟡, `FunctionRule`
parsed+stored+evaluated end-to-end — `grep -c "function_rules\|FunctionRule"
crates/engine/css-parser/src/parser.rs` → 26 hits) — this bug is the
module's *other* at-rule family, `@mixin`/`@apply`/`@contents`, which has
none of that.)
**Найден:** WPT-RUN-3 срез 22 (`ROADMAP.md`) — массовый прогон `css/css-mixins`

## Симптом

```
FAIL CSS Mixins: Basic test
  assert_equals: expected "rgb(0, 128, 0)" but got ""
FAIL @layer (statement) is invalid in @mixin
  CSSStyleSheet is not defined
```

## Механизм

`@mixin --name { ... }` (a named block of declarations/rules) and
`@apply --name(...)` (invoking one inside a style rule) are CSS Mixins
Module Level 1's other half alongside `@function` — entirely absent from
the parser's at-rule dispatch. Every test that applies a mixin and checks
the resulting computed style gets the unset initial value instead
(`expected "rgb(0, 128, 0)" but got ""` — the mixin's declarations never
reached the cascade at all, not a wrong-value bug). `@contents` (the
mixin-body placeholder for @apply's own nested block) is the same gap one
level down. Tests that probe the CSSOM surface for these rules
additionally hit the already-open [BUG-471](BUG-471-OPEN.md)
(`CSSStyleSheet`/`CSSRule` hierarchy missing) — not a separate cause, just
a second gap the same test trips over after the first.

## Масштаб находки

15 files / ~45 subtests, all under `css/css-mixins/mixins/`: `mixin-basic`,
`mixin-conditionals` (7), `mixin-cross-stylesheet`, `mixin-cycle.tentative`,
`mixin-declarations`, `mixin-from-import(-with-media-queries)`,
`mixin-locals` (6), `mixin-parameters` (18 — the largest single file),
`apply-top-level`, `apply-within-mixin`, `contents-rule` (6),
`contents-nested-declarations(-fallback)`, `mixin-shadow-dom`,
`mixin-layers` (4, additionally needs bare-id named access —
[BUG-384](BUG-384-FIXED.md) — since `e1`/`e2`/`e3`/`e4` are read as globals),
`mixin-cssom.tentative`/`mixin-invalidation.tentative` (CSSOM surface,
[BUG-471](BUG-471-OPEN.md)). Not filing the sibling `css-mixins/functions/`
subdirectory under this bug — those 20 files test `@function` itself
(partially implemented) and fail almost entirely on already-open
[BUG-471](BUG-471-OPEN.md)/[BUG-384](BUG-384-FIXED.md) or the documented
CSS-SPECS.md T3 deferred scope (`returns` typing, conditional group rules),
not on a missing `@mixin`/`@apply`/`@contents` construct.

## Что нужно

Parse `@mixin <dashed-ident> { <declaration-list> }` and `@apply
<dashed-ident>([<argument-list>])` (mirroring the already-built
`@function`/`FunctionRule` plumbing — cascade-time lookup by name, argument
substitution) plus `@contents` as the placeholder consumed at `@apply`
call sites. Re-run `run_report.py --all --root css/css-mixins --recursive`
afterward — the `mixins/` subtree's `assert_equals(..., "rgb(0, 128,
0)")`-style checks are the fast way to confirm the fix (a mixin either
applied its declarations or it didn't, no partial-credit ambiguity).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-mixins/mixins/` for all
15 files, `expected: FAIL` per subtest.

## Срез P3 2026-09-06 (часть 1)

Implemented `@mixin`/`@apply`/`@contents` mirroring `@function`'s existing
architecture (parse in css-parser, resolve at cascade time in layout).

**New types** (`crates/engine/css-parser/src/parser/at_rules.rs`):
`MixinRule` (name, `parameters: Vec<MixinParameter>`, `locals: Vec<Declaration>`,
`result: Option<Vec<MixinResultItem>>`), `MixinParameter` (name, raw
`type(<syntax>)` descriptor — stored, not validated — and default),
`MixinResultItem` (`Decl`/`Apply`/`Contents { fallback }`), `ApplyRule`
(name, raw args, optional block). `@mixin` dispatches through
`Parser::parse_at_rule` like every other top-level at-rule. `@apply` does
**not** — it's declaration-position, not a nested conditional-group rule,
so `parse_declaration_block_with_nesting`'s `Some('@')` branch special-cases
it: pushes a marker `Declaration` (`property == MIXIN_APPLY_MARKER == "@apply"`,
`value` = the raw source text after `apply`) at its exact position among
sibling declarations, instead of routing through `parse_nested_at_rule`/
`at_rules` the way `@media`/`@supports` do. This keeps normal cascade
source-order precedence without touching `Rule`'s `Vec<Declaration>` shape.
`Rule::style_css_text` (CSSOM `cssText`) filters the marker out so it can't
leak as a fake `"@apply: ...;"` property string.

**Cascade-time expansion** (`crates/engine/layout/src/style/substitute.rs`,
`expand_mixin_apply`/`expand_apply_rule`/`expand_mixin_result_items`,
hooked into `cascade.rs`'s per-declaration loop before the `attr()`/`var()`/
`@function` pipeline, gated on `!sheet.mixin_rules.is_empty()` like
`@function`'s own gate): positional arguments and their defaults resolve
against the **call site's** scope (confirmed against `mixin-parameters.html`
"Mixin arguments are resolved at call site, not at use" and
`mixin-locals.html` "Parameters do not resolve against locals"); the
mixin's own `--x:` locals resolve against its own scope (bound params +
earlier locals) regardless of whether they're declared before or after
`@result` in source order (`mixin-locals.html` "Locals after `@result` are
seen"); a nested `@apply` inside another mixin's `@result` recurses using
that enclosing mixin's scope as its own call-site scope (covers
`apply-within-mixin.html` and the `mixin-parameters.html` outer/inner-scope
chain tests); `@contents` splices the invoking `@apply`'s own `{ ... }`
block (resolved against the call site's scope) when given, or the mixin's
inline `@contents { fallback }` (resolved against the mixin's own scope)
otherwise. An omitted argument with no declared default is left unbound
rather than failing the whole call (`var()` inside `@result` supplies its
own fallback) — matches the dominant vendored-test shape but not every edge
the spec draws around an explicit `()`. Each `@result` item is resolved
independently (a bad `var()`/function inside one declaration drops only
that declaration, not the rest of the batch), matching ordinary CSS
declaration-block semantics and `cascade.rs`'s own per-declaration
`continue` elsewhere. A later `@mixin` redefinition of the same name wins
at lookup (`.rev().find()`), matching `mixin-basic.html`'s explicit test of
that — deliberately not mirroring `@function`'s own lookup (`.find()`,
first-registered wins), since no `@function` test exercises redefinition
either way and `@mixin`'s vendored test is explicit.

**Deliberately out of scope** (architectural, not an oversight): a nested
style rule inside `@result` (`mixin-basic.html`'s `.cls { color: green; }`,
`contents-rule.html`'s `&.a { @contents {...} }`) — a real implementation
must re-target that nested rule's selector against the `@apply` call site's
own selector (e.g. `div { @apply --m1; }` where `--m1`'s `@result` contains
`.cls {...}` must produce a new top-level rule `div .cls {...}`), which is
the same class of selector-rewriting CSS Nesting's `expand_nesting` already
does — but that happens at **parse time**, whereas mixin/function
resolution happens at **cascade time** (deliberately, to support forward
references and cross-stylesheet definitions, same as `@function`). The two
don't compose without a dedicated architectural pass; left for a follow-up
slice. Also deferred: `type(<syntax>)` parameter validation/coercion
(`mixin-parameters.html` m8/m9), `attr()` inside an `@result` declaration
(`mixin-parameters.html` m7), `@apply` argument-count strictness with an
explicit empty `()` against a no-default required parameter
(`mixin-parameters.html` m2c — ambiguous without a live browser/wptrunner
reference given this engine's existing `var()`-invalidity model already
treats "skip this declaration" rather than CSS's true "reset to initial
value at computed-value time" sitewide, not just for mixins), a
syntactically-invalid default value that's never even referenced
(`mixin-parameters.html` m16/m17 — requires rejecting a bare `!` outside
`!important` per CSS Syntax's `<declaration-value>` grammar, which nothing
in this parser enforces today), cross-stylesheet/`@import` mixin
visibility, shadow-DOM encapsulation, `@layer` interaction, CSSOM
reflection.

**Verification:** 19 new unit tests (13 in
`crates/engine/css-parser/src/parser/tests/nesting.rs` mirroring the
`@function` parser tests' style, structural assertions against `MixinRule`/
`ApplyRule` fields; 12 in `crates/engine/layout/src/style/tests/values.rs`
using the same `cascade_at()` full-document-cascade helper the `@function`
cascade tests use) — transcribing the clearly-unambiguous scenarios from
the vendored `mixin-basic.html`/`mixin-parameters.html`/`mixin-locals.html`/
`contents-rule.html` almost verbatim; all passed on the first run.
`cargo test -p lumen-css-parser -p lumen-layout --lib`: 382 + 3862 passed,
0 failed. `cargo clippy -p lumen-css-parser -p lumen-layout --all-targets --
-D warnings`: clean. No live WPT run (no `.venv` in this slot, the recurring
reason across this whole track's slices) — confidence rests on the unit
tests reproducing vendored scenarios directly, not on running the actual
`.html` files through `wptrunner`. `graphic_tests/dump_golden.py --build`
shows 4/12 mismatches (`samples/page.html`, `65-flex-align-content.html`,
both `--dump-layout`/`--dump-display-list`) — but byte-for-byte identical
(same rects down to the decimal) on a clean `main` checkout with this
branch's changes `git stash`-ed out and rebuilt, confirming pre-existing
drift unrelated to this change (same verification method as
[BUG-1008](BUG-1008-OPEN.md), which tracks the sibling `snapshot_cpu`
gate's version of the same recurring text/line-height drift). Consistent
with the change being gated on `mixin_rules` being non-empty — every
existing page (none use `@mixin`) is a provable no-op through this code
path.

**Expected effect on the vendored category once re-triaged**: most of
`mixin-parameters.html` (18 subtests) and all of `mixin-locals.html` (5)
should go green; `mixin-basic.html` and the `&`-nesting-dependent half of
`contents-rule.html` will not, per the scope note above. Status remains
`OPEN` — next slice is the nested-rule-inside-`@result` architecture.

## Срез P3 2026-09-06 (часть 2)

Implemented the nested-rule-inside-`@result` architecture the previous
slice deferred. The two mechanisms genuinely don't compose at the same
timing (CSS Nesting's `&`-combination happens at *parse* time against an
already-known enclosing selector; mixin/`@apply` resolution happens at
*cascade* time to support forward references) — resolved by keeping
`@apply`'s own flat-declaration splice exactly as it was (still per-element,
cascade-time, in `layout/style/substitute.rs`) and adding a **third**
timing, a stylesheet-level post-parse pass, purely for the nested-rule case.

**Parser** (`crates/engine/css-parser/src/parser/mixins.rs` — new file;
`at_rules.rs` was already at the 2000-line cap, so the whole `@mixin`/
`@apply`/`@contents` section — types, parsing, this slice's new pass — was
split out into its own module rather than grown further, SPLIT-CP1-style,
no behaviour change to anything moved verbatim): `MixinResultItem` gained a
`NestedRule { combinator: Option<Combinator>, selectors: Vec<ComplexSelector>,
body: Vec<MixinResultItem> }` variant. `parse_mixin_result_body` now parses
a nested-rule-start token (`&`, implicit-descendant `.`/`#`/`[`/`:`/`*`, or
an explicit relative combinator `>`/`+`/`~`) into one, mirroring
`parse_declaration_block_with_nesting`'s own grammar — but the selector is
stored **relative and unexpanded**: a `@mixin` block isn't attached to any
selector at definition time (unlike an ordinary nested style rule, which
always has a concrete enclosing rule at parse time), so there is nothing to
combine with yet. `body` recurses through the same grammar one level down
(`contents-rule.html`'s `&.a { @contents {...} }` — a nested rule containing
`@contents`, and in principle further nested rules).

**Stylesheet-level materialization** (same file, `collect_mixin_nested_rules`
+ its recursive helper `collect_nested_rule`, called once from
`lumen_css_parser::parse` after the whole sheet — including every `@mixin`,
even a forward-referenced one — is known): for every `@apply` marker found
in any top-level `Rule`, resolves the mixin and walks its `@result` for
`NestedRule` items, combining each one's stored relative selector with the
*calling* rule's own selector via the parser's existing `expand_nesting`
(the exact function CSS Nesting itself uses for `&`), and pushes the result
as a brand-new standalone top-level `Rule`. This new rule needs no special
handling anywhere else: `RuleIndex`/`CascadeIndex`/`compute_style` already
treat every entry of `sheet.rules` uniformly, so it gets matched against
whichever element(s) its (possibly descendant/sibling-combined) selector
actually targets, completely independent of the element `@apply` was
written on. `layout/style/substitute.rs`'s `expand_mixin_result_items` (the
flat per-element path) gained a matching `NestedRule { .. } => {}` arm —
correct no-op, since this new pass is what actually handles it.

**Why this needed no cross-crate plumbing**: unlike the flat-declaration
path, a nested rule's own `Decl`/`@contents` values are copied *literally*,
not resolved against the mixin's bound-parameter/locals scope
(`expand_vars`/`expand_custom_functions`, which live in the layout crate
and need `em_basis`/`viewport` for unit resolution) — they're left as
ordinary, unresolved declaration text on the new standalone `Rule`, and the
*ordinary* per-element cascade (already running on every rule in the sheet)
resolves any `var()`/`attr()` they contain exactly as if an author had
written that rule directly. This is exactly right when nothing inside them
depends on the mixin's own scope (every vendored test — `mixin-basic.html`'s
`.cls { color: green; }` and `contents-rule.html`'s `&.a`/`&.c { @contents
{...} }` are all literal), and a documented approximation otherwise (a
`var()` inside such a declaration will resolve against whatever element the
*combined* selector ends up matching, not against the `@apply` call site's
own scope — only observably different when the two select different
elements, which no vendored test exercises). It also means the whole
mechanism lives entirely in `lumen-css-parser`, with zero new `Stylesheet`
fields and zero changes to `rule_index.rs`/`cascade_index.rs`/`cascade.rs`'s
existing global rule-numbering/specificity machinery.

**Scope limits, deliberate** (documented in `collect_mixin_nested_rules`'s
and `MixinResultItem::NestedRule`'s doc comments): only the sheet's flat
top-level `rules` are scanned — a call site inside `@media`/`@supports`/
`@layer`/`@scope`/a shadow-tree sheet is out of scope (each keeps its own
separate `Vec<Rule>`, unlike CSS Nesting's own expansion, which flattens
directly into whichever block it found itself in); a nested `@apply` found
inside a nested rule's own body is silently dropped (no vendored test needs
it); `type(<syntax>)` validation, `attr()` inside `@result`, cross-stylesheet
visibility, shadow-DOM, `@layer` interaction and CSSOM reflection remain the
same pre-existing gaps the flat path already documented.

**Revision-cache gate caveat**: `revision.rs`'s workspace-wide
`every_stylesheet_mutation_in_the_workspace_announces_itself` test scans
every file mentioning `Stylesheet` for an in-place `.rules.push/extend/...`
not immediately followed by `mark_mutated()`, and only exempts `parser.rs`
itself (the one sanctioned place `Stylesheet::merge_from` lives). This is
why `collect_mixin_nested_rules` **returns** the extra rules instead of
appending them itself — `pub fn parse` (in `parser.rs`, exempt) does the
actual `sheet.rules.extend(...)`, before the sheet's revision is ever
observed by anything, so the invariant the gate protects (a revision-keyed
cache never seeing rules it wasn't built from) still holds.

**Verification**: 10 new unit tests in `css-parser`'s
`parser/tests/nesting.rs` — 4 covering the parser (relative selector +
combinator detection for compound-join/implicit-descendant/bare-`&`, and a
nested rule containing `@contents`), 6 covering the stylesheet-level
materialization, transcribing `mixin-basic.html`'s and `contents-rule.html`'s
m3/m4 scenarios structurally (asserting the produced `Rule`'s selector and
declarations) rather than through a live DOM/cascade — no v8/JS harness
needed since the transformation is pure `Stylesheet` → `Stylesheet`.
`cargo test -p lumen-css-parser --lib`: 392/392. `cargo test -p lumen-layout
--lib`: 3862/3862 (unchanged count — no new layout-side tests needed, the
one new match arm is a no-op by construction). `cargo clippy -p
lumen-css-parser -p lumen-layout --all-targets -- -D warnings`: clean.
`scripts/scoped-test.sh` (base = `main`'s merge-base): green except two
pre-existing, unrelated failures already tracked elsewhere —
`cases::snapshot_cpu` (BUG-1008, the recurring CPU-snapshot golden drift)
and `dom::tests::v8_perf_typedom_node::native_binding_panic_does_not_abort_
process` (BUG-997, a stale/flaky native-binding test) — reconfirmed
unrelated by running each crate's tests standalone. No live WPT run (same
recurring reason as every slice on this track — no `.venv` in this slot).

**Expected effect on the vendored category once re-triaged**:
`mixin-basic.html` (the category's single demonstration file) and the
`&`-nesting half of `contents-rule.html` (`m3`/`m4`, "Block in @apply
overrides fallback" / "Fallback is used if @apply has no block") should now
go green, on top of what срез 1 already fixed. Remaining known gap in this
file: `contents-rule.html`'s non-`&` cases (`m1`/`m2`/`m6`/`m7`, bare
`@contents` with no nested rule at all) were already covered by срез 1's
flat path. Status remains `OPEN` — no vendored test in this category is
known to need anything from the "deliberate scope limits" list above, but
the category itself (`mixin-cross-stylesheet`, `mixin-shadow-dom`,
`mixin-layers`, `mixin-cssom.tentative`) still needs those follow-ups.

## Срез P3 2026-09-06 (часть 3)

Implemented `@layer` interaction for `@mixin`/`@apply` name resolution
(`mixin-layers.html`, one of the four follow-ups срез 2 left open).

**`MixinRule::layer`** (`crates/engine/css-parser/src/parser/mixins.rs`): a
new `Option<String>` field, `None` for a top-level `@mixin` and `Some(name)`
for one declared directly inside `@layer name { ... }` (or the synthesized
`__anon_N__` for an anonymous block) — stamped by the parser's
`AtRuleOutcome::LayerBlock` handler in `parser.rs` once the enclosing
layer's own name is resolved, mirroring how every ordinary declaration
already carries its layer through `LayerRule`. `MixinRule::layer_priority`
mirrors `cascade.rs`'s existing `layer_idx`/`layer_pri` convention for plain
declarations (CSS Cascade L5 §6.4.5): unlayered beats every layer
(`layer_order.len()`, i.e. one past the last valid index), a layer declared
later in `sheet.layer_order` beats one declared earlier
(`layer_order.iter().position()`), and a name absent from `layer_order`
(shouldn't happen — a layer's name is always pushed to `layer_order` at the
point it's first seen) falls back to `0` rather than panicking.

**Parser plumbing** (`at_rules.rs`/`parser.rs`): `AtRuleOutcome::LayerBlock`
gained a `mixin_rules: Vec<MixinRule>` field. The block-form `@layer`
parser (`at_rules.rs`, previously a blanket `skip_at_rule()` for every
nested at-rule) now special-cases `@mixin` — parses it via the normal
`parse_at_rule` dispatch and collects it — while leaving every other nested
at-rule kind unsupported exactly as before (unrelated pre-existing gap, not
touched). Both of `parser.rs`'s `LayerBlock` match arms (top-level and
inside a conditional group rule) stamp the resolved layer name onto each
collected mixin and fold it into the sheet's flat `mixin_rules`, the same
place a top-level `@mixin` lands — so every existing consumer
(`collect_mixin_nested_rules`, `cascade.rs`'s expansion hook) sees it
without change. `@layer` nested inside an ordinary style rule's own body
(CSS Nesting, `parse_nested_group_body`) does not get this treatment — a
`@mixin` there remains unsupported, no vendored test needs it.

**Lookup call sites updated** (both already existed, both changed from
`.rev().find()`/`.find()` name-only lookup to `layer_priority`-ranked,
`i`-tiebroken lookup): `layout/style/substitute.rs`'s `expand_apply_rule`
(flat per-element `@apply` splice, threaded an extra `layer_order: &[String]`
parameter through its own recursion and `expand_mixin_result_items`) and
`css-parser/parser/mixins.rs`'s `collect_mixin_nested_rules` (stylesheet-level
nested-rule materialization, срез 2's mechanism). Both now do
`mixins.iter().enumerate().filter(name match).max_by_key((layer_priority, i))`
— layer priority is the primary key, source-order index only breaks a tie
(same layer, or two unlayered mixins), matching `mixin-basic.html`'s
"later redefinition wins" for the no-layers case while adding the
layer-aware ranking `mixin-layers.html` needs on top.

**Verification**: 6 new unit tests in `css-parser`'s `parser/tests/nesting.rs`
(`.layer` stamping for top-level/named/anonymous, `layer_priority` unlayered-
beats-everything and later-named-layer-beats-earlier, and one exercising the
nested-rule path's layer ranking) plus 4 in `layout/style/tests/cascade.rs`
transcribing `mixin-layers.html`'s four subtests verbatim (named layer,
anonymous layer, stronger-layer-wins against reversed source order,
stronger-layer-wins with source order matching layer order) using the same
`cascade_at()` helper the file's existing plain-declaration `@layer` tests
(`at_layer_*`) use. `cargo test -p lumen-css-parser --lib`: 398/398 (was
392, +6). `cargo test -p lumen-layout --lib`: 3866/3866 (was 3862, +4).
`cargo clippy -p lumen-css-parser -p lumen-layout --all-targets -- -D
warnings`: clean.
`graphic_tests/dump_golden.py --build`: 4/12 mismatches
(`samples/page.html`, `65-flex-align-content.html`, both dump kinds) —
identical file set and rects to срез 1's own verification, i.e. the
pre-existing [BUG-1008](BUG-1008-OPEN.md)-class drift, unrelated (this
change is gated on `mixin_rules` being non-empty, and neither golden page
uses `@mixin`). No live WPT run — same recurring reason as every slice on
this track (no `.venv` in this slot).

**Expected effect on the vendored category once re-triaged**: all 4
subtests of `mixin-layers.html` should go green — its other blocker,
[BUG-384](BUG-384-FIXED.md) (bare-id named access for `e1`/`e2`/`e3`/`e4`),
is already fixed. Status remains `OPEN` — `mixin-cross-stylesheet`,
`mixin-shadow-dom` and the CSSOM-gated `mixin-cssom.tentative`/
`mixin-invalidation.tentative` are still open follow-ups.

## Срез P3 2026-09-06 (часть 4)

Investigated `mixin-cross-stylesheet.html` and `mixin-shadow-dom.html`, the
next two named follow-ups. **No code change for the cross-stylesheet case —
it already worked**, and the shadow-DOM case turned out to be blocked by a
pre-existing, mixin-unrelated engine gap, filed separately.

**`mixin-cross-stylesheet.html`**: the shell (`build_page_cascade`,
`crates/shell/src/page_pipeline.rs`) concatenates every inline `<style>`
element's text, in document order, into one string (`extract_style_blocks` +
`inline_css_imports`, which also splices in `@import`ed text the same way)
before a *single* `lumen_css_parser::parse` call — by the time `@mixin`/
`@apply` see any of it, there is only one flat `Stylesheet` with one
`mixin_rules` list, not "two stylesheets" to scope by. A forward reference
across that boundary is exactly the same shape as a forward reference within
one `<style>` block, which срез 1's cascade-time (not parse-time) resolution
already supports by design ("to support forward references and
cross-stylesheet definitions, same as `@function`" — срез 1's own note).
`document.adoptedStyleSheets`/constructed stylesheets reach the same place
through `Stylesheet::merge_from` (`crates/engine/css-parser/src/parser.rs`),
which already extends `mixin_rules` along with every other field. Verified
directly (not just read from code) with two new permanent unit tests in
`crates/engine/layout/src/style/tests/values.rs`
(`css_mixin_visible_across_concatenated_style_elements`,
`css_mixin_visible_across_at_import`) transcribing `mixin-cross-stylesheet.html`
and `mixin-from-import.html` respectively — both pass against today's code,
unmodified. `CSS-SPECS.md` updated to record this as closed, not "deferred".

**`mixin-shadow-dom.html`**: NOT fixed, and not attempted — three of its four
subtests (`#e1`/`#e2`/`#e3`, all plain `id` selectors written inside a shadow
root's own `<style>`, targeting elements inside that same shadow tree) hit a
gap one layer below mixins entirely: Lumen's cascade never matches a regular
(non-`:host`/`::slotted`) selector from a shadow tree's own stylesheet
against that tree's own descendants at all — confirmed with a throwaway
probe reproducing the exact shape (`<template shadowrootmode="open"><style>
#e1{color:red}</style><div id="e1">`) through the real `box_tree::layout`
entry point; no box anywhere in the resulting tree carries the red color.
`@apply` inside such a rule can only be as visible as the rule itself, and
the rule itself never reaches `#e1`/`#e2`/`#e3` — a mixin-specific fix here
would be attacking a symptom. Filed as [BUG-1009](BUG-1009-FIXED.md)
(layout-crate cascade gap, no connection to `@mixin`/`@apply`) — fixed
2026-09-06, after this slice. The file's 4th subtest (`#e4`, "style outside
shadow DOM should NOT have access to inside mixins") is a light-DOM rule with
a light-DOM `@apply` and does not depend on BUG-1009 either way — not
separately verified this slice, deferred to whoever revisits this file (needs
a real shadow-tree end-to-end harness to check without also needing the other
three subtests to already be green — now unblocked, but not re-verified
here).

**Verification**: `cargo test -p lumen-layout --lib`: 3869/3869 (+2 for the
two new permanent tests; net +2 not +something-more because the two throwaway
probes used to investigate this were removed before commit).
`cargo clippy -p lumen-layout --all-targets -- -D warnings`: clean. No parser
or cascade code touched, so no new `dump_golden.py`/graphic-test surface.

Status remains `OPEN` — `mixin-shadow-dom.html` was blocked on
[BUG-1009](BUG-1009-FIXED.md) (not further actionable from this bug), fixed
2026-09-06 (unblocked, not re-verified by this slice), and the CSSOM-gated
`mixin-cssom.tentative`/`mixin-invalidation.tentative` remain
blocked on [BUG-471](BUG-471-OPEN.md) as already documented.

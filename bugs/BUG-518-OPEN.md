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

## Срез P3 2026-09-06 (часть 5)

[BUG-1009](BUG-1009-FIXED.md) landed after срез 4, unblocking `mixin-shadow-dom.html`'s
plain-selector subtests. Re-verifying found the selector fix alone was not
enough — a **second, independent gap** in `@apply`'s own mixin-name lookup,
found and fixed here.

**The gap**: `compute_style`'s (`crates/engine/layout/src/style/cascade.rs`)
`MIXIN_APPLY_MARKER` handling always resolved a `@apply`'s mixin name against
`sheet.mixin_rules` — the single document-level `Stylesheet` — regardless of
which stylesheet the `@apply`-bearing declaration itself came from. BUG-1009
gave a shadow-interior element's own regular selectors a *third* matching path
(case (c), `interior_shadow`, matched against `SHADOW_SHEETS[host]`), but left
every declaration matched through cases (a)/(b)/(c) pointing at the same
`sheet.mixin_rules` for mixin lookups. A `@mixin` declared inside that SAME
shadow tree's own `<style>` (`mixin-shadow-dom.html` #e2's `--m1`) lives in
`SHADOW_SHEETS[host].mixin_rules`, which `sheet.mixin_rules` never contains —
so `@apply --m1` inside the shadow tree silently found nothing and left `#e2`
its pre-`@apply` `color: red`, failing the "Style in shadow DOM should have
access to inside mixins" assertion (expected green).

**Fix**: `matched` (the `Vec` of cascade-sorted candidate declarations)
gained an 8th tuple field, `Option<&Stylesheet>` — `None` for anything
matched from the document `sheet` (including @layer/@media/@supports/@scope/
inline, which all read from `sheet` too), `Some(shadow)` for a declaration
matched through case (a) `own_shadow`, (b) `host_shadow`, or (c)
`interior_shadow`. Every one of the ~9 push sites got the new field (a new
`type MatchedDecl<'a>` alias keeps the `Vec` declaration itself readable and
satisfies `clippy::type_complexity`, which flagged the bare 8-tuple). At the
`MIXIN_APPLY_MARKER` site, a `@apply` whose declaration carries `Some(shadow)`
now resolves against `shadow.mixin_rules.iter().chain(sheet.mixin_rules.iter())`
— the shadow's own mixins checked first (so a same-named shadow-local mixin
wins over a same-named outer one, matching how a shadow tree's own plain
declarations already shadow inherited ones), falling back to the document's
own mixins afterward. That fallback is what keeps the *other* direction
working: `mixin-shadow-dom.html` #e1 (`--exists-only-outside-shadow`, declared
in the document's own `<style>`, `@apply`'d from inside the shadow tree) needs
exactly this — its mixin was never in `SHADOW_SHEETS[host].mixin_rules` to
begin with, so the lookup falls through to `sheet.mixin_rules`, unchanged from
before this slice. `#e4` ("outside should NOT see inside mixin") is untouched
by this fix by construction: its `@apply` is matched through the *document*
`sheet` (no shadow origin), so `shadow_origin` is `None` and it keeps using
plain `sheet.mixin_rules`, which never contained the shadow-only `--in-shadow`
to begin with — isolation there was already correct, just never covered by a
permanent test.

**Verification**: 3 new permanent unit tests in
`crates/engine/layout/src/style/tests/shadow_dom_selectors.rs` transcribing
`mixin-shadow-dom.html`'s #e1/#e2/#e4 scenarios directly through
`compute_style` (`apply_inside_shadow_sees_outside_mixin`,
`apply_inside_shadow_sees_own_shadow_mixin`,
`apply_outside_shadow_does_not_see_inside_mixin`) — confirmed the middle one
(#e2, the actual gap) fails red (`(255, 0, 0)` instead of `(0, 128, 0)`)
against the old lookup-always-`sheet.mixin_rules` code, by temporarily
reverting just the `mixins` selection to `&sheet.mixin_rules` and rerunning;
restored before commit. `cargo test -p lumen-layout --lib`: 3874/3874 (was
3871 before this slice, +3, no other test's outcome changed). `cargo clippy -p
lumen-layout -p lumen-css-parser --all-targets -- -D warnings`: clean (the
type-complexity lint the new 8-tuple triggered is resolved by the `MatchedDecl`
alias; the crate's 3 pre-existing `invariants.rs` dead-code errors under `-D
warnings` are unchanged from a clean `main` checkout, confirmed by `git stash`
A/B). `graphic_tests/dump_golden.py --build`: same 4/12 mismatches
(`samples/page.html`, `65-flex-align-content.html`) as every prior slice on
this track — confirmed identical on a clean `main` checkout via `git stash`
A/B, the pre-existing [BUG-1008](BUG-1008-FIXED.md)-class line-height drift,
unrelated (this change is gated on a declaration actually carrying the
`MIXIN_APPLY_MARKER` from inside a shadow tree; neither golden page uses
Shadow DOM mixins). `cargo test -p lumen-driver --test all` /`-p lumen-js --lib
--features v8-backend` each show exactly one pre-existing failure
(`cases::snapshot_cpu::cpu_snapshots_match_references` — BUG-1008;
`native_binding_panic_does_not_abort_process` — BUG-997), matching every
prior slice's documented baseline exactly; full `scripts/scoped-test.sh` was
not run to completion (hangs on the unrelated `lumen-network` gate, BUG-805 —
same recurring reason as every other slice on this track), touched crates
verified standalone instead. `tests/wpt/metadata/css/css-mixins/mixins/
mixin-shadow-dom.html.ini` updated — `expected: FAIL` removed from #e1/#e2/#e4
(all three transcribed above pass through the real cascade now), kept only
for "Style in shadow DOM should have access to mixins from adopted
stylesheets" (#e3), which needs `shadowRoot.adoptedStyleSheets` to feed a
shadow-scoped cascade at all — an unrelated, pre-existing, documented gap
(`web_api_shim_mid.js`: "Lumen has no shadow-scoped cascade at all"). No live
WPT run — same recurring reason as every slice on this track (no `.venv` in
this slot).

Status remains `OPEN` — `mixin-cross-stylesheet.html` was already closed
(срез 4), `mixin-shadow-dom.html` is now fully closed except its
adopted-stylesheets subtest (separate gap, not mixin-specific), and the
CSSOM-gated `mixin-cssom.tentative`/`mixin-invalidation.tentative` remain
blocked on [BUG-471](BUG-471-OPEN.md) as already documented — that CSSOM gap
is the entire remainder of this bug's original 15-file/~45-subtest scope.

## Срез P3 2026-09-07 (срез 6)

Re-investigated the "blocked on BUG-471" note above: BUG-471's own record
shows its read half (`document.styleSheets`/`.sheet`/`cssRules`/rule
classes) closed 2026-09-03 — only the write half (`insertRule`/`deleteRule`/
`new CSSStyleSheet()`) is still open, tracked separately as
[BUG-897](BUG-897-OPEN.md)/CSSOM-5. `mixin-cssom.tentative.html`'s 6
subtests split cleanly along that line: 5 are pure `cssText` reads, only
the 6th (`@apply` illegal at top level) needs `insertRule` — and even that
needs it on an **owned** sheet specifically, which doesn't exist yet (only
a *constructed* sheet has `insertRule`/`deleteRule`, CSSOM-5 срез 3). So the
5 read-only subtests were reachable without touching BUG-897 at all — closed
this slice; the 6th, and all of `mixin-invalidation.tentative.html` (needs
both `insertRule` on an owned sheet AND a live per-declaration `.style`
setter that doesn't exist for *any* CSSOM rule kind today, a gap broader
than BUG-897's own documented scope), stay open follow-ups.

**What was missing**: `@mixin`/`@apply`/`@contents`/`@result` never reached
`Stylesheet::cssom_rules()` at all — `CssomRuleRef` only had `Style`/`Media`
variants, and `Rule::style_css_text` actively *filtered out* the `@apply`
marker declaration so it couldn't leak into an ordinary rule's `cssText`
(built for срез 1's "keep `@apply` invisible to unrelated code" goal, the
opposite of what this slice needs).

**Fix** (`crates/engine/css-parser/src/parser.rs`/`parser/mixins.rs`):
`CssomRuleRef`/`TopLevelRuleKind` gained a `Mixin` variant — `TopLevelRuleKind::
Mixin` carries its own index into `Stylesheet::mixin_rules` (not a position
count like `Style`/`Media` use) because that vec also receives untagged
pushes from `@layer`-nested `@mixin`s (invisible to `cssRules`, same as an
`@layer` block itself), so a same-kind position count would silently pick
the wrong mixin once a layered one sits between two top-level ones in source
order — stamped once, at push time, in `parse`'s single top-level dispatch
site. `delete_rule`'s forced `Mixin` arm removes from `mixin_rules` and
renumbers every later `Mixin` tag's embedded index (removal shifts them all
down one slot) — real correctness, not just the mechanical exhaustive-match
arm the compiler forces; `insert_rule` needed no change at all (a reparsed
`@mixin`/`@apply` snippet already fails its existing "no stray content
beyond the one target rule kind" check, since `mixin_rules` isn't cleared
before that comparison — confirmed by a passing test, not just reasoned
about).

New serialization methods — `MixinRule::css_text`, `MixinResultItem::
css_text`, `ApplyRule::css_text`, `MixinParameter::css_text`, plus a shared
`render_container` helper and `Rule::css_text` (parser.rs, supersedes the
JS shim's own `selectorText + ' { ' + styleCssText + ' }'` string-building
for an ordinary rule too, since it can no longer stay a single space-joined
line once a rule's declarations carry `@apply`) — reproduce the exact
one-child-per-line, two-space-indent, "only a child's own first line gets
indented" format `mixin-cssom.tentative.html`'s `assert_equals` strings
expect. That algorithm was decoded by hand-tracing every non-throwing
subtest's expected string against candidate implementations before writing
the recursive renderer, not guessed then patched to fit; `@mixin`/`@result`
always render multi-line even when their own body would otherwise qualify
as "flat" (a single plain declaration, `mixin-cssom.tentative.html` subtest
6), while an ordinary style rule or a `NestedRule` (`& { ... }`) still
collapse to one line when genuinely flat — that split, not "flat vs.
not-flat" alone, is what the vendored strings actually encode.

JS bridge (`crates/js/src/v8_runtime/install/stylesheets.rs`+
`constructed_stylesheets.rs` — the latter only for the two exhaustive-match
arms `CssomRuleRef` forces, no new write capability added there; `crates/js/
src/shim/web_api_shim_mid.js`): new `mixin_rule_json`/`_lumen_build_css_mixin_rule`
pair exposing a `CSSMixinRule` (`name`, `cssText`, `type` — `0`, following
every other newer CSSOM rule's legacy-attribute convention) alongside the
existing `CSSStyleRule`/`CSSMediaRule`; `_lumen_build_css_style_rule`'s
`cssText` getter now reads a Rust-precomputed field instead of reassembling
it in JS, since the new multi-line `@apply` format isn't a simple string
join.

**Verification**: 7 new css-parser unit tests
(`parser/tests/nesting.rs`) transcribing the 5 non-`insertRule`
`mixin-cssom.tentative.html` subtests' exact expected `cssText` strings
character-for-character, plus `cssom_rules`/`delete_rule` correctness tests
for the shared-vec/embedded-index scheme above — `cargo test -p
lumen-css-parser --lib`: 426/426. Then, since this slice touches the JS
bridge (unlike most of this track's prior slices, which stayed inside
css-parser/layout and could lean on unit tests alone), a genuine end-to-end
check through the real V8 shim: `crates/js/tests/cases/bug518_mixin_cssom.rs`
(new), using `V8JsRuntime::install_dom` + `update_stylesheet_nodes` directly
(bypassing `lumen_driver::InProcessSession`, whose test pipeline never calls
`update_stylesheet_nodes` at all — `document.styleSheets` is unreachable
through it today, confirmed by trying it first and getting `Cannot read
properties of null (reading 'cssRules')`; a pre-existing gap in that test
harness specifically, not in this feature, and not this slice's to fix) —
7/7 passed, including the same 5 subtests plus 2 extra sanity checks
(`instanceof CSSStyleRule` is `false`, `@layer`-scoped mixin excluded from
`cssRules`). `cargo clippy -p lumen-css-parser --all-targets -- -D warnings`:
clean. `cargo clippy -p lumen-js --features v8-backend --all-targets
--no-deps -- -D warnings`: pre-existing `chunks_exact_to_as_chunks` errors in
`worker.rs`/`canvas2d.rs`/`dom/tests/v8_core/selectors_canvas_window.rs` —
confirmed present on a clean `main` checkout too (system `rustc` 1.98.0 vs.
the pinned 1.97.0, the toolchain-mismatch class already on record — none of
those three files are touched by this slice). No paint/display-list surface
touched — no `dump_golden.py`/graphic-test drift possible.
`tests/wpt/metadata/css/css-mixins/mixins/mixin-cssom.tentative.html.ini`
updated — `expected: FAIL` removed from the 5 closed subtests, kept only for
"@apply is not legal at top level".

**Deliberately out of scope, disclosed rather than silently dropped**:
`.cssRules` navigation into `@result`'s own nested children (no subtest in
this slice's 5 needs it — reading a `@mixin`'s `cssText` whole is enough);
`insertRule`/`deleteRule` on an *owned* sheet (BUG-897/CSSOM-5); a live
per-declaration `.style` setter for any CSSOM rule kind (a gap
`mixin-invalidation.tentative.html` exposed but is not itself a
mixin-specific hole — worth its own bug once someone picks up CSSOM
mutation-triggers-relayout as a track).

Status remains `OPEN` — `mixin-cssom.tentative.html`'s 6th subtest and all
of `mixin-invalidation.tentative.html` are the entire remainder, both
gated on `insertRule` for an owned sheet (BUG-897/CSSOM-5) plus, for the
invalidation file specifically, the CSSOM `.style`-mutation gap noted above.

## Срез P3 2026-09-07 (срез 7, реклассификация)

Re-checked the "blocked on BUG-897" note above against
[ROADMAP.md](../ROADMAP.md)'s own CSSOM-5 record: that task closed
2026-09-06 (срез 3), but its own scope note explicitly excludes exactly
what this bug's remainder needs — `insertRule`/`deleteRule` were only ever
added for a *constructed* `CSSStyleSheet` (`crates/js/src/v8_runtime/
install/constructed_stylesheets.rs`, its own separate registry with no
owning DOM node); `document.styleSheets`'s own read-only node-backed
registry (`stylesheet_nodes`, CSSOM-1/2) still has no mutation path at all
— confirmed directly (`grep -rn "insert_rule\|insertRule"
crates/js/src/v8_runtime/install/stylesheets.rs` → zero hits, only a
doc-comment). The other half of the remainder — a live `.style` setter on
any CSSOM rule object (`CSSStyleRule`/the new `CSSMixinRule`/…) — has the
same zero-hits shape (`grep -rn "\"style\"" crates/js/src/v8_runtime/
install/stylesheets.rs` → one JSON-shape literal, no setter).

Both gaps satisfy [docs/probe-method.md §8](../docs/probe-method.md)'s two
reclassification conditions together: (1) the functionality is absent, not
broken — zero grep hits, no member of the mutation surface exists for an
owned sheet or for any rule's `.style`; (2) the size is a new registry plus
a setter across a family of rule types, not a one-line point fix — the
same shape as CSSOM-5 itself was before it earned its own task. Filed as
[CSSOM-8](../ROADMAP.md) (`planned`, refs this bug + BUG-897). `BUGS.md`'s
entry updated to `OPEN (ДОРАБОТКА → CSSOM-8)`. This does **not** touch the
substance already fixed in срезы 1-6 above — `@mixin`/`@apply`/`@contents`/
`@result` themselves are fully implemented and cascading correctly; only
the CSSOM-mutation-shaped tail (1 subtest of `mixin-cssom.tentative.html`
+ all of `mixin-invalidation.tentative.html`) moves out of P3's point-bug
queue and into CSSOM-8's backlog.

## Срез P3 2026-09-07 (срез 8)

Landed concurrently with (and committed just after) срез 7's reclassification
above — a parallel P3 session's worktree already had `insertRule`/
`deleteRule` for an owned sheet implemented and tested by the time срез 7's
reclassification merged to `main`. Rather than discard working, tested code
because the queue-management decision that filed CSSOM-8 preceded it by
minutes, this slice lands it and **narrows CSSOM-8's remaining scope** to just
the second half срез 7 named: a live per-declaration `.style` setter for
CSSOM rule objects.

Implemented `CSSStyleSheet.insertRule`/`.deleteRule` on an **owned**
(`document.styleSheets`) sheet — closing `mixin-cssom.tentative.html`'s 6th
subtest, the entire non-`.style`-setter half of what срез 7 filed under
CSSOM-8. Until this slice only a *constructed* sheet had the two methods
(CSSOM-5 срез 3).

**No parser/cascade change needed**: `Stylesheet::insert_rule`/`delete_rule`
(`crates/engine/css-parser/src/parser.rs`) are the same methods the
constructed-sheet path already calls — already unit-tested at the parser
level (`parser/tests/revision.rs`) and already correct for this slice's own
needs, including the exact case the 6th subtest needs: `insert_rule`
parsing `"@apply --m1();"` as standalone top-level text yields zero rules
(`@apply` is declaration-position only, never a top-level at-rule), so
`top_level_order.len() != 1` already turns into
`CssomRuleMutationError::Syntax` with no special-casing.

**JS bridge** (`crates/js/src/v8_runtime/install/stylesheets.rs`,
`crates/js/src/shim/web_api_shim_mid.js`): two new natives,
`_lumen_stylesheet_insert_rule`/`_lumen_stylesheet_delete_rule`, over the
owned-sheet registry `stylesheet_nodes` — `Arc::make_mut(&mut entry.sheet)`
(copy-on-write; a reader elsewhere holds `&Stylesheet` only for the
duration of one call, never a clone of the `Arc`) then delegates to
`Stylesheet::insert_rule`/`delete_rule`, same sentinel-return convention
already used for the constructed-sheet twin. `_lumen_make_css_style_sheet`
(the owned-sheet JS wrapper) gained `insertRule`/`deleteRule` methods —
copies of the constructed-sheet wrapper's own methods (kept separate rather
than factored out, since the two wrappers already address distinct `idx`
spaces through distinct native function names).

**Deliberately not wired into the page cascade**: `stylesheet_nodes` is
rebuilt wholesale from each `<style>`/`<link>` node's own DOM text on every
relayout (`crates/shell/src/stylesheets.rs::build_stylesheet_node_registry`),
so an inserted/deleted rule is visible to further CSSOM reads only until the
next relayout silently discards it — the same class of gap CSSOM-5 срез 1
documented for constructed sheets before срез 2 wired `adoptedStyleSheets`
into the cascade. No vendored test in this bug's scope needs the layout
effect, only the correct `cssRules`/exception behaviour.

**Verification**: 4 new end-to-end tests in
`crates/js/tests/cases/bug518_mixin_cssom.rs` through the real V8 shim
(`insert_rule_and_delete_rule_mutate_the_owned_sheet`,
`insert_rule_at_top_level_apply_throws_syntax_error` — transcribes the 6th
subtest verbatim, plus both `IndexSizeError` paths) — 11/11 in that file,
`cargo test -p lumen-js --features v8-backend --test all`: 94/94. `cargo
test -p lumen-css-parser --lib`: 426/426 (unchanged — `insert_rule`/
`delete_rule` themselves were not touched). `cargo clippy -p lumen-js
--features v8-backend -p lumen-css-parser --all-targets -- -D warnings`:
clean. No parser, cascade or paint code touched — no `dump_golden.py`
surface. No live WPT run (same recurring reason as every slice on this
track — no `.venv` in this slot).
`tests/wpt/metadata/css/css-mixins/mixins/mixin-cssom.tentative.html.ini`
removed — all 6 subtests of the file now pass (matching the precedent of
other bugs whose fix closed a WPT file entirely, e.g. BUG-512).

Status remains `OPEN (ДОРАБОТКА → CSSOM-8)`, per срез 7's classification
above — this slice does not undo that decision, it shrinks what CSSOM-8
still owes: `mixin-invalidation.tentative.html` (the entire remaining scope
of this bug) needs a live per-declaration `.style` setter for CSSOM rule
objects, not `insertRule`/`deleteRule` — that half is done as of this slice.
`ROADMAP.md`'s CSSOM-8 entry should be re-scoped to drop the
`insertRule`/`deleteRule`-on-owned-sheet line item accordingly.

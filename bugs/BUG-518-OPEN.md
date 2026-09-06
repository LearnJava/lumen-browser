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

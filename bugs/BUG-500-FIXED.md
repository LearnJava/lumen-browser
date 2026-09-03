# BUG-500: `ident()` (CSS Values and Units Level 5, draft) entirely unimplemented

**Статус:** FIXED 2026-09-03
**Дата:** 2026-08-02
**Компонент:** css-parser / layout (`crates/engine/css-parser/src/parser.rs`,
`crates/engine/layout/src/style.rs::expand_vars`)
**Найден:** WPT-RUN-3 срез 10 (`ROADMAP.md`) — массовый прогон `css/css-variables`,
`var-ident-function.html`

## Механизм

`ident()` (<https://drafts.csswg.org/css-values-5/#ident>) is a draft
function that builds a dashed-ident (usable as a custom-property name inside
`var()`) from an arbitrary string, e.g. `var(ident("--myprop" calc(3 *
sign(1em - 1px))), fallback)` should resolve the `ident(...)` argument to the
literal name `--myprop3` before doing the `var()` lookup. `grep -rn
"ident("` across `crates/engine/css-parser/src/` and
`crates/engine/layout/src/` returns zero hits — there is no parsing branch
for the function name at all, not a broken/partial one. `expand_vars`
(`style.rs:13862`) only recognises the literal `var(` token
(`find_var_open`, `style.rs:14214`) and has no concept of a nested
`ident(...)` argument needing evaluation first.

## Симптом

The one WPT file exercising this (`var-ident-function.html`) never reaches
this code path in the current run — its `#target` element is referenced by
the bare identifier `target` in the test script, which throws
`ReferenceError: target is not defined` before any assertion runs (a
different, unrelated bug — see [BUG-384](BUG-384-FIXED.md), which masks this
file's actual signal). This bug is filed from the source-grep evidence
alone, in the same spirit as [BUG-491](BUG-491-OPEN.md)'s `hairline`
keyword: a real, confirmable gap, but its practical blast radius through
`var-ident-function.html`'s 5 subtests is *unmeasured* until BUG-384 is
fixed and the file can actually reach the `ident()` parsing branch.

## Масштаб находки

1 file surveyed (`var-ident-function.html`, `css/css-variables`), currently
fully masked by BUG-384. `ident()` is a very new/draft addition (CSS Values
L5) with no other WPT coverage found in this slice.

## .ini

`var-ident-function.html`'s `.ini` cites [BUG-384](BUG-384-FIXED.md) as the
directly-observed cause (that's what the actual FAIL messages show) and
notes BUG-500 as the deeper, currently-invisible gap underneath it.

## Fix (2026-09-03)

Implemented in `crates/engine/layout/src/style/substitute.rs`, in the one
position the spec permits (CSS Values L5 §4.2): `expand_vars` now recognises
`var(ident(<string> <numeric-arg>*), fallback)` as the first argument to
`var()`. `eval_ident_call` concatenates the string literal with each numeric
argument rounded to the nearest integer — a bare `<number>`/`<integer>` token
or a `calc()`/math-function expression resolved through the existing
`crate::style::calc` engine against the calling element's `em_basis`/
`viewport` (the only two bases meaningful in this position; a percentage has
no defined basis here and makes the whole call invalid). A malformed
`ident()` call (missing/unquoted leading string, non-numeric trailing
argument) makes the containing `var()` invalid at computed-value time without
consulting the fallback — the same rule a syntactically broken `var()`
already follows.

Threading `em_basis`/`viewport` into `expand_vars` required adding both
parameters to `expand_vars_and_env`/`expand_custom_functions`/
`collect_custom_properties` and updating every call site: `style/apply.rs`,
`style/cascade.rs`, `style/container.rs`, `style/parse/font_size.rs`,
`lib.rs`, `crates/driver/src/session.rs`, and
`crates/shell/src/{hibernation,page_load,page_pipeline,relayout}.rs`. Pure
plumbing — behavior for any `var()` call without `ident()` is unchanged.

5 new unit tests in `style/tests/restyle.rs` mirror all 5 subtests of
`var-ident-function.html`, including the exact `calc(3 * sign(1em - 1px))`
example from the spec at a 16px em-basis.

**Not closed:** the file's 5th subtest (`var(ident("nodash"), inherit)`,
expecting a re-cascade through the parent) needs a separate mechanism — a
CSS-wide keyword surviving `var()` fallback substitution re-triggers normal
cascading — that is not specific to `ident()` and applies to any `var()`
fallback. This is the same residual [BUG-487](BUG-487-FIXED.md) already
left explicitly open ("var()/env()/attr()/if()-fallback ... out of scope for
that slice"), not new scope for this bug.

Gates: `cargo test -p lumen-layout` 3680/3680 (0 failed, 1 ignored, incl. the
5 new `expand_vars_var_ident_*` tests); `cargo clippy -p lumen-layout
-p lumen-driver -p lumen-shell --all-targets -- -D warnings` clean.

# BUG-500: `ident()` (CSS Values and Units Level 5, draft) entirely unimplemented

**Статус:** OPEN
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
different, unrelated bug — see [BUG-384](BUG-384-OPEN.md), which masks this
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

`var-ident-function.html`'s `.ini` cites [BUG-384](BUG-384-OPEN.md) as the
directly-observed cause (that's what the actual FAIL messages show) and
notes BUG-500 as the deeper, currently-invisible gap underneath it.

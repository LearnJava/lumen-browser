# BUG-519: `@function` bodies using `if()`/`attr(type())`/local vars/nested `@layer`/`@container`/shadow scoping hang instead of failing

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser/layout, exact trigger not isolated (see Механизм)
**Найден:** WPT-RUN-3 срез 22 (`ROADMAP.md`) — массовый прогон `css/css-mixins`

## Симптом

wptrunner's own per-test timeout (~10s) fires with **zero** subtests
registered, instead of the graceful `NNN is not defined`/`assert_equals`
`FAIL` every other `@function`-testing file in the same directory produces:

```
2:03.12 TEST_END: TIMEOUT, expected OK
Subtests passed 0/0
```

Confirmed as a real wall-clock hang, not a harness artifact: measured
directly off the raw wptrunner log, `function-layer.html` alone ran
`TEST_START`→`TEST_END` in **71 seconds** (0:51.94 → 2:03.12) — far past
the point a normally-failing test reports (typically well under 1s).

## Механизм

Not isolated to a single cause — each of the 9 affected files exercises a
different combination of `@function`-adjacent constructs that are all
individually documented as deferred/unimplemented in CSS-SPECS.md's T3 row
(`@function`: 🟡, "`returns` typing + conditional group rules deferred"):
`if()` (CSS Values L5 conditional function, inside a `result:` expression),
`attr(data-x type(*))`/`attr(data-x type(<length>))` (typed `attr()`),
locally-scoped `--x:`-style declarations inside a `@function` body read
back via `var()`, `@function` nested inside `@layer`/`@container`, and
`@function` invoked from inside a shadow tree. Every other file in the same
directory that touches one of these fails *fast* with a normal `FAIL`
(e.g. `function-container-dynamic.html`, `target is not defined` —
[BUG-384](BUG-384-OPEN.md)) — only the 9 files below, all combining one of
these constructs with the *body* of a `@function` declaration itself (not
just referencing the result from outside), hang. Root cause not isolated
further this session (would need a live `--mcp-live-port` step-through or a
minimal single-construct repro per candidate — `if()` alone, `attr()`
alone, local-var alone, `@layer`-nesting alone — to narrow which of the
five is the actual trigger; plausibly more than one).

## Масштаб находки

9 files, all TIMEOUT with 0/0 subtests registered (0 subtests attempted,
not 0 passed): `function-conditionals.html`, `function-layer.html`,
`function-parameter-types.tentative.html`, `function-shadow-container.html`,
`function-shadow.html`, `local-attr-substitution.html`,
`local-if-substitution.html`, `local-inherit-substitution.html`,
`local-var-substitution.html`.

## Что нужно

Isolate which single construct (`if()`, typed `attr()`, `@function` local
vars, `@function`-in-`@layer`/`@container`, or `@function` invoked in a
shadow tree) causes the hang, via a minimal live-window repro per
candidate. Given `@function`'s "conditional group rules"/`returns`-typing
gaps are already tracked as deferred scope (CSS-SPECS.md T3), the fix
priority here is specifically "don't hang" (fail fast/gracefully on the
unsupported construct), independent of whether full `@function` L1
compliance is implemented.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-mixins/functions/` for
all 9 files, `expected: TIMEOUT`.

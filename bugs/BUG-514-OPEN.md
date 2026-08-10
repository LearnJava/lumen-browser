# BUG-514: `env()` CSS function not implemented in stylesheet parsing/cascade

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** css-parser (`grep -rn '"env"' crates/` finds no CSS-related
hit — only unrelated WebAssembly import-namespace strings in
`crates/js/src/wasm/tests.rs`/`webassembly.rs`)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-env`

## Механизм

`env()` (CSS Environment Variables Module Level 1,
`https://drafts.csswg.org/css-env-1/`) is the mechanism behind
`safe-area-inset-{top,right,bottom,left}` and similar UA-provided values —
shipped everywhere, actively used for notch/safe-area handling on mobile
web. It is not recognized as a function anywhere in `css-parser`: a
declaration whose value contains `env(...)`, at any nesting depth or in any
context (`@supports` condition, `var()` fallback, direct declaration
value), fails to resolve — the whole declaration is treated as invalid
rather than substituting the environment value, so the property falls back
to its own initial value (empty string surfaces through `getComputedStyle`,
since the declaration never took effect at all).

This is distinct from [BUG-484](BUG-484-OPEN.md) (inline `style` setter
accepts anything unvalidated): these failures come from **stylesheet**
declarations (`<style>` blocks, not `element.style = ...`), where the real
`css-parser` grammar is supposed to run — `env()` genuinely isn't a token
the grammar understands, it isn't merely unvalidated.

## Симптом

```
FAIL Test that CSS env vars work with @support
  assert_equals: expected "rgb(0, 128, 0)" but got ""
FAIL background-color: env(test) rgba(0, 0, 0, 0)
  assert_equals: expected "rgba(0, 0, 0, 0)" but got ""
FAIL Test unknown env() names will override previous values
  assert_equals: expected "rgba(0, 0, 0, 0)" but got ""
FAIL Test that CSS env vars work with CSS.supports
  assert_true: expected true got false
```

## Масштаб находки

5 files / 22 subtests: `syntax.tentative.html` (18 — the file's own
parametrized syntax-acceptance table, valid/invalid `env(...)` forms all
fail identically because none are ever substituted), `at-supports.tentative.html`
(1), `unknown-env-names-override-previous.tentative.html` (1),
`supports-script.tentative.html` (1, `CSS.supports("background",
"env(test)")` returns `false` for syntactically valid `env()` usages — same
root cause reached through the OM instead of the cascade),
`fallback-nested-var.tentative.html` (1, `env(test, var(--main-bg-color))`
fallback-to-`var()` never resolves).

Three more files in this category attribute elsewhere: `env-parsing.html` (5)
and `indexed-env.tentative.html` (4) fail on the generic inline-`style`
rejection gap ([BUG-484](BUG-484-OPEN.md) — malformed `env(...)` accepted by
`element.style` instead of rejected, a JS-layer issue independent of this
one), `env-revert-rule.html` (1) fails on the unrelated missing
`revert-rule` keyword ([BUG-487](BUG-487-OPEN.md)), and
`env-in-custom-properties.tentative.html` (2) dies earlier on bare
identifier access ([BUG-384](BUG-384-FIXED.md)).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-env/` for the 5
attributed files, `expected: FAIL` per subtest.

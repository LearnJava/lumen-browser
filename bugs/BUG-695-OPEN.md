# BUG-695 — `URLPattern` is a hand-rolled mini pattern-matcher, not the WHATWG URLPattern spec algorithm

**Статус:** OPEN
**Компонент:** js (`crates/js/src/url_pattern.rs` — `install_url_pattern_api_v8`/`URL_PATTERN_SHIM`)
**Найден:** P2, WPT-VENDOR-urlpattern, 2026-08-09

## Симптом

Категория `urlpattern` (`tests/wpt/urlpattern/`, 12 файлов, self-contained —
no out-of-category deps, no `testdriver.js`) — vendored and run in full
(`run_report.py --all --root urlpattern --recursive`, ~35 s, 9 selected ids)
— **6/9 harness OK, 15/425 subtests**. Per-file breakdown:

| File | Harness | Subtests |
|---|---|---|
| `urlpattern.any.html` | OK | 1/370 |
| `urlpattern-compare.tentative.any.html` | OK | 1/26 |
| `urlpattern-compare.tentative.https.any.html` | ERROR | 0/0 |
| `urlpattern-constructor.any.html` | OK | 1/2 |
| `urlpattern-detached-frame-regexp.html` | TIMEOUT | 0/4 |
| `urlpattern-empty-regexp-group.html` | OK | 1/2 |
| `urlpattern-generate.tentative.any.html` | OK | 11/20 |
| `urlpattern-hasregexpgroups.any.html` | OK | 0/1 |
| `urlpattern.https.any.html` | ERROR | 0/0 |

(The two `.https.` ERRORs are the documented TLS `UnknownIssuer` gap
compounded by the known session-reuse artifact — `urlpattern.https.any.html`
consumed `urlpattern.any.html`'s already-computed result; same class as
`BUG-380`/`focus`'s `ambient-light`-style ERRORs, not a new mechanism.)

`urlpattern.any.html`'s 369 failures are dominated by one message shape:

```
assert_equals: compiled pattern property 'protocol' expected (string) "*" but got (undefined) undefined
assert_equals: compiled pattern property 'protocol' expected (string) "https" but got (undefined) undefined
assert_throws_js: URLPattern() constructor function "_ => new URLPattern(...entry.pattern)" did not throw
```

Plus, standalone:

```
urlpattern-hasregexpgroups: assert_implements: hasRegExpGroups is not implemented undefined
urlpattern-compare: URLPattern.compareComponent is not a function
urlpattern-generate: pattern.generate is not a function
urlpattern-constructor: assert_throws_js: "new URLPattern(new URL('https://example.org/%('))" did not throw
urlpattern-empty-regexp-group: assert_throws_js: "new URLPattern({pathname: '()'})" did not throw
```

## Причина

`crates/js/src/url_pattern.rs` — the JS shim's own doc comment says it
plainly: *"Pure JavaScript implementation of URLPattern"*, a from-scratch
mini pattern-matcher, not a port of the WHATWG URLPattern Standard
algorithm. Concretely, the `URLPattern` class (`URL_PATTERN_SHIM`, lines
147-201):

- **Constructor only reads `pathname`/`search`/`hash`/`hostname`** from
  `init` (line 150-153) — `protocol`, `port`, `username`, `password`, and
  `baseURL` are never read, never stored, never compiled. Any test reading
  `pattern.protocol`/`.port`/`.username`/`.password` gets `undefined`
  because those own properties are simply never assigned — this alone
  accounts for the bulk of `urlpattern.any.html`'s 369 failures (every
  compiled-pattern-property assertion touches at least `protocol`).
- **No component compilation/normalization at all** — the spec requires
  each component (`protocol`, `username`, …, `hash`) to compile through a
  per-component encoding callback and produce a normalized pattern string
  (e.g. an omitted `protocol` compiles to `"*"`, not `undefined`). This shim
  stores the raw `init.pathname`/etc. verbatim with no compilation step.
- **`test()`/`exec()` return a bare `{name: value}` groups object**
  (`matchSegments`, line 106), not the spec-shaped `URLPatternResult`
  (`{inputs, protocol, username, password, hostname, port, pathname,
  search, hash}`, each `{input, groups}`) — every assertion that inspects
  `.exec(...).pathname.groups` instead of the flat object throws or reads
  `undefined`.
- **No `URLPattern.compareComponent` static method** — absent from the
  class entirely (confirmed: `grep -n compareComponent url_pattern.rs` — no
  hits), so `urlpattern-compare.tentative.any.html` fails all 25 real
  comparison assertions with `URLPattern.compareComponent is not a
  function`.
- **No `.hasRegExpGroups` getter** — absent from the class entirely, so
  `urlpattern-hasregexpgroups.any.html`'s single assertion fails
  `assert_implements: hasRegExpGroups is not implemented`.
- **No `.generate(...)` method** (the tentative reverse-templating API) —
  absent from the class entirely, so 6/9 real assertions in
  `urlpattern-generate.tentative.any.html` fail
  `pattern.generate is not a function`.
- **No pattern-syntax validation** — the constructor never throws for
  malformed patterns. Confirmed two ways: `urlpattern-constructor.any.html`
  expects `new URLPattern(new URL('https://example.org/%('))` (an unclosed
  `%`-escape / unclosed token) to throw and it doesn't;
  `urlpattern-empty-regexp-group.html` expects `new URLPattern({pathname:
  '()'})` (an empty regexp group) to throw a `TypeError` and it doesn't —
  `parsePattern` (line 28) has no regexp-group syntax (`(...)`) at all, so
  `(...)`  chars fall through to the `literal` branch untouched, matched
  literally instead of parsed/rejected.
- **`urlpattern-detached-frame-regexp.html` TIMEOUTs outright** — its 4
  subtests construct a regexp-bearing pattern via a same-origin `<iframe>`'s
  own `URLPattern` constructor, detach the frame, then use the pattern —
  exercises both the missing regexp-group support above and (per
  `BUG-480`, already on record) the lack of a real separate browsing
  context for `<iframe>`; not investigated further as a second root cause,
  since the missing regexp-group parsing alone is enough to explain the
  hang (`groups[...]` access on an already-broken match path never
  resolves the promise the harness is awaiting).

## Масштаб

Every one of the category's 9 test files touches this shim in some way;
6/9 harness completed (the other 3 are the documented `.https.`/TIMEOUT
gaps), and of those 6 only 15/425 subtests pass — nearly the entire
category. This is the same class of finding as
[BUG-693](BUG-693-OPEN.md) (`_lumen_parse_url` — a hand-rolled string
splitter standing in for a WHATWG state machine): a Phase 0 placeholder
that implements the *shape* of the API (constructible, has `test`/`exec`)
but none of its parsing/compilation semantics. Unlike BUG-693, there is no
spec-adjacent Rust type to reuse here — the existing `crates/network` `Url`
type resolves/serializes URLs, it does not do URLPattern's glob/regexp
component compilation, so this would be a from-scratch implementation
either way.

## Дальше

Fix scope is large: a real implementation of the URLPattern Standard
(component compilation with `*`/`:name`/`{...}`/`(...)`-regexp syntax per
component, `baseURL` inheritance, `URLPatternResult` shape, static
`compareComponent`, `.hasRegExpGroups`, tentative `.generate()`) — an
architecture decision (pure-JS rewrite vs. a native Rust binding) for
whoever picks this up, not decided here.

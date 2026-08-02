# BUG-486: `document.currentScript` is entirely missing

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs` — live `document` object literal)
**Найден:** WPT-RUN-3 срез 6 (`ROADMAP.md`) — массовый прогон `css/css-cascade`

## Механизм

`document.currentScript` (HTML LS §8.1.3.4) — the `<script>` element
currently being executed, or `null` outside a script's synchronous
top-level execution — is not defined anywhere in the JS shim. Grepped both
`dom.rs` and `v8_runtime.rs` for `currentScript`: zero hits. Bare property
access on the live `document` object therefore falls through to plain
`undefined` rather than the spec-mandated `HTMLScriptElement` reference (or
`null`).

Confirmed live via `--mcp-live-port`: `typeof document.currentScript` →
`"undefined"`.

## Симптом

Three files in this slice (`css/css-cascade`) — `scope-evaluation.html`,
`scope-invalidation.html`, `scope-proximity.html` — share one helper defined
inline at the top of each:

```js
function test_scope(script_element, callback_fn, description) {
  test((t) => {
    let template_element = script_element.previousElementSibling;
    // ...
```

called throughout each file as `test_scope(document.currentScript, () => {
...`. With `document.currentScript` resolving to `undefined`, every call
passes `undefined` as `script_element`, and `.previousElementSibling` on
`undefined` throws `TypeError: Cannot read properties of undefined (reading
'previousElementSibling')` — inside the `test()` callback, so each
individual test reports this as its `FAIL` message rather than crashing the
whole file (harness status stays `OK`, only the subtests fail).

## Масштаб находки

**3 files / 55 subtests** in this slice, 100% attributable — every failing
subtest in these three files carries the identical
`previousElementSibling`-on-`undefined` message, confirmed by reading each
file's full subtest list (no other defect masked underneath):
`scope-evaluation.html` (22), `scope-invalidation.html` (28),
`scope-proximity.html` (5).

Note: all three of these files' `<main id=main>` markup also relies on named
access on Window for other assertions elsewhere in the same pattern family
(the [BUG-384](BUG-384-OPEN.md) mechanism) — but `document.currentScript`
fails **first**, before named access on `main` is ever exercised, so
`document.currentScript` is the correct primary/blocking attribution for
these three files. Fixing this bug alone will not turn every subtest green —
BUG-384 sits immediately behind it for the same files.

Not css-cascade-specific: `document.currentScript` is a generic HTML API
used by any test that needs to locate its own `<script>` tag relative to
sibling markup (a common testing idiom across WPT, not specific to CSS) —
expect this to recur in unrelated future categories.

## Что нужно

Add `get currentScript()` to the `document` object literal, backed by
runtime state that tracks the currently-executing `<script>` element's node
id during synchronous script evaluation (set on entry to each `<script>`'s
execution, cleared — per spec, to `null` — once that script's synchronous
run completes; `null` for the async/deferred/module cases per HTML LS
§8.1.3.4 step list). Both engines (`dom.rs` rquickjs path,
`v8_runtime.rs` V8 path) need the tracking hook wherever they currently
invoke a `<script>`'s source text.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-cascade/` for the 3
attributed files, `expected: FAIL` per the actual run.

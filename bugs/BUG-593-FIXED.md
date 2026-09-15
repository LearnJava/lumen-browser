# BUG-593: `structuredClone` cannot clone `Blob`/`File`/`ImageData`/`Error` -- silently throws `DataCloneError` instead of cloning

**Статус:** FIXED 2026-09-16 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_tail_b.js` -- `structuredClone`; `crates/js/src/file_input.rs`)
**Найден:** P2, WPT-VENDOR-html-webappapis, 2026-08-04

## Исправление

`dom.rs` has since been split into `crates/js/src/dom/`; the `structuredClone`
walker itself now lives in `web_api_shim_tail_b.js` (`shim/`). Added branches
for `Error` (finds the closest of the six native subclasses via `instanceof`,
copies only `message`/`cause`/`stack` -- a subclass's own custom properties,
e.g. `error.foo = ...`, do not survive) and `Blob` (copies `_bytes`/`_type`).
`ImageData` clones by copying its internal `__image_data__` slot via the
existing `_lumen_slot` helper.

`File` needed a different mechanism: `file_input.rs` overrides the global
`File` *after* `structuredClone`'s script runs, with a token-backed
implementation (`FILE_TOKENS` WeakMap) -- a real file-input selection has no
in-memory bytes at all, they live on disk behind a capability token. The
direct `Blob` branch explicitly excludes `File` instances, which instead
clone through the pre-existing `__lumen_platform_cloners` extension point
(the same one `filesystem_access.rs`'s handle cloner uses): the clone gets
the same `name`/`size`/`type`/`lastModified`/`_content` and shares the same
read token as the original. A `File` subclass clones as plain `File`
(WPT: "A subclass instance will deserialize as its closest serializable
superclass").

8 new tests in `dom/tests/v8_url_abort_clone_blob.rs`. `cargo test -p
lumen-js --lib --features v8-backend` 3689/3689 (was 3681), `cargo clippy -p
lumen-js --all-targets --features v8-backend -- -D warnings` clean.

## Симптом

```
FAIL Blob object - assert_true: instanceof Blob expected true got false
```
(`html/webappapis/structured-clone/structured-clone.any.html`, 19 subtests,
each constructing a `Blob`/typed value, round-tripping it through
`structuredClone`, and asserting the clone is `instanceof` the original
platform type)

## Причина

`structuredClone`'s `clone()` walker (`dom.rs:10184` onward) has explicit
branches for `Date`/`RegExp`/wrapper objects/`ArrayBuffer`/typed
arrays/`Map`/`Set`, but no branch for `Blob`, `File`, `ImageData`, or `Error`
-- all four are `[Serializable]` platform objects per the HTML LS structured
serialize/deserialize algorithm. Any such value falls through to the generic
"plain object" path or the `t === 'object'`-without-a-known-constructor
fallthrough that throws `DataCloneError`, so a `Blob` never round-trips at
all -- not even by degrading to a plain-object shell.

## Масштаб

19 of the file's subtests fail on `Blob` alone (`structured-clone.any.html`
also expects `File`/`ImageData`/`Error` to round-trip, per the same code
comment already listing all four as unhandled). Anything downstream that
relies on `structuredClone`/`postMessage` faithfully carrying a `Blob` (worker
messaging, IndexedDB writes that go through structured clone,
`history.pushState` with binary state) inherits the same gap silently.

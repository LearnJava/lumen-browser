# BUG-593: `structuredClone` cannot clone `Blob`/`File`/`ImageData`/`Error` -- silently throws `DataCloneError` instead of cloning

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:10176-10260` `structuredClone` -- own doc comment at `dom.rs:10182-10183` already states "Not handled: ... Blob/File/ImageData/Error and other platform objects", never filed as a tracked bug until now)
**Найден:** P2, WPT-VENDOR-html-webappapis, 2026-08-04

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

# BUG-569: `HTMLImageElement.prototype.decode()` does not exist

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — `HTMLImageElement` wrapper has no
`decode` member; confirmed by `grep -n "\"decode\"" crates/js/src/*.rs`
returning nothing anywhere in the JS crate)
**Найден:** P2, WPT-VENDOR-html-semantics-embedded-content, 2026-08-04

## Симптом

`img.decode()` throws `TypeError: img.decode is not a function` on every
`HTMLImageElement` instance. All 6 variants of
`html/semantics/embedded-content/the-img-element/*decode*` fail identically
before reaching the promise-resolution assertion they test:

```
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in empty picture cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in empty picture not cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in picture with source cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src in picture with source not cached - img.decode is not a function
FAIL HTMLImageElement.prototype.decode(), attach to DOM before promise resolves.: src not cached - img.decode is not a function
```

## Причина

`HTMLImageElement.decode()` (HTML LS §4.8.4.4, "Decoding images") is a
Promise-returning method that resolves once the image data has finished
decoding — distinct from waiting on the `load` event, and the mechanism
sites use to avoid painting a not-yet-decoded frame. It has never been added
to the image element wrapper; the image pipeline decodes synchronously
during load today (no async decode task queued), so there is no natural hook
for the method to await yet.

## Масштаб

57 failing subtests in this category alone, all under
`the-img-element/`, all with the identical `TypeError` shape — a single
missing method blocks every `decode()`-based test regardless of what image
state (cached/picture/source) each variant is actually probing.

# BUG-565: `document.head` does not exist — always `undefined`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — the `Document` object literal, ~line
7140-7210: `get body()` and `get documentElement()` are defined there, `head`
is not)
**Найден:** P2, WPT-VENDOR-html-semantics-document-metadata, 2026-08-04

## Симптом

`document.head` evaluates to `undefined` on any live document — not `null`,
not a lazily-created element, simply an own-property that was never added to
the `Document` object literal. Any script that writes
`document.head.appendChild(...)`/`.append(...)`/`.prepend(...)` throws
synchronously:

```
TypeError: Cannot read properties of undefined (reading 'appendChild')
TypeError: Cannot read properties of undefined (reading 'append')
TypeError: Cannot read properties of undefined (reading 'prepend')
```

Confirmed by direct source read: `grep -n "\bhead\b" crates/js/src/dom.rs`
turns up only skeleton-construction code for a *detached* document built by
`createHTMLDocument()`/`DOMParser` (`dom.rs:4914`, `dom.rs:16016-16037`) and a
mirror of the same helper in `crates/js/src/v8_runtime.rs:5198` — none of
which run for the page's own live `document`. The live `Document` object
literal (`dom.rs:7140`-ish) has explicit getters for `body`
(`get body() { var bid = _lumen_u2n(_lumen_get_body()); … }`) and
`documentElement` (`get documentElement() { var hid =
_lumen_u2n(_lumen_get_html_element()); … }`, added for BUG-281) but no
matching `get head()` — despite a native `_lumen_get_html_element`-style
accessor being the obvious, already-proven pattern to add one.

## Причина

`document.head` (HTML LS §3.1.3: the first `<head>` child of the document
element, or `null` if there is none) was simply never wired up when
`body`/`documentElement` were. Not a design decision — no code path rejects
or stubs it deliberately, the property is absent because no one added it.

## Масштаб

17 test files across `html/semantics/document-metadata` reference
`document.head` (`grep -rl "document\.head\." tests/wpt/html/semantics/
document-metadata/`), spanning `the-link-element/` (stylesheet-loading
tests inject `<link>` via `document.head.append(link)`),
`the-style-element/` (CSP/`<style>` mutation tests), and
`the-meta-element/`. Every one of those either throws directly or silently
skips its setup step, so the failure is not scoped to `document-metadata` —
`document.head` is one of the most commonly used DOM entry points in the
entire WPT corpus (test setup routinely injects `<link>`/`<meta>`/`<style>`
via it) and any category touching page metadata will hit the same throw.

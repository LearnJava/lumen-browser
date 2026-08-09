# BUG-589: `window` is not a proper WebIDL exotic object — no `Symbol.toStringTag`, indexed `[[DefineOwnProperty]]`/`[[Set]]` don't reject out-of-range numeric keys

**Статус:** OPEN
**Компонент:** js/V8 host-object layer (`window` global + its "WindowProperties" global-scope-polluter prototype; the `Symbol.toStringTag` pattern the shim already applies to `HTMLCollection`/`HTMLFormControlsCollection`/`HTMLOptionsCollection` at `crates/js/src/dom.rs:11143-11165` is not applied to `window` itself)
**Найден:** P2, WPT-VENDOR-html-browsers, 2026-08-04

## Симптом

```
FAIL window object - assert_class_string: expected "[object Window]" but got "[object Object]"
FAIL Global scope polluter - assert_class_string: expected "[object WindowProperties]" but got "[object Object]"
```
(`html/browsers/the-window-object/window-prototype-chain.html`, self-contained,
no iframe dependency)

```
FAIL Borderline numeric key: 2 ** 32 - 2 is an index (strict mode) - assert_throws_js: function "() => { window[4294967294] = 1; }" did not throw
```
(`html/browsers/the-window-object/window-indexed-properties-strict.html`,
this specific assertion doesn't depend on any iframe existing)

## Причина

Two independent gaps in `window`'s exotic-object behavior:

1. Neither `window` nor the "WindowProperties" object in its prototype chain
   (`Object.getPrototypeOf(Object.getPrototypeOf(window))`) carry a
   `Symbol.toStringTag`, so `Object.prototype.toString.call(window)` answers
   the generic `[object Object]` instead of `[object Window]` — the same
   class of defect already tracked for `Headers`/`Response`/
   `CredentialsContainer`/etc. in [BUG-369](bugs/BUG-369-OPEN.md)/
   [BUG-366](bugs/BUG-366-FIXED.md), here on the global object itself.
2. `window`'s indexed-property `[[DefineOwnProperty]]`/`[[Set]]` don't
   implement the WebIDL "index in `[0, 2**32-2]` with no indexed setter for
   that slot → throw `TypeError` in strict mode" rule at all: any numeric
   key silently accepts a plain assignment instead of being rejected. This
   holds even for indices that can never correspond to a real nested
   browsing context (`2**32-2`), so it's independent of the iframe-support
   limitation.

## Масштаб

Both assertions above are self-contained (no iframe required). The rest of
`window-indexed-properties-strict.html`/`window-indexed-properties.html`/
`named-access-on-the-window-object/window-named-properties.html` layers the
already-known "`<iframe>` without browsing context" limitation on top (they
assert `window[0] === iframe.contentWindow`), so expect those to need both
fixes before going green.

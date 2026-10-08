# BUG-592: `setHTMLUnsafe`/`getHTML`/`parseHTMLUnsafe` family only implemented on `Element`, missing on `ShadowRoot` and `Document`

**Статус:** FIXED 2026-09-16
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `ShadowRoot.prototype`; `crates/js/src/dom_parser.rs` — `Document.parseHTMLUnsafe`)
**Найден:** P2, WPT-VENDOR-html-webappapis, 2026-08-04

## Симптом

```
FAIL ShadowRoot: setHTMLUnsafe with no shadowdom. - assert_true: container.setHTMLUnsafe is not a function expected true got false
```
(`html/webappapis/dynamic-markup-insertion/html-unsafe-methods/setHTMLUnsafe.html`,
`Element` variant of the same test passes)

```
TIMEOUT html/webappapis/dynamic-markup-insertion/html-unsafe-methods/Document-parseHTMLUnsafe.html
TIMEOUT html/webappapis/dynamic-markup-insertion/html-unsafe-methods/Document-parseHTMLUnsafe-url.html
TIMEOUT html/webappapis/dynamic-markup-insertion/html-unsafe-methods/Document-parseHTMLUnsafe-encoding.html
```

## Причина

`Element.prototype.setHTMLUnsafe`/`getHTML` (WHATWG HTML LS §14.5) were
implemented once, directly inside the per-element object literal; `ShadowRoot`
(by now a real `ShadowRoot.prototype` — DOM LS §4.2.2.1, BUG-676 — extending
`DocumentFragment.prototype`, same file) never got the two methods added
alongside its existing `innerHTML`/`textContent` accessors, even though the
spec places both on a shared `Element`/`ShadowRoot` mixin. Separately,
`Document.parseHTMLUnsafe` — the *static* factory that parses a full HTML
document string into a new detached `Document` — did not exist at all, on
either the `document` object literal or the `Document` constructor.

## Исправлено

- `ShadowRoot.prototype.setHTMLUnsafe`/`getHTML` (`web_api_shim_mid.js`):
  same one-line pattern as `Element`'s copy — both delegate to
  `_lumen_set_inner_html`/`_lumen_get_inner_html` over `this.__nid__`, which
  `ShadowRoot` already had wired for its `innerHTML` accessor.
- `Document.parseHTMLUnsafe` (`dom_parser.rs`, next to `DOMParser`): a static
  alias for `_vBuildDocument(html, 'text/html')` — the same virtual-DOM HTML
  builder `DOMParser().parseFromString(html, 'text/html')` already uses, not
  a second parser. Guarded with `typeof Document !== 'undefined'` since this
  module's own unit-test harness stubs a bare `document` object with no
  `Document` constructor.
- New test `dom_parser::tests_v8::document_parse_html_unsafe_is_static_and_returns_document`.
  `cargo test -p lumen-js --lib --features v8-backend` 3683/3683 (dom_parser
  module: 27/27, was 26). `cargo clippy -p lumen-js --all-targets --features
  v8-backend -- -D warnings` clean.

## Масштаб / live WPT result

Live `run_report.py --all --root
html/webappapis/dynamic-markup-insertion/html-unsafe-methods --recursive`:
9/13 → 9/13 harness-OK unchanged in count, but the 8 previously-TIMEOUT
`Document-parseHTMLUnsafe*.html` files now reach their own subtests (harness
`OK`/`ERROR` instead of `TIMEOUT`), and `setHTMLUnsafe-runScripts.html` gained
4 previously-`FAIL` `ShadowRoot`/`Element` subtests as real `PASS`.
`Document.parseHTMLUnsafe`'s own subtests now `FAIL` in exactly the same
places `DOMParser().parseFromString(html, 'text/html')` already does (no
`compatMode`, no IDL `id` reflection on `VElement`, `noscript` parsed as raw
text) — pre-existing gaps in the shared virtual-DOM HTML builder, not a
regression introduced here; `.ini` expectations updated to match (4 files:
`Document-parseHTMLUnsafe.html`, `Document-parseHTMLUnsafe-style-attribute.html`,
`Document-parseHTMLUnsafe-url-moretests.html`, `setHTMLUnsafe-runScripts.html`).
Three files remain harness `ERROR` (`Document-parseHTMLUnsafe-encoding.html`,
`-url-base-pushstate.html`, `-url-pushstate.html`) — unchanged from before this
fix, out of scope (unrelated `pushstate`/encoding gaps).

Remaining half of the same mixin — the **safe** `Document.parseHTML`/
`ShadowRoot.prototype.setHTML` (sanitizer-backed, not "unsafe" passthrough) —
tracked separately in [BUG-663](BUG-663-FIXED.md), which needs the
config-object `Sanitizer` redesign first.

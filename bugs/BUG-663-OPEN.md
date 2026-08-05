# BUG-663 — Sanitizer API implements an obsolete draft: no `Document.parseHTML`/`parseHTMLUnsafe`, no config-object methods (`get`/`allowElement`/`removeElement`/`allowAttribute`/`removeAttribute`/`removeUnsafe`/`replaceElementWithChildren`), no `ShadowRoot.setHTML`/`setHTMLUnsafe`, and `setHTML({sanitizer: <plain config>})` crashes instead of implicitly constructing a `Sanitizer`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/sanitizer.rs` — JS shim, `install_sanitizer_bindings_v8`, evaluated by the V8 install path per `CLAUDE.md`; `Element.prototype.setHTMLUnsafe` at `crates/js/src/dom.rs:3499`)
**Найден:** P2, WPT-VENDOR-sanitizer-api (2026-08-05), `run_report.py --all --root sanitizer-api --recursive` real run

## Механизм

`sanitizer.rs`'s own doc-comment already labels this "Phase 0 stub", and
`CAPABILITIES.md` lists "Sanitizer (Phase 0)" accordingly — but the shape it
implements is a pre-redesign draft of the API (a single
`sanitizer.sanitizeFor(element, htmlString)` instance method doing a crude
regex strip of `<script>` tags and `on*` attributes). The current spec (and
every upstream WPT test in this category) targets the 2023+ declarative,
config-object-based redesign, which is essentially absent:

1. **`Document.parseHTML`/`Document.parseHTMLUnsafe` static methods don't
   exist at all** — zero hits for either name anywhere in `crates/js/src/`.
   Every test file in the category calls one or both
   (`Document.parseHTML(html, config).body`); all fail with
   `TypeError: Document.parseHTML is not a function` (72 hits) /
   `... parseHTMLUnsafe is not a function` (32 hits).
2. **The config-object methods of `Sanitizer` don't exist.** The redesigned
   spec drives sanitization through a declarative config
   (`{elements, attributes, ...}`) manipulated via `get()`, `allowElement()`,
   `removeElement()`, `allowAttribute()`, `removeAttribute()`,
   `removeUnsafe()`, `replaceElementWithChildren()` — `sanitizer.rs` defines
   none of them, only the constructor and `sanitizeFor()`. Every call site
   throws `TypeError: <ref>.<method> is not a function` (98+10+10+8+4+6+2
   hits across the seven names).
3. **`ShadowRoot` never got `setHTML`/`setHTMLUnsafe` wired at all** —
   `_lumen_make_shadow_root` (`dom.rs:1315`) is a hand-built object literal
   with a fixed, small method set (`querySelector(All)`, `getElementById`,
   `appendChild`, `removeChild`, listeners, `innerHTML`/`textContent`
   accessors) that was never extended when `Element.prototype.setHTML`
   (sanitizer.rs) / `.setHTMLUnsafe` (dom.rs:3499) were added to `Element`.
   `shadowRoot.setHTML(...)` / `.setHTMLUnsafe(...)` throw
   `TypeError: context.setHTML is not a function` (8 hits) /
   `shadowRoot.setHTMLUnsafe is not a function` (8 hits).
4. **`setHTML(html, {sanitizer: <plain config object>})` crashes instead of
   implicitly constructing a `Sanitizer`.** Per spec, `options.sanitizer` may
   be either a `Sanitizer` instance or a plain `SanitizerConfig` dict — a
   conforming `setHTML` constructs `new Sanitizer(options.sanitizer)`
   internally when it's a plain object. `sanitizer.rs`'s shim
   (`Element.prototype.setHTML`, line ~92) instead calls
   `sanitizer.sanitizeFor(this, html)` on whatever `options.sanitizer` is
   verbatim — when the test harness passes a plain
   `JSON.parse(testcase.config)` object (the common case; `sanitizer.rs`'s
   own tests only ever pass a real `new Sanitizer()`), the plain object has
   no `sanitizeFor` method and the call throws
   `TypeError: sanitizer.sanitizeFor is not a function` (166 hits, by far the
   largest single class in the run).

## Live run signal (not exhaustive — see `.tmp/wpt-sanitizer-api.html` for full detail)

```
tests: 26/27 harness OK; subtests: 72/480 passed
```

Top error classes from the run log (`sanitizer-basic-filtering.html`,
`sanitizer-config.html`, `sanitizer-get.html`, `sethtml-tree-construction.html`,
`sethtml-with-declarative-shadow-root.tentative.html`, and others):

```
166  sanitizer.sanitizeFor is not a function     (item 4)
 98  s.get is not a function                     (item 2)
 72  Document.parseHTML is not a function        (item 1)
 44  current1.isEqualNode is not a function       — see "not this bug" below
 32  Document.parseHTMLUnsafe is not a function   (item 1)
 24  node.replaceChildren is not a function       — see "not this bug" below
 24  node.insertAdjacentHTML is not a function    — reconfirms BUG-351
 16  range.createContextualFragment is not a function — not wired at all
 10  s.removeAttribute / s.allowAttribute is not a function  (item 2)
  8  shadowRoot.setHTML(Unsafe) is not a function  (item 3)
  8  s.allowElement is not a function              (item 2)
  8  context.setHTML is not a function             (item 3)
  6  s.replaceElementWithChildren is not a function (item 2)
  4  s.removeElement is not a function              (item 2)
```

## Что НЕ является причиной этого бага (уже задокументированные гэпы или недоисследовано)

- `node.insertAdjacentHTML is not a function` (24 hits) — reconfirmation of
  [BUG-351](../bugs/BUG-351-OPEN.md) (`insertAdjacentHTML` missing on the
  live `Element` entirely).
- `policy.createParserOptions`/`passthrough.createParserOptions is not a
  function` (8+4 hits, `sethtml-with-trustedtypes*.tentative.html`) — Trusted
  Types is a separate, wholly unimplemented API; out of scope for this bug.
- `current1.isEqualNode is not a function` (44 hits) and
  `node.replaceChildren is not a function` (24 hits, same
  `sethtml-with-trustedtypes.tentative.html`/tree-construction files) — not
  investigated to a root cause in this session; `replaceChildren` **is**
  defined on ordinary `Element` instances (`dom.rs:3003`), so these calls are
  landing on some other object shape (a `Document.parseHTML(...).body`
  result, or a node from a detached document per
  [BUG-415](../bugs/BUG-415-OPEN.md)) — plausible but not confirmed; leave
  for a follow-up pass once item 1 exists to actually construct the objects
  these tests exercise.
- `fetch: network error for sethtml-tree-construction.sub.dat` /
  `sethtml-safety.sub.dat` (2+2 hits) — `.sub.dat` fixture fetch failures, not
  investigated; likely a wptserve MIME/substitution quirk rather than an
  engine defect, given the file loads via a relative-path `fetch()` the same
  way other categories' `.sub.js` helpers do.

## Предлагаемый фикс

This is a full API-surface rewrite, not a patch: (1) add
`Document.parseHTML`/`parseHTMLUnsafe` static methods that build a detached
document from sanitized markup; (2) replace `sanitizeFor`-only `Sanitizer`
with the config-object model (`get`/`set`/`allowElement`/`removeElement`/
`allowAttribute`/`removeAttribute`/`removeUnsafe`/`replaceElementWithChildren`,
backed by an actual elements/attributes allow/remove list rather than a
regex strip), with `sanitizeFor`/`setHTML` implicitly wrapping a plain config
object in `new Sanitizer(...)` per spec; (3) wire `setHTML`/`setHTMLUnsafe`
onto `_lumen_make_shadow_root`'s object literal alongside `Element.prototype`.
Given the size, worth splitting into the three items above rather than one
sweep — (1) and (3) are small, additive, and unblock most of the "is not a
function" noise; (2) is the real design work (matching the spec's default
allow-list of safe elements/attributes) and can land after.

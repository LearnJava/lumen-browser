# BUG-351 — `outerHTML` and `insertAdjacentHTML` missing entirely on the live `Element` interface

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs` — the live-DOM `Element` object literal, roughly lines 5522-5880 in the `p1-*`/`p2-*` worktree snapshot; `insertAdjacentText`/`insertAdjacentElement` live right next to the gap)
**Найден:** P2, WPT-VENDOR-domparsing (2026-07-26), `run_report.py --all --root domparsing --recursive`

## Симптом

Both are part of the DOM Parsing and Serialization spec's "Extensions to the
Element interface" (`https://w3c.github.io/DOM-Parsing/#extensions-to-the-element-interface`)
and both are absent from the real, live-document `Element`:

1. `insert-adjacent.html` (0/4) and `insert_adjacent_html.html` (1/31, the one pass
   is an unrelated `assert_throws` check that happens to match by accident) —
   every subtest throws `TypeError: el.insertAdjacentHTML is not a function` /
   `node.insertAdjacentHTML is not a function`. Sibling API `insertAdjacentText`
   was found missing and fixed as BUG-299 (2026-07-17); `insertAdjacentHTML` —
   the far more commonly used of the two in real-world code — was never added.
2. `outerhtml-01.html` (0/1) and `outerhtml-02.html` (1/5) — `element.outerHTML`
   has no getter/setter on the live DOM at all. Assigning to it silently creates
   a plain own JS property (no DOM effect), so `div.innerHTML`/`div.textContent`
   checks after `p.outerHTML = <value>` fail for every non-null RHS (`undefined`,
   `42`, objects with `toString`/`valueOf`) — 4 of 5 subtests in `outerhtml-02.html`.
   (The 1 pass, `outerHTML = null`, is coincidental: it happens to leave the DOM
   in the same state the assertion expects, not evidence the setter runs.)
3. Confirmed by grep: `crates/js/src/dom_parser.rs:111` defines `outerHTML` only
   on `VElement.prototype` — the separate, parallel "virtual node" tree built by
   `DOMParser.parseFromString()` for round-trip serialization (see that file's
   module doc comment). The real, live `Element` returned by `document.createElement`/
   `querySelector`/etc. never gets an `outerHTML` accessor, and `insertAdjacentHTML`
   doesn't exist on either tree.

## Причина

`crates/js/src/dom.rs`'s live-`Element` object literal defines `innerHTML`
get/set via `_lumen_get_inner_html`/`_lumen_set_inner_html`, and separately
defines `insertAdjacentText`/`insertAdjacentElement` (added for BUG-299) by
delegating to `before`/`prepend`/`append`/`after`. Neither `outerHTML` nor
`insertAdjacentHTML` were ever added to this object — a plain omission, not a
deliberate scope decision (unlike the `VElement`/live-DOM split itself, which
is an intentional, documented design for `DOMParser`).

## Масштаб

Both APIs are extremely common in real-world HTML/JS (jQuery-era DOM
manipulation, templating snippets, many current sites) — this is likely to
affect page compatibility broadly, not just the `domparsing` WPT category.

## Возможный фикс (не реализован в этой сессии)

- `outerHTML` getter: serialize the element itself the same way `innerHTML`'s
  getter serializes children (a `_lumen_get_outer_html(nid)` native, or build
  it in JS from `tagName` + attributes + `innerHTML`).
- `outerHTML` setter: parse the assigned string as HTML fragment (same parser
  `_lumen_set_inner_html` already drives) into a detached fragment, then replace
  `this` with the fragment's children in the parent (mirrors `replaceWith`).
  Per spec must throw `NoModificationAllowedError` when `this.parentNode` is a
  `Document` (see `outerhtml-01.html`'s currently-failing throw check — same
  gap, folded into this bug rather than filed separately).
- `insertAdjacentHTML`: parse the string the same way, then delegate to the
  existing `before`/`prepend`/`append`/`after` methods per the `where` argument,
  mirroring the `insertAdjacentText`/`insertAdjacentElement` pattern already in
  place.

Not fixed in this session — P2-wpt vendors and surveys, code fixes are P3's
lane (`CLAUDE.md` developer assignments).

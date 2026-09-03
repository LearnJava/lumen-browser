# BUG-970: cross-frame Element facade has no `.attributes` (`NamedNodeMap`)
at all — reading it throws `TypeError`

**Статус:** OPEN
**Дата:** 2026-09-03
**Компонент:** js (`crates/js/src/frame_bridge.rs::frameElem`)
**Найден:** P2, WPT-RUN-6 срез 55, живой пробой (побочная находка, не
объясняет ни один TIMEOUT сама по себе)

## Механизм

`frameElem` (`crates/js/src/frame_bridge.rs:1734-1861`) builds the read/write
facade a parent document sees for an element living in a child `<iframe>`'s
document (BUG-480's bridge). It defines `nodeType`/`localName`/`tagName`/
`nodeName`/`id`/`className`/`src`/`href`/`getAttribute`/`hasAttribute`/
`children`/`childElementCount`/`firstElementChild`/`lastElementChild`/
`parentElement`/`querySelector(All)`/`setAttribute`/`removeAttribute`/
`appendChild`/`insertBefore`/`removeChild`/`remove`/`textContent`/`click`/
`focus`/`blur`/`dispatchEvent`/`addEventListener`/`removeEventListener`/
`offsetWidth`/`offsetHeight`/`clientWidth`/`clientHeight`/`scrollWidth`/
`scrollHeight`/`getBoundingClientRect` — a wide surface, but **no
`.attributes` property anywhere in the object literal**. Reading
`someFrameElement.attributes` therefore returns plain `undefined` (not even a
getter that throws — the property simply was never defined), and any further
member access on it (`.attributes.length`, `for...of attributes`, etc.)
throws `TypeError: Cannot read properties of undefined`.

## Симптом

Confirmed live (`--mcp-live-port`, `tests/wpt/verify_slice55_gaps.py`,
2026-09-03, `main` = `d05d62fd2`), on
`/html/rendering/non-replaced-elements/the-page/iframe-marginwidth-marginheight.html`
(`<iframe src="/common/blank.html" marginwidth=0 marginheight=0>`, parent
script: `window[0].document.body.attributes.length`):

```
[JS error] Uncaught TypeError: Cannot read properties of undefined (reading 'length')
```

The error is specifically about `'length'`, not `'document'` or `'body'` —
confirming `window[0]` (indexed frame access, fixed by BUG-480 slice 3) and
`.document`/`.body` (the bridge's document/body facades) all resolve fine;
only `.attributes` on the returned body facade is missing. The test itself
does **not** hang on this — `testharness.js`'s global `window.onerror`
handler catches the uncaught exception in `single_test` mode and reports it
as a subtest FAIL, so the harness completes promptly
(`harness-complete status=0 tests=1 ...:1`). This id is therefore **not**
reclassified as this bug's TIMEOUT mechanism in `timeout_audit.py` — it
doesn't reproduce as a hang at all on current `main` — but the underlying
`.attributes` gap is real and independently worth fixing.

## Масштаб

Any parent-document script reading `.attributes` off a same-origin child
frame's element (a fairly common DOM-inspection idiom, not specific to this
one WPT test) hits the same `TypeError`. Cheap fix relative to most of
BUG-480's remaining queue: `.attributes` needs a `NamedNodeMap`-shaped
read-only view over `_lumen_f_*`'s existing per-node attribute access (the
underlying `getAttribute`/`hasAttribute`/`setAttribute`/`removeAttribute`
natives already exist; what's missing is enumerating *all* attribute names
for a given nid and wrapping that in the small `length`/`item()`/
`getNamedItem()`/index-access shape `NamedNodeMap` needs).

## Классификация WPT-RUN-6

Not attributed to any `timeout_audit.py` mechanism — the id this was found
through does not reproduce as a TIMEOUT on current `main` (see above), so it
stays in the `unclassified` residual without a marker, same as slice 53's
`elements-at-point.html`/`scrollbars-2.html` precedent (a real defect found
along the way, filed on its own merits per `docs/probe-method.md` §3, not
forced onto the TIMEOUT it was found while investigating).

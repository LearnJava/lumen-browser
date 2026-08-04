# BUG-580: `Element.prototype.getClientRects()` missing entirely (element has `getBoundingClientRect` but no `getClientRects`)

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:6044-6049` — `getBoundingClientRect`
exists on the element object literal; no sibling `getClientRects`. The only
`getClientRects` in the whole file, `dom.rs:6940`, belongs to `Range`, not
`Element`)
**Найден:** P2, WPT-VENDOR-html-semantics-misc, 2026-08-04

## Симптом

```
FAIL <test name> - el.getClientRects is not a function
```

76 occurrences, mostly `popovers/resources/popover-utils.js`'s `isVisible()`
helper (`el.offsetWidth || el.offsetHeight || el.getClientRects().length`)
called from many popover tests, plus scattered direct calls elsewhere in the
slice.

## Причина

CSSOM View §5 `getClientRects()` (a live `DOMRectList`, normally
one rect per CSS fragment/line-box the element renders in — degenerates to
a one-element list equal to `getBoundingClientRect()` for a non-fragmented
box) is not implemented on `Element` at all. `getBoundingClientRect`
(`dom.rs:6044`) is; nothing calls it from a `getClientRects` wrapper the way
`Range.prototype.getClientRects` already does at `dom.rs:6940`
(`return [this.getBoundingClientRect()];`) — the same one-line pattern would
close this gap for the common (non-fragmented) case.

Two smaller companion gaps surfaced by the same run, same root shape
(present on one related interface, absent on another):
- `Document.prototype.elementFromPoint` — 8 occurrences. `caretPositionFromPoint`-style
  hit-testing exists elsewhere in the file but no plain `elementFromPoint`.
- `Node.prototype.getRootNode()` — 6 occurrences (`current.getRootNode is not
  a function`). Needed for shadow-DOM-aware ancestor walks (`{composed:
  true}` option crosses shadow boundaries); `attachShadow`/`shadowRoot` exist
  (`dom.rs:6039-6042`) but the root-walking helper itself doesn't.

## Масштаб

`getClientRects` is the dominant one (76 hits, self-contained fix per the
`Range` precedent). The two companion gaps are small (8 and 6 hits) and
noted here rather than filed separately, proportional to size, per the
established convention for this WPT-VENDOR track.

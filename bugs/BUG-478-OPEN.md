# BUG-478: `Element.prototype.getClientRects()`/`getBoxQuads()` missing (only `Range` has `getClientRects`)

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

```
FAIL sub element in a child inline box should not be included
  target.getClientRects is not a function
FAIL Element getClientRects()
  document.getElementById(...).getClientRects is not a function
```

## Причина

`grep -n "getClientRects" crates/js/src/dom.rs` finds it defined exactly
once, on the `Range` object literal built by `_lumen_make_range`
(`dom.rs:6947`: `getClientRects: function() { return [this.getBoundingClientRect()]; }`).
The live `Element`/`Node` wrapper has no `getClientRects` at all — `'x' in
document.body` for that name is `false`, not a broken getter.

## Масштаб находки

12 subtests of `getClientRects` (`getClientRects-br-htb-ltr.html`,
`getClientRects-inline-inline-child.html`, `getClientRects-zoom.html`,
`DOMRectList.html`, `cssom-getClientRects.html`,
`cssom-getClientRects-002.html`) plus harness-level TIMEOUT on
`getClientRects-inline-atomic-child.html`; separately, `getBoxQuads()`
(CSSOM View §6, a distinct but structurally identical gap — same "is not a
function" symptom, same missing-entirely root cause) accounts for 7 more
subtests in `cssom-getBoxQuads-001.html`/`cssom-getBoxQuads-002.html`.

## Что нужно

Add `getClientRects()` to the live `Element`/`Range`/`Text` wrapper(s),
returning a `DOMRectList`-like array — for elements, one rect per CSS box the
element generates (a single-rect `[getBoundingClientRect()]` fallback is
spec-incomplete for multi-fragment inlines but would already unblock the
`is not a function` failures; a correct multi-fragment answer needs per-line
fragment rects from layout, which the box tree already has — see the
`InlineRun`/`frag[]` structure `--dump-layout` prints).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: FAIL`/`TIMEOUT` per the actual
run.

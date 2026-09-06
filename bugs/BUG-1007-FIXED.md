# BUG-1007: `Element.prototype.getClientRects()`/`getBoxQuads()` return only a single union rect for multi-fragment inline content

**Статус:** FIXED 2026-09-05 (ветка `p1-gapgeom2-per-fragment-rects`, продолжение задачи [GAP-GEOM](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — тот самый «per-fragment список» остаток, который [BUG-478](BUG-478-FIXED.md) явно назвал отдельной работой при закрытии GAP-GEOM 2026-09-05. Найден и исправлен в одной сессии (P1), поэтому файл сразу заведён как FIXED, без промежуточного OPEN.
**Дата:** 2026-09-05
**Компонент:** engine layout (`crates/engine/layout/src/lib.rs`), js (`crates/js/src/v8_runtime/*`, `crates/js/src/shim/web_api_shim_mid.js`), shell/driver plumbing (`crates/shell/src/*`, `crates/driver/src/session.rs`)
**Найден:** ре-триаж GAP-GEOM (BUG-478/BUG-522, `ROADMAP.md:900`) — задача попросила завести отдельную работу под этот хвост

## Симптом

`Element.prototype.getClientRects()`/`getBoxQuads()` (BUG-478, 2026-09-05) closed
every "is not a function" failure with a single-rect fallback:

```js
getClientRects: function() {
    return new DOMRectList([this.getBoundingClientRect()]);
},
```

Correct for any element that owns its own `LayoutBox` (the common case), but
spec-incomplete for a plain inline element (`<span>`, `<em>`, …) that wraps
onto more than one visual line — CSSOM View §6 requires one `DOMRect` per CSS
fragment (one per line, for a wrapped inline), and the fallback always
answered with exactly one, the union of every line. `contain-intrinsic-size/
auto-010.html` (originally BUG-551, folded into BUG-478) is the WPT case that
depends on this: "Last remembered size" on a multi-fragment element.

## Причина

`Element.prototype.getBoundingClientRect()` reads a native
`_lumen_get_bounding_rect(nid)`, backed by `layout_rects: HashMap<u32,
[f32; 4]>` — a snapshot the shell/driver push after every relayout via
`lumen_layout::collect_layout_rects`, which deliberately **unions** every
`InlineFrag` belonging to a plain inline element (BUG-488) into one summarising
rect. `getClientRects()`/`getBoxQuads()` had nothing else to read, so they
rode the same union table through the same fallback.

## Исправление 2026-09-05 (P1, ветка `p1-gapgeom2-per-fragment-rects`)

New parallel table, `client_rects: HashMap<u32, Vec<[f32; 4]>>`, plumbed
alongside `layout_rects` end to end:

- `lumen_layout::collect_client_rects` (`crates/engine/layout/src/lib.rs`) —
  same tree walk as `collect_layout_rects_rec`, but keeps each line's rect
  apart instead of merging them. A node with its own `LayoutBox` still gets
  exactly one rect (parity with `getBoundingClientRect`); a plain inline
  element gets one rect per line, each the union of only that line's
  `InlineFrag`s reaching it via `inline_element_ancestors`. A precomputed
  `boxed` node-id set stops an inline-block nested mid-line (still an
  `InlineFrag` there for line-height purposes, but with a real box of its
  own) from getting a duplicate approximate rect on top of its true one.
- `V8JsRuntime::client_rects` + `update_client_rects` (`v8_runtime/runtime.rs`),
  a new native `_lumen_get_client_rects(nid)` (`v8_runtime/install/platform.rs`)
  returning a `JsValue::Array` of `[x, y, w, h]` arrays (empty for a node with
  no box) — same `flush.maybe_flush()` call `_lumen_get_bounding_rect` makes,
  so a same-tick `getClientRects()` after a DOM/style mutation never
  disagrees with `getBoundingClientRect()`. `FlushHandles` (CSSOM-4/BUG-493)
  recomputes `client_rects` in the same flush as `layout_rects`.
- `PersistentJs::update_client_rects` (`crates/shell/src/persistent_js.rs`) —
  new trait method, pushed alongside `update_layout_rects` at every one of
  its six shell call sites (`page_pipeline.rs`, `page_load.rs` ×3,
  `relayout.rs`, `frames.rs`, `lumen/hibernation.rs`, `scripts.rs`'s
  parse-time push) plus the separate `InProcessSession` path
  (`crates/driver/src/session.rs::commit_layout`).
- Shim (`web_api_shim_mid.js`): `getClientRects()`/`getBoxQuads()` now call
  `_lumen_get_client_rects(nid)` and map each `[x,y,w,h]` to its own
  `DOMRect`/`DOMQuad` instead of wrapping a single `getBoundingClientRect()`.

**Вне scope:** `Range.prototype.getClientRects()`/`_CaretPosition.prototype
.getClientRects()` stay on the single-rect fallback — a `Range` boundary can
sit mid-text-node, and there is no existing layout API mapping an arbitrary
text offset to a rect (a materially bigger, separate task); `getBoxQuads()`'s
`options.box` parameter (border/padding/content/margin variants) — every rect
answers border-box geometry, same as before this fix.

`cargo test -p lumen-layout --profile dev-release` (3 new tests:
`client_rects_single_rect_for_block_owning_own_box`,
`client_rects_multi_line_span_gets_one_rect_per_line`,
`client_rects_does_not_duplicate_boxed_inline_block_nested_in_line`),
`cargo test -p lumen-js --features v8-backend --profile dev-release`
(3 new tests in `v8_elem_geometry_scroll.rs`), `cargo clippy -p lumen-layout
-p lumen-js -p lumen-shell -p lumen-driver --all-targets --features
"v8-backend v8" -- -D warnings`.

# BUG-969: `srcset`'s `w`-descriptor density correction is never applied —
`<img>` sized via `sizes`+`srcset` uses the raw decoded bitmap size instead

**Статус:** OPEN
**Дата:** 2026-09-03
**Компонент:** html-parser (`crates/engine/html-parser/src/picture.rs::pick_from_srcset`)
**Найден:** P2, WPT-RUN-6 срез 55, живой пробой

## Механизм

HTML LS §4.8.4.3.7 "pixel density descriptors" (the `Nw` form) picks a
`srcset` candidate by computing `effective density = width_descriptor /
source_size` (`pick_best_for_width`,
`crates/engine/html-parser/src/srcset.rs:717-757` — this half is correct and
tested). What the spec does with the *selected* candidate afterwards is the
part that's missing: the candidate's *used* natural size for CSS layout
purposes is not its raw decoded bitmap size, but that size **divided by the
same effective density** — i.e. density-corrected back into the `source_size`
the `sizes` attribute (or its `100vw` default) actually asked for. For an
image with a single `100w` candidate and `sizes="400px"`, effective density
is `100/400 = 0.25`, so the density-corrected natural width is `100 / 0.25 =
400` — the whole point of the feature: pick a small file, but lay it out at
the size the author intended.

`pick_from_srcset` (`crates/engine/html-parser/src/picture.rs:252-279`)
computes `source_size_px` (line 266-269) purely to feed `pick_best_for_width`
for candidate *selection*, then **discards it**:

```rust
picked.map(|c| PickedSource {
    url: c.url.clone(),
    intrinsic_width: None,
    intrinsic_height: None,
});
```

`intrinsic_width`/`intrinsic_height` are always `None` out of this function,
regardless of which descriptor form matched. The caller,
`fill_intrinsic_dims` (`picture.rs:205-216`), only ever fills them back in
from the element's own `width`/`height` *attributes* (a completely different,
author-declared hint) — never from the resolved `source_size_px`. So the
picker correctly chooses *which* URL to fetch, but the resulting `<img>`'s
used width (when CSS gives it `width: auto`, as any responsive-images markup
does) falls through to whatever the decoded bitmap's actual pixel width is —
the density correction step of the algorithm simply does not exist anywhere
in the pipeline.

## Симптом

Confirmed live (`--mcp-live-port`, `tests/wpt/verify_slice55_gaps.py`,
2026-09-03, `main` = `d05d62fd2`):

```
harness-complete status=2 tests=1
  Implicit sizes ignores width:1   (FAIL, but see below)
```

Page: `<img srcset="…/image.png 100w" sizes="400px">` (real 100×100 PNG,
confirmed by the "Загружена картинка: …(100×100, Rgb8)" log line) with CSS
`img { width: auto }`. `document.getElementById("sizes").width` is expected
to read back `400`; it does not (the assertion throws before the test calls
its own `done()`). The harness reports its own overall status as TIMEOUT (`2`)
rather than a clean FAIL, because the test page uses `setup({explicit_done:
true})` and calls `done()` only after both assertions — the second `<img
width="400">` case is never even reached, so `done()` is never called and
`testharnessreport.js` only reports once *its own* internal budget elapses.
This is a distinct, generalizable idiom worth naming: **an
`explicit_done`-mode test whose synchronous `assert_*` throws before its own
`done()` call reports as a harness TIMEOUT, not a FAIL**, even though the
individual subtest's own status is already recorded as FAIL — the same shape
as `docs/probe-method.md`'s existing note about `t.step`-unwrapped listeners,
but for the *harness-level* completion signal rather than a subtest's.

Real-world trigger:
`/html/semantics/embedded-content/the-img-element/sizes/implicit-sizes-ignores-width.html`
— matches the WPT-RUN-5/6 corpus TIMEOUT signature for this id.

## Масштаб

Every `srcset`+`sizes` page using `Nw` descriptors (the common form — density
descriptors `Nx` are the minority idiom) lays out its images at the wrong
size whenever CSS leaves the width to `auto`: too small if the picked
candidate's own pixel width is smaller than the requested display size (the
common "serve a small file, display it big on a wide viewport" case), too
large in the opposite case. This is a layout-correctness gap independent of
the specific WPT id it was found through.

## Что нужно

Return the resolved `source_size_px` (or the computed effective density) out
of `pick_from_srcset`/`try_pick_source` alongside the URL, and use it in
`fill_intrinsic_dims` — or a sibling step run after it — to set
`intrinsic_width`/`intrinsic_height` to `decoded_width / effective_density`
once the resource is actually decoded (the value isn't knowable before that,
since `effective_density` depends on the descriptor value which is only a
declared *hint*; but the used width for `width: auto` layout is the
density-corrected one, not the raw decoded one). Needs a layout-side plumbing
change too, since `image_requests.rs` currently just forwards whatever
`intrinsic_width` the picker returned straight into the box tree.

## Классификация WPT-RUN-6

Attributed via
`_exact_id_marker("/html/semantics/embedded-content/the-img-element/sizes/implicit-sizes-ignores-width.html")`
in `tests/wpt/timeout_audit.py` (marker `srcset-density-correction-missing`).

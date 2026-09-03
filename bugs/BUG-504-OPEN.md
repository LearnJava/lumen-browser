# BUG-504: scrollable-overflow computation (`content_width`/`content_height`)
ignores CSS `transform` on children entirely, and is wrong in several other
css-overflow scenarios (abspos, clip-margin, RTL/logical axes)

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** layout (`crates/engine/layout/src/lib.rs::content_width`/`content_height`)
**Найден:** WPT-RUN-3 срез 11 (`ROADMAP.md`) — массовый прогон `css/css-overflow`

## Механизм

`content_width`/`content_height` (`crates/engine/layout/src/lib.rs:1200-1216`)
compute `scrollWidth`/`scrollHeight` as `max(own rect size, farthest child
edge)`, where "child edge" is read straight off each child's post-layout
`LayoutBox::rect` — the box's static in-flow geometry:

```rust
fn content_width(b: &LayoutBox) -> f32 {
    b.children.iter().fold(b.rect.width, |acc, c| {
        let c_right = c.rect.x + c.rect.width - b.rect.x;
        acc.max(c_right)
    })
}
```

`rect` never reflects a CSS `transform` — Lumen (correctly) treats `transform`
as a paint-time-only visual effect that doesn't move boxes for the purpose of
layout flow. But per the CSS Overflow spec (`css-overflow-3/#scrollable`),
`transform` **does** contribute to the *scrollable* overflow rectangle
specifically — a translated/rotated/scaled child's painted (post-transform)
bounding box is what determines whether a scroll container needs to grow its
scroll range, even though the child's own flow position and the size it
occupies in normal layout are unaffected. `content_width`/`content_height`
never apply the child's `transform` when computing this box, so any content
that only extends beyond the container via a transform is invisible to
`scrollWidth`/`scrollHeight` and reports plain `0` (no overflow) instead of
the transformed extent.

Confirmed directly:

```html
<div style="width:100px;height:100px;overflow:auto" id=c>
  <div style="width:50px;height:50px;transform:translateX(200px)"></div>
</div>
<script>c.scrollWidth /* spec: 250, Lumen: 100 (no overflow detected) */</script>
```

Live `--mcp-live-port` probe on this exact snippet returns `scrollWidth: 0`
above the container's own size — i.e. `content_width` falls back to its
`b.rect.width` floor because the untransformed child fits entirely inside the
100×100 box; the transform is simply never consulted.

## Масштаб находки

20 files in `css/css-overflow`, split into two groups by confidence:

**Verified root cause (transform):** `overflow-abpos-transform.html`,
`overflow-inline-transform-relative.html`,
`single-axis-scroll-apis-dynamic.html`,
`scrollable-overflow-transform-unreachable-region.html`,
`scrollable-overflow-transform-dynamic-{001..006}.html`,
`scrollable-overflow-height-with-flex-item-margin-inline-end{,-rtl}.html`,
`scrollable-overflow-with-{flex,grid}-item-margin-inline-end.html` — all
report `scrollWidth`/`scrollHeight` (or a derived `container`/`container1`/
`container2` reading, once [BUG-384](BUG-384-FIXED.md) is separately fixed) as
`0` where a positive value is expected, and every one of these tests exercises
a `transform` on the overflowing child.

**Same symptom, not yet individually root-caused (list for follow-up, may be
this bug or a sibling one — abspos-without-transform, clip-margin RTL,
scrollbar-gutter space reservation, single-axis clamping all return the same
"`scrollWidth`/`scrollHeight` reads `0`/`undefined` where a positive number is
expected" shape but haven't each been traced to a specific missing
contribution):
`overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html`,
`overflow-outside-padding.html`, `overflow-clip-scroll-size.html`,
`overflow-rtl-scroll-left.html`, `single-axis-scroll-into-view{,-rtl}.html`,
`single-axis-scroll-apis-programmatic.html`,
`scrollbar-gutter-{001,rtl-001,vertical-lr-001,vertical-rl-001}.html`,
`scrollbar-gutter-propagation-{001,002,003,007}.html`.

Once `scrollable-overflow-transform-*`/`scrollable-overflow-with-nested-
elements-*` (currently masked by [BUG-360](BUG-360-FIXED.md), body `onload`
never firing) start actually running `checkLayout`, most of that family will
also land on this bug.

## Что нужно

Extend `content_width`/`content_height` (or their caller) to fold in each
child's rendered/painted bounding box under its accumulated `transform`
(matrix-transform the child's border-box corners, not just translate) when
computing scrollable overflow, while leaving the child's own `rect` (flow
position/size) untouched — the two concerns (flow geometry vs. scrollable
overflow) need to stay separate per spec. The abspos/RTL/clip-margin cluster
needs its own investigation once this lands — some of those may turn out to
be the same fix (e.g. if abspos boxes aren't walked into `content_width` at
all yet), others may be genuinely separate.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-overflow/` for the 20 files
above, `expected: FAIL` per affected subtest.

## Срез 2026-09-03 (P3): transform contribution landed

Implemented exactly the "Что нужно" fix above for the **verified root cause
(transform)** group. `content_width`/`content_height`
(`crates/engine/layout/src/lib.rs`) now consult a new helper,
`child_scrollable_bounds`, which returns a child's border-box corners after
applying its forward transform matrix (`forward_box_transform`, the same
matrix paint uses to emit `PushTransform`) when the child carries one, or the
plain `c.rect` unchanged otherwise (zero-cost fast path for the untransformed
common case — the vast majority of boxes). `LayoutBox::rect` itself is left
untouched, so flow geometry is unaffected; only the scrollable-overflow fold
changes. Confirmed against the exact repro snippet from this file's
"Механизм" section (`translateX(200px)` on a 50×50 child in a 100×100
`overflow:auto` container) via new regression tests
(`collect_scroll_containers_transform_grows_scroll_width`/`_height` in
`crates/engine/layout/src/tests/scroll_interaction_misc.rs`) — `scrollWidth`
now reports `250` (was `0` above the container's own size), matching the
spec value from the snippet's comment. A third regression test
(`collect_scroll_containers_no_transform_unaffected`) guards the
untransformed fast path against regressing to the old plain-`rect` numbers.
`cargo test -p lumen-layout`: 3683/3683 (workspace-wide, not just the new
tests) — no existing geometry test shifted, confirming the fast path is
truly a no-op for untransformed children. This lands via `collect_scroll_containers`
(consumed by both `scrollWidth`/`scrollHeight`'s JS getters via
`_lumen_get_scroll_state` and `set_scroll_position`'s clamp range), so both
read and scroll-clamp surfaces pick up the fix together.

**Not attempted — remains the bug's open scope:** the **"same symptom, not
yet individually root-caused"** group (11 files: abspos-without-transform,
clip-margin RTL, scrollbar-gutter space reservation, single-axis clamping).
None of these involve a child `transform`, so this slice's fix does not
touch them; each still needs its own root-cause pass per the file's original
"Масштаб находки" split. Status remains `OPEN` — only the transform
component of this bug's original 20-file finding is closed.

## Срез 2026-09-03 (P3, часть 2): `overflow-outside-padding.html` root-caused and fixed

Root-caused the first file of the "not yet individually root-caused" group
above. `content_width`/`content_height` (`crates/engine/layout/src/lib.rs`)
had two independent defects, both exposed by this file's asymmetric-border,
abspos-heavy layout:

1. **Floor used the border-box, not the padding box.** `scrollWidth`/
   `scrollHeight` are defined (CSS Overflow L3 §3.3) relative to the padding
   edge, but the floor was plain `b.rect.width`/`height` (border-box,
   `LayoutBox::rect`'s documented contract). A container with a non-zero
   border (this test's `.container` has `border-width: 0 0 50px 80px`) has a
   border-box strictly larger than its padding-box, so `scrollWidth` read
   280 instead of the spec's 200 even with *no* overflowing content at all.
2. **Absolutely/fixed positioned descendants that land wholly outside the
   padding edges still contributed in full**, instead of being excluded per
   CSS Overflow L3 §3.3 ("blocks wholly outside padding edges should not
   contribute to overflow"). The test's six `.target` boxes (`position:
   absolute; width/height: 1000px`) are each pushed via a single physical
   inset (`top`/`right`/`bottom`/`left: -1000px`) to sit just past one edge
   of a 200×200 container — touching it, not inside it. Because
   `content_width`/`content_height` folded in every child's bounds
   unconditionally, each of these boxes blew `scrollWidth` up to ~1000+px.

Fix: a new shared `padding_box(b)` helper (also now used by
`collect_scroll_containers_inner`'s `clip_rect`, replacing its inline
border-subtraction so the floor and the viewport rect can never drift apart
again) replaces the border-box floor and origin in both functions. A new
`contributes_to_scrollable_overflow(child, bounds, padding_box)` gate skips a
child entirely (not just clamps it) when **and only when** it is
`position: absolute`/`fixed` *and* its (transform-adjusted) bounds have zero
overlap with the padding box on the X axis, the Y axis, or both
(`rects_overlap`, strict inequalities — touching at a boundary counts as no
overlap, matching this test's boxes landing exactly on the padding edge).
The gate is conditioned on the child's `position` specifically because CSS
Overflow L3 §3.4 makes the opposite rule for `transform`: an in-flow box
pushed entirely outside by `transform` must still count in full (the
transform-contribution fix from part 1 above, and its two regression tests,
are deliberately left unconditional and unaffected by this slice —
re-verified: both still pass unchanged).

Regression tests added to `crates/engine/layout/src/tests/scroll_interaction_misc.rs`:
`collect_scroll_containers_abspos_wholly_outside_padding_excluded` (mirrors
this file's exact repro: 1000×1000 abspos child at `top: -1000px` inside a
100×100 container — `scrollWidth`/`scrollHeight` must stay at 100, not grow
from the child's horizontal overlap alone), `_abspos_overlapping_child_still_contributes`
(guard: an abspos child overlapping on both axes must still grow
`scrollWidth` normally), and `_scroll_width_floor_is_padding_box_not_border_box`
(200px content + 80px left border → `scrollWidth` must read 200, not 280).
`cargo test -p lumen-layout`: 3686/3686 (workspace's existing 3683 unaffected,
including the two transform-contribution tests from part 1).

**Live WPT verification not performed this slice** — the sandbox this
session ran in refused to bind the `--mcp-port` TCP socket (`os error
10013`, access denied) needed to drive a headless probe, so the fix is
verified by unit tests that reproduce the exact geometry (offsets, border
widths, expected `scrollWidth` values) of the WPT file's six subtests, not
by a live run of the file itself.

**Remaining scope unchanged:** 10 files (abspos-without-transform is now
closed; clip-margin RTL, scrollbar-gutter space reservation, single-axis
clamping remain) — each still needs its own root-cause pass. Status stays
`OPEN`.

## Срез 2026-09-03 (P3, часть 3): `scrollbar-gutter-001.html` root-caused and fixed

Root-caused the "scrollbar-gutter space reservation" component of the
remaining group. `scrollbar_gutter_inline`/`scrollbar_gutter_block`
(`crates/engine/layout/src/box_tree/predicates.rs`) had two independent
defects, both exposed by `css/css-overflow/scrollbar-gutter-001.html`'s 15
overflow×gutter combinations:

1. **`overflow-y`/`overflow-x: hidden` was never eligible for `stable`
   reservation.** Both functions only matched `Scroll | Auto`, but
   `hidden` still establishes a scroll container per CSS Overflow L3 §3.3
   (programmatically scrollable via script even though the UA never paints
   a scrollbar for it) — `scrollbar-gutter: stable` must reserve its gutter
   the same as `scroll`/`auto`. Fix: both match arms now include
   `Overflow::Hidden`. `visible`/`clip` stay excluded (`visible` never
   establishes a scroll container; `clip` explicitly disables the scrolling
   machinery outright, CSS Overflow L3 §3.4, so neither can ever show a
   scrollbar).
2. **`stable both-edges` only narrowed the content box, never shifted it.**
   `scrollbar_gutter_inline` already subtracted `2 × unit` from
   `content_width` for `StableBothEdges`, but `content_x` (`lay_out`'s
   local content-box origin, `crates/engine/layout/src/box_tree/layout_dispatch.rs`)
   was never offset — children stayed flush against the same inline-start
   edge as plain `stable`'s end-edge-only reservation, instead of starting
   one gutter unit further in as the spec's "mirrored" gutter requires.
   Fix: new `scrollbar_gutter_inline_start` helper (returns one unit for
   `StableBothEdges`, `0.0` for `Stable`/`Auto`) added into `content_x`'s
   computation.

6 new/updated regression tests in `scroll_interaction_misc.rs`'s sibling
`filter_transform_snap_mask.rs` module (the crate's existing home for
`scrollbar_gutter_*` unit tests): `..._overflow_hidden` (width, both axes),
`..._no_reduction_overflow_{visible,clip}` (negative guards),
`..._both_edges_shifts_child_start_edge`, `..._single_edge_no_start_shift`.
`cargo test -p lumen-layout`: 3692/3692.

**Live WPT verification:** `tests/wpt/run_smoke.py` against
`scrollbar-gutter-001.html` went from 11/15 subtests passing (4 genuine
fails) to **14/15** — the 6 subtests the two defects above accounted for
(`overflow {auto,scroll,hidden}, scrollbar-gutter stable` +
`… stable both-edges`) all now pass; `.ini` updated to drop those 6
`expected: FAIL` entries (from 7 down to 1). The one remaining failure,
`overflow scroll, scrollbar-gutter auto`, is **not** this bug's gap — it's
Lumen's overlay-scrollbar architecture (`scrollbar_gutter_inline`'s own doc
comment): with `scrollbar-gutter: auto`, Lumen's overlay scrollbar never
consumes layout space regardless of `overflow-y: scroll`'s persistent
scrollbar, so `content.offsetWidth == container.offsetWidth` by design,
whereas the WPT assertion assumes a classic (space-consuming) OS scrollbar.
Fixing that would mean abandoning overlay scrollbars project-wide — left as
`expected: FAIL`, KNOWN_DEBTOR, not a candidate for its own bug filing.

**Checked for regression, found none:** `scrollbar-gutter-rtl-001.html`,
`-vertical-lr-001.html`, `-vertical-rl-001.html` and the four
`scrollbar-gutter-propagation-{001,002,003,007}.html` files were re-run live
against the fixed binary — all still fail **exactly** the same subtests
their committed `.ini` already expects (0 unexpected in every case), so
none of them regressed or accidentally got fixed. For the vertical-writing-
mode pair specifically, the raw per-subtest log shows the *same* 7
width-based fails as before my fix (not the position-based fails
`scrollbar-gutter-001`/`-rtl-001` show) — confirming this is a **separate**,
not-yet-root-caused defect: `scrollbar_gutter_inline`/`_block` reduce
physical `content_width`/height unconditionally, never accounting for
`writing-mode: vertical-*` swapping which physical dimension is the inline
vs. block axis, so the gutter fix above never engages for `width`
declarations that are actually block-size under vertical writing modes.
`scrollbar-gutter-rtl-001.html` shows the width part now fixed but a new
distinct fail shape (`assert_less_than: content position … expected less
than N but got N`) — `scrollbar_gutter_inline_start`'s direction gate
(`Direction::Rtl` → `0.0`, this slice's deliberate scope cut) means the
start-edge shift never applies under RTL, where the physical start edge is
the right, not the left this box-tree pass assumes throughout (not just in
this helper — no existing code path in `layout_dispatch.rs` maps
inline-start to a physical side by direction at all).

**Remaining scope, revised:** 9 files — `scrollbar-gutter-rtl-001.html`,
`-vertical-lr-001.html`, `-vertical-rl-001.html`, the 4
`scrollbar-gutter-propagation-*.html` files (viewport-level gutter
propagation, untouched by this slice — separate mechanism from a single
box's own reservation) — plus the original clip-margin RTL and single-axis
clamping files, each still needing its own root-cause pass. `scrollbar-
gutter-001.html` is fully closed (bar the DEBTOR subtest). Status stays
`OPEN`.

## Срез 2026-09-03 (P3, часть 4): `scrollbar-gutter-rtl-001.html` root-caused and fixed

Root-caused the RTL component flagged as out-of-scope at the end of part 3.
`scrollbar_gutter_inline_start` (`crates/engine/layout/src/box_tree/predicates.rs`)
bailed to `0.0` for **any** `direction: rtl` container regardless of
`scrollbar-gutter` value — a blanket scope cut, not a considered case split.
The two `scrollbar-gutter` values actually behave differently under RTL:

- **`stable` (single-edge):** in LTR the gutter sits on the physical right
  (inline-end there), so the physical-left origin never moves — only
  `content_width` narrows. Under RTL, inline-end is the physical *left*, so
  the gutter now sits exactly where children start from: the whole content
  box must shift right by the **full** unit, not stay put. The old
  RTL-blanket-zero made this case (and only this one) genuinely wrong.
- **`stable both-edges`:** reserves a gutter on *both* physical sides
  symmetrically, so the start-edge shift (half the total narrowing, one
  unit) is the same physical amount regardless of which logical edge is
  "start" — this case never needed a direction branch at all; the RTL
  bail-out was overly broad and zeroed out a shift that should have been
  identical to the already-correct LTR one.

Fix: `scrollbar_gutter_inline_start` now matches on `scrollbar_gutter`
directly — `StableBothEdges` keeps the old direction-independent
`scrollbar_gutter_inline(s) / 2.0`; `Stable` returns `0.0` in LTR (unchanged)
and the **full** `scrollbar_gutter_inline(s)` in RTL; anything else (`Auto`)
stays `0.0`. `content_width`'s narrowing itself was already direction-
independent (`scrollbar_gutter_inline` never branched on `direction`), so
only the start-edge shift needed the fix — confirmed by a live
`--dump-layout` of a two-container repro (`direction:rtl`, one `stable`, one
`stable both-edges`, both `width:200px` `overflow-y:scroll`): both containers'
content box now starts at `container.x + 12` (was `container.x + 0` before
the fix, verified by re-running the same dump against the pre-fix binary),
matching the WPT file's `assert_less_than(container.offsetLeft,
content.offsetLeft)` shape for both gutter values.

Two new regression tests in `scroll_interaction_misc.rs`'s sibling
`filter_transform_snap_mask.rs`: `scrollbar_gutter_stable_rtl_shifts_child_start_edge`
(mirrors the existing LTR `..._single_edge_no_start_shift` negative case, but
under `direction: rtl` asserts the shift now happens) and
`scrollbar_gutter_stable_both_edges_rtl_shifts_child_start_edge` (same
shift amount as the existing LTR both-edges test, guarding the direction-
independent branch against a future regression). `cargo test -p
lumen-layout`: 3696/3696 (existing 3694 unaffected, both new tests pass);
`cargo clippy -p lumen-layout --all-targets -- -D warnings` clean.

**Live WPT verification not performed this slice** — no `tests/wpt/.venv`
exists in this session's worktree (a fresh pool slot, not the one the
`os error 10013` sandbox limitation from part 2 was reported in) and
provisioning one was out of scope for a single-file fix. Verified instead by
(1) the two unit tests above reproducing the exact geometry the WPT
assertions check, and (2) a live `--dump-layout` render of a repro mirroring
the WPT file's container structure, both showing the predicted `container.x
+ 12` shift. `.ini` updated analytically: of the 7 `expected: FAIL` entries,
6 (`stable`/`stable both-edges` × `auto`/`scroll`/`hidden` overflow) are
predicted PASS now that both assertions each subtest makes (width narrowing,
which already worked, and position shift, which this slice fixes) hold; the
7th (`overflow scroll, scrollbar-gutter auto`) is untouched by this slice —
same overlay-scrollbar architecture DEBTOR already carried by the LTR file
(`scrollbar-gutter-001.html`'s own remaining failure), not a direction-
specific gap. A future session with a working WPT venv should re-run this
file to confirm the analytical prediction rather than re-deriving it.

**Remaining scope, revised:** 8 files — `-vertical-lr-001.html`,
`-vertical-rl-001.html`, the 4 `scrollbar-gutter-propagation-*.html` files,
plus the original clip-margin RTL and single-axis clamping files.
`scrollbar-gutter-rtl-001.html` is now closed (bar the same DEBTOR subtest
class as the LTR file). Status stays `OPEN`.

## Срез 2026-09-03 (P3, часть 5): `scrollbar-gutter-vertical-{lr,rl}-001.html` root-caused and fixed

Root-caused the vertical-writing-mode component flagged at the end of part 3.
`scrollbar_gutter_inline`/`scrollbar_gutter_block` (`box_tree/predicates.rs`)
reduce physical `content_width`/`content_height` unconditionally, but
`lay_out_vertical_block` (`vertical.rs`, the layout path for
`writing-mode: vertical-lr`/`vertical-rl`) never consulted either helper at
all — under a vertical writing mode the **block** axis is physically
horizontal (lines stack left-to-right) and the **inline** axis is physically
vertical, so it is `scrollbar_gutter_block`/`_block_start` (keyed on
`overflow-x`, matching a horizontal-scrollbar reservation of *height*) that
apply there, not the `_inline` pair the horizontal-writing-mode path uses —
and `vertical.rs` called neither.

Fix: `pub(crate) use` promotion of `scrollbar_gutter_block`/
`scrollbar_gutter_block_start` out of `box_tree`'s private `mod predicates`
(needed since `vertical.rs` is a crate-root sibling module, not a
`box_tree` submodule), then `lay_out_vertical_block`'s `content_inline`
(physical height handed to children) and `content_y` (physical top origin)
now subtract `scrollbar_gutter_block(&s)` / add
`scrollbar_gutter_block_start(&s)` respectively — mirroring exactly how
`children_available_height`/`content_x` already consult the `_inline` pair
on the horizontal-writing-mode path. `scrollbar_gutter_block` itself also
had its own pre-existing `both-edges` bug fixed in this slice: it previously
returned a single `unit` for `StableBothEdges` (comment claimed
"`both-edges` is not defined for the block axis"), but the two vertical WPT
files' `assert_less_than(content.offsetHeight, reference.offsetHeight)`
subtest requires the doubled `2 × unit` reservation, same as the inline
axis — the single-reduction reading was wrong per spec, not an intentional
axis asymmetry. A sibling `scrollbar_gutter_block_start` helper (mirrors
`scrollbar_gutter_inline_start`) was added for the `both-edges` top-edge
shift; unlike its inline counterpart it has no `direction` branch yet since
neither vertical WPT file here exercises `direction: rtl`.

Confirmed live via `--dump-layout` on a two-container repro (`writing-mode:
vertical-lr`, one `overflow-x:auto; scrollbar-gutter:stable`, one `…:stable
both-edges`, both `height:200px`): plain `stable` child height 200→188
(unit=12) with child.y unchanged from container.y (no shift, correct for
single-edge reservation); `both-edges` child height 200→176 (2×unit) with
child.y = container.y + 12 (shifted, correct for symmetric reservation).
5 new regression tests in `vertical.rs`'s own `tests` module
(`vertical_lr_scrollbar_gutter_stable_reduces_child_inline_size`,
`vertical_lr_scrollbar_gutter_stable_both_edges_shifts_and_double_reduces`,
`vertical_rl_scrollbar_gutter_stable_both_edges_matches_vertical_lr`,
`vertical_lr_scrollbar_gutter_auto_no_reduction`) plus a corrected existing
`filter_transform_snap_mask.rs` test
(`scrollbar_gutter_block_both_edges_double_reduction`, was asserting the old
wrong single-reduction number). `cargo test -p lumen-layout`: 3698/3698;
`cargo clippy -p lumen-layout --all-targets -- -D warnings` clean.

**Live WPT verification not performed this slice** — no `tests/wpt/.venv`
in this pool slot (same gap as part 4). `.ini` for both files updated
analytically: 6 of 7 `expected: FAIL` entries predicted PASS (all four
`overflow {auto,scroll,hidden}` values crossed with `stable`/`stable
both-edges`); the 7th (`overflow scroll, scrollbar-gutter auto`) is
untouched — same overlay-scrollbar architecture DEBTOR already carried by
the LTR/RTL files. `vertical-lr-001.html` and `vertical-rl-001.html` share
byte-identical test bodies (only `writing-mode` and the title differ, `diff`
confirmed), so both `.ini` files get the same prediction.

**Remaining scope, revised:** 6 files — the 4
`scrollbar-gutter-propagation-*.html` files (viewport-level gutter
propagation, a separate mechanism from a single box's own reservation, still
untouched by any slice), plus the original clip-margin RTL and single-axis
clamping files. `scrollbar-gutter-vertical-{lr,rl}-001.html` are now closed
(bar the same DEBTOR subtest class as the other two files). Status stays
`OPEN`.

## Срез 2026-09-03 (P3, часть 6): `overflow-rtl-scroll-left.html` root-caused and fixed — not a content_width defect

Root-caused `overflow-rtl-scroll-left.html` from the "clip-margin RTL/single-axis
clamping" remainder. **This one was not `content_width`'s fault at all** — a
direct `lumen_layout` unit probe (`collect_scroll_containers` on the exact
`direction: rtl; width: 300px` container with a `width: 500px` child)
returned `scroll_width == 500` correctly, matching CSS Overflow L3's spec
value, both before and after this slice. The actual defect was one level up,
in the shell's JS-state seeding: `apply_loaded_page`
(`crates/shell/src/page_load.rs`) is where BUG-382 (2026-07) fixed
`getBoundingClientRect()`/`getComputedStyle()` answering `""`/all-zeros on a
freshly loaded page by pushing `update_layout_rects`/`update_computed_styles`
unconditionally right after layout, before any page script can run — but
that fix's push closure never included `update_scroll_states`, so
`scrollWidth`/`scrollHeight`/`scrollTop`/`scrollLeft` (which read
`_lumen_get_scroll_state`'s per-node cache, `web_api_shim_mid.js`) kept
answering the fallback border-box size (`_lumen_get_bounding_rect`, 300 for
this container) until an unrelated relayout (resize, DOM mutation, a wheel
scroll) happened to race ahead of the first script and populate the cache —
exactly BUG-382's "works in one load out of four" shape, just for the scroll
half of the geometry push instead of the rects/styles half. `relayout()`
(`crates/shell/src/relayout.rs`) and the wheel-scroll path
(`try_scroll_overflow_container`, `crates/shell/src/lumen/scrolling.rs`)
already called `update_scroll_states` correctly; only the initial-load seed
site was missing it.

**Found via:** a live `--mcp-live-port` probe reading `scrollWidth` from a
fresh navigation with `wait: stable` and no prior mutation consistently
returned `300` (the container's own border-box width) across 10 retries with
0.5s spacing — ruling out a race and pointing at a missing push rather than a
late one. Cross-checked against a `lumen_layout`-only unit test (no shell, no
JS) confirming `content_width`/`collect_scroll_containers` compute the
correct `500` from the post-layout box tree, isolating the defect to the
shell↔JS wiring.

**Fix:** `apply_loaded_page`'s existing JS-seed block (`page_load.rs`, the
`#[cfg(feature = "v8")]` block already collecting rects/styles per BUG-382)
now also computes `scroll_states` via the same
`collect_scroll_containers(lb_ref)` → `(node.index(), [scroll_x, scroll_y,
scroll_width, scroll_height])` mapping `relayout()` already uses, and passes
it to `js.update_scroll_states(scroll_states)` in the same `route_task_js`
closure — so scroll geometry is seeded atomically with rects/styles/viewport
on every page load, not just after the next unrelated relayout.

**Regression coverage:** `collect_scroll_containers_rtl_overflow_grows_scroll_width`
(`crates/engine/layout/src/tests/scroll_interaction_misc.rs`) locks in the
correct `content_width` RTL computation itself (guards against a future
regression in the layout half, even though it was never the bug this slice
found). The shell-wiring fix (the actual defect) has no unit-level regression
test — `apply_loaded_page` has no existing mock-`PersistentJs` test harness
in `crates/shell/src/tests/`, and building one from scratch for a single
push-site was judged out of proportion to this slice; verified instead by a
live `--mcp-live-port` before/after (`scrollWidth` 300→500 immediately after
`wait: stable`, no mutation) and a full run of the WPT file's own assertions
via `eval` (`offsetWidth`/`offsetHeight`/`scrollWidth`/`scrollHeight`/
`scrollLeft` before and after the `height: 0px` mutation — all match the
file's `assert_equals` expectations). `cargo test -p lumen-layout --lib`:
3712/3712 passed, 1 ignored (pre-existing), 0 failed. `cargo clippy -p
lumen-shell --all-targets -- -D warnings`: clean.

`.ini` deleted (`tests/wpt/metadata/css/css-overflow/overflow-rtl-scroll-left.html.ini`)
— the file's single `test()` now fully passes, no `expected: FAIL` entries
remain.

**Remaining scope, revised:** 5 files — the 4
`scrollbar-gutter-propagation-*.html` files (still blocked on a *different*
bug: all four read `window.innerWidth`/`outerWidth`, which do not exist in
the engine at all — [BUG-529](BUG-529-OPEN.md) — so they cannot pass
regardless of any scrollbar-gutter fix; verified via `grep -rn "innerWidth"
crates/js/src/shim/*.js` returning zero matches), plus
`overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html` (a
genuinely separate, deeper defect: it needs `scrollLeft` to accept and report
*negative* values under `overflow: hidden` + `writing-mode: vertical-rl`,
but `set_scroll_position`'s clamp — `x.clamp(0.0, (sw - clip_w).max(0.0))`,
`crates/engine/layout/src/lib.rs` — has `0.0` hardcoded as the universal
floor, no direction/writing-mode-aware negative range; out of this slice's
scope, needs its own design pass). Two files from the original "same
symptom" list were re-examined and found to likely share the seeding-gap
root cause just fixed here (`overflow-clip-scroll-size.html`'s initial
`scrollWidth` reads, `single-axis-scroll-apis-programmatic.html`'s pre-scroll
assertions) but were not individually re-verified this slice — a future
session should re-run them live before assuming they are closed, since
`overflow-clip-scroll-size.html` in particular uses `overflow: clip`, which
`collect_scroll_containers_inner` still excludes from `is_scroll_x`/
`is_scroll_y` entirely (only `Scroll | Auto | Hidden` register), so its
`scrollWidth` reads may hit a **different**, still-open gap: CSSOM View
defines `scrollWidth`/`scrollHeight` for every element as `max(padding-box,
scrollable-overflow-extent)` regardless of the `overflow` value, but the
shim's fallback path (`_lumen_get_bounding_rect`) only ever returns the
border-box, never consulting `content_width`/`content_height` for a
non-registered container.

## Срез 2026-09-03 (P3, часть 7): negative scroll range + `overflow: clip` — engine half landed, JS-visible half still blocked

Took the "own design pass" the previous slice deferred, for the exact
`overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html` repro.
**Correction to part 3's remaining-scope note above:** it claimed
`collect_scroll_containers_inner`'s gate was `Scroll | Auto | Hidden` —
re-checked against the live code this slice, it is still `Scroll | Auto`
only (`crates/engine/layout/src/lib.rs`); `Hidden` was never added there.
Part 3's own fix (`overflow-y`/`-x: hidden` eligible for `stable` gutter
reservation) landed in the *sibling* function `scrollbar_gutter_inline`/
`_block` (`box_tree/predicates.rs`), not this one — the note conflated the
two.

**Root cause of the negative-range gap:** `content_width`/`content_height`
(`crates/engine/layout/src/lib.rs`) only ever folded in a child's *right*/
*bottom* edge (`bounds.x + bounds.width - pb.x`) — there was no code path
that could grow the scrollable-overflow region to the *left*/*above* the
padding edge at all. Confirmed via `--dump-layout` on the WPT file's exact
markup: under `writing-mode: vertical-rl` the vertical-writing-mode block
layout path positions an only in-flow child flush with the container's
*right* padding edge (block-start is physically on the right for
`vertical-rl`) and lets it extend left — a 300px child in a 100px container
sits at `x = -191` relative to an 8px-offset ancestor, i.e. entirely left of
the padding box. The old formula's `bounds.x + bounds.width - pb.x` for this
child evaluates to `100` (no rightward overflow at all), so
`content_width` returned exactly `pb.width` (100) — zero detected overflow —
and every scroll clamp collapsed to `[0, 0]`.

**Fix:** `content_width`/`content_height` are now built on top of two new
functions, `scrollable_extent_x`/`scrollable_extent_y`, which track a
`(min, max)` pair relative to the padding edge instead of a single `max` —
`min` starts at `0.0` and is pulled negative by any contributing child whose
bounds start left of/above the padding edge, exactly mirroring the existing
`max` logic on the other side. `content_width`/`content_height` reduce this
to the old single-number magnitude (`max - min`) for their existing callers
(`ScrollContainer.scroll_width`/`height`, i.e. JS `scrollWidth`/
`scrollHeight`), so both stay backward-compatible generalizations — in the
common all-rightward-overflow case `min` stays `0.0` and the numbers are
byte-identical to before (confirmed by the full `cargo test -p lumen-layout`
run below not shifting a single pre-existing assertion).
`set_scroll_position` now clamps directly against `(min_x, max_x - clip_w)`/
`(min_y, max_y - clip_h)` instead of `(0.0, sw - clip_w)`, so a scroll
container whose content extends left/up can accept a negative offset there
too.

**Two more defects fixed in the same function, found while touching it:**
1. `set_scroll_position`'s `clip_w`/`clip_h` read `root.rect.width`/`height`
   directly — the **border**-box size — while `content_width`/`content_height`
   (since part 2 of this bug) are computed relative to the **padding** box.
   A container with a nonzero border therefore had its maximum scroll offset
   clamped `2 × border` short of the true value (border ≥ half the overflow
   could even clamp the max *below* the min, rejecting every scroll request
   outright). Fixed by computing `padding_box(root)` once and using its
   `width`/`height` for both axes' upper bound.
2. `overflow: clip` was never distinguished from `hidden`/`scroll`/`auto` by
   this function at all — CSS Overflow L3 §3.4 says `clip` "disables the
   scrolling machinery outright", so a `clip` axis must report exactly `0`
   and reject every scroll request, not just clamp to a (possibly nonzero)
   range. `set_scroll_position` now checks `style.overflow_x`/`overflow_y ==
   Overflow::Clip` per axis and forces `0.0` unconditionally when so,
   bypassing the extent computation entirely for that axis.

**Regression coverage:** 5 new tests in `scroll_interaction_misc.rs`:
`set_scroll_position_vertical_rl_allows_negative_scroll_x` (mirrors the WPT
file's exact geometry — 300px child, 100px `vertical-rl` container,
`scrollTo(-40, 50)` → `(-40, 50)`; over-scroll clamps to `(-200, 200)`, the
mirror image of the plain rightward-overflow case),
`set_scroll_position_overflow_clip_forces_zero_and_rejects_writes`,
`set_scroll_position_overflow_clip_clamps_existing_offset_back_to_zero`,
`set_scroll_position_max_scroll_uses_padding_box_not_border_box` (40px
border, 300px content in a 200px container — asserts the max clamps to 100,
not the old 60). `cargo test -p lumen-layout --lib`: 3716/3716 (workspace's
existing 3711 unaffected — including the RTL/transform/abspos tests from
earlier slices of this bug, none of which shifted a single number).
`cargo clippy -p lumen-layout --all-targets -- -D warnings`: clean.

**Live WPT verification: fix confirmed correct at the engine level, but the
file still cannot pass live — a second, independent gap blocks it.** A live
`--mcp-live-port` probe against the exact WPT markup, reading `scrollLeft`/
`scrollTop` through the real JS shim (not a unit test), still returns `[0,
0]` after every step, unchanged by this slice's fix. Root cause isolated:
`update_scroll_states`'s payload (what `_lumen_get_scroll_state` — and
therefore the `scrollLeft`/`scrollTop`/`scrollWidth`/`scrollHeight` getters —
actually reads) is built from `collect_scroll_containers`, whose `Scroll |
Auto`-only eligibility gate is *also*, and correctly, what the shell uses to
route mouse-wheel events (confirmed deliberate and tested:
`collect_scroll_containers_overflow_hidden_excluded` in this same test file
asserts `hidden` must NOT be wheel-scrollable, which matches real browsers).
Reusing one function for both purposes means a `hidden`/`clip` container's
scroll offset — which `set_scroll_position` now computes correctly, and
which the shell's `about_to_wait.rs` scroll-request drain applies straight
to the `LayoutBox` tree by node id regardless of `collect_scroll_containers`
(confirmed: that call is unconditional on overflow value, not gated by the
same list) — is written to the *engine* state correctly but never reaches
the *JS-visible* `scroll_states` cache, so every read answers the shim's
zero fallback. Fixing this needs a decoupled JS-state collection (or an
eligibility flag split between "wheel-routable" and "JS-visible", since the
two must diverge for `hidden`/`clip`) threaded through `page_load.rs`'s
initial seed, `relayout.rs`'s `update_scroll_containers()`, and
`about_to_wait.rs`'s post-drain re-push — a real design task, not a
point fix, and out of this slice's scope. **Also co-blocking the same file
regardless:** the already-filed [BUG-523](BUG-523-OPEN.md) — `scrollTo`/
`scrollLeft=` are queue-based (`_lumen_request_scroll` → drained on the next
`about_to_wait` tick), so a synchronous read immediately after a write sees
the pre-write value even once the JS-state gap above is fixed; the live
probe this slice used explicit waits between steps specifically to rule
this out as a confound, and still saw `[0, 0]`, confirming the JS-state gap
is a genuinely separate blocker, not just BUG-523 showing through.

**Remaining scope, unchanged in count but now more precisely understood:** 5
files — the same 4 `scrollbar-gutter-propagation-*.html` (BUG-529) plus
`overflow-clip-clamps-and-ignores-scroll-offsets-vertical-rl.html`, which
needs the `collect_scroll_containers`-vs-JS-state split above (and
BUG-523) before it can pass live, even though its layout-engine half is now
correct and unit-tested. Status stays `OPEN`.

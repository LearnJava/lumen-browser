# BUG-343: flex final-positioning permanently overwrites relative `width`/`height` with a resolved px value — corrupts descendants across repeated layout passes

**Renumbered 2026-07-25** from BUG-342 — collided with another session's
`origin/main` merge (V8 native-function trampoline bug) that landed under the
same number while this branch was in progress.

**Статус:** FIXED 2026-07-29 (P1) — закрыт вместе с [BUG-333](BUG-333-FIXED.md), тот же корень
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs::lay_out_flex`) — **general engine bug, not chrome-specific**
**Найден:** P1, 2026-07-25, while investigating a CPU-snapshot mismatch (`1000000-final`) caused by the [BUG-341](BUG-341-OPEN.md) partial fix.

## Symptom

`lay_out_flex`'s final main-axis positioning loop resolves each flex item's
main size and then does, for row direction (`box_tree.rs:~8553`):

```rust
let inner_main = (outer_main - m_l - m_r).max(0.0);
children[i].style.width = Some(Length::Px(inner_main));
lay_out(&mut children[i], ..., inner_main, ...);
```

(and the mirrored `style.height = Some(Length::Px(inner_main))` for column
direction, `box_tree.rs:~8531`). This **permanently replaces** whatever the
item's own `width`/`height` declaration was — including a *relative* one like
`width: 100%` — with an **absolute resolved pixel value**, baked directly
into the `LayoutBox`'s `ComputedStyle`. The original relative declaration is
gone; nothing restores it afterward.

If the same subtree is laid out again with a **different** available size
(e.g. a re-layout, or — as found here — a nested "preliminary/probe" pass
inside an ancestor's own `lay_out_flex` Step 1 that runs the item under the
*wrong* container width), the relative width can no longer be re-resolved
correctly: `lay_out_inner` sees an explicit `Length::Px(N)` and uses `N`
verbatim, ignoring the actual available width of the *new* pass entirely.
The item is stuck at whatever pixel value the **first** pass happened to
compute, however wrong that pass's context was.

## Repro (found via)

`graphic_tests/1000000-final.html`'s CSS Scroll Snap demo:

```css
.snap-demo { display: flex; gap: 8px; height: 120px; }         /* row */
.snap-demo-x { flex: 1; overflow-x: scroll; display: flex; }   /* nested flex item AND flex container */
.snap-demo-x .snap-panel { flex-shrink: 0; width: 100%; height: 100%; display: flex; }
```

`.snap-demo-x` has `flex: 1` → `flex-basis: 0%` (a `FlexBasis::Length`
item of the outer `.snap-demo` row). Traced with a temporary debug print
(`LUMEN_DEBUG_FLEX=1`, removed after diagnosis) around both the Step-1 loop
and the final-positioning assignment, on **unmodified `lay_out_flex`**
(before the BUG-341 partial fix):

```
[flex-step1-DEBUG] item(display=flex) step1 lay_out with content_width=864   # outer .snap-demo Step 1 probes .snap-demo-x at the FULL outer content width (864), not its true flex-resolved share
[flex-step1-DEBUG] item(display=flex) step1 lay_out with content_width=862   # .snap-demo-x's OWN nested lay_out_flex, Step 1, for its 3 .snap-panel children — resolves width:100% against the WRONG 862 (864 minus .snap-demo-x's own border)
[flex-step1-DEBUG] item(display=flex) step1 lay_out with content_width=862
[flex-step1-DEBUG] item(display=flex) step1 lay_out with content_width=862
[flex-final-DEBUG] setting item(display=flex) style.width=862 content_width_of_container(cb)=862   # .snap-panel.style.width permanently overwritten: "100%" -> Length::Px(862)
[flex-final-DEBUG] setting item(display=flex) style.width=862 content_width_of_container(cb)=862
[flex-final-DEBUG] setting item(display=flex) style.width=862 content_width_of_container(cb)=862
[flex-final-DEBUG] setting item(display=flex) style.width=756 content_width_of_container(cb)=864   # outer .snap-demo's FINAL (correct) pass: .snap-demo-x gets its true resolved width, 756
[flex-step1-DEBUG] item(display=flex) step1 lay_out with content_width=754   # .snap-demo-x's SECOND, now-correct nested lay_out_flex: cb=754 this time (756 - border)
[flex-step1-DEBUG] item(display=flex) step1 lay_out with content_width=754
[flex-step1-DEBUG] item(display=flex) step1 lay_out with content_width=754
[flex-final-DEBUG] setting item(display=flex) style.width=862 content_width_of_container(cb)=754   # BUG: still 862! .snap-panel.style.width is no longer "100%" (destroyed above), so it can't re-resolve against the correct cb=754
[flex-final-DEBUG] setting item(display=flex) style.width=862 content_width_of_container(cb)=754
[flex-final-DEBUG] setting item(display=flex) style.width=862 content_width_of_container(cb)=754
```

Final rendered result: each `.snap-panel` is `862px` wide instead of the
correct `754px` (`.snap-demo-x`'s own content-box width) — 108px too wide,
overflowing its scroll-snap container. Because `.snap-demo-x` has
`overflow-x: scroll`, the extra width is clipped in the visible viewport
(only the first panel is shown), so the bug is **visually almost invisible**
in a static screenshot — it only showed up as a ~750-pixel anti-aliasing
strip at the scroll container's border/corner in the CPU snapshot diff, not
as an obviously-wrong layout. The *scrollable* content width and the true
per-panel size are still wrong, though — this would be visible/reachable via
scrolling, `scrollWidth`, or `scroll-snap-align` landing at the wrong offset.

## Root cause

Two compounding issues:

1. **General:** any relatively-sized (`%`, or otherwise container-relative)
   flex item that undergoes **more than one** `lay_out()` pass with
   *different* available space gets its relative declaration permanently
   replaced by the pixel result of whichever pass ran **last**, because
   `style.width`/`style.height` is mutated in place rather than resolved
   fresh from an unmodified declaration each time.
2. **Trigger:** `lay_out_flex`'s own Step 1 (the preliminary/probe pass,
   still present for several branches after the [BUG-341](BUG-341-OPEN.md)
   partial fix — anything needing real intrinsic content size) runs a flex
   item **as if it were a plain fill-available block** under the *container's
   full available width*, not its true post-flex-resolution share. If that
   item is itself a flex container with relatively-sized children, this
   preliminary pass corrupts those children's style **before** the item's
   real, correctly-constrained final pass ever gets a chance to run — and by
   then it's too late, because (1) means the correct pass can't undo the
   damage.

The [BUG-341](BUG-341-OPEN.md) partial fix (BUG-341 partial fix, 2026-07-25)
incidentally avoids triggering this **for row-direction `FlexBasis::Length`
items specifically** (like `.snap-demo-x`, `flex: 1` → `flex-basis: 0%`),
since Step 1 is now skipped entirely for that combination — no probe pass,
no corruption. It does **not** fix the underlying mutate-in-place issue,
which remains reachable through:
- column-direction items that still take the Step-1 probe path (`Auto`/
  `Content` flex-basis, or `Length` with `min-height` unset + visible
  overflow),
- row-direction `Auto`/`Content` items with an explicit width (still probed),
- any *other* code path in the engine that lays the same subtree out twice
  with different constraints (e.g. two different ancestors, a resize, or
  incremental-vs-full layout mixing).

## Suggested fix direction (not attempted here — architectural, needs its own scoped review)

Do not overwrite the item's own style declaration to communicate the
resolved size into the recursive `lay_out()` call. Options:
- Thread the resolved main/cross size to `lay_out_inner` as an explicit
  override parameter (similar to `explicit_main`/`explicit_cross`) instead
  of writing through `style.width`/`style.height`, so the original
  declaration (percentage, auto, etc.) survives for future passes.
- Or: snapshot the item's original `width`/`height` before the flex loop and
  restore it after painting/positioning is done for this pass, if some
  downstream code genuinely needs the resolved value to remain readable.

Either needs verification against the full flex/grid test suite (this
mutate-through-style pattern also appears in `lay_out_grid` — not audited
here) plus the graphic tests, since several existing regression comments in
`lay_out_flex` (BUG-104, BUG-158, BUG-179, BUG-209, BUG-294) rely on the
current mutate-in-place behavior in ways that would need re-verification
under any fix. Out of scope for this session — filed as a follow-up.

## Impact

Any flex item that is *itself* a flex/grid container with relatively-sized
children, nested inside an ancestor whose own flex-basis is `auto`/`content`
(i.e. still takes the Step-1 probe path), is at risk of this corruption —
not chrome-specific, reachable by any real page with this nesting shape.
Severity in practice is usually low (the wrong pixel value is often close to
the right one, or clipped by `overflow`), but it is a real, silent,
100%-cascading-into-final-output correctness bug, not just a perf one.

## Repro

Minimal reduction: a row flex container with one `flex-basis:auto` sibling
before a `flex:1` flex-container item whose own children use a relative
(`%`) main-size — see `graphic_tests/1000000-final.html`'s
"CSS Scroll Snap L1" card (`.snap-demo` / `.snap-demo-x` / `.snap-panel`).

## Fix (P1, 2026-07-29)

Option 2 of the "suggested fix direction" above — snapshot & restore, chosen
over threading a new override argument because `lay_out`/`lay_out_inner` are
already at the clippy argument limit and every other call site would have had
to grow a `None`.

`SavedItemSizing` (`box_tree.rs`, just above `lay_out_flex`) captures the item's
specified `width` / `height` / `box_sizing` before the used-size overwrite and
puts them back right after the recursive `lay_out` returns. All three overwrite
sites are covered: main-axis column (`style.height`), main-axis row
(`style.width`) and the cross-axis stretch re-layout (`relayout_column_flex`).
The declaration therefore survives any number of probe passes, and every pass
re-resolves percentages against its own containing block.

The same session closed [BUG-333](BUG-333-FIXED.md) with this fix — the chrome
sidebar's `.tab-row`s collapsing to `h=0` were this exact mechanism reached
through the column-direction probe path listed above, not the `var()` failure
the report claimed.

Verified:
- `--dump-layout graphic_tests/1000000-final.html`: `.snap-panel` = **754px**
  with `w=100.00%` still in the style (was 862px with the percentage replaced
  by a burnt-in px value).
- `--dump-layout about:chrome-preview`: all `.tab-row` at `h=28.00` (were 0).
- Regression tests `flex_probe_pass_does_not_burn_percentage_width_into_style`
  and `flex_probe_pass_does_not_burn_item_height_into_style`; both fail without
  the fix with exactly the reported symptoms (stale 300px / height 0).
- `lay_out_grid` audited (grep for `style.width`/`style.height = Some(Length::Px`):
  it does **not** use the mutate-through-style pattern — the note in the "suggested
  fix direction" section above was a guess. The only other write sites are the
  intrinsic sizing of replaced elements (`<img>`/`<canvas>`/`<video>`), where the
  value comes from the resource's own intrinsic dimensions and a repeated pass
  recomputes the same number.
- The pre-existing regression comments that were written around this behaviour
  (BUG-104, BUG-158, BUG-179, BUG-209, BUG-294) all keep their tests green:
  `cargo test -p lumen-layout` 3454 passed, 2 failed (pre-existing
  [BUG-339](BUG-339-OPEN.md), red on `main`).

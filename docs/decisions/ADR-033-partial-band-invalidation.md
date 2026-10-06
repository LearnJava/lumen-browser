# ADR-033: Partial invalidation of the scroll band — redraw dirty rows, not the whole band

## Status

Accepted (design); implementation sliced under THREAD-11

## Date

2026-10-06

## Context

ADR-016 M3 planned "tile raster workers" so that scrolling several screens in
a row never waits for rasterization. THREAD-11 was written for the case where
scroll leaves the cached band. A probe before code (journal 2026-10-06,
lenta.ru, 120 wheel clicks over several screens, Vulkan/Intel Iris, three
runs: on-time 0.87–0.97, 3–5 jerks, max gap 88–94 ms) found a different cause:

- every gap over 60 ms lies between two **`frame` presents** (browser-thread
  commits with a new display list), none between wheel ticks;
- 34 of 35 `band repaint` classifications (`ScrollCache::plan`, measurement
  only) were `delta ContentChanged`, one was the first frame; `scroll_y` stayed
  inside the band for 467 of 634 frames;
- in the wgpu compositor (`renderer/band_compose.rs`, `compose_page`) the same
  run had 265 HIT, 31 frames with an **unstable key** (the display list changed
  by a few commands, 2372 ↔ 2374 ↔ 2377; such frames are drawn monolithically,
  the band is not touched) and 5 full-band **MISS** (1920×2541 px, 2541/2541
  rows).

So the cost is not "the band is too small" but "any change to any command is a
change to the whole key": the key is one hash of the whole content
(`fold_content_dual`), and a miss re-rasters the full band. The page's own
activity (lazy images, carousels) does that a few times per second.

What already exists and is reused:

- row-clipped band passes — `BandStrip { row0, rows }`, `ring_advance_plan`,
  `LUMEN_BAND_RING` (BUG-405 slice 32): a pass draws the **whole** static list
  under a row clip, so painter's order inside the strip is exact;
- `DisplayCommand::cull_rect()` — a safe AABB (`None` for structural commands)
  already used for viewport culling;
- `hash_one_command` per-command digests; `TileGrid` (256 px CSS tiles,
  `update_from_diff`) is index-based, dormant and not wired to any backend.

## Decision

Partial invalidation first, workers later and only if measurement still asks
for them. Granularity is **row strips of the existing band texture**, not a new
tile texture grid: the band is full-width, so a rectangular dirty region
collapses to a row range, and the ring-strip machinery already redraws row
ranges correctly.

1. **Per-command digests are retained.** The key pass already walks every
   command; it also stores a `Vec<u64>` (one digest per command, ~2.4 k
   entries) next to `page_band.key`. No second walk.
2. **Diff on key mismatch** (`band_diff.rs`, pure, unit-tested): trim the common
   prefix and suffix of the two digest vectors; the middle is the changed
   window. Index-wise comparison is not used (an insertion would shift the
   rest of the list and mark everything changed).
3. **Dirty rows.** Union of the `cull_rect()` y-range of every command in the
   changed window, old and new side. The window is grown to a balanced
   `Push*/Pop*` group whenever it contains a structural command; if the
   group's extent is unknown (filter, transform, backdrop, sticky/fixed/scroll
   layers, `None` bounds on a non-structural command) → **fallback to the full
   band** (today's behaviour). Correctness never depends on guessing an extent.
4. **Clip to the band.** Dirty rows outside `[band_top, band_top + band_h]`
   cost nothing: the band stays valid and just adopts the new key.
5. **Strip passes.** Rows inside the band are merged into at most 4 ranges
   (more, or > 50 % of the band → full band), mapped to texture rows with the
   ring formula `(y − ring_base) mod band_h` (a range split by the texture
   edge is two passes, as in the ring). Valid only with an opaque canvas
   background and integer device rows — the same conditions as
   `LUMEN_BAND_RING`.
6. **Unstable-key frames use the same path.** A changed key whose diff is
   partial updates the band in the same frame instead of falling back to a
   monolithic draw; a flapping carousel then costs one strip per tick.
7. **Invariant 5 stays.** A strip is drawn synchronously on the render thread
   in the frame that needs it (small by construction). Asynchronous workers
   (stale blit + checkerboard) are slice 7 and are built **only if** the
   post-slice-6 measurement still shows frame presents over budget.

Flag: `LUMEN_BAND_PARTIAL=0` rolls back to full-band misses (A/B knob; the
default flips after slice 6 measures better than the baseline).

Slices, in order: **S1** `band_diff.rs` + tests (pure; window, group growth,
fallback reasons); **S2** retain digests with the band key; **S3** adopt the
key when dirty rows lie outside the band (no pixels move); **S4** strip redraw
for in-band dirty rows behind the flag, display-list/pixel A/B on the corpus
(`graphic_tests`, `--dump-display-list` sweep); **S5** unstable-key frames
through the partial path; **S6** re-measure (same scenario as the probe, 3 runs
× lenta/ria/rbc, plus THREAD-5) and flip the default; **S7** raster workers —
only on evidence.

Success criterion for the task: the probe scenario has no `frame`-present gap
over 60 ms attributable to a content change, and the on-time share is not
below the Chromium baseline of the THREAD-13 slice 6 table.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Tile texture grid (256 px tiles) rastered by workers, as in ADR-016 M3 | Solves "scroll leaves the band", which the probe did not observe; needs a new texture topology, cross-thread wgpu encoding and a checkerboard path before it removes a single 60 ms gap. Kept as slice 7 |
| Revive `TileGrid::update_from_diff` as is | Index-based: an inserted command shifts the tail and marks the whole list dirty; the grid is CSS-tile oriented while the band is a row ring; no consumer |
| Per-element retained layers (Chromium-style compositing) | Needs layerisation decisions in paint (will-change, stacking) — a different project; the row strip gives the same win for the observed pattern at a fraction of the surface |
| Make the key coarser (ignore small list changes) | Wrong pixels on screen; the gate is "same picture", not "similar" |

## Consequences

- **Positive:** a content change costs its own rows, not 2541; flapping
  display lists stop dropping frames to the monolith; no new thread or
  texture; the fallback is today's behaviour, so the worst case does not
  regress.
- **Negative / trade-offs:** a diff and a digest vector per content change
  (O(n), ~2.4 k commands; the hash pass already pays the walk); strip passes
  repeat the ring's constraints (opaque background, integer rows); an unknown
  extent silently degrades to a full miss — the frame log must say why
  (`partial-skip: <reason>`) so degradation is visible.
- **Future:** close THREAD-11 when the criterion above holds; revisit workers
  (slice 7) only with a measured over-budget `frame` present left; remove
  `LUMEN_BAND_PARTIAL` after one release, as with ADR-029.

### Slice 1 result (2026-10-06)

Landed `crates/engine/paint/src/band_diff.rs` (`diff_band`, `BandDiff`,
`FullReason`, 13 unit tests, not wired to the renderer yet). One rule found
while testing that the design above did not state: a leaf inside an enclosing
transform / scroll / sticky / fixed group has a **local** `cull_rect`, so a
change inside such a group falls back to the full band even when the group's
own commands are unchanged (they sit in the common prefix/suffix, not in the
window). Only clip, opacity and blend groups are extent-bounded.

### Slice 2 result (2026-10-06)

`PageBandCache` now retains `cmds` (the static part of the list the band was
drawn from) and `digests` (`hash_one_command` per command), filled at the band
MISS next to `key`. Deviation from the text above: the digests are computed at
the miss, not in the key pass — `diff_band` also needs the **old commands**
(their `cull_rect` bounds), so the list is cloned too; the cost is paid only
where a full re-raster already costs tens of ms. Nothing reads them yet (S3).

### Slice 3 result (2026-10-06)

`Renderer::try_partial_band` (`band_compose.rs`) runs before the band
hit/miss decision for stable-content frames: if the key changed but
`band_diff::outside_band` says every dirty row lies outside the band (or the
static list is identical), the band adopts the new key, list and digests and
the frame is a HIT. Extra preconditions found while writing it: the viewport
must lie inside the **old** band, size is unchanged, and
`PageBandCache::generation` (new field) equals `content_generation` — the key
also folds the generation (images, fonts, canvas bg), which per-command digests
cannot see, so a generation bump always re-rasters. Behind
`LUMEN_BAND_PARTIAL=1`, **off by default** until slice 6 measures it; frame
log level 2 prints `band-partial: adopt|full (<diff>)`. Not yet measured live.

### Slice 4 result (2026-10-06)

`try_adopt_band_key` became `try_partial_band`: besides adopting the key it now
redraws in-band dirty rows. `dirty_strips` (`band_compose.rs`, pure) clamps the
`band_diff` ranges to the band, rounds outward to whole device rows, merges,
and refuses (→ full MISS) for more than 4 ranges or more than half the band; a
range split by the texture edge becomes two `RingStrip`s, same row formula as
`ring_advance_plan`. Passes go through the existing `BandStrip` clip with the
whole new static list (painter's order stays exact), via the new
`render_band_passes` (also used by the full-miss path). Preconditions beyond
S3: opaque canvas background and integer device rows for `band_top`/
`ring_base` (the ring's own conditions); otherwise full MISS. Frame log level 2
prints `band-partial: strips <rows>`. Gate: GPU test
`partial_band_strips_match_full_redraw` (`--include-ignored`) — a band updated
by strips is byte-identical to a band drawn from scratch. Not measured live;
the flag is still off by default (S6 flips it).

### S5 (landed)

`compose_page` no longer gates `try_partial_band` on a stable key: a frame
whose key differs from the previous frame's goes through the same diff. A
partial verdict (adopt / strips) updates the band in that frame and the key
matches, so the frame is composed from the band instead of drawn monolithically;
a `full` verdict leaves the old behaviour (monolith, band untouched). No new
code path, only the removed condition; correctness rests on the S4 gate.

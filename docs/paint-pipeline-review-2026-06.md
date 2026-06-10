# Paint Pipeline Architectural Review — 2026-06-10

Systemic review of the paint pipeline triggered by the persistent cluster of graphic-test
deviations in `BUGS.md` (29 OPEN at review time). Goal: group bugs by root cause, decide
what to fix systemically vs point-fix vs threshold-calibrate, and order the work so that
upcoming features (canvas 2D, View Transitions) don't have to be built twice.

Analysis performed by 4 parallel read-only agents over `cpu_raster.rs`, `display_list.rs`,
`renderer.rs`, `backends/femtovg_backend.rs`, `rule_index.rs`, and the test pipelines.

---

## Key finding 1: the test gate measures the least complete backend

`graphic_tests/run.py` screenshots the desktop window. The shell default backend is
**femtovg** (`crates/shell/src/backend_factory.rs:39-75`: default femtovg → fallback wgpu).
So every Edge-deviation number in `BUGS.md` is measured through femtovg — the backend with
the largest feature gaps:

| Capability | femtovg | cpu_raster | wgpu renderer |
|---|---|---|---|
| Filters: blur | **NO** — sigma recorded, never applied (`femtovg_backend.rs:1221-1251`) | yes (3× box-blur) | yes (shader) |
| Filters: color-matrix (grayscale/sepia/…) | **NO** — no-op | yes | yes |
| Blend modes | **2/17** — all others silently → SourceOver (`femtovg_backend.rs:260-267`) | 17/17 (`cpu_raster.rs:813-834`) | full (CSS Compositing L1 §8 shader, straight-alpha — spec-correct) |
| backdrop-filter | **NO** — save/restore no-op (`femtovg_backend.rs:1256-1265`) | partial (in-place) | yes (but: silently skipped when `from_level < 2`, `renderer.rs:6139`; REPLACE blend destroys parent alpha, `renderer.rs:6275`) |
| Gradient masks | **NO** — all mask variants collapse to axis-aligned `scissor(rect)` (`femtovg_backend.rs:1270-1294`) | yes (MaskSpec alpha) | yes |

**Consequence:** ~11 of the open Edge-deviation bugs are not logic bugs — they are
"femtovg hasn't implemented X". Fixing cpu_raster or renderer.rs will not move run.py
numbers. Point-fixing these one at a time in P3 is the wrong shape of work; they close
as a small number of femtovg feature implementations (P2 domain).

## Key finding 2: triple implementation of the same scalar math

Blend formulas, gradient stop interpolation, dash/dot offset math, and Mat4→affine
extraction are each hand-rolled 2-3 times across femtovg_backend / cpu_raster / renderer:

- gradient interpolation: `femtovg_backend.rs:272-297` (`interp_conic_color`) vs
  `cpu_raster.rs:1557-1589` (`sample_gradient_color`) — near-identical, duplicated;
- blend mapping: `femtovg_backend.rs:260` (2 modes) vs `cpu_raster.rs:813` (17 modes);
- dashed/dotted: `femtovg_backend.rs:550-610` (hand-drawn quads) vs `renderer.rs:7594-7655`
  (floor-snapped quads/circles) vs cpu_raster (**ignores BorderStyle entirely** —
  `cpu_raster.rs:1094` "dashed/dotted skipped for now").

Any fix made in one backend silently doesn't propagate. This is the second systemic
generator of "residual deviation" bugs.

## Key finding 3: no pixel-snapping policy

Display-list emission passes unrounded f32 coordinates; each primitive/rasterizer rounds
its own way (conic gradient floors its bbox `cpu_raster.rs:1476-1479`; IFC rows are
rounded `box_tree.rs:4219`; inline-run X is not; vertical-align `dy` applied unsnapped
`box_tree.rs:4270`; GPU dashes floor offsets; femtovg doesn't). The 1-3% "sub-pixel"
cluster (BUG-081, 083, partially 084) follows directly from this inconsistency.

---

## Bug clusters and verdicts

### Cluster A — femtovg feature gaps (systemic fix, P2)
- **BUG-082** (filter 33%): femtovg blur+color-matrix not applied + backdrop-filter no-op.
- **BUG-094** (text-shadow blur 6.8%): same — PushFilter wrapper has no blur in femtovg.
- **BUG-098** (mix-blend-mode 14%): femtovg maps 15/17 modes to SourceOver. NOT a formula
  bug — cpu_raster and the wgpu shader both implement spec-correct straight-alpha blending.
- **BUG-076** (box-shadow blur 1.06%): partially same cause (blur path), plus off-screen
  layer bilinear resampling. Sigma conversion (`display_list.rs:3033`, σ = blur/2) is
  spec-correct — do not "fix" it.

**Verdict:** implement blur + color-matrix + full blend-mode set + backdrop-filter in
femtovg (via offscreen layers, reusing shared scalar modules below). Closes 4 bugs as
3 features instead of 4 point-fixes.

### Cluster B — gradient pipeline (investigate, then shared fix)
- **BUG-085** (12%): one agent hypothesized sRGB-vs-linear interpolation; review of
  TEST-39 contradicts this — the page uses opaque hex stops and a same-RGB transparent
  stop, where straight-vs-premultiplied and sRGB-encoded linear interpolation all agree
  with browser behavior. More likely candidates: **radial default sizing**
  (farthest-corner), **hard-stop AA** (`#333 0px, #333 10px, #666 10px`), and femtovg's
  library gradient kernel (`fill_gradient`) vs our custom sampling. Needs a focused diff
  of one failing sub-box at a time.
- **BUG-086 residual / BUG-101** belong to the same gradient module once extracted.

**Verdict:** extract `gradient_math.rs` (single stop-resolve + sample fn for all
backends), then investigate TEST-39 sub-box by sub-box. Don't change colour spaces
blindly.

### Cluster C — sub-pixel / AA (systemic fix, P1 layout + P2 paint)
- **BUG-081** (vertical-align 0.99%), **BUG-083** (list markers 3.4%), **BUG-084**
  (border-radius AA 1.5%): unified pixel-snapping policy at emission time (snap rect
  coords after vertical-align dy, snap marker boxes, snap shadow/border rects in
  display_list emission). ~30 lines, low risk.
- **BUG-080** (dotted/dashed 3%): femtovg dash algorithm vs Edge; also cpu_raster ignores
  BorderStyle (affects the CPU snapshot gate, not run.py). Extract shared dash/dot offset
  math, implement BorderStyle in cpu_raster.

### Cluster D — threshold calibration (not code)
- **BUG-093** (scrollbar 1.39%): platform scrollbar skin will never pixel-match Edge.
  Raise per-test threshold to 2%.
- **BUG-076 residual** after blur lands: if < 0.5% remains from layer resampling,
  calibrate rather than chase.

### RuleIndex (BUG-119) — verdict: scheme is sound, suspect the cache key
The bucketing scheme is structurally correct: the "unknown selector shape → universal
bucket" invariant holds (`rule_index.rs:178` always merges universal; functional pseudos
→ universal at `rule_index.rs:62-65`), and full `matches_complex()` validation runs on
every candidate. Per-test selector analysis found nothing the buckets would drop.

Prime remaining suspect: the **cache key is `(sheet_ptr, sheet_rules_len)`**
(`style.rs:~5177`). If a stylesheet is mutated in place with same pointer and same rule
count but different rule content (e.g. container-query / media re-evaluation rewriting
rules), the cached index is silently stale. That fits TEST-29 (container queries) and
possibly others. Secondary check: relative cascade order of indexed rules vs the
brute-force media/layer/scope/container passes. Also: the doc comment at
`rule_index.rs:14-19` omits `container_rules` from the not-indexed list — fix the doc.

### Architectural ordering constraint
Canvas 2D (BUG-099) and View Transitions (BUG-103) both sit on the layer/compositing
model (offscreen contexts, bounded snapshot layers + transforms). The current model
assumes full-viewport page-coordinate layers and has known holes (backdrop-filter
`from_level < 2` skip; REPLACE-blend alpha destruction). **Fix the layer model and land
the shared scalar modules before starting canvas 2D / View Transitions** — otherwise the
layer code gets rewritten twice.

---

## Recommended task order

| # | Task | Owner | Size | Closes / unblocks |
|---|---|---|---|---|
| 1 | Shared scalar modules: `blend_modes.rs`, `gradient_math.rs`, `matrix_util.rs` in lumen-paint | P2 | S | dedup; prerequisite for 2-4 |
| 2 | femtovg: real blur + color-matrix in PushFilter/PopFilter (offscreen layer) | P2 | M | BUG-082 (large part), BUG-094, BUG-076 |
| 3 | femtovg: full blend-mode set via offscreen layer + shared formulas | P2 | M | BUG-098 |
| 4 | femtovg: backdrop-filter; wgpu: fix `from_level<2` skip + REPLACE-blend alpha | P2 | M | BUG-082 remainder |
| 5 | Pixel-snapping policy at emission (box_tree vertical-align/markers, display_list rects) | P1 | S | BUG-081, 083, part of 084 |
| 6 | cpu_raster: implement BorderStyle dashed/dotted (shared dash math) | P2 | S | BUG-080 (CPU gate parity) |
| 7 | BUG-119: verify `(ptr,len)` cache-key staleness on in-place sheet mutation | P3 | S | 6 run.py regressions |
| 8 | BUG-085: gradient geometry investigation (radial sizing, hard stops) after task 1 | P3 | M | BUG-085 |
| 9 | Threshold calibration: TEST-51 scrollbar → 2% | P3 | XS | BUG-093 |

Items 1-4 before canvas 2D / View Transitions enter the roadmap.

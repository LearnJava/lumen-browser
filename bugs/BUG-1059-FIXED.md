# BUG-1059: chrome content overlapping `chrome_page_host_rect` never paints in the live window

**Статус:** FIXED 2026-09-19
**Компонент:** shell paint compositing (`crates/shell/src/chrome_ui.rs::build_chrome_overlay_strips`, `crates/shell/src/app/window_event/redraw_requested.rs` Step 6 chrome block)
**Найден:** P1, 2026-09-19, при живой проверке CC-18 (плавающая панель управления, `#demoBar`)

## Симптом

`#demoBar` (CC-18's floating control panel, `position:fixed; left:18px;
bottom:18px`) is correctly parsed, laid out and bound — a diagnostic layout
pass against the real `assets/chrome/chrome.html` at a 1920×1040 viewport
places its box at `rect=Rect { x: 18.0, y: 498.7, width: 252.0, height:
523.3 }`, fully inside the window and not display:none — but it never
appears in the live window. A live launch (`lumen.exe --maximized
about:blank`) followed by an OS-level (PowerShell `CopyFromScreen`, not the
engine's own screenshot path — see "Не тот скриншот" below) screenshot shows
the toolbar/tab strip/address bar rendering normally and the entire rest of
the window as flat page background, with no trace of the panel anywhere.

## Причина

`Lumen::relayout_chrome_host` bakes the whole chrome document into
`chrome_dl` — a single flat `DisplayCommand` list — and separately computes
`chrome_page_host_rect` (`Self::page_offset`), the rect where the *live
page*'s own pixels get composited in (the doc comment on
`relayout_chrome_host`, `chrome_ui.rs:36-49`, explains why:
`#contentArea` is pruned out of the chrome tree entirely so the page can be
drawn separately underneath).

The frame assembly in `RedrawRequested` (`redraw_requested.rs:538-653`) then
runs `chrome_dl` through `build_chrome_overlay_strips`
(`chrome_ui.rs:1841-1879`): it wraps `chrome_dl` in **four** `PushClipRect`
strips — top/bottom/left/right of `host` (`chrome_page_host_rect`) — and
nothing else. Any command whose geometry falls entirely *inside* `host`
survives in **none** of the four strips and is silently discarded, by
design: the doc comment on `relayout_chrome_host` explicitly frames this as
"guaranteeing nothing paints inside the live page's own rect", because the
motivating case was `<body>`'s own leftover full-window background box
bleeding over the real page.

That design assumption — the only reason anything would sit geometrically
inside `host` is stale full-window chrome background — was true for every
existing panel (toolbar/tab strip/sidebar sit *outside* `host` by
construction; docked/anchored popovers like the settings/history/bookmarks
overlays render through the *legacy* pixel-geometry path, not `chrome_dl`).
CC-18's `#demoBar` breaks it on purpose: a `position:fixed` panel that
floats **over** the page content is deliberately positioned inside `host`,
and there is currently no path for *any* `chrome_dl` content in that region
to reach the screen. This is a genuine gap in the compositing model, not a
CC-18-specific bug in the panel's own markup/CSS/dispatch code — those are
independently verified working (layout box computed correctly; 3 new
`crates/chrome/src/model.rs` unit tests cover `bind_control_panel`'s
attribute/class syncing against the real parsed asset).

### Не тот скриншот (important for whoever debugs this next)

Every existing automation screenshot surface (`resource://screenshot` over
`--mcp-live-port`, `--screenshot`, `--ipc-server`) renders `self.display_list`
— **page content only, chrome is never drawn there** (see the doc comment on
`Lumen::render_current_page_to_png`, `crates/shell/src/lumen/automation.rs`
— confirms `CAPABILITIES.md`'s DS-4 note about headless/CPU screenshots).
Confirming or refuting this bug (or any future chrome-visual fix) needs a
real OS-level screen capture of the live window — there is currently no
in-repo tool for that; a throwaway PowerShell `System.Drawing`
`CopyFromScreen` script against a `--maximized` launch was used for this
finding.

## Что нужно сделать

Give floating/page-overlapping chrome content ( `#demoBar`/`#infoPanel`
today, potentially more later) a paint path that survives compositing:
detach its `LayoutBox` from the chrome tree before the 4-strip clip is
built (mirroring `take_content_area`'s detach, but for the opposite
reason — this content must paint **unclipped**, not be excluded), build a
small standalone display list for just that detached subtree (the
`lumen_driver::scope::display_list_scoped` provenance-slicing approach is
the closest existing precedent, though it targets the page tree, not
chrome), and append it to `overlay_buf` **after** the page paints, the same
way the omnibox caret (`caret_plan`) already paints unclipped on top of the
strip-clipped chrome segment.

Not attempted in this session: `take_content_area`/`restore_content_area`'s
own doc comments (BUG-341 S22) stress how easy it is to get this kind of
detach subtly wrong against the incremental box-reuse basis the interaction
loop depends on — a rushed copy of that machinery for a second, differently
-motivated detach risked a silent incremental-layout correctness regression
across a very sensitive, heavily-tested subsystem, for a fix outside a
single session's safe review budget.

## Масштаб

Blocks CC-18 (`ROADMAP.md`) from being visually complete — see that task's
срез 1 revision note. Self-contained to chrome paint compositing; does not
affect page rendering, WPT, or any existing panel (none of them currently
place chrome content inside `chrome_page_host_rect`).

## Исправлено (P1, срез 2, тот же день)

Built the paint path the previous session sketched, with the care its own
"Not attempted" note asked for — new, independent code, not a rushed reuse
of `take_content_area`/`restore_content_area`.

- **Detach:** `take_floating_panel`/`take_floating_panel_at`
  (`crates/shell/src/chrome_ui.rs`) walk `LayoutBox` the same way
  `take_content_area_at` does, but skip its salvage step entirely — a
  floating panel paints as one unclipped unit, nothing needs to stay behind
  in the strip-clipped main tree. `relayout_chrome_host` calls this for
  `lumen_chrome::ids::DEMO_BAR`/`INFO_PANEL` right after `#contentArea`'s own
  pruning (siblings under `<body>`, not descendants of `#contentArea` — order
  between the two doesn't matter), before `paint_ordered(&layout)` builds
  `chrome_dl`, so the strip clip never sees them.
- **Standalone paint:** each detached box is flattened on its own via the
  existing `paint_ordered` (already absolute-positioned, same precondition
  `take_content_area`'s doc comment already relies on for its own salvaged
  popovers) and concatenated into `Lumen::chrome_floating_dl`. `RedrawRequested`
  appends it to `overlay_buf`, unclipped, right after the strip-clipped
  segment (+caret) — same idea as the caret, a separate step instead of
  folding into `ChromeOverlayFrameCache` to avoid touching that cache's
  invariants at all.
- **Restore:** `restore_floating_panel`, mirroring `restore_content_area`.
  `chrome_floating_detached: Vec<FloatingPanelDetachment>` is restored into
  the incremental basis at the top of the next `relayout_chrome_host` pass —
  same S22 shape as `chrome_content_area_detached`: a restore failure
  discards the whole basis and falls back to a full layout, never a wrong
  incremental tree.

**Tests:** `bug1059_take_floating_panel_detaches_and_restores_demo_bar`
(detach removes the box from the tree, the detached box still paints
non-empty content, restore puts it back at the same rect) and
`bug1059_chrome_dl_excludes_demo_bar_after_detach_but_floating_dl_includes_it`
(the actual split: `#demoBar`'s own background fill is present in the
standalone `floating_dl` and absent from `chrome_dl`) — both in
`crates/shell/src/tests/chrome_incremental.rs`, next to the srez-1 layout
test this session's predecessor left. `cargo clippy -p lumen-shell
-p lumen-chrome --all-targets -- -D warnings` clean. `scripts/scoped-test.sh`:
only red is `cpu_snapshots_match_references` — the same BUG-1008-class
7-file drift, reproduced on a clean `main`, unrelated to this change.
`dump_golden.py` — same pre-existing 4/12 mismatch as `main`. Live
`--maximized` launch + an OS-level screenshot (same method the symptom
section above used) confirms `#demoBar` now renders in the window's
bottom-left corner.

**Not attempted, still CC-18's own remainder** (unaffected by this fix):
drag-by-header, double-click reset, the ☾/☀ theme override, the QA-panel
no-op button — see `ROADMAP.md`'s CC-18 entry.

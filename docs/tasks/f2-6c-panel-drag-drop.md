# F2-6c — Panel drag & drop + Surface/SurfaceManager + layout persistence

**Developer:** P1
**Branch:** `p1-f2-6c-panel-dnd`
**Size:** L
**Crates:** `lumen-shell` (surface/, panels/, tabs/)

## Goal

Make shell panels (sidebars / vertical tabs) movable and dockable instead of hard-positioned, by routing their placement through the existing `Surface` / `SurfaceManager` infrastructure (ADR-009) rather than the ad-hoc `x = 0..PANEL_WIDTH` offsets scattered across `main.rs`. Add a drag-to-redock interaction (grab a panel by its header, drag it to a different dock slot, drop) and persist the resulting layout (which panel is in which slot, slot sizes, visibility) so it survives a restart. This is the remaining part 2.2 of F2-6; stages 1 (chrome theming) and 2.1 (theming ~22 panels) are already merged.

## Prerequisites

- F2-6 stage 1 (central `Palette` chrome theming) — ✅ done 2026-06-22.
- F2-6 stage 2 part 1 (theming ~22 secondary panels via `&Palette`) — ✅ done 2026-06-22.
- Nothing else blocks this. It is greenfield UI-infrastructure work; the `surface/` module already exists as a self-contained, fully-tested foundation but is **not yet wired into the running shell** — that wiring is the bulk of this task.

## Current state

### What already exists (the `surface/` module — well-built, unused)

The `Surface`/`SurfaceManager` foundation is implemented and unit-tested, but `grep` finds **no references to it from `main.rs`** — it is dead infrastructure waiting to be adopted.

- `crates/shell/src/surface/mod.rs:48` — the `Panel` trait: `id()`, `surface()`, `width()`/`height()` (→ `SizeRule`), `paint(&PaintCtx) -> DisplayList`, `hit_test()`, `on_event(&PanelEvent, &mut EventCtx) -> EventResponse`, plus lifecycle hooks (`on_mount`/`on_unmount`/`on_resize`/`on_focus`/`on_blur`).
- `crates/shell/src/surface/types.rs:28` — `Surface` enum: `Docked { slot }`, `Float { anchor, z_order, close_on_outside_click }`, `OsWindow { … }` (modelled, not composited), `Modal { … }`.
- `crates/shell/src/surface/types.rs:117` — `SizeRule`: `Fixed`, `Flex`, `Content` (stubbed → treated as 0), `Range { min, max, default }`, `Hidden`.
- `crates/shell/src/surface/types.rs:175` — `PanelEvent` (mouse/scroll/text/focus/resize/theme). **Note:** there is *no* drag variant here yet — `DragEnter`/`Drag`/`Drop` exist only in the design doc (`docs/shell-ui-architecture.md:383`), not in the implemented enum. You will add them.
- `crates/shell/src/surface/types.rs:270` — `HitElement`, already including `DragHandle` and `ResizeHandle { panel, horizontal }`; `crates/shell/src/surface/types.rs:258` — `CursorIcon` already includes `Grab`/`Grabbing`/`ResizeHorizontal`/`ResizeVertical`.
- `crates/shell/src/surface/manager.rs:75` — `SurfaceManager`: owns `panels: Vec<PanelEntry>`, `docked_rects: HashMap<&'static str, Rect>`, `theme`, `focused`.
- `crates/shell/src/surface/manager.rs:259` — `compute_slot_rects()`: the flat five-slot cross-layout (`top`/`left`/`right`/`bottom`/`content`; `SLOT_NAMES` at `manager.rs:60`). Slot size comes from the *first visible panel* in that slot (`docked_axis_size`, `manager.rs:314`).
- `crates/shell/src/surface/manager.rs:113` — `composite()` (docked then z-sorted overlay) and `manager.rs:329` — `route_mouse()` (overlay highest-z first, then docked reverse-registration order). Public mouse routers at `manager.rs:232`–`254`.
- `crates/shell/src/surface/ctx.rs:56` — `EventCtx`: `dispatch(Command)`, `request_repaint`, `set_cursor`, `request_focus`/`release_focus`. **No `start_drag` yet** (design doc `docs/shell-ui-architecture.md:671` has it; implementation does not).

### What is hard-coded today (the thing being replaced)

Panels are positioned by literal arithmetic in `main.rs`, not by a layout tree:

- `crates/shell/src/panels/vertical_tabs.rs:21` — `pub const PANEL_WIDTH: f32 = 200.0;`; module doc (`vertical_tabs.rs:6`) states "Panel occupies `x = 0..PANEL_WIDTH`" and the page display list is *shifted right by `PANEL_WIDTH`* when visible.
- `crates/shell/src/panels/sidebar_panel.rs:38` — `pub const PANEL_WIDTH: f32 = 300.0;`; right-docked, built with explicit `Rect::new(px, tab_bar_h, PANEL_WIDTH, …)` calls (`sidebar_panel.rs:218`+).
- `crates/shell/src/tabs/strip.rs:22` — `pub const TAB_BAR_HEIGHT: f32 = 36.0;`, referenced ~40× across `main.rs` to offset everything below the tab strip.
- The content viewport x-offset is recomputed inline everywhere via `if self.vertical_tabs.visible { PANEL_WIDTH } else { 0.0 }` — see `main.rs:8178`, `:10710`, `:10833`, `:10870`, `:11063`. Panel visibility lives as scattered bools on `App` (`vertical_tabs.visible` at `vertical_tabs.rs:54`; the `App` panel fields are constructed at `main.rs:656`–`728`).

There is **no drag-to-redock and no persisted panel layout** today; only `tab_layout: String` is persisted (see below).

### What ADR-009 mandates

`docs/decisions/ADR-009-shell-panel-system.md` — every shell UI block is a `Panel` that (1) declares *where* via `Surface`, (2) declares *size* via `SizeRule`, (3) paints into a given `Rect`, (4) reacts via `EventResponse`, (5) is registered into the single `SurfaceManager` coordinator. No external GUI framework; build on the existing `DisplayList` pipeline; retained-mode (repaint only on state change); "adding a panel = one new file, no existing files change". Cross-platform by construction (no `#[cfg(target_os)]` in panel code). The drag/resize/persistence types are sketched in `docs/shell-ui-architecture.md` (§10 `start_drag`, §13 `SurfaceManager` with a `layout_tree: LayoutNode`) but are **not yet implemented** — this task implements the slice needed for sidebar redocking.

### How layout currently persists

- `crates/storage/src/browser_settings.rs:83` — `BrowserSettings` (SQLite-backed; `open(path)` at `browser_settings.rs:106`, typed setters like `set_tab_layout` at `:257`). The snapshot struct `BrowserSettingsSnapshot` (`browser_settings.rs:43`) already carries `tab_layout: String` (`:63`) — the precedent for storing a UI-layout string.
- Re-exported at `crates/storage/src/lib.rs:64`.
- Portable data dir helper: `crates/shell/src/adblock.rs:44` — `browser_data_dir()` (writes under `<exe_dir>/data/…`; project policy: never use `%APPDATA%`/XDG). Use the existing SQLite `BrowserSettings`, do **not** introduce a new JSON file.

## Entry points

- `crates/shell/src/surface/manager.rs:75` — `SurfaceManager` struct; extend with redock + serialize/deserialize.
- `crates/shell/src/surface/manager.rs:259` — `compute_slot_rects()` / `manager.rs:314` `docked_axis_size()` — where per-slot sizing lives; add per-slot stored size override here.
- `crates/shell/src/surface/types.rs:175` — `PanelEvent` enum — add `DragStart`/`DragMove`/`DragEnd` (or `Drop { slot }`) variants.
- `crates/shell/src/surface/ctx.rs:56` — `EventCtx` — add a drag-request channel (`start_drag` / a `drag: Option<DragData>` field) mirroring the existing `cursor`/`focus_change` pattern.
- `crates/shell/src/surface/types.rs:270` — `HitElement::DragHandle` / `ResizeHandle` already exist; emit them from panel `hit_test()`.
- `crates/shell/src/panels/vertical_tabs.rs:21` and `crates/shell/src/panels/sidebar_panel.rs:38` — the two hard-positioned panels to migrate onto `Surface::Docked`.
- `crates/shell/src/main.rs:656`–`728` — where `App` panel fields are constructed; the place to instantiate and `register()` a `SurfaceManager`.
- `crates/shell/src/main.rs:8178`, `:10710`, `:10833`, `:10870`, `:11063` — the inline `vertical_tabs.visible ? PANEL_WIDTH : 0.0` offset sites that should eventually read from `SurfaceManager::slot_rect("content")` instead.
- `crates/storage/src/browser_settings.rs:43` (`BrowserSettingsSnapshot`) and `:257` (`set_tab_layout`) — the persistence precedent to extend with a `panel_layout` string.

## Steps

Keep the scope to **one real migration done end-to-end** (vertical tabs + sidebar), proving the path; do not try to convert all ~27 panels.

1. **Add drag primitives to the surface types.** In `surface/types.rs`, add a `DragData` struct (at minimum: source panel id, grab offset) and new `PanelEvent` variants `DragStart { pos }`, `DragMove { pos }`, `DragEnd { pos }` (window-local is fine for drag since it crosses panels — document the coordinate space, unlike the panel-local mouse events). Reuse the existing `HitElement::DragHandle` / `CursorIcon::Grab`/`Grabbing`. Add unit tests next to the existing `types.rs` tests.

2. **Add the drag request channel to `EventCtx`.** In `surface/ctx.rs`, mirror the `cursor: Option<CursorIcon>` pattern: add `drag: Option<DragData>` with `start_drag(&mut self, data)` and a `requested_drag()` read-back. Test it like the existing `event_ctx_*` tests.

3. **Teach `SurfaceManager` to redock.** In `surface/manager.rs`, add:
   - per-slot size override storage (so a dragged/resized slot remembers its width/height instead of always taking the first panel's `SizeRule`),
   - `move_panel_to_slot(&mut self, id: &str, slot: &'static str)` that rewrites the panel's docked slot and recomputes rects,
   - a drag state machine fed by the mouse routers (`route_mouse_down` on a `DragHandle` → begin drag; `route_mouse_move` → track + compute the hovered drop slot; `route_mouse_up` → commit `move_panel_to_slot`). Because `Panel::surface()` currently returns a value, you will need a way for the manager to override a panel's slot (store the slot in the `PanelEntry`, not only in `surface()`); adjust `compute_slot_rects`/`assign_panel_rects` (`manager.rs:277`) accordingly.
   - a drop-target highlight hook (return the candidate slot rect so `composite()` can draw an insertion overlay).

4. **Add layout (de)serialization to `SurfaceManager`.** Add `serialize_layout(&self) -> String` and `apply_layout(&mut self, &str)` (slot assignment + per-slot size + visibility per panel id). Keep the format compact and forward-compatible (e.g. JSON via `serde_json`, already a workspace dep). Unit-test round-trip.

5. **Persist via `BrowserSettings`.** In `crates/storage/src/browser_settings.rs`, add a `panel_layout: String` column/field to `BrowserSettingsSnapshot` (`:43`) following the exact pattern of `tab_layout` (`:63`/`:257`): a `set_panel_layout` setter + snapshot read. Save on drag-commit / resize-commit; load at startup and feed into `SurfaceManager::apply_layout`.

6. **Migrate the two real panels.** Implement the `Panel` trait for the vertical-tabs panel and the sidebar panel (or thin adapters wrapping their existing `build_*`/`hit_test` functions), declaring `Surface::Docked { slot: "left" }` / `"right"` and `SizeRule::Range`/`Fixed` from their current `PANEL_WIDTH` constants. Have their `hit_test` return `HitElement::DragHandle` over the panel header so the drag interaction has a grab point.

7. **Wire `SurfaceManager` into `App`.** In `main.rs` (around `:656`–`728`), construct a `SurfaceManager`, `register()` the migrated panels, and on resize call `on_resize`. Drive the content viewport x-offset from `surface_manager.slot_rect("content")` instead of the inline `vertical_tabs.visible ? PANEL_WIDTH : 0.0` (`main.rs:8178` etc.). Route the relevant mouse events through the manager's routers. Keep non-migrated panels on their current path for now (incremental, per ADR-009).

8. **Drain `EventCtx` effects in the shell.** After routing, apply `take_commands()`, `requested_cursor()`, `requested_focus_change()`, and the new `requested_drag()` against `App` state, matching how the manager is documented to drain effects.

## Tests

- `cargo test -p lumen-shell` — extend the existing `surface::types`, `surface::ctx`, and `surface::manager` test modules: drag event construction, `EventCtx::start_drag`/`requested_drag`, `move_panel_to_slot` recomputes rects, per-slot size override, `serialize_layout`/`apply_layout` round-trip, drag down-move-up commits a redock.
- `cargo test -p lumen-storage` — `panel_layout` setter/snapshot round-trip (mirror the `tab_layout` test).
- `cargo clippy -p lumen-shell --all-targets -- -D warnings` and `-p lumen-storage`.
- Manual: `cargo run -p lumen-shell -- samples/page.html`, toggle the vertical tabs (Ctrl+B), drag the panel header to the right edge, confirm it redocks and the content reflows; restart and confirm the layout is restored.
- No new graphic test is required (chrome, not page CSS), but a `--dump-display-list` spot-check that the content slot offset matches the docked panel width is a cheap sanity check.

## Definition of done

- [ ] `PanelEvent` has drag variants; `EventCtx` has `start_drag`/`requested_drag`; both unit-tested.
- [ ] `SurfaceManager` can override a panel's slot, redock via a mouse drag state machine, and store per-slot sizes; unit-tested.
- [ ] `SurfaceManager::serialize_layout`/`apply_layout` round-trip; unit-tested.
- [ ] `BrowserSettings`/`BrowserSettingsSnapshot` carry `panel_layout`, with a setter following the `tab_layout` precedent; persisted under the portable data dir; unit-tested.
- [ ] Vertical-tabs and sidebar panels implement `Panel` and are registered into a live `SurfaceManager` in `App`; their position comes from the manager, not the inline `PANEL_WIDTH` offsets.
- [ ] Content viewport offset is driven by `slot_rect("content")`.
- [ ] Dragging a panel header redocks it; the layout survives a restart (manual verification).
- [ ] `cargo clippy -p lumen-shell -p lumen-storage --all-targets -- -D warnings` clean; `cargo test -p lumen-shell` and `-p lumen-storage` green.
- [ ] Docs updated in the same commit: `CAPABILITIES.md` (shell UI), `subsystems/shell.md`, `SYMBOLS.md` (regen), `STATUS-P1.md` (F2-6 stage 2 part 2 → Recent). If the drag/persistence design lands differently from `docs/shell-ui-architecture.md`, reconcile that doc.

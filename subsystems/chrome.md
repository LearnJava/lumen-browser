# lumen-chrome

Compile-time bridge from the CC design asset to typed Rust (CC track, `docs/tasks/p1-css-chrome.md`).

## Scope

`build.rs` parses `assets/chrome/chrome.html` with the real `lumen_html_parser`/`lumen_css_parser`
(the same parsers the future runtime chrome host uses) and fails the build if the reference uses a
CSS property or selector `lumen-layout` does not implement. On success it code-generates
`OUT_DIR/chrome_gen.rs`, `include!`-d into `lib.rs`: element-id string constants, a typed
`ChromeIds` resolver, the `ChromeAction` enum (from `data-action` attribute values), and the
`<template>` id registry.

The runtime host (parse-once + relayout-on-resize + paint, CC-4), hit-test/hover/dispatch (CC-5),
`ChromeModel` → DOM mutation (CC-6), the toolbar/omnibox hybrid (CC-7), and the sidebar/workspace
tab-bar for both layouts (CC-8) are done — see below and `crates/shell/src/main.rs`.

## Done

- **CSS parse-gate** (`src/gate.rs`, shared verbatim by `build.rs` via `include!("src/gate.rs")`
  and by `lib.rs` via `#[cfg(test)] mod gate;`): walks every `Rule`'s selectors/declarations.
  Declarations must be a custom property (`--*`), in `lumen_css_parser::SUPPORTED_PROPERTIES`, or
  in the explicit `ALLOWED_UNKNOWN_PROPERTIES` allowlist (`-webkit-font-smoothing`, parse-and-ignore
  per CC-CSS-1). Selectors re-walk `ComplexSelector`/`CompoundSelector`/`SimpleSelector` mirroring
  `ComplexSelector::is_supported()`'s private logic, but grant `::-webkit-scrollbar`/`-thumb`/
  `-track` (`PseudoElementKind::Unknown` prefixed `-webkit-scrollbar`) since `layout::style` maps
  those to `scrollbar-width`/`scrollbar-color` (CC-CSS-1). Any other unsupported construct fails
  `cargo build` with the offending property/selector named.
- **`data-action` attributes** (`scripts/gen_chrome_assets.py::add_data_actions`): the reference's
  inline `onclick`/`onfocus`/`oninput` (dead once `<script>` is stripped) are rewritten to
  declarative `data-action="…"` (+ context attribute, e.g. `data-profile`/`data-view`) via an
  exhaustive `ONCLICK_EXACT_ACTIONS`/`CLASS_ONCLICK_ACTIONS` table; an unmapped handler raises
  `GenError` at generation time instead of shipping a dead attribute. 30 distinct actions across 98
  elements in the current asset.
- **ARIA `role`/`aria-*` attributes** (`scripts/gen_chrome_assets.py::add_aria_roles`/
  `add_aria_labels`, CC-13, docs/tasks/p1-css-chrome.md): runs after `add_data_actions` (keys off
  its `data-action` output) — `role="tablist"` on `#sbTabs`/`#hbarTabs`, `role="tab"`+
  `aria-selected` on every `data-action="select-tab"` element, `role="toolbar"`, `role="combobox"`
  +`aria-autocomplete` on `#omniInput`; `aria-label` on icon-only `<button>`s with no other visible
  text, keyed by `data-action` via `ARIA_LABEL_RULES` (buttons with their own text, e.g. the
  profile-menu toggle, are left alone). Consumed by `lumen_a11y::chrome::chrome_root_from_document`
  — see `subsystems/a11y.md`.
- **Codegen** (`build.rs`): `ids` module (one `&str` const per `id`-carrying element, 94 in the
  current asset); `ChromeIds` struct + `ChromeIds::resolve(&Document) -> Result<Self, ChromeIdError>`
  (no `unwrap`/`panic!` — the build-gate guarantees every id exists, but resolution still returns a
  `Result`); `ChromeAction` enum + `from_attr_value`/`attr_value`; `templates::IDS` (currently
  empty — the asset has no `<template>` markup yet, see Invariants).
- **Tests**: 16 `cargo test -p lumen-chrome` (gate unit tests with synthetic CSS fixtures +
  `to_snake_case`/`to_pascal_case` conversion + real-asset `ChromeIds::resolve` + `ChromeAction`
  round-trip + `parse_document` round-trip + CC-CSS-6 `UA_DEFAULTS` pair below). Verified manually
  (not a `cargo test`, since it would require corrupting the committed asset): injecting an unknown
  property into `assets/chrome/chrome.html` makes `cargo build -p lumen-chrome` fail with a clear
  message; reverting restores a clean build.
- **CC-CSS-6 `user-select` UA default** (`parse_document`): prepends `html{user-select:none}` as the
  textually-first rule before the asset's own collected `<style>` text — `user-select` is inherited,
  so this makes all chrome UI text non-selectable by default without touching the frozen design
  reference (which declares no `user-select` of its own; the property has no visual effect). A later
  author rule of equal specificity still overrides it (`ua_defaults_can_be_overridden_by_a_later_author_rule`).
  `pointer-events:none` needed no chrome-specific work: `chrome_hit_test` already calls the same
  `lumen_paint::hit_test` DOM pages use, which already skips such boxes generically. No live
  mouse-drag text-selection feature exists anywhere in Lumen yet (page or chrome) — `Selection`/
  `SelectionHighlight` are wired only to the JS `window.getSelection()` shim — so this is a
  forward-looking default, not something end-to-end-testable via a real drag today.
- **CC-4 runtime host** (`crates/shell/src/main.rs`, behind `LUMEN_CSS_CHROME=1` read once at
  startup — `run_window_mode`'s `css_chrome_enabled`): [`parse_document`] parses
  `chrome_preview::HTML` once into `Lumen::chrome_doc: Option<(Document, Stylesheet)>`.
  `relayout_chrome_host()` — called once from `resumed()` (first window size) and again on every
  `WindowEvent::Resized` — runs `layout_measured_hyp` over the full window size (bundled Inter
  measurer, no web fonts) and `paint_ordered`, caching the result in `Lumen::chrome_layout: Option<
  (LayoutBox, DisplayList)>`. The frame loop prepends `chrome_layout`'s display list to the very
  front of `overlay_buf` (painted first, everything else — panels/scrollbar/tab-bar/toolbar — still
  draws over it) and, under the flag, skips building the legacy tab-bar/toolbar entirely (their
  attached layout-toggle/settings/archive buttons are anchored to that same strip, so they go with
  it) so the two chromes never overlap. The design reference's `#contentArea` element — already the
  container the design reserves for live tab content — doubles as the brief's "`#page-host`"; no new
  id was introduced. `#contentArea` carries its own placeholder markup (new-tab tiles, a demo site
  page — content meant for standalone `about:chrome-preview`, not for stacking under a real page):
  `relayout_chrome_host` removes it from the layout tree entirely (`take_layout_box_by_node`, not
  just its children — its own `background:var(--surface-0)` fill would otherwise paint over the real
  page too, since chrome paints above content in `overlay_buf`) right after layout, capturing its
  rect into `Lumen::chrome_page_host_rect: Option<Rect>` first. That rect replaces the legacy
  `left_dock()` width / `toolbar::CHROME_H` pair in the two render-time page-offset call sites only
  (`set_page_offset` / the `PushTransform` fallback) — **not** in
  `content_layout_viewport`/`viewport_height_css`, so the
  page's own CSS layout viewport width is unaffected by the flag, matching the pre-existing
  vertical-tabs-sidebar limitation already documented on `content_layout_viewport` ("Width is
  unaffected"). Off the flag (default), `chrome_doc`/`chrome_layout` stay `None` and every touched
  call site takes its old branch — zero behavior change.
- **CC-5 hit-test/hover/active/dispatch** (`crates/shell/src/main.rs`): `Lumen::page_offset()` is
  the single source of truth for where page content starts on screen (`chrome_page_host_rect`'s
  origin under the flag, the legacy left-dock/`CHROME_H` pair off it) — `page_point`/
  `update_cursor_icon`'s page-hit-test branch now both read it (previously hardcoded, a latent
  input-coordinate bug under the flag: clicks/hover/cursor near the page's top edge would have
  targeted the wrong page node once `chrome_page_host_rect` diverged from `CHROME_H`).
  `Lumen::point_over_chrome(x, y)` — rect-membership test against `chrome_page_host_rect` — decides
  whether a pointer event belongs to the chrome furniture or to the page/floating popovers drawn
  above it. `Lumen::chrome_hit_test` runs `lumen_paint::hit_test` against `chrome_layout` in window
  coordinates; `Lumen::chrome_action_at` walks its `HitTestResult::path` (innermost first) for the
  nearest ancestor-or-self carrying `data-action`, satisfying the deferred note above about
  `.tab-close`/`.close-tab` nesting inside `.tab-row`/`select-tab`. `Lumen::dispatch_chrome_action`
  routes the ~12 actions with a real shell equivalent (`reload`, `open-cert-viewer`,
  `toggle-shield-popover`, `toggle-find`, `open-web-sidebar`, `open-ai-sidebar`,
  `toggle-downloads`, `open-print-dialog`, `toggle-devtools`, `toggle-profile-menu`, `new-tab`,
  `show-view data-view=settings`) to the exact functions the legacy toolbar dispatcher already
  called; the other ~17 actions only make sense once `ChromeModel` → DOM mutation exists (CC-6) or
  their popover has an engine-drawn equivalent (CC-9/CC-10) — recognised but a no-op for now.
  `chrome_hovered_nid`/`chrome_active_nid` (separate fields from the page's `hovered_nid`/
  `active_nid`, mirroring why `relayout_chrome_host` resets the interactive thread-locals rather
  than inheriting them) feed `:hover`/`:active` into the next `relayout_chrome_host` pass from
  `CursorMoved`/`MouseInput`. Double-dispatch avoidance: the legacy tab-strip/toolbar hit-testers in
  `WindowEvent::MouseInput`/`CursorMoved` (unconditional before this slice — they don't paint under
  the flag, CC-4, but were still live code) are now gated `if self.css_chrome_enabled { <chrome
  path> } else { <legacy path> }`; a click/hover outside the chrome's opaque area (real page content,
  including floating popovers CC-5 opens, e.g. `shields`/`downloads`/`print_panel`) falls through
  unchanged to the existing panel/page dispatch below, since those stay positioned within the
  page-content rect. Verified: `cargo test -p lumen-chrome` (14/14) + `cargo test -p lumen-shell`
  (1695/1695) + `cargo clippy -p lumen-shell --all-targets -D warnings` green; manual smoke launch
  with `LUMEN_CSS_CHROME=1` (no crash, first frame renders) — full click/hover interaction was not
  driven automatically (no OS-level input injector in the repo; the existing `--mcp`/IPC `click`
  bypasses chrome/panel dispatch by design, targeting page content only), so this remains verified
  by code review + the pre-existing CC-4 manual-`PrintWindow` protocol, not a screenshot diff.
- **CC-6 `ChromeModel`/`bind_model`** (`src/model.rs`, new; `Lumen::chrome_model_snapshot`/
  `dispatch_chrome_action` in `crates/shell/src/main.rs`): binds real shell state into `chrome_doc`
  before every `relayout_chrome_host()` call — no separate dirty flag, the rebind is cheap. `<body>`
  gets `data-theme`/`data-layout`/`data-profile` (from `Lumen::dark_mode`/`vertical_tabs.visible`/
  `profile_menu.active_entry()` via the new `panels::profile_menu::slug_for_profile`, `None` omits
  the attribute for a non-seeded profile rather than guessing). `#sbTabs`/`.sb-workspaces` are fully
  rebuilt from `tab_strip.tabs`/`workspace_panel.workspaces` on every call: the frozen design
  reference has no `<template>` markup at all, so rows are built with `Document::create_element`/
  `create_text` instead of cloning one (more robust than depending on one particular static demo
  row's exact class/child shape) — icon glyphs (favicon symbol, `×` close icon) are simplified to a
  first-letter fallback, a documented visual-finish gap, not a DoD item. Each rebuilt tab
  row/workspace button carries `data-tab-id`/`data-ws-id` so `dispatch_chrome_action` can resolve a
  click back to a real `TabEntry`/`WsEntry`; `SelectTab`/`CloseTab`/`SelectWorkspace`/`AddWorkspace`
  now call the real `switch_tab`/`close_tab`/`workspace_panel.set_active`/`workspaces.create`
  (`dispatch_chrome_action` gained an `event_loop: &ActiveEventLoop` parameter for `close_tab`).
  `open_new_tab`/`close_tab`/`switch_tab`, the legacy profile-menu popover's switch branch, and
  `close_settings_panel` (theme/tab-layout) each call `relayout_chrome_host()` directly too, so the
  chrome stays in sync regardless of whether the change came through the new chrome dispatch or
  legacy UI. `SetProfile` clicked *in the new chrome* is still a no-op — the profile-menu popover
  itself remains a legacy overlay (CC-9/10) — but a profile switch made through that legacy popover
  is already reflected (`bind_model` reads `profile_menu` fresh every call). Shields count/downloads
  progress from the `ChromeModel` brief description are not bound — outside the explicit DoD
  sentence, a follow-up. 5 new tests in `model.rs` (`cargo test -p lumen-chrome`: 21/21) + 2 in
  `panels::profile_menu` (`slug_for_profile`); real click/hover via OS injection not driven — same
  gap CC-5 documented, no injector in this environment.
- **CC-7 toolbar/omnibox** (`src/model.rs`'s new `OmniboxModel`/`bind_omnibox`;
  `crates/shell/src/address_bar.rs`'s new `chrome_omnibox_value`; `crates/shell/src/main.rs`): the
  padlock/star/shield icons and the field itself render from the static asset (already engine-drawn
  since CC-4) — only the omnibox's *dynamic* pieces needed wiring. `chrome_omnibox_value` mirrors
  `build_inline_field`'s not-focused/focused branching (current URL vs. live `address_bar` input or
  selected suggestion, both IDN-guarded via `guard_display_text`) and feeds
  `ChromeModel::omnibox: OmniboxModel { value, warning }`; `bind_omnibox` writes `value` onto
  `#omniInput`'s `value` attribute and toggles `#omniWarn`'s `.show` class (rebuilding its message
  `<span>` only while a warning is present). `:focus`/`:focus-within` styling comes from
  `set_interactive_state`'s third argument, now `Some(#omniInput)` exactly while
  `address_bar.is_open()`. The caret stays hand-painted (`FillRect` in `overlay_buf`, anchored to
  `Lumen::chrome_omni_input_rect` — `#omniInput`'s post-layout rect, captured non-destructively each
  `relayout_chrome_host` pass) since no native `<input>` caret exists; hidden while a dropdown
  suggestion is selected, matching the old overlay's behavior. A click inside `#omniInput` is
  special-cased in the chrome hit-test (no `data-action` exists to generate an `onfocus` handler
  from) to open `address_bar` exactly like the legacy `toolbar::ToolbarHit::Omnibox` branch it
  mirrors. **Fixed during review:** `relayout_chrome_host()` only re-binds+re-lays-out on explicit
  triggers (resize, a dispatched action, …), not every `RedrawRequested` — the omnibox needed a call
  after *every* `address_bar`-mutating key/mouse path (typed char, Backspace, arrow-key suggestion
  select, Escape, Enter/commit, the click-to-open branch, and both `OpenAddressBar` shortcuts —
  Ctrl+L/F6 and the command palette) or the engine-rendered field would visibly freeze while typing;
  all of those call sites now call it (no-op off the flag). The suggestion **dropdown** itself is out
  of this slice's DoD — arrow-key selection already works (`address_bar`'s own state), it just isn't
  painted until CC-9 migrates it. `cargo test -p lumen-chrome` (24/24, 3 new) + `cargo test -p
  lumen-shell` (1701/1701, 4 new in `address_bar::tests`) + `cargo clippy -p lumen-chrome -p
  lumen-shell --all-targets -D warnings` green.
- **CC-8 sidebar + workspace tab-bar, both layouts** (`src/model.rs`: `ChromeModel::sidebar_collapsed`,
  `ChromeTabModel::is_child`/`container_color`, `ChromeWorkspaceModel::color`, plus the new
  `rebuild_hbar_tab_list`/`rebuild_hbar_ws_list` mirrors of CC-6's `#sbTabs`/`.sb-workspaces`
  rebuilds; `crates/shell/src/main.rs`): `bind_model` now also toggles `#sidebar.collapsed`
  (`ChromeAction::ToggleSidebar` flips the new `Lumen::chrome_sidebar_collapsed` field and calls
  `relayout_chrome_host`), rebuilds `.hbar-tabs`/`.hbar-ws` — previously left showing the asset's
  static demo rows regardless of layout, since only the vertical sidebar's containers were bound —
  so switching to the horizontal layout (`vertical_tabs.visible = false`) now reflects real tab/
  workspace state there too. `ChromeTabModel::is_child` (from `TabEntry::opener_id.is_some()`, tree-
  style tabs 7A.2) drives the `.child` class + a `.tree-line` connector span; the asset's CSS only
  indents one nesting level (`.tab-row.child`), so a grandchild renders at its parent's indent rather
  than one level deeper — a limitation of the frozen reference itself, not this binding (deeper trees
  already work correctly in the legacy `tree_tabs.rs` panel, which isn't CSS-indent-limited).
  `container_color`/workspace `color` are `#RRGGBB` strings (`Lumen::chrome_hex_color`, dropping
  alpha) written as `.container-stripe`'s `style="background:…"` and `--ws-color` respectively — the
  latter feeds both `.ws-item`/`.hbar-ws-pill`'s CSS custom property and the `.ws-icon` swatch
  background directly (no CSS `var()` needed there). Tab drag-and-drop stays on the legacy pixel-math
  mechanic per the brief (CC-8's DoD doesn't require porting it) — it already operates over
  `chrome_layout`-derived rects via `page_offset()`/hit-test since CC-5, so nothing new was needed for
  it to keep working under the flag. The spinner's `@keyframes spin` is inert (no CSS-animation ticker
  runs over the chrome document yet, CC-11) — the asset's `.spinner` element renders static, which is
  in-scope: CC-8's DoD only requires both layouts functional, not chrome animations. 8 new tests in
  `model.rs` (`cargo test -p lumen-chrome`: 28/28) + `cargo test -p lumen-shell` (green, unchanged
  count — no new shell-side branching beyond the `ChromeAction::ToggleSidebar` one-liner and the
  `chrome_model_snapshot` field additions) + `cargo clippy -p lumen-chrome -p lumen-shell
  --all-targets -D warnings` green.

- **Content views + right sidebar** (CC-10b): `#view-page/history/bookmarks/settings` — 4
  mutually-exclusive `.view.active` (`ChromeContentView`, `bind_content_view`), derived from whichever
  legacy panel (`history_panel`/`bookmark_panel`/`settings_panel`) is `visible`. History/bookmarks
  render real data (`HistoryPanel::rows`/`BookmarkPanel::folders`/`visible_entries()`) via
  `bind_history`/`bind_bookmarks`; per-row actions (star/copy/delete, folder click) stay unbound — the
  frozen markup carries no `data-action` hooks on them. Settings section-nav switches for real
  (`Lumen::chrome_settings_section`, independent of the legacy `SettingsSection` enum — the design's 6
  tabs and the legacy 7 don't line up); only the 2 Adblock & Fingerprinting toggles with clean 1:1
  backing fields are bound read-only (`bind_settings`). `#rightSidebar` merges the legacy
  independently-dockable `ai_panel`/`sidebar` into the design's single tabbed panel, kept mutually
  exclusive under the flag; it's the CC track's first engine-chrome panel that's a real flex sibling
  of `#contentArea` (not an overlay) — `Self::dockable_sidebars()` now excludes `ID_AI`/`ID_SIDEBAR`
  under the flag to avoid double-subtracting their width from the legacy scroll-clamp calculation.
  Also fixed: 5 legacy panels' `MouseInput` hit-tests (plus, found by the same audit,
  `print_panel`/`cert_panel`'s — a CC-10a gap) were gating paint but not the click hit-test, so an
  invisible legacy panel could still swallow clicks meant for the page underneath — same bug class
  CC-5 fixed for the tab-strip/toolbar.

- **Animations + transitions** (CC-11): two new `Lumen` fields —
  `chrome_animation_scheduler`/`chrome_transition_scheduler` — mirror the page's own scheduler pair
  but as fully separate instances, ticked on every `RedrawRequested` and synced at the end of
  `relayout_chrome_host`. Separate instances are a correctness requirement, not a style choice:
  `chrome_doc` and the page `Document` number `NodeId`s independently (both start at 0), so a shared
  scheduler would collide entries between the two trees (regression-tested:
  `chrome_transition_scheduler_stays_independent_of_page_scheduler_for_same_node_id`). Unlike the
  page scheduler, `chrome_animation_scheduler` is never `.clear()`-ed — chrome's DOM nodes persist for
  the process lifetime (no reload equivalent), and `relayout_chrome_host` runs far more often than
  page relayouts (any hover/click), so clearing on every pass would restart the `.spinner`'s
  `infinite` animation on every interaction. Applied via the same compositor-offload path the page
  uses (`AnimationFrame::to_compositor_frame` — opacity/transform/color/background-color patched into
  `chrome_dl` without a second relayout): this animates `.spinner` (`@keyframes spin`), the
  `.hist-actions` hover-opacity fade, and `.toggle .thumb`'s transform/background. `width` transitions
  (`#sidebar`'s collapse, `.dl-progress-fill`) stay unanimated — `width` isn't in
  `TransitionScheduler::sync`'s Phase-0 animatable-property table for either document, a pre-existing
  engine limitation, not a CC-11 gap. 1 new test (`cargo test -p lumen-shell`: 1704/1704) +
  `cargo clippy -p lumen-shell --all-targets -D warnings` green. Live smoke
  (`LUMEN_CSS_CHROME=1 LUMEN_FRAME_LOG=1`, ~45s) painted multiple frames with no panics.

- **BUG-341/CC-14 follow-up: `bind_model` list rebuilds preserve NodeId** (`src/model.rs`'s
  `reconcile_row_list` + `update_tab_row`/`update_hbar_tab`/`update_workspace_item`/
  `update_hbar_ws_pill`/`set_text_in_place`): `rebuild_tab_list`/`rebuild_hbar_tab_list`/
  `rebuild_workspace_list`/`rebuild_hbar_ws_list` used to `remove_children_with_class` + rebuild every
  row from scratch on every single `bind_model` call, regardless of whether `tabs`/`workspaces`
  actually changed — every row (and every descendant: fav/title/close-or-badge) got a fresh `NodeId`
  every relayout. `reconcile_row_list` now matches existing rows by position and updates them in
  place (`set_text_in_place` mutates the existing text node's content instead of detach+recreate) —
  only a genuine shape change (`is_child`/`container_color` presence/`sleeping` flip) falls back to
  rebuilding that one row's children (the row itself keeps its id either way). This was necessary
  (not sufficient) for `layout_mutation_incremental`'s `graft_geometry`, which matches subtrees by
  node id — see BUG-341 for why the incremental switch still doesn't help (a separate
  `lumen-layout`-crate inefficiency in `graft_geometry` itself). 6 new tests in `model.rs`
  (`cargo test -p lumen-chrome`: 55/55) assert identity survives an unchanged rebind, a title-only
  change, a shrinking list, and a shape-changing (`sleeping`) flip.

## Deferred

- **`<template>` markup + `templates::IDS` population**: the frozen design reference has none —
  CC-6's list rebuilds (tab rows, workspace buttons) construct nodes programmatically instead (see
  above). Revisit if/when the reference grows real `<template>` markup (e.g. omnibox suggestions,
  download cards, history entries in CC-9/CC-10) rather than retrofitting it onto CC-6's rebuilds.
- Tab-row icon glyphs (favicon symbol, `×` close icon) in `bind_model`'s rebuilt rows — simplified to
  a first-letter fallback; visual finish, not blocking any DoD.
- Shields-count / downloads-progress `ChromeModel` binding — mentioned in the CC-6 brief's
  description but not required by its DoD sentence; same rebuild pattern as tabs/workspaces once
  picked up.
- `SetProfile` dispatched from the *new* chrome (profile-menu popover click) — still routed through
  the legacy popover; `SetPermission`/`ClosePalette`/`CloseModal`/`OmniGo` were wired by CC-9/CC-10,
  and `SetSettingsSection`/`SetSidebarTab`/`CloseRightSidebar` by CC-10b (see below) — the remaining
  demo-only `ChromeAction`s dispatched as no-ops (`ArchiveCard`, `ToggleSwitch`,
  `ToggleFocusTimer`, `ToggleFocus`, `SetDevtoolsTab`) have no clean 1:1 backing state or (for
  `ToggleSwitch`) no way to resolve which of 6 identical `.toggle` elements was clicked from
  `data-action` alone. `ToggleSidebar` is wired (CC-8).
- Omnibox suggestion **dropdown** rendering (CC-9): keyboard `ArrowUp`/`ArrowDown` selection already
  updates `address_bar`'s own state (and `chrome_omnibox_value` already reflects a selected
  suggestion in `#omniInput`'s value), but the dropdown list itself is not painted under the flag —
  the legacy dropdown builder is gated off alongside the rest of the legacy toolbar (CC-4).
- Chrome-document CSS-animation ticking (CC-11): `.spinner`'s `@keyframes spin` is present in the
  asset and parses fine (CC-3's build-gate already validated it), but no `AnimationScheduler`
  instance runs over `chrome_doc` yet — only the page document is ticked
  (`Lumen::animation_scheduler`). A "restoring…" tab row renders the spinner element statically until
  CC-11 wires a chrome-side scheduler instance into the frame loop.
- Tree-tab indentation beyond one level (`.tab-row.child`): the asset's CSS has no `depth`-scaled
  indent rule, so `ChromeTabModel::is_child` collapses `tabs::tree::depth_of`'s full depth to a single
  boolean — a grandchild renders at the same indent as its parent's direct children. Would need a new
  CSS construct in the reference (out of scope for CC-8/CC-CSS-*, this is a markup gap not an engine one).

## Invariants

- `assets/chrome/chrome.html` is generated (`python scripts/gen_chrome_assets.py`) — never hand-edit;
  a rendering mismatch vs. `docs/design/lumen-v3_3.html` is an engine bug, not something to patch
  in the asset.
- `src/gate.rs` must stay self-contained (its own `use`s only, no `super::`/crate-root references):
  `build.rs` splices it in verbatim via `include!`, so anything importing from outside the file
  would need duplicate `use`s at two different inclusion sites.
- `chrome_gen.rs` is regenerated by every `cargo build`/`check`/`test` of this crate (`OUT_DIR` is
  not committed) — never assume its content between builds; read `ids`/`ChromeAction` from the
  compiled crate, not by grepping `target/`.

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
- **CC-4 runtime host** (`crates/shell/src/main.rs`, originally behind `LUMEN_CSS_CHROME=1` read once
  at startup — `run_window_mode`'s `css_chrome_enabled`, now the default since CC-14, see below):
  [`parse_document`] parses
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

- **BUG-341 S5 (superseded by S6 below): `ChromeModel` (+ all ~20 nested model types) derives
  `PartialEq`** (`src/model.rs`) — used by S5's `Lumen::relayout_chrome_host` to gate the incremental
  path on whole-model equality (a coarse stand-in for a real diff). The derive is still present (no
  other code currently reads it) but `relayout_chrome_host` no longer compares by it — see the S6 bullet
  below for what replaced it.

- **BUG-341 S6: `bind_model_tracked`** (`src/model.rs`) — like `bind_model`, but also returns the
  `HashSet<NodeId>` of nodes it actually touched (a selector-relevant attribute/class value changed, or
  a `reconcile_row_list`-driven container gained/lost a row), so `Lumen::relayout_chrome_host` can feed
  `lumen_layout::style::restyle_root_set_for_node_change` a real per-cycle diff instead of S5's
  whole-`ChromeModel`-equality gate. Implemented by instrumenting the handful of shared low-level
  mutation primitives every `bind_*` helper already funnels through — `set_attr`/`remove_attr` (record
  only when the value actually differs — `bind_model` writes every field unconditionally every cycle),
  `remove_children_with_class` (record the container only when it actually detaches a child), and
  `reconcile_row_list` (record the container only when `items.len() != existing.len()`, i.e. a genuine
  add/remove — a same-count reorder/update is already caught by the per-row `set_attr` calls) — not by
  threading a parameter through every individual `bind_*` function. A thread-local `RefCell<Option<
  HashSet<NodeId>>>` collects the touched set only while `bind_model_tracked` is on the call stack;
  plain `bind_model` (e.g. the very first bind, before any previous cascade cache exists) pays no
  tracking cost. Newly-created nodes need no explicit reporting: `incremental_precompute_counters`
  already force-recomputes any node absent from `prev_styles` — see
  `crates/engine/layout/subsystems/layout.md` and BUG-341 "S6".
- **BUG-341 S16 — the tracker is now complete for *content*, and that completeness is structural.**
  `bind_model_tracked` returns `ChromeMutations { selector, content }`: `selector` is S6's set above
  (drives the cascade root-set), `content` additionally names every node whose text data or child list
  moved (drives `lumen_layout::counters::ContentDirty::Nodes`, i.e. which `LayoutBox` subtrees may be
  cloned rather than rebuilt). A miss here is not a slow frame — it is a **stale box on screen** — so
  the invariant is not maintained by inspection: every raw `Document` mutator goes through
  `attach_child` / `insert_child_before` / `detach_node` / `set_attr` / `remove_attr` / `set_text`,
  and `every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive` scans this file's own source
  (`include_str!`) rejecting any un-marked `doc.append_child(` / `doc.insert_before(` / `doc.detach(` /
  `doc.get_mut(`. If it fails on your new code, route the call through a wrapper — don't add the marker.
  **Gotcha this surfaced (the same shape as `bind_palette`'s, two slices later, and it would have
  silently undone S15):** twelve cells (`bind_cert`'s title/value/fingerprint, `#findCount`,
  `#statTrackers`, the bookmarks/history titles) called `set_text`, which detached and recreated its
  text node on *every* bind, changed or not. Nothing noticed while only selector-relevance was tracked;
  the moment content was, an unchanged rebind reported twelve content-dirty nodes and would have
  cancelled whole-document box reuse on every hover frame. `set_text` absorbed `set_text_in_place`'s
  compare-then-write-in-place body — there is now one text setter, and it is a genuine no-op when the
  string is unchanged. `bind_omnibox`'s warning banner got the same treatment. **Any new binding must
  compare before it writes**; `bind_model_tracked_reports_no_content_for_an_unchanged_model` is the
  gate.
  **Gotcha this surfaced**: `bind_palette` used to unconditionally remove+recreate its `.cp-empty`
  "nothing found" placeholder on *every* call whenever `results` was empty (the common case — palette
  normally closed), which made `#cpList` permanently "touched" and needlessly widened `dirty_roots`
  every single cycle, including pure hover/focus/active transitions that never touch the palette at
  all. Fixed by skipping the remove+recreate when the placeholder is already showing. The other five
  `remove_children_with_class` call sites (`bind_history` ×2, `bind_bookmarks` ×2, `bind_dropdown`,
  `bind_downloads`) don't have this pattern — an empty model list there is already a genuine no-op (0
  removed, 0 created) — but a *future* dumb-rebuild helper added to this file should check for the same
  trap before assuming "empty results" is a no-op. Measured: `CC12_KEY` (typed omnibox text, changes
  every cycle) ~30% p50 win, the first real improvement on that fixture since BUG-341 opened; `CC12_
  HOVER` (S3's own worst-case fixture) unaffected, as expected — see BUG-341 "S6" for full numbers.
- **BUG-341 S17 — the tracker reports attribute *names*, not just node ids.** `ChromeMutations::selector`
  is now `HashMap<NodeId, SelectorTouch { attrs: BTreeSet<String>, structural: bool }>`:
  `set_attr`/`remove_attr` record the name they wrote (`record_attr`), `reconcile_row_list` and
  `remove_children_with_class` record a child-list change (`record_structural`). The name is what lets
  `lumen_layout::style::restyle_root_set_for_node_change` ask, per node, whether any selector could
  reach a *sibling* from a compound matching it — otherwise it must widen every change to the parent's
  whole subtree, which on a one-character omnibox keystroke re-cascaded all 12 elements of
  `div.omnibox` to byte-identical styles. A sheet-wide "does any selector use `+`/`~`" test would give
  the same answer on today's `chrome.html` and lose it the day someone adds one unrelated `.a + .b`
  rule; with the name in hand, that rule only widens the nodes its left compound can actually match.
  `structural` stays conservative — no attribute name describes "the child list moved", and
  `:nth-child`/`:empty`/sibling combinators all react to it. See BUG-341 "S17".

- **CC-14: default flip** (`crates/shell/src/main.rs`): `css_chrome_enabled` is now `true` unless
  `LUMEN_LEGACY_CHROME=1` — engine chrome ships by default; the opt-in `LUMEN_CSS_CHROME=1` flag CC-4
  introduced is retired (same shape as ADR-018's V8 cutover, see ADR-021). Explicit parity checklist
  (navigation/tabs/panels/themes/DPI-zoom/split-view, mapped to which scenarios are engine-rendered vs.
  permanently-legacy-overlay) and what it found — a latent MCP/BiDi coordinate bug in
  `resolve_automation_target` (hardcoded `toolbar::CHROME_H`/`left_dock()` instead of the existing
  single-source-of-truth `page_offset()`, harmless while the flag defaulted off, would have silently
  misfired every automated click/type once engine chrome became default — fixed in the same slice) —
  documented in `docs/tasks/p1-css-chrome.md` §CC-14, not duplicated here. `cargo test -p lumen-chrome`
  (61/61), `cargo test -p lumen-shell` (1721+1/1722), `cargo test -p lumen-a11y` (23+134/157) green;
  `python graphic_tests/run.py --continue-on-fail` run against the new default (mandatory — the flip can
  move pixels near TEST-00's magenta calibration).

- **CC-15-3: legacy chrome paint/hit-test removed** (`crates/shell/src/{toolbar.rs,tabs/strip.rs,
  tabs/archive.rs,address_bar.rs,main.rs}`): everything the `!self.focus.active &&
  !self.css_chrome_enabled` gate guarded is gone (~2100 lines) — `build_toolbar`/`build_tab_bar`/
  `build_inline_field`/`build_dropdown` and friends. Geometry constants (`CHROME_H`, `avatar_x()`,
  `TAB_BAR_HEIGHT`, `LAYOUT_BTN_W`, `SETTINGS_BTN_W`) and data types (`TabStrip`, `TabEntry`,
  `TabLayout`, `ArchivedTab`) stay — read unconditionally by `content_layout_viewport()`,
  `resumed()` and the keep-forever panels. Two things the cut-out surfaced, both fixed in the same
  slice because deletion would otherwise have dropped behaviour silently: the removed layout-toggle
  button was the only caller of `set_tab_layout` outside the settings-panel snapshot (now
  `Lumen::persist_tab_layout()`, called from the keyboard/palette toggles), and
  `chrome_model_snapshot` fed `ChromeSuggestionModel` raw `label()`/`sub_label()` while the removed
  `build_dropdown` punycode-guarded both — DS-6's IDN spoof guard was bypassed for `#omniDropdown`
  rows since the CC-14 flip (now `address_bar::chrome_suggestion_text()`). Three parity gaps filed
  rather than silently deleted, their data kept under `#[allow(dead_code, reason = "BUG-NNN: …")]`:
  BUG-408 (archive panel unreachable), BUG-409 (tab-group colours unrendered), BUG-410 (dropdown
  row type tag lost). `strip::hit_test`/`TabHit` were kept at the time — the live, ungated
  right-click tab-context-menu path still used them, which was itself a second BUG-404 site.
  **BUG-404 (fixed 2026-07-31):** the right-click path now resolves the tab under the cursor via
  `Lumen::chrome_hit_test` + `data-tab-id` (same mechanism `ChromeAction::SelectTab` uses for a
  left-click); `strip::hit_test`/`TabHit` had no remaining caller and were removed along with the
  private helpers/tests that only they used.

- **CC-15-4: legacy paint of ten gated `panels/*` removed** (`crates/shell/src/panels/{bookmark,
  print,settings,cert,history,ai,sidebar,shields,permission}_panel.rs`, `command_palette.rs`,
  `main.rs`): each module's `build_panel` (plus `settings_panel`'s `tooltip_for`/`build_tooltip`),
  every paint-only helper that fell dead with it (`emit_*`/`push_*`/`render_*`/`tt_*`/`txt`/
  `truncate*`/`uniform_radii`), the paint-only colour/metric constants, the tests of the removed
  builders, and the gated paint call sites in `main.rs` — ~3.5k lines. Panel *state* and `hit_test`
  stay: `chrome_model_snapshot` reads the state, and the still-gated `MouseInput` branches read the
  hit-tests. Two follow-ons this cut-out surfaced: the `settings_hover` field plus its `CursorMoved`
  writer were dead work (they fed only the removed tooltip — every mouse move with the settings panel
  open recomputed hover and forced a redraw for nothing; removed), and the three *ungated* hit-tests
  (`command_palette`, `shields_panel`, `permission_panel`) remain live — while the engine chrome's
  own overlay is open, a click inside the panel's legacy rectangle can be swallowed by an invisible
  hit-test or activate the wrong row. Unlike the CC-15-3 tab-context-menu site (BUG-404, fixed), this
  one isn't a simple offset swap — `.cp-row` carries no `data-action`/`data-idx` at all, so removing
  the legacy hit-test would break palette-row activation outright; needs either a measured chrome-node
  rect or new `ChromeAction`s (tracked as [BUG-461](../bugs/BUG-461-OPEN.md)). One parity gap filed:
  BUG-411 (`#permPopover` carries neither the current domain nor the shields on/off indicator, and
  has rows only for Camera/Microphone — Notifications and Clipboard are unreachable from the UI).

- **CC-15-5: three orphan `Palette` fields removed** (`crates/shell/src/panels/themes.rs`):
  `toolbar_bg`, `tab_sleep_bg`, `tab_hibernate_bg` — the only fields of the 17 with no reader outside
  `themes.rs` after CC-15-3 took out `toolbar.rs`/`tabs/strip.rs` paint. The remaining 14 are live
  (the keep-forever `vertical_tabs`/`tree_tabs`/`note_viewer` overlays read them).

- **CC-15-6: rollback flag removed** (`crates/shell/src/main.rs` + seven `panels/*`, `find.rs`):
  the `LUMEN_LEGACY_CHROME` env var and the `css_chrome_enabled` field are gone, along with all 24
  branches they gated — every legacy side deleted, every engine side kept as the only path
  (`page_offset`, `point_over_chrome`, chrome hover/active tracking, click routing, cursor icon,
  the chrome animation/transition tick, the render-time page offsets — the last of which was a
  second copy of `page_offset()`'s formula and now calls it). Two shape changes: `chrome_doc` is
  always `Some` (the `Option` stays — 20+ readers go through it — which leaves `chrome_ax_nodes`'
  DS-17 synthetic fallback as an unreachable `None`-arm), and `dockable_sidebars()` shrank from 4
  entries to 2 (`ID_AI`/`ID_SIDEBAR` were entries with a permanent `visible: false` since CC-10b;
  entries with no purpose, not sidebars with no width). Removing the gate made seven panels' legacy
  `hit_test` unreachable — the CC-15-4 method ("delete the gate, let clippy enumerate the dead")
  produced 84 warnings in one pass, all swept: `find.rs`'s whole bar overlay, the panels'
  `hit_test`/`*Hit`/layout constants/`SettingsSection`, and `main.rs`'s `bookmark_anchor`/
  `history_panel_anchor`/`finish_bookmark_drop` plus the bookmark drag machinery. One behaviour was
  ported rather than dropped: the legacy find bar's `ERR` marker for an invalid regex now goes into
  `ChromeFindModel::count_label` (without it an invalid pattern reads as "0 matches"). Four parity
  gaps filed — BUG-419 (no colour for that `ERR`), BUG-420 (`#printOverlay` prints nothing and binds
  no print setting), BUG-421 (`#view-settings` writes no setting — `ToggleSwitch` has been a no-op
  since CC-9), BUG-422 (no actions on history/bookmark entries) — with the now-readerless state kept
  under `#[allow(dead_code, reason = "BUG-NNN: …")]`.

- **BUG-408 fix (2026-07-31, P1): tab-archive panel added to the engine chrome.** The frozen design
  reference had no archive UI at all (unlike downloads/history/bookmarks, which CC-9/CC-10b already
  ported) — `.nt-restore` on `about:newtab` ("Восстановить закрытые") was the only pre-existing hook,
  and it was purely decorative (no `onclick`). Fixed by extending the reference itself (not
  `assets/chrome/chrome.html` directly — the generator's "changes only through the reference" rule,
  CC-13, applies to genuinely new markup too): a new toolbar button `#archiveToggleBtn`, `.nt-restore`
  wired to the same action, and `#archivePanel`/`.arc-list`/`.arc-card` (1:1 structural mirror of
  `.downloads-panel`/`.dl-list`/`.dl-card`, plus a `.arc-stripe` container-colour strip reusing the
  tab row's `.container-stripe` flex-child trick rather than absolute positioning). Three new
  `scripts/gen_chrome_assets.py` onclick→data-action mappings (`toggle-archive`/`archive-restore`/
  `archive-dismiss`) plus two `ARIA_LABEL_RULES` entries. `crates/chrome/src/model.rs` gained
  `ChromeArchiveEntryModel`/`bind_archive`/`build_arc_card` (restore/dismiss buttons carry their own
  `data-archive-id` copy, same reason `.tab-close` carries its own `data-tab-id`). `main.rs`'s
  `dispatch_chrome_action` gained `ToggleArchive`/`ArchiveRestore`/`ArchiveDismiss`, reusing the
  `archive.take`/`navigate_to(PageSource::Url(...))` logic that already existed in the legacy
  pixel-geometry click handler (that handler is not deleted — `hit_test_panel` is reachable
  unconditionally per CC-15-3 — but is now effectively dead in practice, since the only writer of
  `archive.visible` is the new `ToggleArchive` action). `ArchivedTab::{title,container}`'s
  `#[allow(dead_code)]` markers are gone — both fields are read by `chrome_model_snapshot` now.

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
- `UA_DEFAULTS` carries two rules, both on `html` and both relying on inheritance: `user-select:none`
  (CC-CSS-6) and `font-family:'Golos Text'` (BUG-128 ч.2). The second exists because the chrome
  document is laid out by the ordinary engine and therefore starts from
  `lumen_layout::ComputedStyle::root()`, whose default family became `serif`; without the pin every
  chrome label with no explicit `font-family` would render in Times New Roman. `'Golos Text'` is a
  reserved family name in both render backends — it resolves to the bundled face without the system
  `FontProvider`, so chrome typography does not depend on what is installed on the machine.

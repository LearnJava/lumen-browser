# lumen-chrome

Compile-time bridge from the CC design asset to typed Rust (CC track, `docs/tasks/p1-css-chrome.md`).

## Scope

`build.rs` parses `assets/chrome/chrome.html` with the real `lumen_html_parser`/`lumen_css_parser`
(the same parsers the future runtime chrome host uses) and fails the build if the reference uses a
CSS property or selector `lumen-layout` does not implement. On success it code-generates
`OUT_DIR/chrome_gen.rs`, `include!`-d into `lib.rs`: element-id string constants, a typed
`ChromeIds` resolver, the `ChromeAction` enum (from `data-action` attribute values), and the
`<template>` id registry.

**Not in scope:** `ChromeModel` → DOM mutation (CC-6). The runtime host (parse-once +
relayout-on-resize + paint, CC-4) and hit-test/hover/dispatch (CC-5) are done — see below and
`crates/shell/src/main.rs`.

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
- **Codegen** (`build.rs`): `ids` module (one `&str` const per `id`-carrying element, 94 in the
  current asset); `ChromeIds` struct + `ChromeIds::resolve(&Document) -> Result<Self, ChromeIdError>`
  (no `unwrap`/`panic!` — the build-gate guarantees every id exists, but resolution still returns a
  `Result`); `ChromeAction` enum + `from_attr_value`/`attr_value`; `templates::IDS` (currently
  empty — the asset has no `<template>` markup yet, see Invariants).
- **Tests**: 14 `cargo test -p lumen-chrome` (gate unit tests with synthetic CSS fixtures +
  `to_snake_case`/`to_pascal_case` conversion + real-asset `ChromeIds::resolve` + `ChromeAction`
  round-trip + `parse_document` round-trip). Verified manually (not a `cargo test`, since it would
  require corrupting the committed asset): injecting an unknown property into
  `assets/chrome/chrome.html` makes `cargo build -p lumen-chrome` fail with a clear message;
  reverting restores a clean build.
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

## Deferred

- **CC-6**: `<template>` markup + `templates::IDS` population, once `ChromeModel` → DOM diffing
  needs real list cloning (tab rows, omnibox suggestions, download cards, history entries) — also
  the point where the ~17 demo-only `ChromeAction`s dispatched as no-ops by CC-5 get real backing.

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

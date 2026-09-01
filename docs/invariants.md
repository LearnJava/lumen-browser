# Lumen invariants

**One page of things a local change must not break.** Each line is a rule plus where it comes from;
`[gate: …]` marks the ones a machine already checks, so a violation is a build/CI failure rather than a
review finding. Everything else is prose and survives only by being read.

Why this file exists: the rules below were spread across 28 ADRs, `REVIEW.md`, `CLAUDE.md` and
`docs/plan/architecture.md`. A task-scoped agent reads none of those in full, so an architectural
constraint was only enforced when the reviewer happened to remember it.

**This is an index, not the argument.** Each line links the file that explains *why*; when this page and
the linked source disagree, the source wins and this line is the thing to fix. Do not add a rule here
that no ADR/policy file states — decide it there first.

---

## Layering

- Dependency direction is one-way: `lumen-core` → dom/font/parsers → layout → paint → shell. No cycles. — [architecture.md §3](plan/architecture.md) `[gate: cargo refuses a dependency cycle]`
- `lumen-js` sits *above* layout/paint (it depends on both), not beside them. A layout or paint crate must never depend on `lumen-js`. — `crates/js/Cargo.toml`
- CSS logic flows `css-parser` (parsing) → `layout/style.rs` (ComputedStyle, cascade) → `paint/display_list.rs` (wiring). CSS decisions do not live in `shell`. — [REVIEW.md](../REVIEW.md) §Architecture boundaries
- The DOM does not depend on layout. Layout reads the DOM; it does not mutate it. — [architecture.md §3](plan/architecture.md)
- Chrome (tabs, omnibox, panels) is separated from the engine by `BrowserController` + `ChromeView`; state lives in the model, so swapping the view needs no engine change. — [ADR-015](decisions/ADR-015-swappable-chrome-view.md), [ADR-021](decisions/ADR-021-css-chrome-engine.md)

## Engine identity

- **The rendering engine is ours.** Wrapping Chromium/WebKit/Gecko is out of scope, permanently. — [ADR-001](decisions/ADR-001-custom-rendering-engine.md)
- **The JS engine is vendored, and it is V8 (`rusty_v8`) only.** QuickJS/`rquickjs` is gone from the workspace — never target it. — [ADR-018](decisions/ADR-018-v8-cutover.md)
- Engine-independent JS fixes belong in the shared shim (`crates/js/src/shim/*.js`), not duplicated per feature module — but note that per-feature shims (`xhr.rs`, `worker.rs`, …) install their own JS a page-shim fix never reaches. — [CLAUDE.md](../CLAUDE.md) §Engine invariants
- Own-vs-vendored is decided **per sub-decision, by decision ownership**: ours where we decide what correct means (layout, cascade, paint order, browsing contexts), vendored where a committee already decided (file format, Unicode table, OpenType lookup, URL state machine). Test: *if we disagree with the reference implementation, are we wrong by definition?* — [ADR-027](decisions/ADR-027-own-vs-vendored-boundary.md), [ADR-028](decisions/ADR-028-vendoring-is-per-decision.md)
- Permanently forbidden dependencies — `html5ever`, `cssparser`, `stylo`, `taffy`, `hyper`, `hickory-resolver`, `encoding_rs`, `adblock`, `readability`, `tokio`, `egui`/`iced`/`Slint`. Those *are* the engine. — [tech-stack.md §5](plan/tech-stack.md)
- Every new `[dependencies]` entry carries its justification in the commit body (category, trait-anchor, graduation criterion). — [ADR-002](decisions/ADR-002-dependency-policy.md)

## Privacy and surface

- No telemetry, no accounts, no cloud service without explicit opt-in. Every outgoing byte is visible to the user. — [architecture.md §1](plan/architecture.md)
- **Controlled surface**: WebUSB / WebBluetooth / WebMIDI / WebSerial / WebNFC / FedCM / Payment Request are not implemented — declining them is the feature, not a gap. — [architecture.md §1–2](plan/architecture.md)
- Anti-detection is a privacy stack, not a circumvention tool: fingerprint defaults yes; CAPTCHA-solving and IP rotation never. — [ADR-007](decisions/ADR-007-anti-detection-stack.md)
- Global Privacy Control is *derived* from `HttpProfile` (`sends_global_privacy_control`), never an independent toggle; when off the JS property is **absent**, not `false`. — [ADR-026](decisions/ADR-026-global-privacy-control-signal.md)
- Profiles are security contexts with three isolation levels, not cosmetic groupings. — [ADR-020](decisions/ADR-020-profile-security-contexts.md)
- WASM plugins get an inbound-only capability model; OS data is blocked at the Plugin API layer, not by asking the plugin nicely. — [ADR-013](decisions/ADR-013-wasm-plugin-sandbox.md)
- `:visited` always matches `false`. Leaking history through the cascade is not a bug to fix later. — [subsystems/layout.md](../subsystems/layout.md)

## Storage

- Persistent browser storage is SQLite, partitioned into several DBs by lifecycle/write-frequency; a KV store (redb) only for a **measured** blob cache. — [ADR-003](decisions/ADR-003-sqlite-storage.md), [ADR-012](decisions/ADR-012-storage-partitioning.md)
- User data lives in the browser folder (`<exe_dir>/data/`, `browser_data_dir()`) — never `%APPDATA%`/`~/.config`/`lumen_cache_dir()`. — user decision 2026-06-16, [CLAUDE.md](../CLAUDE.md)

## Rendering

- `--screenshot` (CPU, `cpu_raster.rs`) and the live window (wgpu, `renderer.rs`) are **independent implementations of every `DisplayCommand`**, not two callers of one renderer. A new command must be implemented in both, and a match on one proves nothing about the other. — [CLAUDE.md](../CLAUDE.md) §Driving the browser, [ADR-028](decisions/ADR-028-vendoring-is-per-decision.md)
- wgpu is the default backend, femtovg the explicit override and init-failure fallback, both behind `RenderBackend`. — [ADR-010](decisions/ADR-010-render-backend-abstraction.md), [ADR-017](decisions/ADR-017-wgpu-default-backend.md)
- Deterministic output is a hard requirement of the test gates: identical bytes must rasterize identically on any machine. Nothing may seed rendering from host state (e.g. SVG `text`/`system-fonts` stay off for exactly this reason). — [graphic-tests.md](graphic-tests.md), [subsystems/image.md](../subsystems/image.md)
- Paint consumes layout output; it does not mutate layout state. — [architecture.md §3](plan/architecture.md)
- Introspection provenance is a **side index** over the display list, not a field on `DisplayCommand`; box identity is `BoxOrigin { node, role }`, not a bare `NodeId`. — [ADR-025](decisions/ADR-025-identity-propagation.md)

## Threading

- The engine thread is on by default; `Lumen::js_ctx` is therefore `None` in a live window — reach the runtime through `route_task_js`/`route_query_js`/`clone_js_ctx`, never by reading the field. — [ADR-023](decisions/ADR-023-engine-thread-default.md)
- The JS runtime lives on its own thread behind a handle + command channel. Install-time state must be captured **by value** into the native's closure — a `thread_local!` set by the installer reads back its default inside the native. — [ADR-014](decisions/ADR-014-js-runtime-thread.md), [CLAUDE.md](../CLAUDE.md)
- Snapshot message passing between UI/render threads, staged M0–M4. — [ADR-016](decisions/ADR-016-multithreaded-render-pipeline.md)

## Automation surface

- The automation API is a first-class engine surface (`BrowserSession`), not test scaffolding bolted on. — [ADR-006](decisions/ADR-006-automation-api.md)
- Introspection levels L0 (internal) / L1 (`x-` prefixed, unstable) / L2 (stable, versioned). **No L2 before v1.0**, and MCP/BiDi on loopback require a token with no anonymous escape hatch. — [ADR-024](decisions/ADR-024-introspection-api-levels.md)

## Code rules (all machine-checked)

- No `panic!`/`unwrap()`/`expect()` in production code. — `[gate: clippy::panic / unwrap_used / expect_used = deny]`
- `unsafe` only at FFI boundaries, every block carrying `// SAFETY:`. — `[gate: clippy::undocumented_unsafe_blocks = deny]`
- `///` on every public item. — `[gate: missing_docs = deny]` (pre-existing debt behind file-scoped `#![allow]`, [lint-policy.md §10](lint-policy.md))
- A new `.rs` file is ≤2000 lines; an over-size file does not grow unnoticed. — `[gate: scripts/check_file_sizes.py, CI job file-size]`
- No hardcoded version string — everything derives from `CARGO_PKG_VERSION`. One deliberate exception: the `navigator.userAgent` literal in `crates/js/src/shim/web_api_shim_mid_b.js`. — [CLAUDE.md](../CLAUDE.md) §Versioning
- Every member crate carries `[lints] workspace = true`, or it silently escapes all of the above. — [lint-policy.md](lint-policy.md)

## Cross-cutting traps that read as invariants

These are not decisions — they are places where a correct-looking local change is silently wrong.

- Changing the stylesheet set from Rust must move `inline_style_fingerprint` or `stylesheet_link_fingerprint`, or the cascade is never rebuilt. — [BUG-443](../bugs/BUG-443-FIXED.md)
- Anything added to a JS prototype must be non-enumerable. — [CLAUDE.md](../CLAUDE.md)
- A callback the shim makes on the page's behalf is queued as a task, never dispatched inline. — [CLAUDE.md](../CLAUDE.md)
- Nothing tag-specific belongs in `_LUMEN_WRAPPER_MEMBERS` — it shadows every interface prototype. — [CLAUDE.md](../CLAUDE.md)
- A fourth writer of the page display list must splice frame content in, like the three existing ones. — [CLAUDE.md](../CLAUDE.md)

---

## Maintenance

Add a line here when an ADR establishes a constraint that a *local* change could plausibly violate. Do not
add one for a decision that only affects the subsystem it lives in — that belongs in `subsystems/<crate>.md`.
When a prose rule becomes machine-checked, move it to §Code rules and mark the gate; that is the direction
this file should drift ([lint-policy.md](lint-policy.md)).

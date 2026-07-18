# Automation & introspection surfaces — what to use when

Reference for every built-in automation/diagnostic capability of `lumen.exe` and the workspace crates. Read this **before** writing a new debugging script, adding a `time.sleep`, or reaching for pixel-diffing — the browser probably already has a cheaper surface for the job. Audit date: 2026-07-16 (full sweep of shell/mcp/bidi-server/ipc/driver/devtools + scripts/).

Related docs: [`docs/commands.md`](commands.md) (day-to-day commands), [`docs/graphic-tests.md`](graphic-tests.md) (visual pipeline). Improvement tasks for the gaps below: `DEVX-*` rows in [`ROADMAP.md`](../ROADMAP.md) (owner P2).

---

## Quick chooser: task → tool

| I need to… | Use | Caveat |
|---|---|---|
| See the decoded HTML the parser received | `lumen --dump-source <src>` | headless, stdout |
| Check box geometry without pixel-diffing | `lumen --dump-layout <src>` | headless, stdout; text-diffable |
| Check paint order / colors / z-index | `lumen --dump-display-list <src>` | headless, stdout; text-diffable |
| Catch layout/paint-order regressions without GPU/Edge | `python graphic_tests/dump_golden.py` (`--update` to promote, DEVX-3) | fixed page set, text-diff vs `graphic_tests/dump-golden/`; complements the pixel pipeline, doesn't replace it |
| Deterministic screenshot without a window | `lumen --screenshot out.png <src>` | CPU path: no JS execution, not at parity with windowed render (BUG-221) |
| See the load waterfall of one navigation (find the next bottleneck) | `lumen --trace-nav out.json <src>` (PERF-1) | headless CPU path (same as `--screenshot`); Chrome Trace Event Format → open in Perfetto / chrome://tracing / edge://tracing. Phase spans (`fetch-document`→`parse-html`→`fetch-*`/`layout`→`paint`→`first-paint`) + per-resource fetch spans with byte `size`. No DNS/TLS/TTFB sub-split yet |
| User-centric metrics (FCP/LCP/TTI/TBT proxies) over the corpus + regression compare | `python scripts/perf_metrics.py` (PERF-2) over `docs/perf/corpus.txt` | derives metrics from `--trace-nav` spans (no engine code); `--repeat N` (median), `--compare <prev.json>`, `--selftest` (extraction check, no network). Journal `docs/perf/metrics.md`, runs `docs/perf/metrics-runs/*.json`. Headless single paint → fcp/lcp/tti collapse to paint-complete (ttfb/script/tbt/layout/paint are the distinct numbers) |
| Full visual regression vs Edge | `python graphic_tests/run.py` (`--live` = one window per run; SDC-4 will make it default) | see docs/graphic-tests.md |
| Localize which paint optimization causes an artifact | `python graphic_tests/run.py --paint-bisect NN` (DEVX-4) | runs baseline + each `LUMEN_NO_*` flag (see table below) once, prints a diff% table |
| Kill flake from `Date.now()` / `Math.random()` / rAF timestamps | `--deterministic` (+ `--viewport WxH` to keep a non-default window size, DEVX-1) | `--rng-seed`/`--monotonic-clock` are parsed but not wired (see CLI flags below); crates/shell/src/deterministic.rs |
| Drive the real visible window from a script | `--mcp-live-port <N>` (MCP JSON-RPC over TCP) or `--bidi-port <N>` (WebDriver BiDi over WS) | both wired to the live window (SDC-2) |
| Drive a page without a window (CI, no GPU) | `--mcp [url]` / `--mcp-port N` (MCP JSON-RPC, DEVX-5) | `InProcessSession`: CPU screenshot, persistent-V8 `eval`, DOM-level click/type/scroll (no relayout after mutation, BUG-221) |
| Headless tab control (no GPU, CI) | `--ipc-server` (prints `LUMEN_IPC_PORT=<port>`) | CPU screenshots, BUG-221 parity |
| Assert on geometry/cascade/a11y in Rust tests | `lumen-driver::BrowserSession`: `layout_box_by_selector`, `computed_style_snapshot`, `query_a11y` | InProcessSession = full headless pipeline |
| Read console/network/a11y/layout of a live page | MCP resources `resource://console` / `network` / `a11y_tree` / `layout` / `screenshot` | live window only |
| Per-frame paint timings | `LUMEN_FRAME_LOG=1` (or `=2` for top-8 DisplayCommands) | used by scripts/scroll_perf.py |
| Layout/cascade phase timings as a call tree | `LUMEN_PROFILE_TREE=1` | stderr |
| GUI timeline profiler | `cargo run --features tracy` | Tracy client needed |
| Scroll performance benchmark | `scripts/scroll_perf.py`, `scripts/mt_stall_bench.py` | drives `--mcp-live-port` |
| Real-site load perf: live GUI run (tab per site), stats, journal, bug filing | `python scripts/perf_audit.py` over `docs/perf/corpus.txt` (default: one visible window, `new_tab` per site, cumulative RAM); `--phases` = headless per-phase decomposition; full protocol = skill `/lumen-perf-audit` | dev-release build; screenshots via CPU path (BUG-221) |
| Cache/memory growth diagnosis | `LUMEN_MEM_REPORT=1` (~10 s cadence dump) | TEMP instrumentation from BUG-272 |
| Print/pagination check | `lumen --print-to-pdf out.pdf <src>` | A4 |
| Reproduce input-order bugs | `--activity-log` / `--click-log` → `activity.log` | |
| Reproduce a user session | `--import-session <file.lsession>` | URL + scroll restored |
| Network isolation / proxy / Tor testing | `--network-service`, `--proxy <url>`, `--tor [--tor-port N]` | |

---

## CLI flags (crates/shell/src/main.rs, `print_usage()`)

Headless one-shot: `--dump-source` · `--dump-layout` · `--dump-display-list` · `--screenshot` · `--trace-nav <out.json>` (PERF-1, load waterfall as Chrome-trace JSON) · `--print-to-pdf`.
Servers: `--ipc-server [--ipc-port N]` · `--mcp [url]` · `--mcp-port N` · `--mcp-live-port N <src>` · `--bidi-port N` · `--devtools-port N` (CDP, stub — see below).
Determinism: `--deterministic` · `--rng-seed N` · `--monotonic-clock` (parsed into `DetConfig` but **not currently wired** to the JS runtime — only `--deterministic`'s plain on/off reaches `set_deterministic_mode`; the RNG seed always derives from the page URL hash regardless of `--rng-seed`'s value) · `--viewport WxH` (DEVX-1: pins the window's CSS content viewport, overriding `--deterministic`'s 1280×800 default — used by `graphic_tests/run.py --live` to combine determinism with the pipeline's calibrated 1024×720).
Misc: `--maximized` (window opens full-screen — live perf audit) · `--no-scrollbar` (cleaner screenshot crops) · `--activity-log` / `--click-log` · `--import-session` · `--network-service` · `--proxy` · `--tor`.

## MCP (`crates/mcp`) — the richest scripting surface

`--mcp-live-port N <src>` runs MCP JSON-RPC over TCP against the **live window**. All 8 tools wired: `navigate`, `new_tab` (opens a tab, makes it active, navigates — used by the live perf audit to give every site its own tab), `wait` (conditions: `document_ready` / `visible` / `stable` / `network_idle` / `js_idle`), `click`, `type`, `scroll`, `eval` (JS), `query` (CSS selector → DOM nodes). All 5 resources wired: `resource://screenshot` (PNG, CPU path), `resource://a11y_tree`, `resource://layout` (box model JSON), `resource://console`, `resource://network`.

Headless variants `--mcp` / `--mcp-port` (default build, `v8`+`cpu-render` features): `screenshot`/`eval`/`click`/`type`/`scroll` are wired against `InProcessSession` (DEVX-5, complete). Slice 1 gave `screenshot` (CPU tiny-skia path, `cpu-render` — a **default** feature of `lumen-driver`, no GPU adapter needed) and `scroll` (wired to the existing off-main-thread `scroll_page_by`). Slice 2 gave `eval`/`click`/`type` a persistent V8 runtime (`v8`, now also default) installed on the DOM at navigation time (`Arc<Mutex<Document>>`, re-installed on every `navigate()`) — JS-side mutations persist across `eval()` calls, unlike the live window's per-navigation-only runtime — plus synthetic mouse/keyboard event dispatch (`click`/`type` — `mousedown`→`mouseup`→`click` via `_lumen_dispatch_mouse_event`, `lumen_paint::hit_test` for point-based `click`; per-char `keydown`/`input`/`keyup` for `type`).

`graphic_tests/run.py --live` (DEVX-1) also spawns with `--deterministic --viewport 1024x720` (kills Date.now/Math.random/rAF flake in JS tests like TEST-57/129-138 while keeping the pipeline's calibrated viewport) and reads `resource://console` after every test — a `console.error` FAILs the test and its text lands in the HTML report, independent of the pixel diff. `resource://console` for `LiveWindowSession` round-trips through a new `AutomationCommand::ConsoleLog` to the shell's DevTools console buffer (cleared on every `navigate()`). `network`/`layout` resources remain untapped for `--live` (still returns real data only via `InProcessSession`, not the live window — SDC-2 MVP scope).

## Golden dump gate (`graphic_tests/dump_golden.py`, DEVX-3)

Runs `lumen --dump-layout`/`--dump-display-list` over a fixed 6-page set (`samples/page.html`, `samples/test-06-layout.html`, and three `graphic_tests/*.html` pages covering table/grid/flex/transform-stacking) and text-diffs the stdout against committed golden files in `graphic_tests/dump-golden/<page>.{layout,display-list}.txt`. `--update` promotes the current output to the new golden; a mismatch prints a unified diff and exits 1. No GPU, no Edge, no ffmpeg — catches geometry/paint-order/z-index regressions cheaply and cross-platform, complementing (not replacing) the pixel pipeline.

## WebDriver BiDi (`crates/bidi-server`, `--bidi-port N`)

Live-window MVP (SDC-2): `browsingContext.navigate` (blocks on real `document.readyState`) / `captureScreenshot` / `setViewport`, `script.evaluate` / `callFunction` / preload scripts, `input.performActions` (pointer+key subset), `session.*`. Also implemented but **accepted-and-stored only, no live-window effect** (protocol-correct, `BidiState` genuinely updated, but nothing reads it back — [BUG-295](../bugs/BUG-295-OPEN.md), found+tested by DEVX-6's `tests/wpt/verify_devx6_bidi_scenarios.py`): `network.addIntercept` / `continueRequest|Response` / `failRequest` / `setOfflineStatus`, `browser.setTimezoneOverride`, `emulation.setUserAgentOverride`. `storage.getCookies|setCookie|deleteCookies` likewise unverified against a live window (not covered by DEVX-6). Without a window the server falls back to an in-memory stub state. Consumer today: `tools/wptrunner` plugin (session negotiation only).

## IPC (`crates/ipc`, `--ipc-server`)

Length-prefixed bincode over TCP loopback: `CreateTab` / `NavigateTab` / `Screenshot` (CPU PNG) / `CloseTab` (+ `Fetch`/`Ping`/`Shutdown` for the network service). Consumer: `graphic_tests/run.py --ipc`.

## lumen-driver (`crates/driver`) — for Rust-side tests

`BrowserSession` trait; `InProcessSession` runs the full headless pipeline. Read-only resources ideal for **non-pixel regression tests** (DEVX-2): `layout_snapshot`, `layout_box_by_selector`, `all_layout_boxes_by_selector`, `computed_style` / `computed_style_snapshot` (typed), `query_a11y` / `query_a11y_all` (`AxQuery::Role{role,name}` — Playwright-style getByRole), `a11y_tree`, `network_log`, `console_log`, `screenshot`. Setters: `set_fingerprint_profile`, `set_user_agent`, `set_clock`, `set_rng_seed`, `freeze_fingerprint`.

Interaction (DEVX-5, cargo features `v8-backend`/`cpu-render`, both on by default in `lumen-shell`): `click`/`type_text` mutate the DOM directly (link nav, checkbox/radio toggle, `value` overwrite — no synthetic event dispatch), `scroll` drives the existing off-main-thread compositor scroll, `eval` runs against a **persistent** V8 runtime installed on the document at navigation time — unlike `WinitSession::eval`'s one-shot runtime, JS-side DOM mutations are visible to later `eval()`/`click`/`type_text` calls within the same navigation (shared `Arc<Mutex<Document>>`). None of this feeds back into `layout_root`/`flat_tree` — no relayout after a mutation, so `layout_snapshot`/`screenshot` still reflect the DOM as of `navigate()`.

Snapshot-test env vars: `SNAPSHOT_VS_EDGE_STRICT=1` (hard-gate `crates/driver/tests/cases/snapshot_vs_edge.rs`), `SAVE_CPU_SNAPSHOTS`, `SAVE_SNAPSHOTS`, `UPDATE_SNAPSHOTS` (layout/paint golden tests).

## Env toggles (rendering / engine)

| Var | Effect |
|---|---|
| `LUMEN_BACKEND` | Renderer: empty = probe (wgpu first), `femtovg`, `wgpu` |
| `WGPU_BACKEND` / `LUMEN_NO_BACKEND_PROBE` | Force / skip GPU backend probe |
| `LUMEN_ENGINE_THREAD=1` | Off-thread layout (ADR-016 M2.2) |
| `LUMEN_RENDER_THREAD` | Render thread on/off |
| `LUMEN_PRESENT` | Present mode override |
| `LUMEN_NO_FRAME_SKIP` / `LUMEN_NO_SCROLL_COMPOSITOR` / `LUMEN_NO_ANIM_SPLIT` / `LUMEN_NO_BBOX_SCISSOR` / `LUMEN_NO_BBOX_BACKDROP` / `LUMEN_NO_IMAGE_MIPS` / `LUMEN_NO_BAND_BIAS` | Disable one paint optimization each — **the paint-regression bisection kit** (crates/engine/paint/src/renderer.rs), driven automatically by `run.py --paint-bisect NN` (DEVX-4) |
| `LUMEN_SCROLL_BLIT` / `LUMEN_NO_FAST_SCROLL_DEGRADE` | Scroll-blit opt / fast-scroll quality degrade |
| `LUMEN_FRAME_LOG=1\|2` · `LUMEN_PROFILE_TREE=1` · `LUMEN_MEM_REPORT=1` · `LUMEN_BENCH` / `LUMEN_BENCH_ITERS` | Diagnostics (see chooser table) |

## Known stubs — do NOT rely on these

- **CDP `--devtools-port`**: only `Browser.getVersion` is real; `DOM.getDocument` returns an empty document, `*.enable` are ACK stubs, everything else → `-32601`. Use BiDi or MCP instead; no DEVX task (BiDi/MCP cover the need).
- **CPU screenshot path** (`--screenshot`, `--ipc`, `resource://screenshot`, and now headless MCP's `screenshot` tool — DEVX-5): no relayout after JS/DOM mutation, rendering not at parity with the windowed backend (BUG-221) — fine for coarse checks, not for the Edge-diff gate.
- In-app DevTools panels (console/inspector/network in the shell UI) are interactive-only — not scriptable.

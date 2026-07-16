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
| Deterministic screenshot without a window | `lumen --screenshot out.png <src>` | CPU path: no JS execution, not at parity with windowed render (BUG-221) |
| Full visual regression vs Edge | `python graphic_tests/run.py` (`--live` = one window per run; SDC-4 will make it default) | see docs/graphic-tests.md |
| Localize which paint optimization causes an artifact | `LUMEN_NO_*` env flags (see table below) + `run.py --only NN` | A/B bisection, one flag at a time |
| Kill flake from `Date.now()` / `Math.random()` / rAF timestamps | `--deterministic`, `--rng-seed <N>`, `--monotonic-clock` | crates/shell/src/deterministic.rs |
| Drive the real visible window from a script | `--mcp-live-port <N>` (MCP JSON-RPC over TCP) or `--bidi-port <N>` (WebDriver BiDi over WS) | both wired to the live window (SDC-2) |
| Headless tab control (no GPU, CI) | `--ipc-server` (prints `LUMEN_IPC_PORT=<port>`) | CPU screenshots, BUG-221 parity |
| Assert on geometry/cascade/a11y in Rust tests | `lumen-driver::BrowserSession`: `layout_box_by_selector`, `computed_style_snapshot`, `query_a11y` | InProcessSession = full headless pipeline |
| Read console/network/a11y/layout of a live page | MCP resources `resource://console` / `network` / `a11y_tree` / `layout` / `screenshot` | live window only |
| Per-frame paint timings | `LUMEN_FRAME_LOG=1` (or `=2` for top-8 DisplayCommands) | used by scripts/scroll_perf.py |
| Layout/cascade phase timings as a call tree | `LUMEN_PROFILE_TREE=1` | stderr |
| GUI timeline profiler | `cargo run --features tracy` | Tracy client needed |
| Scroll performance benchmark | `scripts/scroll_perf.py`, `scripts/mt_stall_bench.py` | drives `--mcp-live-port` |
| Cache/memory growth diagnosis | `LUMEN_MEM_REPORT=1` (~10 s cadence dump) | TEMP instrumentation from BUG-272 |
| Print/pagination check | `lumen --print-to-pdf out.pdf <src>` | A4 |
| Reproduce input-order bugs | `--activity-log` / `--click-log` → `activity.log` | |
| Reproduce a user session | `--import-session <file.lsession>` | URL + scroll restored |
| Network isolation / proxy / Tor testing | `--network-service`, `--proxy <url>`, `--tor [--tor-port N]` | |

---

## CLI flags (crates/shell/src/main.rs, `print_usage()`)

Headless one-shot: `--dump-source` · `--dump-layout` · `--dump-display-list` · `--screenshot` · `--print-to-pdf`.
Servers: `--ipc-server [--ipc-port N]` · `--mcp [url]` · `--mcp-port N` · `--mcp-live-port N <src>` · `--bidi-port N` · `--devtools-port N` (CDP, stub — see below).
Determinism: `--deterministic` · `--rng-seed N` · `--monotonic-clock`.
Misc: `--no-scrollbar` (cleaner screenshot crops) · `--activity-log` / `--click-log` · `--import-session` · `--network-service` · `--proxy` · `--tor`.

## MCP (`crates/mcp`) — the richest scripting surface

`--mcp-live-port N <src>` runs MCP JSON-RPC over TCP against the **live window**. All 7 tools wired: `navigate`, `wait` (conditions: `document_ready` / `visible` / `stable` / `network_idle` / `js_idle`), `click`, `type`, `scroll`, `eval` (JS), `query` (CSS selector → DOM nodes). All 5 resources wired: `resource://screenshot` (PNG, CPU path), `resource://a11y_tree`, `resource://layout` (box model JSON), `resource://console`, `resource://network`.

Headless variants `--mcp` / `--mcp-port` exist but `screenshot`/`eval`/`click`/`type`/`scroll` return errors there (InProcessSession stubs — DEVX-5).

`graphic_tests/run.py --live` uses only `navigate` + `wait{document_ready}` today; console/network/layout checks are untapped (DEVX-1).

## WebDriver BiDi (`crates/bidi-server`, `--bidi-port N`)

Live-window MVP (SDC-2): `browsingContext.navigate` (blocks on real `document.readyState`) / `captureScreenshot` / `setViewport`, `script.evaluate` / `callFunction` / preload scripts, `input.performActions` (pointer+key subset), `session.*`. Also implemented and **unused by any tooling**: `network.addIntercept` / `continueRequest|Response` / `failRequest` / `setOfflineStatus`, `browser.setTimezoneOverride`, `emulation.setUserAgentOverride`, `storage.getCookies|setCookie|deleteCookies` (DEVX-6). Without a window the server falls back to an in-memory stub state. Consumer today: `tools/wptrunner` plugin (session negotiation only).

## IPC (`crates/ipc`, `--ipc-server`)

Length-prefixed bincode over TCP loopback: `CreateTab` / `NavigateTab` / `Screenshot` (CPU PNG) / `CloseTab` (+ `Fetch`/`Ping`/`Shutdown` for the network service). Consumer: `graphic_tests/run.py --ipc`.

## lumen-driver (`crates/driver`) — for Rust-side tests

`BrowserSession` trait; `InProcessSession` runs the full headless pipeline. Read-only resources ideal for **non-pixel regression tests** (DEVX-2): `layout_snapshot`, `layout_box_by_selector`, `all_layout_boxes_by_selector`, `computed_style` / `computed_style_snapshot` (typed), `query_a11y` / `query_a11y_all` (`AxQuery::Role{role,name}` — Playwright-style getByRole), `a11y_tree`, `network_log`, `console_log`, `screenshot`. Setters: `set_fingerprint_profile`, `set_user_agent`, `set_clock`, `set_rng_seed`, `freeze_fingerprint`.

Snapshot-test env vars: `SNAPSHOT_VS_EDGE_STRICT=1` (hard-gate `crates/driver/tests/cases/snapshot_vs_edge.rs`), `SAVE_CPU_SNAPSHOTS`, `SAVE_SNAPSHOTS`, `UPDATE_SNAPSHOTS` (layout/paint golden tests).

## Env toggles (rendering / engine)

| Var | Effect |
|---|---|
| `LUMEN_BACKEND` | Renderer: empty = probe (wgpu first), `femtovg`, `wgpu` |
| `WGPU_BACKEND` / `LUMEN_NO_BACKEND_PROBE` | Force / skip GPU backend probe |
| `LUMEN_ENGINE_THREAD=1` | Off-thread layout (ADR-016 M2.2) |
| `LUMEN_RENDER_THREAD` | Render thread on/off |
| `LUMEN_PRESENT` | Present mode override |
| `LUMEN_NO_FRAME_SKIP` / `LUMEN_NO_SCROLL_COMPOSITOR` / `LUMEN_NO_ANIM_SPLIT` / `LUMEN_NO_BBOX_SCISSOR` / `LUMEN_NO_BBOX_BACKDROP` / `LUMEN_NO_IMAGE_MIPS` / `LUMEN_NO_BAND_BIAS` | Disable one paint optimization each — **the paint-regression bisection kit** (crates/engine/paint/src/renderer.rs). DEVX-4 automates this |
| `LUMEN_SCROLL_BLIT` / `LUMEN_NO_FAST_SCROLL_DEGRADE` | Scroll-blit opt / fast-scroll quality degrade |
| `LUMEN_FRAME_LOG=1\|2` · `LUMEN_PROFILE_TREE=1` · `LUMEN_MEM_REPORT=1` · `LUMEN_BENCH` / `LUMEN_BENCH_ITERS` | Diagnostics (see chooser table) |

## Known stubs — do NOT rely on these

- **CDP `--devtools-port`**: only `Browser.getVersion` is real; `DOM.getDocument` returns an empty document, `*.enable` are ACK stubs, everything else → `-32601`. Use BiDi or MCP instead; no DEVX task (BiDi/MCP cover the need).
- **Headless MCP** (`--mcp`/`--mcp-port`): `eval`/`screenshot`/`click`/`type`/`scroll` error out (DEVX-5).
- **CPU screenshot path** (`--screenshot`, `--ipc`, `resource://screenshot`): no JS execution, rendering not at parity with the windowed backend (BUG-221) — fine for coarse checks, not for the Edge-diff gate.
- In-app DevTools panels (console/inspector/network in the shell UI) are interactive-only — not scriptable.

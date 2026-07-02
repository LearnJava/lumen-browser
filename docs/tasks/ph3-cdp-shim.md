# Ph3 — lumen-cdp-shim (Chrome DevTools Protocol subset)

**Developer:** P1
**Branch:** `p1-ph3-cdp-shim`
**Size:** L
**Crates:** `lumen-devtools` (extended), optionally new `lumen-cdp-shim`, `lumen-driver`

---

## Status

**Phase 3 (v1.0), Automation track, OPTIONAL — on-demand per ADR-006.**

Do not start this task until the graduation trigger fires (see "Trigger condition" below).
Roadmap references: `docs/plan/phases.md:138`, `docs/plan/engine.md:176`.

---

## Goal

Expose a Puppeteer-compatible subset of the Chrome DevTools Protocol (CDP) over a
WebSocket endpoint, implemented as a thin adapter layer on top of `BrowserSession`
(`crates/driver/src/lib.rs`). The goal is to allow a legacy Puppeteer-driven test suite
to drive Lumen without code changes beyond the `--remote-debugging-port` flag, covering
the Page, Runtime, DOM, Target, and Input domains needed for basic navigation, screenshot,
`evaluate`, DOM queries, and synthetic input.

CDP is not a first-class protocol for Lumen (see ADR-006). BiDi (`lumen-bidi-server`) is
the preferred path. This shim exists solely to unblock named Puppeteer users who cannot
migrate to BiDi.

---

## Trigger condition

**Do not implement this task until ALL of the following are true:**

1. At least one real, named project contacts the Lumen team and states that it:
   - uses Puppeteer (not Playwright, not Selenium) in production or CI,
   - cannot migrate to WebDriver BiDi or `lumen-bidi-server` within its timeline, and
   - names specific CDP methods it requires.
2. The ADR-006 graduation trigger is explicitly declared satisfied in a commit to
   `docs/decisions/ADR-006-automation-api.md` (new "Graduation" section, date + project name).

Until then, the `lumen-devtools` crate remains as-is and no CDP shim code is written.

---

## Current state — reconciling "no CDP code" with the existing devtools crate

`docs/plan/phases.md:138` states: *"До этого CDP-кода в Lumen нет"* (no CDP code in Lumen
until this task). That claim is **factually incorrect** as of 2026-06-22. The `lumen-devtools`
crate already ships a minimal CDP dispatcher:

| File | What it contains |
|---|---|
| `crates/devtools/src/cdp.rs:1-143` | `dispatch(msg) -> String` — JSON-RPC router |
| `crates/devtools/src/server.rs:1-79` | `DevToolsServer::spawn(port)` — TCP listener, WebSocket upgrade, per-connection thread |
| `crates/devtools/src/ws.rs:1-286` | RFC 6455 WebSocket codec (text frames, ping/pong, close) |
| `crates/devtools/src/lib.rs:1-17` | Re-exports `DevToolsServer` |

Implemented CDP methods (`crates/devtools/src/cdp.rs:38-47`):
- `Browser.getVersion` — returns `protocolVersion: "1.3"`, product, userAgent, revision
- `DOM.getDocument` — stub: returns a minimal Document node (`nodeType: 9`, no children)
- `Network.enable` / `CSS.enable` / `Page.enable` / `Runtime.enable` — ACK-only (empty result)

The current `lumen-devtools` dispatcher (`crates/devtools/src/cdp.rs`) is **stateless**: it
receives a raw JSON-RPC string and returns a JSON-RPC string. It does **not** hold a reference
to `BrowserSession`; it cannot navigate, screenshot, evaluate JS, or query the DOM. That is the
core gap this task fills.

The `BrowserSession` trait (`crates/driver/src/lib.rs:62-228`) exposes:

| Session method | Maps to CDP domain |
|---|---|
| `navigate(url)` | `Page.navigate` |
| `screenshot()` | `Page.captureScreenshot` |
| `eval(js)` | `Runtime.evaluate` |
| `query(selector)` | `DOM.querySelector` / `DOM.querySelectorAll` |
| `layout_snapshot()` | `DOM.getBoxModel` (per-node) |
| `a11y_tree()` | `Accessibility.getFullAXTree` |
| `click(target)` | `Input.dispatchMouseEvent` |
| `type_text(target, text)` | `Input.dispatchKeyEvent` |
| `scroll(target, delta)` | `Input.dispatchMouseEvent` (scroll) |
| `wait(cond, ms)` | used internally by `Page.navigate` auto-wait |
| `current_url()` | `Target.getTargetInfo` |
| `console_log()` | `Runtime.consoleAPICalled` (events) |
| `network_log()` | `Network.responseReceived` (events) |

**Architectural conclusion:** this task extends the existing `lumen-devtools` crate rather
than creating a new `lumen-cdp-shim` crate. The WebSocket server and codec are already
correct. The only missing piece is wiring `cdp.rs` dispatcher to a live `BrowserSession`
instance. Whether to keep it as `lumen-devtools` or rename to `lumen-cdp-shim` is a
packaging decision for the implementer (the roadmap uses both names; `lumen-devtools` is
simpler because it already exists and has no rename cost).

---

## Architecture

```text
Puppeteer (Node.js)
  └─ WebSocket ws://127.0.0.1:<port>/json/version
       └─ DevToolsServer (crates/devtools/src/server.rs)
            └─ CdpSession (proposed: stateful, holds Arc<Mutex<dyn BrowserSession>>)
                 └─ cdp::dispatch_with_session(msg, &mut session) -> String
                      ├─ Page domain    → session.navigate / screenshot / wait
                      ├─ Runtime domain → session.eval
                      ├─ DOM domain     → session.query / layout_snapshot
                      ├─ Input domain   → session.click / type_text / scroll
                      └─ Target domain  → session.current_url
```

**Key design constraints:**

- The dispatcher must become stateful: `dispatch(&msg)` becomes
  `dispatch_with_session(&msg, session: &mut dyn BrowserSession)`.
- `DevToolsServer` must accept a factory closure `Fn() -> Box<dyn BrowserSession>` and
  create one session per Target (Puppeteer's `newPage()` creates a new Target).
- Events (console, network) are delivered as unsolicited JSON frames — the existing
  per-connection write loop must be extended to drain an event queue alongside request responses.
- The `lumen-devtools` crate gains a dependency on `lumen-driver`. To avoid a cycle,
  `lumen-driver` must not depend on `lumen-devtools` — this is already the case
  (`crates/devtools/Cargo.toml` depends only on `lumen-core`).
- Zero cost when not started: `DevToolsServer::spawn` is called only when `--cdp-port <N>`
  is passed on the command line. Default build has no running CDP server (ADR-006 §Performance gate).

**Puppeteer connection handshake (`/json/version` endpoint):**

Puppeteer's `puppeteer.connect({ browserWSEndpoint })` first issues an HTTP GET to
`/json/version`. This must return:

```json
{
  "Browser": "Lumen/0.2.0",
  "Protocol-Version": "1.3",
  "webSocketDebuggerUrl": "ws://127.0.0.1:<port>"
}
```

The existing `ws.rs` codec handles only the WebSocket upgrade; an HTTP router for
`/json/version` and `/json/list` must be added before the WebSocket upgrade path.

---

## Entry points

**Existing (real file:line):**

| Symbol | File | Notes |
|---|---|---|
| `DevToolsServer::spawn` | `crates/devtools/src/server.rs:19` | Starts TCP listener on `127.0.0.1:port` |
| `accept_loop` | `crates/devtools/src/server.rs:33` | Per-connection thread spawn |
| `handle_connection` | `crates/devtools/src/server.rs:47` | WebSocket lifecycle loop |
| `cdp::dispatch` | `crates/devtools/src/cdp.rs:18` | Stateless JSON-RPC router |
| `cdp::try_dispatch` | `crates/devtools/src/cdp.rs:25` | Internal dispatch with method routing |
| `BrowserSession` trait | `crates/driver/src/lib.rs:62` | Full session API |
| `InProcessSession` | `crates/driver/src/session.rs:53` | Headless implementation |
| `WinitSession` | `crates/driver/src/winit_session.rs` | Windowed implementation |

**Proposed additions (not yet in code, mark as such):**

| Symbol | Proposed location | Purpose |
|---|---|---|
| `CdpSession` struct | `crates/devtools/src/cdp_session.rs` [proposed] | Holds `Box<dyn BrowserSession>` + event queue |
| `cdp::dispatch_with_session` | `crates/devtools/src/cdp.rs` [proposed] | Stateful dispatch |
| `http_handler` | `crates/devtools/src/server.rs` [proposed] | `/json/version` and `/json/list` HTTP responses |
| `--cdp-port <N>` flag | `crates/shell/src/main.rs` [proposed] | CLI flag to start the CDP server |
| Target map | `crates/devtools/src/target.rs` [proposed] | `HashMap<TargetId, CdpSession>` for multi-tab |

---

## Steps

1. **[Pre-condition]** Confirm graduation trigger per ADR-006. Update
   `docs/decisions/ADR-006-automation-api.md` with a "Graduation" section (date + project name).
   This commit is the go signal.

2. **Add `lumen-driver` dependency to `lumen-devtools`** (`crates/devtools/Cargo.toml`).
   Check for cycles: `lumen-driver` depends on `lumen-core`, `lumen-layout`, `lumen-paint`,
   `lumen-dom`, etc., but not on `lumen-devtools` — cycle-free.

3. **HTTP pre-router in `server.rs`** (`crates/devtools/src/server.rs:47`).
   Before calling `ws::upgrade`, peek at the first line of the TCP stream. If it is
   `GET /json/version` or `GET /json/list`, respond with the JSON payload over HTTP/1.0
   and close the connection. Otherwise fall through to WebSocket upgrade as before.

4. **`CdpSession` struct** (`crates/devtools/src/cdp_session.rs` [proposed]).
   Fields: `session: Box<dyn BrowserSession>`, `event_queue: VecDeque<String>`,
   `target_id: String` (UUID), `target_type: "page"`.
   Methods: `new(session)`, `drain_events() -> Vec<String>`.

5. **`dispatch_with_session`** (`crates/devtools/src/cdp.rs` [proposed]).
   Signature: `pub fn dispatch_with_session(msg: &str, cdp: &mut CdpSession) -> Vec<String>`.
   Returns a vec of JSON strings: first element is the response to the request, subsequent
   elements are unsolicited events queued during the call (e.g. `Page.loadEventFired`,
   `Runtime.consoleAPICalled`). Implement the method table below.

6. **CDP method table** — minimum Puppeteer subset:

   | CDP Method | `BrowserSession` call | Notes |
   |---|---|---|
   | `Browser.getVersion` | — | Already implemented `cdp.rs:50` |
   | `Target.getTargets` | `current_url()` | Return single Target |
   | `Target.createTarget` | construct new session via factory | Returns new `targetId` |
   | `Target.attachToTarget` | — | Return `sessionId` |
   | `Page.enable` | — | ACK; enables load events |
   | `Page.navigate` | `navigate(url)` | Emit `Page.loadEventFired` after |
   | `Page.captureScreenshot` | `screenshot()` | Base64-encode PNG |
   | `Page.getFrameTree` | `current_url()` | Single frame |
   | `Runtime.enable` | — | ACK |
   | `Runtime.evaluate` | `eval(js)` | Map result/exception |
   | `DOM.enable` | — | ACK |
   | `DOM.getDocument` | — | Stub already exists `cdp.rs:60` |
   | `DOM.querySelector` | `query(selector)` | Return first `nodeId` |
   | `DOM.querySelectorAll` | `query(selector)` | Return all `nodeId`s |
   | `DOM.getBoxModel` | `layout_box_by_selector(selector)` | Coordinate map |
   | `Input.dispatchMouseEvent` | `click(Target::Point{x,y})` | type="mousePressed/Released" |
   | `Input.dispatchKeyEvent` | `type_text(target, text)` | type="keyDown/keyUp" |
   | `Network.enable` | — | ACK; enables request/response events |

7. **Event delivery**: extend `handle_connection` (`server.rs:47`) to call
   `drain_events()` after each `dispatch_with_session` call and write each pending
   event as a separate WebSocket text frame.

8. **`--cdp-port <N>` CLI flag** in `crates/shell/src/main.rs` [proposed].
   When present, call `DevToolsServer::spawn_with_factory(port, factory)` after the
   window/session is created. The factory closure captures an `Arc<Mutex<dyn BrowserSession>>`
   or creates a fresh `InProcessSession` per Target depending on the implementation choice.

9. **Clippy + tests** (see "Tests" section).

10. **Update docs**: `CAPABILITIES.md` (add CDP row), `CSS-SPECS.md` N/A, `STATUS-P1.md`
    (move to recent), `subsystems/lumen-devtools.md` (update Done section), regenerate
    `SYMBOLS.md` via `python scripts/gen_symbols.py`.

---

## Tests

**Unit tests in `crates/devtools/src/cdp.rs`** (extend existing `#[cfg(test)]` block,
`cdp.rs:95`):

- `page_navigate_returns_frame_id` — call `dispatch_with_session` with
  `Page.navigate{url:"about:blank"}`, assert response contains `frameId`.
- `runtime_evaluate_returns_value` — `Runtime.evaluate{expression:"1+1"}`, assert
  `result.value == 2`.
- `dom_query_selector_returns_node_id` — load minimal HTML, `DOM.querySelector`, assert
  non-zero `nodeId`.
- `page_capture_screenshot_returns_base64_png` — assert first bytes of decoded base64
  are the PNG magic bytes `\x89PNG`.

**Smoke test against real Puppeteer** (`tests/cdp_puppeteer_smoke/` [proposed]):

Create a Node.js test script (committed as `tests/cdp_puppeteer_smoke/smoke.mjs`) that:
1. Spawns `lumen-shell --cdp-port 9222 --headless` as a child process.
2. Connects with `puppeteer.connect({ browserWSEndpoint: 'ws://127.0.0.1:9222' })`.
3. Opens a new page, navigates to `file:///...samples/page.html`.
4. Calls `page.screenshot()` and asserts the buffer starts with the PNG magic bytes.
5. Calls `page.evaluate(() => document.title)` and asserts a non-empty string.
6. Calls `page.$('body')` and asserts a non-null `ElementHandle`.

The script is not run in CI by default (requires Node.js + puppeteer installed). It is
documented in `tests/cdp_puppeteer_smoke/README.md` as a manual acceptance gate tied to
the graduation trigger.

---

## Definition of done

- [ ] ADR-006 graduation section committed (date + named project).
- [ ] `DevToolsServer::spawn_with_factory` accepts a `BrowserSession` factory.
- [ ] `/json/version` HTTP endpoint returns Puppeteer-compatible JSON.
- [ ] All 18 CDP methods in the table above are implemented (ACK-only where noted).
- [ ] `Page.navigate`, `Page.captureScreenshot`, `Runtime.evaluate`, `DOM.querySelector`
      are wired to live `BrowserSession` calls (not stubs).
- [ ] `Page.loadEventFired` event is emitted after every `Page.navigate` completes.
- [ ] `Runtime.consoleAPICalled` events are emitted for messages in `console_log()`.
- [ ] Unit tests pass: `cargo test -p lumen-devtools`.
- [ ] Clippy clean: `cargo clippy -p lumen-devtools --all-targets -- -D warnings`.
- [ ] Puppeteer smoke test (`smoke.mjs`) passes manually on the developer's machine.
- [ ] `lumen-bench` median not regressed >5% vs baseline when `--cdp-port` is NOT passed
      (ADR-006 §Performance gate, `docs/decisions/ADR-006-automation-api.md:92`).
- [ ] `CAPABILITIES.md` updated with CDP row.
- [ ] `subsystems/lumen-devtools.md` updated.
- [ ] `SYMBOLS.md` regenerated.

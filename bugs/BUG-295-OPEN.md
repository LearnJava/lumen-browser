# BUG-295 — six BiDi commands (`network.setOfflineStatus`/`addIntercept`/`continueRequest`/`failRequest`, `browser.setTimezoneOverride`, `emulation.setUserAgentOverride`) accept and ACK but have zero observable effect on a live window

**Статус:** OPEN (частичная митигация влита — `network.setOfflineStatus` и `emulation.setUserAgentOverride` теперь реально действуют на живое окно; `network.addIntercept`+`continueRequest`/`failRequest` и `browser.setTimezoneOverride` остаются без эффекта, см. «Остаток» ниже)
**Компонент:** bidi-server (`crates/bidi-server/src/protocol.rs`) + `crates/driver` (`BrowserSession`/`LiveWindowSession`/`AutomationCommand`) + shell (`crates/shell/src/main.rs`) + `crates/network` (process-global offline/UA-override) + `crates/js` (`v8_runtime.rs` navigator.userAgent override)
**Найден:** DEVX-6 (`ROADMAP.md`), writing `tests/wpt/verify_devx6_bidi_scenarios.py`
**Частично исправлено:** 2026-08-05 (P3)

## Симптом (как было заведено)

Sending any of the following BiDi commands against a live `--bidi-port` window succeeded (200-style
`result: {}` response, correct error handling for bad params), and the value was genuinely stored in
`BidiState`, but a page in that same live window observed no behavioral change at all:

- `network.setOfflineStatus({"offline": true})` — a subsequent `fetch()` from the page still succeeded
  against a reachable URL; nothing simulated a connection failure. **Fixed.**
- `network.addIntercept({"phases": ["beforeRequestSent"], "urlPatterns": [...]})` followed by
  `network.failRequest`/`network.continueRequest` — no request was ever actually paused waiting for a
  decision. **Still open** — see «Остаток».
- `browser.setTimezoneOverride({"timezoneId": "America/New_York"})` — `Intl`/`Date` on the page still
  reflected the host OS timezone. **Still open** — see «Остаток».
- `emulation.setUserAgentOverride({"userAgent": "..."})` — `navigator.userAgent` evaluated on the page
  still returned Lumen's real UA string. **Fixed** (also now overrides the real HTTP `User-Agent`
  header, which the original repro didn't even check).

## Фикс (влит, 2 из 4)

Both fixed commands follow the same shape — [`BidiState`]'s existing per-connection field feeds into
[`LiveWindowSession`] (`crates/driver/src/live_session.rs`), which round-trips a new
`AutomationCommand` (`crates/driver/src/types.rs`) to the shell's automation-command dispatch loop
(`crates/shell/src/main.rs`), which flips a **process-global** consulted at the actual request/install
chokepoint — mirroring the pre-existing `GLOBAL_ADBLOCK_ENABLED`/`GLOBAL_ADBLOCK_FILTER` pattern in
`crates/network/src/lib.rs` (a process-global was chosen over threading a new parameter through every
`HttpClient`/`V8JsRuntime` construction site — Lumen runs one window/one JS runtime per process, so a
global reads identically to a session-level BiDi override in practice, and a fresh `V8JsRuntime` is
constructed on every navigation anyway, so there's no single long-lived instance to carry a field on):

- **`network.setOfflineStatus`** → `lumen_network::{set_global_offline, is_global_offline}`. Checked in
  `fetch_with_redirect` right after scheme validation, before mixed-content/filter/`RequestStarted` —
  every fetch path (top-level navigation, JS `fetch()`/XHR, subresources, both H1 and H2) already
  funnels through this one function. Returns `Err(Error::Network("net::ERR_INTERNET_DISCONNECTED"))`
  with no `RequestStarted`/`RequestCompleted` emitted, same contract as a blocked (ad-block) request.
  Tests: `global_offline_fails_before_touching_network` (proves via a mock server that zero real TCP
  connections happen while offline, one happens after clearing).
- **`emulation.setUserAgentOverride`** → two independent effects, both gated by the same
  `lumen_network::{set_global_ua_override, global_ua_override}` for the HTTP header, plus
  `lumen_js::v8_runtime::{set_global_user_agent_override, global_user_agent_override}` for the JS side:
  - Real HTTP header: `apply_ua_override`/`apply_ua_override_h2` surgically splice the `User-Agent:`
    value in the already-built H1 header block / H2 header list, leaving every other header
    byte-identical. Tests: `apply_ua_override_*`, `global_ua_override_replaces_header_on_wire_h1`
    (captures the literal bytes a mock server received).
  - `navigator.userAgent`: `V8JsRuntime::install_dom` evals a tiny `navigator.userAgent = "..."` script
    right after `WEB_API_SHIM` (same ordering constraint as the pre-existing deterministic-seed
    script) — before any page `<script>` runs, so even a synchronous top-level read sees the override.
    Since a fresh `V8JsRuntime` is built on every navigation, this only covers the *next* navigation;
    the shell's `AutomationCommand::SetUserAgent` handler additionally re-evals the same snippet on the
    *already-loaded* page via the existing `eval_js_value` round-trip, covering BUG-295's own manual
    repro (set override, then `script.evaluate` with no navigation in between). Clearing the override
    (`""`) is *not* retroactively re-applied to an already-loaded page — only the next navigation
    reverts to the real `WEB_API_SHIM` default; documented as an accepted MVP-scope gap, same spirit as
    `LiveWindowSession`'s other "not yet wired — MVP" comments. Test:
    `user_agent_override_applies_at_install_dom` (in-process `V8JsRuntime` + `install_dom`, no live
    winit window needed — mirrors `runtime_with_dom` used by the crate's other JS tests).
- `emulation_set_ua_override`'s per-context targeting still can't route different UAs to different
  windows — Lumen runs one native window per process (`CLIENT_WINDOW_ID`) — so the live window always
  gets the effective UA of the *first* BiDi context (documented on `emulation_set_ua_override`'s doc
  comment); this was already true of the in-memory bookkeeping before this fix and isn't a regression.

**Verification:** `python tests/wpt/verify_devx6_bidi_scenarios.py` still reports `SKIP(env)` (not
`OK`) for all four live-effect checks in this sandbox — a **pre-existing, documented environment
limitation** unrelated to this fix (the live window's JS runtime/event pump never finishes installing
in this specific sandboxed session; same symptom independently hit trying a bare `--mcp-live-port
about:blank` + `navigate` + `wait document_ready`, which timed out before any BiDi/automation command
specific to this bug was even involved). The mechanism was instead verified with in-process,
deterministic unit tests exercising the exact same code paths (`crates/network`, `crates/js`) a live
window's requests/navigations go through — see the six tests named above (`cargo test -p lumen-network
--lib` / `cargo test -p lumen-js --features v8-backend --lib`, all green, 2124/2486 total). A session
with a working live window should re-run `verify_devx6_bidi_scenarios.py` to confirm the two `SKIP`s
for offline/UA flip to `OK` (or promote them out of the script's `XFAIL(BUG-295)`/`SKIP(env)` framing
now that they're fixed — that script's own comments still describe the pre-fix state).

## Остаток (не устранён этой задачей)

`network.addIntercept`+`continueRequest`/`failRequest` and `browser.setTimezoneOverride` remain exactly
as originally diagnosed — no live-window effect:

- **Intercept + continue/fail**: `network.continueRequest`/`continueResponse`/`continueWithAuth`/
  `failRequest` (`protocol.rs:661-665`) aren't even named handlers — bare ACKs with no lookup against
  `state.intercepts` or any in-flight-request bookkeeping. Closing this needs a genuine
  pause-and-wait-for-decision subsystem: the network layer would have to block a matching request at
  `beforeRequestSent`/`responseStarted` and wait (cross-thread, with a timeout) for a BiDi client to
  send `continueRequest`/`failRequest` by `request` id — a materially different, larger piece of
  engineering than the two process-global toggles above (closer to new feature work than a bug fix).
- **Timezone override**: `browser_set_timezone` still only writes `BidiState::timezone_override`.
  Threading it into the JS engine's `Date`/`Intl` needs either a V8-level ICU timezone override or a
  `Date`/`Intl.DateTimeFormat` JS-shim monkeypatch (the same *shape* as the UA-override script above,
  but `Intl.DateTimeFormat` is a full ECMA-402 object, not a single string property — patching just
  `resolvedOptions().timeZone` without also shifting every `Date` formatting/parsing method that
  implicitly uses the host zone would be a half-correct emulation) — not attempted this session.

Both remain real, filed gaps for a future session; `BidiState::intercept_count()`/`::timezone()`
keep their `#[allow(dead_code)]` (still only test-consumed, no live wiring added for them).

## Repro (still reproduces the two open items)

1. Build `lumen.exe` (`dev-release`), run `python tests/wpt/verify_devx6_bidi_scenarios.py` — protocol
   round-trips all pass; `network.addIntercept+failRequest: live request actually fails` and
   `browser.setTimezoneOverride: live Intl timezone shifts` still report `XFAIL(BUG-295)` (or
   `SKIP(env)` in a sandbox without a working live window — see «Verification» above).
2. Or manually with a working live window: spawn `lumen --bidi-port <port>`, `network.addIntercept` +
   `network.failRequest` for a matching URL, then `fetch()` that URL from the page — it still resolves
   normally instead of being paused/failed.

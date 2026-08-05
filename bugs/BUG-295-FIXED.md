# BUG-295 — six BiDi commands (`network.setOfflineStatus`/`addIntercept`/`continueRequest`/`failRequest`, `browser.setTimezoneOverride`, `emulation.setUserAgentOverride`) accept and ACK but have zero observable effect on a live window

**Статус:** FIXED 2026-08-06 (P3, 4/4) — все шесть команд теперь реально действуют на живое окно. Узкий задокументированный остаток: только фаза `beforeRequestSent` реально приостанавливает запрос; `responseStarted`/`authRequired` (`network.continueResponse`/`continueWithAuth`) остаются bare-ACK — см. «Остаток».
**Компонент:** bidi-server (`crates/bidi-server/src/protocol.rs`) + `crates/driver` (`BrowserSession`/`LiveWindowSession`/`AutomationCommand`) + shell (`crates/shell/src/main.rs`) + `crates/network` (process-global offline/UA-override/intercept registry, `crates/network/src/intercept.rs`) + `crates/js` (`v8_runtime.rs` navigator.userAgent + timezone override, `intl_bindings.rs` shim fallback)
**Найден:** DEVX-6 (`ROADMAP.md`), writing `tests/wpt/verify_devx6_bidi_scenarios.py`
**Исправлено:** 2026-08-05 (P3, 2/4), 2026-08-06 (P3, 3/4 — `browser.setTimezoneOverride`), 2026-08-06 (P3, 4/4 — `network.addIntercept`+`continueRequest`/`failRequest`)

## Симптом (как было заведено)

Sending any of the following BiDi commands against a live `--bidi-port` window succeeded (200-style
`result: {}` response, correct error handling for bad params), and the value was genuinely stored in
`BidiState`, but a page in that same live window observed no behavioral change at all:

- `network.setOfflineStatus({"offline": true})` — a subsequent `fetch()` from the page still succeeded
  against a reachable URL; nothing simulated a connection failure. **Fixed.**
- `network.addIntercept({"phases": ["beforeRequestSent"], "urlPatterns": [...]})` followed by
  `network.failRequest`/`network.continueRequest` — no request was ever actually paused waiting for a
  decision. **Fixed** (2026-08-06) — see «Фикс (`network.addIntercept`, 4/4)» below.
- `browser.setTimezoneOverride({"timezoneId": "America/New_York"})` — `Intl`/`Date` on the page still
  reflected the host OS timezone. **Fixed** (2026-08-06).
- `emulation.setUserAgentOverride({"userAgent": "..."})` — `navigator.userAgent` evaluated on the page
  still returned Lumen's real UA string. **Fixed** (also now overrides the real HTTP `User-Agent`
  header, which the original repro didn't even check).

## Фикс (`network.setOfflineStatus`/`emulation.setUserAgentOverride`/`browser.setTimezoneOverride`, 3 из 4)

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
- **`browser.setTimezoneOverride`** (2026-08-06) → `lumen_js::v8_runtime::{set_global_timezone_override,
  global_timezone_override}` + `timezone_override_script`, wired the same shape as the UA override:
  `browser_set_timezone` (`protocol.rs`) now also calls `LiveWindowSession::set_timezone` when a live
  window is attached, which round-trips a new `AutomationCommand::SetTimezone` to the shell, which sets
  the process-global and — for the *already-loaded* page — re-evals the override script immediately
  (same "next navigation via `install_dom`, current page via an extra eval" split as UA override;
  clearing the override is likewise not retroactively re-applied to an already-loaded page, same
  accepted MVP-scope gap).
  - **Discovered while implementing:** the concern noted in this bug's earlier "Остаток" section — that
    patching only `resolvedOptions().timeZone` would be a "half-correct emulation" needing a full ICU
    timezone database — turned out to not apply. Lumen's V8 build (`v8 = { version = "150.1.0",
    default-features = false }`) ships **native** `Intl` with full ICU tzdata (confirmed empirically:
    `Intl.DateTimeFormat().resolvedOptions().timeZone` returns the real host IANA zone, e.g.
    `"Europe/Moscow"`, not a hand-rolled offset) — `crates/js/src/intl_bindings.rs`'s pure-JS ECMA-402
    shim (which *would* have needed the offset-table treatment) only activates as a fallback when a V8
    build lacks ICU i18n data, and defers to native `Intl` otherwise (its own module doc, and the
    `if (typeof global.Intl !== 'undefined' ...) return;` guard at the top of `INTL_SHIM`). So the fix
    is a JS-level wrap of the *native* `Intl.DateTimeFormat` constructor (`timezone_override_script` in
    `v8_runtime.rs`): when constructed without an explicit `options.timeZone`, it injects the override
    IANA id before delegating to the original constructor — explicit `timeZone` from calling JS always
    wins (spec behaviour), and because it's the real ICU-backed constructor underneath, `Date`
    formatting/parsing that goes through a `DateTimeFormat` instance is genuinely DST-aware correct for
    the overridden zone, not just a label. The shim's own `DateTimeFormat` (`intl_bindings.rs`) was
    *also* updated to read the same global marker (`this._tz`) for the no-ICU fallback case, even though
    that path isn't exercised by this build — defense in depth for the scenario the module doc describes.
  - **Known residual gap:** the override is not threaded into bare `Date` methods
    (`Date.prototype.getTimezoneOffset`/`toString`/etc.) — only `Intl.DateTimeFormat` (and, transitively,
    `Date.prototype.toLocaleString`/`toLocaleDateString`/`toLocaleTimeString` *when running under the
    pure-JS shim*, since those delegate to the shim's own `DateTimeFormat` closure; under native `Intl`
    they still delegate to V8's own unwrapped `Date.prototype.toLocaleString`, which is not wrapped).
    Matches the verification bar this bug was filed against
    (`Intl.DateTimeFormat().resolvedOptions().timeZone`, see `tests/wpt/verify_devx6_bidi_scenarios.py`'s
    `check_timezone_override`); widening to `Date.prototype` methods is a separate follow-up, not
    attempted here.
  - Tests: `timezone_override_applies_at_install_dom`, `timezone_override_is_noop_when_unset`,
    `timezone_override_does_not_win_over_explicit_option` (`crates/js/src/v8_runtime.rs`, in-process
    `V8JsRuntime` + `install_dom`, no live winit window needed — same style as the UA-override tests);
    full `intl_bindings` suite (19 tests) reconfirmed green, unaffected by the shim edit.

**Verification:** `python tests/wpt/verify_devx6_bidi_scenarios.py` still reports `SKIP(env)` (not
`OK`) for all live-effect checks in this sandbox — a **pre-existing, documented environment
limitation** unrelated to this fix (the live window's JS runtime/event pump never finishes installing
in this specific sandboxed session; same symptom independently hit trying a bare `--mcp-live-port
about:blank` + `navigate` + `wait document_ready`, which timed out before any BiDi/automation command
specific to this bug was even involved). The mechanism was instead verified with in-process,
deterministic unit tests exercising the exact same code paths (`crates/network`, `crates/js`) a live
window's requests/navigations go through — see the tests named above (`cargo test -p lumen-network
--lib` / `cargo test -p lumen-js --features v8-backend --lib`, all green, 2489/2489 total). A session
with a working live window should re-run `verify_devx6_bidi_scenarios.py` to confirm the `SKIP`s
for offline/UA/timezone flip to `OK` (or promote them out of the script's `XFAIL(BUG-295)`/`SKIP(env)`
framing now that they're fixed — that script's own comments still describe the pre-fix state).

## Фикс (`network.addIntercept`+`continueRequest`/`failRequest`, 4/4, 2026-08-06)

Unlike the three process-global toggles above, an intercept match doesn't just flip a flag — it must
genuinely pause the calling thread until a BiDi client decides, then unblock it. New module
`crates/network/src/intercept.rs`:

- **Registry**: `GLOBAL_INTERCEPTS` (`RwLock<Vec<GlobalIntercept>>`) mirrors `BidiState::intercepts`
  (same shape: id/phases/url_patterns), synced by `network_add_intercept`/`network_remove_intercept`
  (`protocol.rs`) via two new `LiveWindowSession` methods (`add_intercept`/`remove_intercept`) → two new
  `AutomationCommand` variants → shell calls `lumen_network::{add_global_intercept,
  remove_global_intercept}` — the same round-trip shape as `set_offline`/`set_timezone`.
- **Pause point**: `pause_for_intercept(url, "beforeRequestSent")` is called from `fetch_with_redirect`
  right after the ad-block filter check (before CORS preflight/`RequestStarted`) — the same chokepoint
  every fetch path already funnels through. A URL matching an active rule registers a `PendingIntercept`
  (keyed by a fresh opaque request id) and blocks the calling thread on a `Condvar` for up to
  `INTERCEPT_DECISION_TIMEOUT` (30s — not spec-mandated, an engineering safety net so a client that
  registers an intercept and never resolves it can't hang a fetch forever). Verified safe: `fetch()`'s
  async path (`crates/js/src/dom.rs`) already runs on its own worker thread via
  `_lumen_fetch_async_start`/poll, never the JS engine or UI thread — this pause cannot freeze a page or
  the browser chrome.
- **Resolution**: `network.continueRequest`/`network.failRequest` (previously bare ACKs regardless of
  params) now parse the `request` id and call `LiveWindowSession::resolve_intercepted_request` →
  `AutomationCommand::ResolveIntercept` → `lumen_network::resolve_intercept(id, Continue|Fail)`, which
  sets the pending entry's decision and notifies the condvar. An unknown/already-resolved id still ACKs
  (`Ok(false)`) — matches the bare-ACK tolerance these handlers already had before real bookkeeping
  existed, so a client resolving defensively doesn't get a spurious error.
- **Event delivery** (`network.beforeRequestSent`): this transport has no spontaneous event-push path —
  `transport::handle` is a strictly sequential blocking read → `dispatch` → write loop, one physical
  response per incoming client message (same constraint every other event in this file already lives
  with, e.g. `browsingContext.load` only rides along on the `navigate` call that causes it). So `dispatch`
  now opportunistically drains `LiveWindowSession::poll_intercepted_requests` (→
  `lumen_network::drain_new_intercept_announcements`) at the end of *every* incoming command when the
  connection is subscribed to `network.beforeRequestSent`, and prepends an event frame per newly-paused
  request ahead of that command's own response. A real BiDi client polling at all (which any client
  driving a paused request must do — see `fetch_probe`/`poll_eval_json` in
  `verify_devx6_bidi_scenarios.py`) observes the event within its next round-trip; this is an
  approximation of proactive push, not a rearchitecture of the transport's read loop.
- **Scope**: only the `"beforeRequestSent"` phase is actually paused on. `"responseStarted"`/
  `"authRequired"` rules are accepted and stored (so `network.addIntercept` doesn't reject them) but
  `network.continueResponse`/`network.continueWithAuth` remain bare ACKs — pausing mid-response (holding
  a live socket open while a BiDi client decides) and modeling an auth challenge are both materially
  different pieces of engineering than the request-phase pause implemented here; not attempted in this
  slice.

Tests: `crates/network/src/intercept.rs`'s own suite (non-matching URL/phase is a no-op, `Continue`
unblocks with `Ok`, `Fail` unblocks with `Err`, unknown id resolve returns `false`, `removeIntercept`
stops matching) plus `global_intercept_pauses_real_fetch_until_resolved` in `crates/network/src/lib.rs`
(end-to-end through the real `fetch_with_redirect` chokepoint against a mock HTTP server — proves the
fetch thread genuinely blocks, `!handle.is_finished()`, until resolved). `crates/bidi-server/src/
protocol.rs`: `network_continue_request_acks`/`network_fail_request_acks` (updated to the spec-correct
string `request` id shape — the pre-existing tests used a made-up object shape that only worked because
the old handler ignored `params` entirely), `network_continue_request_missing_request_id_errors`,
`before_request_sent_event_delivered_when_subscribed` (+ not-delivered-without-subscription,
+ not-re-announced), `continue_request_with_live_window_resolves_matching_id` — the last three via a new
`fake_live_session_with_intercept()` test double (same style as the existing `fake_live_session*`
helpers).

## Остаток (не устранён этой задачей)

- `network.continueResponse`/`network.continueWithAuth` and the `"responseStarted"`/`"authRequired"`
  phases remain bare ACKs / stored-but-unactuated — see «Scope» above. `BidiState::intercept_count()`
  keeps its `#[allow(dead_code)]` (still only test-consumed).
- The `INTERCEPT_DECISION_TIMEOUT` (30s) is an engineering choice, not from the BiDi spec — a real client
  is expected to decide promptly; nothing currently makes this configurable per intercept or per session.

## Repro

1. Build `lumen.exe` (`dev-release`), run `python tests/wpt/verify_devx6_bidi_scenarios.py` — protocol
   round-trips all pass; `network.addIntercept+failRequest: live request actually fails` should now
   report `OK` (was `XFAIL(BUG-295)`) with a working live window, or `SKIP(env)` in a sandbox without one
   — see «Verification» above (same pre-existing environment limitation, unrelated to this fix).
2. Or manually with a working live window: spawn `lumen --bidi-port <port>`, `network.addIntercept` +
   `network.failRequest` for a matching URL, then `fetch()` that URL from the page — the fetch now stays
   pending until `failRequest` resolves it (or `INTERCEPT_DECISION_TIMEOUT` elapses), then rejects.

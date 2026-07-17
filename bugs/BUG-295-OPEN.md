# BUG-295 — six BiDi commands (`network.setOfflineStatus`/`addIntercept`/`continueRequest`/`failRequest`, `browser.setTimezoneOverride`, `emulation.setUserAgentOverride`) accept and ACK but have zero observable effect on a live window

**Статус:** OPEN
**Компонент:** bidi-server (`crates/bidi-server/src/protocol.rs`) + shell live-window wiring (`crates/shell/src/main.rs`) — the commands themselves are correct protocol responders; the gap is that `BidiState`'s stored fields are never read by anything that drives the live window/network/JS engine
**Найден:** DEVX-6 (`ROADMAP.md`), writing `tests/wpt/verify_devx6_bidi_scenarios.py`

## Симптом

Sending any of the following BiDi commands against a live `--bidi-port` window succeeds (200-style
`result: {}` response, correct error handling for bad params), and the value is genuinely stored in
`BidiState`, but a page in that same live window observes no behavioral change at all:

- `network.setOfflineStatus({"offline": true})` — a subsequent `fetch()` from the page still succeeds
  against a reachable URL; nothing simulates a connection failure.
- `network.addIntercept({"phases": ["beforeRequestSent"], "urlPatterns": [...]})` followed by
  `network.failRequest`/`network.continueRequest` — no request is ever actually paused waiting for a
  decision; a real in-flight request for a matching URL completes normally regardless of whether
  `failRequest` or `continueRequest` (or neither) is sent for it.
- `browser.setTimezoneOverride({"timezoneId": "America/New_York"})` — `Intl.DateTimeFormat().resolvedOptions().timeZone`
  and `new Date().getTimezoneOffset()` evaluated on the page still reflect the host OS timezone.
- `emulation.setUserAgentOverride({"userAgent": "..."})` — `navigator.userAgent` evaluated on the page
  still returns Lumen's real UA string, session-level or per-context.

## Причина (confirmed by code reading, `crates/bidi-server/src/protocol.rs`)

Each command is dispatched (`dispatch_method`, `protocol.rs:640-673`) to a handler that mutates a
field on `BidiState` and returns a bare `{}` ACK:

- `network_set_offline` (`:1328-1338`) → `state.offline: bool`
- `network_add_intercept`/`network_remove_intercept` (`:1452-1511`) → `state.intercepts: Vec<NetworkIntercept>`
- `network.continueRequest`/`continueResponse`/`continueWithAuth`/`failRequest` (`:661-665`) — not
  even a named handler; the match arms return `DispatchResult::single(make_success(id, empty_obj()))`
  directly, with no lookup against `state.intercepts` or any in-flight-request bookkeeping at all —
  there is no "paused request" state to continue or fail in the first place.
- `browser_set_timezone` (`:1317-1321`) → `state.timezone_override: Option<String>`
- `emulation_set_ua_override` (`:1346-1377`) → `state.session_ua_override` / per-context `ctx.ua_override`

All five stored fields have read accessors (`BidiState::is_offline()`, `::timezone()`,
`::user_agent_for()`, `protocol.rs:300-343`) explicitly marked `#[allow(dead_code)]` with a
"Shell layer … not wired yet" comment — confirming this is a known, intentional gap, not a
regression. Contrast with `browsingContext.navigate`/`script.evaluate`/`captureScreenshot`/pointer
`input.performActions`, which do reach `state.live: Option<LiveWindowSession>` and drive the real
shell window. None of the six commands above ever touch `state.live`.

Also documented in `docs/automation.md`'s WebDriver BiDi section ("Also implemented and **unused by
any tooling**: …") — this bug formalizes that known gap with a repro and file:line trail so it can be
picked up as real feature work.

## Repro

1. Build `lumen.exe` (`dev-release`), run `python tests/wpt/verify_devx6_bidi_scenarios.py` — the
   protocol round-trip assertions pass (correct ACK shape, correct error handling), but the four
   behavioral/live-effect assertions are reported as `XFAIL(BUG-295)`, not `OK`.
2. Or manually: spawn `lumen --bidi-port <port>`, send `emulation.setUserAgentOverride`
   `{"userAgent": "TestUA/1.0"}` at the session level, then `script.evaluate`
   `"navigator.userAgent"` on the default context — result is the real UA, not `"TestUA/1.0"`.

## Что нужно для закрытия

Wire each of `BidiState::offline`/`intercepts`/`timezone_override`/`{session,ctx}_ua_override` into
the actual live-window pipeline: offline → make the shell's `HttpClient` (or the network service) fail
requests when `state.offline`; intercepts → real beforeRequestSent/response pause-and-wait-for-decision
bookkeeping wired to the network layer, with `continueRequest`/`failRequest`/`continueResponse` acting
on the specific paused request by `request` id; timezone override → thread through to the JS engine's
`Date`/`Intl` implementation (likely a per-runtime override rather than reading the OS timezone);
UA override → thread into the `navigator.userAgent` JS shim value and the real HTTP `User-Agent`
request header, both session- and per-context-scoped. This is engine/shell-level feature work (not a
Python-tooling task) — likely P1 scope given it touches `crates/network`, `crates/js`, and
`crates/shell` rather than `lumen-bidi-server` alone.

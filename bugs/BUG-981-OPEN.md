# BUG-981: BiDi read/dispatch loop never services WebSocket ping while a
command is running — a slow command can get the client's own 20s ping
liveness timeout to kill the whole session

**Статус:** OPEN
**Дата:** 2026-09-04
**Компонент:** bidi-server (`crates/bidi-server/src/transport.rs::handle`,
line 32-60 read/dispatch loop; ping/pong auto-answer lives in
`crates/devtools/src/ws.rs::read_text_frame`, line 106-124)
**Найден:** P2, WPT-RUN-6 срез 61, живой пробой (реальный `wptrunner`+
`wptserve`, `run_report.py`)

## Механизм

`transport::handle`'s loop is: `read_text_frame` (blocks until a frame
arrives) → `dispatch(&msg, &mut state)` (blocks until the command
finishes — for `browsingContext.navigate` with `wait: "complete"`, until
the whole page, including any synchronous JS it runs, is done) → write the
response → loop back to `read_text_frame`. Ping→Pong is answered *only*
inside `read_text_frame` (`ws.rs:116-118`, opcode `0x9` → immediate
`0xA` reply) — while `dispatch()` is running, the thread is not calling
`read_text_frame` at all, so a Ping frame the client sends during that
window sits unread in the OS socket buffer and gets no Pong until
`dispatch()` returns, however long that takes.

`wptrunner`'s own BiDi client (`tools/webdriver/webdriver/bidi/
transport.py:47`) opens the connection via `websockets.connect(url,
max_size=…)` with no `ping_interval`/`ping_timeout` override — confirmed
in the project's own venv (`websockets==16.1`): library defaults
`ping_interval=20`, `ping_timeout=20`. So the client sends a Ping every 20s
of connection idleness and gives up (raises `ConnectionClosed`, surfaced by
`executorlumen.py` as `"unknown error (WebSocket connection closed)"`) if
20 more seconds pass with no Pong. Any single `dispatch()` call — a
`navigate` whose page runs the `XMLHttpRequest.send()`-is-fully-synchronous
defect (BUG-980) for ~20s, or any other long blocking work — can straddle
that window and get the *client* to tear down the connection, even though
the browser process itself is alive and working the whole time.

## Замер

Three `run_report.py --binary target/dev-release/lumen --root
content-security-policy/frame-ancestors --recursive` invocations, same
binary, same two test files (`report-blocked-frame.sub.html` and
`report-only-frame.sub.html` — both run the identical 20-second-blocking
`checkReport.sub.js` XHR idiom, see BUG-980):

1. **Full 34-file directory run** (`.tmp/s61-frame-ancestors.raw.jsonl`):
   test #33 `report-blocked-frame.sub.html` completes `FAIL` (own
   assertion, unrelated to this bug) after 20.32s — connection still young,
   survives. Test #34 `report-only-frame.sub.html` starts **0ms** after
   (`test_end`/`test_start` share the same millisecond timestamp), and
   fails at +20.27s with `ExecutorException: ('ERROR',
   'browsingContext.navigate(…report-only-frame.sub.html) failed: unknown
   error (WebSocket connection closed)')`. The browser's own
   `process_output` log shows it kept working normally — fetched
   `support/content-security-policy-report-only.sub.html`, parsed 20
   DOM nodes — at timestamps up to and past the instant the client already
   reported the connection dead, so this is a client-perceived closure,
   not an engine crash (same `browser_pid` throughout, no restart, `Runner
   process exited with code 0` at suite end).
2. **Isolated solo run** (`--offset 33 --limit 1`, fresh session,
   `.tmp/s61-report-only-solo.raw.jsonl`): `report-only-frame.sub.html`
   alone completes fine — `FAIL` (not `ERROR`) after 20.39s, `tests: 1/1
   harness OK`.
3. **Isolated pair run** (`--offset 32 --limit 2`, fresh session,
   `.tmp/s61-pair-run1.raw.jsonl`): both files back-to-back, no prior
   tests in the session — completes fine, `tests: 2/2 harness OK`, no
   `ERROR`.

Timing-dependent, as expected from a ping-phase race: whether the client's
periodic Ping lands inside a `dispatch()` window that outlasts its 20s
`ping_timeout` depends on how old the connection already was when the slow
command started, not on any fixed pair of tests. Root cause is pinned by
source reading (`transport.rs`/`ws.rs`/the venv's confirmed `websockets`
defaults), not inferred from the timing coincidence alone.

## Масштаб

Any BiDi automation session — WPT via `wptrunner`, or any other BiDi
client using ordinary WebSocket ping/pong liveness — running a page that
blocks the main thread for ~20s or more (BUG-980's fully-synchronous
`XMLHttpRequest.send()` is the concrete WPT-corpus source measured here,
but any long synchronous engine-side operation reached through `dispatch()`
would have the same effect) risks the *client* killing the connection
mid-command. Per `docs/probe-method.md` §4 ("стена TIMEOUT — обычно ОДНА
зависшая страница"), if `wptrunner` does not restart the browser process
after a client-perceived close (it doesn't here — same `browser_pid`,
no relaunch observed), every subsequent test that reuses the dead
connection in the same shard is collateral damage, not an independent
finding. Classifies the two `content-security-policy/frame-ancestors/
report-*.sub.html` ids left in the WPT-RUN-6 residual list.

## Что нужно

Decouple WebSocket control-frame servicing from command dispatch: e.g. run
`dispatch()` on its own thread while a dedicated reader keeps draining the
socket (answering Ping/Close) concurrently, or make `dispatch()` itself
yield periodically so the read loop gets a turn. The existing 60s
`read_timeout` (`transport.rs:24`) does not help — it guards against a
silent *client*, not against the *server* going quiet while it is busy.
Fixing BUG-980 (making `XMLHttpRequest.send()` actually async) removes the
one concrete WPT-corpus trigger measured here, but does not fix this bug —
any other long-blocking `dispatch()` call would still starve the ping
loop.

## Классификация WPT-RUN-6

Attributed via `_exact_id_marker` in `tests/wpt/timeout_audit.py` (marker
`bidi-ping-starved-by-blocking-dispatch`) for
`/content-security-policy/frame-ancestors/report-blocked-frame.sub.html`
and `/content-security-policy/frame-ancestors/report-only-frame.sub.html`.
Per `docs/probe-method.md` §9, this is a timing-dependent race, not a
deterministic hang on either specific id — a solo or freshly-sessioned
rerun of either file can complete without hitting it (measured above); the
code-level cause and the corpus-visible failure are both real and
reproduced, but which id absorbs it is a matter of shard-order luck, not a
property of the file itself.

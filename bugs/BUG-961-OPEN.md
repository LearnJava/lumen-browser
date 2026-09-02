# BUG-961: `browsingContext.navigate` takes 30-40s and then TIMEOUTs under
`wptrunner`'s own orchestration, for a page that loads and completes in
under 1s when driven directly

**Статус:** OPEN
**Дата:** 2026-09-02
**Компонент:** `tests/wpt/` Python tooling (`tools/wptrunner/wptrunner/executors/executorlumen.py`,
`run_smoke.py`/`TestRunnerManager`) — **not yet isolated to a specific
function**, see "Что нужно" below. Ruled out: `lumen-bidi-server`,
`lumen-driver`, `mozprocess.ProcessReader`'s Python-side line handling
(срез 40). **Not ruled out as of срез 40:** the engine itself, specifically
contention between `executorlumen.py`'s polling/evaluate calls and the V8
main thread while it formats the large array for `console.log` — срез 39's
"ruled out: engine" conclusion rested on comparing against a raw
`--dump-layout` run (no testharness.js, no polling) and a bare-BiDi-client
run that measured `navigate` completion, not console.log-arrival; neither
is a clean control for "engine busy handling wptrunner's own harness
traffic concurrently". See срез 40's note on item 1 below.
**Найден:** P2, WPT-RUN-6 срез 39, 2026-09-02, investigating why
`/console/console-log-large-array.any.html` (and its `.any.worker.html`
twin) sit in `timeout_audit.py`'s `unclassified` bucket. Continued срез 40,
2026-09-02.

## Симптом

`/console/console-log-large-array.any.js`:

```js
test(() => {
    console.log(new Array(10000000).fill("x"));
    console.log(new Uint8Array(10000000));
}, "Logging large arrays works");
```

Both the `.any.html` (window) and `.any.worker.html` variants TIMEOUT at
`test_timeout: 10` in the corpus run's raw log
(`.tmp/wpt-corpus/console.raw.jsonl`). Re-run in isolation via
`tests/wpt/run_report.py --root console --all --recursive --offset 4 --limit 1
--timeout-multiplier 6 --binary target/dev-release/lumen` (60s effective test
budget) still fails — not with wptrunner's own `TIMEOUT` this time, but with:

```
wptrunner.executors.base.ExecutorException: ('ERROR',
'browsingContext.navigate(http://127.0.0.1:18300/console/console-log-large-array.any.html)
failed: unknown error (navigate: automation command timed out)')
```

i.e. even a 6× multiplier isn't enough — `browsingContext.navigate` itself
never returns.

## Что доказано (прямым замером, не догадкой)

**1. It is not the array/console.log computation.** A raw script
(`console.log(new Array(10000000).fill("x")); console.log(new
Uint8Array(10000000));`, no testharness.js) run via `--dump-layout` with
stdout redirected to a file completes in **0.79s total**, producing the full
40 MB of expected output. Piping that same run's stderr through a Python
`readline()` loop (mimicking `mozprocess.ProcessReader`) or through `cat`
made no measurable difference (~0.75-0.79s in all three cases) — ruling out
"slow external reader backpressures a huge single-line write" as well.

**2. It is not `lumen-bidi-server`'s `browsingContext.navigate` or
`NAVIGATE_LOAD_TIMEOUT_MS`/`DEFAULT_TIMEOUT` (both 30s,
`crates/bidi-server/src/protocol.rs:62` / `crates/driver/src/live_session.rs:39`).**
A bare Python script (no wptrunner, just `webdriver.bidi.client.BidiSession`
directly against the same `target/dev-release/lumen --bidi-port N` binary)
navigating to the byte-identical page **completes end to end in under 1
second**, `navigate returned OK`, three times over with increasingly faithful
reproductions of the real page:
  - a hand-written 3-`<script src>` wrapper (no `self.GLOBAL`, no `<div
    id=log>`) — 0.6s from "navigating" to "PROBE harness-complete status=0";
  - the **exact bytes wptserve actually serves** for this test id (fetched
    live via `curl` mid-run: includes the `self.GLOBAL={isWindow:...}`
    block and `<div id=log>` that my first wrapper omitted) — same result,
    under 1s;
  - the exact bytes AND Lumen's own product-specific
    `tests/wpt/resources/testharnessreport.js` (NOT the generic
    `wptrunner/testharnessreport.js` + `message-queue.js` pair
    `serve_wpt_like.py` currently serves — see "Побочно" below) — same
    result, under 1s, `navigate returned OK` at t=3.00s.

**3. It is not `wptserve` itself being slow to answer this request.** `curl`
against the real, currently-running wptrunner corpus server
(`http://127.0.0.1:18300/console/console-log-large-array.any.html`), issued
*while the browser was already stuck inside the failing `navigate` call*,
returned in **8ms**.

**4. The delay is real CPU/engine time, not a hung process** — the raw log
from the focused `run_report.py` re-run
(`.tmp` not committed; see reproduction below for the exact command) shows
the browser process printing nothing between "Загружен скрипт:
.../testharness.js" (t=459388ms) and the first `[JS] x,x,x,...` line
(t=490006ms) — a **30618ms gap**, ending right around when
`NAVIGATE_LOAD_TIMEOUT_MS`'s 30s budget would expire. The array's
`console.log` output does eventually print (the page is not actually
wedged), it just takes ~30s longer than the identical computation takes when
driven directly (point 1/2 above).

## Вывод

Something specific to running this test **under `wptrunner`'s own process
orchestration** (`run_smoke.py`/`TestRunnerManager`, or something
`executorlumen.py` does that a bare `browsing_context.navigate(...,
wait="complete")` + `session.script.evaluate` call does not — e.g.
`_reset_and_mark`'s preceding poll, real `capabilities` dict contents,
wptrunner's multiprocessing browser-lifecycle machinery) makes this specific
page's script execution (or `document.readyState` reaching `"complete"`)
take ~30-40× longer than it does standalone. Every content-level and
protocol-level hypothesis tested above was refuted by direct, reproducible
measurement — not worth another WPT-RUN-6 slice re-deriving points 1-3 from
scratch. (Срез 40 update: whether the *mechanism* behind this is wptrunner
orchestration alone or genuine V8-thread contention it triggers is still
open — see the срез 40 section and header below; "not an engine defect" was
premature.)

## Побочно найдено: `tests/wpt/serve_wpt_like.py` serves the wrong
`testharnessreport.js`

`serve_wpt_like.py::_build_testharnessreport()` hard-codes the **generic**
`tools/wptrunner/wptrunner/executors/message-queue.js` +
`tools/wptrunner/wptrunner/testharnessreport.js` pairing — this is
wptrunner's *default*, but `browsers/lumen.py::env_options()` overrides it
per-product to `tests/wpt/resources/testharnessreport.js` **alone** (no
`message-queue.js` prefix; this is the BUG-301 fix, see that file's own
comment). A probe run through `serve_wpt_like.py` therefore exercises a
different reporting contract than a real corpus run — harmless for this
investigation (both pairings completed fast against a bare client, point 2
above), but a future probe relying on `window.__lumen_wpt_results` /
`RESET_EXPRESSION`/`POLL_EXPRESSION` semantics specifically (not just a
`console.log` marker) would get misleading results from this script as-is.
Not fixed in slice 39 — worth a small follow-up patching
`_build_testharnessreport()` to read `tests/wpt/resources/testharnessreport.js`
directly, matching `env_options()`. **Fixed in WPT-RUN-6 срез 40** (this is
tooling in P2's own domain — `tests/wpt/` Python scripts — not an engine
bug, so the "P1/P2/P4 don't fix bugs" rule doesn't apply to it): `serve_wpt_like.py`
now reads `tests/wpt/resources/testharnessreport.js` directly instead of the
generic `message-queue.js` + `wptrunner/testharnessreport.js` pairing.

## Масштаб находки

Unknown how many of `timeout_audit.py`'s other 39 `unclassified` ids (or ids
currently classified under some other mechanism that's actually this same
orchestration stall) share this exact root cause — not checked, out of
scope for one slice. `console-log-large-array.any.js`'s worker variant
(`.any.worker.html`) almost certainly shares it (same corpus-run TIMEOUT
shape, same test body) but was not independently re-verified standalone.

## WPT-RUN-6 срез 40: `mozprocess.ProcessReader`'s real callback ruled out

Slice 39 point 1 had already ruled out "slow external reader backpressures a
huge single-line write" using a **mimicked** `readline()` loop standing in
for `mozprocess.ProcessReader`. This slice instruments the **real** callback
in the actual failing run instead of a stand-in, closing "Что нужно" item 2
below.

Added temporary timing (`time.time()` around `line.decode()` and
`self.logger.process_output(...)`) inside
`tools/wptrunner/wptrunner/browsers/base.py::OutputHandler.__call__` (not
committed — reverted after the measurement, per
`docs/probe-method.md`'s "your own server, not the page" evidence rule: a
throwaway instrumentation diff is not a fix and does not belong in the
tree) and re-ran the slice-39 repro command
(`tests/wpt/run_report.py --root console --all --recursive --offset 4
--limit 1 --binary target/dev-release/lumen`):

```
0:09.46 pid:22174 Загружен скрипт: http://…/console-log-large-array.any.js
0:34.44 TEST_END: TIMEOUT, expected OK - TestRunner hit external timeout
0:40.55 INFO STDERR: [BUG961-PROBE] line_len=20000004 decode=0.019s process_output=0.099s
```

The 20 000 004-byte line (the array's `console.log` output) took **0.019s**
to decode and **0.099s** to hand to the logger once `mozprocess`'s reader
thread actually had it in hand — and that line only *arrives* at t=40.55s,
six seconds **after** wptrunner had already declared TIMEOUT at t=34.44s.
The gap is entirely upstream of this callback: either inside
`ProcessReader._read_stream`'s `stream.readline()` call (blocked waiting for
the pipe to deliver the line) or, more likely given slice 39 point 4's
matching ~30s figure, in the browser process itself taking ~30s of real
CPU/engine time to produce that line **only when driven through
`TestRunnerManager`/`executorlumen.py`'s orchestration** — the same
computation takes under 1s standalone (slice 39 point 1) and under 1s via a
bare BiDi client through `navigate` completion (slice 39 point 2, though
that measured navigate-completion, not console.log-arrival — a gap worth
closing explicitly in the next slice, see item 1 below).

This rules out `OutputHandler`'s line-processing as the bottleneck with a
real measurement, not a mimic — "Что нужно" item 2 is answered: it is not a
health-check/`is_alive()` poll contending inside `TestRunnerManager`'s own
Python-side output handling. The remaining candidate is genuine engine-side
contention specific to `executorlumen.py`'s orchestration (item 1 below is
now the only open path).

## Что нужно

1. Reproduce with `LumenTestharnessExecutor`/`LumenBidiProtocol`'s own
   classes directly (bypass `TestRunnerManager` but keep `executorlumen.py`'s
   exact code, including `_reset_and_mark`'s preceding poll and the real
   `capabilities` dict) — narrows whether the stall is in `executorlumen.py`
   itself (e.g. a poll/evaluate call contending with the page's own V8
   thread while it formats the giant array) or in wptrunner's surrounding
   multiprocess scheduling. Make sure this reproduction measures
   console.log-arrival time, not just `navigate` completion — slice 39's
   bare-BiDi-client comparison measured the latter, which does not
   necessarily prove the async array-logging had already finished.

## Как проверить фикс

Once the mechanism is found: `tests/wpt/run_report.py --root console --all
--recursive --offset 4 --limit 1 --binary target/dev-release/lumen` (no
`--timeout-multiplier` override — default 1×, 10s budget) should PASS, since
the underlying computation genuinely only needs well under 1s.

# BUG-961: `browsingContext.navigate` takes 30-40s and then TIMEOUTs under
`wptrunner`'s own orchestration, for a page that loads and completes in
under 1s when driven directly

**Статус:** OPEN
**Дата:** 2026-09-02
**Компонент:** `tests/wpt/` Python tooling (`tools/wptrunner/wptrunner/executors/executorlumen.py`,
`run_smoke.py`/`TestRunnerManager`) — **not yet isolated to a specific
function**, see "Что нужно" below. Ruled out: engine, `lumen-bidi-server`,
`lumen-driver`.
**Найден:** P2, WPT-RUN-6 срез 39, 2026-09-02, investigating why
`/console/console-log-large-array.any.html` (and its `.any.worker.html`
twin) sit in `timeout_audit.py`'s `unclassified` bucket.

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
measurement — this is **not an engine defect** and not worth another
WPT-RUN-6 slice re-deriving points 1-3 from scratch.

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
Not fixed in this slice (P2 does not fix bugs while doing WPT-RUN-6
triage work, `docs/dev-roles.md` §"P1, P2, P4 do not fix bugs") — worth a
small follow-up patching `_build_testharnessreport()` to read
`tests/wpt/resources/testharnessreport.js` directly, matching
`env_options()`.

## Масштаб находки

Unknown how many of `timeout_audit.py`'s other 39 `unclassified` ids (or ids
currently classified under some other mechanism that's actually this same
orchestration stall) share this exact root cause — not checked, out of
scope for one slice. `console-log-large-array.any.js`'s worker variant
(`.any.worker.html`) almost certainly shares it (same corpus-run TIMEOUT
shape, same test body) but was not independently re-verified standalone.

## Что нужно

1. Reproduce with `LumenTestharnessExecutor`/`LumenBidiProtocol`'s own
   classes directly (bypass `TestRunnerManager` but keep `executorlumen.py`'s
   exact code, including `_reset_and_mark`'s preceding poll and the real
   `capabilities` dict) — narrows whether the stall is in `executorlumen.py`
   itself or in wptrunner's surrounding multiprocess scheduling.
2. If step 1 doesn't reproduce it either, instrument
   `TestRunnerManager`/`run_smoke.py`'s browser-process management (does it
   run a periodic health check or `is_alive()` poll on a separate thread that
   could contend with the automation channel?).

## Как проверить фикс

Once the mechanism is found: `tests/wpt/run_report.py --root console --all
--recursive --offset 4 --limit 1 --binary target/dev-release/lumen` (no
`--timeout-multiplier` override — default 1×, 10s budget) should PASS, since
the underlying computation genuinely only needs well under 1s.

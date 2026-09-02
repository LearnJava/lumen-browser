# BUG-961: `browsingContext.navigate` takes 30-40s and then TIMEOUTs — a
genuine ~32s engine/BiDi-path stall, reproducible with a bare `BidiSession`
and a plain static file server, no wptrunner/wptserve involved

**Статус:** OPEN
**Дата:** 2026-09-02
**Компонент:** not yet isolated — **срез 41 refuted both prior candidates**
(`executorlumen.py`/`TestRunnerManager` orchestration, and engine
contention specific to that orchestration's polling), and **срез 42
refuted content-serving** (live `AnyHtmlHandler` route vs. a static copy —
both stall identically) as well as the standing assumption that this page
"completes in under 1s when driven directly": a byte-identical static copy,
served by a plain `http.server` with zero relation to wptserve or
wptrunner, reproduces the same ~32s stall on current `main`. Ruled out:
`lumen-bidi-server`, `lumen-driver` (both just surface a 30s
`RecvTimeoutError`, not the source), `mozprocess.ProcessReader`'s
Python-side line handling (срез 40), `TestRunnerManager`/`executorlumen.py`
orchestration (срез 41), wptserve's live route vs. a static copy (срез 42).
**Open as of срез 42:** what the `lumen` process is actually blocked on
during the 31s silent window — needs process-level profiling
(`/proc/<pid>/wchan` sampling, no `perf` on this box) or a `git`-bisect
between срез 39's date and now for the regression window — see срез 42's
"Что нужно" item 2.
**Найден:** P2, WPT-RUN-6 срез 39, 2026-09-02, investigating why
`/console/console-log-large-array.any.html` (and its `.any.worker.html`
twin) sit in `timeout_audit.py`'s `unclassified` bucket. Continued срезы 40,
41, 2026-09-02.

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

## WPT-RUN-6 срез 41: reproduces WITHOUT `executorlumen.py`/`TestRunnerManager`
— и WITHOUT any wptrunner code at all

Item 1 above asked for two things: (a) reproduce through
`LumenTestharnessExecutor`/`LumenBidiProtocol` directly, bypassing
`TestRunnerManager`'s multiprocess scheduling but keeping
`executorlumen.py`'s own code (`_reset_and_mark`, `POLL_EXPRESSION`); (b)
measure console.log-**arrival**, not `navigate`-completion. Script:
`tests/wpt/verify_bug961_orchestration.py` (committed this slice). It builds
the same real `wptrunner.environment.TestEnvironment`/`wptserve` instance
`run_report.py` uses, then runs two variants against it:

- **Variant A** — `LumenBrowser` + `LumenTestharnessExecutor`/
  `LumenBidiProtocol`, imported unmodified from `executorlumen.py`/
  `browsers/lumen.py`, calling `executor._run_testharness(url, timeout)`
  directly. No `TestRunnerManager`, no multiprocessing, no health-check
  polling loop around it.
- **Variant B** — a **bare** `webdriver.bidi.client.BidiSession` (zero
  `executorlumen.py` code — no `_reset_and_mark`, no `POLL_EXPRESSION`,
  just `session.start()` + `browsing_context.navigate(wait="complete")`),
  against the **same live `wptserve` instance**, i.e. the real
  `AnyHtmlHandler`-generated `.any.html` response, not a hand-copied file.

Result — **both reproduce the ~30-32s stall**:

```
[probe] variant A RESULT: ExecutorException after 32.0s: ('ERROR',
'browsingContext.navigate(...) failed: unknown error (navigate: automation
command timed out)')
[probe] variant B RESULT: exception after 32.0s: UnknownErrorException(unknown
error, navigate: automation command timed out, )
```

Variant A's raw browser log (item (b) above — arrival, not just
`navigate`-completion) shows all three scripts loaded by t=2.26s
(`Загружен скрипт: .../console-log-large-array.any.js`) and then **31
seconds of complete silence** before the array's `console.log` output
appears at t=33.13s — i.e. the console.log genuinely does eventually print
(the page is not wedged), confirming item (b): the stall is real
script-execution time between "scripts loaded" and "array logged", not an
artifact of `navigate` returning early while logging was still pending.
Variant B was torn down by the probe's `finally: browser.stop()` right after
the automation-level timeout fired, so it has no console.log-arrival
timestamp of its own, but the same 32.0s `navigate: automation command timed
out` shape (`crates/driver/src/automation.rs:85`'s `RecvTimeoutError::Timeout`,
surfaced through `crates/driver/src/live_session.rs:39`'s 30s
`DEFAULT_TIMEOUT` and `crates/bidi-server/src/protocol.rs:962`'s
`NAVIGATE_LOAD_TIMEOUT_MS`) is the same failure both variants hit.

**This answers item 1, but not the way срез 40 expected.** Variant B has
*zero* `executorlumen.py`/wptrunner code in the loop — it is exactly the
same shape as срез 39 point 2's "bare BiDi client" control, which measured
under 1 second. The one thing that differs between that control and this
slice's Variant B is **what serves the page**: срез 39 point 2 served a
hand-copied/curl-fetched byte-identical file through a separate static
server (`serve_wpt_like.py` cannot itself serve `.any.html` — there is no
such file on disk, only `console-log-large-array.any.js`; wptserve's
`AnyHtmlHandler` route builds the wrapper HTML dynamically), while this
slice's Variant B hits wptserve's **own live dynamic route** for the same
test id. So the срез 39/40 "orchestration" and "engine contention with
executorlumen.py's polling" hypotheses are both refuted by this slice —
neither is present in Variant B and it still stalls — but the byte-identical
content claim in срез 39 point 2 was never actually validated against the
live route itself, only against a copy served a different way. Which of
those two candidates (real vs. copied content/response-shape) is the actual
variable is still open — see item 1 below, now re-scoped.

## WPT-RUN-6 срез 42: content-serving hypothesis refuted — a byte-identical
static copy, served by a server with zero relation to `wptserve`, stalls
the same ~32s on current `main`

Item 1 above asked to control срез 39 point 2's "<1s" result against
*today's* `main` by re-running the bare-`BidiSession` shape against a
static-served copy of the exact live-route bytes, not wptserve's own
dynamic route. Script: `tests/wpt/verify_bug961_slice42_static.py`
(committed this slice). It fetches the live `AnyHtmlHandler`-generated
response for `/console/console-log-large-array.any.html` via `urllib`
(396 bytes, byte-for-byte what a real corpus run receives — no hand-copying
or curl transcription that could silently diverge), serves those exact
bytes from a **separate, independent `http.server` instance** (own port,
own process thread, `/resources/testharnessreport.js` substituted the same
way `serve_wpt_like.py` does so the script doesn't hit an unsubstituted
`%(...)s` syntax error — CLAUDE.md's WPT-harness gotcha), and then runs two
bare-`BidiSession` variants back to back: **Variant C** against the static
copy, **Variant D** as a same-process control replay of срез 41's Variant B
(wptserve's own live route).

Result — **both stall ~32s, no difference**:

```
[probe] variant C (static copy) RESULT: exception after 32.0s: UnknownErrorException(...)
[probe] variant D (live route) RESULT: exception after 32.0s: UnknownErrorException(...)
```

Variant C's raw browser log shows the identical shape срез 41 measured on
Variant A/B: all three scripts (`testharness.js`, the `.any.js` file,
`testharnessreport.js`) loaded by t=2.31s, then **30.7s of complete
silence**, then the array's `console.log` output appears at t=33.05s — on a
server that has never talked to wptserve and shares nothing with it but the
byte content. This rules out the live `AnyHtmlHandler` route vs. a static
copy as the variable (item 1's first outcome) — **срез 39 point 2's
original "<1s" bare-client measurement does not hold up against current
`main`**, confirming item 1's second outcome instead. Whatever content-shape
difference срез 39 measured, it is not what is stalling this test today;
the stall is reproducible with nothing more than
a lumen binary, a static file server, and a `navigate(wait="complete")`
call — no wptrunner, no wptserve, no `executorlumen.py`, no orchestration
of any kind. The remaining open question is a genuine regression
window between срез 39 (2026-09-02, same calendar day, several merges
apart — SPLIT-DL19, SPLIT-LB0, BUG-481 named as candidates in срез 41)
and now, or срез 39 point 2's original measurement itself being wrong
(e.g. testing a different/cached response, or `wait="complete"` behaving
differently against a hand-copied file vs. the two servers used here).

## Что нужно

1. **(closed by срез 42 — content-serving is not the variable)**
2. Profile the `lumen` process itself during the 31s silent window to find
   what it is actually blocked on — `perf`/`gdb` are not viable here (no
   `perf` binary on this box; `gdb` only attaches to its own child per
   `ptrace_scope`, not an already-running process spawned by wptrunner's
   browser-lifecycle code). The documented fallback for this box
   (`docs/probe-method.md` / this repo's own convention) is
   `/proc/<pid>/wchan` and `/proc/<pid>/stack` polled at short intervals
   during the stall — get the `lumen` pid either by spawning it directly
   with `subprocess.Popen` (bypassing `LumenBrowser`'s process management)
   or by reading it off the `pid:NNNNN` prefix wptrunner's own logger emits
   per line, then sample `/proc/<pid>/task/*/wchan` for every thread every
   ~200ms through the 31s window to see which thread is parked and on what
   (a futex/condvar wait names the specific lock; a `poll`/`epoll_wait`
   parked on a socket names a different, network-side culprit). Bisecting
   `git log` between срез 39's date and now for the three named candidate
   merges (SPLIT-DL19, SPLIT-LB0, BUG-481) is the cheaper first move if the
   `/proc` sampling is inconclusive — a bisect needs no new tooling, just
   rebuilds at each candidate commit and a re-run of this slice's probe.

## Как проверить фикс

Once the mechanism is found: `tests/wpt/run_report.py --root console --all
--recursive --offset 4 --limit 1 --binary target/dev-release/lumen` (no
`--timeout-multiplier` override — default 1×, 10s budget) should PASS, since
the underlying computation genuinely only needs well under 1s.

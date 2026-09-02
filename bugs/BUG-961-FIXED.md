# BUG-961: `browsingContext.navigate` takes 30-40s and then TIMEOUTs — a
~32s stall reproducible on almost every run through the real wptrunner
`LumenBrowser`/`mozprocess` launch path, but almost never through a bare
`subprocess.Popen` replaying the identical BiDi traffic — root cause:
`mozprocess`'s unbuffered stdout pipe (`bufsize=0`) degrades `readline()` to
one syscall per byte on a long no-newline line

**Статус:** FIXED 2026-09-02 (срез 47)
**Дата:** 2026-09-02
**Компонент:** `tools/wptrunner/wptrunner/browsers/lumen.py` (WPT test
tooling, P2's own domain — not an engine bug) — **срез 41 refuted both prior
candidates**
(`executorlumen.py`/`TestRunnerManager` orchestration, and engine
contention specific to that orchestration's polling), **срез 42 refuted
content-serving** (live `AnyHtmlHandler` route vs. a static copy — both
stall identically) **but srez 42's own two variants went through
wptrunner's real `product.get_browser_cls("lumen")`/`browser.start()`
launch (`mozprocess`-based), not a bare `Popen`** — a distinction срез 46
(below) shows matters. Срез 43 built a *minimal* repro instead (plain
`subprocess.Popen`, no `LumenBrowser`, no `mozprocess`) that completed in
~1.1-1.2s six times in a row, then stalled ~32s once under a controlled A/B
re-run (срез 44), which срез 44 read as refuting the launch mechanism
(`mozprocess.ProcessHandler`'s env merging or `preexec_fn`/process-group)
since the minimal shape reproduced the stall too — but that read one
sample per condition. **Срез 45 then measured a real hit-rate on the
minimal Popen repro: 0/30 fresh-process runs stalled** (later срез 46 added
150 more, still 0 — 187 total runs of this shape, 1 stall), including 17
runs at срез 44's own reported "load 2.0-2.3" band, refuting system load in
that band as a sufficient cause on its own. **Срез 46 then re-ran срез 42's
own script — same test, same BiDi call, but launched through the real
`LumenBrowser`/`mozprocess` path instead of a bare `Popen` — and got 5
stalls out of 5 completed variant-runs across 3 fresh trials** (2 trials
2/2, 1 trial's variant C stalled before the outer command was killed by a
tooling timeout on variant D). **The launch mechanism срез 44 called
refuted is therefore the leading candidate again**: the bare-`Popen` shape
that срезы 43/45/46 pushed to 187 clean runs never goes through
`mozprocess`/`LumenBrowser` at all, so its near-0% hit rate is consistent
with `mozprocess` (or something `LumenBrowser.start()` does that a raw
`Popen` doesn't — env merging, `preexec_fn`, process-group setup,
`mozprocess.ProcessReader`'s reader thread) being necessary for the stall,
not with the stall being launch-independent. No `wchan` sample has been
captured yet — срез 42's script has no sampler; срез 43's sampler was only
ever wired to the *minimal* Popen shape, which barely reproduces. Ruled out
as the *sole or required* cause: `lumen-bidi-server`, `lumen-driver` (both
just surface a 30s `RecvTimeoutError`, not the source), content-serving
(live route vs. static copy — срез 42, still true, both stall identically
under the real launch path), system load in the 2.0-2.3 band alone (срез
45).
**Найден:** P2, WPT-RUN-6 срез 39, 2026-09-02, investigating why
`/console/console-log-large-array.any.html` (and its `.any.worker.html`
twin) sit in `timeout_audit.py`'s `unclassified` bucket. Continued срезы
40-46, 2026-09-02.

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

## WPT-RUN-6 срез 43: direct `subprocess.Popen` (no `LumenBrowser`, no
`mozprocess`) — first sample says «no stall», repeated sampling says
«unreliable single-shot conclusion»

Item 2's first move: spawn `lumen --bidi-port <port>` with a plain
`subprocess.Popen` — bypassing `LumenBrowser`'s process management (and
therefore `mozprocess.ProcessHandler`) entirely, not just
`TestRunnerManager`/`executorlumen.py` as срез 41 already did — and sample
`/proc/<pid>/task/*/wchan` through the run. Script:
`tests/wpt/verify_bug961_slice43_wchan.py` (committed this slice).

First run: `navigate returned OK in 1.19s`. No stall, no 31s silent window,
`wchan` samples show nothing but ordinary `futex_wait`/`epoll_wait`/
`inet_csk_accept` parks across all threads. Re-run 5 times back to back —
**every one completed in ~1.1s**, no exceptions. Taken alone this would
close item 2 by pointing at `LumenBrowser`/`mozprocess.ProcessHandler`
itself (env merging, `preexec_fn`, pipe buffering — see срез 44) as the
necessary ingredient for the stall, on top of срез 41's Variant B (bare
`BidiSession`, but still launched via `LumenBrowser`) and срез 42's Variant
C/D — neither of which used a raw `Popen`.

## WPT-RUN-6 срез 44: that conclusion does not survive a repeat — the same
direct-`Popen` shape stalls too, so single-shot A/B is not sufficient
evidence for this bug

Isolated the one remaining structural difference between срез 43's script
and a `LumenBrowser`-driven launch: `mozprocess.processhandler.Process.
__init__` (`tests/wpt/.venv/lib/python3.14/site-packages/mozprocess/
processhandler.py:118-126`) unconditionally sets
`preexec_fn = lambda: os.setpgid(0, 0)` (new process group for the child)
unless `ignore_children=True`, which `LumenBrowser`/`WebDriverBrowser` never
pass. Env merging was already ruled out — `WebDriverBrowser.env` is
`{**os.environ, **env}` (`browsers/base.py:324`), not a stripped dict.
Script: `tests/wpt/verify_bug961_slice44_setpgid.py` (committed this
slice) — Variant P (plain `Popen`, срез 43's exact shape) vs. Variant G
(same, plus the identical `setpgidfn` `preexec_fn`), back to back, same
process.

Result — **both variants stalled ~32s**, including Variant P, which is
structurally the same probe срез 43 ran 6 times (1 + 5 repeats) at ~1.1-1.2s
with zero failures:

```
[Variant P (plain Popen, no preexec_fn)] navigate FAILED: … (after 32.2s)
[Variant G (setpgid preexec_fn, mozprocess-identical)] navigate FAILED: … (after 32.2s)
```

Re-running срез 43's own script again immediately after (same binary, same
box, nothing else changed) went back to **1.07-1.20s across 3 more runs** —
i.e. the *same* minimal repro (`Popen`, no `preexec_fn`, static file server,
bare `BidiSession`, `navigate(wait="complete")`) produced both a clean
under-1.3s completion (срез 43, 6/6) and a 32s stall (срез 44 Variant P,
1/1) on the same machine within the same working session.
**`preexec_fn`/process-group is refuted as the variable** — Variant P and
Variant G stalled identically, matching each other, not the setpgid
hypothesis.

This means every A/B conclusion in this bug's history so far (срез 41's
"reproduces without orchestration", срез 42's "content-serving is not the
variable", срез 43's initial "direct Popen avoids it") was drawn from a
**single sample per condition** — sufficient to prove a mechanism is not
*required* to reproduce the stall (a single reproduction under a leaner
shape is real evidence), but **not sufficient to prove a mechanism
prevents it**, since the stall itself now looks intermittent rather than
tied to any one launch path tested so far. `uptime` during срез 44's run
showed load average 2.0-2.3 on an 8-core box (no other build/lumen process
visible in `ps --sort=-pcpu` besides ordinary desktop apps) — not obviously
saturated, but not controlled for either; no run in срезы 39-44 has logged
system load alongside its result.

## WPT-RUN-6 срез 45: item 3's hit-rate run — 0/30 stalls, including 17
runs at срез 44's own "load 2.0-2.3" load-average band

Ran срез 43's own script (`verify_bug961_slice43_wchan.py`, unmodified) 30
times total, **each in a fresh `python` interpreter process**
(`subprocess.run([sys.executable, ...])`, not a loop inside one script —
item 3's explicit requirement to rule out interpreter-level state leaking
between attempts). Driver: `tests/wpt/verify_bug961_slice45_hitrate.py`
(committed this slice) — records pass/fail parsed from each child's own
stdout, `/proc/loadavg` sampled immediately before every spawn, and keeps
each run's full stdout/stderr (incl. срез 43's `_WchanSampler` summary,
whenever the sampler collects enough to print one) in a per-run log under
`.tmp/` (gitignored, not committed — this is a measurement, not a fixture).

Result — **0/30 stalled**, all 30 completed in 1.5-2.1s wall-clock
(`navigate returned OK` in 1.0-1.3s):

```
[hitrate] SUMMARY: 0/10 stalled, 10/10 OK, 0/10 unknown   (batch 1, .tmp/bug961-slice45/)
[hitrate] SUMMARY: 0/20 stalled, 20/20 OK, 0/20 unknown   (batch 2, .tmp/bug961-slice45-batch2/)
```

`/proc/loadavg` at launch ranged 1.36-2.45 across the 30 runs (batch 2 runs
4-20 specifically sat at 2.11-2.45 1-minute load — the same band срез 44
reported, "load average 2.0-2.3", for its one stall). **This refutes system
load in that band as sufficient on its own**: 17 runs landed inside or
above срез 44's exact load window and every one of them completed cleanly,
so "load ~2-2.3" cannot be the deciding variable срез 44's single sample
pointed at — it was, at most, coincidental with that one stall.

This also sharpens what "intermittent" means here: срез 43 (6/6 clean) +
this slice (30/30 clean) is **36 consecutive clean runs** of the exact same
minimal shape against 1 confirmed stall (срез 44) and 5 stalls total across
срезы 41/42/44 (each under a *different* shape — live wptserve route,
static copy, this same Popen shape once). The true hit rate for this
specific minimal repro looks considerably below 1/10 — a wchan sample taken
*during* an actual stall (item 3's other ask) still has not been captured,
and at this hit rate a targeted capture attempt needs O(100) fresh-process
runs, not 30, to have good odds of landing on one. No wchan evidence to add
from this slice — every one of the 30 runs was too fast for the sampler to
see anything but ordinary `futex_wait`/`epoll_wait` parks.

## WPT-RUN-6 срез 46: matched-N re-run of both shapes — bare-Popen stays at
0/225, the real `LumenBrowser`/`mozprocess` launch path hits 5/5

Item 3's own suggestion, both halves. First, 150 more fresh-process runs of
срез 45's minimal bare-Popen repro (`verify_bug961_slice45_hitrate.py`,
unmodified, two batches of 75 — the first batch's driving `Bash` call hit a
tooling timeout after completing 75/150 and was restarted with a longer
budget rather than resumed, so both batches' logs are kept under
`.tmp/bug961-slice46/` and `.tmp/bug961-slice46-batch2/`, gitignored):
**0/150 stalled**, both batches, all runs completing in 1.5-2.1s. Combined
with срезы 43 (6) + 45 (30), that is **186 consecutive clean runs of the
bare-Popen shape against 1 confirmed stall (срез 44)** — the true hit rate
for this specific shape is now bounded well under 1%.

Second — the matched-N re-run item 3 flagged as still owed: срез 42's own
script (`verify_bug961_slice42_static.py`, unmodified), which boots the
real `wptrunner` `TestEnvironment` and launches the browser through
`product.get_browser_cls("lumen")`/`browser.start()` — wptrunner's actual
`LumenBrowser`, `mozprocess`-based, the exact mechanism срез 44 concluded
was refuted — run 3 times, each a fresh interpreter process (`python
tests/wpt/verify_bug961_slice42_static.py`, no args, ~35-70s per trial
since each boots its own wptserve instance and launches 2 browser
processes, variant C then variant D):

```
trial 1: variant C (static copy) STALL 32.0s   variant D (live route) STALL 32.0s
trial 2: variant C (static copy) STALL 32.0s   variant D (live route) STALL 32.0s
trial 3: variant C (static copy) STALL 32.0s   variant D (live route) — run killed by a
                                                Bash-tool 2-minute default timeout while
                                                variant D was still in flight, no result
```

**5 stalls out of 5 completed variant-runs, 0 clean.** No orphaned `lumen`/
`wptserve`/`python` process was left behind by the killed trial 3
(`ps aux` checked immediately after — clean).

This flips срез 44's conclusion: срез 44's "refutation" of the launch
mechanism rested on ONE bare-Popen stall sample, read as proof the stall
reproduces independently of `mozprocess`/`LumenBrowser`. Срезы 45/46 now
show that same bare-Popen shape barely reproduces at all (≤1/186), while
srez 42's shape — identical BiDi traffic, identical test id, the only
change being the launch path (`LumenBrowser`/`mozprocess.ProcessHandler`
via `product.get_browser_cls()` instead of a raw `subprocess.Popen`) —
reproduces on effectively every trial. The launch mechanism (env merging,
`preexec_fn`/process-group setup, or `mozprocess.ProcessReader`'s reader
thread racing the child) is the leading candidate again, not a refuted one.

No `wchan` sample has been captured yet in either shape at this slice:
срез 42's script has no sampler (it predates срез 43's `_WchanSampler`),
and срез 43's sampler was only ever wired into the shape that barely
stalls. The two need to be combined — srez 42's `_bare_control` (real
`LumenBrowser`/`mozprocess` launch) instrumented with срез 43's
`_WchanSampler`, sampling both the browser's own PID tree and (mozprocess
uses a reader thread) the interpreter's own threads — which at this hit
rate should catch a live stall on close to the first attempt.

## WPT-RUN-6 срез 47: root cause found and fixed — `mozprocess`'s unbuffered
(`bufsize=0`) stdout/stderr pipe makes `readline()` read ONE BYTE PER SYSCALL
for this test's 20MB no-newline line

Item 2's own suggestion: instrumented срез 42's actual launch path (real
`LumenBrowser`/`mozprocess.ProcessHandler`, via `product.get_browser_cls()`)
with срез 43's `_WchanSampler`, sampling BOTH process trees — the `lumen`
process itself, and (new this slice) the calling Python interpreter, since
`mozprocess.ProcessReader`'s reader threads run there, not inside `lumen`.
Script: `tests/wpt/verify_bug961_slice47_wchan_real_launch.py` (committed
this slice, reuses срез 42's `_build_kwargs`/`_make_static_server` and срез
43's `_WchanSampler` by direct import — no copy-paste).

Reproduced on the first attempt (matches срез 46's 5/5 hit rate). Wchan trace
for the full ~32s stall:

```
lumen process tree (pid=43889): every lumen/V8/IO thread at futex_wait or
  do_epoll_wait — ordinary idle parks — EXCEPT:
  tid=43931 comm='lumen-v8' wchan='anon_pipe_write.cold' count=203 span=[0.61s..31.86s]
    (blocked writing to a pipe for essentially the ENTIRE stall)

interpreter process (pid=43824, where mozprocess's reader threads live):
  tid=43890 comm='ProcessReaderSt' wchan='0' (RUNNING, not parked in the
    kernel) count=206 span=[0.60s..31.88s]
    (CPU-busy for essentially the entire stall, not blocked in a read syscall)
```

`lumen-v8` blocked on a pipe **write** while the Python-side reader thread is
CPU-busy rather than blocked on a pipe **read** is the opposite of what a
slow-consumer-blocks-producer scenario normally looks like at this sampling
granularity — it means the reader is spending its time in userspace/syscall
overhead, not waiting for bytes, while the writer just can't get room in the
pipe buffer fast enough. That points at the reader's *own* read loop being
the bottleneck, not real network/IO waiting or engine computation.

Traced to `mozprocess.processhandler.Process.__init__`
(`mozprocess/processhandler.py:97`): default **`bufsize=0`**, passed to
`subprocess.Popen` unchanged. Neither `browsers/base.py::WebDriverBrowser
._run_server` nor (pre-fix) `browsers/lumen.py::LumenBrowser._run_server`
override it, so every `lumen` launch through `mozprocess` — i.e. every real
wptrunner corpus run — got raw, unbuffered `stdout`/`stderr` pipes.
`io.FileIO.readline()` on such a stream has no buffer to search and falls
back to `io.RawIOBase`'s generic implementation: **one `read(1)` syscall per
byte** until a `\n` or EOF. `mozprocess.ProcessReader._read_stream`
(`processhandler.py:1076`) calls exactly `stream.readline()` on this raw
pipe. `console-log-large-array.any.js` logs a ~20MB array with **no embedded
newline until the very end** — the worst possible shape for this code path.

Isolated with ZERO lumen/wptrunner/mozprocess code in the loop (per
`docs/probe-method.md`'s "your own server, not the page and not the browser
log" rule) in `tests/wpt/verify_bug961_slice47b_bufsize0_readline.py`: a
plain `os.pipe()`, a writer thread pushing a 20 000 004-byte line with no
newline until the end, `readline()` timed on the read end opened two ways —

```
[probe] buffered io.BufferedReader (buffering=131072): readline() of 20000005 bytes took 0.024s
[probe] unbuffered io.FileIO (bufsize=0):              readline() of 20000005 bytes took 30.086s
[probe] ratio unbuffered/buffered: 1241.0x
```

**30.086s for the unbuffered case — matching this bug's observed ~30-32s
stall almost exactly, with nothing but stock `os`/`io`/`threading` involved.**
This also explains срез 43/45/46's ≤1/187 bare-`Popen` hit rate for free:
`subprocess.Popen(..., bufsize=1, universal_newlines=False)` (binary mode)
is silently promoted by `subprocess.py` to `io.DEFAULT_BUFFER_SIZE` ("line
buffering isn't supported in binary mode"), giving an ordinary
`io.BufferedReader` whose `readline()` is O(n) total, not O(n) syscalls —
that shape was never going to reproduce this, regardless of sample count.

**Fix** (`tools/wptrunner/wptrunner/browsers/lumen.py`, Lumen's own product
plugin — not vendored harness code, same status as the already-documented
`create_output_handler` override): pass an explicit
`bufsize=io.DEFAULT_BUFFER_SIZE` to every `mozprocess.ProcessHandler(...)`
call this module makes. `ProcessHandler.__init__` forwards unrecognized
kwargs straight into `Process.__init__` via `self.keywordargs`
(`processhandler.py:822-849`), so this needed no vendor patch and no
reapply-per-venv step (unlike the pywebsocket3 patch, CLAUDE.md's WPT-harness
gotcha) — just an explicit kwarg at the one call site this product's plugin
controls. The bidi (non-`--ipc-server`) launch path had no override at all
before this slice (`_run_server` unconditionally called
`super()._run_server()`), so it gained a full `_run_server_bidi` override
replicating `WebDriverBrowser._run_server` with that one added kwarg; the
`--ipc-server` path already had its own override and just needed the kwarg
added.

**Verified fixed**: re-ran срез 42's own script unmodified after the fix —
both variants that stalled 5/5 across срез 46's 3 trials now complete in
under 1.1s on 2 consecutive re-runs:

```
[probe] variant C (static copy) RESULT: navigate(wait=complete) returned in 0.96s
[probe] variant D (live route) RESULT: navigate(wait=complete) returned in 0.92s
[probe] variant C (static copy) RESULT: navigate(wait=complete) returned in 1.02s
[probe] variant D (live route) RESULT: navigate(wait=complete) returned in 0.96s
```

And the real acceptance command from "Как проверить фикс" below now passes
clean, no `--timeout-multiplier` needed:

```
0:11.82 TEST_END: Test OK. Subtests passed 1/1. Unexpected 0
tests: 1/1 harness OK; subtests: 1/1 passed
```

**Масштаб**: this fix applies to every corpus run through the real
`LumenBrowser`/`mozprocess` launch path (i.e. all of them) — any test whose
output contains a long line with a late or absent newline was paying this
same unbounded-syscall tax, not just this one console.log test. Unknown how
many of `timeout_audit.py`'s other `unclassified`/TIMEOUT ids this explains;
worth a full corpus re-run to remeasure now that this is fixed (out of scope
for this slice — see "Масштаб находки" above, still not independently
re-verified for the `.any.worker.html` twin either, though it shares the
exact same shape and should be fixed by the same change).

## Что нужно

1. **(closed by срез 42 — content-serving is not the variable, reconfirmed
   срез 46)**
2. **(CLOSED by срез 47 — root cause found: `mozprocess`'s unbuffered stdout
   pipe degrades `readline()` to one syscall per byte on a long no-newline
   line. Fixed in `browsers/lumen.py`, verified against both the direct
   repro and the real acceptance command.)**
3. **(superseded by item 2 — the hit-rate question this item asked is
   answered: it is not a property of the minimal repro's rare intermittency,
   it is the launch path. No further bare-Popen hit-rate runs needed.)**
4. Re-run the full WPT corpus (or at least `timeout_audit.py`'s
   `unclassified` bucket) now that this fix is in, to measure how many other
   ids it was silently costing 30s+ each — not done in this slice, scope is
   one WPT-RUN-6 task, not the whole corpus.

## Как проверить фикс

`tests/wpt/run_report.py --root console --all --recursive --offset 4 --limit
1 --binary target/dev-release/lumen` (no `--timeout-multiplier` override —
default 1×, 10s budget) — **verified PASSing above (срез 47)**, since the
underlying computation genuinely only needs well under 1s once the pipe is
buffered.

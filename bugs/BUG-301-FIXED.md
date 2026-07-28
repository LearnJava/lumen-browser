# BUG-301 — `tests/wpt/run_smoke.py` (wptrunner + wptserve) timed out: wptrunner overrides `/resources/testharnessreport.js` with its own message-queue report, so Lumen's `__lumen_wpt_results` stash never runs

**Статус:** FIXED 2026-07-18
**Компонент:** wptrunner integration (`tools/wptrunner/wptrunner/browsers/lumen.py` — product plugin `env_options`)
**Найден:** P2-wpt S4, 2026-07-17; root-caused + fixed P2-wpt, 2026-07-18

## Симптом

`LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py` reported
`TIMEOUT` for `/dom/nodes/Element-hasAttribute.html`, reproducibly, while a
manual BiDi driver hitting the identical binary + identical vendored test file
over a plain `http.server` completed correctly. Same binary, same BiDi driving,
same DOM — only the *server* differed.

## Root cause

The `wptserve` doc-root serves test files from disk, **but `wptrunner`
registers a higher-priority static route** for `/resources/testharnessreport.js`
(`tools/wptrunner/wptrunner/environment.py::TestEnvironment.get_routes`) that
serves *its own* bundled report script
(`tools/wptrunner/wptrunner/testharnessreport.js` — delivers results by pushing
onto `window.__wptrunner_message_queue`). That static route **wins over any
on-disk file of the same URL**, so Lumen's vendored
`tests/wpt/resources/testharnessreport.js` — the one that stashes results on
`window.__lumen_wpt_results` — was **never served under wptrunner+wptserve**.

`LumenTestharnessExecutor` (`executors/executorlumen.py`) polls
`window.__lumen_wpt_results`, which nothing on the page ever sets in that
configuration → the poll runs to timeout. Under a plain HTTP server the on-disk
file *is* served, so `__lumen_wpt_results` gets set and the harness "works
manually" — exactly the observed asymmetry.

## How it was bisected

1. Reproduced the timeout with a **manual** `BidiSession.bidi_only` probe
   (no `wptrunner`) pointed at the live `wptserve` — isolating the cause to the
   *server*, not the BiDi driving sequence.
2. Instrumented the vendored `testharness.js`/`testharnessreport.js` with
   `window.__thlog` breadcrumbs (served from disk by `wptserve`, read back over
   BiDi `script.evaluate`); the breadcrumbs were byte-identical for the working
   (plain) and failing (wptserve) runs *up to* completion — proving the harness
   ran and `Tests.complete()` fired in both.
3. Raw-captured the bytes `wptserve` served for `/resources/testharnessreport.js`
   → `(function() { if (window.__wptrunner_message_queue …`, i.e. wptrunner's
   own report, **not** Lumen's (`// Lumen's testharnessreport.js`). That was the
   whole difference.

("Not close-delimited framing" — a hand-rolled close-delimited/keep-alive
server serving Lumen's own report completed fine; "not Cache-Control"; "not the
BiDi driving" — all ruled out along the way.)

## Fix

`browsers/lumen.py::env_options` now sets
`"testharnessreport": [<repo>/tests/wpt/resources/testharnessreport.js]`, which
`get_routes` uses in place of wptrunner's default
`message-queue.js` + `testharnessreport.js` pair. `wptserve` then serves
Lumen's own report at `/resources/testharnessreport.js`, restoring the
`__lumen_wpt_results` contract the executor expects.

With the fix (and a *current* build — a stale `target/dev-release/lumen.exe`
predating recent subresource-fetch work will masquerade as an unrelated
empty-body fetch; rebuild first), `run_smoke.py` now drives the test end to end:
`Test OK. Subtests passed 1/2` — subtest 1 genuinely **FAIL**s on the missing
`Element.setAttributeNS` ([BUG-309](BUG-309-OPEN.md), recorded as
`expected: FAIL` in `tests/wpt/metadata/dom/nodes/Element-hasAttribute.html.ini`),
subtest 2 PASSes. This clears the "harness genuinely checks assertions"
milestone (`docs/tasks/p2-wpt-integration.md`).

## Note (environment)

On a machine whose Windows *excluded TCP port ranges*
(`netsh interface ipv4 show excludedportrange protocol=tcp`) cover the default
`8000/8001` (common with Hyper-V/WSL/Docker reservations), `wptserve` fails to
bind with `WinError 10013` before this bug's path is even reached. Point
`tests/wpt/config.json`'s `http` ports at a free range (e.g. `8300/8301`) for a
local run; the committed default stays `8000/8001`.

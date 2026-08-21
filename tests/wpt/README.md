# Web Platform Tests (WPT) — Lumen integration

P2-wpt (`docs/tasks/p2-wpt-integration.md`, slices S1–S8). Runs the real, unmodified
`wptrunner` against Lumen over WebDriver BiDi (`lumen --bidi-port N`) — not a
bespoke test runner. See the task doc for the full architecture and slice plan.

**Status:** S1–S7 done (this task complete; S8 reftests is a separate
follow-up). `tests/wpt/run_smoke.py` drives the real, unmodified `wptrunner`
against `lumen --bidi-port` (navigate + eval) end to end; `tests/wpt/run_suite.py`
(S7) runs the **whole curated subset** — the 18 synchronous `dom/nodes/` tests
(S5) plus the 3 async `MutationObserver-*` tests (S6), 21 tests / 64 checks —
as one pass/fail gate against its committed `.ini` expectations, **green**
(0 unexpected), fully offline. See "The curated subset" and "Running the whole
suite" below. Nine real engine/shell gaps surfaced and were fixed while
proving the S4 path (the last blocker, [BUG-301](../../bugs/BUG-301-FIXED.md),
was fixed 2026-07-18): [BUG-278](../../bugs/BUG-278-FIXED.md)
(HTTP client rejected `wptserve`'s close-delimited responses), [BUG-279](../../bugs/BUG-279-FIXED.md)
(`document.getElementsByTagName` was missing entirely — broke `testharness.js`'s
own module-level setup), [BUG-280](../../bugs/BUG-280-FIXED.md) (`window` wasn't
the JS engine's real global object, so `testharness.js`'s `expose()`-based public API
was unreachable as bare identifiers), [BUG-291](../../bugs/BUG-291-FIXED.md) (DOM
node wrappers weren't interned, breaking `===` node identity and crashing
`testharness.js`'s built-in results renderer, `Output.show_results`),
[BUG-296](../../bugs/BUG-296-FIXED.md) (a stale on-disk `last_session.db` — session
restore, not a "default homepage" feature — could reopen a leftover tab and race the
test driver's explicit `browsingContext.navigate`; `--bidi-port`/`--mcp-live-port`
launches now skip session restore), [BUG-298](../../bugs/BUG-298-FIXED.md)
(`Element`/`DocumentFragment`/`ShadowRoot`.querySelector(All) searched the whole
document instead of the calling node's subtree — `Output.show_results` builds a
detached results tree and queries into it, always getting nothing),
[BUG-299](../../bugs/BUG-299-FIXED.md) (`Element.prototype.insertAdjacentText` was
missing entirely, thrown from the same code path), [BUG-300](../../bugs/BUG-300-FIXED.md)
(`browsingContext.navigate`'s `DocumentReady` wait could ACK using the *previous*
page's stale `layout_box` before the new page had even started loading), and
[BUG-301](../../bugs/BUG-301-FIXED.md) (`wptrunner`'s vendored harness registers its
own static route for `/resources/testharnessreport.js`, winning over Lumen's
vendored file that sets `window.__lumen_wpt_results` — plus a related, independently
found persistent-on-disk-HTTP-cache angle on the same symptom, see
[BUG-315](../../bugs/BUG-315-FIXED.md)). Together BUG-298/299/300 fully explain (and
disprove as environment-flaky) the "`script.evaluate`-install race" theory
previously in `CLAUDE.md` → "Known gotchas". See those bug files and the task doc's
S4 section for the full diagnosis trail (BiDi-eval-based bisection of
`testharness.js`'s execution).

## What's here

- `tools/wptrunner/`, `tools/manifest/`, `tools/serve/`, `tools/wptserve/`,
  `tools/webdriver/`, `tools/metadata/`, `tools/gitignore/`, `tools/localpaths.py`
  (repo root, alongside `crates/`) — vendored upstream WPT tooling. Pin +
  rationale: `tests/wpt/VENDOR.md`. **Not upstream-unmodified in one spot:**
  `tools/wptrunner/wptrunner/products.py`'s `BUILTIN_PRODUCTS` frozenset has a
  `"lumen"` entry added (see `docs/tasks/p2-wpt-integration.md` S3 DoD) — there is
  no plugin-registration seam that avoids touching this file, so a re-vendor must
  reapply that one line.
- `tools/wptrunner/wptrunner/browsers/lumen.py` — **ours** — the wptrunner
  product plugin: `LumenBrowser` (spawn/stop `lumen --bidi-port <port>`,
  `WebDriverBrowser` subclass) + `__wptrunner__` registration.
- `tools/wptrunner/wptrunner/executors/executorlumen.py` — **ours** —
  `LumenBidiProtocol` (BiDi-only session negotiation via
  `webdriver.bidi.client.BidiSession.bidi_only`, no classic HTTP session) and
  `LumenTestharnessExecutor.do_test` (S4): `browsingContext.navigate` then
  `script.evaluate`-polls for `tests/wpt/resources/testharnessreport.js`'s
  JSON result global, tolerating the transient "JS context not available"
  BiDi error while the new document's JS runtime is still installing. Clears
  that global and marks the outgoing document before every navigation
  ([BUG-380](../../bugs/BUG-380-FIXED.md)) — a navigation that fails to load
  answers successfully over BiDi while leaving the previous document live, so
  without the reset the next test read the previous test's result.
- `tests/wpt/resources/testharness.js` — vendored upstream client-side test harness.
- `tests/wpt/resources/check-layout-th.js` — vendored upstream self-checking
  layout helper (`WPT-RUN-1`, `ROADMAP.md`): reads `data-expected-*`
  attributes and asserts them, giving `expected width = 342, actual = 318`
  diagnostics for free. The single most-referenced out-of-category helper in
  `css/` (1119 files — `tests/wpt/find_missing_resources.py --root css`).
- `tests/wpt/find_missing_resources.py` — static scan for out-of-category
  `src=`/`href=`/`url()` references a category's test files 404 on, without
  running a single test (a live `--all --root css --recursive` run doesn't
  finish — see `docs/wpt-status.md` row `css`). Use before vendoring more
  helpers to find the next highest-ROI gap.
- `tests/wpt/resources/testharnessreport.js` — **ours** (S4) — on harness
  completion, serializes `[url, harness_status, message, stack, subtests]` to
  JSON on `window.__lumen_wpt_results`, polled by `do_test` above.
- `tests/wpt/dom/nodes/` — one vendored test category. S4's smoke test is
  `Element-hasAttribute.html` (not `Document-createElement.html`, floated as an
  "e.g." example when this file was first drafted — turned out to need
  un-vendored iframe fixtures and `async_test`, not actually trivial).
- `tests/wpt/requirements.txt` — pip requirements to make the above importable.
- `tests/wpt/verify_s3_bidi_session.py` — S3 verification: spawns a real
  `lumen --bidi-port <port>` and confirms BiDi session negotiation succeeds
  (real `sessionId` + `capabilities`). Run with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/verify_s3_bidi_session.py
  ```

- `tests/wpt/verify_devx6_bidi_scenarios.py` — **ours** (DEVX-6, `ROADMAP.md`) —
  integration scenario tests for six previously-unused BiDi commands
  (`network.setOfflineStatus`, `network.addIntercept`+`failRequest`/
  `continueRequest`, `browser.setTimezoneOverride`,
  `emulation.setUserAgentOverride`) against a real spawned `lumen --bidi-port`
  window, same raw `BidiSession` pattern as `verify_s3_bidi_session.py`
  (not wptrunner). Checks two things per command: the protocol round-trip
  (real verification value, catches `lumen-bidi-server` regressions) and
  whether a live page actually observes the effect — all six now wired for
  real ([BUG-295](../../bugs/BUG-295-FIXED.md), closed 2026-08-06; `XFAIL(BUG-295)`
  remains in the report as a defensive fallback, not the expected outcome).
  Also documents a separate, environment-dependent gap
  found while writing it: the live window's JS runtime can fail to install at
  all in some sessions (`SKIP(env)` — see `CLAUDE.md` "Known gotchas"). Run
  with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/verify_devx6_bidi_scenarios.py
  ```

- `tests/wpt/verify_bug380_navigation_staleness.py` — **ours** —
  [BUG-380](../../bugs/BUG-380-FIXED.md) regression check: drives the real
  `LumenTestharnessExecutor._run_testharness` coroutine over two navigations
  (one that loads, one to a dead port) against a spawned `lumen --bidi-port`,
  and fails if the second inherits the first's testharness result instead of
  erroring out. Run with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/verify_bug380_navigation_staleness.py
  ```

- `tests/wpt/run_smoke.py` — **ours** (S4) — minimal driver that calls
  `wptcommandline`/`wptrunner.run_tests` directly against the smoke test (see
  its own docstring for why this isn't `tools/wpt/wpt`). Passes
  `--no-restart-on-unexpected`: wptrunner's own default respawns the browser
  process after every test whose result doesn't match its expectation, which
  under `--all` (no committed `.ini` for most tests) meant a fresh `lumen.exe`
  per failing test. One `lumen.exe` process now runs the whole selected test
  set, reusing a single browsing context (`LumenBidiProtocol.context_id`,
  `executorlumen.py`) that gets a fresh `browsingContext.navigate` per test —
  still test-isolated, just not process-isolated. That isolation rests on the
  navigation actually loading, which is why the executor also resets the result
  global explicitly ([BUG-380](../../bugs/BUG-380-FIXED.md)). The browser still
  restarts on an actual crash/hang. Run with:

  ```bash
  LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py
  ```

  Both scripts default to `target/<LUMEN_PROFILE>/lumen.exe` (`LUMEN_PROFILE`
  env var, default `release`), same convention as `graphic_tests/run.py`.
- `tests/wpt/run_suite.py` — **ours** (S7) — the CI wrapper: discovers the
  curated subset from the committed `metadata/dom/nodes/*.ini` expectations
  (one test id per `.ini`) and runs them all through `run_smoke.run()` as a
  single pass/fail gate — exit 0 iff 0 unexpected results. This is the
  repeatable local/CI invocation (see "Running the whole suite" below). Adding
  an `.ini` grows the gate; there is no separate list to maintain.
- `tests/wpt/run_report.py` — **ours** — HTML report, not a gate: always runs
  and always exits 0 (unless the run itself couldn't start), writing a
  self-contained `.tmp/wpt-report.html` (test/subtest counts, pass/fail per
  test, expandable per-subtest detail with the failure message, and whether
  each result matched its `.ini` expectation). Defaults to the same curated
  subset as `run_suite.py`; `--all` instead runs every vendored `.html` under
  `dom/nodes/` (168 files, not just the 20 curated ones) — most of those were
  never vetted for this minimal BiDi-only executor (no `test_driver.*`,
  multi-window, iframes), so expect ERROR/TIMEOUT/FAIL noise there, not bugs
  worth filing without individually checking first; use it to survey, not to
  gate. See "HTML report" below.
- `tests/wpt/config.json` — **ours** (S4) — `wptserve` config override: pins
  `browser_host` to `127.0.0.1` (the default, `web-platform.test`, needs
  `/etc/hosts` entries this task's "no live network" rule can't rely on) and
  disables the `wss`/`h2`/`webtransport-h3`/`dns` servers the smoke test
  doesn't need (Python 3.14's `ssl` module dropped `wrap_socket`, breaking
  `wptserve`'s `wss` server; unrelated to Lumen). HTTP ports are `18300`/`18301`,
  not the WPT default `8000`/`8001` — the 8000-range falls inside a Windows
  dynamic excluded-port range here (`netsh interface ipv4 show
  excludedportrange protocol=tcp`), so `wptserve` failed to bind with
  `WinError 10013`. The first replacement, `8300`/`8301`, turned out to sit
  inside this machine's *ephemeral* range (1024-15000, `netsh int ipv4 show
  dynamicport tcp`) and was stolen by an unrelated process on 2026-07-28,
  costing three failed runs — hence the move above every common ephemeral
  range. See "Troubleshooting" below.
- `tests/wpt/corpus_stats.py` — **ours** (WPT-RUN-4) — the pass-rate
  *denominator*, read from `metadata/MANIFEST.json` rather than from a file
  glob. Every other count in this tree (`all_vendored_test_ids()`, the numbers
  quoted in `docs/wpt-status.md`) comes from globbing `*.html`, which
  over-counts (a `-ref.html` is a file, not a runnable id) and under-counts
  (`?variant`, `.any.js` expansion) at the same time, so none of them are
  comparable to wpt.fyi/Servo/Ladybird. Prints per-category/per-type id counts;
  `--json` dumps the table. What is in the denominator and why —
  `docs/wpt/pass-rate.md`.
- `tests/wpt/run_corpus.py` — **ours** (WPT-RUN-4) — runs the **whole**
  vendored corpus in shards and scores it into one pass-rate. Selects from the
  manifest, drives `run_smoke.py` as a subprocess per shard (a hung shard can
  then be killed — by process *tree*, or the orphaned `lumen.exe` keeps its
  BiDi port and breaks the next shard), checkpoints after every shard
  (`--resume`), budgets each shard's wall-clock from the per-test timeouts the
  manifest declares (`timeout: long` → 60 s, otherwise 10 s, plus wptrunner's
  5 s `extra_timeout`, divided by `--processes`) instead of a flat per-id
  constant, and updates `MANIFEST.json` exactly once per run (wptrunner's
  default would rescan 72k files on each of the ~400 shards). `--pilot` runs
  ten categories chosen to exercise the orchestrator's hazards (https-only, ws,
  reftest-dominated, an unexecutable test type) rather than the engine.
  Scoring — including "an id that never ran scores 0" — is written down in
  `docs/wpt/pass-rate.md`. Two flags exist because a corpus run must never
  quietly misreport its own coverage:
  - `--skip-https` excludes `.https.` ids from the *run* (they stay in the
    denominator and score 0). Measured cost of not skipping them: 200 sampled
    https ids gave 200 TIMEOUT / 0 subtests / 200 `UnknownIssuer`, ~39 s each —
    ≈14 h for all 6874 to reach a result already known ([BUG-785](../../bugs/BUG-785-FIXED.md)).
    The summary prints `NOT RUN ON PURPOSE` with the count and the reason.
  - a shard killed on its budget is recovered from the mozlog raw stream
    (`--log-wptreport` is written only at the end, so a kill used to lose the
    whole shard). Recovered shards are named in the summary; a shard that ran
    nothing because everything was excluded is reported separately
    (`ran nothing`), so the "recovered" line keeps meaning "something went
    wrong".
  - a shard that produced **no verdicts at all** is not reported as `ran`.
    wptrunner opens `--log-wptreport` up front, so a shard that dies before its
    first test leaves a zero-byte report that used to be indistinguishable from
    a category with nothing to run — and `ran` is what makes `--resume` treat a
    shard as finished. The two are now told apart by wptrunner's own words in
    the shard log (`Unable to find any tests` / `No tests ran` → `no-tests`,
    terminal; anything else → `empty-report`, retried once on the spot and
    again on the next `--resume`). Same for a shard that exits on a code
    wptrunner never returns (anything but 0/1/64): it was ended from outside,
    is recorded as `signalled`, and is reachable by `--retry-timeouts`.
    Measured cost of not doing this: on the Windows half of the 2026-08-20
    corpus run, 158 shards (17 683 ids) came back in ~11 s with an empty report
    and 41 more (13 136 ids) were killed mid-run — 36 % of the corpus scored 0
    while the run reported 479/479 shards `ran` (WPT-RUN-5 slice 16).
  - the summary and the `--run-json` snapshot say **why** the ids with no
    verdict have none, instead of one `never ran: N` line covering four
    unrelated causes: a type no executor is registered for (a runner gap,
    `WPT-RUN-8`), a shard killed on its budget, a shard that wrote an empty
    report, and — the one that matters — ids lost inside a shard that reported
    success, printed as `SILENT HOLE` with the shards named. The classification
    is `score_audit.accounting`, called from the run rather than copied into
    it, so the audit and the summary can never disagree. Measured cost of not
    printing it: the Windows half of the 2026-08-20 run finished
    `479/479 shards ran` with 27 117 ids at zero, and which of them were a
    ceiling and which a hole took a separate audit a day later
    (WPT-RUN-5 slice 19). `--selftest` checks the split on a synthetic run.
  - `--resume` therefore treats a budget-killed shard whose raw stream
    salvaged something as **done**, instead of replaying it: the budget is the
    same and the machine is no faster, so the replay dies at the same wall and
    buys nothing — measured at 50 min per resume on the 2026-08-20 Linux run.
    Which shards were kept is printed, not silent. `--retry-timeouts` runs them
    again, which is what a resume with a wider budget wants;
    that is safe because the previous raw stream is rotated to `.raw.jsonl.prev`
    and both generations are read at scoring time, so a shorter retry cannot
    destroy the results the first attempt already had (mozlog opens `--log-raw`
    with mode `"w"`).
- `tests/wpt/score_audit.py` — **ours** (WPT-RUN-5 slice 15) — audits a run's
  *zero bucket* before its number is published: splits every id with no verdict
  into "type has no executor" / "shard killed on its budget" / **leak** (an id
  lost inside a shard that ran to completion), prices what the killed shards
  were actually worth, and fits the wall-clock cost of a TIMEOUT against a
  resolved test. Read-only, safe against a live run's `--out-dir`. A `LEAK`
  line means the number is not publishable until it is explained — on the
  2026-08-20 Linux run that bucket was empty (0 of 24 702 reftest ids, 0 of
  15 572 testharness ids outside the killed shards).
  Since slice 19 the same accounting also runs as part of `run_corpus.py`'s own
  summary — this script stays the way to get the leaked ids listed, the kill
  cost and the wall-clock fit, none of which belong in a run's summary line.
  `--snapshot docs/wpt/runs/<date>.json` runs the same audit on a *published*
  snapshot, which is all another machine's number ships with — the run
  directory stays on that machine. `--compare <second snapshot>` additionally
  checks the two runs against each other on the categories both reached (the
  engine is deterministic, so a category both ran to the same depth must
  produce identical counters) and prints what the halves are worth fused, per
  category, taking whichever run executed more of it. Added by slice 16, which
  is how the Windows half's 36 % hole was found.
- `tests/wpt/port_guard.py` — **ours** (WPT-RUN-5 slice 18) — makes sure a run
  owns the `wptserve` ports before it uses them. `config.json` pins every port,
  and `wptserve` starts its servers as `multiprocessing` children that survive
  the run above them, so a run killed mid-flight strands a full set of servers
  that live forever. The next run then does not fail: on Linux the stranded
  server *answers* it (328 of the 342 shards of the 2026-08-20 Linux half were
  served this way — every shard that got as far as needing a server), on
  Windows the shard dies ~11 s in with an empty report (the 158-shard hole
  slice 16 found). `--report` names the holders, `--reclaim` kills the stranded
  ones, `--selftest` proves detect → classify → reclaim on a spare port. Never
  kills a port holder that belongs to somebody else's *running* corpus run —
  it stops and says so. `run_corpus.py` calls it before every shard;
  `--no-port-guard` turns that off.
- `tests/wpt/expectations.py` — **ours** (TEST-3, `docs/tasks/p2-test-track.md`) — generates and
  gates on per-category `.ini` baselines for `run_report.py --update-expected`/`--check`, on top
  of the same native `wptrunner` `--metadata` mechanism `run_suite.py` uses for `dom/nodes`, not
  a separate format. See "Per-category regression gate" below.
- `tests/wpt/metadata/` — `--metadata` root; holds the generated (gitignored)
  `MANIFEST.json` and the committed `.ini` expectations: `metadata/dom/nodes/` is the hand-curated
  S5/S6 set `run_suite.py` gates on, every other `metadata/<category>/` is a TEST-3 ratchet
  baseline generated by `run_report.py --update-expected` (partial coverage so far — see
  `docs/tasks/p2-test-track.md#test-3-состояние` for which categories have one).

## Python setup

Requires Python 3.9+ (verified against 3.14). From the repo root:

```bash
python -m venv .venv-wpt          # any venv location outside the repo's gitignored area works
.venv-wpt/Scripts/python -m pip install -r tests/wpt/requirements.txt   # Windows
# .venv-wpt/bin/python -m pip install -r tests/wpt/requirements.txt    # Linux/macOS
```

This is tooling setup only — not a Cargo dependency, no `docs/plan/tech-stack.md`
entry needed (see that file's dependency-policy scope: Rust deps only).

### Verifying the install (import-chain smoke check)

Confirms the vendored tree + pip deps actually resolve, cheaper than a full
`run_smoke.py` run when only checking the Python side:

```bash
python - <<'PY'
import sys, os
root = os.path.abspath(".")
here = os.path.join(root, "tools")
sys.path[:0] = [root, here, os.path.join(here, "wptserve"),
                os.path.join(here, "webdriver"), os.path.join(here, "wptrunner")]

import localpaths                       # noqa: F401  (repo_root bootstrap)
import manifest.manifest                # noqa: F401  (test manifest)
from tools.serve import serve           # noqa: F401  (local HTTP/WS test server)
import wptrunner.wptrunner              # noqa: F401  (the runner)
import wptrunner.wptcommandline         # noqa: F401  (CLI arg parsing)
from webdriver.bidi.client import BidiSession  # noqa: F401  (S3 will drive this)
print("wptrunner import chain OK")
PY
```

Expected output: `wptrunner import chain OK`. This is exactly the import closure
`tools/wptrunner`'s own module-load time touches (`environment.py` →
`tools.serve.serve`, `testloader.py`/`metadata.py` → `manifest.manifest`) — if any
of it breaks after a re-vendor or a dependency bump, this is where it'll show up
first, before anything BiDi-specific.

## Fully offline

Once `pip install -r tests/wpt/requirements.txt` has populated the venv, nothing
above touches the network — the vendored tree in `tools/`/`tests/wpt/` is a
committed snapshot (`tests/wpt/VENDOR.md`), not a submodule or a runtime clone.

## The curated subset (S5 + S6)

`metadata/dom/nodes/*.html.ini` pins the expected result of a curated set of
**21 `dom/nodes/` tests** — 18 fully-synchronous ones (S5; no iframes / XHR /
`testdriver` / `async_test`) plus 3 async `MutationObserver-*` tests (S6;
`promise_test`/`async_test`, the only self-contained async tests in the
vendored `dom/` corpus). Every genuine failure is recorded as `expected: FAIL`
(and one whole-harness `expected: TIMEOUT`, `MutationObserver-disconnect.html`),
so the whole set runs **green** (0 unexpected) and acts as a regression ratchet
— same idea as `KNOWN_DEBTORS` in `graphic_tests/`, but tool-native. Each `.ini`
is header-commented with the engine bug it tracks (BUG-302/309/310/311/312/
313/314 for S5; BUG-317/318/319 for S6); flip a `FAIL` to `PASS` in the same
commit that lands the fix.

## Running the whole suite (S7 gate)

`tests/wpt/run_suite.py` is the repeatable local/CI invocation: it discovers
the curated subset from the committed `.ini` files (no hand-kept test list) and
runs it as one pass/fail gate. On Windows Git Bash set
`MSYS2_ARG_CONV_EXCL='/dom'` so the leading-slash test IDs the runner emits
aren't mangled into Windows paths:

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
BIN=$(cygpath -w "$PWD/target/dev-release/lumen.exe")
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_suite.py --binary "$BIN"
# → "running 20 curated WPT tests" then
#   "Ran 61 checks (41 subtests, 20 tests) ... Unexpected results: 0", exit 0
```

The curated-subset size drifts as tests are added/excluded (see "Adding a test"
below) — don't hardcode it elsewhere; `run_suite.py --binary "$BIN"` always
prints the current count.

## HTML report

`tests/wpt/run_report.py` runs the same curated subset (or, with `--all`,
every vendored `dom/nodes/*.html` test) and writes a self-contained HTML file
— open it in any browser, no server needed. Unlike `run_suite.py` it's not a
gate: it always writes the report and exits 0 regardless of how many tests
failed, so use it to *look at* results rather than to fail a build.

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
BIN=$(cygpath -w "$PWD/target/dev-release/lumen.exe")
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" --out .tmp/wpt-report.html
# → "tests: 20/20 harness OK; subtests: 35/41 passed"
#   "report written to .tmp/wpt-report.html"
```

The report shows, per test: harness status (`OK`/`ERROR`/`TIMEOUT`/`CRASH`),
subtests passed/total, and duration; expand a row for the per-subtest
breakdown with failure messages. Summary cards at the top separate "raw"
pass/fail counts from "unexpected (vs `.ini`)" — a subtest can legitimately
`FAIL` while still being 0 unexpected, if that failure is pinned as
`expected: FAIL` (a tracked, known gap) rather than a surprise regression.

`--all` runs every vendored/generatable test under `--root` (default
`dom/nodes`, 168 files) instead of just the 20 curated ones — most were never
vetted for this project's minimal BiDi-only executor (no `test_driver.*`,
multi-window, iframes), so expect a lot of ERROR/TIMEOUT/FAIL noise there.
Useful to survey what else might be worth curating next; don't file bugs off
it without checking each failure individually first (same discipline as
"Adding a test" below) — and budget more time for it (no per-test
parallelism). Pass `--root FileAPI` (or any other vendored category under
`tests/wpt/`) plus `--recursive` to survey a category organized into
subdirectories — `--recursive` walks the directory tree and expands
`.any.js`/`.window.js` templates into their `.any.html`/`.window.html` ids
the way `wptserve`'s `AnyHtmlHandler`/`WindowHandler` do at request time,
skipping `support`/`resources` fixture dirs and `-manual.html` tests.
Without `--recursive`, `--root` still just globs `*.html` at that
directory's top level (the `dom/nodes` default's original, deliberately
non-recursive behavior — its own subdirectories are crashtests/other
never-vetted sub-suites, not part of the 168-file count).

(Omit `--binary` and it defaults to `target/$LUMEN_PROFILE/lumen.exe`; pass it
explicitly when running the script from a `git worktree`, whose own `target/`
is empty. Use `run_smoke.py` with an explicit test-id list instead only to run
an ad-hoc subset.)

## Per-category regression gate (TEST-3)

`run_suite.py` only gates the hand-curated `dom/nodes` subset. For any other
vendored category, `run_report.py --update-expected`/`--check`
(`tests/wpt/expectations.py`, `docs/tasks/p2-test-track.md`) turn a `--all`
run's current result into a committed baseline and a pass/fail gate against
it, without needing every test to actually PASS first — most vendored
categories have a large, expected FAIL/ERROR count against this minimal
executor, and that's fine; the gate only fires on something getting *worse*.

```bash
# once, after vendoring/triaging a category — writes tests/wpt/metadata/<category>/*.ini
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" \
    --all --root <category> --update-expected
# on every later run — exit 0 unless something regressed
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py --binary "$BIN" \
    --all --root <category> --check
```

A regression is either a `(sub)test` that was expected `PASS`/`OK` and no
longer is, or any new `TIMEOUT` (a hang is always worth surfacing, even on an
already-known-bad test) — printed as `REGRESSION: ...`. An unexpected `PASS`
(the baseline can now be tightened) or any other status swap (e.g.
`FAIL` -> `ERROR`) is printed as informational, not a gate failure. Refuses
`--root dom/nodes`: that subtree stays `run_suite.py`'s hand-curated gate,
regenerating it here would overwrite its annotated, fully-PASS `.ini` files.

## Adding a test / growing the suite

1. Pick a **synchronous** `test()`-based test (grep the candidate for
   `async_test`/`promise_test`/`test_driver`/`<iframe>`/`XMLHttpRequest` — skip
   if any match; those wait on machinery the BiDi-only executor doesn't drive
   yet). Confirm any `<script src=...>` helpers it pulls are vendored under
   `tests/wpt/` (a missing helper makes the test error, not fail cleanly).
2. Run it through `run_smoke.py` (above) with an **empty** `--metadata` dir to
   see the raw per-subtest result (all results show as "unexpected"), or read a
   `--log-wptreport=out.json` dump.
3. For each genuinely-failing subtest, add an `[<subtest name>]` /
   `expected: FAIL` block to `metadata/dom/nodes/<test>.html.ini` (escape `\`,
   `[`, `]` in the heading). File a `BUG-NNN` for the underlying engine gap and
   name it in the `.ini` header comment — **never edit the vendored test to
   make it pass.** A whole-test `TIMEOUT`/`ERROR` (harness never completed) is a
   deeper gap: prefer excluding the test and filing the bug over pinning
   `expected: TIMEOUT`.
4. Re-run the curated set and confirm it's still green (0 unexpected).

## Troubleshooting: "Servers failed to start: http:8301"

`wptserve` binds the two fixed ports from `tests/wpt/config.json`. If either is
taken, the run dies after ~35 s of CA generation with an opaque traceback whose
only useful line is

```
wptserve CRITICAL Failed to start HTTP server on port <N>; is something already using that port?
[WinError 10013] (WSAEACCES — access denied, *not* "in use")
```

Two traps when diagnosing this:

* **`netstat` can show nothing.** A socket merely *bound* (not listening, not
  connected) is invisible to `netstat -ano`; `Get-NetTCPConnection -LocalPort <N>`
  shows it with `State: Bound` and the owning PID. On 2026-07-28 that PID was a
  VPN client holding 8301/8302/8304/8305/8307/8309 this way.
* **The ports used to live inside this machine's ephemeral range.**
  `netsh int ipv4 show dynamicport tcp` reported start 1024, 13977 ports — i.e.
  1024-15000, which contained the old 8300/8301. Any outgoing connection from
  any process could therefore be handed a WPT port. That is why the ports were
  moved to **18300/18301** (above every common ephemeral range: Windows'
  default 49152+, Linux' 32768+, and this machine's 1024-15000).

Quick check for whether a port is usable at all:

```bash
python -c "
import socket
s=socket.socket(); s.bind(('127.0.0.1',18301)); s.listen(1); print('OK')"
```

## Troubleshooting: `wss` server fails with `module 'ssl' has no attribute 'wrap_socket'`

`tests/wpt/config.json` enables the `ws`/`wss`/`h2` server ports (needed by
the `websockets` category, `docs/wpt-vendor-notes/websockets.md`) — `ws` and
`h2` work with a stock `pip install -r requirements.txt`, but `wss`
(`tools/serve/serve.py::WebSocketDaemon`, backed by `pywebsocket3`) does not:
`pywebsocket3==4.0.2`'s `websocket_server.py` calls the deprecated
`ssl.wrap_socket()`, which Python 3.12+ removed outright. Symptom:

```
wptserve CRITICAL start_wss_server: Caught exception from WebSocketDomain: module 'ssl' has no attribute 'wrap_socket'
```

— and because `wss`'s port is non-`None` in the committed config,
`TestEnvironment.ensure_started()` treats this as fatal and aborts the
**entire** run (`OSError: Servers failed to start: wss:<port>`), not just the
tests that need `wss`. This isn't fixed in the repo (`tests/wpt/.venv` is
gitignored — nothing under `site-packages` is vendored/committed), so it
resurfaces on every fresh `pip install`. One-time local fix, applied to the
installed package (not `tools/wptserve`/`tools/wptrunner` — this is a pip
dependency, not vendored WPT tooling, so patching it doesn't violate the
unmodified-vendor rule): in
`<venv>/Lib/site-packages/pywebsocket3/websocket_server.py`, replace the
`ssl.wrap_socket(socket_, keyfile=..., certfile=..., ca_certs=...,
cert_reqs=...)` call (~line 160) with the modern `SSLContext` equivalent:

```python
ssl_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ssl_ctx.load_cert_chain(certfile=server_options.certificate,
                         keyfile=server_options.private_key)
if server_options.tls_client_ca:
    ssl_ctx.load_verify_locations(cafile=server_options.tls_client_ca)
ssl_ctx.verify_mode = client_cert_
socket_ = ssl_ctx.wrap_socket(socket_, server_side=True)
```

Without this patch, revert `tests/wpt/config.json`'s `"wss": [...]` to
`[null]` to keep other categories runnable — but note `websockets/
constants.sub.js` (and any future `.sub.js` that unconditionally embeds
`{{ports[wss][0]}}`/`{{ports[h2][0]}}` regardless of which URL variant is
requested — the pipe substitution runs over the whole file before any JS
executes, so an unreachable `if` branch still needs its token to resolve)
then 500s on **every** variant, not just `?wss`, because the port
substitution fails file-wide when any referenced scheme has no server.

## Re-vendoring

See the "Re-vendoring" section of `tests/wpt/VENDOR.md`.

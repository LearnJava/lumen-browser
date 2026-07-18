# BUG-315 — `tests/wpt/run_smoke.py` timed out after the BUG-291 fix, despite the same page reaching a genuine result over a direct BiDi probe

**Renumbered 2026-07-18** from `BUG-295`, then `BUG-310` — assigned twice more by independent parallel sessions to different bugs (six BiDi commands with no live-window effect, see [BUG-295](BUG-295-OPEN.md); ElementTraversal/`ParentNode.children` gap, see [BUG-310](BUG-310-OPEN.md)); this bug kept its content but moved to the next free number each time.

**Статус:** FIXED 2026-07-17
**Компонент:** `tools/wptrunner/wptrunner/browsers/lumen.py` (product plugin, wrong report.js route) + `crates/shell/src/main.rs` (persistent HTTP cache poisoned automation runs)
**Найден:** P2-wpt S4/S5, 2026-07-17, re-running `tests/wpt/run_smoke.py` after fixing [BUG-291](BUG-291-FIXED.md)

## Симптом

With BUG-291 fixed, `testharness.js`'s results renderer no longer throws, and a
standalone BiDi probe (spawn `lumen --bidi-port`, serve `tests/wpt/` with a plain
`http.server`, navigate + poll `window.__lumen_wpt_results`) sees results in
~0.4s. But the full `wptrunner` harness (`tests/wpt/run_smoke.py`, its own
`wptserve` + `LumenTestharnessExecutor`) against the *same* page timed out:

```
wptrunner.executors.base.ExecutorException: ('TIMEOUT', 'Timed out waiting for
testharnessreport.js results: http://127.0.0.1:8000/dom/nodes/Element-hasAttribute.html')
```

## Root cause — two compounding faults

### 1. wptrunner served the wrong `testharnessreport.js` (wrong file)

`TestEnvironment.get_routes` (`tools/wptrunner/wptrunner/environment.py`) *always*
registers a static route for `/resources/testharnessreport.js`, defaulting to
wptrunner's own generic `executors/message-queue.js` + `testharnessreport.js` (the
postMessage / testdriver-message-queue reporter that sets
`window.__wptrunner_message_queue`) unless the product's `env_options()` returns a
`"testharnessreport"` override. That default silently shadowed Lumen's own
`tests/wpt/resources/testharnessreport.js` (the one that stashes results on
`window.__lumen_wpt_results`, which `LumenTestharnessExecutor` polls). So under the
full harness the served report.js set the wrong global and the executor waited
forever. A plain `http.server` serving `tests/wpt/` from disk has no such route —
it serves Lumen's real file — which is why the direct probe worked and the harness
did not.

### 2. Lumen's persistent disk HTTP cache replayed the wrong file across runs (stale cache)

This is what made fault #1 *survive* the fix and masked the whole diagnosis.
`wptserve` serves static routes with `Cache-Control: max-age=3600`. Lumen's
`DiskHttpCache` (SQLite at `<exe_dir>/data/cache/http_cache.db`, survives restarts)
honored it and cached the response keyed by URL. Because `wptserve` always binds
the **fixed** ports 8000/8001, the cache key
`http://127.0.0.1:8000/resources/testharnessreport.js` is identical every run. So
the very first `run_smoke.py` (before fault #1 was fixed) cached the *wrong*
default report.js for an hour, and **every subsequent run — including after the
`env_options` fix — served that stale wrong file from disk**, never re-fetching the
corrected one. Dumping the cache DB confirmed it: one entry for port 8000,
3048-byte body containing `__wptrunner_message_queue` (the wrong reporter), cached
at the timestamp of the first pre-fix run.

This also explains the earlier mis-diagnosis ("report.js fetched in full but not
executed"): `curl` fetched the *correct* file from `wptserve` (env_options fix
live), but *Lumen* served the *cached wrong* file — which executes fine, it just
sets `__wptrunner_message_queue` instead of `__lumen_wpt_results`. The temporary
`__bug295_*` instrumentation markers lived only in the on-disk correct file, so
they never appeared, which read as "not executed". The connection-pool / framing
hypotheses in the prior draft of this file were red herrings — every framing
variant reproduced against a plain server works; the only real variable was the
stale cache.

## Fix

1. **`tools/wptrunner/wptrunner/browsers/lumen.py::env_options`** now returns
   `"testharnessreport": [<abs path to tests/wpt/resources/testharnessreport.js>]`,
   so `get_routes` registers Lumen's own report script instead of wptrunner's
   generic default.
2. **`crates/shell/src/main.rs`** — automation sessions (`--bidi-port`,
   `--mcp-live-port`, `--mcp`, `--mcp-port`) now set `no_persistent_state = true`,
   selecting the in-memory HTTP cache instead of the on-disk `DiskHttpCache`. Each
   automation process starts with an empty cache → deterministic, no cross-run
   poisoning on the fixed automation-server ports. This is decided **before** the
   `config::init_global` at startup, because the profile `OnceLock` is set-once (a
   later `init_global` is a no-op — the same latent reason the CLI `--proxy`/`--tor`
   overrides sit right at that call site); the automation flags are detected with a
   raw `std::env::args()` scan there rather than the later `extract_*` parsers.

## Verification

`tests/wpt/run_smoke.py` (dev-release, default V8 backend, disk cache cleared once)
now produces a genuine result instead of a timeout:

```
TEST_END: Test OK. Subtests passed 1/2. Unexpected 1
FAIL hasAttribute should check for attribute presence, irrespective of namespace
     - el.setAttributeNS is not a function
Ran 3 checks (2 subtests, 1 tests)
```

The harness genuinely runs and reports per-subtest PASS/FAIL — one subtest PASSes,
one FAILs on a real engine gap ([BUG-309](BUG-309-OPEN.md), `setAttributeNS`
unimplemented). Re-running **without** clearing the cache reproduces the identical
genuine result (previously the second run always timed out), and the disk cache DB
stays empty during automation runs — confirming fix #2 engages. This is exactly the
"deliberately-failing assertion is observed as FAIL" the `docs/tasks/p2-wpt-integration.md`
checkbox asked for.

`run_smoke.py` still exits non-zero, correctly: the `setAttributeNS` FAIL has no
`.ini` expectation yet (committing curated expectations is a separate S5/S6 task),
so it counts as an "unexpected" result — which is the harness working, not a bug.

## Repro (historical)

1. Build `lumen.exe` (`dev-release`) with the BUG-291 fix but WITHOUT this bug's two
   fixes.
2. `pip install -r tests/wpt/requirements.txt` in a venv.
3. `LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py` — TIMEOUT; and
   it keeps timing out on every subsequent run even after applying only the
   `env_options` fix, until `<exe_dir>/data/cache/http_cache.db` is deleted.

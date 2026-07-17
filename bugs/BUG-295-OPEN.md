# BUG-295 — `tests/wpt/run_smoke.py` still times out after BUG-291 fix, despite the same page reaching a genuine result over a direct BiDi probe

**Статус:** OPEN
**Компонент:** unknown — either `tools/wptrunner`'s `wptserve`/executor overhead, or a Lumen HTTP/JS-runtime slowdown specific to the full harness's serving setup
**Найден:** P2-wpt S4/S5, 2026-07-17, re-running `tests/wpt/run_smoke.py` after fixing [BUG-291](BUG-291-FIXED.md)

## Симптом

With BUG-291 fixed (`Element.querySelector(All)` now scoped to descendants, plus
the `insertAdjacentText`/`insertAdjacentElement` methods it exposed as missing —
see BUG-291's closing note), `tests/wpt/resources/testharness.js`'s results
renderer (`Output.show_results`) no longer throws. Verified two ways:

1. A Rust-level reproduction of the exact `Output.show_results`/`get_asserts_output`
   code path (`crates/js/src/v8_runtime.rs`,
   `bug291_testharness_results_table_pattern_does_not_throw`) completes cleanly.
2. A standalone BiDi probe (spawn `lumen --bidi-port`, serve `tests/wpt/` with a
   plain `http.server`, navigate to `/dom/nodes/Element-hasAttribute.html`, poll
   `window.__lumen_wpt_results`) sees `readyState: "complete"` and
   `results: true` within ~1-2s of the scripts loading — no timeout, no thrown
   exception.

Despite that, `tests/wpt/run_smoke.py` (the full `wptrunner` harness — its own
`wptserve` instance, `LumenTestharnessExecutor`/`LumenBidiProtocol`, `wpt`'s
multiprocess runner) against the **same** test page still hits:

```
wptrunner.executors.base.ExecutorException: ('TIMEOUT', 'Timed out waiting for
testharnessreport.js results: http://127.0.0.1:8000/dom/nodes/Element-hasAttribute.html')
```

`TEST_START` to `TEST_END` elapsed ~15s (the default wptrunner testharness
timeout + `extra_timeout`) with no earlier signal.

## Что уже исключено

- Not BUG-291's crash: the standalone probe proves `Output.show_results`
  completes and does not throw against the real vendored `testharness.js`/
  `testharnessreport.js` pair over a real BiDi session against a really-spawned
  `lumen.exe` (dev-release, default V8 backend).
- Not a missing DOM API on this exact code path: `insertAdjacentText`/
  `insertAdjacentElement` (BUG-291's companion finding) are implemented.
- Tried `--timeout-multiplier` to rule out "just needs more time" — the repro
  attempt via a custom multiprocessing-unsafe driver script crashed on Windows
  (`multiprocessing` re-import guard), not concluded either way. Worth retrying
  properly (`if __name__ == "__main__":` guard) before deeper investigation.

## Hypothesis (unconfirmed)

`wptrunner`'s own `wptserve` (two instances, ports 8000/8001) may differ from a
plain `http.server.SimpleHTTPRequestHandler` in ways that matter to Lumen's HTTP
client (response framing/headers/encoding — the same general class as
[BUG-278](BUG-278-FIXED.md), close-delimited responses), and/or the multiprocess
executor adds enough latency that the real exchange doesn't fit in the default
timeout window even though it completes quickly in isolation. Not diagnosed —
needs the same live BiDi-bisection technique the BUG-278/279/280/291 chain used,
this time pointed at the full harness's actual request/response traffic (e.g.
compare `wptserve`'s response headers/framing against a plain `http.server` for
the same file).

## Repro

1. Build `lumen.exe` (`dev-release`, default V8 backend) with BUG-291's fix applied.
2. `pip install -r tests/wpt/requirements.txt` in a venv.
3. `LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py` — still times out.
4. Compare against the direct-probe path (no `wptserve`, no `wptrunner` executor)
   to see the same page succeed quickly — script not committed, see this bug's
   history for the pattern (spawn `lumen --bidi-port`, serve `tests/wpt/` with
   `http.server`, `BidiSession.bidi_only`, navigate + poll
   `window.__lumen_wpt_results`, handling `UnknownErrorException` for the
   transient "JS context not available" case exactly like
   `executorlumen.py::_run_testharness` does).

## Что нужно для закрытия

Diagnose why the full `wptrunner` harness path is slower/different than the
direct-probe path for the identical page and identical Lumen binary — likely
either a `wptserve`-specific serving quirk Lumen's HTTP client mishandles, or a
genuine performance gap in the full harness's request/response round-trip. Once
root-caused and fixed, `docs/tasks/p2-wpt-integration.md:328`'s "a deliberately-
failing assertion is observed as FAIL" checkbox becomes reachable.

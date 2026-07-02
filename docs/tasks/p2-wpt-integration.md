# P2-wpt — Web Platform Tests integration

> **SPECULATIVE — not tracked in `ROADMAP.md`.** Forward design brief; verify assumptions
> against code and add a ROADMAP.md row before starting.

**Developer:** P1
**Branch:** `p1-p2-wpt`
**Size:** XL
**Crates:** `lumen-driver` (harness), `lumen-shell` (headless entry), `lumen-js` (testharness support)

## Goal

Integrate a curated subset of the [Web Platform Tests](https://github.com/web-platform-tests/wpt)
suite as an automated conformance harness for Lumen. A WPT test is an HTML file that loads
`testharness.js` + `testharnessreport.js`, runs `test()`/`promise_test()` assertions, and reports
per-assertion PASS/FAIL/TIMEOUT/ERROR through JS callbacks. Lumen must vendor a small subset, drive
each test headless through the real engine + QuickJS, capture the harness results out of the JS
runtime, compare them against a committed expectations baseline (like `graphic_tests`), and print a
pass/fail summary. This is greenfield infrastructure — the engine already executes page scripts and
exposes a console drain, but there is no result-serialization shim, no runner, and no baseline.

## Prerequisites / scope decisions

- **Start tiny.** Pick ~10–20 *synchronous, JS-only* tests from `dom/` and `html/dom/`
  (e.g. `Document-createElement`, `Node-childNodes`, basic `querySelector`). Avoid anything needing
  fetch, workers, reftests, iframes, or visual comparison in the first phase. Async
  (`promise_test`, `async_test`) comes in phase 2 once the timer/microtask drive is proven.
- **No live network.** Vendor the chosen subset + `testharness.js` into the repo
  (`tests/wpt/` is already reserved in `docs/plan/architecture.md:155`). Do **not** add a runtime
  dependency on cloning github.com/web-platform-tests/wpt; a pinned vendored snapshot is the
  source of truth, mirroring how `graphic_tests/` Edge baselines are committed.
- **Reftests are out of scope** for this task — they need pixel comparison and belong with the
  existing `graphic_tests` / `snapshot_vs_edge` machinery, not the testharness path.
- **Per-spec discipline (`docs/plan/testing.md:75`):** the documented v1.0 target is "WPT subset —
  DOM, CSS, fetch, 60% pass". This task establishes the harness and a first DOM slice; raising the
  pass rate is follow-up work, not part of DoD here.
- **Custom `testharnessreport.js`.** WPT's upstream report file talks to a results server over
  postMessage; we replace it with a Lumen shim that serializes results to a global JS value the
  runner can read back. This file is ours, committed alongside the vendored harness.

## Current state

What exists today (real file:line):

- **Driver harness.** `lumen-driver` is the in-process programmatic interface to the engine —
  `crates/driver/src/lib.rs:62` defines the `BrowserSession` trait with `navigate`, `eval`,
  `console_log`, `wait`, `query`, etc. Integration tests aggregate into one binary via
  `crates/driver/tests/all.rs:4` → `crates/driver/tests/cases/mod.rs` (~70 submodules,
  one test-binary to avoid 64 link steps, see the header comment). This is the natural home for the
  WPT runner: add a `wpt.rs` case module + a driver-side runner type.
- **Two session impls.** `InProcessSession` (`crates/driver/src/session.rs`) runs the engine
  headless **without** a JS runtime — its `eval` is a deliberate stub returning `Err`
  (`crates/driver/src/session.rs:526`), and `click`/`type_text` are no-ops pending a persistent JS
  runtime. `GpuSession` (`crates/driver/src/gpu_session.rs:60`) is the JS-capable path:
  `RenderedPage.js_navigate` (`gpu_session.rs:37`) shows JS already runs during load.
- **The real JS runtime lives in the shell.** `crates/shell/src/main.rs` wraps
  `lumen_js::QuickJsRuntime` (field at `crates/shell/src/main.rs:2077`) behind a `JsHandle` trait
  with `eval_js` (`main.rs:2084` calls `self.rt.eval(script)`), `tick_timers`
  (`main.rs:2097` → `_lumen_tick_timers()`), and `take_console_messages`
  (`main.rs:2226` → `self.rt.take_console_messages()`).
- **JS engine eval API.** `crates/js/src/lib.rs:2075` — `QuickJsRuntime` implements
  `JsRuntime::eval(&self, script) -> JsResult<JsValue>`, plus `set_global`/`get_global`/
  `call_function` (`lib.rs:2094`–`2134`) and `eval_module` (`lib.rs:571`). Console output is
  buffered in `console_messages: Arc<Mutex<Vec<(u8,String)>>>` (`lib.rs:258`) and drained by
  `take_console_messages()` (`lib.rs:1773`). `get_global` is the clean channel for reading a
  serialized results object back out after a test runs.
- **DOM/JS surface testharness.js needs — mostly present.** `crates/js/src/dom.rs` provides
  `document.createElement`/`getElementById`/`querySelector`/`querySelectorAll`
  (`dom.rs:5269`–`5306`), `addEventListener` incl. `DOMContentLoaded` firing immediately when ready
  (`dom.rs:5306`), per-element `addEventListener` (`dom.rs:3524`, `dom.rs:4398`),
  `EventTarget.prototype.addEventListener` (`dom.rs:2708`), and `setTimeout`/`setInterval`
  (`dom.rs:6046`, timers ticked via `_lumen_tick_timers`). `JsValue` round-trips JSON-shaped
  objects/arrays (`crates/js/src/lib.rs:2368`–`2392`).
- **Headless entry points (shell).** `crates/shell/src/main.rs:7-11` documents the dump modes:
  `--dump-source`, `--dump-layout`, `--dump-display-list`, and `--screenshot <out.png> <src>`
  (`run_screenshot` at `main.rs:802`, full pipeline). `--ipc-server` (`main.rs` extract at
  `main.rs:1531`) is a long-lived TCP tab-command server; its protocol
  (`crates/ipc/src/lib.rs`) currently supports `NavigateTab` + `Screenshot` only — **no `Eval`
  command exists yet**.
- **Python runner pattern to mirror.** `graphic_tests/run.py` (header at top, uses `argparse`,
  `subprocess`, `json`; results to `graphic_tests/results/*.json` with `latest.json`) is the
  established Lumen test-runner shape: drive the binary per test, diff against a committed baseline,
  emit one line per test + a JSON record. The WPT runner should follow this convention.

What is **missing** (must be built):

1. No vendored WPT tests or `testharness.js` in the repo (grep for `wpt`/`testharness`/`web-platform`
   in `.rs`/`.py` returns only doc-plan mentions: `docs/plan/testing.md:9,25,75,85`,
   `docs/plan/phases.md:117`, `docs/plan/architecture.md:155`).
2. No `testharnessreport.js` shim that serializes results into a JS global.
3. No way to drive the JS-capable engine over an arbitrary HTML file *and read a JS value back* from
   a non-IPC headless invocation — `--screenshot` runs JS but only emits a PNG; IPC has no `Eval`.
4. No expectations/baseline file and no comparison logic.

## Architecture

How WPT runs upstream:

```
test.html
  ├─ <script src="/resources/testharness.js">      ← defines test(), async_test(), assert_*, add_result_callback, add_completion_callback
  ├─ <script src="/resources/testharnessreport.js"> ← OUR SHIM: subscribes to callbacks, serializes results
  └─ <script> test(() => { assert_equals(...); }, "name"); </script>
```

`testharness.js` accumulates `Test` objects and, on completion, invokes registered
`add_completion_callback(tests, harness_status)` listeners. The standard report file ships those to a
remote server; we instead install a completion callback that writes a JSON-serializable array of
`{name, status, message}` into a known JS global (e.g. `window.__lumen_wpt_results`), where
`status` ∈ {PASS=0, FAIL=1, TIMEOUT=2, NOTRUN=3}.

Wiring into Lumen — four pieces:

1. **Vendoring (`tests/wpt/`).** Commit a pinned snapshot:
   - `tests/wpt/resources/testharness.js` — upstream, unmodified, pinned to a recorded commit hash
     (record the hash in `tests/wpt/VENDOR.md`).
   - `tests/wpt/resources/testharnessreport.js` — **our shim** (below).
   - `tests/wpt/<spec>/<test>.html` — the curated subset.
   Rewrite the upstream `/resources/...` absolute script URLs to repo-relative `file://` paths at
   vendor time (a small import-fixup is acceptable since these are static fixtures, not the test
   logic — this does not violate the "never rewrite test pages" rule, which is about not weakening
   assertions; the assertions stay byte-identical).

2. **`testharnessreport.js` shim.** Minimal:
   ```js
   add_completion_callback(function (tests, status) {
     globalThis.__lumen_wpt_results = JSON.stringify({
       harness: { status: status.status, message: status.message },
       tests: tests.map(t => ({ name: t.name, status: t.status, message: t.message })),
     });
   });
   ```
   The runner reads `__lumen_wpt_results` back via `JsRuntime::get_global`
   (`crates/js/src/lib.rs:2101`) after driving the page to completion.

3. **Runner (Rust, in `lumen-driver`).** Add `crates/driver/tests/cases/wpt.rs` plus a reusable
   `WptRunner` in `crates/driver/src/` that, per test file:
   - constructs the JS-capable session,
   - navigates to the `file://` test URL,
   - drives `tick_timers` + microtask pump in a bounded loop (cap iterations / wall-clock for
     TIMEOUT — reuse the polling shape of `BrowserSession::wait` at `session.rs:495`) until
     `__lumen_wpt_results` is populated or the timeout fires,
   - reads + parses the results global,
   - returns `Vec<{name, status, message}>`.

   **Critical dependency:** the runner needs the JS-capable path. `InProcessSession::eval` is a stub
   (`session.rs:526`); the working JS+`eval`+`tick_timers`+`get_global` loop lives in the shell
   (`main.rs:2077`–2226). Decide the drive channel before coding (see Steps 1):
   - **(a)** Expose the shell's JS-capable engine through `GpuSession`/a new driver session so the
     Rust runner drives it in-process (preferred — no subprocess, results via `get_global`); **or**
   - **(b)** Add a headless shell entry point — either a new `--dump-wpt <test.html>` mode next to
     `run_screenshot` (`main.rs:802`) that runs the full pipeline, drives timers to quiescence, and
     prints `__lumen_wpt_results` to stdout, or an `Eval { tab_id, script }` IPC command
     (`crates/ipc/src/lib.rs`) — then a Python runner subprocess-drives the binary like
     `graphic_tests/run.py`.

   Path (a) keeps everything in `cargo test`; path (b) matches the existing Python-runner ergonomics
   but requires a new shell flag/IPC verb. Pick one and document it in the branch's first commit.

4. **Expectations baseline.** Commit `tests/wpt/expectations.json` mapping
   `test-file → { subtest-name → expected-status }`. The runner diffs actual vs expected: an
   unexpected FAIL is a regression (fail the run); a newly-passing subtest is a ratchet candidate
   (update the baseline). This mirrors `KNOWN_DEBTORS` in `graphic_tests/run.py` and lets the suite
   gate CI even before 100% pass. Results land in `tests/wpt/results/*.json` (+ `latest.json`),
   gitignoring any HTML report as `graphic_tests` does.

## Entry points

- `crates/driver/src/lib.rs:62` — `BrowserSession` trait; `eval` (`:155`), `console_log` (`:127`),
  `wait` (`:151`), `query` (`:159`) are the verbs the runner composes.
- `crates/driver/src/session.rs:526` — `InProcessSession::eval` stub (returns Err): the gap that
  forces choosing the JS-capable drive channel.
- `crates/driver/src/gpu_session.rs:60` — `GpuSession`, the JS-capable session
  (`RenderedPage.js_navigate` at `:37` proves JS runs during load).
- `crates/driver/tests/cases/mod.rs` — register the new `wpt` submodule here.
- `crates/js/src/lib.rs:2075` — `QuickJsRuntime::eval`; `:2101` `get_global` (read results back);
  `:1773` `take_console_messages`; `:571` `eval_module`.
- `crates/js/src/dom.rs:5269` — `document` shim (createElement/querySelector/getElementById);
  `:5306` DOMContentLoaded fast-path; `:6046` `setTimeout`.
- `crates/shell/src/main.rs:2077` — `JsHandle` over `QuickJsRuntime`; `:2084` `eval_js`,
  `:2097` `tick_timers`, `:2226` `take_console_messages`.
- `crates/shell/src/main.rs:802` — `run_screenshot` / full headless pipeline (model for a new
  `--dump-wpt` mode if path (b) is chosen).
- `crates/ipc/src/lib.rs` — IPC request/response enums (`NavigateTab`/`Screenshot`); add `Eval`
  here if path (b)+IPC is chosen.
- `graphic_tests/run.py` — Python runner pattern (argparse/subprocess/json, `results/latest.json`)
  to mirror for the WPT runner.
- `docs/plan/architecture.md:155` — reserved `tests/wpt/` location.
- `docs/plan/testing.md:75` — documented WPT subset scope + 60% v1.0 target.

## Steps

Phased; ship the smallest end-to-end slice first.

1. **Decide the drive channel.** Spike both options minimally and pick (a) in-process JS-capable
   driver session vs (b) headless shell `--dump-wpt` / IPC `Eval`. Record the decision + rationale
   in the first commit and (if architecturally significant) an ADR under `docs/decisions/`.
2. **Vendor one test.** Add `tests/wpt/resources/testharness.js` (pinned, hash in
   `tests/wpt/VENDOR.md`), the shim `tests/wpt/resources/testharnessreport.js`, and a single trivial
   synchronous DOM test (e.g. `tests/wpt/dom/nodes/Document-createElement.html`) with its
   `/resources/...` script URLs rewritten to repo-relative `file://`.
3. **Manual end-to-end.** Drive that one file through the chosen channel; confirm
   `__lumen_wpt_results` is populated and parses. Fix DOM/JS surface gaps surfaced by
   `testharness.js` itself (it exercises a lot of the API on load) — file any engine gap as a
   `BUG-NNN` per the bug-ownership rule rather than patching the test.
4. **`WptRunner` + first case.** Implement the runner type in `crates/driver/src/`, add
   `crates/driver/tests/cases/wpt.rs`, register it in `mod.rs`. Assert the single test passes.
5. **Expectations baseline.** Add `tests/wpt/expectations.json`; make the runner diff actual vs
   expected and classify regression / new-pass / known-fail.
6. **Grow the synchronous subset** to ~10–20 DOM tests. Commit `expectations.json` with whatever
   currently passes; do **not** weaken any test to force a pass.
7. **Async phase (follow-up, may be its own task).** Once the timer/microtask drive loop is proven,
   admit `async_test`/`promise_test` and a handful of `html/dom` async tests; enforce per-test
   TIMEOUT via the bounded drive loop.
8. **Optional Python wrapper.** If path (b), add `tests/wpt/run.py` mirroring `graphic_tests/run.py`
   (argparse `--only`/`--continue-on-fail`/`--recheck`, `results/*.json` + `latest.json`).
9. **Docs.** Update `docs/plan/testing.md` (mark the WPT harness as existing) and add a short
   `tests/wpt/README.md` (how to add a test, how to re-vendor, how to ratchet expectations).

## Tests / verification

- **Unit:** the shim's serialization shape round-trips — a hand-written results object set via
  `set_global` and read via `get_global` (`crates/js/src/lib.rs:2094`/`:2101`) parses to the
  expected `{harness, tests}` struct.
- **End-to-end (the real proof):** `cargo test -p lumen-driver` (path a) or the runner subprocess
  (path b) executes the vendored DOM subset and reports each subtest's PASS/FAIL matching
  `tests/wpt/expectations.json`. A deliberately-broken assertion in a scratch test must surface as a
  FAIL (proves the harness actually observes assertion failures, not just "ran without throwing").
- **Regression gate:** flipping an expected PASS to FAIL in the engine causes the run to fail with a
  named subtest, not a silent pass.
- **No network:** the suite runs fully offline from vendored files (no clone, no fetch to
  github.com/web-platform-tests/wpt at test time).
- `cargo clippy -p lumen-driver --all-targets -- -D warnings` clean.

## Definition of done

- [ ] Drive-channel decision documented (first commit + ADR if significant).
- [ ] `tests/wpt/resources/testharness.js` vendored + pinned (hash in `tests/wpt/VENDOR.md`).
- [ ] `tests/wpt/resources/testharnessreport.js` shim serializes results into a JS global.
- [ ] `WptRunner` in `lumen-driver` drives a `file://` WPT test to completion (timer/microtask loop
      with TIMEOUT) and reads results back via `get_global`.
- [ ] `crates/driver/tests/cases/wpt.rs` registered in `mod.rs`; ~10–20 synchronous DOM tests
      execute and report per-subtest status.
- [ ] `tests/wpt/expectations.json` committed; runner classifies regression / new-pass / known-fail.
- [ ] A deliberately-failing assertion is observed as FAIL (harness genuinely checks assertions).
- [ ] Suite runs fully offline.
- [ ] `tests/wpt/README.md` (add test / re-vendor / ratchet); `docs/plan/testing.md` updated.
- [ ] `cargo clippy -p lumen-driver --all-targets -- -D warnings` and `cargo test -p lumen-driver`
      pass.
- [ ] Any engine/DOM gaps found while running the harness filed as `BUG-NNN` (no test weakened to
      pass).

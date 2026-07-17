# BUG-301 — `tests/wpt/run_smoke.py` (wptrunner + wptserve + `LumenTestharnessExecutor`) still times out after BUG-298/299/300, cause not identified

**Статус:** OPEN
**Компонент:** unclear — js (`testharness.js` completion path) and/or bidi-server/wptrunner integration (`tools/wptrunner/wptrunner/executors/executorlumen.py`)
**Найден:** P2-wpt S4, 2026-07-17, after fixing [BUG-298](BUG-298-FIXED.md)/[BUG-299](BUG-299-FIXED.md)/[BUG-300](BUG-300-FIXED.md)

## Симптом

`LUMEN_PROFILE=dev-release <venv>/python tests/wpt/run_smoke.py` still reports `TIMEOUT` for `/dom/nodes/Element-hasAttribute.html`, reproducibly, even after fixing all three engine/shell bugs found while diagnosing this task's original blocker (the "JS-context-install race" described in the pre-fix `CLAUDE.md` gotcha, now removed — see BUG-298/299/300 for what that symptom actually was).

Diagnostic instrumentation added temporarily to `executorlumen.py`'s `_run_testharness` (reverted before committing — not part of this bug's fix) showed, for the whole ~15s poll window:

- `navigate()` returns in a realistic ~0.25–0.6s (not the ~10ms BUG-300 previously caused).
- `document.readyState` reads `"complete"` from the very first poll onward.
- `typeof window.test === "function"`, `typeof window.assert_true === "function"` — `testharness.js` fully evaluated and exposed its public API.
- `document.querySelectorAll('script').length === 3` and `document.title` matches the test's `<title>` — the whole document parsed correctly.
- `document.getElementById('t')` (the test's own `<span>`) exists.
- `document.getElementById('log')` is **never** present, at any polled point — this element is `Output.prototype.resolve_log()`'s very first side effect (called from `show_status`/`show_results`), so its total absence for the entire window means the harness's `test_state`/`result`/`completion` callback chain never fires *at all* in this configuration, not merely that one specific callback throws (contrast with BUG-298/299, which produced a stale-but-existing `#log`).

## Что уже исключено

- **Not BUG-298/299/300** — those are independently confirmed fixed (BUG-298/299 via direct `document.createElementNS`/`querySelector`/`insertAdjacentText` bisection against a live window; BUG-300 via the corrected `navigate()` timing above).
- **Not the JS-context-install race** the old `CLAUDE.md` gotcha described — no `"JS context not available"` error was observed anywhere in the 15s window; `script.evaluate` succeeds on every poll.
- **Not reproducible via a manual BiDi driver** hitting the identical dev-release binary and the identical vendored `tests/wpt/resources/testharness.js` + `testharnessreport.js` + `dom/nodes/Element-hasAttribute.html`, served over a plain `http.server.BaseHTTPRequestHandler` instead of the vendored `wptserve`: in that configuration the harness completes correctly (`window.__lumen_wpt_results` becomes a populated JSON string within ~2s of navigation, `#log` gets created, both `test()` calls report results — one genuinely FAILs on the separately-known-missing `Element.setAttributeNS`, one PASSes).
- **Not test-content-dependent in the trivial sense** — the same exact vendored test file behaves differently only depending on whether it's served/driven through `wptrunner`+`wptserve` vs. a hand-rolled BiDi client + plain HTTP server.

## Что нужно для закрытия

Isolate which half of the difference (the `wptserve` HTTP responses vs. the `wptrunner`/`LumenTestharnessExecutor` BiDi driving sequence) actually matters — e.g. drive the manual BiDi probe against a real `wptserve` instance (not a plain server) with the exact same request, or point `wptrunner` at a substitute non-`wptserve` HTTP server for this one test, to bisect. Candidates not yet ruled out: response headers `wptserve` adds that a plain `http.server` doesn't (caching, CORS, etc.) subtly changing page lifecycle; extra BiDi session/event-subscription setup `BidiSession.start()` performs that a bare `send()`-based probe doesn't; or a genuine timing-sensitive interaction between the shell's automation-command channel and the JS engine thread that only manifests under `wptrunner`'s specific request cadence.

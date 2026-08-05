# BUG-659: `generate_test_report` testdriver action never implemented by Lumen's minimal WPT executor — masked all of `reporting`'s own-API signal

**Статус:** FIXED 2026-08-05
**Дата:** 2026-08-05
**Компонент:** tooling (`tools/wptrunner/wptrunner/executors/executorlumen.py`)
**Найден:** WPT-VENDOR-reporting (`ROADMAP.md:508`)

## Механизм

`executorlumen.py::_handle_action` (Lumen's own BiDi-only test executor glue,
not upstream-vendored code — see the module's own docstring) only ever
executed the `click` testdriver action; every other action name raised
`ActionError(f"action {action!r} not implemented by Lumen's minimal WPT
executor")`, which the page observes as a rejected `test_driver.<method>()`
promise. `reporting/`'s own tests are built almost entirely around
`test_driver.generate_test_report(message)` — the only way WPT exercises
`ReportingObserver` without a real browser-internal violation (CSP,
deprecation, intervention, crash) — so every test calling it either failed
its `promise_test` immediately (`bufferSize.html`, `disconnect.html`) or hung
until the harness timeout in an `async_test` that never got a callback fired
(`generateTestReport.html`, `order.html`, `nestedReport.html`).

## Симптом

Before the fix: `reporting` category run — **3/25 harness OK, 0/15
subtests passed**. All five of the above files failed/timed out with either

```
Unhandled rejection with value: "failure: action 'generate_test_report' not implemented by Lumen's minimal WPT executor"
```

or a bare harness-level `TIMEOUT` (the `async_test` variants, which never
catch the rejection — the callback that would call `test.done()` simply
never fires).

## Фикс

Lumen's Reporting API shim already exposes report delivery as a plain
page-visible JS global — `crates/js/src/reporting_api.rs::_lumen_deliver_report(type,
url, body_json)` — so no new engine binding or BiDi surface was needed. Added
an `_action_generate_test_report` handler to `executorlumen.py` that resolves
`params["context"]` (defaulting to the top-level context) and evaluates
`_lumen_deliver_report('test', location.href, JSON.stringify({message:
<message>}))` in it via the existing `script.evaluate` plumbing, wired into
`_handle_action`'s dispatch alongside `click`. `report.type` is hardcoded to
`"test"` and the body shape to `{message}` per W3C Reporting API §8.2
(`TestReportBody`) — confirmed against every test file's own assertions
(`reports[0].type === "test"`, `reports[0].body.message === "..."`).

## Результат

Re-running the same category after the fix: **5/25 harness OK, 5/15
subtests passed** — `bufferSize.html`, `disconnect.html`,
`generateTestReport.html`, `order.html`, `nestedReport.html` all pass
cleanly now (verified: no more FAIL/TIMEOUT lines naming their subtests in
the unexpected-results log). Remaining `reporting` signal after this fix is
unrelated: 17 TIMEOUT on `.https.sub.html` ids (TLS `UnknownIssuer` gap,
already documented across many categories), 3 ERROR (BUG-380's browsing-context
result-poisoning — a `.https.` TIMEOUT's stale result bleeding into the next
test), and `crashReport-test.html` (reconfirmation of
[BUG-480](BUG-480-OPEN.md), `<iframe>` has no separate browsing context —
`crashReport` itself is an unrelated, entirely-unimplemented Crash Reporting
API and not claimed anywhere in `CAPABILITIES.md`).

## Масштаб находки

Not specific to `reporting` — any category whose tests rely on
`test_driver.generate_test_report()` to synthesize a report for
`ReportingObserver` (e.g. future Crash Reporting / Deprecation Reporting /
Intervention Reporting WPT categories, if their tests use the same testdriver
primitive rather than a real violation) benefits from this fix retroactively.

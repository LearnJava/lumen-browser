# BUG-654: `resources/testdriver-actions.js` never vendored — `test_driver.Actions` undefined, masking real per-test findings

**Статус:** FIXED 2026-08-05
**Дата:** 2026-08-05
**Компонент:** tooling (`tests/wpt/resources/`, `tests/wpt/VENDOR.md`)
**Найден:** WPT-VENDOR-pointerevents (`ROADMAP.md`)

## Механизм

Every vendored test's `<script src="/resources/testdriver-actions.js">` tag
resolves through wptserve's normal doc-root routing to
`tests/wpt/resources/testdriver-actions.js` (unlike the special-cased
`/resources/testdriver.js` route documented in `CLAUDE.md`, which is
hardcoded in `environment.py` to the repo-root `resources/testdriver.js`).
That file — upstream's `resources/testdriver-actions.js`, 607 lines defining
the `test_driver.Actions` builder class used by nearly every WPT test that
synthesizes pointer/touch/keyboard input sequences — was never vendored into
`tests/wpt/resources/`; only `testharness.js`, `testharnessreport.js` and
`check-layout-th.js` were (per `VENDOR.md`'s table). Same gap for the sibling
`resources/testdriver-vendor.js` (a deliberately empty upstream
customization hook, not itself a defect, but also missing from the vendored
tree).

## Симптом

Any test constructing `new test_driver.Actions()` fails synchronously with:

```
TypeError: test_driver.Actions is not a constructor
```

— a network-404-driven test-infra artifact, not a reflection of any real
engine behavior. On `pointerevents` this hit dozens of subtests (`Predicted
list in pointer-capture events`, `Wheel-scroll over pointer-events: none
scroller skips that scroller`, every `setPointerCapture`/boundary-event test
using drag simulation, etc.) — masking whatever the tests were actually
meant to probe.

## Масштаб находки

Not specific to `pointerevents` — every previously-vendored/run category
whose tests use `test_driver.Actions` (drag gestures, multi-touch, pointer
capture, wheel simulation) hit this same masking artifact and reported it as
an undifferentiated `TypeError` rather than the real underlying cause.
Retroactive re-audit of those categories is out of scope here; noted for
future WPT-VENDOR sessions in
[[reference_wpt_run_report_invocation_recipe]] equivalent memory.

## Фикс

Vendored both files verbatim from the pinned upstream commit (`35be3b44`,
same clone used for all `WPT-VENDOR-*` tasks) into `tests/wpt/resources/`:
`testdriver-actions.js` (607 lines, real content) and `testdriver-vendor.js`
(0 bytes, intentionally empty per upstream). `VENDOR.md`'s resource table
updated accordingly.

## Побочный эффект — не баг, ожидаемое поведение

With the constructor now real, `Actions.send()` correctly reaches
`executorlumen.py::_handle_action`, which — per `WPT-RUN-2`'s documented
scope decision (`ROADMAP.md`) — implements only the `click` action; every
other action (`pointerMove`/`pointerDown`/`keyDown`/etc., the building
blocks `action_sequence` posts) rejects explicitly. Because rejection now
happens deep inside each test's own promise chain (often after the test has
already started waiting on a follow-up DOM event that the undelivered
gesture would have triggered), affected tests now pay the harness's full
timeout instead of failing instantly — a real cost increase for
`Actions`-heavy categories, but a *correct* one: it replaces a
vendoring-404 artifact with the true (already-scoped) executor limitation.
Net effect on `pointerevents`' pass rate was ~neutral (94/258 harness OK
post-fix vs 100/258 with the masking artifact still in place) — the fix
trades run time for signal accuracy, not for pass count.

## Дополнительная находка при разборе прогона

The now-unmasked failure text surfaced [BUG-622](BUG-622-OPEN.md)
(`document.defaultView` missing entirely) as the single largest failure
cluster in the corrected run — dozens of hits via
`testdriver-extra.js::get_context`'s `element.ownerDocument.defaultView`
check (thrown as `Error: Browsing context for element was detached`,
misleading text but same root cause) — strongly reconfirming that bug's own
prediction of "likely wider-impact given how common
`elem.ownerDocument.defaultView` is as a cross-window-safe idiom in test
helper libraries."

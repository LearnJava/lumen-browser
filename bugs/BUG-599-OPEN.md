# BUG-599: `Node.prototype.getRootNode()` missing entirely — breaks `get_selector_array` in the vendored wptrunner testdriver shim, silently masking/hanging every `test_driver_internal.*` action

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs`, `Node.prototype` shared-method block — same location as `hasChildNodes`, see [BUG-574](BUG-574-OPEN.md) which already predicted this exact sibling gap for `Node.prototype.contains()`)
**Найден:** P2, WPT-VENDOR-html-interaction, 2026-08-04

## Симптом

```
FAIL Tablist child gets implied tab role - promise_test: Unhandled rejection with value: object "TypeError: current.getRootNode is not a function"
```
(`focusgroup/tentative/ax-role-inference-children.html`, 21 subtests across
the file — every `test_driver.get_computed_role(element)` call)

Also observed as a silent **TIMEOUT** rather than a rejection on tests that
call `test_driver.send_keys`/`.click` without surfacing the rejection to the
harness directly: `focus-01.html`/`focus-02.html` (async keyboard-event
tests) hang for the full 10-15s timeout instead of failing fast — same root
cause, worse symptom, harder to attribute.

## Причина

`Node.prototype.getRootNode(options)` (DOM §4.4 — returns the node's
shadow-inclusive root, or `this` if unattached) is absent end to end:
`grep -rn "getRootNode" crates/js/src/` returns zero method-definition hits.
[BUG-574](BUG-574-OPEN.md), which found the sibling `Node.prototype.contains()`
gap, already flagged `getRootNode` (along with `compareDocumentPosition`/
`isSameNode`/`isEqualNode`) as "worth checking in the same fix pass" but
didn't verify it against a concrete failure; this run provides that
verification via a different call site than `contains()`'s.

The break is in `tools/wptrunner/wptrunner/testdriver-extra.js` (vendored,
unmodified — appended onto every `/resources/testdriver.js` response, see
the `CLAUDE.md` gotcha on that static route), inside the selector-builder
used by essentially every `test_driver_internal.*` action:

```js
// testdriver-extra.js:144, get_selector()
if (element.getRootNode() == element.ownerDocument) { ... }

// testdriver-extra.js:163, get_selector_array()
current = current.getRootNode().host;
```

`get_selector_array` is on the call path of `click`, `send_keys`,
`action_sequence`, `get_computed_role`, `get_computed_label`, and
`get_accessibility_properties_for_element` — i.e. nearly every
`test_driver_internal` action a WPT test can take, not just clicks (which
already fail via BUG-574/BUG-462's `contains()` gap on a *different* call
path, `resources/testdriver.js`'s own `getInViewCenterPoint`).

## Масштаб

Because both `contains()` and `getRootNode()` sit on independent, widely-used
paths inside the vendored (unpatchable) testdriver machinery, this pair is a
plausible root cause behind an unknown share of `testdriver`-flagged
TIMEOUTs/FAILs across every already-run WPT-VENDOR category that uses
`test_driver_internal.*` beyond plain `click()` — e.g. `send_keys`,
`get_computed_role`/`get_computed_label` (accessibility-flavored tests),
`action_sequence` (drag/pointer tests). Worth a targeted re-check once both
this bug and BUG-574/BUG-462 are fixed together, since several categories'
"testdriver SKIP wall" numbers were computed before either gap was known and
may have absorbed this as unexplained noise.

In this session's slice alone: 21 subtests directly show `current.getRootNode
is not a function`, and 2 of the 21 top-level TIMEOUTs (`focus-01.html`,
`focus-02.html`) are consistent with the same failure manifesting as a hang.

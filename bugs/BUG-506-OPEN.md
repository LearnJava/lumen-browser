# BUG-506: `<script src="../other-category/support/helper.js">` (cross-directory relative external script) never executes before dependent inline code under the wptrunner-driven pipeline

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** unclear — likely shell script-loading order (`crates/shell/src/main.rs`) or a
wptrunner/BiDi-navigation-specific timing gap; not root-caused to a single file/line this slice
**Найден:** WPT-RUN-3 срез 12 (`ROADMAP.md`) — массовый прогон `css/css-logical`

## Симптом

`animation-001.html`/`animation-002.html`/`animation-003.tentative.html`/`animation-004.html`
and `animations/logical-shorthand-relative-prioritization-by-number-of-components.tentative.html`
all include, in document order:

```html
<script src="/resources/testharness.js"></script>
<script src="/resources/testharnessreport.js"></script>
<script src="../css-animations/support/testcommon.js"></script>
<script>
'use strict';
test(t => {
  const div = addDiv(t);   // ReferenceError: addDiv is not defined
  ...
```

`../css-animations/support/testcommon.js` defines `addDiv`/`addStyle`
(`tests/wpt/css/css-animations/support/testcommon.js:125`/`139`) — a
helper shared across `css/css-animations` and (via this relative path)
`css/css-logical`. Every `test()` body in these 5 files throws
`ReferenceError: addDiv is not defined` (or `addStyle is not defined`),
i.e. the third external `<script src>` never ran (or its top-level
`function addDiv(...)` declaration never landed on the global scope)
before the fourth, inline `<script>` block executed — **reproducibly**,
confirmed by re-running `run_report.py --root css/css-logical --limit 1`
against just this one file: same `addDiv is not defined` failures every
time.

## Investigation — not a simple "helper never loads"

A manual `--mcp-live-port` probe of the exact same URL (served the same
way, `python -m http.server`), navigating and then — in a **separate**
`eval()` call issued after `navigate()` returned — querying
`typeof window.addDiv`, returns `"function"`:

```js
JSON.stringify({addDiv: typeof window.addDiv, addStyle: typeof window.addStyle,
                 test: typeof window.test, ready: document.readyState})
// → {"addDiv":"function","addStyle":"function","test":"function","ready":"complete"}
```

The shell's own stdout for that same manual load additionally shows the
three external scripts fetched out of network-arrival order (`testharness.js`,
then `testcommon.js`, then `testharnessreport.js` — NOT the order they were
requested in) but **executed** in the correct document order (`Загружен
скрипт: testharness.js` → `testharnessreport.js` → `testcommon.js`), which
is the spec-correct blocking-classic-script behaviour. So a plain
navigate-then-inspect probe shows nothing wrong.

Yet the actual wptrunner/BiDi test-execution pipeline (`tools/wptrunner`,
the same mechanism used for every WPT-RUN-3 slice) reproducibly observes
`addDiv is not defined` for these exact files, every run. The discrepancy
between "works under a simple navigate+eval probe" and "fails
deterministically under the real wptrunner harness" was not resolved this
slice — candidate mechanisms not yet ruled in/out:

- wptrunner's own executor may start polling/evaluating page state (or
  the page's own inline script may begin executing) before
  `browsingContext.navigate`'s readiness wait has actually let all three
  external scripts finish running, specifically when their relative URLs
  cross a directory boundary (`../css-animations/support/...` — every
  other helper script this track has exercised so far lives under
  `/resources/` or `css/support/`, both same-or-shallower-depth paths;
  this is the first slice to hit a `../`-crossing relative `<script src>`
  under wptrunner).
- The raw, non-normalized request URL observed in the shell log
  (`http://.../css/css-logical/../css-animations/support/testcommon.js`,
  `..` not collapsed) could interact with wptrunner's own `wptserve`
  routing/caching differently than with a plain `http.server`.

## Масштаб находки

5 files / 57 subtests this slice, all sharing the identical
`../css-animations/support/testcommon.js` dependency:
`animation-001.html` (24), `animation-002.html` (15),
`animation-003.tentative.html` (1), `animation-004.html` (16),
`animations/logical-shorthand-relative-prioritization-by-number-of-components.tentative.html`
(1). Not surveyed beyond `css/css-logical` this slice — the same helper
is referenced (unexercised so far) by `css/css-view-transitions`
(`pseudo-element-animations.html`/`-rerun.html`), which would be the next
place to confirm whether this is specific to `css-logical`'s particular
`../`-crossing path or a general "external script from a differently-
rooted relative path" gap.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-logical/` for all 5
files, `expected: FAIL` per subtest (harness itself completes `OK`, only
individual subtests fail).

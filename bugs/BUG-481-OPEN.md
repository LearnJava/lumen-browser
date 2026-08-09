# BUG-481: `window.visualViewport` not implemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

```
FAIL Element.scrollIntoView doesn't scroll a position:fixed element ...
  promise_test: Unhandled rejection with value: object
  "ReferenceError: visualViewport is not defined"
```

`grep -n "visualViewport" crates/js/src/dom.rs` — zero matches. The Visual
Viewport API (`window.visualViewport`, a `VisualViewport` with
`width`/`height`/`offsetLeft`/`offsetTop`/`scale`/`onresize`/`onscroll`) is
entirely absent.

## Масштаб находки

4 subtests, `scrollIntoView-fixed-outside-of-viewport.html`.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for the one
attributed file, `expected: FAIL` per the actual run.

## Срез 21 (`css/css-device-adapt`, 2026-08-03)

5 files, 5 subtests, same signature — `window.visualViewport` is `undefined`,
so `window.visualViewport.scale` throws `Cannot read properties of undefined
(reading 'scale')` before the assertion runs:
`viewport-clamp-initial-scale-to-max.tentative.html`,
`viewport-clamp-initial-scale-to-min.tentative.html`,
`viewport-user-scalable-no-clamp-to-max.tentative.html`,
`viewport-user-scalable-no-clamp-to-min.tentative.html`,
`viewport-user-scalable-no-wide-content.tentative.html` — all five test
`<meta name="viewport">` initial/min/max-scale clamping, which this engine
cannot expose without a `VisualViewport` object regardless of whether the
underlying zoom clamping itself is implemented. `.ini` under
`tests/wpt/metadata/css/css-device-adapt/` for all 5 files.

## WPT-VENDOR-visual-viewport (2026-08-09)

Category vendored and run whole (`run_report.py --all --root visual-viewport
--recursive`, 19 selected ids): **11/19 harness OK, 0/22 subtests**. Same
`window.visualViewport` absence, now observed as three distinct failure
shapes depending on where each file's first reference sits relative to
`test()`/`async_test().step()`: plain `FAIL` (`TypeError: Cannot read
properties of undefined`) when wrapped in a step, `TIMEOUT` or `ERROR: done()
was called without first defining any tests` when the reference is at the
file's top level or inside an unwrapped callback and the resulting unhandled
exception aborts the script before `done()` runs. No new bug number filed —
same root cause as the entries above.

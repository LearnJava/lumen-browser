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

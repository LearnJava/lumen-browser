# BUG-477: `document.elementFromPoint`/`elementsFromPoint` not implemented — no point-based hit-testing API on the JS side

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 4 (`ROADMAP.md`) — массовый прогон `css/cssom-view`

## Симптом

```
FAIL Element at (400, 100)
  document.elementFromPoint is not a function
FAIL elementsFromPoint should return all elements under a point
  document.elementsFromPoint is not a function
```

`grep -n "elementFromPoint\|elementsFromPoint" crates/js/src/dom.rs` — zero
matches. Both methods are absent from the shim entirely.

## Связано

`document.caretPositionFromPoint` (CSSOM View §5.1, `dom.rs:7396-7402`) *is*
defined but is a Phase-0 stub whose own comment says "no layout hit-testing
yet — returns body at offset 0" (`dom.rs:10513`) — consistent with the same
underlying gap: there is no point→node hit-testing primitive exposed to JS at
all, so every API built on it (`elementFromPoint`, `elementsFromPoint`,
`caretPositionFromPoint`, and by extension `document.caretRangeFromPoint`,
[BUG-474](BUG-474-OPEN.md)) is either missing or a stub.

## Масштаб находки

68 subtests (50× `elementFromPoint is not a function`, 18×
`elementsFromPoint is not a function`) plus harness-level TIMEOUT on
`elementFromPoint-001.html`, `elementsFromPoint-iframes.html`,
`elementsFromPoint-shadowroot.html`, `CaretPosition-001.html`.

## Что нужно

A real point→box hit-test over the current layout/paint tree (the shell
already hit-tests pointer events for click dispatch — reuse that path),
exposed as one native (`_lumen_element_from_point(x, y) -> Option<NodeId>` and
an `_..._elements_from_point` variant returning the whole z-order stack),
wired to `document.elementFromPoint`/`elementsFromPoint`.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/cssom-view/` for files whose
dominant/sole cause is this gap, `expected: FAIL`/`TIMEOUT` per the actual
run.

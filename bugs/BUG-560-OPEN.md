# BUG-560: `:focus-within` style/match never reflects a synchronous `element.focus()` call — the shell applies the focus request only on its next pump

**Статус:** OPEN
**Дата:** 2026-08-04
**Компонент:** js/shell (`crates/js/src/dom.rs:13474` — `HTMLElement.prototype.focus`, queues via `_lumen_request_focus`; `crates/shell/src/main.rs:3048` — drains the queue "on its next pump")
**Найден:** P2, WPT-RUN-3 срез 40 (`css/selectors`), 2026-08-04

## Симптом

`selectors/focus-within-009.html` (harness itself now completes — previously `ERROR`, see «Побочная находка» below): every subtest that calls `target.focus()` and then immediately checks `:focus-within` styling/matching fails; every subtest that expects an *empty* result (initial state, after detaching the container, after moving the target out) passes.

```
FAIL Focus 'target1' - assert_array_equals: lengths differ, expected
  array ["html","body","test","container1","sibling2","target1"] length 6,
  got [] length 0
FAIL Focus 'target2' - assert_array_equals: … got []
FAIL Focus 'target1' again - assert_array_equals: … got []
FAIL Attach 'container1' in 'container2' - assert_array_equals: … got []
```

`elementsStyledWithFocusWithinSelector()` (via `getComputedStyle().backgroundColor`) and `elementsMatchingFocusWithinSelector()` (via `document.querySelectorAll(":focus-within")`) both report empty immediately after `.focus()`, in the same synchronous `test()` callback.

## Причина

`HTMLElement.prototype.focus()` (`crates/js/src/dom.rs:13474`) calls `_lumen_request_focus(nid)`, which only *queues* the request (`crates/js/src/lib.rs:298`/`v8_runtime.rs:322`, doc comment: "Focus requests queued by JS"). The shell drains this queue and applies the real focus state — the one `:focus`/`:focus-within` style matching in `layout/src/style.rs` reads — "on its next pump" (`crates/shell/src/main.rs:3048`, `13514`). A synchronous script (testharness.js's `test()` callbacks run back-to-back with no `await`/`requestAnimationFrame`/timer yield) never reaches that pump before reading `getComputedStyle()`/`querySelectorAll(":focus-within")`, so the style/match state is always one focus-change behind — permanently stale for any test, and potentially for any page script, that reads focus-dependent style synchronously after calling `.focus()`.

Real engines resolve this synchronously: `.focus()` triggers an immediate (or forced-on-next-read) style recalc, so `getComputedStyle()` called right after `.focus()` reflects the new `:focus`/`:focus-within` state in the same task. Lumen's queue-and-pump design defers this past the current synchronous script, which is observable and spec-incorrect (HTML LS §6.6.3 requires the focus update steps, including the "used for CSS" flag, to run synchronously as part of the focusing steps triggered by `.focus()`).

Not investigated: whether an interactive session (real winit event loop, `.focus()` from a user gesture followed by a later script) also observes staleness, or whether this only bites purely-synchronous same-task reads like this WPT test does. `:focus` itself wasn't independently exercised here — only the negative (`assert_false(...matches(":focus"))`) cases ran, and those pass regardless of whether focus is applied or stuck at "nothing focused".

## Побочная находка (не баг)

The file-level `expected: ERROR` in the committed `.ini` (from WPT-RUN-3 slice 30) is stale: the harness now completes fully (`status: OK`) — some unrelated engine fix between slice 30 (2026-08-03) and this re-run resolved whatever previously aborted the test before its first assertion. `.ini` updated to `expected: OK` with the 8 real `FAIL`/4 real `PASS` subtests from this bug.

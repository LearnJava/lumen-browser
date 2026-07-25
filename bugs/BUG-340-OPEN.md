# BUG-340: `CloseWatcher.requestClose()` recurses without bound when called re-entrantly from its own `cancel` handler

**Статус:** OPEN
**Компонент:** js (`crates/js/src/close_watcher.rs`, `CLOSE_WATCHER_SHIM`)
**Найден:** P2, WPT-VENDOR-close-watcher 2026-07-25 (`run_report.py --all --root close-watcher --recursive`, vendored `close-watcher/inside-event-listeners.html`)

## Симптом

WPT test `inside-event-listeners.html`, subtest "requestClose() inside oncancel":

```js
watcher.oncancel = () => { watcher.requestClose(); };
watcher.requestClose();
assert_array_equals(events, ["cancel[cancelable=true]", "close"]);
```

Expected 2 events. Actual: harness recorded a `close` array of length **9497** (and a
sibling subtest, "requestClose() inside oncancel with preventDefault()", produced a
`cancel` array of length **5520**) before the test's own timeout/assertion cut it off.

## Root cause

`CloseWatcher.prototype.requestClose` (`close_watcher.rs:97`-`106`):

```js
CloseWatcher.prototype.requestClose = function() {
  if (this._closed) return;
  var cancelEvt = _makeEvent('cancel', true);
  _dispatch(this._cancelListeners, cancelEvt);
  if (cancelEvt.defaultPrevented) return;
  this._fireClose();
};
```

`this._closed` is only set inside `_fireClose()` (`close_watcher.rs:121`-`126`), i.e.
*after* the `cancel` event has already been dispatched and returned. If the `cancel`
listener itself calls `requestClose()` again (a legal, spec-relevant pattern — see the
test), the guard `if (this._closed) return;` at the top of the re-entrant call still
sees `false`, so it dispatches a *second* `cancel` event, whose listener (the same
`oncancel`) calls `requestClose()` a third time, recursing until the JS call stack or
the event-array assertion blows up. There is no re-entrancy guard set *before*
dispatching `cancel`, only a "already closed" guard set *after* the whole
cancel→close sequence completes.

## Impact

- Fails the vendored WPT subtest (currently unpinned — no `.ini` for this category yet).
- Real-world hazard beyond WPT: any page whose `oncancel` handler calls
  `requestClose()`/`close()` again (e.g. to force-close after some synchronous check)
  will recurse until stack overflow or produce thousands of spurious `cancel`/`close`
  events, not just "the test's” pattern.

## Suspected fix direction

Set a re-entrancy guard (e.g. `this._closing = true`) *before* dispatching the `cancel`
event, and have `requestClose()` (and `close()`) no-op if already `_closing` — clearing
it once `_fireClose()`/return path completes. Cross-check against the sibling subtests
in the same file (`close()`/`destroy()` inside `oncancel`/`onclose`, all already
passing) to make sure the guard doesn't change their currently-correct behavior.

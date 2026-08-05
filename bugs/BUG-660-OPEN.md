# BUG-660 — `requestIdleCallback`/`IdleDeadline` shim is a fixed-delay stub, not spec-shaped: no `IdleDeadline` class, constant fake `timeRemaining()`, `didTimeout` always `false`, callback exceptions silently swallowed

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:8456`–`8480`, `requestIdleCallback`/`cancelIdleCallback` — JS shim, evaluated by the V8 install path per `CLAUDE.md`)
**Найден:** P2, WPT-VENDOR-requestidlecallback (2026-08-05), `run_report.py --all --root requestidlecallback --recursive` real run

## Механизм

The shim's own comment (`dom.rs:8457`–`8459`) already flags it as a stub
("fires via `setTimeout(~50ms)` with a synthetic `IdleDeadline` that always
reports 50ms remaining — Lumen is single-process, so there is no real idle
detection"), but no `BUG-NNN` was ever attached to that comment, so the gap
never showed up in `BUGS.md`/`STATUS-P3.md`. Running the real (never
previously vendored) `requestidlecallback` WPT category surfaces the
concrete spec deviations that comment was gesturing at:

1. **No `IdleDeadline` class.** `requestIdleCallback` invokes the user
   callback with a plain object literal (`dom.rs:8472`,
   `{ timeRemaining: function() { return 50; }, didTimeout: false }`), not an
   instance of a constructor exposed as `window.IdleDeadline`. Any branding
   check fails: `basic.html` — `assert_class_string: expected "[object
   IdleDeadline]" but got "[object Object]"`; the cross-realm test
   (`callback-timeRemaining-cross-realm-method.html`) can't even reference
   `iframeDelayed.contentWindow.IdleDeadline` — the global doesn't exist.
2. **`timeRemaining()` is a hardcoded constant `50`**, regardless of the
   actual time budget (current animation-frame deadline, pending timer, or
   how busy the "idle period" genuinely is). Three tests assert a real
   decreasing/bounded budget and get the constant back instead:
   `deadline-max-rAF.html` (expects ≤16.67ms, got 50),
   `deadline-max-rAF-dynamic.html` (same), `deadline-max-timeout-dynamic.html`
   (expects ≤10ms, got 50).
3. **`didTimeout` is hardcoded `false`** (`dom.rs:8472`) even on the path
   where the callback fires *because* `options.timeout` elapsed while the
   main thread was busy — spec requires `true` in that case.
   `callback-timeout.html` — `assert_true: expected true got false`.
4. **Callback exceptions are silently swallowed** (`dom.rs:8473`,
   `try { fn(deadline); } catch(e) {}`) instead of being reported to the
   global error handler the way an ordinary uncaught exception from a task
   would be (`window.onerror`/`error` event). `callback-exception.html`
   TIMEOUTs waiting for that report, since it's never dispatched.
5. **No real busy/idle modeling.** `requestIdleCallback` always fires after
   a fixed `setTimeout` delay, never actually deferring until the main
   thread is idle. `callback-timeout-when-busy.html` — both of its assertions
   (should not fire until the busy loop finishes; should still fire after
   `timeout` even while busy) fail because the stub has no concept of "busy"
   at all.

## Live run signal (not exhaustive — see report for full detail)

```
tests: 14/20 harness OK; subtests: 16/28 passed
```

Of the 13 unexpected harness/subtest results, 8 trace to this one root cause
(items 1–5 above, spread across `basic.html`, `callback-timeout-when-busy.html`,
`callback-timeout.html`, `deadline-max-rAF.html`, `deadline-max-rAF-dynamic.html`,
`deadline-max-timeout-dynamic.html`, `callback-exception.html`,
`callback-timeRemaining-cross-realm-method.html`).

## Что НЕ является причиной этого бага (уже задокументированные гэпы, не новый сигнал)

- `callback-iframe.html`, `callback-iframe-different-origin.html`,
  `callback-removed-frame.html` — all fail/TIMEOUT on `<iframe>`/
  `contentWindow` access, reconfirming [BUG-480](../bugs/BUG-480-OPEN.md)
  (`<iframe>` without its own browsing context).
- `callback-suspended.html` — TIMEOUT; depends on `window.open` + bfcache +
  cross-window navigation, same missing multi-window/browsing-context
  infrastructure as BUG-480, not a distinct gap.
- `idlharness.window.html` — TIMEOUT; the recurring un-vendored
  `WebIDLParser.js`/`idlharness.js` infra gap seen across every idlharness
  category so far, not engine-specific.

## Предлагаемый фикс

Replace the plain-object `deadline` literal with a real `IdleDeadline`
constructor exposed on `window` (so `instanceof`/class-string/cross-realm
prototype checks work), thread through the actual `didTimeout` value
computed from whether the callback fired via the timeout path vs. a real
idle slot, and route callback exceptions through the same
uncaught-exception/`window.onerror` reporting path `setTimeout` callbacks
already use instead of a bare `catch(e) {}`. Real busy/idle detection
(items 2 and 5) requires tracking actual main-thread idle periods — bigger
scope, worth splitting into a follow-up if the class/`didTimeout`/exception
fixes land first.

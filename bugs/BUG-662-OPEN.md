# BUG-662 — `rt.eval()`'s completion-value serialization eagerly invokes every getter on the result object, corrupting the reported error message (and running unintended side effects) for any classic script whose last statement evaluates to an object with a getter

**Статус:** OPEN
**Компонент:** js (`crates/js/src/v8_runtime.rs:5041`–`5068` — `from_v8_bounded`, object branch; reached from `V8JsRuntime::eval`'s return-value conversion)
**Найден:** P2, WPT-VENDOR-resize-observer (2026-08-05), real `run_report.py` run + isolated `--dump-layout` repro

## Механизм

`crates/shell/src/main.rs:7002`–`7013` runs every classic `<script>` block through
`rt.eval(src)` and only distinguishes `Ok`/`Err(NotImplemented)`/other `Err` — the `Ok`
value itself is discarded, never read by any caller. To build that `Result`, `eval()`
converts V8's completion value (the value of the *last executed expression statement* in
the script, standard ECMAScript `Script Evaluation` semantics) into a `JsValue` via
`from_v8_bounded`. For an object completion value, that function enumerates
`get_own_property_names` and calls `obj.get(scope, key)` for **every** own property
(`v8_runtime.rs:5061`–`5063`) — including accessor (`get`) properties, which this
unconditionally invokes. If a script's last statement is (or evaluates to) an object
literal containing a `get` accessor, and that accessor throws when invoked outside its
intended calling context, `obj.get()` returns `None` and the whole `eval()` call fails with
the opaque `format!("get '{key_str}' failed")` — even though the script itself ran
correctly to completion and its actual, real error (if any) is lost.

## Minimal repro

`tests/wpt/resize-observer/resources/resizeTestHelper.js` (an unmodified upstream WPT
helper, vendored verbatim) ends with:

```js
ResizeTestHelper.prototype = {
  get _currentStep() {
    return this._steps[this._stepIdx];
  },
  ...
};
```

The whole assignment's value (the object literal) is the script's completion value.
Isolated repro (`--dump-layout` on a page with only this file plus `testharness.js`/
`testharnessreport.js`, no test-specific script, confirmed against the *actual* wptrunner
run's log too — same message appears there): `rt.eval()` fails with

```
script error: JS runtime error: get '_currentStep' failed
```

even though **nothing in the script ever reads `.prototype._currentStep`** — the getter is
only ever accessed via `this._currentStep` inside `ResizeTestHelper` instance methods, on
actual instances (which have a real `_steps`/`_stepIdx`), never on the raw prototype object.
Calling the getter with `this` bound to the bare prototype object throws (`this._steps` is
`undefined`, so `this._steps[this._stepIdx]` throws `TypeError: Cannot read properties of
undefined`), and that throw is what `from_v8_bounded` surfaces as the misleading
`get '_currentStep' failed`.

Confirmed non-fatal to script execution: appending a trailing `<script>` block after
`resizeTestHelper.js` runs normally and its own (unrelated, error-free) completion value is
reported with no error — i.e. the bug is confined to whichever single script's own
completion value contains the throwing getter, and does not abort the classic-script
execution loop as a whole. Confirmed the error is a probe artifact only when it *isn't*
present: reproducing with a plain `python -m http.server` instead of the real wptrunner
initially also showed a **second**, unrelated `Unexpected token '%'` error from
`testharnessreport.js` — that one is expected (the file relies on wptrunner's own
`%(...)`-style Python string substitution, undocumented-but-intentional per that file's own
comment, and is *not* a real Lumen bug); it disappears once the placeholders are manually
substituted the way wptrunner substitutes them, isolating the `_currentStep` finding as the
one that also reproduces inside the real wptrunner-driven run.

## Impact

- **Masks the real error message for any script whose last-executed statement is an object
  with a throwing/side-effecting getter** — a common pattern (`Foo.prototype = {get x(){...}}`,
  IIFEs returning config objects with getters, etc.). Every such script reports the wrong,
  generic `get '<key>' failed` instead of whatever actually went wrong (or, as here, instead
  of reporting success at all, since nothing actually went wrong from the script's own
  perspective).
- **Invokes getters that were never meant to run**, purely to serialize a completion value
  nobody reads — at best wasted work, at worst (for a getter with real side effects) an
  unintended extra invocation of page logic. `console.log`-style debug value formatting
  should not need to be exception-safe *and transparent to program behavior* at the same
  time; converting an eval-result object to `JsValue` should catch (and represent, e.g. as an
  `"[Getter threw]"` placeholder like the existing `"[Circular]"`/`"[Max Depth Exceeded]"`
  sentinels) a getter exception rather than propagating it as the whole conversion's error,
  and arguably should not invoke arbitrary getters at all when the caller (here,
  `crates/shell/src/main.rs`) never reads the successful value in the first place.

## Предлагаемый фикс

In `from_v8_bounded`'s object branch (`v8_runtime.rs:5061`–`5064`), wrap the `obj.get(scope, key)`
call in a `TryCatch` and substitute a sentinel string (matching the existing `"[Circular]"`/
`"[Max Depth Exceeded]"` pattern) instead of propagating the getter's exception as the whole
conversion's `Err`. Separately, `crates/shell/src/main.rs`'s classic-script loop never consumes
`eval()`'s `Ok` value at all — consider a cheaper `eval_discard()`/`exec()` entry point for that
call site that skips completion-value conversion entirely, which would both fix this class of
bug for that call site and avoid the wasted serialization work on every classic script.

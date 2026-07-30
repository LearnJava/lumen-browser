# BUG-457 — `document.createElement()` at the DOM-node quota on V8: `u32::MAX` sentinel widens to a positive number, `nid < 0` check misses it, arena indexed out of bounds → process abort

**Статус:** FIXED 2026-07-30
**Компонент:** js (`crates/js/src/v8_runtime.rs::_lumen_create_element`/`_lumen_create_element_ns`,
`crates/js/src/v8_compat.rs::IntoJsReturn for u32`)
**Найден:** P1, S12b-24-perf-typedom-node 2026-07-30 (porting
`dom_create_element_throws_quota_exceeded_when_full` off the QuickJS monolith)
**Исправлен:** P1, S12b-24-perf-typedom-node 2026-07-30

## Симптом

With the DOM arena pre-filled to `lumen_dom::MAX_DOM_NODES` (50 000), calling
`document.createElement('p')` on the default (V8) engine does not throw
`QuotaExceededError` — it aborts the whole test process:

```
thread 'lumen-v8' panicked at crates\engine\dom\src\lib.rs:1106:20:
index out of bounds: the len is 50000 but the index is 4294967295
thread 'lumen-v8' panicked at .../panicking.rs:225:5:
panic in a function that cannot unwind
```

(`STATUS_STACK_BUFFER_OVERRUN`, exit code `0xc0000409` — a panic crossing the
V8-callback FFI boundary aborts instead of unwinding.)

## Причина

`_lumen_create_element`/`_lumen_create_element_ns` (`v8_runtime.rs`) signal
"arena full" by returning `u32::MAX` — a comment right above them says
"Returns u32::MAX when MAX_DOM_NODES is reached; JS shim handles this", and
the engine-agnostic `WEB_API_SHIM` (`dom.rs:4832`) does exactly that:

```js
doc.createElement = function(tag) {
    var nid = _lumen_create_element(String(tag).toLowerCase());
    if (nid < 0) { throw new DOMException(.... 'QuotaExceededError'); }
    return _lumen_make_element(nid);
};
```

This contract was written for and only ever exercised against rquickjs,
where marshalling a Rust `u32` back into a JS number happens to truncate
through a signed 32-bit intermediate — `u32::MAX` (`4294967295`) comes out as
`-1`, so `nid < 0` is `true` and the shim throws correctly. `IntoJsReturn for
u32` (`v8_compat.rs`) does not have that accidental truncation: it widens via
`self as f64`, so `u32::MAX` becomes the *positive* `4294967295.0`. The shim's
`nid < 0` check is then always `false` on V8, `_lumen_make_element` proceeds
to wrap node id `4294967295` into an `Element`, and the first arena access
(`Arena::get`, `crates/engine/dom/src/lib.rs:1106`, `&self.nodes[id.index()]`,
no bounds check — the arena's whole design assumes ids are always valid)
indexes 4 billion past a 50 000-length `Vec` and panics. Because this panic
crosses back into V8's C++ call stack through the native-function
trampoline, `panic in a function that cannot unwind` aborts the process
instead of unwinding a normal Rust panic — this bug could not have surfaced
as an `Err`/`catch_unwind`-recoverable test failure no matter what the test
asserted; every WPT/graphic-tests run that ever hit the real 50 000-node
limit under V8 would have hard-crashed the process silently, not failed a
single test.

The QuickJS-vs-V8 truncation difference had no coverage until this slice
because `dom_create_element_throws_quota_exceeded_when_full` was one of the
~1113 tests still sitting in the QuickJS-only `dom.rs::mod tests` monolith
(S12b-24) — the exact blind spot this migration slice-by-slice is closing.

## Fix

Changed both bindings' return type from `u32` to `i32` (`v8_runtime.rs`):
the arena never holds more than `MAX_DOM_NODES` (50 000) nodes, so every
real node id fits comfortably in `i32`, and `IntoJsReturn for i32` already
converts `self as f64` — which, unlike `u32`, preserves the sign of a `-1`
sentinel. Success path: `nid.index() as i32`. Error path: `-1` literal
(replacing the `u32::MAX` sentinel). No change to the JS shim needed — its
`nid < 0` check was already correct, only the native side was silently
engine-specific.

Verified: the ported test `dom_create_element_throws_quota_exceeded_when_full`
now asserts `QuotaExceededError` is thrown and passes; full `cargo test -p
lumen-js --features v8-backend` — 2575/2575, no regressions. `dom_node_count_at_max_after_prefill`
(same slice, reads `_lumen_dom_node_count()` rather than `createElement`'s
return value) was unaffected either way — it doesn't touch this sentinel.

## Связанные

* [BUG-442](BUG-442-FIXED.md) / [BUG-342](BUG-342-FIXED.md) — same class of
  bug: a shim/native contract written for rquickjs's marshalling quirks
  silently broke on V8's more literal conversions, uncovered only once the
  covering test was ported off the QuickJS monolith.

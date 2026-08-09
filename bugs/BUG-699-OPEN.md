# BUG-699: `WebAssembly.Table` constructor throws on a BigInt `initial`/`maximum` (`address: "i64"`), poisoning the shared WPT harness setup for the whole file

**Статус:** OPEN
**Компонент:** js (`crates/js/src/webassembly.rs:112-125` — `WebAssembly.Table` constructor in `WEBASSEMBLY_SHIM`)
**Найден:** P2, WPT-VENDOR-wasm, 2026-08-09

## Симптом

Found while triaging the `wasm` category run (see [BUG-698](BUG-698-OPEN.md)
for the run numbers). Every vendored `.wast.js` test file's shared harness
(`tests/wpt/wasm/core/js/harness/async_index.js::reinitializeRegistry`)
builds a default `spectest` import module once per file, including a
64-bit-addressed table:

```js
table64: new WebAssembly.Table({
  initial: 10n, maximum: 20n, element: "anyfunc", address: "i64"
}),
```

Lumen's `Table` constructor (`crates/js/src/webassembly.rs:112-125`)
coerces both fields with the bitwise-OR idiom, which V8 rejects outright
when either operand is a `BigInt`:

```js
var initial = descriptor.initial | 0;
...
this._max = (descriptor.maximum !== undefined) ? (descriptor.maximum | 0) : Infinity;
```

```
10n | 0  →  TypeError: Cannot mix BigInt and other types, use explicit conversions
```

Because `reinitializeRegistry()` runs inside the shared promise `chain`
that every subsequent `module()`/`instance()`/`run()` call in the *same*
test file threads through (`chain = chain.then(_ => WebAssembly.compile(...))...`),
this synchronous throw rejects `chain` at the very first link — and a
rejected promise's `.then(onFulfilled)` (no rejection handler) silently
propagates the rejection instead of calling `onFulfilled`. Every later
assertion in that file therefore inherits the *same* stale error instead
of evaluating its own module, surfacing as unrelated-looking failures
further down:

```
FAIL Test that WebAssembly compilation succeeds (br.wast:3) -
  assert_true: WebAssembly.compile failed unexpectedly with
  TypeError: Cannot mix BigInt and other types, use explicit conversions
FAIL Test that WebAssembly instantiation succeeds -
  assert_true: unexpected instantiation error, observed
  TypeError: Cannot read properties of undefined (reading 'source')
FAIL run - assert_true: unexpected runtime error, observed
  TypeError: Cannot read properties of undefined (reading 'run')
```

`"Reinitialize the default imports"` itself failed as an unhandled promise
rejection **510 times** across the run's 373 test files (`grep -c
'Reinitialize the default imports' /tmp/wpt_wasm.log`), each one capable of
poisoning the rest of its own file's chain the same way — a second,
independent multiplier on top of [BUG-698](BUG-698-OPEN.md) for the run's
depressed subtest pass rate (11314/179992).

## Причина

`Table.constructor` was written before the memory64 proposal's
64-bit-addressed table variant (`address: "i64"`, `initial`/`maximum` as
`BigInt`) existed as an input shape; `descriptor.initial | 0` assumes a
plain `Number` unconditionally. `Global`'s constructor (a few lines below,
same file) does not have this bug — it stores `value` as-is without
coercion, so it already tolerates a `BigInt` `i64` global correctly.

## Как воспроизвести

Live probe (no WPT needed):
```js
new WebAssembly.Table({ initial: 10n, maximum: 20n, element: "anyfunc" });
// → TypeError: Cannot mix BigInt and other types, use explicit conversions
```
or the full run:
```
tests/wpt/run_report.py --binary <lumen.exe> --all --root wasm --recursive --processes=4
```
grep the log for `Reinitialize the default imports` to see every poisoned
file.

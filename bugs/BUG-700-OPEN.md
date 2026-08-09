# BUG-700: WASM element-segment decoder only implements flags 0/1/2 — rejects the majority of real-world modules

**Статус:** OPEN
**Компонент:** js (`crates/js/src/wasm/parser.rs:547-599` — `parse_element_section`)
**Найден:** P2, WPT-VENDOR-wasm, 2026-08-09

## Симптом

`wasm` (scope ⬜, candidate) — vendored and run in full
(`run_report.py --all --root wasm --recursive --processes=4`, ~6:04, 830
files / 405 selected ids): **342/373 harness OK, 11314/179992 subtests**
(≈6.3%). The headline "342/373 harness OK" is misleading — most of those
"OK" files still failed almost every subtest inside them. The single
largest failure cluster across the whole run (`wasm/core` 49513 FAILs,
`wasm/core/simd` 32443, `wasm/core/relaxed-simd` 19725, `wasm/core/memory64`
10751, `wasm/core/multi-memory` 8932, plus a smaller slice of
`wasm/core/bulk-memory`) traces to one root cause: `WebAssembly.validate()`/
`WebAssembly.compile()` reject the module outright before any code ever
runs, because the module's element (`elem`) section uses one of the
encoding flags Lumen's parser doesn't implement.

Live repro (exact bytes extracted from the vendored
`tests/wpt/wasm/core/js/br.wast.js`, module `br.wast:3` — a plain
control-flow test, nothing SIMD/GC/memory64-specific):

```js
WebAssembly.validate(bytes.buffer);   // → false (spec says: valid module)
WebAssembly.compile(bytes.buffer);    // → rejects: CompileError: unsupported element segment flags 4
```

`parser::parse_element_section` (`crates/js/src/wasm/parser.rs:547-599`)
only handles `flags` values 0 (active, table 0, func-index list), 1
(passive, func-index list) and 2 (active, explicit table index,
func-index list); every other value in the legal 0-7 range falls through
to `_ => return Err(format!("unsupported element segment flags {flags}"))`:

```rust
match flags {
    0 => { /* active table 0, func indices */ }
    1 => { /* passive, elemkind + func indices */ }
    2 => { /* active, explicit table idx, elemkind + func indices */ }
    _ => return Err(format!("unsupported element segment flags {flags}")),
}
```

Flags 3-7 (declarative segments, and — critically — the three "expr-encoded
elements" variants 4/5/7 where the element list is a sequence of constant
expressions `ref.func $x`/`ref.null` instead of a bare func-index list) are
entirely unimplemented. The official WASM spec-test generator (the same
tool that produced every `.wast.js.html` file in this vendored category)
emits the expr-encoded form by default for any element segment that isn't
the absolute-simplest case — so the gap isn't proposal-specific, it hits
plain MVP-level control-flow tests (`br.wast`) exactly as hard as
`simd`/`relaxed-simd`/`memory64`/`multi-memory` ones. `CAPABILITIES.md`'s
"🟡 WebAssembly MVP" entry currently claims the interpreter "decodes the
WASM 1.0 core binary format and executes it" and lists SIMD/relaxed-SIMD
as "fully supported" — that claim cannot currently be verified against the
official spec test corpus, because the vast majority of it never gets past
`compile()`.

## Причина

`parse_element_section` was written against a subset of the element
segment grammar (the flags-0/1/2 "func-index list" encodings only) and
never extended to the flags-3..7 "expression list" / declarative variants
added by the bulk-memory-operations + reference-types proposals — both
merged into what test tooling and every shipping engine now treat as core
WebAssembly.

## Как воспроизвести

```
tests/wpt/run_report.py --binary <lumen.exe> --all --root wasm --recursive --processes=4
```
or a live probe against a single extracted module:
```js
// bytes = the exact `br.wast:3` module from tests/wpt/wasm/core/js/br.wast.js
WebAssembly.compile(bytes.buffer).catch(e => console.log(String(e)));
// → CompileError: unsupported element segment flags 4
```

## Замечание для последующей ревизии

This is the dominant but not sole contributor to the run's low subtest
pass rate — see also [BUG-699](BUG-699-OPEN.md) (a `WebAssembly.Table`
crash that separately poisons the shared per-file harness setup). Until
both are fixed, `wasm/core/simd`'s and `wasm/core/relaxed-simd`'s true
pass rate against **executable** SIMD code is not measurable from this
run — most of their failures happen before a single SIMD instruction
executes. Re-run `wasm` after both fixes land before revising
`CAPABILITIES.md`'s SIMD/relaxed-SIMD claims up or down.

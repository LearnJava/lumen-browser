# ADR-004: rquickjs (QuickJS) as Phase 0 JS engine, rusty_v8 for v1.0+

## Status

Accepted

## Date

2026-05-20

## Context

Lumen needs a JS engine. Permanent exception #5: we do not write our own JS engine.

Two realistic candidates:
- **QuickJS** via `rquickjs` crate: ~200 KB, ES2020-compliant, simple integration, pure in-process, no system dependencies.
- **V8** via `rusty_v8`: 15 years of production use, billions of users, JIT-compiled — required for SPAs (React/Vue/Angular). But ~150 MB shared library, complex to link on Windows/Linux/macOS.

## Decision

Use `rquickjs` (QuickJS) for Phase 0–2. Switch to V8 via `rusty_v8` for v1.0+ when SPA support becomes required.

The JS engine is isolated behind the `JsRuntime` trait in `lumen-core::ext`. Switching implementations is a drop-in replacement in `lumen-js` — no API change for callers.

## Implementation notes

- `Mutex<Inner>` for `Send + Sync`: QuickJS is single-threaded; standard pattern for C bindings.
- `call_function` via eval workaround: rquickjs 0.11 `Function::call` requires fixed-arity tuples. Temporary global `__lum_args__` + `fn.apply(null, __lum_args__)` via `ctx.eval`. Remove when rquickjs exposes native `apply`.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| V8 / `rusty_v8` now | Too heavy for Phase 0 (150 MB, complex Windows linking). JIT not needed until SPAs. |
| SpiderMonkey | Even heavier than V8; Rust bindings less maintained. |
| Boa (pure Rust) | ~60% ES2022 compliance; too many spec gaps for real-world pages. |

## Consequences

- **Positive:** tiny binary footprint; simple integration; good enough for Phase 0 inline scripts and eval.
- **Negative:** no JIT — SPAs (React/Vue/Angular) will be slow; ES2020 only (some modern syntax unsupported); `call_function` workaround is a known technical debt.
- **Future:** when benchmark shows QuickJS is too slow for target pages, or when SPA support is required, replace `lumen-js` impl with `rusty_v8`. The `JsRuntime` trait boundary makes this a one-crate change.

# BUG-547: `navigator.storageBuckets` does not exist under the default V8 build — CAPABILITIES.md claims it ✅ with full detail

**Статус:** FIXED 2026-08-04
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/storage_buckets.rs`, install site was `QuickJsRuntime::install_dom` — QuickJS-only)
**Найден:** P1, S12b-G0 триаж (13 модулей без V8-порта)

## Механизм

`storage_buckets::init_storage_buckets` (`navigator.storageBuckets.{open,keys,delete}`
+ `StorageBucket.{persisted,persist,estimate,durability,setExpires,expires,getDirectory}`)
was only called from `QuickJsRuntime::install_dom`. `grep -n
"storage_buckets::" crates/js/src/v8_runtime.rs` returned zero hits — no `_v8`
variant existed at the time.

## Симптом

`CAPABILITIES.md`'s Storage bullet spelled out the full Storage Buckets API
surface under an unconditional ✅ — false on the default (V8) build:
`typeof navigator.storageBuckets` → `"undefined"`. This was the most
detailed of the 13 G-group overclaims in CAPABILITIES.md (the bullet listed
open/keys/delete plus all seven per-bucket methods as if verified against
the shipped engine).

## Фикс

**Закрыт 2026-08-04 (P1, S12b-G2).** Ported per the standard S12b-G group
procedure (`docs/tasks/p1-s12b-cleanup-queue.md` §4): a pure quick-path
port — `init_storage_buckets_v8` calling `rt.eval(SHIM)`, registered via
`install_v8!(storage_buckets::install_storage_buckets_v8)` in
`v8_runtime.rs::install_dom` (`crates/js/src/v8_runtime.rs:4309`). 8 existing
tests ported against `V8JsRuntime` (`#[cfg(all(test, feature =
"v8-backend"))]`). rquickjs side removed in the same batch, then the
`QuickJsRuntime` fallback itself deleted outright by S12b-F1..F4.
`CAPABILITIES.md`'s Storage bullet downgraded from unconditional ✅ to 🟡
(Phase 0 in-memory, no longer QuickJS-only) — corrected in the S12b-F4 commit
(2026-08-04, after the CAPABILITIES.md text had drifted from the G2 fix).

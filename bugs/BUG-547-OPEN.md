# BUG-547: `navigator.storageBuckets` does not exist under the default V8 build — CAPABILITIES.md claims it ✅ with full detail

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/storage_buckets.rs`, install site `crates/js/src/lib.rs:1024` — QuickJS-only)
**Найден:** P1, S12b-G0 триаж (13 модулей без V8-порта)

## Механизм

`storage_buckets::init_storage_buckets` (`navigator.storageBuckets.{open,keys,delete}`
+ `StorageBucket.{persisted,persist,estimate,durability,setExpires,expires,getDirectory}`)
is only called from `QuickJsRuntime::install_dom`. `grep -n
"storage_buckets::" crates/js/src/v8_runtime.rs` — zero hits, no `_v8`
variant exists.

## Симптом

`CAPABILITIES.md`'s Storage bullet spells out the full Storage Buckets API
surface under an unconditional ✅ — false on the default (V8) build:
`typeof navigator.storageBuckets` → `"undefined"`. This is the most detailed
of the 13 G-group overclaims in CAPABILITIES.md (the bullet lists
open/keys/delete plus all seven per-bucket methods as if verified against
the shipped engine).

## Фикс (не сделан)

Port per the standard S12b-G group procedure
(`docs/tasks/p1-s12b-cleanup-queue.md` §4, S12b-G2 slot): add
`init_storage_buckets_v8`, register in `v8_runtime.rs::install_dom`, port
the 8 existing tests against `V8JsRuntime`. Downgrade the CAPABILITIES.md
Storage bullet's "Storage Buckets" from unconditional ✅ to 🟡 (QuickJS-only)
until then — done in this commit.

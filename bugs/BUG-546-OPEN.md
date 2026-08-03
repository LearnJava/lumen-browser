# BUG-546: `cookieStore` global does not exist under the default V8 build — CAPABILITIES.md claims it ✅

**Статус:** OPEN
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/cookie_store.rs`, install site `crates/js/src/lib.rs:1016` — QuickJS-only)
**Найден:** P1, S12b-G0 триаж (13 модулей без V8-порта)

## Механизм

`cookie_store::init_cookie_store` (the WHATWG Cookie Store API shim —
`cookieStore.get`/`getAll`/`set`/`delete` + the `change` event) is only
called from `QuickJsRuntime::install_dom`. `grep -n "cookie_store::"
crates/js/src/v8_runtime.rs` — zero hits, no `_v8` variant exists.

## Симптом

`CAPABILITIES.md`'s Storage bullet claims "✅ ... Cookie Store, Storage
Buckets (...) ..." unconditionally — false on the default (V8) build:
`typeof cookieStore` → `"undefined"`. Any page that reads/writes cookies via
the modern async API instead of `document.cookie` gets a silent
`TypeError` (or, if it feature-detects first, falls back to
`document.cookie`, which does work — so the practical severity is lower than
Storage Buckets/View Transitions, but the CAPABILITIES.md claim is still
wrong).

## Фикс (не сделан)

Port per the standard S12b-G group procedure
(`docs/tasks/p1-s12b-cleanup-queue.md` §4, S12b-G5 slot): add
`init_cookie_store_v8`, register in `v8_runtime.rs::install_dom`, port the 8
existing tests against `V8JsRuntime`. Downgrade the CAPABILITIES.md Storage
bullet's "Cookie Store" from unconditional ✅ to 🟡 (QuickJS-only) until
then — done in this commit.

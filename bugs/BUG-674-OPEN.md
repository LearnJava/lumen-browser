# BUG-674 — Service Worker registry and Cache Storage are keyed by an unauthenticated JS-string "origin" argument — any page script can read/write another origin's registrations and caches

**Статус:** OPEN
**Компонент:** js — `crates/js/src/dom.rs` (`_sw_origin`, line 4805 — plain mutable `var` derived once from `location.protocol + '//' + location.host`; every SW/Cache Storage shim call forwards it verbatim) + `crates/js/src/v8_runtime.rs` (`_lumen_sw_register`/`_lumen_sw_has_registration`/`_lumen_sw_unregister`/`_lumen_sw_persist`/`_lumen_sw_load` at ~line 1900-1940, and the `_lumen_cache_*` family below it — every one takes `origin: String` as a plain first argument with zero validation against the page's real navigated origin)
**Найден:** P2, WPT-VENDOR-service-workers (2026-08-06), `run_report.py --all --root service-workers --recursive --processes=4` gave no signal (285/287 TIMEOUT on the documented TLS gap `UnknownIssuer`, 1 ERROR is a stale-context artifact — same class as BUG-380, 1 harness OK with its own subtest failing on an unrelated `.sub.html` HTTP 500). Found by a live `--mcp-live-port` probe of the native binding surface.

## Механизм

`_sw_origin` (`dom.rs:4805`) is computed once from `location` but is an ordinary writable global — any page script can reassign it. Worse, the natives it feeds (`_lumen_sw_register`, `_lumen_sw_has_registration`, `_lumen_sw_unregister`, `_lumen_sw_persist`, `_lumen_sw_load`, and the whole `_lumen_cache_*` family for Cache Storage) are themselves plain `window`-visible functions (`reg!` macro registration, `v8_runtime.rs:1900`-1940 and following) that accept `origin` as a caller-supplied string with **no server-side binding to the actual browsing context's navigated origin** — `grep -n "current_origin\|page_origin\|effective_origin" crates/js/src/v8_runtime.rs` returns nothing. The Rust-side `SwMap`/`CacheMap` key on whatever string the JS call passes, not on anything the engine independently knows about the page.

This is the same defect class as [BUG-371](BUG-371-FIXED.md) (`file-system-access`'s `_lumen_file_read_text` family: capability-bearing natives exposed as plain `window` properties with a guessable/forgeable argument instead of an unforgeable per-origin token) — here the forgeable argument is the origin string itself, for the entire Service Worker registry and Cache Storage.

## Живое воспроизведение

Через `--mcp-live-port` (`file://` страница с одним `<script>`):

```
_lumen_sw_has_registration('https://victim.example')                       => false
_lumen_sw_register('https://victim.example', '/', '/evil-sw.js')           => (no error)
_lumen_sw_has_registration('https://victim.example')                       => true
```

The registration for `https://victim.example` was created directly from a page whose own real origin is `file://D` (drive-letter `location.host` quirk, separately tracked) — no cross-origin check anywhere in the call path. The same native functions back `caches.open`/`caches.match`/etc. (`_lumen_cache_put` and siblings take `origin: String` identically), so the same forgeable-argument gap applies to Cache Storage reads and writes, not just the SW registration table.

Additionally, reassigning the page-global `_sw_origin` itself (no native call needed) redirects every subsequent `navigator.serviceWorker.*`/`caches.*` call transparently:

```js
_sw_origin = 'https://spoofed.example';
navigator.serviceWorker.getRegistration('/')
// resolves with a registration object that was registered under a *different*
// script's chosen origin string, with no origin check anywhere in the path
```

## Симптом

Any script running in a Lumen tab — including, per [BUG-480](BUG-480-OPEN.md) (`<iframe>` has no separate browsing context, so a same-page iframe from a different nominal origin shares the parent's JS globals) an embedded cross-origin `<iframe>` — can enumerate, register, unregister, or overwrite another origin's Service Worker registrations, and read or write another origin's Cache Storage contents, by supplying an arbitrary origin string. This is a same-origin-policy violation for two storage-like APIs the spec requires to be strictly origin-partitioned (Service Workers §2.2, Cache Storage relies on the same partitioning).

## Что НЕ является причиной

Not the TLS gap (`UnknownIssuer`) documented for this category — that blocks `.https.` network navigation before JS ever runs; this bug reproduces on a live `file://` page, no navigation involved. Not BUG-657 (`ServiceWorkerRegistration` missing its prototype members) — that is about the *shape* of a registration object; this bug is about *who* can read/write *which* origin's registrations and caches in the first place, a distinct and more severe defect (the shape bug is meaningless once any origin's registry is writable by any other origin).

## Предлагаемый фикс

Stop threading `origin` as a JS-supplied string through the native boundary. Bind SW/Cache Storage state to the origin the Rust-side browsing context already knows for the active document (the same value `PageSource`/navigation already computes) and drop the `origin` parameter from every `_lumen_sw_*`/`_lumen_cache_*` native signature — or, if a per-call parameter is kept for internal reasons, validate it server-side against the real navigated origin and reject/ignore mismatches instead of trusting it verbatim.

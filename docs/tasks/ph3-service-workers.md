# Ph3 — Service Worker runtime

**Developer:** P3 + P4
**Branch:** `ph3-service-workers`
**Size:** XL
**Crates:** `lumen-js`, `lumen-network`, `lumen-storage`, `lumen-shell`
**Phase:** 3 (v1.0 target, `docs/plan/phases.md:114,129`)

---

## Status

Partially stubbed. The fetch interception pipeline (PH3-20) is complete.
Remaining work: push event delivery, background sync queue wiring, and a richer
SW execution context (request body, streaming response, response metadata).

See "Current state" below for a precise gap map.

---

## Goal

Full Service Worker runtime per W3C Service Worker spec (https://w3c.github.io/ServiceWorker/):

- **P3 owns:** fetch interception improvements, push event delivery end-to-end,
  background sync queue persistence and event firing.
- **P4 owns:** SW JS context completeness — `ServiceWorkerGlobalScope`, lifecycle
  events (install/activate), `clients.claim()` / `skipWaiting()` semantics,
  correct scope-matching algorithm, periodic background sync.

After this task:
1. A page can register a SW that intercepts navigation and subresource fetches
   and serves responses from `caches` or custom logic.
2. A server can deliver a Web Push notification; the browser wakes the SW's
   thread and dispatches a `push` event with the payload.
3. A page can schedule a background sync tag; the browser fires a `sync` event
   at the SW on the next network opportunity.

---

## Current state

### Fetch interception — largely done (PH3-20)

The fetch gate is live in four `HttpClient` methods:

- `crates/network/src/lib.rs:2874` — `fetch_page`
- `crates/network/src/lib.rs:2939` — `fetch_page_streaming`
- `crates/network/src/lib.rs:3000` — `NetworkTransport::fetch`
- `crates/network/src/lib.rs:3121` — `fetch_sync` (JS `fetch()`)

All four call `FetchInterceptor::intercept(url, origin)` before any network I/O.
The trait is declared at `crates/core/src/ext.rs:1507`.

`ServiceWorkerInterceptor` (SQLite-backed) lives at
`crates/storage/src/sw_interceptor.rs`. It routes to a live SW execution thread
via `SwWorkerStore` (longest-scope-prefix match), then falls back to
`CacheStorage`.

SW execution thread: `crates/js/src/sw_worker.rs` — one `std::thread` per
activated SW, running a bare QuickJS `Context` with `ServiceWorkerGlobalScope`
globals. Spawned via `_lumen_sw_activate_script` native binding
(`crates/js/src/dom.rs:823`). Thread handle stored in `SwWorkerStore`
(`crates/core/src/ext.rs:3522`).

Shell wires everything at `crates/shell/src/main.rs:730–743`.

**Gaps in fetch interception:**
- `FetchInterceptor::intercept` returns `Option<Vec<u8>>` (body bytes only).
  Status code, response headers, and MIME type are lost. The SW thread's
  `dispatch_fetch` (`crates/js/src/sw_worker.rs:101`) extracts only the string
  body via a JS global, discarding the `Response` object's metadata.
- Request body is not passed into the SW thread (`SwFetchRequest` at
  `crates/core/src/ext.rs:3494` has `url` + `method` but no `body` field).
  POST intercepts can see the method but not the payload.

### SW JS context — JS-shim layer (Phase 0)

Registration, lifecycle simulation, and `caches` API are in
`crates/js/src/dom.rs` (the large JS shim, lines ~5742–5920).
The shim creates a JS-only `ServiceWorkerRegistration` object in the page
context; the Rust activation path fires when the JS shim calls
`_lumen_sw_activate_script`.

`ServiceWorkerGlobalScope` globals installed in the worker thread:
`crates/js/src/sw_worker.rs:142` — `globalThis`, `location`, `registration`,
`skipWaiting`, `clients`, `addEventListener`, minimal `Headers` / `Response`,
`caches` (backed by `CacheBackend`), and `atob`/`btoa`.

SW registration persistence: `crates/js/src/dom.rs` delegates to `SwBackend`
(trait `crates/core/src/ext.rs:1843`); concrete implementation is
`crates/storage/src/sw_store.rs` (JSON snapshot per origin in
`StorageBackend`). Full SQLite registration table:
`crates/storage/src/service_workers.rs`.

### Push API — stub only (Phase 0)

JS shim: `crates/js/src/push_api.rs`.
`PushManager.subscribe()` generates a fake static endpoint and calls
`_lumen_push_subscribe(endpoint, userVisibleOnly)` if present, but **no Rust
binding for `_lumen_push_subscribe` is registered anywhere** — the call is a
no-op.

SQLite subscription storage exists: `crates/storage/src/push_subscriptions.rs`
(`PushSubscriptions` struct with `subscribe`, `get`, `get_by_scope`,
`list_for_origin`, `unsubscribe`).

**Missing:** VAPID key generation, real push endpoint registration with a push
service, incoming push message reception, and delivery of a `push` event to the
SW execution thread. `SwWorkerHandle` only has a `SwFetchRequest` channel; there
is no `SwPushMessage` type or delivery path.

### Background sync — stub only (Phase 0)

JS shim: `crates/js/src/background_sync.rs`.
`SyncManager.register(tag)` calls `_lumen_sw_sync_register(tag)` if present,
but **no Rust binding is registered**. Tags are stored in-memory in the JS shim
only and are lost on page reload.

No Rust sync queue, no `SyncEvent` dispatch, no `_lumen_sw_sync_register`
binding in `crates/js/src/dom.rs` or `lib.rs`.

`PeriodicSyncManager` shim: `crates/js/src/periodic_sync.rs` — same situation
(JS-only, no Rust backing).

---

## Architecture

### DOM-less worker JS context (P4)

The SW thread already runs a bare `rquickjs::Context` (`crates/js/src/sw_worker.rs:53`).
It is DOM-less by design — correct per spec. Gaps to close (P4):

1. **`ServiceWorkerGlobalScope` completeness:** `fetch()` global (calls back into
   `lumen-network`), `importScripts()` stub, `self.registration` pointing to a
   live `ServiceWorkerRegistration` object.
2. **Lifecycle event sequencing:** the shim fires `install` then `activate` in
   order (`sw_worker.rs:75–88`) but does not handle `waitUntil()` promises across
   the `install → activate` boundary correctly under concurrent requests.
3. **Response metadata propagation:** `FetchInterceptor` must return a full
   response struct (status, headers, body) rather than `Option<Vec<u8>>`. This
   requires a `FetchResponse` type in `lumen-core::ext` and updates to
   `ServiceWorkerInterceptor`, `InMemoryFetchInterceptor`, and all four call
   sites in `lib.rs`.

### Scope matching (P3)

`ServiceWorkerInterceptor::intercept` currently does longest-prefix match on
`url.path()` against `scope` strings stored in `SwWorkerStore`
(`crates/storage/src/sw_interceptor.rs:65–80`). The W3C spec requires matching
against the full serialized URL (scheme + host + path). Cross-origin SWs must
be rejected. This is currently unchecked.

### Fetch interception at the network gate (P3)

The four `interceptor.intercept(url, origin)` call sites in
`crates/network/src/lib.rs` are the correct hook points — they already fire
before any TCP connection. The `origin` parameter enables origin-partitioned
cache lookup.

To pass request body to the SW: extend `SwFetchRequest`
(`crates/core/src/ext.rs:3494`) with a `body: Option<Vec<u8>>` field and
propagate it from `fetch_sync` (the JS `fetch()` path at `lib.rs:3121`).

### Cache API (sub-dependency — already done)

`CacheBackend` trait (`crates/core/src/ext.rs:1865`) is fully implemented in
`lumen-storage::CacheStorage` (`crates/storage/src/cache_storage.rs:352`).
The SW worker thread has access via `_lumen_sw_cache_*` native bindings
(`crates/js/src/sw_worker.rs:155–196`). No new work needed here.

### Push event delivery (P3)

Required pieces:
1. **`SwPushMessage` type** in `lumen-core::ext` alongside `SwFetchRequest`
   (`crates/core/src/ext.rs:~3494`). Fields: `payload: Vec<u8>`, `response_tx`.
2. **Extend `SwWorkerHandle`** (`crates/core/src/ext.rs:3509`) with a second
   sender channel for push messages, or add a `SwMessage` enum.
3. **`_lumen_push_subscribe` Rust binding** registered in
   `crates/js/src/dom.rs` (near the other `_lumen_sw_*` bindings at ~line 817).
   Must write to `PushSubscriptions` SQLite store
   (`crates/storage/src/push_subscriptions.rs`).
4. **Incoming push delivery path:** a shell-level component (e.g. a background
   thread or a new shell command `--deliver-push`) that reads from a push
   service connection (or a test fixture), looks up the matching SW by origin +
   scope in `SwWorkerStore`, and sends a `SwPushMessage`.
5. **`_sw_fire_push(payload)` JS function** in the SW worker context
   (`crates/js/src/sw_worker.rs`) analogous to `_sw_fire_fetch`. Fires the
   `push` event on `self` with a `PushMessageData` object.

Note: Full VAPID / Web Push Protocol (RFC 8030 + RFC 8291) integration with an
external push service is a Phase 3+ concern. An initial implementation can use
a local loopback delivery mechanism for testing.

### Background sync queue (P3)

1. **`_lumen_sw_sync_register` Rust binding** in `crates/js/src/dom.rs` — stores
   the tag in a new SQLite table (or reuse `crates/storage/src/service_workers.rs`
   with an added `sync_tags` table) keyed by `(origin, scope, tag)`.
2. **`SwSyncMessage` type** in `lumen-core::ext` with `tag: String`.
3. **Extend `SwWorkerHandle`** for sync dispatch (same enum as push above).
4. **Sync-fire trigger:** on network-online transitions or page navigations,
   iterate pending sync tags in the store, dispatch `SwSyncMessage` to the
   matching worker thread, and on success remove the tag.
5. **`_sw_fire_sync(tag)` JS function** in the SW worker context — fires the
   `sync` event with a `SyncEvent` object (`.tag` property).

---

## Entry points (real file:line; [proposed] = does not exist yet)

| File | Line | Purpose |
|---|---|---|
| `crates/core/src/ext.rs` | 1507 | `FetchInterceptor::intercept` trait |
| `crates/core/src/ext.rs` | 1843 | `SwBackend` trait (registration persistence) |
| `crates/core/src/ext.rs` | 1865 | `CacheBackend` trait |
| `crates/core/src/ext.rs` | 3494 | `SwFetchRequest` struct — extend with `body` [P3] |
| `crates/core/src/ext.rs` | 3509 | `SwWorkerHandle` struct — extend with push/sync channels [P3] |
| `crates/core/src/ext.rs` | 3522 | `SwWorkerStore` type alias |
| `crates/js/src/sw_worker.rs` | 24 | `spawn_sw_worker` — entry point for SW thread spawn |
| `crates/js/src/sw_worker.rs` | 101 | `dispatch_fetch` — extend to return response metadata [P4] |
| `crates/js/src/sw_worker.rs` | 142 | `install_sw_globals` — add push/sync event globals [P3+P4] |
| `crates/js/src/dom.rs` | 817 | `_lumen_sw_activate_script` binding — model for new bindings |
| `crates/js/src/dom.rs` | 823 | `spawn_sw_worker` call site |
| `crates/js/src/push_api.rs` | 1 | Push API JS shim (Phase 0) |
| `crates/js/src/background_sync.rs` | 1 | Background Sync shim (Phase 0) |
| `crates/js/src/lib.rs` | 983 | `init_push_api` call in context init |
| `crates/js/src/lib.rs` | 969 | `init_background_sync` call in context init |
| `crates/network/src/lib.rs` | 2874 | SW intercept in `fetch_page` |
| `crates/network/src/lib.rs` | 2939 | SW intercept in `fetch_page_streaming` |
| `crates/network/src/lib.rs` | 3000 | SW intercept in `NetworkTransport::fetch` |
| `crates/network/src/lib.rs` | 3121 | SW intercept in `fetch_sync` (JS `fetch()`) |
| `crates/network/src/lib.rs` | 2116 | `HttpClient` — `interceptor` field |
| `crates/network/src/lib.rs` | 2201 | `HttpClient::with_interceptor` builder method |
| `crates/storage/src/sw_interceptor.rs` | 1 | `ServiceWorkerInterceptor` — SQLite-backed `FetchInterceptor` |
| `crates/storage/src/service_workers.rs` | 1 | SW registration SQLite table |
| `crates/storage/src/sw_store.rs` | 1 | `SwStore` — JSON snapshot backend for JS registration state |
| `crates/storage/src/push_subscriptions.rs` | 1 | `PushSubscriptions` SQLite store |
| `crates/storage/src/cache_storage.rs` | 352 | `CacheBackend` impl |
| `crates/shell/src/main.rs` | 730 | SW interceptor installation (PH3-20 wiring) |
| `crates/shell/src/main.rs` | 3062 | `SW_FETCH_INTERCEPTOR` session-global OnceLock |
| `crates/shell/src/main.rs` | 5396 | `sw_worker_store` field in session state |
| `crates/core/src/ext.rs` | `~3500` | `SwPushMessage` struct [proposed — P3] |
| `crates/core/src/ext.rs` | `~3501` | `SwSyncMessage` struct [proposed — P3] |
| `crates/core/src/ext.rs` | `~1507` | `FetchResponse` struct (status + headers + body) [proposed — P4] |
| `crates/storage/src/lib.rs` | `~97` | `sync_tags` module [proposed — P3] |

---

## Steps

### Phase A — Response metadata propagation (P4, prerequisite for correctness)

1. Add `FetchResponse { status: u16, headers: Vec<(String,String)>, body: Vec<u8> }` to
   `crates/core/src/ext.rs`.
2. Change `FetchInterceptor::intercept` return type from `Option<Vec<u8>>` to
   `Option<FetchResponse>`.
3. Update `ServiceWorkerInterceptor`, `InMemoryFetchInterceptor`, and all four
   call sites in `crates/network/src/lib.rs`.
4. Propagate status/headers back into the `JsFetchResult` returned to JS code.
5. In `dispatch_fetch` (`sw_worker.rs:101`) write status and headers into JS
   globals so the SW's `new Response(body, {status, headers})` is correct.

### Phase B — Request body in SW intercept (P3)

1. Add `body: Option<Vec<u8>>` and `content_type: String` to `SwFetchRequest`
   (`crates/core/src/ext.rs:3494`).
2. Populate them from `fetch_with_body_sync` call site in
   `crates/network/src/lib.rs:3173`.
3. Pass to `dispatch_fetch` in `sw_worker.rs`; expose as `event.request.body`
   via a JS `ReadableStream` stub or `arrayBuffer()` method on the `Request`
   object.

### Phase C — Background sync queue (P3)

1. Create `crates/storage/src/sync_tags.rs` — SQLite table
   `sw_sync_tags(origin, scope, tag, created_at)`; implement `register(origin, scope, tag)`,
   `pending(origin, scope) -> Vec<String>`, `remove(origin, scope, tag)`.
2. Register `_lumen_sw_sync_register(tag)` native binding in
   `crates/js/src/dom.rs` near line 817. Write to `SyncTags` store via a new
   trait in `lumen-core::ext` (same pattern as `SwBackend`).
3. Add `SwSyncMessage { tag: String }` to `crates/core/src/ext.rs`.
4. Extend `SwWorkerHandle` with a `sync_tx: mpsc::Sender<SwSyncMessage>` channel.
5. Add `_sw_fire_sync(tag)` JS helper in `sw_worker.rs` / `install_sw_globals`;
   update the message loop to handle `SwSyncMessage` by calling it.
6. In shell (`crates/shell/src/main.rs`): on each page navigation or
   `online` event, iterate `SyncTags::pending` for the current origin and
   dispatch `SwSyncMessage` to all matching SW threads. Remove the tag on
   successful resolution.
7. Wire `_lumen_sw_get_tags` binding in `dom.rs` to read from `SyncTags` store.

### Phase D — Push event delivery (P3)

1. Register `_lumen_push_subscribe(endpoint, userVisibleOnly)` native binding in
   `crates/js/src/dom.rs` near line 817. Write to `PushSubscriptions` SQLite
   store (`crates/storage/src/push_subscriptions.rs`).
2. Register `_lumen_push_unsubscribe(endpoint)` binding in the same location.
3. Add `SwPushMessage { payload: Vec<u8> }` to `crates/core/src/ext.rs`.
4. Extend `SwWorkerHandle` with a `push_tx: mpsc::Sender<SwPushMessage>` channel
   (or add a `SwMessage` enum combining push and sync).
5. Add `_sw_fire_push(payloadBase64)` JS helper in `sw_worker.rs`; update the
   message loop to handle `SwPushMessage`.
6. Add a test delivery path (shell CLI flag `--deliver-push <origin> <scope> <payload>`)
   that looks up the matching SW handle in `SwWorkerStore` and sends a
   `SwPushMessage`. This is the testable surface; full VAPID push service
   integration is deferred to a sub-task.

### Phase E — Scope matching correctness (P3)

1. Fix `ServiceWorkerInterceptor::intercept` (`sw_interceptor.rs:65`) to match
   on the full serialized URL (`scheme://host/path`) rather than `url.path()`
   alone.
2. Reject cross-origin fetch interception (SW origin must match request origin).
3. Add unit tests for same-origin and cross-origin scope matching edge cases.

### Phase F — SW lifecycle correctness (P4)

1. Honour `waitUntil(promise)` in `install` and `activate` events: pause the
   `install → activate` transition until the promise resolves before processing
   incoming fetches. The current implementation fires both events and then
   immediately starts the message loop (`sw_worker.rs:75–94`).
2. Implement `clients.matchAll()` with a real window-client lookup via a
   shared registry in shell state.
3. Implement correct `navigator.serviceWorker.ready` resolution: the promise
   should resolve only after the SW reaches `activated` state, not immediately.

---

## Dependencies / open questions

- **rquickjs argument limit.** `rquickjs::IntoJsFunc` is limited to 7 arguments.
  The `_lumen_cache_put` binding in `dom.rs:840` works around this by grouping
  parameters into a single JSON string. New bindings must follow the same
  pattern or use a struct serialised as JSON.
- **`SwWorkerHandle` channel design.** Adding separate `push_tx` and `sync_tx`
  fields is simple but means two `mpsc::Sender` clones per worker lookup.
  An enum `SwMessage { Fetch(SwFetchRequest), Push(SwPushMessage), Sync(SwSyncMessage) }`
  over a single channel is cleaner; either approach is acceptable.
- **Push service integration.** Full end-to-end Web Push (RFC 8030 VAPID, P-256
  key exchange, encrypted payload) requires either `p256` + `hkdf` crates or a
  push gateway proxy. This is not in scope for Phase 3 initial; the loopback
  test fixture (Phase D step 6) unblocks integration testing.
- **SW update check timing.** The spec requires checking for SW script updates
  on navigation and after 24 hours. The current implementation never re-fetches
  the SW script after initial registration. This is a known deferred gap.
- **Multi-context isolation.** Each SW runs in its own QuickJS `Runtime` +
  `Context`, fully isolated. This is correct per spec. The `clients` API requires
  cross-context messaging, which is not yet implemented beyond the stub.
- **`fetch()` inside SW.** The SW execution context currently has no `fetch()`
  global. Implementing it requires passing a `JsFetchProvider` (or a sub-channel
  to the shell's `HttpClient`) into `spawn_sw_worker`.

---

## Tests

### Unit tests (per crate)

- `crates/storage/` — `sync_tags`: register, list, remove; duplicate registration
  is idempotent; origin partitioning (tags for origin A not visible to origin B).
- `crates/storage/` — `push_subscriptions`: `subscribe` / `get_by_scope` / `unsubscribe`
  round-trip; `list_for_origin` respects partitioning.
- `crates/network/` — `interceptor_tests` (existing at `lib.rs:6727`): add a
  case that returns a `FetchResponse` with non-200 status and verify it
  propagates to the caller.
- `crates/js/sw_worker` — `dispatch_fetch` returns `None` when SW does not call
  `respondWith`; returns correct body bytes when it does; times out after
  `FETCH_TIMEOUT` (`sw_worker.rs:17`).

### Integration tests (lumen-driver / shell)

- Register a SW with a `fetch` handler that returns a synthetic response; load a
  page under its scope; assert the response body matches the SW handler, not the
  network.
- Register a sync tag; trigger a sync delivery; assert the `sync` event handler
  ran and the tag was removed from the store.
- Subscribe to push; deliver a push message via `--deliver-push`; assert the
  `push` event handler received the correct payload.

### Graphic tests

No visual output — SW is a background-runtime feature. Functional coverage via
driver integration tests is sufficient.

---

## Definition of done

- [ ] `FetchInterceptor::intercept` returns `FetchResponse` (status + headers + body);
      all four call sites in `network/lib.rs` propagate status to callers.
- [ ] `SwFetchRequest` carries request body; `fetch_sync` populates it for POST/PUT.
- [ ] `ServiceWorkerInterceptor` scope-matching uses full URL, not just path.
- [ ] Background sync: `SyncTags` SQLite table; `_lumen_sw_sync_register` binding wired;
      `sync` event dispatched to SW thread on navigation; tag removed on success.
- [ ] Push: `_lumen_push_subscribe` / `_lumen_push_unsubscribe` bindings wired to
      `PushSubscriptions` store; `--deliver-push` CLI fixture dispatches `push` event to SW.
- [ ] `waitUntil()` in `install` / `activate` correctly defers the `activate` event.
- [ ] `cargo clippy -p lumen-js -p lumen-network -p lumen-storage --all-targets -- -D warnings` clean.
- [ ] All new unit tests pass; driver integration tests for fetch intercept, sync,
      and push all pass.
- [ ] `CAPABILITIES.md` updated with Service Worker subsystem entry.

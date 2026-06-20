# lumen-js

Crate providing the `JsRuntime` implementation backed by QuickJS via `rquickjs` v0.11.
Phase 0–1 engine; `rusty_v8` is planned for v1.0+.

## Scope

- `QuickJsRuntime` struct: wraps `rquickjs::Runtime + Context` under a `Mutex`.
- Implements `lumen_core::JsRuntime`: `eval`, `set_global`, `get_global`, `call_function`.
- JSON-compatible value conversion: `JsValue ↔ rquickjs::Value<'js>`.
- Shell wires it in via `features = ["quickjs"]`; without the feature `NullJsRuntime` is used.

## Done

- **QuickJS runtime on a dedicated thread (B-1, ADR-014).** 2026-06-19. `QuickJsRuntime` is now a *handle*: a `lumen-js` thread owns `Inner { Runtime, Context }` for its whole life (created and dropped there, since both are `!Send`); the handle holds a bounded `SyncSender<JsCommand>` (keeps it `Sync`) + the existing `Arc` output channels. Every QuickJS access goes through the private `run(f)` choke point — ships `f` as `JsCommand::Run(Box<dyn FnOnce(&Inner)+Send>)` and blocks on a reply, so `f` may borrow the caller's stack (one documented `unsafe` lifetime-erasure, sound because `run` blocks until the job finishes). The two unsound `unsafe impl Send/Sync` are deleted — the handle is genuinely `Send + Sync`. `Drop` sends `Shutdown` + joins. Behaviour unchanged (callers still block); this *relocates* the runtime so BUG-171 can drive it off the UI thread. `js_thread_main` teardown calls `wasm::clear_registry()` before `Inner` drops → closes BUG-222. `pointer_lock` + `file_input` token registry moved from `thread_local` → process-global `Mutex` (shell writes on UI thread, JS reads on JS thread); per-runtime JS-thread-only registries (canvas2d/webgl/offscreen/wasm/subtle_crypto/capture) kept.
- `QuickJsRuntime` — all four trait methods, 16 tests (eval, globals, function call, round-trip, Send+Sync). 2026-05-20.
- `call_function` dynamic-args workaround: temporary global `__lum_args__` + `fn.apply(null, __lum_args__)` eval. Reason: `rquickjs 0.11` `Function::call` requires fixed-size `IntoArgs` tuples; no `apply()` method.
- `lumen-shell` feature `quickjs` enables `QuickJsRuntime` via `run_scripts_with_dom()`.
- **JS↔DOM bindings Phase 0** (`install_dom_api`, `crates/js/src/dom.rs`). 2026-05-20.
  - 24 native `_lumen_*` Rust functions exposed to QuickJS.
  - JS Web API shim: `console`, `document`, `window`, `alert`, `setTimeout` (synchronous).
  - DOM read: `getElementById`, `querySelector`, `querySelectorAll`, `getAttribute`, `tagName`, `textContent`, `parentElement`, `children`.
  - DOM write: `setAttribute`, `removeAttribute`, `textContent =`, `innerHTML =`, `createElement`, `createTextNode`, `appendChild`, `removeChild`.
  - `document.title` get/set.
  - querySelector uses full CSS3 selector engine (lumen_layout::query_all): compound selectors, combinators ( > + ~), pseudo-classes, attribute selectors. element.matches() and element.closest() use per-node matches_selector. (P2 2026-06-03)
  - 19 DOM tests + 16 runtime tests = 35 total. All pass.
  - Shell integration: `run_scripts_with_dom` wraps `Document` in `Arc<Mutex<>>`, calls `install_dom`, drops runtime to release Arc clones, recovers `Document`.
- **Fetch API JS shim** (`install_dom_api`, `crates/js/src/dom.rs`). 2026-05-22.
  - 5 native `_lumen_fetch_*` bindings: `_lumen_fetch_sync`, `_lumen_fetch_get_status`, `_lumen_fetch_get_status_text`, `_lumen_fetch_get_headers`, `_lumen_fetch_get_body`. Shared result via `Arc<Mutex<Option<FetchCache>>>`.
  - `install_dom` now accepts `Option<Arc<dyn JsFetchProvider>>` — `None` makes `fetch()` reject immediately.
  - JS classes: `AbortSignal`, `AbortController`, `Headers`, `Response`, `Request`, `fetch()` global + `window.fetch`.
  - `Response.ok` (200–299), `Response.text()` / `Response.json()` returning Promises, `Headers` case-insensitive get/set/has/delete.
  - `AbortController.abort()` sets `signal.aborted = true`.
  - 109 JS tests (was 35 before). All pass.
  - **AA-4 (2026-06-12):** `AbortSignal.abort(reason)` / `AbortSignal.timeout(ms)` (TimeoutError via setTimeout shim) / `AbortSignal.any(signals)` statics per DOM §3.2.2. `any()` adopts the first aborting source's reason and detaches listeners once the race is decided. `onabort` handler fires alongside `addEventListener` listeners (shared `_lumen_abort_signal_fire` helper). `fetch()` pre-flight check: already-aborted `init.signal`/`Request.signal` rejects with `signal.reason` before any network call (Fetch §4.1 step 13; no in-flight abort — fetch is synchronous).
  - **AA-5 (2026-06-12):** Trusted Types per W3C TT L2 (`crates/js/src/trusted_types.rs`, rewrite of the A-9 stub). `createPolicy(name, rules)` invokes the policy's own rule callbacks (missing rule → `TypeError`); `"default"` registers `trustedTypes.defaultPolicy` exactly once (second registration throws — DefaultPolicy guard); duplicate non-default names allowed (no CSP in Phase 0). `TrustedHTML`/`TrustedScript`/`TrustedScriptURL`/`TrustedTypePolicy` are non-constructible from page script (construction token + WeakMap brand; `isHTML`/`isScript`/`isScriptURL` reject forged prototypes). Added `emptyHTML`/`emptyScript` and `getAttributeType`/`getPropertyType` sink tables. No sink enforcement (Phase 0). Non-spec `TrustedURL`/`getPolicy`/`getPolicyNames` removed. 8 new + 11 updated unit tests.
- **Web Storage API** (`install_dom_api`, `crates/js/src/dom.rs`). 2026-05-25.
  - 12 native `_lumen_ls_*` / `_lumen_ss_*` bindings (length, key, get, set, remove, clear for localStorage + sessionStorage).
  - `install_dom` now accepts `ls_store: Option<Arc<Mutex<WebStorage>>>` — `None` → fresh in-memory store.
  - `_lumen_make_storage` JS factory + `localStorage`/`sessionStorage` globals in shim. `length` property via `Object.defineProperty` with getter.
  - `sessionStorage` — fresh `Arc::new(Mutex::new(WebStorage::default()))` per `install_dom` call (page-load isolation).
  - `localStorage` — shared `Arc<Mutex<WebStorage>>` from shell (SOP-partitioned, persists across reloads within session).
  - 8 new tests (getItem/setItem/removeItem/clear/key/length/overwrite/session-isolation). 140 JS tests total. All pass.
- **URL / URLSearchParams / performance / queueMicrotask** (`crates/js/src/dom.rs`). 2026-05-25.
  - `_lumen_now_ms()` — native Rust function: `SystemTime::now()` as f64 milliseconds since Unix epoch.
  - `URLSearchParams` (WHATWG URL §5): parse from string/object/array, `get/getAll/set/append/delete/has/sort/size/toString/forEach/keys/values/entries`.
  - `URL` (WHATWG URL §6.1): parse absolute URLs, resolve relative URLs and protocol-relative against a base (dot-segment normalization per RFC 3986 §5.2.4). Properties: `href/protocol/hostname/host/port/pathname/search/hash/origin/username/password/searchParams` (lazy). `URL.createObjectURL` / `revokeObjectURL` stubs.
  - `performance` (W3C HR Time L2): `now()` (DOMHighResTimeStamp, time origin captured at `install_dom` call), `timeOrigin`, stub `mark/measure/getEntriesByName/getEntriesByType/clearMarks/clearMeasures`. Exposed on `window.performance`.
  - `queueMicrotask(fn)` (HTML LS §8.1.4.4): schedules via `Promise.resolve().then(fn)`; throws `TypeError` for non-function.
  - All four APIs exposed on `window.*` via post-literal assignment (avoids `var` hoisting issue with `performance`).
  - 42 new tests. 166 JS tests total. All pass.
- **DOM dirty flag / layout invalidation** (`QuickJsRuntime::dom_dirty: Arc<AtomicBool>`). 2026-05-25.
  - `dom_dirty` set to `true` by all DOM-mutating bindings: `_lumen_set_attr`, `_lumen_remove_attr`, `_lumen_set_text_content`, `_lumen_set_inner_html`, `_lumen_append_child`, `_lumen_remove_child`.
  - `QuickJsRuntime::take_dom_dirty() -> bool` — atomic swap(false); cleared after each rAF pass in the shell.
  - Shell: `PersistentJs::take_dom_dirty()` added to trait; `RedrawRequested` step 6 checks flag and calls `self.relayout()` when set.
  - Result: JS DOM mutations (textContent, setAttribute, appendChild, etc.) now cause an automatic relayout before the next paint, making interactive JS pages reflect DOM changes correctly.
- **Async setTimeout / setInterval / clearTimeout / clearInterval + scheduler.postTask** (`crates/js/src/dom.rs`). 2026-05-25.
  - `_lumen_request_wakeup(deadline_ms: f64)` — native Rust function: writes the earliest timer deadline (Unix epoch ms) to `QuickJsRuntime::timer_wakeup: Arc<Mutex<Option<f64>>>`. Stores only the minimum deadline (min-update semantics).
  - JS timer queue (`_lumen_timers`) — plain JS array `{id, fn, deadline, interval}`. `setTimeout`/`setInterval` append; `clearTimeout`/`clearInterval` splice; `_lumen_tick_timers()` drains expired entries, reschedules intervals, runs callbacks, and calls `_lumen_request_wakeup` for the next timer.
  - Shell integration: `PersistentJs::tick_timers()` + `take_timer_wakeup()` — called in `about_to_wait`; if a timer deadline is pending, sets `ControlFlow::WaitUntil` so winit wakes up precisely at the next expiry without polling.
  - `scheduler` (W3C Prioritized Task Scheduling API): `postTask(fn, {priority?, delay?}) → Promise` (delay maps to `setTimeout`; priority ignored — Phase 2); `yield() → Promise` (defers via `setTimeout 0`). Exposed on `window.scheduler`.
  - Old synchronous stubs replaced. Timers are now correctly deferred: `setTimeout(fn, 0); x` evaluates `x` before `fn` runs.
  - 6 new tests (deferred, fires-after-tick, clearTimeout, setInterval repeat, clearInterval, scheduler.postTask). 172 JS tests total. All pass.

- **`requestAnimationFrame` / `cancelAnimationFrame`** (`crates/js/src/dom.rs`). 2026-05-25.
  - `_lumen_mark_raf_pending()` native Rust function: sets `QuickJsRuntime::raf_pending: Arc<AtomicBool>` to `true` when JS calls `requestAnimationFrame`.
  - `QuickJsRuntime::take_raf_pending() -> bool` — atomic swap(false); read by shell after each rendering step.
  - JS: `requestAnimationFrame(fn)` queues `{id, fn}` into `_lumen_raf_callbacks`, calls `_lumen_mark_raf_pending()`, returns numeric ID. Returns 0 for non-function argument.
  - JS: `cancelAnimationFrame(id)` splices callback from queue; unknown ID is a no-op.
  - JS: `_lumen_run_raf_callbacks(timestamp_ms)` — snapshot-pattern (splice all, run, new callbacks go to next frame). Returns `true` when any callback ran.
  - Shell: `PersistentJs::run_animation_frame(timestamp_ms)` calls `_lumen_run_raf_callbacks`; `take_raf_pending()` detects animation loops and requests next redraw.
  - Shell integration: in `RedrawRequested` step 5.1 — after Rust rAF, before CSS animation tick; new rAF registered during callbacks automatically triggers next frame.
  - `window.requestAnimationFrame` and `window.cancelAnimationFrame` both wired.
  - 11 new tests (id, sequential ids, non-function→0, mark-pending, snapshot-pattern, recursive-pending, cancel, cancel-unknown, window properties). 183 JS tests total.

- **MutationObserver / ResizeObserver / IntersectionObserver + getBoundingClientRect** (`crates/js/src/dom.rs`). 2026-05-26.
  - `_lumen_get_bounding_rect(nid: u32) -> Option<Vec<f64>>` — Rust binding backed by `Arc<Mutex<HashMap<u32,[f32;4]>>>` populated by shell after each `relayout_page`. Returns `[x, y, width, height]` in CSS px.
  - `_lumen_get_viewport_size() -> Vec<f64>` — Rust binding backed by `Arc<Mutex<[f32;2]>>` updated by shell on window resize.
  - `MutationObserver` (WHATWG DOM §4.3.2): `observe(target, options)` with full options normalization (`childList`, `attributes`, `attributeFilter`, `attributeOldValue`, `characterData`, `characterDataOldValue`, `subtree`); `disconnect()`; `takeRecords()`. `_mo_notify(nid, type, ...)` fires from primitive wrappers, delivers via `_lumen_flush_mutation_observers()` (sync) and `queueMicrotask` (async production path).
  - `ResizeObserver` (W3C): `observe(target)`, `unobserve(target)`, `disconnect()`. `_lumen_deliver_resize_observers()` delivers only if width/height changed by >0.5 px. Shell calls it after `relayout_page`.
  - `IntersectionObserver` (WICG): `observe(target)`, `unobserve(target)`, `disconnect()`. `_lumen_deliver_intersection_observers()` intersects element rect with root expanded by `rootMargin` (`_parse_root_margin` supports `px` shorthand 1–4 values), delivers full `IntersectionObserverEntry` shape with threshold crossing semantics. Shell calls it after `relayout_page`.
  - `element.getBoundingClientRect()` wired via `_lumen_get_bounding_rect`.
  - 17 new tests (getBoundingClientRect, MutationObserver attribute/childList/subtree/disconnect/takeRecords, ResizeObserver fire/fire-on-resize/no-fire-same-size/unobserve/disconnect, IntersectionObserver fire/not-visible/threshold/multiple/unobserve/disconnect). **200 JS tests total.**

- **loading=lazy via IntersectionObserver** (`crates/js/src/dom.rs`). 2026-05-29.
  - `_lumen_init_lazy_images(pairs)` now creates an internal `IntersectionObserver` (`_lazy_io`) with `rootMargin: 0px 0px Mpx 0px` where `M = viewport height` (HTML LS lazy-loading distance threshold: 1 viewport ahead). Observes each image via a proxy object `{__nid__: nid}`.
  - The IO callback calls `_lumen_request_lazy_image_load` for intersecting images and calls `unobserve` after first load (each image loaded exactly once).
  - `_lumen_deliver_lazy_images()` is now a no-op; delivery happens inside `_lumen_deliver_intersection_observers()` called by `deliver_layout_observers()` in shell — images and site IO observers fire on the same pass.
  - `JsRuntime::resume()` stub added to `QuickJsRuntime` (returns error; full snapshot restore deferred — BUG-042).
  - `SuspendedHeap` re-exported from `lumen_core` (was missing from `pub use ext::{…}` in core's lib.rs).
  - 7 new tests (lazy via IO, within margin below fold, not-queued far below, removed after load, idempotent init, deliver-lazy-images is noop, rootMargin 1/2/4 values, rootMargin expands/doesn't expand viewport). **244 JS tests total.**

- **IndexedDB (W3C Indexed Database API 3.0)** (`WEB_API_SHIM`, `crates/js/src/dom.rs`). 2026-05-29.
  - Pure-JS in-memory implementation (no native bindings): `indexedDB` (`open`/`deleteDatabase`/`databases`/`cmp`), `IDBOpenDBRequest`/`IDBRequest`, `IDBDatabase` (`createObjectStore`/`deleteObjectStore`/`transaction`/`close`/`objectStoreNames`), `IDBTransaction` (`objectStore`/`abort`/`oncomplete`/`onabort`, auto-commit), `IDBObjectStore` (`add`/`put`/`get`/`getKey`/`getAll`/`getAllKeys`/`count`/`delete`/`clear`/`createIndex`/`deleteIndex`/`index`/`openCursor`/`openKeyCursor`), `IDBIndex` (`get`/`getKey`/`getAll`/`getAllKeys`/`count`/`openCursor`/`openKeyCursor`), `IDBCursor`/`IDBCursorWithValue` (`continue`/`advance`/`update`/`delete`), `IDBKeyRange` (`only`/`bound`/`lowerBound`/`upperBound`/`includes`).
  - Key support: number, string, Date, and array keys with spec ordering (`number < date < string < array`); dotted + array key paths; `autoIncrement` key generators (in-line and out-of-line); unique + `multiEntry` indexes (index entries materialised per query by scanning records).
  - Deferred execution model: each request's data read/write runs at **dispatch time in FIFO order** within its transaction; transactions flush in creation order. This gives correct intra- and inter-transaction ordering (e.g. a readonly transaction created after a readwrite one sees the latter's committed writes). `request.result` is only valid once the `success` event fires.
  - Event delivery: `success`/`error`/`upgradeneeded`/`complete`/`abort` fire via `_lumen_idb_flush()`, scheduled by `queueMicrotask` and callable directly by the shell each tick and by tests (mirrors the raf / MutationObserver pattern). An unhandled request error (no `preventDefault`) aborts its transaction.
  - **Persistence (2026-05-29):** databases survive page reload via the `IdbBackend` trait (`lumen-core::ext`), supplied to `install_dom`. On shim init `_lumen_idb_load()` restores the per-origin snapshot into the JS heap; after every mutating flush (`txn.mode !== 'readonly'`, version upgrade, or `deleteDatabase`) `_lumen_idb_persist(snapshot)` writes it back. The snapshot is the whole `_idb_databases` set as tagged JSON — Date keys/values encoded as `{__idb_date__: ms}` (JSON has no Date type), everything else plain structured data. Read-only transactions never re-persist (`_idb_dirty` flag gates it). When no backend is installed (unit tests / sandboxed contexts) the `typeof _lumen_idb_persist === 'function'` guards keep it in-heap-only. Backend impl: `lumen_storage::IdbStore` over `StorageBackend` (in-memory or SQLite), origin-partitioned under key `__indexeddb__`.
  - 23 tests (open+upgrade, keyPath/autoIncrement CRUD, put-overwrite, duplicate→abort, getAll ordering + key range, delete/clear, index get/getAll, unique-index violation, cursor forward/reverse/update/delete, IDBKeyRange.includes, cmp, version downgrade error, deleteDatabase, second-connection persistence; + persistence: reload round-trip, version restore, Date round-trip, delete-database restore, read-only no-persist). **267 JS tests total.**

- **Service Worker API stub** (`crates/js/src/dom.rs` + `lumen-storage/src/sw_store.rs`, §8E). 2026-06-01.
  - `navigator.serviceWorker` → `ServiceWorkerContainer`: `register(url, opts?)`, `unregister(scope)`, `getRegistration(url)`, `getRegistrations()`, `ready` Promise, `addEventListener('message'/'controllerchange')`.
  - `ServiceWorkerRegistration`: `scope`, `installing`/`waiting`/`active` worker slots, `update()`, `unregister()`, `addEventListener('updatefound')`.
  - `ServiceWorker`: `scriptURL`, `state` (`installing→installed→activating→activated`), `postMessage()`, `addEventListener('statechange')`, EventTarget mixin.
  - Lifecycle driven by `_sw_run_lifecycle(reg)`: `setTimeout`-based state machine fires `install` on the worker, then `activate`; `statechange` events emitted at each transition.
  - Persistence via `SwBackend` trait (`lumen-core::ext:1530`): `_lumen_sw_persist(origin, snapshot)` / `_lumen_sw_load(origin)` / `_lumen_sw_unregister(origin, scope)` Rust bindings. `SwStore` impl in `lumen-storage` (JSON snapshot under key `__sw_registrations__`, origin-partitioned, same pattern as `IdbStore`).
  - Shell: `sw_store_for_base(base, backend)` extracts origin → `SwStore::new()` → passed as 7th arg to `install_dom`.
  - `install_dom` / `run_scripts_with_dom` got `#[allow(clippy::too_many_arguments)]` (8 params).
  - 10 unit tests: register/resolve Promise, state progression, persist no-throw, duplicate scope, getRegistration, unregister, getRegistrations, ready, multiple-scope isolation. **623 JS tests total.**

- **`Intl` i18n shim** (`crates/js/src/intl_bindings.rs`, ECMA-402; §91). 2026-06-02.
  - Pure-JS shim (no native bindings) installed last in `install_dom` — QuickJS (default rquickjs features) ships without ECMA-402, so `Intl` was `undefined`. Defers to a native `Intl` if a future feature-enabled build provides one. Re-exported as `window.Intl`.
  - Two locales: `en-US` (default fallback for any non-Russian tag) and `ru-RU` (matched on `ru`/`ru-*`). `Intl.NumberFormat` (decimal/currency/percent; `en-US` `,`/`.`, `ru-RU` NBSP/`,`; currency symbols USD/EUR/RUB/GBP/JPY/KRW/CNY; min/max fraction digits, grouping). `Intl.DateTimeFormat` (year/month/day/weekday/hour/minute/second; locale month+weekday names; ru genitive month + "г." suffix when a day is present; default short date `M/D/YYYY` vs `DD.MM.YYYY`; `hour12` defaults true for en, false for ru). `Intl.Collator` (locale compare placing `ё` after `е`; `sensitivity:'base'` case-insensitive; `numeric:true`). `Intl.PluralRules` (CLDR cardinal+ordinal; ru resolves one/few/many/other). Plus `getCanonicalLocales` / per-constructor `supportedLocalesOf` / `resolvedOptions`.
  - `Number.prototype.toLocaleString` and `Date.prototype.toLocaleString`/`toLocaleDateString`/`toLocaleTimeString` are rerouted through the matching `Intl` constructor.
  - 19 unit tests (grouping, currency incl. negative sign placement, percent, fraction limits, default+long date, ru genitive, hour12 vs 24h, Cyrillic+numeric+base collation, ru/en plural categories, toLocaleString delegation, locale fallback, supportedLocalesOf). **812 JS lib tests total.**

- **WebAuthn / `navigator.credentials`** (`crates/js/src/credentials.rs`, W3C WebAuthn L2). 2026-06-02.
  - `navigator.credentials.create(options)` / `.get(options)` (Promise-based), `preventSilentAccess`/`store` stubs; `PublicKeyCredential` (+ `.isUserVerifyingPlatformAuthenticatorAvailable()` → real provider answer, `.isConditionalMediationAvailable()` → false), `CredentialsContainer`, `Credential`, `AuthenticatorResponse`/`AuthenticatorAttestationResponse`/`AuthenticatorAssertionResponse` constructors (so RP `instanceof` checks work; response objects carry the right prototype). Credential has `id`/`rawId`/`type`/`authenticatorAttachment`/`response`/`getClientExtensionResults()`/`toJSON()`; attestation response exposes `attestationObject`/`clientDataJSON` + `getAuthenticatorData()`/`getPublicKey()`/`getPublicKeyAlgorithm()`/`getTransports()`; assertion response exposes `authenticatorData`/`signature`/`clientDataJSON`/`userHandle`.
  - Marshalling avoids `serde_json`: the request is packed into one `|`-separated string of base64url fields (rp/user/challenge/origin text encoded via `strToB64url`, buffers via `bufToB64url`; algs as decimal CSV, exclude/allow as base64url CSV) — base64url's alphabet contains neither `|` nor `,`. The response is a small hand-built JSON object (base64url / numbers / fixed strings only), so JS `JSON.parse` is safe.
  - Native bindings: `_lumen_webauthn_create(packed)→json`, `_lumen_webauthn_get(packed)→json`, `_lumen_webauthn_uvpa()→bool`. All forward to the process-global `CredentialProvider` installed via `lumen_js::set_credential_provider` (mirrors `clipboard`). No provider → `{ok:false,error:"NotAllowedError"}`, so the promise rejects with `NotAllowedError` (privacy-preserving "no authenticator" default). Shell wiring (P3): install a `lumen_network::VirtualAuthenticator` at startup.
  - 6 unit tests (base64url roundtrip incl. `-`/`_`, UTF-8 text decode, CSV parsing, no-provider rejection, full create+get through an installed double) + 4 e2e tests in `crates/js/tests/webauthn_credentials.rs` (full QuickJS runtime: `navigator.credentials` shape, create→`PublicKeyCredential` with correct ArrayBuffers/prototypes/accessors + unpacked request assertions, get→assertion, missing-publicKey → `NotSupportedError`). **769 JS lib tests + 10 webauthn tests.**

- **Broadcast Channel API** (`crates/js/src/broadcast_channel.rs`, WHATWG HTML §9.5). 2026-06-02.
  - `new BroadcastChannel(name)`, `postMessage(message)`, `close()`, `onmessage`/`onmessageerror`, `addEventListener`/`removeEventListener`/`dispatchEvent`.
  - Routing via a process-global `BroadcastHub` (`static OnceLock<Mutex<…>>`) keyed by channel name, holding one `mpsc::Sender<String>` per live instance. `post` clones the JSON payload to every same-name sender except the sender itself (spec: senders never receive their own messages), pruning dead receivers on send failure.
  - Each runtime owns a `BroadcastRegistry` (`Arc<Mutex<Vec<LocalChannel>>>`) of receiver halves; `QuickJsRuntime::pump_broadcast_channels()` drains them and calls `_lumen_deliver_broadcast_messages(msgs)` in JS (delivery payload reuses `build_worker_messages_json`, so `m.json` arrives already-parsed — no double `JSON.parse`). Cross-thread/cross-context delivery works because the hub is process-global.
  - Native bindings: `_lumen_bc_register(name)→u32`, `_lumen_bc_post(id, name, json)`, `_lumen_bc_close(id, name)`. Installed after the DOM shim (needs `MessageEvent`, `DOMException`).
  - Shell wiring: `PersistentJs::pump_broadcast_channels()` called in `about_to_wait` alongside `pump_workers()`.
  - 14 unit tests (constructor/name stringify, missing-arg throw, same-name delivery, no-self-delivery, name isolation, addEventListener/removeEventListener, closed-channel stops receiving, post-on-closed throws, MessageEvent type, 3-way fan-out, structured-data round-trip, window-exposed). **752 JS tests total.**

- **Configurable navigator profile** (`crates/js/src/navigator_bindings.rs`, ADR-007 Layer 4, 9D.6 / 9F.1). 2026-06-02.
  - `NavigatorProfile` struct (hardware_concurrency / device_memory / platform / languages / screen_width / screen_height / color_depth / timezone_offset). `Default` reproduces the previous hardcoded mid-tier values (2 cores, 8 GiB, Win32, en-US/en, 1920×1080, depth 24, UTC), so behaviour is unchanged without a config.
  - Process-global override: `set_navigator_profile(profile)` (shell calls it once at startup from `fingerprint.toml`); `current_navigator_profile()` reads it (default if unset). No-arg `install_navigator_bindings(ctx)` uses the global; `install_navigator_bindings_with(ctx, &profile)` ignores the global (used by tests + explicit callers).
  - The JS shim is now built dynamically from the profile (`build_navigator_shim`): locales JSON-escaped (`json_string`), empty `languages` falls back to `["en-US"]`, `getTimezoneOffset()` returns the configured minutes.
  - Wiring: `lib.rs` re-exports `NavigatorProfile` + `set_navigator_profile`; the shell's `config::FingerprintProfile::install_navigator()` builds and installs the profile.
  - 11 unit tests (9 default-value assertions via `_with(default)` to stay isolated from the process-global + custom-profile-applies-all-fields, empty-languages-fallback, quote-escape-safety, default-matches-legacy, set/read global).

- **AudioContext fingerprint noise** (`crates/js/src/audio_bindings.rs`, ADR-007 Layer 4, 9D.3). 2026-05-30.
  - New module `audio_bindings`: `install_audio_bindings(ctx, seed)` + `new_session_seed()`.
  - JS shim (IIFE): defines `globalThis.AudioContext`, `webkitAudioContext`, `OfflineAudioContext`, `AudioBuffer`.
  - Per-session LCG noise (±1e-7) baked into `AudioBuffer.getChannelData()`, `copyFromChannel()`, and `AnalyserNode.getFloatFrequencyData()` — prevents audio fingerprinting while preserving API shape.
  - `SESSION_COUNTER: AtomicU32` ensures each `install_audio_bindings` call gets a unique seed; seed captured in JS closure at IIFE evaluation time.
  - `install_dom()` calls `new_session_seed()` + `install_audio_bindings()` after WebGL bindings.
  - 14 unit tests (`install_succeeds`, `audio_context_is_defined`, `webkit_audio_context_alias`, `offline_audio_context_is_defined`, `audio_buffer_is_defined`, `audio_buffer_get_channel_data_length`, `audio_buffer_noise_is_tiny`, `different_seeds_produce_different_noise`, `audio_context_state_transitions`, `analyser_frequency_data_length`, `offline_audio_context_start_rendering_returns_thenable`, `offline_audio_context_length_matches_constructor`, `session_seeds_are_unique`, `session_seeds_monotonically_increase`). **280 JS tests total** (14 new audio + 266 previously passing).

- **Functional WebGL context** (`crates/js/src/webgl_canvas.rs`, §7F, task #28). 2026-06-02.
  - `install_webgl_canvas(ctx, &GpuFingerprint)` — registers `_lumen_webgl_*` natives + a JS shim that intercepts `document.createElement('canvas')` so `canvas.getContext('webgl'/'webgl2'/'experimental-webgl')` returns a *functional* context backed by `lumen_paint::SoftwareWebGl` (replaces the fingerprint-only `webgl_bindings` shim in `install_dom`).
  - Forwards the full documented surface: `createBuffer`/`bindBuffer`/`bufferData`, `createShader`/`shaderSource`/`compileShader`, `createProgram`/`attachShader`/`linkProgram`/`useProgram`, `getAttribLocation`/`getUniformLocation`, `enableVertexAttribArray`/`vertexAttribPointer`, `uniform4f`/`uniform4fv`/`uniform3f`, `clearColor`/`clear`, `viewport`, `drawArrays`, `readPixels` (WebGL bottom-left origin, crops + Y-flips the backend's top-left framebuffer), `getParameter`/`getExtension`/`getSupportedExtensions`. Texture calls accepted as no-ops (flat-shaded path).
  - Per-thread `SoftwareWebGl` registry keyed by opaque context id (`thread_local`), giving correct per-runtime isolation across Web Worker threads. GL objects are opaque `{__wid}` / `{__loc}` wrappers; methods unwrap either a wrapper or a raw number.
  - Preserves ADR-007 Layer 4: `getParameter(UNMASKED_VENDOR/RENDERER_WEBGL)` + `getParameter(VENDOR/RENDERER)` return normalized `GpuFingerprint` strings; `toDataURL`/`toBlob` stay blank.
  - 10 unit tests (functional object, 2d→null, context caching, fingerprint normalization, blank toDataURL, clear→readPixels roundtrip, full compile→buffer→draw→readback pipeline, attrib location, non-canvas, lose-context extension). The 19 `no_automation_markers.rs` integration tests still pass.

- **Functional Canvas 2D context** (`crates/js/src/canvas2d.rs` + `dom.rs`, HTML LS §4.12.4, task 5A.2). 2026-06-02.
  - `install_canvas2d_bindings(ctx)` registers `_lumen_canvas2d_*` natives backed by `lumen_canvas::Context2D` in a per-thread registry keyed by **DOM node index** (`__nid__`). Installed in `install_dom` right after WebGL.
  - The `getContext('2d')` shim lives in `dom.rs::_lumen_make_element` (not the WebGL-style `document.createElement` patch) because element wrappers are ephemeral — every wrapper for a node carries `getContext`/`toDataURL`/reflected `width`/`height`. The `CanvasRenderingContext2D` object is cached in the module-level `_canvas2d_ctxs[nid]` map (same persistence pattern as `_input_values`). The native key `nid` matches `LayoutBox::node.index()`, so the display list's `DrawImage src="canvas:{nid}"` resolves to the right bitmap.
  - Forwarded surface: `fillRect`/`clearRect`/`strokeRect`, `beginPath`/`moveTo`/`lineTo`/`closePath`/`arc`/`fill`/`stroke`, `rect`/`ellipse` (ellipse≈circle in Phase 0), `fillStyle`/`strokeStyle` (via `CanvasColor::from_css_str`), `lineWidth`/`globalAlpha` (spec-validated ranges), `getImageData` (applies per-session fingerprint noise via `Context2D::get_image_data`). Transforms/text/shadows/gradients/clip are accepted as no-ops/stubs. `toDataURL`/`toBlob` stay blank (ADR-007 anti-fingerprint).
  - Draw ops mark the canvas dirty; `QuickJsRuntime::flush_canvas_updates()` (→ `canvas2d::flush_dirty()`) drains `(nid, w, h, rgba)` once per frame. The shell registers each via `Renderer::register_image("canvas:{nid}", ...)` and requests a redraw. Unregistered canvases (no JS drawing, e.g. the cpu_raster snapshot driver which runs no JS) render as the transparent `DrawImage` placeholder.
  - 16 unit tests in `canvas2d.rs` (create/clamp/idempotent, fill/clear/stroke/path dirty-tracking, flush-once, line-width/alpha validation, resize, get_image_data, unknown-canvas no-ops, isolation) + 6 e2e tests in `dom.rs` (getContext object/caching, default 300×150, draw→flush, webgl→null, non-canvas→null). Graphic test `57-canvas-2d.html` + demo in `1000000-final.html`.

- **HTML Popover API** (`crates/js/src/dom.rs`, WHATWG HTML §6.12). 2026-06-03.
  - `showPopover/hidePopover/togglePopover` + `popover` getter/setter on every HTMLElement (in `_lumen_make_element`).
  - Top-layer emulation: `showPopover()` sets `data-lumen-popover-open` sentinel (read by `is_closed_popover` in `layout/box_tree.rs` to skip hidden popovers) + applies `position:fixed;z-index:2147483647` inline style. `hidePopover()` removes sentinel + restores saved style.
  - `popover="auto"` stack: opening a new auto-popover closes all previously open auto-popovers (newest-first). `popover="manual"` is independent.
  - `beforetoggle` / `toggle` events with `oldState`/`newState` fired synchronously on show and hide.
  - Click-outside capture handler: clicks outside all open auto-popovers close them from newest to oldest.
  - Escape keydown: closes topmost auto-popover (dialog modal takes priority if present).
  - `popovertarget`/`popovertargetaction` attributes on buttons: click dispatches to `showPopover`/`hidePopover`/`togglePopover` on the element with matching `id`. Default action is `toggle`.
  - Layout side: `is_closed_popover(doc, id)` in `box_tree.rs` returns `true` when `popover` attribute is set but `data-lumen-popover-open` is absent → `BoxKind::Skip` (mirrors `<details>` child hiding pattern).
  - 14 unit tests: getter/setter, show/hide/toggle, events, auto-stack, manual isolation, fixed style on show, style restore on hide, popovertarget button click.
  - Note: `:popover-open` CSS pseudo-class (already parsed by css-parser) always returns `false` until P4 wires it to `data-lumen-popover-open` attribute.

- **Heap-snapshot deflate compression + 5 MB cap** (`crates/js/src/heap_snapshot.rs`, ADR-008 §10C.3). 2026-06-02.
  - `compress_heap(&[u8]) -> Result<SuspendedHeap, HeapSnapshotError>` — `LJH1` magic prefix + zlib (deflate) stream; rejects with `HeapSnapshotError::TooLarge` when the compressed result exceeds `MAX_HEAP_SNAPSHOT_BYTES` (5 MiB).
  - `decompress_heap(&SuspendedHeap) -> Result<Vec<u8>, HeapSnapshotError>` — strips magic + inflates; payload without the magic prefix is returned verbatim (raw/legacy), empty → empty.
  - Reuses the already-vendored `flate2` (PNG iCCP path; same precedent as `lumen-storage` DOM-blob compression §10J.1) — no new external dependency. The `compressed` field / trait doc say "zstd" aspirationally; the 4-byte magic lets the on-disk format evolve.
  - Wired into `QuickJsRuntime::suspend()` (pause → `capture_raw_heap` → compress; `TooLarge` → empty snapshot so hibernation never blocks) and `resume()` (validate-inflate → fresh runtime). `capture_raw_heap` returns empty until full heap serialisation (task 10C.2) lands — blocked by native-function bindings that `JS_ReadObject` cannot reconstruct; the shell re-runs inline scripts on restore instead.
  - 10 unit tests (roundtrip simple/empty/binary, magic prefix, repetitive shrink >4×, cap rejects incompressible, large-compressible fits, legacy passthrough, corrupt stream, error Display) + 3 runtime tests (`suspend_produces_compressed_snapshot`, `resume_rebuilds_runtime_from_valid_snapshot`, `resume_rejects_corrupt_snapshot`).

- **HTMLIFrameElement JS stubs** (`crates/js/src/iframe_element.rs`, HTML spec §4.8.5, P1 2026-06-03).
  - `src`/`name`/`srcdoc`/`width`/`height`/`sandbox`/`allow`/`referrerPolicy`/`loading` properties reflect HTML attributes via `reflectAttr` helper.
  - `contentDocument` getter → `null` (Phase 0 — no sub-document navigation; matches cross-origin spec behaviour).
  - `contentWindow` getter → `null` (same reason).
  - `getSVGDocument()` → `null`.
  - Patches existing `<iframe>` elements at load time + intercepts `document.createElement('iframe')`.
  - 10 unit tests: install_succeeds, src getter/setter, contentDocument null, contentWindow null, name getter/setter, width/height attrs, sandbox reflects, getSVGDocument null, src default empty string. **922 lumen-js lib tests total.**

- **Gamepad API** (`crates/js/src/gamepad.rs`, W3C Gamepad Level 2 §4, P1 2026-06-03).
  - `navigator.getGamepads()` → sparse array of 4 null slots (Phase 0, no hardware polling).
  - `Gamepad` class: id/index/connected/timestamp/mapping/axes(4)/buttons(17)/vibrationActuator/hapticActuators.
  - `GamepadButton` class: pressed/touched/value.
  - `GamepadHapticActuator` stub: type, `playEffect(type, params) → Promise<'complete'>`, `reset() → Promise<'complete'>`.
  - `GamepadEvent` class: gamepad property.
  - Shell integration helpers: `_lumen_gamepad_connect(index, id, mapping)` fires `gamepadconnected`; `_lumen_gamepad_disconnect(index)` fires `gamepaddisconnected`. P3 shell calls these when polling OS gamepad APIs.
  - `window.Gamepad`, `window.GamepadButton`, `window.GamepadHapticActuator`, `window.GamepadEvent` exported as globals.
  - 15 unit tests. **1086 lumen-js lib tests total** (after combining with speech task's 1071 base).

- **MediaSession API** (`crates/js/src/media_session.rs`, W3C Media Session API L1/L2 §5, P1 2026-06-03).
  - `navigator.mediaSession` singleton.
  - `MediaMetadata` class: title/artist/album/artwork[].
  - `mediaSession.metadata` getter/setter (accepts MediaMetadata or null).
  - `mediaSession.playbackState` getter/setter: "none" | "paused" | "playing" (invalid values ignored).
  - `mediaSession.setActionHandler(action, cb)`: play/pause/stop/seekbackward/seekforward/seekto/previoustrack/nexttrack/skipad/togglemicrophone/togglecamera/hangup/togglecaptionstrack/enterpictureinpicture. Passing `null` removes handler.
  - `mediaSession.setPositionState(state)`: duration/playbackRate/position; `null` resets.
  - `mediaSession.setCameraActive(active)` / `setMicrophoneActive(active)` (L2 §5.4).
  - Shell integration: `_lumen_take_media_session_update()` → JSON snapshot (returns null if no change since last call, detected by sequence counter). P3 shell polls this to forward metadata to OS (SMTC/MPRIS/Now Playing).
  - OS → JS direction: `_lumen_fire_media_action(action, details)` invokes the registered handler (e.g. OS media keys trigger play/pause).
  - `window.MediaMetadata` exported as global.
  - 16 unit tests. **1101 lumen-js lib tests total.**

- **TC39 Decorators (Stage 3) Phase 0** (`crates/js/src/decorators.rs`). 2026-06-12 (P1, AA-1).
  - Pure-JS source-to-source transformer `__lumen_transform_decorators(src)`: minimal lexer (strings/comments/templates/regex opaque) + rewrite of `@dec` on named class declarations, instance/static methods and fields into ES2023 + runtime helper calls.
  - Runtime helpers: `__lumen_apply_class_decorators` (bottom-up, return value replaces class), `__lumen_apply_method_decorators` (`(fn, context) -> fn?`), `__lumen_apply_field_decorators` (composed init transformer, called with `this` = instance).
  - Well-known symbols `Symbol.ClassDecorator` / `Symbol.MethodDecorator` — `true` tags on decorator `context`.
  - Hooked into `JsRuntime::eval` and `eval_module` (fast path: no `@` in source → no transform; fail-open on transformer errors).
  - Phase 0 limits (documented in module): field decorator exprs evaluated per instantiation; class expressions / anonymous classes / accessors / `#private` / computed names unsupported.
  - 10 unit tests. All pass.

- **AsyncContext (TC39 Stage 2.7) Phase 0** (`crates/js/src/async_context.rs`). 2026-06-12 (P1, AA-2).
  - `AsyncContext.Variable` (`{name, defaultValue}` options; `get()` / `run(value, fn, ...args)`) and `AsyncContext.Snapshot` (`run(fn, ...args)`, static `wrap(fn)`). Pure-JS shim, installed after the DOM shim.
  - Context mapping = copy-on-write `Map` keyed by Variable identity; internals in WeakMaps (untamperable). `run` restores the previous mapping on exit, including on throw.
  - Microtask propagation: `Promise.prototype.then` patched to capture the mapping at registration and restore it around reactions. `catch`/`finally` delegate to the public `then`, `queueMicrotask` is `Promise.resolve().then(fn)` in the DOM shim — all covered by the single patch.
  - Phase 0 limits (documented in module): `await` continuations (engine-internal `PerformPromiseThen`) and tasks (`setTimeout`, event handlers) do not propagate — use `Snapshot.wrap` manually.
  - 8 unit tests. All pass.

- **Import Attributes (TC39 Stage 3) Phase 0** (`crates/js/src/import_attributes.rs`). 2026-06-12 (P1, AA-3).
  - QuickJS can't parse `with { … }` and `Loader::load` gets no attributes → Rust source preprocessor `strip_import_attributes`: strips `with { … }` / legacy `assert { … }` clauses from static `import` / `export … from` statements, records declared types in a shared `ModuleTypeRegistry` (specifier resolved exactly like the ESM resolver will).
  - `LumenLoader::with_types`: `type: 'json'` modules are validated as JSON (JSON-assert guard — invalid JSON fails the load) and compiled as synthetic `export default JSON.parse(...)`; any other declared type fails the load with `Error::new_loading_message`.
  - Hooked into `eval_module` (base = page URL) and `register_module_source` (base = the module's own specifier). Minimal lexer keeps strings / comments / templates / regex opaque, so `with` inside them is never a clause.
  - Phase 0 limits (documented in module): dynamic `import(spec, { with: { … } })` options left untouched; attribute keys other than `type` stripped and ignored.
  - 11 unit tests (7 transformer + 4 end-to-end). All pass.

- **HTML5 Drag and Drop API (HTML LS §9.10, PH3-9).** JS shim in `dom.rs`.
  - `DataTransfer` class: `setData`/`getData`/`clearData`; `types` DOMStringList; `.effectAllowed`/`.dropEffect`.
  - `DataTransferItem` class: `kind`, `type`, `getAsString(cb)`.
  - `DataTransferItemList` class: index-access, `add(data, type)`, `remove(idx)`, `clear()`, `length`.
  - `DragEvent` class: extends `MouseEvent`; `dataTransfer` property (auto-populated).
  - `window.DragEvent`, `window.DataTransfer`, `window.DataTransferItem`, `window.DataTransferItemList` — all exported.
  - `draggable` getter/setter on `Element` (backed by HTML attribute).
  - `ondragstart`, `ondrag`, `ondragend`, `ondragenter`, `ondragover`, `ondragleave`, `ondrop` null-init properties on all elements.
  - `_lumen_dispatch_drag_event(nid, type, x, y, data_json)` — called from shell; creates `DataTransfer` from JSON dict, dispatches via `_lumen_dispatch_rich`.
  - 5 new tests (fires on element, coordinates, DataTransfer payload, bubbling, default-not-prevented). 12 total DnD tests (7 DataTransfer/DragEvent + 5 dispatch).
- **WebAssembly MVP interpreter** (`crates/js/src/wasm/`, U-4 stage 1). 2026-06-18.
  - Pure-Rust, no external runtime dep. `parser.rs` decodes the WASM 1.0 core binary (all sections; LEB128; function bodies into a flat `Instr` stream with pre-resolved `block`/`loop`/`if` → `End`/`Else` targets). `interp.rs` is a stack interpreter: full numeric/comparison/conversion/sign-extension ops, structured control flow (`block`/`loop`/`if`/`br`/`br_if`/`br_table`/`return`), `call`/`call_indirect` with signature checks, linear memory load/store + `memory.size`/`grow`/`copy`/`fill`, globals, tables, traps (div-by-zero, OOB, bad-cast), saturating truncation (`0xFC`), reference-null. `value.rs` = `Value`/`ValType`/`FuncType`.
  - Bridge (`mod.rs`): thread-local module/instance registry; `webassembly.rs` registers `__lumen_wasm_*` native bindings and drives them from the JS shim so `WebAssembly.Module`/`Instance`/`compile`/`validate`/`instantiate` execute real bytecode. `Instance.exports` are callable functions; exported memory/globals are live-backed via copy helpers.
  - Host imports: JS functions stored as `Persistent<Function>`, called from the interpreter via the `HostImports` trait (`JsHost`). Re-entrant calls into a *busy* instance return a trap (instance is removed from the registry during a call) instead of panicking.
  - **Typed numeric boundary (U-4 i64/BigInt, 2026-06-18):** values cross JS↔WASM by their WASM type — `i64` rides as a JS `BigInt` (full 64-bit precision, per the W3C WebAssembly JS Interface), the rest as `Number` — for exported function args/results, host-import args/results, and exported globals. Shared marshalling helpers `wasm_value_to_js` / `js_value_to_wasm` (`mod.rs`); `__lumen_wasm_call` and the global get/set bindings carry `rquickjs::Value` (not `f64`), coercing each arg to its declared type via `func_signature`. The export wrapper and global setter pass JS values through untouched (the old `+arg` numified a BigInt → throw/precision loss).
  - `clear_registry()` drops all modules/instances (releasing import `Persistent`s); must run before the owning `Runtime` is dropped or QuickJS aborts on `list_empty(&rt->gc_obj_list)`. Wiring into shell context teardown is BUG-222 (tests call it from the `with_wasm` harness).
  - **Fixed-width SIMD (`v128`, the `0xFD` prefix — U-4a, 2026-06-20).** `simd.rs` executes the complete fixed-width SIMD opcode set; `value.rs` carries the new `V128([u8;16])`/`ValType::V128`. `parser.rs` decodes `0xFD`-prefixed ops into dedicated `Instr` variants (`V128Const`/`V128Load`/`V128Store`/`V128LoadLane`/`V128StoreLane`/`Shuffle`/`SimdLane`/`Simd`). Covered: const, all memarg loads (`loadNxM_s/u`, `loadN_splat`, `loadN_zero`) + lane load/store, splat, swizzle/shuffle, extract/replace lane, all integer + float arithmetic/compare/min-max/abs/neg/sqrt/rounding, bitwise (`and`/`or`/`xor`/`not`/`andnot`/`bitselect`/`any_true`), shifts, `all_true`/`bitmask`, saturating add/sub, `avgr_u`, `q15mulr`, narrow/extend(low/high)/extmul/dot/extadd_pairwise, and the float↔int conversions (`trunc_sat`/`convert`/`demote`/`promote`). Lanes are little-endian; v128 has no JS-boundary representation (collapses to 0, never reached by a spec-valid call).
  - **Relaxed-SIMD (`0xFD` sub-opcodes `0x100..=0x113` — U-4a, 2026-06-20).** `simd.rs::exec_simd_relaxed` (routed from `exec_simd`) executes the full relaxed-SIMD set. The spec permits implementation-defined results in edge cases (NaN, out-of-range swizzle indices, fused vs split multiply-add); we always compute the strict/deterministic variant (a conforming choice). Strict-equivalent ops delegate to the existing fixed-width code: `relaxed_swizzle`→swizzle, `relaxed_trunc_f32x4_s/u` + `_f64x2_*_zero`→`trunc_sat`, `relaxed_laneselect` (i8/16/32/64)→`bitselect` (bytewise-identical for every width), `relaxed_min/max` (f32x4/f64x2)→`min`/`max`, `relaxed_q15mulr_s`→`q15mulr_sat_s`. Implemented directly: `relaxed_madd`/`relaxed_nmadd` (f32x4/f64x2 FMA `a*b+c` / `-(a*b)+c`), `relaxed_dot_i8x16_i7x16_s` (signed i8 lane-pair products → saturating i16x8), `relaxed_dot_i8x16_i7x16_add_s` (that dot widened + accumulated into the i32x4 operand). The parser was unchanged (relaxed ops are immediate-free, already decoded into `Instr::Simd`).
  - **Threads / atomics (`0xFE` prefix — U-4a, 2026-06-20), single-threaded semantics.** `parser.rs` decodes `0xFE`-prefixed ops into `Instr::Atomic { sub, offset }` (memarg-carrying notify/wait/load/store/rmw/cmpxchg) and `Instr::AtomicFence` (`0xFE 0x03`, reserved byte). `interp.rs::exec_atomic` executes all of them with a single agent, where every read-modify-write is trivially atomic: atomic loads (`0x10..=0x16`) and stores (`0x17..=0x1D`) delegate to the plain `load`/`store`; binary RMW add/sub/and/or/xor/xchg (`0x1E..=0x47`, 6 groups × 7 widths i32/i64/8u/16u/32u) return the previous value; cmpxchg (`0x48..=0x4E`) compares against the width-masked expected; `memory.atomic.notify` always wakes 0; `wait32`/`wait64` never block (`1` not-equal if the cell changed, else `2` timed-out); `atomic.fence` is a no-op. **Every access traps on a misaligned address** (`check_atomic_align`, natural alignment per spec); sub-width RMW reads unsigned zero-extended (`atomic_read_u64`) and writes the low bytes (`atomic_write_u64`). Shared-memory limits flag `0x03` already parsed, so threads modules `compile`/`validate`. Helpers `atomic_width`/`atomic_rmw_layout`/`atomic_old_value` map sub-opcodes to (width, is-i64).
  - **Live memory aliasing (U-4b, 2026-06-20).** Exported `Memory.buffer` is one **stable** JS `ArrayBuffer` (built once in `makeExportMemory`) kept coherent with Rust-owned linear memory at call boundaries: each exported-function wrapper (`makeExportFn`, given the memory via a shared `memRef`) runs `mem._syncIn()` (JS buffer → Rust, `__lumen_wasm_mem_write`) before `__lumen_wasm_call` and `mem._syncOut()` (Rust → JS *in place*, `new Uint8Array(_buf).set(...)`) after, so `HEAP32 = new Int32Array(memory.buffer)` reads/writes propagate in both directions and a captured view stays valid across calls. Growth (JS `Memory.grow` or a `memory.grow` instruction detected via a page-count change in `_syncOut`) allocates a fresh, larger buffer — matching the spec's detach-on-grow; callers re-acquire their HEAP views. New native `__lumen_wasm_mem_buffer(instId)` (→ `wasm::mem_read_all`) returns the whole linear memory as one bulk-copied `ArrayBuffer`. Cost: a full in/out memory copy per call when memory is exported (skipped otherwise) — a documented correctness/perf trade-off, exact for the single-agent model (ADR-014). `mem.read`/`mem.write` remain MVP escape hatches that bypass the sync.
  - Boundaries (documented): a host import can't observe writes made earlier in the same in-flight call; an *imported* `Memory` is not aliased to the instance's internal memory (only the exported path is); `Memory.buffer` is not backed by a JS `SharedArrayBuffer`; `memory.init` / unknown `0xFE` sub-opcodes rejected at decode (graceful `CompileError`). (JS-level `SharedArrayBuffer`/`Atomics` exist independently — see the `Atomics.waitAsync` bullet below — but a WASM linear memory does not alias one.)
  - Bytes cross JS→Rust as `TypedArray<u8>` (the engine's `Vec<u8>` `FromJs` requires a real `Array`, which the shim does not pass).
  - 56 tests: 14 engine (add/factorial-loop/if-else/memory/import/div-trap/f64/nested-branch + parse/validate) + bridge/JS-integration (instantiate-and-call, exports introspection, i64 export/global/import BigInt round-trips) + 15 SIMD (i32x4/i8x16/f32x4 arithmetic, extract/replace lane, splat, eq-mask, v128 store↔load, shuffle, bitselect, extend_low, dot, add_sat_s, trunc_sat, decode-not-rejected) + 18 atomics (store/load roundtrip, rmw add/xor/xchg + sum-write, byte-wide rmw8 wrap, cmpxchg success/mismatch, i64 rmw, notify=0, wait not-equal/timed-out, fence-nop, unaligned-trap, decode-validate) + 12 relaxed-SIMD (madd/nmadd f32x4+f64x2, laneselect, min/max, trunc, swizzle, q15mulr, both dots, decode-accept) + 5 live memory aliasing (WASM-write-visible-through-stable-view, JS-write-visible-to-WASM, buffer-identity-stable, JS-grow-resizes, round-trip-through-heap).
- **JS-level `Atomics.waitAsync`** (`tc39_proposals.rs`, U-4a JS wrapper). 2026-06-20. QuickJS ships JS-level `SharedArrayBuffer` + synchronous `Atomics` (load/store/add/sub/and/or/xor/exchange/compareExchange/notify/isLockFree, growable SAB) natively; the synchronous `Atomics.wait` throws `TypeError: cannot block in this thread` because Lumen runs all JS on a single non-blocking agent (one JS thread, ADR-014), exactly like a browser main thread. The only spec gap was `Atomics.waitAsync` (ES2024). A pure-JS shim (section 11) implements it for the single-agent model: validates the view (Int32Array/BigInt64Array over a `SharedArrayBuffer`, else `TypeError`/`RangeError`), returns `{async:false,value:'not-equal'}` on a value mismatch and `{async:false,value:'timed-out'}` on a non-positive timeout, otherwise parks a FIFO waiter (keyed on the SAB data block + byte offset) and returns `{async:true,value:<Promise>}`. `Atomics.notify` is wrapped to also resolve matching async waiters to `'ok'` (folding their count into its return value), and a finite timeout resolves `'timed-out'` via `setTimeout`. 8 tests (is-function, sync not-equal, zero-timeout, notify resolves ok + count, other-index no-wake, non-shared `TypeError`, non-integer-array `TypeError`, BigInt64 round-trip).
- **WebCodecs graceful degradation** (`web_codecs.rs`, U-4 stage 2). 2026-06-18. `VideoEncoder`/`VideoDecoder`/`AudioEncoder`/`AudioDecoder` `configure()` no longer throw synchronously (which white-screened SPAs not wrapping it in try/catch) — they transition to `configured` and report unsupported codecs through the spec async error callback. Added the missing `InvalidStateError` class. `isConfigSupported()` still resolves `false` for feature detection.

## Deferred

- WebGL: GLSL execution (per-vertex colour / texture sampling — currently flat `uniform4f` fill), `drawElements` / indexed draws, real textures. Backend stub lives in `lumen_paint::webgl`.
- PerformanceObserver API.
- `rusty_v8` backend (v1.0+).

## Invariants

- `QuickJsRuntime: Send + Sync` (enforced by `unsafe impl` + `Mutex`).
- `call_function` pollutes the global namespace with `__lum_args__` only transiently — cleaned up with `delete` after each call.
- `from_rq` maps `Type::Undefined` to `JsValue::Null` (not `Undefined`) — matches the trait docs which say "simple JSON-compatible types".
- rquickjs 0.11 `Function::call` takes `IntoArgs` (fixed-size tuples). Dynamic calls must use the eval workaround until rquickjs adds `Function::apply` or `Rest<T>: IntoArgs`.
- DOM shim: `parentElement` and `children` are defined with `enumerable: false` via `Object.defineProperty`. Prevents `from_rq`'s `obj.props()` loop from serializing these cyclic getters → infinite recursion / stack overflow.
- DOM shim: `Option<T>` in rquickjs maps `None → undefined` (not `null`). All nullable-returning native functions are wrapped with `_lumen_u2n(v)` in the shim to convert `undefined → null` as Web API requires.
- `install_dom` must be called before `eval`. Drop the runtime before `Arc::try_unwrap(doc_arc)` — closures hold Arc clones until the runtime is dropped.
- Web Storage closures capture `Arc<Mutex<WebStorage>>` clones — dropped with the runtime. The outer `Arc` in the shell's `ls_storage` map remains the authoritative store; JS mutations are immediately visible in Rust after the closure releases the lock.
- IndexedDB requests defer their data operation to `_idb_dispatch_request` (run once via `req._action`), not to the calling site. Reading `request.result` before the `success` event is therefore always `undefined`; tests and the shell must call `_lumen_idb_flush()` to drain pending events. Synchronous validation (invalid key range → `DataError`, read-only transaction → `ReadOnlyError`) still throws at the call site, before the request is queued.
- **`Runtime::execute_pending_job()` must not be called inside a `ctx.with(...)` closure.** `ctx.with` holds a context borrow; calling a `Runtime` method re-enters it and panics in `rquickjs-core safe_ref.rs`. In `sw_worker.rs` the rule is: fire the JS event inside `ctx.with`, then drain microtasks via `flush_jobs(&rt)` *outside* it, then read results in a second `ctx.with`. Globals persist across separate `ctx.with` calls on the same context, so multi-step promise resolution (e.g. `respondWith(...).then(r => r.text()).then(t => global = t)`) works by draining between the fire and the read.
- Service Worker execution threads (`sw_worker::spawn_sw_worker`, PH3-20) have an isolated `ServiceWorkerGlobalScope`: their `caches`/`atob`/`btoa` are native-backed (real base64), and their `fetch` is cache-first only (no network in Phase 1). A SW serves only responses already present in the shared `CacheBackend` the page cached — `cache.addAll()` cannot pull from network inside the SW.

# lumen-js

Crate providing the `JsRuntime` implementation. **V8 (`rusty_v8` 150.1.0) is the ONLY
engine since S12b-F1 (2026-08-04) removed the `quickjs` shell feature.**
`QuickJsRuntime` itself was deleted in S12b-F2 (2026-08-04, same day); `dom.rs::install_primitives`
(2736 lines, the rquickjs native-registration entry point) was deleted in S12b-F3 (2026-08-04);
`rquickjs`/`rquickjs-core`/`rquickjs-sys` were removed from `Cargo.toml`/`Cargo.lock` outright in
the last slice, S12b-F4 (2026-08-04) — the crate now has zero QuickJS code or dependency. Historical
entries below predating F1/F2 still describe `QuickJsRuntime`/`--features quickjs` as they were at
the time — read dates.

> **Coverage note (2026-07-02):** the code wires **~90 Web-API modules**; this file curates
> only the highlights with implementation detail. For the full shipped-API list use
> `CAPABILITIES.md` (source of truth), not this file.

## Scope

- `V8JsRuntime` struct (`v8_runtime.rs`, `#[cfg(feature = "v8-backend")]`): owns a `rusty_v8`
  isolate on a dedicated thread.
- Implements `lumen_core::JsRuntime`: `eval`, `set_global`, `get_global`, `call_function`.
- Shell wires it in via `features = ["v8"]` (the crate's `v8-backend`); without the feature
  `NullJsRuntime` is used.

## Done

- **`permissions.query()` recognises names instead of saying yes to all of them
  (BUG-386, P3, 2026-08-10).** The Permissions API used to be 25 lines of
  `WEB_API_SHIM`: one `_perm_denied` array of 11 names and `granted` for
  everything else, including names the engine had never heard of. That is not a
  default policy but a missing WebIDL conversion — and it destroys the one thing
  `query()` is for, since a page asks `query({name: 'X'})` precisely to learn
  whether `X` exists. Now a module of its own, `permissions.rs`, built on two
  rules: **(1)** a 34-name registry, everything else rejected with a `TypeError`
  (rejection, never a synchronous throw — `query` returns a promise); **(2)**
  every recognised name carries an explicit state, with no fallback branch, so a
  new name cannot be added without classifying it (the
  `every_recognised_name_has_a_state` gate). `granted` is reserved for
  operations that really have their specified effect today — the clipboard
  natives, unpartitioned cookies, `IdleDetector`'s OS idle polling — and
  everything else is `denied`, which is what keeps `queryLocalFonts()`'s gate
  shut until OS font enumeration has a way to ask the user. `notifications` is
  not a table entry at all: it is read off `Notification.permission` at query
  time, because copying the engine's own answer into a table only lets the two
  drift. `PermissionStatus` extends the shim's `EventTarget` via
  `Reflect.construct` (so the base's body runs without going through the
  throwing no-constructor stub), `name`/`state` are readonly prototype getters
  over a private `WeakMap` and `state` recomputes on every read, so it cannot go
  stale. To keep the `change` machinery from being ornamental, the internal
  `_lumen_permission_state_changed(name)` is called from
  `Notification.requestPermission()` — and only on a real move, since an event
  for an unchanged value would misreport the engine. `navigator.permissions` is
  a non-writable own property (the BUG-366 precedent); the spec's
  `Navigator.prototype` accessor is impossible while there is no `Navigator`
  interface at all ([BUG-624](../bugs/BUG-624-OPEN.md)).
- **Sensors are real `EventTarget`s, and the two abstract bases are not
  constructible (BUG-394, P3, 2026-08-11).** `generic_sensor.rs` used to define
  a private `_SensorEventTarget` mixin "to avoid depending on a global
  `EventTarget`" — a QuickJS-era motivation that outlived the engine it was
  written for. `Sensor.prototype` now descends from the shim's own
  `EventTarget`, so `once`/`capture` and the `on<type>` step come from the same
  implementation as every other event source; the module refuses to install at
  all without global `Event`/`EventTarget` (the `permissions.rs` precedent —
  install order guarantees them on a page, since `install_dom` evaluates
  `WEB_API_SHIM` before every `install_v8!` module). `new Sensor()` and
  `new OrientationSensor()` throw `TypeError: Illegal constructor` on a
  `new.target` check; subclasses enter the same body through
  `Sensor.call(this, options)`, where `new.target` is undefined. Inheriting the
  real base also meant deleting `start()`'s hand-written `onactivate` call —
  `dispatchEvent` performs that step itself, so keeping both would have
  delivered `activate` twice. `SensorErrorEvent` is still not an `Event`
  ([BUG-761](../bugs/BUG-761-OPEN.md)).
- **`queryLocalFonts()` exists, and the 2020-draft `navigator.fonts` is gone
  (BUG-385, P3, 2026-08-10).** `local_font_access.rs` implemented a WICG draft that
  was dropped before the API shipped: `navigator.fonts.query()` — a surface no
  browser exposes and no test calls, i.e. an own enumerable `navigator` property
  that is a fingerprint and nothing else — while `queryLocalFonts()`, the entry
  point every real page and all six upstream `font-access` tests start from, did
  not exist. `FontData` was an ES5 constructor assigning its four fields onto
  `this`, so `window.FontData({family: 'LEAK'})` (a plain call, no `new`) wrote
  `family`/`style`/`fullName`/`postscriptName` onto `window` instead of throwing.
  The shim now follows `FSAL_SHIM`'s WebIDL idiom: no constructor, readonly
  prototype getters over a private `WeakMap`, `Symbol.toStringTag`,
  non-enumerable globals, argument conversion reported as a rejection. The §2
  gates (transient activation → `SecurityError`, `local-fonts` permission →
  `NotAllowedError`) are written **now**, fail-closed, even though Phase 0 has
  nothing to withhold — Phase 1 must not be able to land without them, which at
  the time meant [BUG-386](../bugs/BUG-386-FIXED.md) answering `granted` by
  default (fixed 2026-08-10: `local-fonts` is `denied`, so the gate is what
  holds the branch shut). OS font
  enumeration stays unimplemented (`[]`); the natives are captured in closure
  scope at install time, so page script cannot shadow their names to feed the
  shim a font list of its own.
- **One `FileSystemDirectoryHandle`, and `navigator.storage.getDirectory()` returns
  it over a real sandbox (BUG-372, P3, 2026-08-10).** `storage_manager.rs` used to
  define a second class of that name inside its shim's IIFE and resolve
  `getDirectory()` to *that*, so the engine had two same-named classes with disjoint
  method sets and every method of the one a page actually got answered without a file
  system behind it (`getFileHandle()` → an object literal with no `getFile()`,
  `removeEntry()` → a successful promise that removed nothing). The stub is gone;
  `getDirectory()` builds the root through the private `__lumen_fsa_internal` factory
  `filesystem_access.rs` publishes (BUG-374 took the public constructor away) and
  rejects with `NotSupportedError` if that shim did not install, rather than
  substituting a look-alike. Behind it: `filesystem_access::opfs_root_entry_json`
  creates `<exe_dir>/data/opfs/<origin-slug>/` (portable-data convention, not
  `%APPDATA%`; slug = readable prefix + FNV-1a of the *full* origin, because an opaque
  origin is a whole URL) and hands out an ordinary `DIR_REG` grant for it. `DirGrant`
  gained `writable`: the OPFS subtree is writable and subdirectories inherit it, a
  picked directory stays read-only, which is what tells `{create:true}` apart from
  `NotAllowedError`. The directory natives now take `create`/`recursive` and answer
  with a **DOMException name** (`{"error":"…"}`) instead of `null` — "missing",
  "read-only grant" and "wrong kind" are three different answers, and collapsing them
  into one falsy value was half the bug. `_lumen_fs_resolve` compares two grant ids in
  Rust (no path crosses into JS); `_lumen_writable_from_token` lets an OPFS file
  handle write straight to its file, because routing it through the save dialog would
  have replaced one silent wrong answer with another. Entry names are validated in
  Rust (`valid_entry_name`) — without that, `{create:true}` is a write-anywhere
  primitive. Directory listings mint a grant per entry
  (BUG-750, closed with BUG-374 below).
- **File System Access is a WebIDL hierarchy, and none of it is constructible from
  page script (BUG-374, P3, 2026-08-10).** `FSAL_SHIM` used to define three unrelated
  ES5 constructor functions. Ten reported divergences followed from that one shape:
  no `FileSystemHandle` base on the global (so `if (window.FileSystemHandle)`
  feature-detects concluded the API was missing), public constructors that took the
  internal grant id as an argument, `kind`/`name` as enumerable **writable own data
  properties** (`fileHandle.kind = 'directory'` was accepted, after which the object
  lied about its own type), no `Symbol.toStringTag`, no
  `queryPermission`/`requestPermission`/`remove`/`getUniqueId`/`move`, no async
  iteration of a directory, a writable stream that was not a `WritableStream` and
  whose `seek`/`truncate` returned a resolved promise and did nothing, no
  `FileSystemSyncAccessHandle`, no `[Serializable]`, and pickers that took an options
  dictionary and ignored it whole. Three transferable points:
  * **`isSameEntry` cannot compare grant tokens.** Every `getFileHandle()` mints a
    fresh one, so two handles on the same file compared unequal. Comparison goes
    through `_lumen_fs_unique_id`, a per-path label drawn from the same CSPRNG as a
    grant id — *not* a hash of the path, which would let a page confirm a guessed
    absolute path by comparing digests.
  * **A writable stream is a queue.** Commands join it synchronously at call time, so
    `w.write(x); w.truncate(1); w.close();` without `await` cannot commit before the
    write lands. Deferring the enqueue by even one microtask lets a later command
    overtake an earlier one — which is exactly how the first version of the tests
    failed, with an empty file.
  * **A handle caches nothing the file system owns.** `getFile()` re-stats through
    `_lumen_fs_file_size`: the size captured when the handle was created goes stale
    the moment anything writes, and a `File` that reads new contents while reporting
    the old length is a silent wrong answer (the live probe caught `hello/0`).

  `structuredClone` gained an extension point for `[Serializable]` platform
  interfaces (`window.__lumen_platform_cloners` in `dom.rs`, a closed-over
  (test, clone) list) — a platform object cloned as a plain object loses its class and
  its internal slots. The picker gate reads `navigator.userActivation`, which the
  engine hardcodes to `isActive: true`, so it is structurally right and currently
  inert ([BUG-751](../bugs/BUG-751-OPEN.md)).
- **`FileSystemObserver` is a snapshot differ, not an OS watcher (BUG-389, P3,
  2026-08-10).** There is no file-watching dependency in the workspace and none was
  added: `_lumen_fs_observe` snapshots the observed subtree, `_lumen_fs_poll_changes`
  re-snapshots and diffs, and one shared `setInterval` (100 ms) drives every live
  observation. Four points worth carrying:
  * **Two snapshots cannot show a rename.** A move looks exactly like a
    disappearance plus an appearance, so `moved` is only reported when a poll saw
    *exactly one* of each carrying identical metadata (kind, length, mtime — all
    preserved by `rename`, none by a fresh write). Ambiguous polls stay two honest
    records rather than one invented one; guessing here would report a `moved`
    between two unrelated files that happened to change in the same tick.
  * **What the snapshot omits is load-bearing.** It holds kind/length/mtime and
    deliberately not the full `Metadata`: comparing access time would report
    `modified` for a plain read.
  * **The observation state lives in Rust, not in the shim.** A page that dropped
    its observer must stop costing a directory walk per tick, and the origin check
    has to happen where the grant registries are. Installing for a new document
    revokes the previous one's observations exactly as it revokes its grants.
  * **A poll tick is a test step, not a wall-clock wait.** The unit-test harness
    stubs `setInterval` into a callback collector and exposes `__tick()`, so the
    JS-level tests assert on delivered records without sleeping — and without the
    flakiness a real 100 ms interval would bring.
- **File-API grants are unguessable, origin-bound and page-scoped; the natives are
  off the global object (BUG-371, P3, 2026-08-10).** The documented "JS only receives
  an opaque token" model did not hold: all ten `file_input`/`filesystem_access`
  natives were plain `window` properties and all three registries
  (`FILE_REGISTRY`/`DIR_REG`/`WRITE_REG`) were process-global with counters starting
  at 1, so any page could read every file the user had ever picked, walk a granted
  directory (`_lumen_dir_get_file` *mints* new read tokens), or overwrite another
  page's open save handle — by enumerating `1, 2, 3, …`. Three mechanisms now:
  (1) ids are 128 bits of `getrandom` entropy as a 32-char hex **string**
  (`file_input::new_grant_id`) — an `f64` cannot carry that, hence all ten natives
  moved from numeric to string parameters and the JSON carries `"token":"…"`;
  (2) every registry entry records its issuing origin (`file_input::origin_for_url`
  — tuple origin for URLs with a host, the full URL for `file:`/`data:`/`about:`, so
  two local pages never share one), and the reader's origin is captured **in Rust**
  at install time from `page_url`, never taken from a JS argument (unlike the
  SW/Cache natives, where origin is a forgeable argument); (3) installing a
  document's bindings revokes the previous document's grants on that same origin, so
  a token does not outlive its page. On top of that `install_dom` calls
  `file_input::seal_file_natives_v8` once both shims have copied the bindings into
  closure variables, deleting all eleven names (ten natives + the
  `__lumen_fs_internal` bridge) from the global object — `FSAL_SHIM` moved into an
  IIFE for this. `File`'s token and the handles' internals
  (`_token`/`_size`/`_pathId`/`_id`/`_closed`) live in `WeakMap`s and the public
  `File` constructor no longer accepts `_token` from its options dictionary. The
  shell reads the origin back via `file_input::active_document_origin()` rather than
  re-deriving it from `PageSource` — a second derivation would drift silently (every
  read would just return an empty string). WebIDL shape of the handles stays with
  [BUG-374](../bugs/BUG-374-FIXED.md).
- **`on<type>` event handler content/IDL attributes on live elements (BUG-360, P3,
  2026-08-09).** `<div onclick="…">`/`el.onclick = fn` now actually fire — previously the
  attribute was never compiled and all three live dispatch paths (`_lumen_dispatch`,
  `_lumen_dispatch_bubble`, `_lumen_dispatch_rich`) only consulted `_lumen_listeners`
  (`addEventListener`), ignoring `on<type>` entirely, so even a manually-assigned
  `el.onclick` never ran. New `_lumen_on_handlers` table (`nid:type` key, cleared by
  `_lumen_gc_collect` alongside `_lumen_listeners`) backs a curated
  `_LUMEN_EVENT_HANDLER_ATTRS` accessor list (GlobalEventHandlers) on every live element;
  `<body onload>` forwards to `window.onload` per HTML LS §8.1.7.3 (the one attribute in
  that forwarding set actually evidenced by a failure — `check-layout-th.js`'s
  `<body onload="checkLayout(...)">` idiom, blocking dozens of `css/css-overflow`/
  `css-sizing`/`css-scrollbars` WPT files). 11 tests in `dom.rs::tests::v8_inline_event_handlers`.
  **Non-obvious:** `WEB_API_SHIM` (`dom.rs:324`) is a plain `"..."` Rust string, not a raw
  string — a bare `"` anywhere inside it (even in a `//` comment) silently truncates the
  literal and turns everything after it into raw Rust source until the next stray `"`,
  producing a wall of "character literal"/"unknown prefix" errors anchored deep inside
  unrelated JS, nowhere near the actual mistake. Cost real time on this fix; see the
  warning comment now sitting right above the `const` declaration.
- **`dom.rs` test monolith, timing/observer families ([P1] P3-v8-s12b-24-perf-observers,
  2026-07-30).** Performance API + PerformanceObserver (incl. the single-type observe form),
  `queueMicrotask` + rAF/cAF with EE-5 vsync batching, and
  MutationObserver/ResizeObserver/IntersectionObserver — 71 tests — now in
  `dom.rs::tests::v8_perf_observers`; QuickJS copies deleted. Needed no new mechanics: one
  `v8_runtime_with_dom` twin, and the six non-`eval` runtime methods the bodies use
  (`update_layout_rects`, `update_viewport_size`, `take_raf_pending`, `take_dom_dirty`,
  `raf_pending_flag`, `dom_dirty_flag`) already exist on `V8JsRuntime` with identical signatures.
  **Non-obvious:** `_lumen_drain_microtasks` is a deliberate no-op stub on the V8 side
  (`v8_runtime.rs:3611` — the compat-layer closure cannot reach the isolate for
  `perform_microtask_checkpoint`), and V8 runs a checkpoint after every script. That makes
  microtask *ordering* observable from Rust for the first time — the three ported
  `queue_microtask_*` tests only pinned the function's existence, since a QuickJS `eval()`
  returned with the job queue unprocessed — so a 72nd test was added asserting the callback runs
  neither inline at the call site nor later than the end of the queuing script. A test needing a
  forced flush where V8 does not choose one must split into two `eval` calls; there is no drain
  primitive.
- **`dom.rs` test monolith, navigation/URL/storage families ([P1] P3-v8-s12b-24-nav-url-storage,
  2026-07-29).** Location/`NavigateRequest` + fragment navigation + History API
  (`pushState`/`replaceState`, `popstate`/`hashchange`), Web Storage, `URLSearchParams`/`URL` —
  67 tests — now in `dom.rs::tests::v8_nav_url_storage`; QuickJS copies deleted, and with them the
  last callers of `runtime_with_url`/`runtime_with_storage` (both helpers moved into the V8 module).
  **Non-obvious:** `V8JsRuntime::new()` tolerates repeated construction inside one test process —
  `local_storage_persists_across_runtimes` builds two runtimes over a shared
  `Arc<Mutex<WebStorage>>` and passes, so the isolate-lifecycle worry the scoping note raised for
  the IndexedDB reload cluster does not apply. Green 67/67 with untouched bodies; the 19 `URL`
  tests pin Lumen's own Phase-0 plumbing, *not* the spec — `URL.prototype` setters were still dead
  (BUG-375, fixed 2026-08-10) and `Url::resolve` still kept dot-segments (BUG-346), both
  engine-agnostic.
- **`URL` component setters go through re-serialization, not per-setter patching (BUG-375,
  2026-08-10).** A `URL` object keeps its components in non-enumerable slots; each of the nine
  writable setters writes only its own slot and then calls `_lumen_url_reserialize`, which rebuilds
  the href with `_lumen_url_serialize` (URL Standard §4.1) and re-parses it through the *same*
  `_lumen_parse_url` the constructor uses. Derived components (`host` after a `port` write,
  `origin` after a `hostname` write) therefore fall out of one parse instead of being maintained by
  hand nine times, and the shim still has exactly one URL parser. **Non-obvious:** the parse result
  carries `hasAuthority`, because an empty `host` does not distinguish an authority-bearing URL
  from an opaque-path one (`mailto:a@b`) — the serializer needs it to decide whether to emit `//`,
  and the host-ish setters need it to know they must be no-ops. A component whose spec attribute is
  readonly (`origin`, `searchParams`) is defined with **no** `set` at all; the previous
  `set: setter || function() {}` idiom made strict-mode writes silently disappear.
- **`dom.rs` test monolith, mock-provider families ([P1] P3-v8-s12b-24-ws-sse, 2026-07-29).**
  WebSocket (incl. mock session + binary mode), EventSource/SSE, fetch bindings and IME +
  bfcache — 73 tests — now in `dom.rs::tests::v8_ws_sse`; QuickJS copies deleted. **Non-obvious:**
  the mock provider structs (`JsWebSocketProvider`/`JsSseProvider` impls) port verbatim — they
  implement `lumen_core::ext` traits and never name an engine type — and `V8JsRuntime::install_dom`
  takes the providers at the same argument positions as the QuickJS one, so only the
  `runtime_with_*` constructors change. `_lumen_pump_websockets`/`_lumen_pump_sse` deliver queued
  events under V8 unchanged. Green 73/73 with untouched bodies; the three EventSource/Worker bugs
  found on this slice (BUG-362/363/364, all since fixed) were engine-agnostic shim gaps this suite
  didn't cover on *either* engine — "73/73 green" was not a spec-conformance claim.
- **`dom.rs` test monolith, first porting slice ([P1] P3-v8-s12b-24-core, 2026-07-29).** The
  "Core DOM basics" family (console/SVG/wrapper identity/`self`&`window`, Canvas 2D,
  `getElementById`/`querySelector`/attributes/`textContent`/`Image`, `alert`/`print`, timers +
  `scheduler.postTask`, History API) — 99 tests — now lives in `dom.rs::tests::v8_core`, a nested
  module gated `#[cfg(feature = "v8-backend")]`; the QuickJS copies are deleted. **Where to look:**
  everything still under `mod tests` directly is unported and runs against `QuickJsRuntime`;
  everything under a `v8_*` nested module is ported. Twin constructors `v8_runtime_with_dom` /
  `v8_runtime_with_url` mirror the outer helpers — `V8JsRuntime::install_dom`'s signature is
  identical to `QuickJsRuntime::install_dom`. **Non-obvious:** a helper whose only callers move
  into the V8 module must move with them, otherwise it is dead code in a build *without*
  `v8-backend` and `clippy -D warnings` goes red only in that configuration — check both.
  Slice log and the two behavioral divergences found (`register_img_bitmaps`,
  `getContext('webgl')`) — `docs/tasks/ph3-v8-migration.md`.
- **`V8JsRuntime::register_img_bitmaps` ([P1], 2026-07-29, [BUG-447](../bugs/BUG-447-FIXED.md)).**
  Mirrors the QuickJS method: clears `img_bitmap_store` (navigation-scoped) and writes the
  `(nid, Arc<Image>)` pairs **on the JS thread** via `run`, because the store is `thread_local!`.
  It had no V8 counterpart at all, and the shell's `PersistentJs` trait default silently absorbed
  the missing override — `drawImage(imgElement, …)` painted nothing on the default engine.
- **ES modules on V8 — `<script type=module>` ([P1] P3-v8-s12b-23, 2026-07-29, closes
  [BUG-350](../bugs/BUG-350-FIXED.md)).** New `v8_esm.rs`: `script_compiler::compile_module`
  → `instantiate_module` → `evaluate` → `perform_microtask_checkpoint`; `V8JsRuntime` now
  overrides `eval_module`/`register_module_source` (the `ext.rs` trait default ran module
  source through classic `eval`, which rejects top-level `export`/`import` at parse time) and
  gains `set_import_map`, which the shell calls on the V8 branch. **Non-obvious:** V8's
  `ResolveModuleCallback` is a captureless `extern "C" fn` — there is no `data` pointer — so
  the module registries (sources, compiled modules, `identity_hash → specifier`, page URL,
  import map) live in a `thread_local!` on the isolate's JS thread, not in the `Arc<Mutex<…>>`
  fields the rquickjs `Loader`/`Resolver` share with `QuickJsRuntime`. Specifier resolution is
  *not* duplicated: `esm::resolve_specifier_with` is the shared core both engines call, so
  import maps / relative URLs / the virtual `lumen://inline-N` base cannot drift. Import
  attributes (`with { type: 'json' }`) and dynamic `import()` use V8's native machinery (the
  callback's `FixedArray`; `set_host_import_module_dynamically_callback`), so the Phase 0
  `import_attributes.rs` preprocessor is rquickjs-only; `import.meta` keeps the shared
  `import_meta.rs` transformer because its `.url`/`.resolve()`/`.env` shape is Lumen policy.
  19 tests in `v8_esm.rs`. **Gap:** the shell never calls `register_module_source`, so a
  page's `import './x.js'` still fails "module not found" —
  [BUG-446](../bugs/BUG-446-FIXED.md), engine-independent (rquickjs had it too).
- **Service-worker scope: network `importScripts`, real `fetch`, `indexedDB`,
  `MessageChannel` ([P3], 2026-08-17, `sw_worker.rs`).** The scope had no `importScripts` at all,
  a `fetch` that answered only from CacheStorage, and neither `indexedDB` nor `MessageChannel` —
  so a worker that opens with a library import (push SDK, workbox) threw on its **first line** and
  registered no listener at all; the live `tbank.ru/invest/sw.js` did exactly that. **Four
  non-obvious points.** (1) *The worker's own network must bypass the interceptor* — new
  `JsFetchProvider::fetch_bypassing_sw` (default = `fetch_sync`, real split in
  `HttpClient::fetch_request_impl`): through the ordinary path the request would reach
  `ServiceWorkerInterceptor`, which picks the worker by scope prefix and posts it a message — but
  that worker is standing inside its own `fetch` and cannot read the message, so the thread would
  wait on itself. The test asserts the counter of *intercepted-path* calls stays 0, not just that
  a body came back. (2) *`indexedDB`/`MessageChannel` are slices, not copies* — both blocks were
  cut out of the page shim into `dom.rs::IDB_SHIM`/`MESSAGE_CHANNEL_SHIM` (the pattern
  `EVENT_TARGET_SHIM`/`URL_SHIM` established for BUG-401) and are evaluated by both scopes; their
  publication switched from `window.X` to `globalThis.X`, which is the same object on the page
  (BUG-280) and the only one in a worker. `web_api_shim_splices_its_parts_in_source_order` guards
  the reassembly. (3) *`atob` is not usable for response bodies here* — the SW's native decodes
  through `String::from_utf8` and answers `undefined` for anything else, so the shim carries its
  own base64→byte-string decoder and marks network/cache-born bodies `_binary`, decoding UTF-8
  only for those (a body the worker built itself must not be decoded twice). (4) *`importScripts`
  runs through indirect `eval`* so declarations land in the global scope, and it **throws** on a
  non-2xx status — a worker with a half-loaded library is worse than one that visibly failed.
- **Module graph fetched level by level, each level in one parallel batch ([P3], 2026-08-17,
  `v8_esm.rs::prefetch_graph`).** `ResolveModuleCallback` must return a module *synchronously*,
  so every registry miss inside it is a blocking network round trip on the JS thread; a
  code-split bundle therefore paid the **sum** of its round trips (`tbank.ru/login/`: 31 chunks
  × ~63 ms = 1937 ms of frozen JS thread, measured with `LUMEN_ESM_TRACE=1`). Now a freshly
  compiled module's `Module::get_module_requests()` is read *before* `instantiate_module`, each
  request resolved through the same `resolve_specifier_with`, and the whole level fetched by 8
  scoped threads at once — the same shape as HTML LS §8.1.3.2 "fetch the descendants of a
  module script". Same page: 2 batches, 367 ms. **Three non-obvious points.** (1) *Compiling the
  level is not the goal, it is the only way to see the next one* — imports are readable only off
  a parsed module, so each round compiles what it just fetched. A module already in the map is
  reused, never recompiled: a second `Module` under the same key would run the body twice.
  (2) *Prefetch errors are swallowed on purpose* (`tc_scope` around the compile, `failed` map for
  the network): a 404, a parse error or a dead host must reach the page from the resolve callback
  exactly as without prefetch, one exception per import — the prefetch only warms the registry.
  (3) *Worker threads must not touch the registry* — `EsmState` is `thread_local!` to the isolate
  thread, so `fetch_over_network` takes the provider by argument and returns raw text; the JS
  thread alone writes `sources`/`failed`. Levers: `LUMEN_NO_ESM_PREFETCH=1` (rollback),
  `LUMEN_ESM_TRACE=1` (census). The A/B gate is the peak count of concurrent module requests
  (≥2 with prefetch, exactly 1 without), not wall-clock.
- **Live "prepare the script element" — dynamically inserted `<script>` ([P3],
  2026-08-09, closes [BUG-571](../bugs/BUG-571-FIXED.md)).** Script execution used to be
  the shell's one-shot walk of the parsed tree (`main.rs::collect_scripts_ordered`, once per
  navigation), so a `<script>` built with `document.createElement` and appended into the live
  document was inert forever. The live half of HTML LS §4.12.1 now lives in the shim
  (`dom.rs`, `_lumen_script_*` block); the shell is untouched. **Non-obvious, three points.**
  (1) *Membership, not a flag:* `createElement`/`createElementNS` put a `<script>`'s nid into
  `_lumen_script_pending`, and preparation deletes it — so the map is simultaneously the
  "which elements may run" filter and the spec's per-element `already started` flag. Parser
  and fragment-parser (`innerHTML`) scripts never enter it, which is exactly why they stay
  spec-inert with no extra check. (2) *One hook, not thirty:* the insertion hook wraps the
  `_lumen_append_child` / `_lumen_insert_before` **natives** rather than the ~30 shim methods
  that call them (`appendChild`, `insertBefore`, `replaceChild`, `append`/`prepend`/`before`/
  `after`/`replaceWith`, `select.add`, `insertAdjacentElement`, …) — the natives are globals
  set with a plain `ctx.global().set`, so reassigning them from the shim is legal and every
  present *and future* insertion path is covered. Cost when nothing is pending is one property
  read. (3) *Module bodies bypass the shell:* `_lumen_esm_register` /
  `_lumen_esm_register_inline` write straight into `v8_esm`'s thread-local source map (plain
  map writes — a compat-layer native cannot re-enter V8), after which the shim calls dynamic
  `import()`. That `import()` is compiled lazily through `new Function`, not written as a
  literal, because the shim is compiled as one classic script and a host refusing dynamic
  import in that position would take the entire shim down instead of just module scripts.
  **Gap:** `run_dump` never ticks timers, so under `--dump-layout` only the synchronous inline
  path is observable; external/module scripts need a live window (same as any `setTimeout`).
- **The same machinery now carries `<link rel=stylesheet>` ([P3], 2026-08-09, closes
  [BUG-722](../bugs/BUG-722-FIXED.md)).** The block above was written strictly for `<script>`,
  so a dynamically inserted stylesheet link fired neither `load` nor `error` — the shell picks
  such a sheet up for the cascade on its next tree walk, but nothing ever told the page. Since
  `'onload' in link` is `true`, feature detection in the wild takes the event branch over its
  timer fallback, and a loader shaped like `'onload' in o ? o.onload = a : setTimeout(a, 50)`
  waits forever. Everything is renamed `_lumen_script_*` → `_lumen_resource_*` and the pending
  map holds the element **kind** (`'script' | 'link'`) instead of `true`; `_lumen_link_prepare`
  is the `<link>` branch. **Non-obvious:** the `load` is driven by the shim's *own* `fetch` of
  the href, not by a report from the cascade loader — the shell re-collects link hrefs from the
  whole tree (`main.rs::collect_link_hrefs`) and has no per-node completion signal to forward.
  So `load` means "the bytes arrived" (a second, cache-warm request), not "the sheet is in the
  cascade" — the same approximation the `<script>` path already makes. Deliberately narrow:
  `rel=stylesheet` only (giving `preload`/`prefetch` an event would mean fetching resources the
  page never asked for) and `createElement`-minted links only, exactly like scripts.
- **`document.currentScript` ([P3], 2026-08-09, closes
  [BUG-486](../bugs/BUG-486-FIXED.md), last blocker of
  [BUG-703](../bugs/BUG-703-FIXED.md)).** Missing from the engine outright until now. It is a
  **stack** (`_lumen_current_script_stack`), not a single slot: a classic script may
  synchronously insert and run another one, and the outer script has to see itself again once
  the inner one returns. Push/pop brackets the body on both execution paths — the dynamic one
  in `_lumen_script_execute_classic(text, nid)` and the parser one in the shell, where
  `ScriptSource` now carries the `<script>`'s `NodeId` all the way through
  `resolve_script_sources` into `run_scripts_with_dom` (which brackets each classic `eval`,
  error branches included, or one throwing script would leave a stale value behind for every
  script after it). Modules, event handlers and any asynchronous callback read `null` for
  free — the stack is empty by the time a task or microtask runs, which is exactly the spec's
  rule. **Non-obvious:** the property must also exist on detached documents (always `null`)
  — feature detection that reads `undefined` there takes a different branch. **Why it
  matters beyond WPT:** self-locating bundles key themselves off it; tbank.ru's 44 micro-block
  bundles all registered under the key `undefined` (`currentScript.dataset.mmid`), overwriting
  each other, so the app found none of its 81 blocks and rendered a blank page.

- **Document Picture-in-Picture reaches a real OS window ([P1] P3-pip, 2026-07-17).**
  `documentPictureInPicture.requestWindow({width,height})` (`document_pip.rs`) called
  `_lumen_pip_request_window(width,height)` — but that native was never registered, so the
  call silently no-op'd and no window ever opened. `pip_bindings.rs` now registers it
  (`into_v8_fn2`), enqueuing a new `PipRequest::OpenDocument { width, height }` the shell drains
  into `open_pip_os_document` (real sized always-on-top winit window). `PictureInPictureWindow`
  is unified: `document_pip.rs` defines the class (it installs first, alphabetically) and
  `video_pip.rs` reuses `globalThis.PictureInPictureWindow` rather than defining a rival one
  (falling back to its own minimal definition only when evaluated in isolation, e.g. its own
  rquickjs unit tests). Both PiP paths publish the active window as
  `globalThis.__lumen_pip_active_window`, so `_lumen_pip_deliver_resize(w,h)` (shell → JS on
  OS-window resize) updates whichever session is open and fires its `resize` event. Follow-up
  at the time: forwarding the page's real DOM content into the document-PiP window
  (`PictureInPictureWindow.document` was still an in-memory stub) — done in later slices
  (`p1-docpip-content`/`p1-docpip-authorcss`, see `ROADMAP.md` P3-pip). 6 new tests across
  `pip_bindings.rs` + `document_pip.rs`.
- **`CustomStateSet` reflects into `data-lumen-state-<name>` ([P1] P3-customstate, 2026-07-17).**
  `element_internals.rs`'s `ELEMENT_INTERNALS_SHIM` (shared by both engines — QuickJS
  `install_element_internals_bindings` and V8 `install_element_internals_bindings_v8` eval the
  same string) — `add`/`delete`/`clear` now call `_lumen_set_attr`/`_lumen_remove_attr` on the
  host element, same sentinel-attribute bridge as `:fullscreen`/`:modal`. The CSS-side matcher
  (`PseudoClass::State`, `layout/src/style.rs`) landed earlier (2026-07-15) on the mistaken
  assumption this reflection already existed — it didn't; `CustomStateSet` only held an in-memory
  `Set`. 2 new tests (add/delete/clear round-trip, delete-of-absent-state no-op).
- **`structuredClone` — spec-conformant cycles/typed-arrays/DataCloneError
  (`P3-structclone` partial, [P1] 2026-07-18).** The shared `WEB_API_SHIM`
  implementation (`dom.rs`) now threads a `memory` Map (original → clone) through
  a nested `clone()` recursion, so self-referential and diamond-shared object
  graphs round-trip with preserved identity instead of overflowing the stack.
  Added: BigInt passthrough; `Boolean`/`Number`/`String` wrapper objects;
  `ArrayBuffer` (deep-copied via `slice(0)`) and every typed-array/`DataView`
  view (re-viewed over a single cloned backing buffer, so two views of one
  buffer stay one buffer after cloning); `SharedArrayBuffer` passed by reference.
  Non-serializable values (functions, symbols) now throw a `DataCloneError`
  `DOMException` per HTML LS §2.7, instead of the old silent passthrough/drop.
  Still deferred: the `transfer` option (transferables aren't detached — they're
  copied). Validated by 9 new `dom::tests::structured_clone_*` (rquickjs) plus a
  consolidated V8 mirror `v8_runtime::tests::structured_clone_cycles_typed_arrays_and_dataclone_error`
  (`--features v8-backend`, the default engine per ADR-018).
- **`_lumen_dispatch_pointer_move_coalesced` — real Pointer Events L3 §4.1
  `getCoalescedEvents()`/`getPredictedEvents()` (`P3-pointerfull`, 2026-07-17).**
  New engine-agnostic `WEB_API_SHIM` function (`dom.rs`, registered on
  `window`): takes a JSON array of `[x, y]` CSS-px samples batched by the
  shell (`Lumen::flush_pointer_moves`, `crates/shell/src/main.rs`) and builds
  one `PointerEvent` per point, dispatching the last as the "main" event via
  `_lumen_dispatch_rich`. `getCoalescedEvents()` returns the full list (main
  event last, by reference); `getPredictedEvents()` linearly extrapolates 2
  points from the last two samples' velocity, `[]` below 2 samples. The
  older `_lumen_dispatch_pointer_event`'s `[ev]`/`[]` stubs were kept
  unchanged for non-move types (down/up/enter/leave/over/out,
  `_lumen_dispatch_capture_event`) and one-off synthetic pointermove
  (pointer-lock, automation) — already spec-correct for a genuinely single,
  non-coalescing event. 3 new tests (`dom::tests::pointer_move_coalesced_*`),
  green under both QuickJS (default) and `--features v8-backend`. See
  `subsystems/shell.md` for the shell-side buffering; `docs/tasks/ph3-pointer-events-l3.md`.
- **`window` is now the engine's real global object (BUG-280 fix, [P2] P2-wpt S4, 2026-07-16).**
  `WEB_API_SHIM` copies every own property of `window` onto `globalThis` (values via plain
  assignment — required because some quickjs-ng built-ins like `addEventListener` are
  non-configurable-but-writable, `defineProperty` would throw on them; accessors via
  `Object.defineProperty` to preserve the live getter/setter rather than freezing a one-time
  read), then `window = globalThis`. From then on `window === self === globalThis`, matching
  the real-browser invariant, so a property assigned via `window.x = ...`/`self.x = ...` —
  including dynamic, not-known-in-advance names (`testharness.js`'s `expose(fn, name)`) — is
  reachable as a bare identifier, same as any real browser. Shared `WEB_API_SHIM` source fixes
  both engines (V8 `v8_runtime.rs` evaluates the same string). Verified: mirrored unit-test
  pairs `dynamic_window_property_is_bare_reachable`/`dynamic_self_property_is_bare_reachable`
  in both `dom::tests` (rquickjs) and `v8_runtime::tests` (V8, `--features v8-backend`), plus
  live BiDi `script.evaluate` probes against the default V8 dev-release build.
  Follow-up fix alongside: `File.prototype` now extends `Blob.prototype` (`file_input.rs`, W3C
  File API §4) — this fix made `window.File` reach the real global `File`, surfacing the missing
  prototype link. Fixing BUG-280 got the WPT smoke test far enough to expose a second, unrelated
  blocker — [BUG-291](../bugs/BUG-291-FIXED.md), now fixed (see below).
- **`document.getElementsByTagName(tag)` (BUG-279 fix, [P2] P2-wpt S4, 2026-07-13).**
  Was missing entirely from `var document = {...}` in `dom.rs` — broke `testharness.js`'s own
  module-level setup (`test_timeout()`/`get_script_url()` call it unconditionally) with a
  `TypeError`. Delegates to the existing `_lumen_query_selector_all` bridge, same pattern as
  `querySelectorAll` (a bare tag name / `*` is already a valid CSS type/universal selector).
  Returns a static array, not a live `HTMLCollection` (same simplification `querySelectorAll`
  already makes). `Element.prototype.getElementsByTagName` is still missing (out of scope)
  — closed later by BUG-416 below, **which also replaced the delegation described here**.
- **`getElementsByTagName`/`getElementsByTagNameNS` on `Element` + `…NS` on `document`
  (BUG-416 fix, [P3] 2026-08-22).** The element had neither method and the document had no
  `…NS`; fixing that also had to undo the delegation the BUG-279 entry above describes,
  because a CSS type selector is the wrong matcher for a tag name: `matches_simple`
  (`style.rs:9602`) compares it to the local name by **exact string equality** and
  `parse_ident` does not fold case, so `document.getElementsByTagName('DIV')` answered `[]`
  instead of every `<div>`, and a non-identifier name (`'a b'`) parsed as some other selector.
  Name matching now lives in the shim as three shared helpers —
  `_lumen_tag_name_predicate` / `_lumen_tag_ns_predicate` / `_lumen_collect_matching` — and
  the selector engine is used only as the tree-order subtree walker (`'*'`). DOM LS §4.5
  verbatim: an HTML-namespace element folds ASCII case, anything else (SVG/MathML/no
  namespace) matches its qualified name exactly, `…NS` treats `null` and `''` alike as "no
  namespace" with `'*'` a wildcard in either position and a case-sensitive local name.
  A `null` local name is what the native side answers for every non-element node, so it
  doubles as the element check — that is what keeps a `'*'` ask off text nodes. All three
  consumers share the predicates: live document, live element wrapper (descendants only) and
  the detached document ([BUG-415](../bugs/BUG-415-FIXED.md)), whose own `'*'` branch used to
  return text nodes because `_detached_walk` visits every child. Two things the fix cannot
  reach: markup-parsed foreign content is still HTML-namespaced ([BUG-685](../bugs/BUG-685-OPEN.md)),
  so the case rule is only observable on `createElementNS`-built subtrees; and
  `'getElementsByTagName' in Element.prototype` stays `false` like every other member of this
  wrapper factory ([BUG-747](../bugs/BUG-747-OPEN.md)).
- **`getElementsByClassName(names)` on `document` + `Element` (BUG-302 fix, [P3] 2026-07-19).**
  Was missing from the main `WEB_API_SHIM` (only `dom_parser.rs`'s `VElement`/`VDocument` had it)
  — `news.ycombinator.com` scripts died wholesale on `el.getElementsByClassName is not a function`.
  Helper `_lumen_class_selector(names)` splits the whitespace-separated token list, drops empties
  and builds a compound class selector (`.a.b`); an empty token list returns `null` so callers
  short-circuit to `[]` (a `''` selector would throw). The `document` variant delegates to
  `_lumen_query_selector_all`, the `Element` variant to the scoped `_lumen_query_selector_all_scoped`
  (descendants only). Static array, not a live `HTMLCollection`, same as `getElementsByTagName`.
  Known limitation (as with `getElementsByTagName`): class tokens are spliced into the selector
  without CSS escaping — exotic class names with special chars are unsupported.
- **`document.getElementsByName(elementName)` + the `NodeList` interface (BUG-412 fix, [P3] 2026-08-20).**
  The third accessor of HTML LS §3.1.5 was missing outright (`'getElementsByName' in document` ===
  `false`), so every call threw `is not a function`. Unlike its two neighbours above it returns a
  **live** collection: `_lumen_make_nid_collection` — the `Proxy` `document.images` already uses —
  over `_lumen_elements_named_nids(name)`. Three details worth keeping in mind when touching it:
  (1) the member query is the fixed selector `[name]` plus a JS string comparison, **not** a
  `[name="…"]` selector, so an argument with quotes/backslashes/newlines needs no CSS escaping
  (the known limitation of `getElementsByClassName` above does not apply here);
  (2) members are filtered by `_lumen_is_html_namespace`, because the spec matches HTML elements
  only — with markup-parsed foreign content still landing in the XHTML namespace
  ([BUG-685](../bugs/BUG-685-OPEN.md)) that filter only bites for `createElementNS`-built nodes so far;
  (3) `NodeList` (DOM §4.2.10.1) is a separate marker interface from `HTMLCollection` — HTML LS
  requires this accessor to return one, and the WPT interface test asserts `instanceof NodeList` **and**
  `!(instanceof HTMLCollection)`. To keep one `Proxy` implementation for both, the shared factory took a
  third parameter `noNamed`, which drops the named half (`namedItem`, `list['someName']`, named
  own-keys): that is `HTMLCollection` behaviour (DOM §4.2.10.2) and a `NodeList` must not have it.
- **`Image` / `HTMLImageElement` constructors (BUG-305 fix, [P3] 2026-07-19).**
  Both were absent from `WEB_API_SHIM`, so `new Image()` (image preloading, tracking pixels, canvas
  sources) threw `Image is not defined` and took the whole script down (ria.ru). Added two global
  declarations before the `document` literal: `function HTMLImageElement() {}` (bare interface global;
  `instanceof` was not wired at the time — element wrappers were plain objects, since fixed by BUG-322)
  and `function Image(width?, height?)`
  which does `document.createElement('img')`, sets the width/height content attributes from its args,
  and **returns** the element (a returned object wins over `this`), so `new Image()` yields a native
  `<img>` wrapper that participates in layout/paint. Companion change: `_lumen_build_element` now
  reflects the `src` content attribute (`get/set src`, shared by `<img>/<script>/<iframe>/<source>`),
  so `img.src = …` reaches layout; the getter returns the raw attribute string (URL resolution
  deferred), empty string when unset. `onload`/`onerror` on JS-initiated fetch stays deferred.
- **Namespaced attribute accessors on `Element` (BUG-309 fix, [P3] 2026-07-19).**
  `setAttributeNS`/`getAttributeNS`/`removeAttributeNS`/`hasAttributeNS` were absent — WPT
  `dom/nodes/Element-hasAttribute.html` §1 threw `el.setAttributeNS is not a function`. Added the
  four methods to `Element.prototype` right after `hasAttribute`. Lumen's attribute model is
  name-only, so the `namespace` argument is accepted but ignored: each method stores/looks up the
  attribute under its qualified name, matching the name-based `getAttribute`/`hasAttribute` lookup;
  `setAttributeNS` fires the custom-element `attributeChangedCallback` hook exactly like
  `setAttribute`. The Attr-node variants (`getAttributeNodeNS`/`setAttributeNodeNS`) are omitted —
  the base `getAttributeNode`/`setAttributeNode` do not exist in the shim either (no Attr node
  objects), so adding only the `NS` forms would be inconsistent.
- **ElementTraversal + `ParentNode.children` + `Node.parentNode` (BUG-310 fix, [P3] 2026-07-19).**
  `childElementCount`/`firstElementChild`/`lastElementChild`/`nextElementSibling`/
  `previousElementSibling` were entirely absent, `.children` was a bare array (no `.item()`), and —
  surprisingly — element wrappers had only `parentElement`, never `parentNode` (so
  `node.parentNode.children` threw `Cannot read properties of undefined`). Fix, all in the shared
  `WEB_API_SHIM`: element-only helpers `_lumen_is_element_nid` (element = not a text node and
  tag-name not `#`-prefixed — `_lumen_get_children` returns text/comment children too) and
  `_lumen_element_child_nids` (element children in tree order); the five traversal accessors on the
  element wrapper (siblings locate the node among the parent's element children and step past text);
  a `parentNode` getter mirroring `parentElement`. `children` (element + `DocumentFragment`) is now
  a live `HTMLCollection` built by `_lumen_make_html_collection` over a `Proxy` — `length`, numeric
  indices, `item(i)` and `namedItem(name)` re-query the live tree on every access; the prototype is
  the marker `HTMLCollection` (exposed as `window.HTMLCollection`) so `x instanceof HTMLCollection`
  holds. Consumers that index `el.children[i]`/read `.length` are unaffected (no array methods were
  used on `.children`). `Element-children.html`'s two subtests were WPT-FAIL at the time (needed
  `ownKeys`/`getOwnPropertyDescriptor` traps for enumeration, and a `[[Prototype]]` on element
  wrappers for `instanceof`) — both closed since, see BUG-323 and BUG-322 below.
- **`Node.isConnected` (DOM §4.4, BUG-311 fix, [P3] 2026-07-19).** Was entirely absent (returned
  `undefined`). Added as a getter on the element wrapper in the shared `WEB_API_SHIM`: a node is
  connected iff `documentElement` (`<html>`, via `_lumen_get_html_element`) is on its ancestor chain
  (walked with `_lumen_get_parent`) or is the node itself — a detached subtree's topmost ancestor is
  an orphan node, so it reports `false`; after `remove()` a node reports `false` again. WPT
  `Node-isConnected.html` «Test with ordinary child nodes» now PASSes. Known limitation: «Test with
  iframes» stays WPT-FAIL — it needs separate iframe sub-documents via `contentDocument`, which the
  shim does not model as distinct connected trees.
- **`document.createProcessingInstruction(target, data)` (DOM §4.5, BUG-313 fix, [P3] 2026-07-19).**
  Was entirely absent. Added to the `document` literal in the shared `WEB_API_SHIM`, plus two
  top-level helpers before it: `_lumen_is_xml_name(s)` (an `^[NameStartChar][NameChar]*$` regexp built
  from the XML 1.0 Name production ranges — BMP only; the astral `#x10000-#xEFFFF` range is omitted
  since no subtest exercises it; the split ranges are what correctly exclude `U+00D7` (×) and `U+00B7`
  (·) from NameStartChar while still admitting `·` as a NameChar, e.g. `A·A`), and
  `_lumen_make_processing_instruction(target, data)` (a detached JS-only CharacterData node — PIs never
  participate in layout — exposing `target`/`data` (mutable)/`nodeValue`/`nodeType 7`/`nodeName`/
  `length`/`ownerDocument`). The method throws `DOMException(…, 'InvalidCharacterError')` (legacy code
  5, set by the `v8_runtime.rs` DOMException polyfill) when `target` is not a valid XML Name or `data`
  contains `?>`. Closes 8 of the 11 WPT `Document-createProcessingInstruction.html` sub-fails (the
  `INVALID_CHARACTER_ERR` group). The remaining 3 (`Should get a ProcessingInstruction …`) assert
  `pi instanceof ProcessingInstruction` / `pi instanceof Node` — now satisfied by BUG-314 (the PI
  object's `[[Prototype]]` is `ProcessingInstruction.prototype`). 3 unit tests
  (`dom::tests::create_processing_instruction_*`).
- **DOM node-family interface globals + `Comment`/`Text`/`DocumentFragment` constructors (DOM §4,
  BUG-314 fix, [P3] 2026-07-20).** Node-family interfaces were entirely absent as globals, so a bare
  `x instanceof Node` or `window['Comment']` threw `X is not defined` / `is not a constructor`, taking
  whole scripts (and testharness feature-detection) down. New "DOM interface constructors" block in the
  shared `WEB_API_SHIM`: (1) bare, non-constructible interface globals for reference/`instanceof`
  resolution — `Node`/`Element`/`CharacterData`/`Attr`/`Document`/`DocumentType`/`ProcessingInstruction`
  /`HTMLElement` (hoisted function declarations) plus ~36 `HTML*Element` generated via `globalThis[name]`
  with an `name in globalThis` guard (so the richer pre-existing `HTMLImageElement`/`Image` pair is not
  clobbered); prototypes chain `HTML*Element → HTMLElement → Element → Node`. (2) constructible
  `new Comment(data)`/`new Text(data)` build detached CharacterData objects via
  `_lumen_make_character_data(nodeType, nodeName, data, proto)` with the full prototype chain
  (`Comment.prototype → CharacterData.prototype → Node.prototype`) and working `instanceof`; `data` is
  stringified per DOM §4.5 (undefined → `''`, first argument only). `new DocumentFragment()` returns a
  native (arena-backed) empty fragment; `_lumen_make_document_fragment` gained `ownerDocument`/
  `firstChild`. Element wrappers stayed plain native-backed objects at the time, so
  `el instanceof HTMLDivElement` was still false — closed by BUG-322 below. 4 unit tests
  (`dom::tests::{comment_text_constructors_build_nodes, character_data_prototype_chain,
  document_fragment_constructor, dom_interface_globals_defined}`).
- **`document.doctype` + constructible `new Document()` (DOM §4.5/§4.9, [BUG-321](../bugs/BUG-321-FIXED.md)
  fix, [P3] 2026-07-20).** Follows up BUG-314's node-family work. New natives in **both** engines
  (`dom.rs` + `v8_runtime.rs`): `_lumen_is_doctype(nid)`, `_lumen_get_document_doctype()` (the document's
  doctype child), `_lumen_get_doctype_field(nid, 'name'|'public'|'system')`. Shim: `_lumen_make_doctype(nid)`
  builds a DocumentType wrapper (`nodeType 10`, prototype `DocumentType.prototype` → `instanceof` works),
  interned in the shared `_lumen_element_wrappers` cache so `document.doctype === document.childNodes[1]`
  (same object) and `_lumen_gc_collect` purges it. `document` gained kind-aware `childNodes` (maps children
  through `_lumen_make_node`, which routes doctype→`_lumen_make_doctype`, else `_lumen_make_element`) and a
  `doctype` getter. `Document` is now constructible — a detached document tracking its children in a JS
  array with `createElement`/`createTextNode`/`appendChild`, a `doctype` getter (scans for `nodeType 10`,
  `null` on a fresh document) and `documentElement`. Passes both WPT `dom/nodes/Document-doctype.html`
  subtests. Element-wrapper `instanceof HTMLXElement` (BUG-321 item 3) is NOT part of this — spun off
  as BUG-322 (fixed below). 3 unit tests
  (`dom::tests::{document_doctype_is_document_type, document_doctype_null_when_absent,
  new_document_constructor}`).
- **HTMLCollection `for-in`/`Object.getOwnPropertyNames` enumeration (DOM §4.2.6.2, BUG-323 fix,
  [P3] 2026-07-21).** The live `HTMLCollection` `Proxy` (`_lumen_make_html_collection`) had no
  `ownKeys`/`getOwnPropertyDescriptor` traps, so enumerating one (`for...in`, `Object.keys`,
  `Object.getOwnPropertyNames`) silently yielded nothing despite `length`/`item()`/`namedItem()`
  all working. Fix: both traps added, backed by a new `_lumen_html_collection_own_names` helper
  (same id-then-name pass as the existing `_lumen_html_collection_named`) — numeric indices
  `enumerable: true`, `id`/`name`-derived keys `enumerable: false` (visible via
  `getOwnPropertyNames`/`hasOwnProperty`, not `for-in`), matching real engines. Unit test
  `html_collection_supports_enumeration`.
- **Native element/text wrapper `[[Prototype]]` chain — `instanceof Element`/`Node`/`HTML*Element`/
  `Text`/`CharacterData` (DOM §4.9/§4.4, HTML §3.1.3, [BUG-322](../bugs/BUG-322-FIXED.md) fix,
  [P1] 2026-07-21).** `_lumen_build_element` (the single builder behind `_lumen_make_element`, used
  for both element and text nodes) always returned a plain object with no `[[Prototype]]`, so
  `document.body instanceof Element`/`Node`/`HTMLElement`, `document.createElement('div') instanceof
  HTMLDivElement`, etc. were always `false` — the gap BUG-314/BUG-321 explicitly deferred. Fix: a
  `_lumen_html_tag_prototypes` table (`TAGNAME → HTML*Element` constructor, ~40 common tags; tags
  without an entry fell back to plain `HTMLElement`, later split into `HTMLElement` vs
  `HTMLUnknownElement` by BUG-367 below) plus
  `_lumen_element_prototype_for(nid)` (non-HTML-namespace nodes — SVG/MathML — get the generic
  `Element.prototype`; the SVG shim's `createElementNS` decorator, which re-points results at typed
  `SVG*Element.prototype` afterward, chains through `Element.prototype` too via `class SVGElement
  extends Element`, so the two don't conflict). One `Object.setPrototypeOf` call at the end of
  `_lumen_build_element` — `Text.prototype` for text nodes (`_lumen_is_text_node`), else the
  tag-resolved prototype. Every accessor/method on the wrapper is still an own property, so it
  shadows the inherited chain. Fixed a latent hole found along the way: `HTMLImageElement` (BUG-305)
  had no `.prototype` chain at all (bare `Object.prototype`) — would have broken `instanceof
  Element`/`Node` specifically for `<img>` once tag-mapped; now matches the other `HTML*Element`
  interfaces (`Object.create(HTMLElement.prototype)`). Custom elements and parsed (non-
  `createElementNS`) SVG/MathML markup are untouched — separate, non-overlapping perimeters. Unit
  test `element_prototype_chain_instanceof`; full `lumen-js` suite green (2506+68, `--features
  v8-backend`).
- **DOM §4.10 `CharacterData` interface methods + real `Comment` node identity ([P1] 2026-07-21).**
  `CharacterData.prototype` gained `length`/`substringData`/`appendData`/`insertData`/`deleteData`/
  `replaceData` (all defined once on the shared prototype, so `Text`, `Comment` and
  `ProcessingInstruction` all inherit them; offset/count follow WebIDL `unsigned long` coercion —
  `>>> 0` — and out-of-range offsets throw `IndexSizeError`). Along the way, two pre-existing bugs
  surfaced: (1) `document.createComment(data)` ignored `data` and always built an empty *Text* node
  (`nodeType` 3, not 8, wrong `[[Prototype]]`) — new native `_lumen_create_comment` (mirrored in
  `dom.rs` + `v8_runtime.rs`) plus a `_lumen_is_comment_node` classifier fix `nodeType`/`nodeName`/
  `data`/`[[Prototype]]` (`Comment.prototype`) for both native (arena-backed) and `_nf_accepts`
  (`NodeIterator`/`TreeWalker` `SHOW_COMMENT`) paths; (2) `set_text_content`/`collect_text_content`
  in both engines applied Element/Document "replace all children" semantics even to a leaf
  Text/Comment receiver — detached its (empty) children and appended a *new child* text node instead
  of mutating the node's own string in place, so a second `.data` write read back a stale/duplicated
  value. Fixed by special-casing `NodeData::Text`/`NodeData::Comment` first in both setters/getters.
  3 unit tests (`dom::tests::{create_comment_is_a_real_comment_node,
  live_text_and_comment_data_mutates_in_place, character_data_methods_spec_examples}`); closes most
  of WPT `dom/nodes/CharacterData-{data,appendData,insertData,deleteData,replaceData,
  substringData,surrogates}.html`.
- **Window event handler IDL attributes and how they are dispatched (BUG-392, 2026-08-11).**
  `window.on<type>` attributes are plain nullable properties on the `window` object literal
  in `dom.rs` (`onpopstate`, `onhashchange`, `onunhandledrejection`, `ongamepadconnected`, …)
  — Window has no accessor machinery like the element-side `_lumen_on_handlers`, which is
  keyed by node id and therefore not usable here. Delivery: `window.dispatchEvent`'s generic
  branch calls `window['on' + evt.type]` *after* the explicit
  `_other_win_listeners[type]` listeners, so declaring a new handler attribute needs no
  dispatch-side change. `load`/`error` keep their own earlier branches; the engine's own
  delivery of `hashchange`/`popstate`/`message` calls the handler directly instead of going
  through `dispatchEvent` — hence no double-fire. Before BUG-392 the generic branch skipped
  the attribute entirely, so any `window.onX = fn` outside `load`/`error` (`onscroll`,
  `ongamepadconnected`) was stored where no dispatch path looked.
- **`_lumen_bfcache_blocked()` — bfcache eligibility check (Ph3 `P3-bfcache` level 1, 2026-07-13).**
  Global JS function in `dom.rs` next to `_lumen_fire_page_lifecycle`. Returns
  `true` when any `_ws_instances`/`_sse_instances` entry has
  `readyState === 1` (OPEN — same numeric value for both `WebSocket` and
  `EventSource`), or a `unload`/`beforeunload` handler is registered: either
  via `addEventListener` (both fall through to the generic
  `_other_win_listeners[type]` bucket — `dom.rs`'s `addEventListener` has no
  dedicated case for them) or via the plain `window.onunload`/
  `onbeforeunload` property assignment. Called from the shell via
  `PersistentJs::has_bfcache_freeze_blocker` → `eval_js_value`. 7 unit tests
  (`dom::tests::bfcache_blocked_*`) cover each trigger plus the default-false
  and closed-WebSocket (`readyState === 3`) cases.
- **TextTrack API for `<video>` (P3-webvtt slice 4).** 2026-07-04. `video.textTracks` now exposes the shell-parsed `<track>` cues. New `text_track_store.rs` mirror (`TextTrackStore { tracks: Mutex<HashMap<u32, Vec<TextTrackData>>> }`, keyed by DOM node index, `set/get_text_track_store` process-global like `video_gif_store`); `TextTrackData { kind, label, language, mode, cues: Vec<CueData{id,start,end,text}> }`; `tracks_json(nid)` serializes one video's tracks via `serde_json`. The shell (`Lumen::sync_text_track_store`) fills it after every page-load/bfcache-restore. Native bridge `__lumen_texttracks_json(nid)` added to `video_bindings.rs`; the VIDEO_SHIM builds `TextTrackList` (`length`, indexed, `getTrackById`), `TextTrack` (`kind/label/language/mode`, `cues`→null when disabled, `activeCues` computed from `el.currentTime`, `addEventListener('cuechange')`/`oncuechange`), `TextTrackCue` (`id/startTime/endTime/text`). `cuechange` fires from the timeupdate loop, the `currentTime` seek setter, and a deferred t=0 check; the list is lazily (re)built while empty so late shell population is picked up. Tests: `text_tracks_exposed_from_store`, `text_tracks_empty_without_store_entry` (video_bindings), 3 in `text_track_store`. Deferred: `addTextTrack()`, `mode`-setter re-render. See `subsystems/shell.md` (WebVTT slice 4) for the overlay/clock side.
- **WebSocket sub-protocol + `wasClean` (Ph3 P3-ws Phase B step 2).** 2026-06-25. `new WebSocket(url, protocols)` now forwards the requested sub-protocol(s): the constructor normalises `protocols` (string or array) to a CSV and passes it through the `_lumen_ws_connect(url, csv)` bridge, which splits it into a `Vec<String>` for `JsWebSocketProvider::connect(url, protocols)`. The server-selected protocol travels back in the `Open` poll event JSON (`{"t":"open","protocol":"…"}`, sourced from `JsWebSocketSession::protocol()`) and is surfaced as `ws.protocol` in `_lumen_ws_pump_one`. `CloseEvent.wasClean` is now `true` whenever a Close frame was received (the closing handshake completed) rather than the old `code === 1000` heuristic. Tests: `websocket_subprotocol_surfaced_on_open`, `websocket_subprotocol_string_arg` (mock echoes the first requested protocol). See `subsystems/network.md` for the handshake/trait side.
- **WebSocket close/send ready-state machine (Ph3 P3-ws Phase B step 3).** 2026-06-25. `WebSocket` now enforces the WHATWG/RFC 6455 §7 send/close contract. `send()` in `CONNECTING` throws `InvalidStateError`; in `CLOSING`/`CLOSED` the data is silently discarded but still counted in `bufferedAmount` (UTF-8 byte length via the new `_lumen_ws_bytelen` `TextEncoder` helper for strings, `byteLength` for buffers). `close(code, reason)` validates the code (`1000` or `3000–4999`, else `InvalidAccessError`) and the reason (`>123` UTF-8 bytes → `SyntaxError`), and is idempotent once in `CLOSING`/`CLOSED`. The `CONNECTING`/`OPEN`/`CLOSING`/`CLOSED` constants are duplicated onto `WebSocket.prototype` so instances expose `ws.CLOSING` etc. Tests: `websocket_send_in_connecting_throws`, `websocket_close_code_validation`, `websocket_close_reason_too_long_throws`, `websocket_buffered_amount_in_closing`, `websocket_instance_constants`, `websocket_close_idempotent`.
- **Real WebGPU backend — U-4c Stage 3 sub-step 2 (canvas present).** 2026-06-21. Closes the WebGPU backend: a GPU-rendered texture now appears on the page `<canvas>`. `canvas.getContext('webgpu')` (new branch in `dom.rs` getContext, cached in `_canvas_webgpu_ctxs`) returns a `GPUCanvasContext` bound to the canvas's `__nid__`. The shim's `GPUCanvasContext.configure` allocates a real render-target `GPUTexture` (sized to the canvas) and registers the context in a shim-level `_gpuCanvasContexts` list; `getCurrentTexture` returns it; `unconfigure` destroys it and deregisters. `GPUQueue.submit` collects the texture ids written by render passes in the batch, and after a successful real `_lumen_webgpu_submit` presents every configured canvas whose current texture was a render target this submit (so unrelated compute/copy submits don't blit stale frames). Present native `_lumen_webgpu_canvas_present(nid, textureHandle)` calls `lumen_paint::webgpu_compute::texture_read_rgba` (copy-texture-to-buffer into a `MAP_READ` buffer, strip the 256-byte row padding, swap BGRA→RGBA) and `canvas2d::present_rgba(nid, w, h, rgba)` writes the dense RGBA8 into the `<canvas>` `nid`'s `Context2D` buffer + `mark_dirty`, so the shell's per-frame `flush_canvas_updates` uploads it as `canvas:{nid}`. Graceful fallback: no feature / no GPU / unknown handle → present is a no-op (canvas stays blank, as before). Tests: 3 in `webgpu_compute` (paint, GPU-gated — `texture_read_rgba_present_path_returns_cleared_frame` verifies BGRA→RGBA + row-unpad on real GPU; unknown-handle None), 2 in `canvas2d` (js — present writes pixels + marks dirty; resizes existing canvas), 2 in `webgpu` (js — unconfigure drops texture; present native rejects unknown handle). **U-4c WebGPU backend complete.**
- **Real WebGPU backend — U-4c Stage 2 sub-step 2 (compute pipelines + dispatch).** 2026-06-21. The JS compute path was a Phase 0 no-op (`createComputePipeline`/`getBindGroupLayout`/`createBindGroup`/`beginComputePass`/`dispatchWorkgroups` all returned stubs). Now WGSL actually runs on the GPU. `lumen_paint::webgpu_compute` gained four registries (shaders / compute pipelines / bind-group layouts / bind groups, monotonic handles) and functions: `shader_create` (real `wgpu::ShaderModule`), `compute_pipeline_create` (auto-layout `wgpu::ComputePipeline`, entry point), `pipeline_bind_group_layout` (`getBindGroupLayout(idx)`), `bind_group_create(layout, &[BufferBindEntry])` (buffers bound by `@binding` index), `compute_pipeline_destroy`. `GpuOp` gained `ComputePass { commands: Vec<ComputeCmd> }` (`SetPipeline`/`SetBindGroup`/`Dispatch`); `submit` records them into a real `wgpu::ComputePass` and dispatches. Pipeline/bind-group/layout creation runs under a shared `GPU_LOCK` validation error scope (`guarded_create`) so wgpu validation failures return `None` instead of poisoning later use. Five native bridges (`_lumen_webgpu_shader_create`/`_compute_pipeline_create`/`_pipeline_bind_group_layout`/`_bind_group_create`/`_compute_pipeline_destroy`); the bind-group native parses the entries JSON (lumen-js owns `serde_json`) into `BufferBindEntry` so `lumen-paint` takes no JSON dep. The shim's `GPUShaderModule`/`GPUComputePipeline`/`GPUBindGroupLayout`/`GPUBindGroup` carry opaque `_id`s; `GPUComputePassEncoder` records its command list onto the parent encoder, flushed on `queue.submit`. Graceful fallback: no feature / no GPU → compute is a no-op (WGSL can't run on the CPU), no regression. Tests: 12 in `webgpu_compute` (paint, GPU-gated — incl. `compute_pipeline_doubles_buffer` real round-trip, bad-shader reject, unknown-pipeline submit fail), 22 in `webgpu` (js — incl. `real_backend_runs_compute_shader` end-to-end doubling on GPU, pass-records-op, dispatch y/z defaults). **Stage 2 remaining (deferred):** render pipelines + canvas present.
- **Real WebGPU backend — U-4c Stage 2 sub-step 1 (GPUBuffer).** 2026-06-21. Backs the JS `GPUBuffer` with a real `wgpu::Buffer`. `lumen_paint::webgpu_compute` gained a buffer registry (`OnceLock<Mutex<HashMap<u64, BufferEntry>>>`, monotonic handles): `buffer_create`/`buffer_write` (`queue.write_buffer`)/`buffer_read` (map_async + `poll(Wait)` + get_mapped_range + unmap)/`buffer_destroy`, plus `submit(&[GpuOp])` that runs recorded command-encoder ops (currently only `CopyBufferToBuffer`) in one encoder + `queue.submit`, mirroring how a browser batches work on `GPUQueue.submit`. Five native bridges (`_lumen_webgpu_buffer_create/_write/_read/_destroy/_submit`, gated by feature `webgpu`); `_lumen_webgpu_buffer_read` is a free fn (single `'js`) returning a `Uint8Array` or null. The shim's `GPUBuffer` holds an opaque `_id`; `queue.writeBuffer`, `commandEncoder.copyBufferToBuffer` (both the 5-arg and 3-arg signatures) + `queue.submit`, and `mapAsync`/`getMappedRange`/`unmap` route through real GPU memory, each degrading to the Phase 0 in-memory path when no handle/adapter is present. `mappedAtCreation` is emulated in-memory (the native buffer stays unmapped and writable via queue). `validate_wgsl` now serializes on a `Mutex` (wgpu error scopes are a per-device stack — concurrent validations would catch each other's errors). **Stage 2 remaining (deferred):** compute pipelines + `dispatchWorkgroups`, render pipelines + canvas present. Tests: 9 in `webgpu_compute` (paint, GPU-gated skip — buffer round-trip + copy + bounds), 18 in `webgpu` (js — incl. write→copy→map round-trip, 3-arg copy, mappedAtCreation).
- **Real WebGPU backend — U-4c Stage 1 (adapter info + WGSL validation).** 2026-06-21. `navigator.gpu` was a pure in-memory JS shim (`crates/js/src/webgpu.rs`, Phase 0): fake stub adapter, no shader validation. Stage 1 adds a real wgpu device in `lumen_paint::webgpu_compute` (headless, no surface — same backend choice as the renderer: DX12 on Windows / PRIMARY else; lazily created once in a `OnceLock`, `None` when no GPU). Two native bridges, registered before the shim eval and gated by the new `lumen-js` feature `webgpu` (→ `lumen-paint/backend-wgpu`, enabled by shell's `quickjs`): `_lumen_webgpu_adapter_info()` → real `{vendor,architecture,device,description}` JSON from `wgpu::Adapter::get_info` (vendor PCI-id mapped to `nvidia`/`amd`/`intel`/…), and `_lumen_webgpu_validate_shader(code)` → real WGSL compilation error text via a device error scope (`push_error_scope(Validation)` + `create_shader_module` + `pop_error_scope`). The shim's `GPUAdapterInfo` now reflects the real GPU, and `GPUShaderModule.getCompilationInfo()` returns real diagnostics. Graceful fallback: no feature / no GPU → original stub behaviour, no regression. Tests: 4 in `webgpu_compute` (paint, GPU-gated skip), 15 in `webgpu` (js). `navigator.gpu` was a pure in-memory JS shim (`crates/js/src/webgpu.rs`, Phase 0): fake stub adapter, no shader validation. Stage 1 adds a real wgpu device in `lumen_paint::webgpu_compute` (headless, no surface — same backend choice as the renderer: DX12 on Windows / PRIMARY else; lazily created once in a `OnceLock`, `None` when no GPU). Two native bridges, registered before the shim eval and gated by the new `lumen-js` feature `webgpu` (→ `lumen-paint/backend-wgpu`, enabled by shell's `quickjs`): `_lumen_webgpu_adapter_info()` → real `{vendor,architecture,device,description}` JSON from `wgpu::Adapter::get_info` (vendor PCI-id mapped to `nvidia`/`amd`/`intel`/…), and `_lumen_webgpu_validate_shader(code)` → real WGSL compilation error text via a device error scope (`push_error_scope(Validation)` + `create_shader_module` + `pop_error_scope`). The shim's `GPUAdapterInfo` now reflects the real GPU, and `GPUShaderModule.getCompilationInfo()` returns real diagnostics. Graceful fallback: no feature / no GPU → original stub behaviour, no regression. **Stage 2 (deferred):** real `GPUBuffer` create/write/map, compute pipelines + `dispatchWorkgroups`, render pipelines + canvas present. Tests: 4 in `webgpu_compute` (paint, GPU-gated skip), 15 in `webgpu` (js).
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
  - **BUG-291 fix (P2-wpt, 2026-07-17).** `Element`/`DocumentFragment`/`ShadowRoot.querySelector(All)` now use `lumen_layout::query_all_scoped(doc, scope, sel)` (new natives `_lumen_query_selector(_all)_scoped`) instead of the document-global `query_all` — scoped to the calling node's descendants (DOM Parentnode §4.2.5), which also makes them work on subtrees not yet attached to the document. `document.querySelector(All)` is unchanged (still whole-document). Also: node-wrapper identity is now stable under `===` — `_lumen_make_element` interns wrappers in `_lumen_element_wrappers[nid]` (purged per-nid by the existing idle `_lumen_gc_collect` tick) instead of minting a fresh object on every call. Added `insertAdjacentText`/`insertAdjacentElement` (were entirely missing).
  - **BUG-368/BUG-351 fix (P3, 2026-08-06).** `innerHTML =` above had been a Phase-0 text-only stub since 2026-05-20 (this bullet's own line falsely implied it worked) — the setter stored the assigned markup as a single text node, the getter returned plain `textContent`. Fixed with a real HTML fragment serializer/parser in `v8_runtime.rs`: `serialize_node`/`serialize_children` (HTML LS §13.3, escaping + void-element list) and `parse_html_fragment`/`import_node` (drives `lumen_html_parser::parse`, then recreates the parsed nodes in the live document — node arenas are per-`Document`, a `NodeId` cannot cross documents). New crate dependency: `lumen-html-parser` (workspace-internal, added to `crates/js/Cargo.toml`). Same serializer/parser now backs `outerHTML` get/set and `insertAdjacentHTML` (added to the live `Element`, both previously entirely missing — BUG-351), via two new natives `_lumen_get_outer_html`/`_lumen_parse_html_fragment` and delegation into the existing `replaceWith`/`before`/`prepend`/`append`/`after` JS helpers. Not implemented: HTML LS §13.4 fragment-context tree-construction adjustments (parses as a full document; no effect on non-table/non-foreign-content markup).
  - 19 DOM tests + 16 runtime tests = 35 total. All pass.
  - Shell integration: `run_scripts_with_dom` wraps `Document` in `Arc<Mutex<>>`, calls `install_dom`, drops runtime to release Arc clones, recovers `Document`.
- **Fetch API JS shim** (`install_dom_api`, `crates/js/src/dom.rs`). 2026-05-22.
  - 5 native `_lumen_fetch_*` bindings: `_lumen_fetch_sync`, `_lumen_fetch_get_status`, `_lumen_fetch_get_status_text`, `_lumen_fetch_get_headers`, `_lumen_fetch_get_body`. Shared result via `Arc<Mutex<Option<FetchCache>>>`.
  - `install_dom` now accepts `Option<Arc<dyn JsFetchProvider>>` — `None` makes `fetch()` reject immediately.
  - JS classes: `AbortSignal`, `AbortController`, `Headers`, `Response`, `Request`, `fetch()` global + `window.fetch`.
  - `Response.ok` (200–299), `Response.text()` / `Response.json()` returning Promises, `Headers` case-insensitive get/set/has/delete.
  - **BUG-370 (2026-08-10):** `Response`/`Request` became WebIDL interfaces — full Body mixin on both, argument validation in both constructors, `Response.json()`/`error()`/`redirect()` per spec, body-derived `Content-Type`, read-only prototype accessors over `WeakMap`-private slots, `Symbol.toStringTag`, and a `configurable` global `fetch` (declared `_lumen_fetch`, published via `defineProperty` — a top-level `function fetch()` is `configurable: false` and cannot be redefined at all).
  - ✅ **Request headers reach the network** ([BUG-749](../bugs/BUG-749-FIXED.md), fixed 2026-08-17): `init.headers` / `Request.headers` / `XMLHttpRequest.setRequestHeader` are serialised into a flat `[name, value, …]` array, passed as the trailing argument of every `_lumen_fetch_*` binding and reach `HttpClient` through `lumen_core::ext::JsFetchRequest` — one struct per request instead of a fifth parameter on each of the four `JsFetchProvider` methods (the four multiply body × cancellability, which is how the headers channel came to be missing at all). The shim re-fills any header source through a guard-`'request'` `Headers` before serialising, so a page-built `new Headers()` (guard `'none'`) cannot smuggle `Host`/`Cookie`/`Origin` through; the wire side filters forbidden names and CR/LF values again, because `setRequestHeader` writes past the `Headers` guard entirely.
  - `AbortController.abort()` sets `signal.aborted = true`.
  - 109 JS tests (was 35 before). All pass.
  - **AA-4 (2026-06-12):** `AbortSignal.abort(reason)` / `AbortSignal.timeout(ms)` (TimeoutError via setTimeout shim) / `AbortSignal.any(signals)` statics per DOM §3.2.2. `any()` adopts the first aborting source's reason and detaches listeners once the race is decided. `onabort` handler fires alongside `addEventListener` listeners (shared `_lumen_abort_signal_fire` helper). `fetch()` pre-flight check: already-aborted `init.signal`/`Request.signal` rejects with `signal.reason` before any network call (Fetch §4.1 step 13).
  - **Ph3 in-flight abort (2026-06-25):** `AbortSignal.timeout(ms)` now records `signal._timeoutMs`; `fetch()` routes a positive deadline to the native `_lumen_fetch_cancellable[_with_body]` bridges (returns `0` ok / `1` net-error / `2` aborted). The JS thread is parked in the synchronous call, so a Rust deadline thread flips an `AbortToken` and the network-layer `AbortWatchdog` tears the socket down; a timed-out request rejects with a `TimeoutError`. A generic `AbortController` signal still only cancels pre-flight (a JS-thread `abort()` can't fire while parked — true JS-observable abort for the controller case needs async fetch). Backed by `JsFetchProvider::fetch_cancellable`/`fetch_with_body_cancellable`.
  - **Ph3 async fetch (2026-06-25):** a live, non-timeout `AbortSignal` now routes `fetch()` through an async path so a generic `AbortController.abort()` fired *during* the request cancels it. The request runs on a worker thread via five bridges (`_lumen_fetch_async_start`/`_poll`/`_abort`/`_commit`/`_free`); `fetch()` returns a Promise immediately and a `setTimeout` poll loop (driven by the existing timer pump — no shell change) resolves on completion or rejects with `signal.reason`/`AbortError` when the `abort` listener flips the `AbortToken` (network `AbortWatchdog` tears the socket down). On success the worker result is committed to the shared `FetchCache` and the response built via `_lumen_response_from_fetch_cache` (was `Response._fromFetchCache` until BUG-370). No-signal and timeout-only fetches keep the synchronous fast path (unchanged semantics for existing callers). Test: `fetch_inflight_abort_rejects_with_abort_error` (blocking mock provider, abort → `AbortError`).
  - **BUG-347 (2026-08-06):** `fetch(input, init)` resolves its URL argument against the document base (`_url_resolve(url, _lumen_document_base_url())`) before reaching any native `_lumen_fetch_*` binding — previously a bare relative URL (`fetch('resources/x.js')`) failed outright as `invalid url: missing scheme`, since the native bindings require an absolute URL and no base was ever threaded in. `new Request(url).url` gets the same treatment (was the un-absolutized companion, tracked as BUG-370 item A2).
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
  - `performance` (W3C HR Time L2): `now()` (DOMHighResTimeStamp, time origin captured at `install_dom` call), `timeOrigin`, stub `mark/measure/getEntriesByName/getEntriesByType/clearMarks/clearMeasures`. Exposed on `window.performance`. **BUG-400 ([P3] 2026-08-11):** was a flat object literal; now a real `interface Performance : EventTarget` (HR Time L3 §4) — non-constructible interface object on `window.Performance`, operations on `Performance.prototype`, singleton built with `Object.create` + `EventTarget.call`, `timeOrigin` a getter-only accessor. Moving the operations off the instance is what makes the WebIDL default `toJSON()` correct: it must serialise attributes only, and an instance with no own enumerable properties can only report `{timeOrigin}`. The legacy Navigation Timing attributes `timing`/`navigation` are deliberately absent, not faked — no navigation milestone data exists anywhere in the engine ([BUG-767](../bugs/BUG-767-OPEN.md), blocked by [BUG-640](../bugs/BUG-640-OPEN.md)).
  - `queueMicrotask(fn)` (HTML LS §8.1.4.4): schedules via `Promise.resolve().then(fn)`; throws `TypeError` for non-function. **BUG-702 ([P3] 2026-08-09):** the `resolve`/`then` pair is captured once, at shim-install time, and kept in a closure — never re-read from the global. A page may legitimately replace `globalThis.Promise` (core-js does it on any site whose feature detection it fails), and such polyfills schedule their reaction jobs through the host `queueMicrotask`; reading `Promise` from the global at call time closed that into unbounded recursion. Any other shim internal that hands a callback to page-replaceable machinery has the same trap.
  - `PromiseRejectionEvent` (HTML LS §8.1.7.5) + `window.onunhandledrejection`/`onrejectionhandled`. **BUG-702 ([P3] 2026-08-09):** the interface exists for construction and feature detection. Defining it is load-bearing on its own: core-js reads a missing `PromiseRejectionEvent` as proof that the native `Promise` is untrustworthy and swaps in its own polyfill, so before this every core-js site ran on a polyfilled `Promise` layered over our microtask machinery. **BUG-716 ([P1] 2026-08-22):** the two events are now actually dispatched. `v8_runtime.rs::install_promise_reject_hook` registers `v8::Isolate::set_promise_reject_callback` once per isolate (alongside `install_dynamic_import_hook`); the callback is a bare `extern "C" fn` with no closure, so it recovers its `HandleScope` from `v8::callback_scope!(unsafe scope, &msg)` rather than from `V8Inner`, which it cannot reach. Rejections are tracked per-isolate in three `thread_local!` lists keyed by the promise's identity hash (`PENDING_UNHANDLED`, `NOTIFIED_UNHANDLED`, `PENDING_HANDLED`) and the actual dispatch is deferred onto the isolate's own microtask queue via `Isolate::enqueue_microtask` — the same technique Node.js/Deno use for this hook — so a `.catch()` attached in the same synchronous turn correctly cancels the notification (HTML LS §8.1.7.5 step 3) instead of racing it. The flush step calls a new shim function, `_lumen_dispatch_unhandled_rejection(type, promise, reason)`, through `Function::call` with the live `Local<Value>`s (not `eval`/JSON — the only other Rust→JS calling convention in the codebase — because that would lose an `Error` reason's class/`.stack` and cannot carry a promise at all). Scope: page-level `window` only; worker global scopes don't define the shim function the flush looks up, so a worker's own rejections are silently unnotified for now — a gap, not a regression (nothing fired there before either), and the broader `window.onerror`/synchronous-`throw` half of the same class of defect is tracked separately as [BUG-591](../bugs/BUG-591-OPEN.md).
  - `window` `'error'`/`onerror` for a synchronous throw (HTML LS §8.1.3.6 "report the exception"). **BUG-591 ([P1] 2026-08-22, partial):** four callback boundaries that used to swallow the exception (`catch(e){}` or a bare `console.error`) now call the shim's `_lumen_report_exception(err)` — `_lumen_tick_timers`, `_lumen_run_raf_callbacks`, `queueMicrotask`'s wrapper (previously an uncaught throw there became an `unhandledrejection` on the untouched wrapper promise instead of `'error'` — the wrong event, not just a missing one), and `_lumen_script_execute_classic` (the DOM-insertion classic-script path, `createElement('script')` + `appendChild`, and the parser's own insertion). The initial page-load classic-script loop (`crates/shell/src/main.rs`) instead calls a new inherent method, `V8JsRuntime::eval_and_report` (`v8_runtime.rs`) — a near-duplicate of `eval()`'s body that additionally reads `v8::Message` (populated by V8 for both compile and runtime errors) for a structured filename/line/column and calls `_lumen_report_exception(exc, filename, lineno, colno)` with the live exception value, mirroring BUG-716's `Function::call`-with-live-`Local<Value>`s technique rather than `eval`/JSON; a lookup failure (shim not installed) is silently skipped, same as BUG-716's flush step. `_lumen_report_exception` itself best-effort-parses `Error.stack`'s first `at file:line:col` frame when no explicit location was passed (every JS-side caller). Independently of whether `'error'` ever fired, `window.dispatchEvent`'s `'error'` branch called `window.onerror` with the `Event` object as its sole argument, like every other `on<type>` handler — wrong, because `onerror`'s WebIDL type is `OnErrorEventHandler`, whose "internal raw handler" takes 5 positional arguments (`message, source, lineno, colno, error`) when the event is genuinely an `ErrorEvent`, and cancels the event on a truthy return; both are now implemented. **Same day, follow-up slice:** exceptions thrown by an `addEventListener`/`on<type>` DOM event listener are now reported too — `_lumen_dispatch`/`_lumen_dispatch_bubble`/`_lumen_dispatch_rich` (native element/document listener paths) and `document.dispatchEvent`'s own listener loop now call `_lumen_report_exception(e)` from their catch arms, and `EventTarget.prototype.dispatchEvent` (the pure-JS base class many Web API shims `extend`) goes through a new `typeof`-guarded wrapper, `_lumen_et_report(e)`, instead of calling `_lumen_report_exception` directly — that shim is also spliced into `WorkerGlobalScope` (`worker_exposed_shim`), which does not carry the page-only `_lumen_report_exception` native, so a direct reference would trade a swallowed exception for a `ReferenceError`. `window.dispatchEvent`'s own `'error'` branch is deliberately left unrouted: it's what `_lumen_report_exception` dispatches into, so wiring it up would let a self-rethrowing `window.onerror`/`'error'` listener recurse forever (regression-tested). **Not in scope:** the Worker parent-side mechanism above and module-script top-level runtime errors (`_lumen_script_run_module`'s `.catch` still only fires the element's own `load`/`error`). 15 new tests total (`dom.rs::tests::v8_core::bug591_*` ×11, `v8_runtime.rs::tests::eval_and_report_*` ×3, `worker.rs::tests_v8::v8_worker_event_target_listener_exception_does_not_reference_error`).
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
  - `MutationObserver` (WHATWG DOM §4.3.2): `observe(target, options)` with full options normalization (`childList`, `attributes`, `attributeFilter`, `attributeOldValue`, `characterData`, `characterDataOldValue`, `subtree`); `disconnect()`; `takeRecords()`. `_mo_notify(nid, type, ...)` fires from primitive wrappers, delivers via `_lumen_flush_mutation_observers()` (sync) and `queueMicrotask` (async production path). **BUG-317 ([P3] 2026-07-20):** `MutationRecord` exposed as a non-constructible interface global (top-level `function` + `window.MutationRecord`, BUG-314 pattern); each record built in `_mo_notify` gets `MutationRecord.prototype` via `Object.setPrototypeOf` so `record instanceof MutationRecord` holds (DOM §4.3.3). **BUG-318 ([P3] 2026-07-20):** record accounting made spec-correct — `observe()` re-registers the observer in `_mo_observers` after a `disconnect()` (only the constructor did before); `subtree:true` is scoped via `_lumen_mo_in_subtree` (parent-chain walk) instead of matching every document mutation; `element.textContent` emits a `childList` record (removed old children + added text node) not `characterData`; live text nodes gained `.data`/`.nodeValue` setters routing through `_lumen_set_text_content` (→ `characterData` with `oldValue`); each record's `target` is the mutated node, `addedNodes`/`removedNodes` are node wrappers, `attributeNamespace` present.
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
  - Pure-JS shim (no native bindings) installed last in `install_dom`, written when QuickJS (default rquickjs features) shipped without ECMA-402. **Since the V8 cutover this is a fallback-only path** (BUG-295, discovered 2026-08-06): the default build's `v8` crate ships **native** `Intl` with full ICU tzdata (confirmed empirically — `Intl.DateTimeFormat().resolvedOptions().timeZone` returns the real host IANA zone, not a stub), so `INTL_SHIM`'s own `if (typeof global.Intl !== 'undefined' ...) return;` guard always defers to it in the default build; the shim only activates on a hypothetical V8 build without ICU i18n data. Re-exported as `window.Intl` when it does activate.
  - Two locales: `en-US` (default fallback for any non-Russian tag) and `ru-RU` (matched on `ru`/`ru-*`). `Intl.NumberFormat` (decimal/currency/percent; `en-US` `,`/`.`, `ru-RU` NBSP/`,`; currency symbols USD/EUR/RUB/GBP/JPY/KRW/CNY; min/max fraction digits, grouping). `Intl.DateTimeFormat` (year/month/day/weekday/hour/minute/second; locale month+weekday names; ru genitive month + "г." suffix when a day is present; default short date `M/D/YYYY` vs `DD.MM.YYYY`; `hour12` defaults true for en, false for ru; `resolvedOptions().timeZone` reads the BUG-295 timezone-override marker below when no explicit `timeZone` option). `Intl.Collator` (locale compare placing `ё` after `е`; `sensitivity:'base'` case-insensitive; `numeric:true`). `Intl.PluralRules` (CLDR cardinal+ordinal; ru resolves one/few/many/other). Plus `getCanonicalLocales` / per-constructor `supportedLocalesOf` / `resolvedOptions`.
  - `Number.prototype.toLocaleString` and `Date.prototype.toLocaleString`/`toLocaleDateString`/`toLocaleTimeString` are rerouted through the matching `Intl` constructor.
  - 19 unit tests (grouping, currency incl. negative sign placement, percent, fraction limits, default+long date, ru genitive, hour12 vs 24h, Cyrillic+numeric+base collation, ru/en plural categories, toLocaleString delegation, locale fallback, supportedLocalesOf). **812 JS lib tests total.**

- **`browser.setTimezoneOverride` live-window wiring** (`crates/js/src/v8_runtime.rs::timezone_override_script`, BUG-295, 2026-08-06).
  - Process-global `GLOBAL_TIMEZONE_OVERRIDE` (mirrors `GLOBAL_UA_OVERRIDE`'s rationale — one JS runtime per process, fresh `V8JsRuntime` per navigation). `install_dom` evals a script setting `globalThis.__lumen_timezone_override` and, the first time on a given context, wraps the **native** `Intl.DateTimeFormat` constructor (`LumenDateTimeFormat`, idempotent via a `__lumenPatched` marker) so a construction without an explicit `options.timeZone` injects the override IANA id before delegating to the original constructor — explicit `timeZone` from calling JS always wins. Because the wrapped constructor is the real ICU-backed one, formatting through it is genuinely DST-aware correct for the overridden zone, not a label-only stub.
  - The `intl_bindings.rs` shim's own `DateTimeFormat` also reads the same `globalThis.__lumen_timezone_override` marker (`this._tz`) for the no-ICU fallback case, though that path isn't exercised by the default build (see the `Intl` shim entry above).
  - Known gap: only `Intl.DateTimeFormat` is wrapped — bare `Date.prototype` methods (`getTimezoneOffset`/`toString`/etc.) still reflect the host timezone, unaffected by the override.

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
  - 13 unit tests (functional object, 2d-delegation + 2d-after-webgl→null + unknown-type→null, context caching, fingerprint normalization, blank toDataURL fallback + no-clobber, clear→readPixels roundtrip, full compile→buffer→draw→readback pipeline, attrib location, non-canvas gets no WebGL stub, lose-context extension). The 19 `no_automation_markers.rs` integration tests still pass.
  - **`WEBGL_SHIM`'s `createElement` wrapper must delegate, never overwrite (BUG-348, fixed 2026-07-29).** It is installed *after* the element factory of `dom.rs::WEB_API_SHIM`, so whatever it assigns to `el.getContext`/`toDataURL`/`toBlob` is the last write and permanently shadows the factory's version — there is nothing downstream to restore it. Between the V8 cutover (2026-07-14) and the fix, its blanket `return null` for non-WebGL context types therefore disabled Canvas 2D for every `document.createElement('canvas')` (canvases reached through `getElementById`/`querySelector` were unaffected — the wrapper never sees them, which is why the symptom looked selective). `_addCanvasStubs` now captures the factory's `getContext` and delegates to it for unhandled types (keeping HTML LS §4.12.4's one-context-per-canvas rule explicit), and installs its privacy stubs only where the factory left none.

- **Functional Canvas 2D context** (`crates/js/src/canvas2d.rs` + `dom.rs`, HTML LS §4.12.4, task 5A.2). 2026-06-02.
  - `install_canvas2d_bindings(ctx)` registers `_lumen_canvas2d_*` natives backed by `lumen_canvas::Context2D` in a per-thread registry keyed by **DOM node index** (`__nid__`). Installed in `install_dom` right after WebGL.
  - The `getContext('2d')` shim lives in `dom.rs::_lumen_make_element` (not the WebGL-style `document.createElement` patch) because element wrappers are ephemeral — every wrapper for a node carries `getContext`/`toDataURL`/reflected `width`/`height`. The `CanvasRenderingContext2D` object is cached in the module-level `_canvas2d_ctxs[nid]` map (same persistence pattern as `_input_values`). The native key `nid` matches `LayoutBox::node.index()`, so the display list's `DrawImage src="canvas:{nid}"` resolves to the right bitmap.
  - Forwarded surface: `fillRect`/`clearRect`/`strokeRect`, `beginPath`/`moveTo`/`lineTo`/`closePath`/`arc`/`fill`/`stroke`, `rect`/`ellipse` (ellipse≈circle in Phase 0), `fillStyle`/`strokeStyle` (via `CanvasColor::from_css_str`), `lineWidth`/`globalAlpha` (spec-validated ranges), `getImageData` (applies per-session fingerprint noise via `Context2D::get_image_data`). (transforms/text/shadows/gradients/patterns/drawImage/clip were stubs in Phase 0 — wired in later phases; see the dated bullets below). `toDataURL`/`toBlob` stay blank (ADR-007 anti-fingerprint).
  - Draw ops mark the canvas dirty; `QuickJsRuntime::flush_canvas_updates()` (→ `canvas2d::flush_dirty()`) drains `(nid, w, h, rgba)` once per frame. The shell registers each via `Renderer::register_image("canvas:{nid}", ...)` and requests a redraw. Unregistered canvases (no JS drawing, e.g. the cpu_raster snapshot driver which runs no JS) render as the transparent `DrawImage` placeholder.
  - 16 unit tests in `canvas2d.rs` (create/clamp/idempotent, fill/clear/stroke/path dirty-tracking, flush-once, line-width/alpha validation, resize, get_image_data, unknown-canvas no-ops, isolation) + 6 e2e tests in `dom.rs` (getContext object/caching, default 300×150, draw→flush, webgl→null, non-canvas→null). Graphic test `57-canvas-2d.html` + demo in `1000000-final.html`.

- **Canvas 2D paint sources wired to JS shim** (`crates/js/src/dom.rs::_lumen_make_canvas2d_ctx`). 2026-06-20.
  - The Phase-3 native paint sources (`_lumen_canvas2d_create_{linear,radial,conic}_gradient`, `_gradient_add_color_stop`, `_set_{fill,stroke}_style_gradient/_pattern`, `_set_shadow_*`, `_draw_image`, `_put_image_data` in `canvas2d.rs`) existed but the `getContext('2d')` JS shim returned **fake stubs** (`createLinearGradient` → `{addColorStop:noop}`, `createPattern` → `null`, `drawImage`/`putImageData` → empty), so none of it reached a page. Now wired:
    - `createLinearGradient/createRadialGradient/createConicGradient` build a `CanvasGradient` via `_lumen_make_canvas_gradient(gid)` carrying `__gid__` + a real `addColorStop`. `fillStyle`/`strokeStyle` setters dispatch on the assigned value: object with `__gid__` → `_set_*_style_gradient`, object with `__patid__` → `_set_*_style_pattern`, else CSS string (old path). Getters return the stored object/string.
    - `createPattern(image, repetition)` → `_lumen_canvas2d_create_pattern(image.__nid__, rep)`, returns `{__patid__}` (or `null` for a source without a backing native bitmap).
    - `shadowColor`/`shadowBlur`/`shadowOffsetX`/`shadowOffsetY` moved off the no-op `_stubProps` list into wired `Object.defineProperty` accessors forwarding to `_set_shadow_*` (spec range checks).
    - `drawImage` forms `(img,dx,dy)`, `(img,dx,dy,dw,dh)`, `(img,sx,sy,sw,sh,dx,dy,dw,dh)`. Source may be a `<canvas>`/OffscreenCanvas (carrying `__nid__`) or an `<img>` element. Canvas sources blit via `_lumen_canvas2d_draw_image[_crop]`. `<img>` sources go through `img_bitmap_store` (thread-local on the JS thread, populated by the shell after `fetch_and_decode_images` via `QuickJsRuntime::register_img_bitmaps`) and dispatched to `_lumen_canvas2d_draw_image[_crop]_from_img`. The 9-arg crop form passes 8 coords as a CSV string to stay within rquickjs's 7-closure-param limit. **Limitation:** inline `drawImage(img,…)` called before image decode (e.g. before `window.load`) may see an empty bitmap.
    - `putImageData(imageData, dx, dy)` hex-encodes `imageData.data` and calls `_lumen_canvas2d_put_image_data`.
  - `__patid__` is used for patterns (distinct from Path2D's `__pid__`) so fillStyle dispatch never confuses a pattern with a path.
  - +8 e2e tests in `dom.rs` (gradient object shape, radial/conic distinct ids, gradient fillStyle paints pixels, shadow props wired, createPattern id + null-for-invalid-source, drawImage canvas→canvas blit, putImageData paints).
  - **drawImage 9-arg source-crop** (2026-06-20): `Context2D::draw_image_cropped` samples only the source crop rectangle (per-pixel source coords clamped to bitmap bounds, no tiling); `draw_image` is now a thin full-source wrapper. JS shim 9-arg form routes to `_lumen_canvas2d_draw_image_crop`. +4 lumen-canvas unit tests (full blit, crop sub-rect, top-left quadrant isolation, zero-extent no-op) + 1 dom.rs e2e (`canvas_draw_image_9arg_crops_source_subrect`: cropped blue column drawn, red column excluded).

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
  - `navigator.getGamepads()` → snapshot array, **empty until a device connects** (BUG-392; W3C Gamepad L2 §5.1 forbids a pre-declared non-zero length). The internal list grows to `index + 1` in `_lumen_gamepad_connect` and never shrinks on disconnect. Phase 0 has no hardware polling, so in practice it stays `[]`.
  - `Gamepad` class: id/index/connected/timestamp/mapping/axes(4)/buttons(17)/vibrationActuator/hapticActuators.
  - `GamepadButton` class: pressed/touched/value.
  - `GamepadHapticActuator` stub: type, `playEffect(type, params) → Promise<'complete'>`, `reset() → Promise<'complete'>`.
  - `GamepadEvent` class: gamepad property.
  - Shell integration helpers: `_lumen_gamepad_connect(index, id, mapping)` fires `gamepadconnected`; `_lumen_gamepad_disconnect(index)` fires `gamepaddisconnected`. P3 shell calls these when polling OS gamepad APIs.
  - `window.Gamepad`, `window.GamepadButton`, `window.GamepadHapticActuator`, `window.GamepadEvent` exported as globals.
  - `window.ongamepadconnected` / `window.ongamepaddisconnected` event handler IDL attributes (BUG-392) — plain nullable properties, the same shape the main shim uses for `window.onpopstate`/`onhashchange`.
  - 17 unit tests. **1086 lumen-js lib tests total** (after combining with speech task's 1071 base).

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
- **`location.hash` setter + `hashchange` event** (`dom.rs`, P3-navapi Phase 1b). 2026-06-25. The `location.hash` accessor became a real HTML LS setter: assigning it changes only the fragment of the current URL and performs a **same-document** navigation — no page reload. The setter updates `location` (via a private `_lumen_loc_hash` backing var so internal `_lumen_location_update` writes do not re-enter the setter), pushes a same-document history entry (JS mirror `_lumen_history_push` + shell `_lumen_history_push_url`, so the address bar and back-stack advance without a fetch), and fires a `hashchange` event on `window.onhashchange` and `addEventListener('hashchange')` listeners (`_lumen_fire_hashchange`, using the existing `HashChangeEvent` class). Setting the hash to its current value is a no-op (no event, no entry). 8 unit tests (`location_hash_setter_*`). Remaining P3-navapi work: fragment-only routing through `location.href=`/link clicks, multi-step `history.go` unification, and the Navigation API wiring.
- **WebCodecs graceful degradation** (`web_codecs.rs`, U-4 stage 2). 2026-06-18. `VideoEncoder`/`VideoDecoder`/`AudioEncoder`/`AudioDecoder` `configure()` no longer throw synchronously (which white-screened SPAs not wrapping it in try/catch) — they transition to `configured` and report unsupported codecs through the spec async error callback. Added the missing `InvalidStateError` class. `isConfigSupported()` still resolves `false` for feature detection.
- **Storage Buckets API** (`storage_buckets.rs`, P3-storagebuckets). 2026-06-26. Pure ES5 JS shim (`init_storage_buckets`), installed after the DOM so `Promise`/`navigator`/`DOMException` exist. `navigator.storageBuckets` is a `StorageBucketManager` with `open(name, options?)` (validates the name as `^[a-z0-9][a-z0-9_-]*$`, length 1..=64; rejects with `TypeError`; dedupes by name), `keys()` (sorted), `delete(name)`. Each `StorageBucket` carries a read-only `name` and the async surface `persisted()`/`persist()`/`estimate()` (`{usage:0, quota}`)/`durability()`/`setExpires(ms)`/`expires()`/`getDirectory()` (delegates to `navigator.storage.getDirectory` OPFS when present, else rejects `InvalidStateError`), plus `indexedDB`/`caches` accessors returning the global instances. Phase 0: buckets live in memory for the JS-context lifetime; quota/persistence advisory. 8 unit tests (`storage_buckets::tests::*`).

- **Web Crypto SubtleCrypto — slice 2: AES-CBC/CTR + PBKDF2 + HKDF** (`crates/js/src/subtle_crypto.rs`, P3-webcrypto, P1 2026-07-08).
  - **AES-CBC** (`cbc` crate, PKCS7 padding): `generateKey` (128/256-bit), `importKey` (raw + JWK), `exportKey` (raw + JWK), `encrypt`, `decrypt`. JWK `alg` = `A128CBC`/`A256CBC`.
  - **AES-CTR** (`ctr` crate, Ctr128BE): `generateKey` (128/256-bit), `importKey` (raw + JWK), `exportKey` (raw + JWK), `encrypt`/`decrypt` (symmetric — same operation). Supports `length` bits 1–128 for the counter portion; `length=128` = full block counter increment.
  - **PBKDF2** (RFC 2898 §5.2, manual HMAC over existing `hmac`+`sha2` — no new dep): `importKey` (raw password, always non-extractable), `deriveBits`, `deriveKey`. Supports SHA-256/384/512 PRF, arbitrary iteration count and output length.
  - **HKDF** (RFC 5869 extract+expand, manual): `importKey` (raw IKM, non-extractable), `deriveBits`, `deriveKey`. Supports SHA-256/384/512, optional salt (defaults to zero block), info binding.
  - JS shim updated: `subtle.encrypt`/`decrypt` dispatch by `algorithm.name` (AES-GCM/AES-CBC/AES-CTR); `deriveBits` and `deriveKey` replaced stubs with real calls to `_lumen_subtle_derive_bits`.
  - 4 new native bindings: `_lumen_subtle_aes_cbc_encrypt`, `_lumen_subtle_aes_cbc_decrypt`, `_lumen_subtle_aes_ctr_crypt`, `_lumen_subtle_derive_bits`.
  - 19 new Rust unit tests + 3 new JS-level tests (AES-CBC round-trip, PBKDF2 `deriveBits`, HKDF→AES-GCM round-trip). RFC known-vector tests: PBKDF2-HMAC-SHA256 (RFC 6070 §2), HKDF-SHA256 (RFC 5869 A.1).
  - New deps: `aes = "0.8"` (explicit pin, already transitive via `aes-gcm`), `cbc = "0.1"`, `ctr = "0.9"`. All RustCrypto family, permanent.
  - **Previously shipped (slice 1):** SHA-*/`digest`, HMAC-SHA256/384/512 (sign/verify), ECDSA-P256 (sign/verify), AES-GCM (encrypt/decrypt). **Remaining:** RSA-OAEP, RSA-PSS, RSASSA-PKCS1-v1_5, ECDH.

- **Web Crypto SubtleCrypto — slice 3: RSA-OAEP / RSA-PSS / RSASSA-PKCS1-v1_5 / ECDH P-256** (`crates/js/src/subtle_crypto.rs`, P3-webcrypto, P1 2026-07-13). Task complete.
  - **RSA-OAEP** (`rsa::Oaep`): `generateKey` (default 2048-bit, configurable `modulusLength`), `importKey` (spki/pkcs8/jwk), `exportKey` (spki/pkcs8/jwk), `encrypt`, `decrypt`. SHA-256/384/512. Optional `label`.
  - **RSA-PSS** (`rsa::pss`): `generateKey`, `importKey`/`exportKey` (spki/pkcs8/jwk), `sign`, `verify`. `saltLength` from alg params. SHA-256/384/512.
  - **RSASSA-PKCS1-v1_5** (`rsa::pkcs1v15`): `generateKey`, `importKey`/`exportKey` (spki/pkcs8/jwk), `sign` (deterministic), `verify`. SHA-256/384/512.
  - **ECDH P-256** (`p256::ecdh::diffie_hellman`): `generateKey`, `importKey` (raw/spki/pkcs8/jwk), `exportKey` (raw/spki/pkcs8/jwk), `deriveBits`/`deriveKey` — returns 32-byte X coordinate of shared point.
  - JS shim: `encrypt`/`decrypt` dispatch adds RSA-OAEP via `_lumen_subtle_rsa_oaep_encrypt/decrypt`; `deriveBits` dispatch adds ECDH via `publicKeyId` in JSON (peer public key by registry id).
  - 2 new native bindings: `_lumen_subtle_rsa_oaep_encrypt`, `_lumen_subtle_rsa_oaep_decrypt`.
  - 11 new Rust unit tests; 40/40 total subtle_crypto tests pass.
  - New deps: `rsa = { version = "0.9", features = ["sha2"] }` (permanent, same crate as lumen-network), `rand_core = { version = "0.6", features = ["getrandom"] }` (companion to rsa, OsRng), `p256` updated: +`ecdh` feature.

- **DOM node-wrapper interning fix** (BUG-291, [P2] P2-wpt, 2026-07-17). `_lumen_make_element(nid)`
  (`crates/js/src/dom.rs`) minted a brand-new JS wrapper object on every call, so repeated access to
  the same underlying node (`.lastChild`/`.firstChild`/`.parentElement`/`.children`/etc.) returned a
  *different* object each time — broke `===` node identity (`tbody.lastChild === tr` was `false`) and
  silently dropped expando properties set between accesses. This is what made `testharness.js`'s
  built-in results renderer (`Output.show_results`) throw `TypeError: Cannot read properties of null
  (reading 'appendChild')` on `tbody.lastChild.lastChild.appendChild(...)`, aborting
  `notify_complete()` before `testharnessreport.js`'s own completion callback ran. Fix: new per-nid
  cache `_lumen_node_wrappers` (same pattern as the existing `_validity_msg`/`_canvas2d_ctxs` maps) —
  `_lumen_make_element` now returns the interned wrapper for a previously-wrapped `nid` instead of
  building a fresh one. Safe for the life of one JS context: the DOM arena
  (`crates/engine/dom/src/lib.rs`) allocates node ids append-only with no free-list reuse, and the
  whole shim is re-evaluated fresh on every navigation/bfcache thaw. Shared `WEB_API_SHIM`, fixes both
  engines. Regression test: `dom::tests::repeated_node_access_returns_identical_wrapper`. `tests/wpt/run_smoke.py`
  still doesn't reach a real PASS — a separate, unrelated blocker (a `script.evaluate`-install race, not a
  DOM/JS gap — see [BUG-296](../bugs/BUG-296-FIXED.md)'s "Остаток"; the stale-session mechanism BUG-296
  itself diagnosed is fixed).

- **Compression Streams — error signalling** (`crates/js/src/dom.rs`, WHATWG Compression Streams). 2026-07-18.
  - `CompressionStream`/`DecompressionStream` were already functional for the three spec formats
    (`deflate-raw`/`deflate`/`gzip`, buffer-then-flush over `TransformStream`, native `flate2`
    bindings `_lumen_compress_bytes`/`_lumen_decompress_bytes`). Gap: `_lumen_decompress_bytes`
    swallowed decode errors (`.ok()`) and returned an empty `Vec`, so a corrupt/truncated stream
    closed silently with no output instead of erroring the readable side (spec violation).
  - Fix: the two per-engine `_lumen_decompress_bytes` natives now both delegate to one shared
    `crate::dom::_decompress_status_prefixed(data, format)` that returns a **status-prefixed** byte
    array — `out[0]==1` → success (`out[1..]` are the inflated bytes, possibly empty for a valid
    stream that decompresses to nothing), `out[0]==0` → decode error (corrupt/truncated/unknown
    format). The shared `WEB_API_SHIM` `DecompressionStream.flush` calls `controller.error(TypeError)`
    on status `0`, so `reader.read()` rejects. A status prefix (rather than throwing across the FFI
    boundary, which the `reg!` macros don't uniformly support) keeps the fix engine-agnostic and lets
    a genuine empty result stay distinct from an error.
  - `brotli` is intentionally **not** implemented — it is not part of the WHATWG Compression Streams
    spec (only `deflate-raw`/`deflate`/`gzip`; `zstd` is a newer, not-yet-universal addition).
  - Tests (`dom.rs` mod tests): the pre-existing round-trip tests still pass through the stripped
    prefix; new `decompression_stream_corrupt_input_errors_stream` (bad gzip bytes → `reader.read()`
    rejects, does not resolve) and `decompression_stream_multi_chunk_matches_single_chunk`
    (split-write body decodes identically to a single chunk).

- **BUG-341 S7 (part 1): `v8_runtime::DomTouched`** — page-side DOM-mutation tracker, V8-only,
  mirroring `lumen_chrome::bind_model_tracked` (BUG-341 S6). `V8JsRuntime::take_dom_touched()`
  drains `{ nodes: HashSet<NodeId>, unattributed: bool }` since the last call. Instruments the 9
  native mutation primitives whose selector-relevant effect is precisely attributable —
  `_lumen_set_attr`/`_lumen_remove_attr` (only when the value actually changed),
  `_lumen_append_child`/`_lumen_remove_child`/`_lumen_insert_before` (record the parent),
  `_lumen_set_text_content`/`_lumen_set_inner_html` (record the node itself — a text/childList
  change can flip `:empty`), and the CSS Typed OM `_lumen_set_style_property`/
  `_lumen_delete_style_property` (bypass `_lumen_set_attr`, needed their own change-detection).
  `classList`/`className`/inline `style.color = …` in the JS shim all route through
  `_lumen_set_attr` already, so no shim changes were needed for those. The other 13
  `dom_dirty`-setting natives — Shadow DOM attach, Selection/Range get-set-clear,
  contenteditable key-handler bindings, `execCommand`'s mutating branches — set
  `unattributed: true` (their effect isn't attributable to a simple node set; the caller must
  fall back to a full cascade for the cycle). 12 unit tests (`v8_runtime.rs`,
  `take_dom_touched_*`), all green.
  **BUG-341 S7 (part 2): wired into the page pipeline.** `Lumen::try_relayout_raf_incremental`
  (`crates/shell/src/main.rs`) drains `take_dom_touched()` and, when attributed and a matching
  cascade cache (`Lumen::page_prev_cascade_styles`) exists, takes the restyle-aware
  `layout_mutation_incremental_restyle` path — same shape as chrome's S6 wiring — falling back to
  the plain graft-only `layout_mutation_incremental` otherwise (still correct, just without the
  cascade-skip win). New differential test
  (`v8_runtime::tests::dom_touched_drives_incremental_restyle_matching_full_cascade`) drives a real
  V8 `classList.add` mutation end-to-end and asserts the result matches a fresh full-cascade
  recompute. The engine-thread relayout job is not wired (see BUG-341 "S7 (part 2)" for why —
  crossing the thread boundary with a `CounterMap`/dirty-roots is a separate design question).

- **Focus API (HTML LS §6.6, BUG-381, 2026-07-29).** `HTMLElement.prototype.focus(options)`/`blur()`,
  `document.activeElement`/`hasFocus()`, `window.focus()`/`blur()`, `tabIndex`/`autofocus` IDL
  reflection, focusability per §6.6.1, and the `[autofocus]` flush at `readyState = 'interactive'`.
  Both directions (page-initiated `focus()` and shell-reported focus change) funnel through the single
  `_lumen_focus_update` entry point, which is idempotent — that is what lets the shell echo a focus the
  page just requested without dispatching the event sequence twice. Page-side state moves
  **synchronously** (`document.activeElement` must be current on the next statement) while the shell is
  told through the pre-existing `_lumen_request_focus`/`_lumen_request_blur` queue and applies it on its
  next pump.
- **`document.designMode` (HTML LS §6.6.3, BUG-353, 2026-08-09).** `'on'`/`'off'`, backed by a
  `design_mode: bool` field on `lumen_dom::Document` (`#[serde(default)]`, survives tab
  hibernation) rather than any per-element attribute. `find_editing_host` (`crates/engine/dom/src/lib.rs`
  — the single ancestor-walk both `_lumen_is_contenteditable` and the shell's contenteditable-key
  routing go through) falls back to `doc.body()` when the walk finds no explicit `contenteditable`
  and design mode is on; an explicit `contenteditable` closer to the node still wins, since the
  fallback only runs after the walk exhausts. Setter is spec-correct: a value that is neither `'on'`
  nor `'off'` (case-insensitively) is a no-op, not a silent reset to `'off'`.
- **IDL attribute reflection, form-control collections and activation (HTML LS §2.6.1/§4.10, BUG-383,
  2026-07-29).** Reflection is one declarative table `{idl, content-attr, kind, default}` plus one
  generic accessor pair per kind (`string`/`bool`/`long`/`ulong`/`url`/`enum`), installed by
  `_lumen_install_reflection` onto the **interface prototypes** — adding an attribute is a table row,
  and the properties cost nothing per element. `url`-kind getters resolve through
  `_lumen_document_base_url()` (first `<base href>` resolved against the page URL, else the page URL)
  over the pre-existing `_url_resolve`. Non-reflection half: `HTMLFormControlsCollection` /
  `HTMLOptionsCollection` over `_lumen_make_nid_collection` (extracted from
  `_lumen_make_html_collection`), the `form`/`labels`/`label.control` association graph,
  `select`/`option` APIs, the text-selection API, and `HTMLElement.prototype.click()` with the full
  activation sequence. `form.reset()` runs entirely in the document; `form.submit()`/`requestSubmit()`
  queue `NavigateRequest::SubmitForm`, which the shell answers with `Lumen::run_form_submission` — the
  same code path a real submit-button press takes.
- **`dispatchEvent` runs activation behavior for script-synthesised clicks (BUG-439, 2026-07-30).**
  `HTMLElement.prototype.click()` already ran `_lumen_run_activation_behavior` after its internal
  dispatch (BUG-383), but `el.dispatchEvent(new MouseEvent('click', ...))` did not — that call lands on
  the per-element `dispatchEvent` closure inside `_lumen_make_element` (`_lumen_dispatch(nid, evt)`,
  *not* `_lumen_dispatch_rich`, which only serves shell-originated mouse/key dispatch), and it had no
  activation step at all. Fixed by calling `_lumen_run_activation_behavior(nid, this)` there too, gated
  on `!evt.defaultPrevented && evt.isTrusted === false && evt.type === 'click'` — covers submit/reset
  buttons, `<a href>`/`<area>`, checkbox/radio, `<summary>`, `<label>` in one place since they all share
  that same function.
- **Live node wrapper matches WebIDL on `localName`/`prefix`, namespace-aware `tagName`, a hidden
  `__nid__` handle and `HTMLUnknownElement` ([BUG-367](../bugs/BUG-367-FIXED.md) points 1/2/3/5,
  [P3] 2026-08-10).** Four independent WebIDL gaps in `_lumen_build_element`, the one factory behind
  every live wrapper. Points 1-2 shared a root in the **native** layer: `_lumen_get_tag_name` returned
  `name.local.to_ascii_uppercase()` and was the only route from the arena to JS, so the un-folded local
  name simply never crossed the boundary — `localName` could not be derived in the shim
  (`tagName.toLowerCase()` would mangle `<linearGradient>`) and `tagName` had nothing to un-upper-case.
  The upper-cased form is still needed *inside* the shim (it keys `_lumen_html_tag_prototypes` and ~50
  `=== 'IMG'`-style tag comparisons), so the two roles were split rather than swapped: a new
  `_lumen_get_local_name(nid) -> Option<String>` native returns `name.local` verbatim (`None` for
  non-elements), `_lumen_get_tag_name` is untouched, and a shim helper `_lumen_qualified_tag_name`
  upper-cases only when `namespaceURI === 'http://www.w3.org/1999/xhtml'` and now feeds both `tagName`
  and `nodeName`. `localName`/`prefix` are getters on the wrapper (`prefix` always `null` — present and
  null, which is what `'prefix' in el` detects — Lumen parses no prefixes). `__nid__` is re-declared
  after the literal as non-enumerable/non-writable/non-configurable, which both un-fingerprints every
  node (`Object.keys(el)[0]` used to be `__nid__`) and closes a real mutation-redirect: the shim
  resolves tree mutations through `child.__nid__`, so `a.__nid__ = b.__nid__` from page script made
  `dest.appendChild(a)` move `b`. `_LUMEN_KNOWN_HTML_TAGS` splits known tags (→ `HTMLElement`) from
  unrecognized ones (→ `HTMLUnknownElement`, HTML LS §3.1.3), with hyphenated names (valid custom
  element names) staying `HTMLElement` per spec. Point 4 of the report — moving the wrapper's ~220 own
  members onto the prototypes — is a rewrite of the factory and is tracked separately as
  [BUG-747](../bugs/BUG-747-OPEN.md). 4 unit tests; `create_document_builds_xml_document` was corrected
  on the merits (it asserted `documentElement.tagName === 'SVG'`, encoding the very defect removed).

- **Named access on Window (HTML LS §7.3.3, [BUG-384](../bugs/BUG-384-FIXED.md), 2026-08-10).** An
  element with an `id` — and `img`/`form`/`iframe`/`embed`/`object` with a `name` — is reachable as
  `window.x`, as the bare identifier `x`, and answers `true` to `'x' in window`. The property used to
  not exist at all, so the `<div id=app>` + bare `app` idiom threw `ReferenceError` on the first line
  and took the whole script with it. Implemented as a V8 named-property interceptor on the **global
  object template** (`v8_runtime.rs::window_named_properties_template`, wired through
  `v8::ContextOptions { global_template }` in `v8_thread_main`) — deliberately not a line in
  `WEB_API_SHIM`, because the shim cannot see a name that fails to resolve. `PropertyHandlerFlags::
  NON_MASKING` is what buys the spec's resolution order for free: V8 consults the interceptor **only**
  for names that resolve nowhere else, so own `Window` properties and page `var`/`function`
  declarations keep winning with no bookkeeping on our side. Non-obvious details for anyone touching
  this: the interceptor is installed at context creation, long before a document exists, and the
  document is published into a **thread-local** by every `install_dom` (so it follows navigation and
  is simply inert in worker isolates); the lookup takes the document with `try_lock`, never `lock`,
  because the interceptor fires on *any* global-name miss including one made by JS that a native
  called while holding the document lock (a blocking take would deadlock the JS thread against
  itself); and the returned value is built by calling the shim's own `_lumen_make_element`, so
  `window.probe === document.getElementById('probe')` holds by identity. Measured cost (A/B on one
  build): reads of an *existing* global are unchanged — the interceptor does not deoptimize global
  variable access — while an unresolved name costs ~0.75 µs vs ~0.02 µs on an 8-node document, an
  O(n) tree walk per miss. Spec simplifications: several matches yield the first in tree order rather
  than an `HTMLCollection`, a matching `iframe` yields the element rather than its `contentWindow`.
- **`window.isSecureContext` is computed once, from the URL the runtime was installed with
  (BUG-399, 2026-08-11).** `_lumen_url_is_potentially_trustworthy` implements Secure Contexts
  §3.1/§3.2 next to `_lumen_loc_parts`. Two non-obvious constraints shape it. **(1) Read the scheme
  off `href`, never off `parts.protocol`.** `_lumen_parse_url` splits on the first `://`, so it
  reports `blob:https://h/id` as protocol `blob:https:` and a `data:` URL whose payload contains
  `://` as something arbitrary — any scheme test written against `protocol` is wrong for exactly
  the two schemes (`blob:`, `data:`) the spec singles out. The URL Standard's scheme is everything
  before the first colon, and that is what the predicate uses. **(2) Snapshot the value, don't
  re-read it per access.** The flag belongs to the environment settings object and is fixed when
  the document is created (HTML LS §8.1.5.1) — which is exactly the granularity of `install_dom`.
  A live read is also actively wrong: same-document `history.pushState(s, '', '/x')` stores that
  raw *relative* string in `_lumen_loc_parts` (see `_lumen_location_update`), which would flip an
  https page to insecure. The snapshot lives in a closure rather than a `_lumen_…` global, since
  `seal_internal_globals_v8` leaves engine state writable. Nothing in the engine reads the flag yet
  — `[SecureContext]` gating is [BUG-765](../bugs/BUG-765-OPEN.md), and `WorkerGlobalScope` has no
  such property at all ([BUG-766](../bugs/BUG-766-OPEN.md)).

- **`HTMLElement.innerText`/`outerText` — setters (BUG-413 slice 1, [P3] 2026-08-21).**
  Neither property existed at all — not on the wrapper, not on a prototype — so `el.innerText = s`
  quietly minted an expando and the caller died on the *next* statement (WPT's own helper reads
  `offsetWidth`/`firstChild` off the result, which is why 169 subtests of
  `html/dom/elements/the-innertext-and-outertext-properties/` failed on a line that never mentions
  `innerText`). Both setters live in the live-wrapper factory (`_lumen_build_element`, JS shim)
  next to `textContent`, and share one helper, `_lumen_rendered_text_nids` — HTML LS §3.2.7's
  «rendered text fragment» literally: a run of non-break code points becomes a Text node, each
  line break becomes a `<br>`, a `\r\n` pair counts as **one** break while `\n\n`/`\r\r` count as
  two, and a leading or trailing break yields a lone `<br>`. Three details worth keeping in mind
  when the getter (slice 2) lands on top:
  - **The white-space caveat in the bug report is not in the spec.** The rendered-text-fragment
    algorithm is unconditional; there is no `white-space: pre*` branch that would emit Text nodes
    instead of `<br>`s. Only the *getter* depends on layout.
  - **`outerText`'s merge is deliberately narrower than `normalize()`.** The spec merges exactly
    the two Text nodes that used to touch the replaced element (steps 7–8), so
    `A|B|<span>|D|E` becomes `A|BReplacedD|E`, not one node — `_lumen_merge_with_next_text` folds
    a single next sibling and nothing else. Assigning `''` still inserts one empty Text node
    (step 5), which is what makes the two neighbours end up merged with the element gone.
  - **Absence outside the HTML namespace is emulated, not inherited.** Both are `HTMLElement`
    members, but the shim builds one wrapper shape for every element, so the accessor is present
    on an SVG/MathML wrapper too; `_lumen_assign_as_expando` redefines it as a plain data property
    on assignment, which is what a write to a missing property does. The namespace check reads
    `namespaceURI`, so it is correct for `createElementNS` and wrong for markup-parsed foreign
    content — the parser has no foreign-content mode ([BUG-685](../bugs/BUG-685-OPEN.md)).
- **`HTMLElement.innerText`/`outerText` — getters (BUG-413 slice 2, [P3] 2026-08-22).**
  Both properties now read `_lumen_rendered_text` — the spec gives `outerText` no getter of its
  own, only the same steps — implementing HTML LS §3.2.7's collection steps and the getter's own
  post-processing. The part worth remembering is **what the getter is allowed to believe about the
  layout bridge**, because the plan the bug filed for this slice was wrong and cost one attempt:
  - **An inline element appears in NEITHER snapshot.** It owns no `LayoutBox` — its content is
    flattened into the enclosing block's `InlineRun` segments — so `_lumen_get_bounding_rect` and
    `_lumen_get_computed_style` both answer nothing for `<b>`/`<span>`, exactly as they do for
    `display: none`. Taking «is it being rendered» from the rect the way `offsetWidth` does
    therefore drops every inline subtree: `<p>hello <b>world</b></p>` read back as `"hello"`.
    Measured on a real `InProcessSession` layout, guarded by
    `crates/driver/tests/cases/inner_text_getter.rs::inline_element_has_no_snapshot_entry_but_its_text_node_does`.
  - **The style that governs inline text lives on the text node.** `collect_computed_styles` was
    extended to publish an entry per `InlineSegment::source_node` carrying
    `lumen_layout::INLINE_SEGMENT_PROPERTIES` (`visibility`/`white-space`/`text-transform`), so a
    `text-transform` or `visibility` set on a `<span>` is visible even though the `<span>` is not.
    Every style lookup in the getter is made against the text node, never against its parent.
  - **Presence of an entry is the rendered test.** `display: none` yields neither box nor segment,
    so an absent entry means «not laid out» — and an entry-less *element* is treated as a
    transparent inline wrapper (recurse through, contribute nothing), which is also what makes a
    `display: none` block contribute no line break. `<br>` is the one exception handled by tag
    name before that rule, since it owns no box under any style.
  What it does not do: soft wraps contribute no line feed (the bridge carries no per-line text),
  `<select>`/`<optgroup>`/`<option>` get none of step 3's synthetic boxes, and a hidden `<br>`
  still emits its feed. A node with no entry at all answers `textContent`, per step 1 —
  detached elements, and anything read from a parser-time script
  ([BUG-443](../bugs/BUG-443-OPEN.md)).

## Deferred

- WebGL: GLSL execution (per-vertex colour / texture sampling — currently flat `uniform4f` fill), `drawElements` / indexed draws, real textures. Backend stub lives in `lumen_paint::webgl`.
- PerformanceObserver API.
- `rusty_v8` backend porting (S12 remains; S0/S1/S2/S3/S4/S5-S7/S8/S9/S10/S11 done
  2026-07-13/14 — v8 v150.1.0 optional dep под `v8-backend`;
  `V8JsRuntime`/`V8Inner`/`v8_thread_main` + `JsRuntime` trait impl + `v8_compat`:
  `into_v8_fnN` (arity 0..7) + `V8NativeFn` + `OwnedNativeFn` + trampoline +
  `register_v8_native` (+ `Vec<String>`/`Vec<u32>`/`Vec<f64>`/`Vec<u8>`/`u8` FromJsValue/IntoJsReturn
  added in S3); `V8JsRuntime::install_dom` ports `dom::install_primitives` — 183/184 natives
  byte-identical closure bodies, `WEB_API_SHIM` reused unchanged (`pub(crate)`), deterministic-seed
  eval kept verbatim; `_lumen_drain_microtasks` is a no-op stub (V8 auto-runs its microtask queue,
  unlike QuickJS's manual-drain model). Shell wiring (`V8PersistentJs`, `crates/shell` `v8`
  feature) done in S4 — see `subsystems/shell.md`. S5-S7 batch 1 (p1-v8-s57): 68 of 90
  simple `install_*` modules ported (each a `install_X_v8(rt: &V8JsRuntime)` sibling next
  to the rquickjs original, wired best-effort — logs and continues on error, mirroring
  `lib.rs`'s orchestration — via an `install_v8!` macro at the end of `install_dom`); added a
  `DOM_EXCEPTION_POLYFILL` (V8 has no web-platform globals at all — quickjs-ng bundles
  `DOMException` as a built-in, so this gap was silently latent since S3). S5-S7 batch 2
  (p1-v8-s57, 2026-07-13): 11 more modules with 1-5 `Function::new` natives ported
  (download_bindings, filesystem_access, idle_detection, network_log_bindings, speech,
  web_audio, file_input, pip_bindings, wake_lock, media_capture, screen_capture) via
  `into_v8_fnN` + new `V8JsRuntime::register_native` (registers an already-wrapped native
  as a global without duplicating `install_dom`'s inline scope/store setup) — 79/90 ported
  overall. S5-S7 batch 3 (p1-v8-s57-batch3, 2026-07-13, **closes S5-S7**, 90/90 ported):
  video_bindings + audio_element (13-16 `Function::new` natives each, process-global
  provider state — no new `V8JsRuntime` plumbing needed); geolocation (no natives at all,
  `fake_coords` only baked into an injected JS global); broadcast_channel +
  notifications_bindings (needed new `V8JsRuntime` plumbing — `broadcast_channels`/
  `pending_notifications` fields mirroring `QuickJsRuntime`'s fields of the same name,
  plus `broadcast_registry()`/`notification_queue()` accessors and
  `pump_broadcast_channels()`/`take_notification_requests()` public methods mirroring the
  QuickJS API). All tests green (v8_runtime + module tests). S8 (p1-v8-s8, 2026-07-14,
  hand-port, not part of the S5-S7 simple-module batch): `install_canvas2d_bindings_v8`
  (77 natives) + `install_webgl_canvas_v8` (34 natives), both pattern (b) — module-level
  `thread_local!` state (`CANVASES`/`DIRTY`/`GRADIENTS`/`PATTERNS`/`PATHS`/`TRANSFERRED` in
  `canvas2d.rs`, `CONTEXTS`/`NEXT_ID` in `webgl_canvas.rs`), no new `V8JsRuntime` fields.
  Max arity 6 (`_lumen_webgl_uniform4f`); every arg/return type already covered by
  `v8_compat.rs` — no `v8::Global<Function>` GC-root mechanism needed (that's S9's
  concern, per `wasm::clear_registry` teardown). `canvas2d`'s `getContext('2d')` shim is
  already in `WEB_API_SHIM` (shared, unchanged); `webgl_canvas`'s `WEBGL_SHIM` is private
  and is `rt.eval`'d explicitly (mirrors `geolocation_v8`'s global-seeding pattern).
  `V8JsRuntime::flush_canvas_updates()` added (dispatches `canvas2d::flush_dirty()` on the
  JS thread); `V8PersistentJs::flush_canvas_updates` (shell) now delegates to it instead of
  the S4 no-op stub. `offscreen_canvas.rs` intentionally NOT ported in S8 (not in the
  ROADMAP task's scope; `transferControlToOffscreen()` still returns a valid id but the
  resulting `OffscreenCanvas.getContext('2d')` won't work under v8 until a future slice
  ports it). Verified via `--dump-display-list` on `graphic_tests/57-canvas-2d.html`:
  byte-identical output between v8 and quickjs builds. S9 (p1-v8-s9, 2026-07-14): wasm +
  webgpu. `webgpu.rs` had zero `Persistent` usage (confirmed S8's prediction) — ports
  unchanged through `into_v8_fnN`; without the `webgpu` Cargo feature it's just
  `rt.eval(WEBGPU_SHIM)` (0 natives), mirroring `webgl_canvas`'s shim-eval pattern.
  `webassembly.rs` needed the GC-root mechanism the generic compat layer can't express —
  `JsValue` has no function variant, so a JS `Function` argument collapses to `Null`
  (`v8_to_jsvalue`). Added `v8_compat::V8NativeFnScoped`: a second, object-safe native
  trait giving raw `(scope, FunctionCallbackArguments, ReturnValue)` access instead of the
  `JsValue` abstraction, with its own trampoline (`native_fn_trampoline_scoped`) and store
  (`V8Inner::native_fn_store_scoped`, twin of `native_fn_store`); `V8JsRuntime::register_native_scoped`
  mirrors `register_native`. 5 wasm natives use it: `__lumen_wasm_compile` (throws
  `CompileError` — `IntoJsReturn` has no error variant), `__lumen_wasm_instantiate`
  (captures the JS import-function array as `Vec<v8::Global<v8::Function>>`),
  `__lumen_wasm_call` (may re-enter a host import mid-call, needs a live scope to invoke
  the stored `Global`), `__lumen_wasm_global_get`/`_set` (need exact `i64` `BigInt`, which
  `f64`-only `FromJsValue`/`IntoJsReturn` would truncate past 2^53). New submodule
  `wasm::v8_bridge` (`#[cfg(feature = "v8-backend")]`) is a **separate** thread-local
  instance registry from the QuickJS one (module ids shared via the existing
  `with_module`; instances not, so the two backends never collide even if both features
  are compiled in); its `JsHost` resurrects a `v8::Local<Function>` from the stored
  `Global` via `v8::Local::new(scope, &global)` and invokes it with `Function::call`.
  `wasm::v8_bridge::clear_registry()` wired into `v8_thread_main`'s teardown (mirrors
  `lib.rs:447`'s `wasm::clear_registry()` for QuickJS; V8's `Global::drop` actually
  no-ops safely on an already-disposed isolate, so this isn't needed to avoid an abort
  like QuickJS's `gc_obj_list` assertion, but is still the correct leak-free order).
  Verification: `cargo test -p lumen-js --features v8-backend[,webgpu]` — 2402 lib tests
  (2399 + 3 new `tests_v8`) + 68 integration, all green; the 2 new `webassembly::tests_v8`
  tests are load-bearing — one exported-call round-trip and one host-import `i64`/`BigInt`
  round-trip (reusing `tests::IMPORT64_WASM`'s bytes) proving the `Global<Function>`
  actually resurrects and invokes at runtime, not merely compiles (no display-list
  equivalent exists for wasm to verify against, unlike S8's canvas diff).
  `offscreen_canvas`/`worker`/`shared_worker`/`sw_worker` remain unported (S10). S10
  (p1-v8-s10, 2026-07-14): worker + shared_worker + sw_worker. Each thread constructs a
  full `V8JsRuntime::new()` instead of a bare isolate (reuses the S1-S9 machinery
  wholesale rather than hand-rolling a second bare-isolate construct) — one extra OS
  thread per worker vs QuickJS, accepted for the risk reduction. All natives are plain
  `String`/`u32`/`bool`/`Option<String>` except `worker.rs`'s `atob`/`btoa`, which throw
  on invalid input and go through `V8NativeFnScoped` (same mechanism as S9's
  `wasm_compile_native_v8`); `shared_worker`/`sw_worker`'s equivalents don't throw and use
  the plain `into_v8_fnN` path. `WorkerHandle`/`WorkerRegistry`/`SharedWorkerThread`/
  `SwWorkerHandle` etc. are engine-agnostic and reused unchanged by both backends;
  `WORKER_SHIM`/`SHARED_WORKER_SHIM`/worker-global-scope JS extracted into shared
  `worker_global_shim`/`sw_globals_shim` functions so both engines eval identical JS.
  `shared_worker.rs` gets a separate `HUB_V8` identity-keyed registry (mirrors S9's
  `wasm::v8_bridge` cross-backend-collision rationale). `sw_worker.rs` needs no
  `flush_jobs` equivalent — V8's microtask queue auto-runs, verified by
  `tests_v8::v8_sw_responds_from_cache` resolving a `respondWith(caches.match(...))`
  chain immediately after firing the fetch event with no manual pump.
  `_lumen_sw_activate_script` (wired since S3, previously always spawning a
  QuickJS-backed SW thread regardless of the calling page's engine) now calls
  `spawn_sw_worker_v8`. `V8PersistentJs::pump_workers`/`pump_shared_workers` (shell,
  previously no-op stubs explicitly waiting on this slice) now delegate to
  `V8JsRuntime::pump_workers`/`pump_shared_workers`. `offscreen_canvas.rs` still has no
  V8 port (same known gap as S8) — a V8-backed dedicated worker referencing
  `OffscreenCanvas` sees `undefined`, degrading gracefully via the existing
  `_deserializeTransfers` typeof-guard. Verification: `cargo test -p lumen-js --features
  v8-backend` — 2413 lib tests (2402 + 11 new `tests_v8`: 4 worker + 3 shared_worker + 3
  sw_worker), all green; default (QuickJS) suite unaffected (2372 tests) by the shim
  extraction refactors; clippy clean on both. S11 (p1-v8-s11, 2026-07-14):
  `V8JsRuntime::suspend`/`resume` — unlike `QuickJsRuntime` (`capture_raw_heap` still a
  stub, blocked on native-function bindings `JS_ReadObject` can't reconstruct),
  V8's port implements a real, if partial, round-trip via `v8::ValueSerializer`/
  `ValueDeserializer`. `V8Inner` gained `baseline_globals: HashSet<String>` — the
  global object's own-enumerable keys snapshotted once in `v8_thread_main` right
  after `Context::new`, before `install_dom`/any script runs. `suspend()` diffs the
  live global against that baseline (so DOM natives and ECMAScript built-ins are
  never candidates), structured-clone-probes each new key's value in isolation
  (a throwing probe — `DataCloneError`, F1 closures — is dropped without voiding
  sibling keys), then serializes the surviving keys as one wrapper object into
  `SuspendedHeap.compressed` via the existing `heap_snapshot` zlib framing.
  `resume()` deserializes the wrapper and copies its own-properties onto a fresh
  bare runtime's global object. Closures remain unrestorable (F1) — the
  re-run-scripts fallback (`main.rs:14599`) stays the closure-recovery path; this
  slice only closes the *data*-globals half of task 10C.2. Verification: `cargo
  test -p lumen-js --features v8-backend` — 2419 lib tests (2413 + 6 new
  `v8_runtime::tests::suspend_*`/`resume_*`, covering number/string/array/object
  round-trip and closure-drop-without-poisoning-siblings), all green; clippy clean
  on v8-backend and the default (QuickJS-only) build. Ported/pending checklist in
  `docs/tasks/ph3-v8-migration.md`. S12a (p1-v8-s12, 2026-07-14, ADR-018): `lumen-shell`'s
  `default` feature flipped from `quickjs` to `v8` — this crate itself is unchanged (the
  V8 port was already complete through S11); the flip lives entirely in
  `crates/shell/Cargo.toml` + ~80 broadened `#[cfg(any(feature = "quickjs", feature =
  "v8"))]` gates in shell code for generic (non engine-specific) JS-enabled plumbing.
  Full `rquickjs` removal from this crate (`QuickJsRuntime`, ~380 dual `install_*`
  bindings, `__lum_args__`) is S12b, not yet started — `rquickjs` is still a real
  dependency and `QuickJsRuntime` still the fallback engine behind `--features quickjs`.
  P1-imagebitmap (2026-07-17): `offscreen_canvas.rs` V8-ported (`install_offscreen_canvas_bindings_v8`,
  19 natives, same `into_v8_fnN`/`rt.register_native`/`rt.eval(OFFSCREEN_CANVAS_SHIM)` pattern as
  `webgl_canvas_v8`) — closes the S8/S10 gap noted above; `OffscreenCanvas`/`createImageBitmap`
  now exist under the default (V8) engine, not just QuickJS. Same commit implements
  `createImageBitmap` + `ImageBitmapRenderingContext` (`getContext('bitmaprenderer')`,
  HTML LS §4.12.5) end-to-end for both engines: bitmap shape unified to
  `{width, height, __canvas_id__, close()}` across all sources (ImageData, OffscreenCanvas,
  `<img>` via `img_bitmap_store`, `Blob` via new `lumen-js → lumen-image` dependency +
  `lumen_image::decode`), `sx/sy/sw/sh` crop as a JS-side post-process over the existing
  `get_image_data`/`from_image_data` natives, and `transferFromImageBitmap` presenting into a
  page `<canvas>` via `canvas2d::present_rgba` (the pre-existing WebGPU-present helper) through
  a new shared native `_lumen_bitmaprenderer_transfer_from_image_bitmap` in `canvas2d.rs`.
  `createImageBitmap(OffscreenCanvas)` no longer reuses the destructive `transferToImageBitmap`
  native (that neutered the source canvas as a side effect); it now snapshots via
  `get_image_data`/`from_image_data` instead, matching spec (only `OffscreenCanvas.transferToImageBitmap()`
  itself, called directly, still neuters).

## Invariants

- `QuickJsRuntime: Send + Sync` (enforced by `unsafe impl` + `Mutex`).
- `call_function` pollutes the global namespace with `__lum_args__` only transiently — cleaned up with `delete` after each call.
- `from_rq` maps `Type::Undefined` to `JsValue::Null` (not `Undefined`) — matches the trait docs which say "simple JSON-compatible types".
- rquickjs 0.11 `Function::call` takes `IntoArgs` (fixed-size tuples). Dynamic calls must use the eval workaround until rquickjs adds `Function::apply` or `Rest<T>: IntoArgs`.
- DOM shim: `parentElement` and `children` are defined with `enumerable: false` via `Object.defineProperty`. Prevents `from_rq`'s `obj.props()` loop from serializing these cyclic getters → infinite recursion / stack overflow.
- DOM shim: `Option<T>` in rquickjs maps `None → undefined` (not `null`). All nullable-returning native functions are wrapped with `_lumen_u2n(v)` in the shim to convert `undefined → null` as Web API requires.
- **DOM shim: the same `Option<T>` maps to `null` on the V8 bindings, not `undefined` (BUG-442).** `_lumen_u2n`-wrapped reads are therefore engine-agnostic, but the shim's other idiom — testing attribute *presence* with a bare `_lumen_get_attr(...) !== undefined` — is true for every name on the default engine. Use `_lumen_has_attr(nid, name)` (`dom.rs`, added with BUG-381) for presence; never compare a native's result with `undefined` directly.
- **DOM shim: a shim-level `function` name shadows the same-named Rust native for the whole page.** Both
  live as ordinary properties of the global object, and the shim's own top-level `function` declarations
  are installed after the natives, so a helper called `_lumen_set_selection` silently replaced the native
  that drives `window.getSelection()` (caught by `selection_collapse_to_start` while adding BUG-383; the
  helper is now `_lumen_set_text_selection`). Grep the native registrations before naming a new
  `_lumen_*` helper.
- **DOM shim: reflected IDL attributes live on the interface prototypes, current `value`/`checked` do
  not.** `_lumen_build_element` still owns `value` and `checked` because they are *state* seeded by a
  content attribute (HTML LS §4.10.5.5 dirty-value flag), and an own property shadows the prototype —
  so adding a `value`/`checked` row to the reflection table would be dead code. Everything else belongs
  in the table (`_LUMEN_*` entries near `_lumen_install_reflection`), never as a new own property.
- **DOM shim: `Object.defineProperty` that *redefines* an existing property inherits every attribute
  the new descriptor omits — the `false` defaults only apply to brand-new properties (BUG-367).** The
  shim's usual lock-down idiom, copied from `_lumen_make_doctype` (`{ value: nid, enumerable: false }`),
  is written against a freshly `Object.create`d object where the omitted `writable`/`configurable`
  really do default to `false`. Applied to a property that already exists — e.g. `__nid__`, seeded by
  `_lumen_build_element`'s object literal as writable+configurable — the same call flips only
  `enumerable` and silently leaves the property writable. Spell out all four attributes whenever the
  target property may already exist.
- **DOM shim: `_lumen_get_tag_name` is the *internal* upper-cased tag key, `_lumen_get_local_name` is
  the web-visible name (BUG-367).** The former keys `_lumen_html_tag_prototypes` and every
  `=== 'IMG'`-style comparison in the shim, so it must stay unconditionally upper-cased; anything
  surfacing a name to page script goes through `_lumen_qualified_tag_name` (`tagName`/`nodeName`) or
  the `localName` getter, which upper-case only inside the HTML namespace. Never expose
  `_lumen_get_tag_name`'s result to the page directly.
- **DOM shim: `_lumen_dispatch_rich` runs the document-level listeners even for a non-bubbling event.** It stops the ancestor walk on `!event.bubbles` but not the document hop afterwards, so a non-bubbling event dispatched through it still reaches `document.addEventListener(type, …)`. The focus family (BUG-381) needed its own dispatcher for exactly this reason; anything else adding a non-bubbling shell-driven event must not reuse `_lumen_dispatch_rich` as-is.
- **DOM shim: the shell tracks focus by layout box, whose node may be a text node.** Anything exposing `focused_node` to the page must normalise through `_lumen_nearest_element_nid` first — the spec-level focus surface only ever names elements.
- **`internal_globals::seal_internal_globals_v8` must stay the LAST step of `install_dom` (BUG-378).** It hides every internal global (`/^__|^_+lumen/i`) from enumeration and freezes the function-valued ones, so anything registering a native or patching one *after* it either stays visible (a late `register_native` lands as a plain enumerable property again) or fails silently (a late `_lumen_x = wrapper` write hits a read-only slot in sloppy-mode shim code). Only *functions* are frozen: engine state (`_lumen_timers`, `_lumen_loc_parts`, `_lumen_timer_nesting`, `_lumen_last_focused_nid`, `_lumen_document_implementation`) is written by the shim long after the pass and is therefore left writable, hidden only.
- **`WEB_API_SHIM` is evaluated through *indirect eval*, not as a Script, and must not contain a top-level `let`/`const`/`class` (BUG-378).** A Script's `GlobalDeclarationInstantiation` passes `D = false` (ECMA-262 §16.1.7), so the shim's ~250 top-level `var`/`function _lumen_…` would be **non-configurable** global properties — and `enumerable: true → false` is exactly the transition `Object.defineProperty` forbids on a non-configurable property, which is why sealing them was impossible before the switch (247 names stayed visible to `for (k in window)`). Indirect eval's `EvalDeclarationInstantiation` passes `D = true` (§19.2.1.3). The catch: lexical declarations under eval go into a declarative environment that dies with the eval call instead of onto the global object, so a top-level `const` added to the shim would vanish silently — declare shim top-level bindings with `var`. Guarded by `internal_globals::tests::shim_has_no_top_level_lexical_declarations`. Module shims (`rt.eval(X_SHIM)`) still run as Scripts; they are IIFEs, so they declare almost nothing on the global, and what they do declare the pass can only make read-only, not hidden.
- **`eval()` transparently runs scripts ≥512 bytes through a process-wide V8 bytecode cache (PERF-9, `CODE_CACHE`/`compile_cached!` in `v8_runtime.rs`).** The ~94 `install_v8!` module shims (`crate::*::install_*_v8`, each `rt.eval(X_SHIM)`) are `&'static str` constants re-evaluated on every navigation, byte-identical every time — a hit skips reparsing them, execution is unchanged either way. `WEB_API_SHIM` is deliberately excluded: it runs as a string argument to indirect `eval()`, not a `v8::Script` (see the bullet below), so there is no `UnboundScript` to cache. Routing it through the same mechanism would mean compiling it as a plain `Script`, which reintroduces exactly the non-configurable-binding problem the indirect-eval switch (BUG-378) fixed — do not "optimize" `WEB_API_SHIM` this way without first landing BUG-753 (rewrite its top-level bindings as explicit `Object.defineProperty` calls instead of bare `var`/`function`).
- **`surface_api.rs` must never *define* an automation-marker name in order to make it read as `undefined` (BUG-379).** Reading as `undefined` and being absent are different observable states, and automation detectors query the difference (`Object.getOwnPropertyNames(window)`, `'x' in window`, `hasOwnProperty`) — so the old "belt-and-braces" loop that created all 15 markers (`__playwright`, `__webdriver_evaluate`, `__selenium_*`, `_phantom`, `domAutomationController`, …) as non-configurable `undefined`-returning getters made the anti-fingerprint layer a 15-marker fingerprint of its own, unremovable afterwards. Reserving a name also cannot block a foreign `eval` injection: the injected property shows up in `getOwnPropertyNames` either way. Intercepting *writes* needs a `globalThis` proxy, which is a different mechanism. Two consequences for tests: assert page-observable state (`in` / `hasOwnProperty` / `getOwnPropertyNames`), never `typeof x === 'undefined'` (the one form a marker-shaped getter satisfies) and never a source-text scan; and do not let a unit-test harness shadow `globalThis` with a plain `{}` — the pre-fix harness did, so every marker assertion in `surface_api.rs::tests` was measuring a throwaway object instead of the page's global. Related open model question: `navigator.webdriver` is absent here where Chrome and Firefox both expose it as `false` (BUG-754).
- **`navigator.globalPrivacyControl` is derived from the network layer, never set independently (BUG-397, ADR-026).** The property and the `Sec-GPC: 1` request header are two halves of one signal that a page can read separately, so a disagreement between them is itself a fingerprinting bit — exactly what the privacy signal is meant to avoid. Both are wired from `lumen_network::sends_global_privacy_control(HttpProfile)`: the shell pushes it here via `set_global_privacy_control` at startup, and `driver::session::sync_global_privacy_control` re-pushes it whenever `BrowserSession::set_fingerprint_profile` changes the profile at runtime (the HTTP client is rebuilt per request, so the header switches immediately and the JS side must follow). Never add a second way to turn the property on. When the signal is off the property must be **absent**, not `false` — same absent-vs-`undefined` reasoning as the BUG-379 bullet above: the profiles that do not send the header impersonate browsers with no native GPC, and `'globalPrivacyControl' in navigator` is what a fingerprinting script asks.
- `install_dom` must be called before `eval`. Drop the runtime before `Arc::try_unwrap(doc_arc)` — closures hold Arc clones until the runtime is dropped.
- Web Storage closures capture `Arc<Mutex<WebStorage>>` clones — dropped with the runtime. The outer `Arc` in the shell's `ls_storage` map remains the authoritative store; JS mutations are immediately visible in Rust after the closure releases the lock.
- IndexedDB requests defer their data operation to `_idb_dispatch_request` (run once via `req._action`), not to the calling site. Reading `request.result` before the `success` event is therefore always `undefined`; tests and the shell must call `_lumen_idb_flush()` to drain pending events. Synchronous validation (invalid key range → `DataError`, read-only transaction → `ReadOnlyError`) still throws at the call site, before the request is queued.
- **`Runtime::execute_pending_job()` must not be called inside a `ctx.with(...)` closure.** `ctx.with` holds a context borrow; calling a `Runtime` method re-enters it and panics in `rquickjs-core safe_ref.rs`. In `sw_worker.rs` the rule is: fire the JS event inside `ctx.with`, then drain microtasks via `flush_jobs(&rt)` *outside* it, then read results in a second `ctx.with`. Globals persist across separate `ctx.with` calls on the same context, so multi-step promise resolution (e.g. `respondWith(...).then(r => r.text()).then(t => global = t)`) works by draining between the fire and the read.
- Service Worker execution threads (`sw_worker::spawn_sw_worker`, PH3-20) have an isolated `ServiceWorkerGlobalScope`: their `caches`/`atob`/`btoa` are native-backed (real base64), and their `fetch` is cache-first only (no network in Phase 1). A SW serves only responses already present in the shared `CacheBackend` the page cached — `cache.addAll()` cannot pull from network inside the SW.
- **A native that signals "error"/"overflow" via a `u32::MAX` sentinel and relies on the shim's `nid < 0` (or similar signed) check is a V8 trap (BUG-457).** `IntoJsReturn for u32` (`v8_compat.rs`) widens via `self as f64`, so `u32::MAX` becomes the *positive* `4294967295.0` on V8 — the check silently never fires. This "worked" on rquickjs only by accident: its FFI truncates `u32` through a signed 32-bit intermediate, turning `u32::MAX` into `-1`. Any native meant to return a negative sentinel must be declared `-> i32` (or `i64`) and return the literal negative value, never `u32::MAX`/`as u32`.
- **`v8_runtime.rs::from_v8` is cycle/depth-guarded (BUG-633, 2026-08-05).** It walks `is_array()`/`is_object()` branches while tracking ancestor `get_identity_hash()`es on the current path — a repeated hash returns `JsValue::String("[Circular]")` instead of recursing, and an absolute cap (`FROM_V8_MAX_DEPTH = 64`) truncates non-cyclic-but-pathologically-deep values the same way. Any JS value converted through this boundary is attacker/page-controlled (`eval`/`get_global`/`call_function` completion values), so an unbounded recursive walk over it is a crash primitive — a self-referencing object (e.g. `testharness.js`'s `test.eventExpectations_.test_ === test`) drove the old unguarded walk deep enough that a GC triggered mid-recursion failed V8's `isolate_->IsOnCentralStack()` invariant and killed the whole process with `V8_Fatal`, uncatchable by `TryCatch`. Any new recursive V8-value walk added to this file needs the same guard, not just `from_v8`.
- **`el.value` on `<input>`/`<textarea>` is document-backed, not a JS shadow (BUG-441, 2026-08-04).** The shim's accessor pair goes through the `_lumen_get_dirty_value` / `_lumen_set_dirty_value` natives into `Document::dirty_values`, because layout and form submission read the value from the document and cannot see a JS-side map. The old `_input_values[nid]` map survives only for the control kinds that do not have a document-side value (`<select>` picks, `<option>`), so a fix for one of those must not be «copied» to the text-entry path. The setter deliberately leaves the `value` content attribute alone — it is the default value `defaultValue`/`form.reset()` restore — and `HTMLFormElement.reset()` clears the document-side entry via `_lumen_clear_dirty_value`.
- **`TextDecoder`'s streaming/BOM state lives entirely in the shim, not natively (BUG-357, 2026-08-09).** `_lumen_text_decode`/`_lumen_text_encoding_for_label` (`v8_runtime.rs`, bridging `lumen_encoding::Encoding::from_label`/`decode_to_string_opts`) are plain stateless functions — one call, one complete buffer in, one `String` out. The `TextDecoder` JS wrapper (`dom.rs` `WEB_API_SHIM`) is what reassembles `{stream:true}` chunks (`_pending`, `_lumenTextPendingTailLen`) and enforces that a BOM is only stripped on a stream's first chunk (`_sawInput`, forcing `ignoreBOM=true` on later native calls regardless of the constructor option). A native handle/registry pattern (as used by WebSocket, fetch streaming) was considered and rejected: `lumen_encoding` has no incremental decoder object to hold, so there is no native state to hand JS a handle to. Only 8 encodings recognized (`lumen_encoding::Encoding`'s UTF-8/16/32 + windows-1251/koi8-r/ibm866) — every other WHATWG label (Shift_JIS, GBK, windows-1252, …) is a deliberate `RangeError`, per `docs/plan/tech-stack.md`'s rejection of `encoding_rs`/full label-table porting, not a gap to close later.

- **Custom properties are a snapshot of their own, not part of the computed-style map (BUG-732, 2026-08-10).** `_lumen_get_computed_style` answers standard properties from `V8JsRuntime::computed_styles`; `--`-prefixed names go to `_lumen_get_custom_property` / `custom_properties`, fed by `lumen_layout::collect_custom_properties` → `update_custom_properties` from the same five places that publish computed styles (four in `crates/shell/src/main.rs`, one in `crates/driver/src/session.rs`). The split is deliberate and load-bearing: custom properties inherit, so a `:root` set is one copy-on-write allocation (`CustomProps`) shared by the whole document, and every node inheriting it is handed the *same* `Arc` — merging them into the per-node standard map would re-materialise every variable on every node and multiply the cost of a snapshot the shell rebuilds after every relayout by the number of declared variables. Values arrive already computed (`var()`/`env()` substituted; an unresolvable reference is `""`, i.e. guaranteed-invalid), because the substitution needs the cascade's map and cannot be done JS-side. A new publish site must push both maps or the page sees variables from the previous layout.
- **`Headers` lives inside a closure in `WEB_API_SHIM`, and its Fetch guard is reachable only through two internal globals (BUG-369, 2026-08-10).** The header list and the guard sit in a `WeakMap` private to the IIFE that defines `Headers`, so there is no `_map`/`_key` for a page to read or clobber — but that also means no code outside the IIFE can set the guard, which `Response`/`Request` must do. The IIFE therefore assigns two pre-declared globals on the way out: `_lumen_headers_new(init, guard)` applies the guard *before* filling (the `Response`/`Request` constructors, where `init.headers` has to be guard-checked) and `_lumen_headers_set_guard(h, guard)` applies it *after* (the network path `Response._fromFetchCache` and both `clone()`s, where the header list comes from the wire or from another `Headers` and must be copied verbatim). Getting that order wrong is not cosmetic: filling a `response`-guarded `Headers` drops every `Set-Cookie`, so a network response — or a clone of one — would silently lose its cookies. A second consequence of the closure: a `Headers` instance has **no enumerable own properties**, so any `for..in` over a value that might be a `Headers` (there was one in `fetch()`, reading `Content-Type` out of `init.headers`) now sees nothing and needs an explicit `instanceof Headers` branch. The service-worker scope has its own, unrelated `Headers` in `sw_worker.rs` and is *not* covered by any of this (BUG-748).

- **`Response`/`Request` share one closure, and everything the rest of the shim needs from their slots goes through two named globals (BUG-370, 2026-08-10).** They were rewritten together, not separately, because they share the Body mixin (`installBody(proto, stateOf)` installs the identical seven members on both prototypes — divergence between them is now structurally impossible) and the same private-slot shape (`RSTATE`/`QSTATE`, one `WeakMap` each). Two consequences to know before touching this block. **(1)** Nothing outside the closure can reach a slot, so the network path and `fetch()` go through `_lumen_response_from_fetch_cache(status, statusText, headers, url)` and `_lumen_body_source(obj)`. The first replaced `Response._fromFetchCache(...)` *plus* an external `resp.url = url` — with `url` now a getter, that assignment would fail silently in sloppy mode, which is exactly why the URL became a factory parameter. The second exists because `Request.body` is now a `ReadableStream` (the Body mixin), while `fetch()` needs the raw string/`FormData`/bytes the constructor was handed. **(2)** The public `Response` constructor rejects states the shim itself has to build — status 0, `type: 'error'`, immutable headers — so `error()`/`redirect()`/`clone()` and the network factory all go through `rawResponse(slots)` (`Object.create(Response.prototype)` + `RSTATE.set`) instead. Guard ordering carries over from BUG-369 unchanged: `_lumen_headers_new` guards *before* filling, `_lumen_headers_set_guard` locks *after*, and `Response.redirect()` needs the second form because an `immutable` guard would reject its own `Location` write. A test can no longer fabricate a Response by assigning private fields onto `Object.create(Response.prototype)` — drive the real path (a mock `JsFetchProvider`) instead; the BUG-703 regression test was rewritten that way.
- **`DOMException` reaches a page only through `install_dom`, and its constructor is `(message, name)` — never the other way round (BUG-373, 2026-08-10).** The class does not exist in V8; `v8_runtime.rs` evaluates `DOM_EXCEPTION_POLYFILL` as one step of `install_dom`, so a module shim installed on its own — which is how every `install_*` unit test stands one up — has no `DOMException` at all and every `throw new DOMException(...)` inside it degrades to a `ReferenceError`. That is why a test asserting *anything* about a rejection has to eval `crate::v8_runtime::DOM_EXCEPTION_POLYFILL` first (`filesystem_access::tests_v8::with_fsa_for` does), and why it must use that constant rather than a hand-written `function DOMException(msg, name)` twin: the argument order is exactly what such a test is checking, and a stub would only prove itself. The order is uniform across `dom.rs` (~20 sites) and every module shim; `filesystem_access.rs` was the one file that wrote the name first, in all nine of its sites, so a swap is a whole-file habit rather than a slip — grep a shim as a unit, not line by line.
- **`Node.contains`/`compareDocumentPosition` walk arena node ids, never wrapper identity (BUG-732, 2026-08-10).** `_lumen_tree_nid` maps a receiver to its `__nid__`, and the live `document` literal to `_lumen_root_nid`. Identity-based walking is wrong here twice over: `document` is an object literal with no `__nid__`, and `documentElement.parentNode` answers `_lumen_make_element(root)` — a wrapper for the document root node, not the `document` object — so a `parentNode` walk comparing with `===` reports `document.contains(el) === false` for every element on the page. The same literal is not wired to `Node.prototype` (see `hasChildNodes`, BUG-327), so both methods also exist as own properties on it; anything else added to `Node.prototype` needs the same duplicate or it will be missing on `document`.
- **`Node.prototype` reaches most nodes, but FOUR node shapes are prototype-less literals and need an own copy of anything added there (BUG-377, 2026-08-10).** The BUG-732 note above names `document`; there are three more. `_lumen_make_document_fragment` and `_lumen_make_shadow_root` end with a bare `return frag`/`return sr` — no `Object.setPrototypeOf`, unlike `_lumen_build_element` (BUG-322), `_lumen_make_character_data`, `_lumen_make_processing_instruction` and `_lumen_make_doctype` (BUG-314/321), which all wire a real chain. `_lumen_build_detached_document` *does* have a chain (`Object.create(Document.prototype)`) but must still override, for the opposite reason: a document with no browsing context would otherwise inherit the *live* page's answer. So the checklist for a new `Node` member is five sites, not one: `Node.prototype` + those four. `Node.baseURI` is the worked example — one accessor plus four own copies, three delegating to `_lumen_document_base_url()` and the detached document answering `'about:blank'` like its neighbouring `URL`/`documentURI`.
- **The shim's `<base href>`-aware base URL has exactly one implementation, `_lumen_document_base_url()`, and every consumer delegates to it (BUG-377/BUG-383).** It computes HTML LS §4.2.3 (first `<base href>` resolved against the document URL, else the document URL) and backs `url`-kind IDL reflection, `fetch()`, `new Request()`, `Worker`/`SharedWorker` script URLs and now `Node.baseURI`. Adding a second `<base>` walk anywhere is how `el.href`, `fetch('rel')` and `node.baseURI` start disagreeing on one page. Note this holds only *within* the shim: the engine's own subresource resolution (`ResourceBase::resolve`, shell) ignores `<base href>` entirely and therefore already disagrees with all of the above — [BUG-752](../bugs/BUG-752-OPEN.md).
- **A navigating global accessor must NOT also be listed in the `var window = {…}` literal (BUG-376, 2026-08-10).** The end of `WEB_API_SHIM` copies every own property of the `window` literal onto `globalThis` and then repoints `window = globalThis` (BUG-280). The copy loop splits by descriptor kind: accessors go through `defineProperty`, plain values through assignment (`globalThis[k] = d.value`) — and that assignment is a `[[Set]]`, so it *runs* a setter already defined on `globalThis`. `location` is the first global whose setter has a side effect (it navigates), so leaving `location: location` in the literal fired a full navigation to the current URL on every page load. Neither branch is usable for such a property: the value branch invokes the setter, and the `defineProperty` branch throws `TypeError` against the `configurable:false` descriptor that `[LegacyUnforgeable]` requires. The property is therefore defined once, directly on `globalThis`, and omitted from the literal — `window.location` still resolves because `window` *becomes* `globalThis` a few lines later. Any future unforgeable/side-effecting global (`document`, `top`, `origin` — see BUG-587) needs the same treatment.
- **`Location` component setters delegate to a throwaway `URL`, and the engine's own URL commit bypasses them entirely (BUG-376, 2026-08-10).** `location.pathname = '/x'` builds `new URL(_lumen_loc_href)`, writes the component through `URL.prototype`'s setter, and navigates to the resulting `href`. This is deliberate reuse, not indirection: `URL.prototype` already owns every parsing/percent-encoding/re-serialization rule (BUG-375), and a second URL writer in the shim would drift from it. It also gets the ignore-cases right for free — a write the URL Standard drops (opaque path, invalid scheme, non-numeric port) leaves `href` unchanged, so nothing navigates and the object never half-applies. The mirror-image constraint: `_lumen_location_update`, which the engine calls to commit a navigation, writes **only** the backing vars (`_lumen_loc_parts`/`_lumen_loc_href`/`_lumen_loc_hash`). Before the fix it wrote `location.protocol = …` and friends — the same slots a page wrote — which is precisely why a page write updated a field and navigated nowhere; routing it through the new setters instead would turn every committed navigation into a fresh navigation request.
- **The two CSS Typed OM maps must never share a reader, and the class hierarchy is what enforces that (BUG-387, 2026-08-10).** `element.attributeStyleMap` reflects the inline `style=""` attribute; `element.computedStyleMap()` reflects the resolved cascade. Both live in `typed_om_api.rs::TYPED_OM_SHIM`, and the inheritance runs the spec's way (Typed OM L1 §6): `StylePropertyMapReadOnly` is the base — it *is* what `computedStyleMap()` returns — and the mutable `StylePropertyMap` extends it. Which source a map reads is fixed by the `__computed__` flag on the **subclass prototype**, not by the caller, so a member added to the base cannot silently inherit the wrong backing store. That is exactly what went wrong before: the computed map subclassed the inline one, inherited `_lumen_get_style_property`, and answered `undefined` for every property that came from a stylesheet. The computed side deliberately calls the *same* two natives `getComputedStyle` does (`_lumen_get_computed_style` for standard properties, `_lumen_get_custom_property` for `--`-names, per the BUG-732 split above), so the two APIs cannot drift; the corollary is that everything limiting `getComputedStyle` limits this map identically — the `computed_style_to_map` whitelist ([BUG-472](../bugs/BUG-472-OPEN.md)) and the fact that the snapshot is published by a layout pass rather than resolved on demand. Iteration has its own pair of natives (`_lumen_get_style_entries` / `_lumen_get_computed_style_entries`), both returning a **name-sorted** JSON array of `[property, value]` — both sources are `HashMap`s, so without the sort the iteration order of one page would differ between runs. `_css_property_key` (`v8_runtime.rs`) is the one place that decides the lookup key: a `--`-prefixed name passes through verbatim because custom-property names are case-sensitive, everything else is folded to kebab-case — all four inline bindings use it, and they have to agree or `set('--Foo')` and `get('--Foo')` land on different keys.

- **The detached document's child edge is a JS array, not an arena link — every `Node` member added to it has to bridge that one hop by hand (BUG-415, 2026-08-22).** `_lumen_build_detached_document` builds real arena nodes (`_lumen_create_element`, so children carry `__nid__` and mutate through the ordinary element wrappers), but the document→child edge itself has no arena backing: it lives in the builder's `_children` array. Two consequences that bit in order. **(1)** An inherited `Node.prototype` member that walks `parentNode` answers wrong rather than throwing — `contains` reported `false` for the document's own subtree, because the topmost child's `parentNode` is `null`. Same shape as the live `document` gotcha in the BUG-732 note above, different missing link. **(2)** Wrapper identity is *not* a usable child key: `_lumen_make_element` mints a fresh wrapper on every access, so `_detached_child_index` compares by identity first and falls back to `_lumen_tree_nid`. The pre-insert step (`_detached_adopt`, DOM 4.2.3) has to detach from *both* worlds — an arena parent via `_lumen_remove_child`, this list via `splice` — and `insertBefore` must re-read the reference index afterwards, since removing the inserted node from this same list can shift it. Related: `head`/`body` are rooted at the **html element**, which HTML LS 3.1.4 defines as the document element only when it is `html` in the HTML namespace; the `body` setter deliberately appends to the document element regardless, so a non-`html` root legitimately takes the child while the getter keeps answering `null`. That namespace test is only as good as the arena's `Namespace` enum, which cannot hold an arbitrary URI — [BUG-830](../bugs/BUG-830-OPEN.md).
- **`WEB_API_SHIM` is five consts, and two of them are the worker's copy of the page scope (BUG-401, 2026-08-11).** `dom.rs` keeps `WEB_API_SHIM_HEAD` / `EVENT_TARGET_SHIM` / `WEB_API_SHIM_MID` / `PERFORMANCE_SHIM` / `WEB_API_SHIM_TAIL`; `web_api_shim()` concatenates them **in source order**, so V8 still compiles the same single program with one hoisting scope — the split is invisible to the shim's own code and a reordering would break it (`Object.create(EventTarget.prototype)` before `EventTarget` exists). The two extracted blocks are exactly the ones WHATWG exposes in a `WorkerGlobalScope` (`EventTarget` is `[Exposed=*]`, `Performance` is `[Exposed=(Window,Worker)]`), and `worker_exposed_shim()` hands them to `worker::install_worker_scope_globals_v8`, which **all three** worker flavours call — `install_worker_globals_v8` (dedicated), `install_shared_worker_globals_v8`, `install_sw_globals_v8`. Anything else the page and a worker must share belongs in a sixth const evaluated from that one function, not in a second hand-written copy: the page's `Performance` had just been rebuilt as a real `EventTarget` subclass (BUG-400) and a private worker twin would have drifted from it immediately. Two consequences. **(1)** A block moved into a shared const must not depend on anything the page shim defines *after* it at top level — `PERFORMANCE_SHIM` calls `_perf_observer_notify` (`PerformanceObserver`, page-only) through a `typeof` guard for this reason; the guard is a no-op for the page and the whole reason `performance.mark()` works in a worker. **(2)** The worker's `_lumen_now_ms` native is registered per worker runtime and is wall-clock: `--deterministic` never reaches a worker isolate at all (the seed/clock patch is a `v8::Script` run in the page context inside `install_dom`), so freezing only `performance.now()` there would fake a determinism the scope does not have — [BUG-768](../bugs/BUG-768-OPEN.md).

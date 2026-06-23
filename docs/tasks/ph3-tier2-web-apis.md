# Ph3 — Tier 2 Web APIs (umbrella / index)

**Developer:** P1 (multiple sub-tasks, some shared with P3/P4)
**Branch:** various (one per constituent task)
**Size:** XL (umbrella — covers many independent sub-tasks)
**Crates:** `lumen-js`, `lumen-shell`

---

## Status

**Phase 3 future.** Do not start individual sub-tasks until Phase 2 closes and
a developer reserves the item in the matching `STATUS-PN.md`.

This file is an **index and orientation guide**, not a single implementation
task. Each constituent API either has its own dedicated task file (linked below)
or is tracked here with a one-line current-state note. When you claim a
constituent, create or update a dedicated `docs/tasks/ph3-<api>.md` and link it
back here.

---

## What "Tier 2" means

The project's own tiering is defined in `docs/plan/web-apis-shell.md` §7:

> **Tier 1** (needed by most sites): `document.*`, `Element.*`, `Node.*`, DOM
> API, `querySelector/All`, `addEventListener`, `fetch()`, `XMLHttpRequest`,
> `localStorage`/`sessionStorage`, `setTimeout`/`setInterval`/`rAF`,
> `console.*`, `window.location`, `window.history`, `URL`, `URLSearchParams`,
> `FormData`, `Blob`, `File`, `FileReader`, `btoa`/`atob`, `Promise`.
>
> **Tier 2**: `Canvas 2D`, `IndexedDB`, `WebSocket`, `MutationObserver`,
> `IntersectionObserver`, `ResizeObserver`, `requestIdleCallback`, Clipboard API
> (read/write with permission).

`docs/plan/phases.md:111` names "Tier 2 Web APIs" as a Phase 3 (v1.0) goal.
In practice most of the items listed under Tier 2 already reached functional
status during Phase 2 work. The Phase 3 task is to close the remaining gaps and
add the broader cluster of secondary APIs (observers, Intl, crypto, workers,
communication) to WPT-testable completeness. The goal is **WPT pass rate ≥ 60%**
(`docs/plan/phases.md:117`).

---

## Constituent APIs with dedicated task files

These are the large Tier-2 work items that have their own Phase 3 task files.
Each link is to `docs/tasks/`.

| Task file | API cluster | Current phase state |
|---|---|---|
| [ph3-indexeddb.md](ph3-indexeddb.md) | IndexedDB (W3C IDB 3.0) | Full JS shim + per-origin SQLite backend present (`crates/js/src/dom.rs:9452`). Transaction semantics and cursor edge-cases need WPT hardening. |
| [ph3-service-workers.md](ph3-service-workers.md) | Service Worker runtime | Lifecycle + registration persist ✅; fetch interception Phase 1 (SW runs in dedicated QuickJS thread, `FetchEvent`/`respondWith` dispatched). In-SW `fetch()` / `cache.addAll()` precaching absent. |
| [ph3-websockets-sse-fetch.md](ph3-websockets-sse-fetch.md) | WebSocket + SSE + Fetch/AbortController | WebSocket (RFC 6455 + permessage-deflate) ✅; SSE ✅; Fetch + AbortController ✅. Phase 3 = edge-case hardening + AbortSignal.timeout/any. |
| [ph3-navigation-history-api.md](ph3-navigation-history-api.md) | Navigation API + History API | `window.history` push/replace/back/forward present. Navigation API (`navigation.navigate()`, `NavigationEvent`) is a JS-only stub (`crates/js/src/navigation_api.rs`). |
| [ph3-web-animations-api.md](ph3-web-animations-api.md) | Web Animations API runtime | Real value interpolation + `document.timeline` present (`crates/js/src/dom.rs:11063`). Compositor offload and CSS animation integration (P2+P4 work) are Phase 3. |

---

## Remaining Tier-2 APIs without a dedicated task file

These APIs are installed in `crates/js/src/` but are not yet at WPT-grade
completeness. Each entry shows the current state and the primary source
location.

### Observers and timing

| API | Current state | Primary source |
|---|---|---|
| `MutationObserver` | **Functional** — full W3C subtree/attributes/childList/characterData observation, synchronous `_lumen_flush_mutations()` delivery hook (`crates/js/src/dom.rs:7693`) | `dom.rs:7693` |
| `ResizeObserver` | **Functional** — observes layout-rect changes, backed by `layout_rects` shared map updated by the shell each frame (`dom.rs:7734`) | `dom.rs:7734` |
| `IntersectionObserver` | **Functional** — root/threshold/rootMargin options, drives `loading=lazy` image load via `_lazy_io` (`dom.rs:7823`); threshold array fully supported | `dom.rs:7823` |
| `requestIdleCallback` / `cancelIdleCallback` | **Functional** — pure JS implementation, ticks via shared `requestAnimationFrame` loop (`dom.rs:9109`) | `dom.rs:9109` |
| `PerformanceObserver` | **Absent** — `performance.mark`/`measure`/`getEntries` present; `PerformanceObserver` class and `observe({type})` not yet installed | `dom.rs:8825` (performance object) |

### Communication APIs

| API | Current state | Primary source |
|---|---|---|
| `BroadcastChannel` | **Functional** — process-global hub, cross-tab same-origin delivery via `mpsc`, cross-thread delivery for Web Workers, fully wired in `QuickJsRuntime` | `crates/js/src/broadcast_channel.rs` |
| `MessageChannel` / `MessagePort` | **Functional** — entangled port pairs, `structuredClone` of payload, `postMessage`/`onmessage`/`addEventListener` (`dom.rs:9128`) | `dom.rs:9128` |
| `structuredClone` | **Functional** — supports primitives, Object, Array, Map, Set, ArrayBuffer; circular references and Transferable are not handled (`dom.rs:10694`) | `dom.rs:10694` |
| `SharedWorker` | **Functional** — per-process hub, real `std::thread`, `MessagePort` cross-thread delivery (`crates/js/src/shared_worker.rs`) | `crates/js/src/shared_worker.rs` |

### Clipboard API

| API | Current state | Primary source |
|---|---|---|
| `navigator.clipboard` | **Functional** — `readText`/`writeText` delegate to `_lumen_clipboard_read`/`_lumen_clipboard_write` natives installed by the shell; `read()`/`write()` are no-op stubs returning empty/void (`dom.rs:5944`) | `dom.rs:5944` |
| `navigator.permissions` | **Functional** — `query({name})` returns `PermissionStatus`; AV/sensor names resolve `denied`, everything else `granted`; no per-site persistence yet (`dom.rs:5977`) | `dom.rs:5977` |

### Intl / i18n

| API | Current state | Primary source |
|---|---|---|
| `Intl.NumberFormat` | **Functional (en-US, ru-RU)** — decimal/currency/percent styles, grouping, fraction digits, common currency symbols (`crates/js/src/intl_bindings.rs`) | `crates/js/src/intl_bindings.rs` |
| `Intl.DateTimeFormat` | **Functional (en-US, ru-RU)** — year/month/day/weekday/hour/minute/second, locale names, `hour12` (`intl_bindings.rs`) | `crates/js/src/intl_bindings.rs` |
| `Intl.Collator` | **Functional (en-US, ru-RU)** — case-insensitive, numeric, Cyrillic `ё` placement (`intl_bindings.rs`) | `crates/js/src/intl_bindings.rs` |
| `Intl.PluralRules` | **Functional (en-US, ru-RU)** — CLDR cardinal/ordinal categories (`intl_bindings.rs`) | `crates/js/src/intl_bindings.rs` |
| `Intl.RelativeTimeFormat` | **Absent** — not installed | — |
| `Intl.ListFormat` | **Absent** — not installed | — |
| `Intl.Segmenter` | **Absent** — not installed | — |

### Web Crypto

| API | Current state | Primary source |
|---|---|---|
| `crypto.getRandomValues` / `randomUUID` | **Functional** — backed by OS CSPRNG (`getrandom`) (`dom.rs:10452`) | `dom.rs:10452` |
| `crypto.subtle` (SubtleCrypto) | **Functional** — ECDSA P-256, HMAC-SHA*, AES-GCM: generateKey/importKey/exportKey/sign/verify/encrypt/decrypt. `digest` for SHA-1/256/384/512 via Rust `sha2`. Phase 3: RSA-OAEP, ECDH, HKDF, PBKDF2 absent. | `crates/js/src/subtle_crypto.rs` |

### Device / hardware APIs

| API | Current state | Primary source |
|---|---|---|
| `navigator.geolocation` | **Stub (deny-by-default)** — `getCurrentPosition`/`watchPosition` call error callback with `PERMISSION_DENIED` unless `FakeCoords` injected (fingerprint privacy); full W3C interface present (`crates/js/src/geolocation.rs`) | `crates/js/src/geolocation.rs` |
| `navigator.getBattery()` | **Disabled stub** — ADR-007 Layer 4 (fingerprinting source); returns rejected `Promise` matching Chrome's Permissions Policy removal (`crates/js/src/battery_bindings.rs`) | `crates/js/src/battery_bindings.rs` |
| `navigator.getGamepads()` | **Stub (no hardware)** — full W3C Gamepad Level 2 interface installed, `getGamepads()` returns 4 `null` slots, events never fire; no OS HID polling yet (`crates/js/src/gamepad.rs`) | `crates/js/src/gamepad.rs` |
| `navigator.mediaDevices.getUserMedia` | **Partial** — `{audio:true}` resolves with live `MediaStream` when `AudioCaptureProvider` installed by shell; video capture rejects `NotAllowedError`; `enumerateDevices()` real when provider present (`crates/js/src/media_devices.rs`) | `crates/js/src/media_devices.rs` |
| `navigator.mediaDevices.getDisplayMedia` | **Partial** — live when `ScreenCaptureProvider` installed (Win32 GDI backend) | `crates/js/src/screen_capture.rs` |
| `speechSynthesis` / `SpeechSynthesisUtterance` | **Functional** — OS TTS (PowerShell SAPI on Windows, `espeak` on Linux, `say` on macOS); estimated `start`/`end` events (`crates/js/src/speech.rs`) | `crates/js/src/speech.rs` |
| `SpeechRecognition` | **Stub (rejects)** — interface present, always rejects `service-not-allowed`; no ML model (`speech.rs:226`) | `crates/js/src/speech.rs:226` |
| `navigator.wakeLock` | **Functional (no-op)** — `request('screen')` resolves with `WakeLockSentinel`; no OS screen-wake integration (`dom.rs:11684`) | `dom.rs:11684` |
| `navigator.connection` | **Functional (fixed values)** — `effectiveType='4g'`, `downlink=10`, `rtt=50`, `saveData=false`; no real NIC polling (`dom.rs:11724`) | `dom.rs:11724` |

### Storage / quota

| API | Current state | Primary source |
|---|---|---|
| `navigator.storage` (StorageManager) | **Stub** — `estimate()` returns 0/10GiB; `persist()`/`persisted()` resolve `true`; `getDirectory()` returns OPFS root stub; no real OS metrics wired (`crates/js/src/storage_manager.rs`) | `crates/js/src/storage_manager.rs` |
| Cache API | **Functional** — `caches.open/match/put/delete` backed by in-process store shared with Service Worker intercept | `CAPABILITIES.md:123` |

### Push / Notifications

| API | Current state | Primary source |
|---|---|---|
| Notifications API | **Partial** — `new Notification()` constructor, `requestPermission()`, `close()`, events; shell wires `_lumen_show_notification` to OS delivery; permission `"denied"` by default (`crates/js/src/notifications_bindings.rs`) | `crates/js/src/notifications_bindings.rs` |
| Push API | **Stub** — in-memory subscriptions, static endpoint, no real push server connectivity (`crates/js/src/push_api.rs`) | `crates/js/src/push_api.rs` |

### Scheduler / Timing

| API | Current state | Primary source |
|---|---|---|
| `scheduler.postTask` / `scheduler.yield` | **Functional** — priority-based scheduling via `queueMicrotask`/`setTimeout`; `TaskController`/`TaskSignal`/`AbortSignal` integration; no rendering-pipeline integration yet (`crates/js/src/scheduler.rs`) | `crates/js/src/scheduler.rs` |

### Navigator misc

| API | Current state | Primary source |
|---|---|---|
| `navigator.share` | **Stub (rejects)** — `NotSupportedError` (`dom.rs:11756`) | `dom.rs:11756` |
| `navigator.userActivation` | **Functional** — `hasBeenActive`/`isActive` tracked (`dom.rs:23210` test confirms) | `dom.rs` |
| `navigator.languages` | **Stub (normalized)** — `["en-US","en"]` (ADR-007 fingerprint normalization) | `crates/js/src/navigator_bindings.rs` |
| `navigator.hardwareConcurrency` | **Normalized stub** — fixed `2` (ADR-007) | `crates/js/src/navigator_bindings.rs` |
| `navigator.deviceMemory` | **Normalized stub** — fixed `8` (ADR-007) | `crates/js/src/navigator_bindings.rs` |
| Web Locks (`navigator.locks`) | **Functional** — exclusive/shared/`ifAvailable`/`steal`/`signal` (AbortSignal), `query()` (`dom.rs:11527`) | `dom.rs:11527` |

---

## Suggested grouping into future tasks

The remaining gaps are small enough that they should be grouped by theme, not
one-per-API. Suggested bundles for Phase 3 task files:

1. **`ph3-intl-extended.md`** — `Intl.RelativeTimeFormat`, `Intl.ListFormat`,
   `Intl.Segmenter`, `Intl.DisplayNames`; CLDR data bundle decision.

2. **`ph3-performance-observer.md`** — `PerformanceObserver` + `PerformanceEntry`
   subtypes (`resource`, `paint`, `longtask`, `event`); connect to existing
   `performance.mark`/`measure` infrastructure at `dom.rs:8825`.

3. **`ph3-crypto-subtle-extended.md`** — RSA-OAEP, ECDH key derivation, HKDF,
   PBKDF2; extend `crates/js/src/subtle_crypto.rs`.

4. **`ph3-push-notifications.md`** — real Push API endpoint (VAPID, subscription
   to a real push service), OS notification integration across platforms;
   replaces Phase 0 in-memory stubs in `push_api.rs`.

5. **`ph3-gamepad.md`** — OS HID polling (Windows `XInput` / `DirectInput`,
   Linux evdev), haptic actuator wiring; upgrade `crates/js/src/gamepad.rs`
   from null-slot stub to live device dispatch.

6. **`ph3-storage-manager.md`** — real OS quota via `statvfs`/Windows
   `GetDiskFreeSpaceEx`, OPFS sandbox path; upgrade `storage_manager.rs` from
   stub to spec-compliant `estimate()`/`getDirectory()`.

7. **`ph3-web-speech-recognition.md`** — On-device speech recognition (Whisper
   or OS API); `SpeechRecognition` currently always rejects at `speech.rs:226`.

8. **`ph3-geolocation-runtime.md`** — OS geolocation (Windows Location API /
   Linux `geoclue`), permission UI, per-site grant; geolocation stub at
   `geolocation.rs` currently only supports injected `FakeCoords`.

---

## Definition of done

"Tier 2 complete" for Phase 3 means all of the following are true:

1. Every API in the Tier 2 definition (`docs/plan/web-apis-shell.md` §7) is
   functional, not stub-only.
2. Canvas 2D, IndexedDB, WebSocket pass relevant WPT subtests contributing to
   the workspace-level **WPT pass rate ≥ 60%** gate
   (`docs/plan/phases.md:117`).
3. `MutationObserver`, `ResizeObserver`, `IntersectionObserver` pass WPT
   `dom/observers/` subtests with no regressions.
4. `requestIdleCallback` and `Scheduler API` pass
   `html/infrastructure/task-source/` subtests.
5. Clipboard API (`readText`/`writeText`) functional with shell permission UI
   (see `ph3-permission-download-ui.md`).
6. `CAPABILITIES.md` updated to reflect ✅ on every completed item.
7. Any API that remains stub-only has an `OPEN` `BUGS.md` entry or a
   `KNOWN_DEBTORS` entry with rationale.

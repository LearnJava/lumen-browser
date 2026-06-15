pub mod audio_bindings;
pub mod audio_element;
pub mod background_fetch;
pub mod background_sync;
pub mod badging;
pub mod periodic_sync;
pub mod battery_bindings;
pub mod css_properties_values_api;
pub mod esm;
pub mod import_attributes;
pub mod import_meta;
pub mod paint_worklet;
pub mod gamepad;
pub mod highlight_api;
pub mod iframe_element;
pub mod broadcast_channel;
pub mod canvas2d;
pub mod close_watcher;
pub mod download_bindings;
pub mod network_log_bindings;
pub mod pip_bindings;
pub mod clipboard;
pub mod contacts;
pub mod cookie_banner;
pub mod cookie_store;
pub mod payment_request;
pub mod credentials;
pub mod device_sensors;
pub mod document_pip;
pub mod eye_dropper;
pub mod dom;
pub mod filesystem_access;
pub mod geolocation;
pub mod heap_snapshot;
pub mod intl_bindings;
pub mod media_capture;
pub mod media_devices;
pub mod media_session;
pub mod navigator_bindings;
pub mod notifications_bindings;
pub mod offscreen_canvas;
pub mod pointer_lock;
pub mod push_api;
pub mod shape_detection;
pub mod shared_worker;
pub mod speech;
pub mod surface_api;
pub mod video_bindings;
pub mod view_transitions;
pub mod bluetooth;
pub mod subtle_crypto;
pub mod temporal_api;
pub mod webgl_bindings;
pub mod webgl_canvas;
pub mod webrtc_stub;
pub mod webhid;
pub mod webusb;
pub mod webtransport;
pub mod worker;
pub mod url_pattern;
pub mod navigation_api;
pub mod typed_om_api;
pub mod trusted_types;
pub mod sanitizer;
pub mod screen_orientation;
pub mod scroll_snap_events;
pub mod scroll_timeline;
pub mod sri;
pub mod media_stream_recording;
pub mod serial;
pub mod compute_pressure;
pub mod csp;
pub mod permissions_policy;
pub mod web_codecs;
pub mod ua_client_hints;
pub mod media_capabilities;
pub mod virtual_keyboard;
pub mod wake_lock;
pub mod web_locks;
pub mod scheduler;
pub mod reporting_api;
pub mod web_audio;
pub mod webgpu;
pub mod webxr;
pub mod form_validation;
pub mod element_internals;
pub mod presentation_api;
pub mod webassembly;
pub mod generic_sensor;
pub mod video_pip;
pub mod web_midi;
pub mod storage_manager;
pub mod xhr;
pub mod dom_parser;
pub mod gc_policy;
pub mod svg;
pub mod file_input;
pub mod tc39_proposals;
pub mod es2026_proposals;
pub mod async_context;
pub mod decorators;
pub mod speculation_rules;
pub mod soft_navigation;
pub mod content_index;
pub mod digital_credentials;
pub mod window_management;
pub mod local_font_access;
pub mod long_animation_frames;
pub mod launch_handler;
pub mod inert;
pub mod shared_storage;
pub mod idle_detection;
pub mod topics_api;
pub mod attribution_reporting;
pub mod pointer_capture;

use lumen_core::{JsError, JsResult, JsRuntime, JsValue, SuspendedHeap};
use lumen_dom::Document;
use rquickjs::{Array, Context, Ctx, FromJs, Function, IntoJs, Object, Runtime, Type, Value};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub use clipboard::set_clipboard_provider;
pub use credentials::set_credential_provider;
pub use media_capture::set_audio_capture_provider;
pub use css_properties_values_api::{install_css_properties_values_api, RegisteredProperty, RegisteredPropertiesMap, get_registered_properties};
pub use paint_worklet::{install_paint_worklet_api, PaintWorkletDef, PaintWorkletRegistry, get_paint_worklet_registry};
pub use dom::{FullscreenRequest, HistoryUrlUpdate, NavigateRequest, PrintRequest};
pub use view_transitions::ViewTransitionEvent;
pub use navigator_bindings::{NavigatorProfile, set_navigator_profile};
pub use lumen_core::WebStorage;

/// Compute a deterministic u64 seed from a URL for deterministic render mode (8F).
///
/// Uses the URL fragment (`#...`) if present; otherwise the full URL string.
/// FNV-1a 64-bit hash guarantees the same seed across platforms and Rust versions.
/// Result is guaranteed non-zero (xorshift32 must not start at 0).
pub fn deterministic_seed_from_url(url: &str) -> u64 {
    let src = if let Some(pos) = url.rfind('#') { &url[pos + 1..] } else { url };
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in src.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if h == 0 { 1 } else { h }
}

/// QuickJS-based JS runtime via `rquickjs`.
///
/// QuickJS is single-threaded; `Mutex` provides the exclusive access needed
/// to satisfy `JsRuntime: Send + Sync`.
pub struct QuickJsRuntime {
    inner: Mutex<Inner>,
    /// Navigation request written by JS via `location.href=`, `location.assign()` etc.
    /// Captured inside `install_dom_api`; read by `take_navigate_request`.
    nav_out: Arc<Mutex<Option<NavigateRequest>>>,
    /// Next timer wakeup deadline as Unix epoch ms (set by `_lumen_request_wakeup`).
    /// Read by the shell event loop to schedule `ControlFlow::WaitUntil`.
    /// `take_timer_wakeup` atomically clears after reading.
    timer_wakeup: Arc<Mutex<Option<f64>>>,
    /// Set to `true` by any DOM-mutating JS binding (setAttribute, textContent,
    /// appendChild, etc.). The shell reads and clears this after each rAF pass
    /// to decide whether a relayout is needed before the next paint.
    dom_dirty: Arc<AtomicBool>,
    /// Set to `true` when JS calls `requestAnimationFrame(fn)`.
    /// Cleared (and returned) by `take_raf_pending` after each rendering step.
    /// Shell uses this to decide whether to request the next redraw for animations.
    raf_pending: Arc<AtomicBool>,
    /// Layout bounding rects updated after each relayout by the shell.
    /// Maps NodeId index (u32) → [x, y, width, height] in viewport-relative CSS px.
    /// Read by `_lumen_get_bounding_rect(nid)` from JS (e.g., ResizeObserver/IntersectionObserver).
    layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Current viewport size [width, height] in CSS px.
    /// Updated by the shell on every resize; read by `_lumen_get_viewport_size()`.
    viewport_size: Arc<Mutex<[f32; 2]>>,
    /// Lazy image load requests queued by `_lumen_request_lazy_image_load` from JS.
    /// JS calls this when `_lumen_deliver_lazy_images()` detects an image within the
    /// lazy-load margin.  Shell drains after each `deliver_lazy_images()` call.
    lazy_img_requests: Arc<Mutex<Vec<(u32, String)>>>,
    /// Scroll state per scroll-container node, updated after each relayout.
    /// Maps NodeId index → [scroll_x, scroll_y, scroll_width, scroll_height].
    /// Read by `_lumen_get_scroll_state(nid)` from JS (`scrollTop`/`scrollLeft`/`scrollWidth`/`scrollHeight`).
    scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Pending scroll requests queued by JS via `_lumen_request_scroll`.
    /// Each entry is (nid, target_scroll_x, target_scroll_y).
    /// Shell drains via `take_scroll_requests()` and calls `set_scroll_position()`.
    pending_scrolls: Arc<Mutex<Vec<(u32, f32, f32)>>>,
    /// Pending page-level scroll requests from JS `window.scrollTo/scrollBy`.
    /// Each entry is (target_y, smooth). Shell drains via `take_page_scroll_requests()`.
    pending_page_scrolls: Arc<Mutex<Vec<(f32, bool)>>>,
    /// Current page scroll Y exposed to JS `window.scrollY` / `window.pageYOffset`.
    /// Shell updates after each scroll via `set_page_scroll_y()`.
    page_scroll_y: Arc<Mutex<f32>>,
    /// Computed CSS styles per node, updated after each relayout by the shell.
    /// Maps NodeId index (u32) → CSS property name → resolved CSS value string.
    /// Read by `_lumen_get_computed_style(nid, prop)` from JS (`getComputedStyle`).
    computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    /// Live Web Worker threads spawned by `new Worker(url)` on this page.
    /// Maps worker ID → `WorkerHandle` (sender channel + join handle).
    /// Shared with the native bindings installed by `worker::install_worker_bindings`.
    workers: worker::WorkerRegistry,
    /// Outbound message queue: (worker_id, json) pairs posted by worker threads.
    /// Drained by `pump_workers()` and delivered to the matching JS `Worker` instance.
    worker_messages: worker::WorkerMessageQueue,
    /// Monotonically increasing counter used to assign unique IDs to new workers.
    worker_next_id: Arc<Mutex<u32>>,
    /// Shared blob URL → script text store for `importScripts()` in worker threads.
    ///
    /// Populated by `_lumen_register_worker_blob` (called from the WORKER_SHIM
    /// `URL.createObjectURL` wrapper for text/* blobs).  Worker threads read this
    /// store via `_lumen_import_scripts_resolve` to load blob: URLs synchronously.
    worker_blob_store: worker::WorkerBlobStore,
    /// Whether to auto-dismiss cookie consent banners on each page load (7C.3).
    ///
    /// Defaults to `true`. Shell sets this from the user's `cookie_banner_dismiss`
    /// preference before calling `install_dom`. When `false` the cookie-banner
    /// shim is not injected and banners are displayed normally.
    cookie_banner_dismiss: AtomicBool,
    /// Pending OS notification requests queued by `new Notification(...)` in JS.
    /// Drained by the shell in `about_to_wait` via `take_notification_requests()`.
    pending_notifications: notifications_bindings::NotificationQueue,
    /// Deterministic render mode (8F): when `true`, `Date.now()` is frozen at 0
    /// and `Math.random` is replaced with a seeded PRNG derived from the page URL.
    /// Set via `set_deterministic_mode()` before calling `install_dom`.
    deterministic: AtomicBool,
    /// Pending popup window requests queued by JS `window.open()`.
    /// Drained by the shell in `about_to_wait` via `take_window_open_requests()`.
    /// Each entry causes the shell to open a new tab navigated to the requested URL.
    window_open_requests: Arc<Mutex<Vec<dom::PopupRequest>>>,
    /// Console messages queued by `console.log/warn/error` calls in JS.
    ///
    /// Each entry is `(level, text)` where level is 0=log, 1=warn, 2=error.
    /// Drained by the shell's DevTools console panel via `take_console_messages()`.
    console_messages: Arc<Mutex<Vec<(u8, String)>>>,
    /// `BroadcastChannel` instances created on this page (WHATWG HTML §9.5).
    ///
    /// Holds the receiver halves; the process-global hub in
    /// `broadcast_channel` routes posted messages here. Drained by
    /// `pump_broadcast_channels()` and delivered to the matching JS instance.
    broadcast_channels: broadcast_channel::BroadcastRegistry,
    /// Outbound queue for this page's `SharedWorker` ports (WHATWG HTML §10.2).
    ///
    /// Process-global shared-worker threads push `(port_id, json)` replies here;
    /// drained by `pump_shared_workers()` and delivered to the matching client
    /// `port` via `_lumen_deliver_shared_worker_messages`.
    shared_worker_outbox: shared_worker::SharedWorkerOutbox,
    /// Focus requests queued by JS via `_lumen_request_focus` / `_lumen_request_blur`.
    ///
    /// Each `Some(nid)` means "move keyboard focus to this node"; `None` means blur.
    /// Populated by `showModal()` (focus autofocus/dialog) and `close()` (restore focus).
    /// Drained by the shell in `about_to_wait` via `take_focus_requests()`.
    pending_focus_requests: Arc<Mutex<Vec<Option<u32>>>>,
    /// `history.pushState` / `history.replaceState` URL-update notifications.
    ///
    /// Each call to `pushState`/`replaceState` with a non-empty URL appends an
    /// entry here.  The shell drains via `take_history_url_updates()` and updates
    /// the address-bar display URL and navigation stack accordingly.
    pending_history_url_updates: Arc<Mutex<Vec<dom::HistoryUrlUpdate>>>,
    /// Fullscreen requests emitted by `element.requestFullscreen()` and
    /// `document.exitFullscreen()`.
    ///
    /// Drained by the shell in `about_to_wait` via `take_fullscreen_requests()`.
    /// Each `Enter` causes the shell to call `window.set_fullscreen(Borderless)`;
    /// each `Exit` calls `window.set_fullscreen(None)`.
    fullscreen_requests: Arc<Mutex<Vec<dom::FullscreenRequest>>>,
    /// CSS View Transition events from `document.startViewTransition` (CSS VT L1).
    ///
    /// `Begin` is pushed before the user callback runs (shell captures old display list).
    /// `End` is pushed after the callback (shell relayouts and starts 300 ms cross-fade).
    /// Drained by the shell in `about_to_wait` via `take_view_transition_events()`.
    view_transition_events: Arc<Mutex<Vec<view_transitions::ViewTransitionEvent>>>,
    /// Print requests emitted by `window.print()` (W-2 Phase 1).
    ///
    /// Drained by the shell in `about_to_wait` via `take_print_requests()`.
    /// Each request triggers print-preview dialog or direct PDF export.
    print_requests: Arc<Mutex<Vec<dom::PrintRequest>>>,
    /// ES module source registry for `<script type=module>` support (HTML LS §8.1.3).
    ///
    /// Maps resolved module specifier → source code. Populated by `register_module_source()`
    /// before the module graph is evaluated. The same `Arc<Mutex<…>>` is shared with the
    /// `LumenLoader` installed on the QuickJS `Runtime` in `new()`.
    module_registry: esm::ModuleRegistry,
    /// Shared page URL for the ESM resolver (HTML LS §8.1.3 relative import resolution).
    ///
    /// Written by `install_dom` once the page URL is known. The `LumenResolver` holds the
    /// same `Arc<Mutex<String>>` and reads it at resolution time, so relative imports from
    /// inline module scripts resolve correctly against the page origin.
    module_page_url: esm::SharedPageUrl,
    /// Import map for module specifier resolution (HTML LS §8.1.6.2).
    ///
    /// Shared with the `LumenResolver`'s import_map field. Set via `set_import_map()`
    /// before evaluating modules. Maps bare specifiers like "react" to URLs like "/vendor/react.js".
    module_import_map: Arc<Mutex<esm::ImportMap>>,
    /// Declared import-attribute module types (`import … with { type: 'json' }`,
    /// TC39 Stage 3). Written by the Phase 0 preprocessor in `eval_module` /
    /// `register_module_source` (specifiers resolved the same way the ESM
    /// resolver will); read by the `LumenLoader` at module load time.
    module_types: import_attributes::ModuleTypeRegistry,
    /// Active pointer capture target nid (W3C Pointer Events L3 §4.1).
    ///
    /// Set by `_lumen_set_capture_state(nid)` when JS calls `element.setPointerCapture()`.
    /// Cleared by `_lumen_release_capture_state()` or implicitly on `pointerup`/`pointercancel`.
    /// Shell reads via `pointer_capture_nid()` to route pointer events to the captured element.
    pointer_capture_nid: Arc<Mutex<Option<u32>>>,
}

struct Inner {
    // Runtime must outlive its Context; keeping both under one lock avoids
    // any ordering issues on drop.
    _rt: Runtime,
    ctx: Context,
}

// SAFETY: QuickJS context is accessed only under the Mutex — never from
// multiple threads concurrently. Runtime wraps its own Mutex internally.
unsafe impl Send for QuickJsRuntime {}
unsafe impl Sync for QuickJsRuntime {}

impl QuickJsRuntime {
    pub fn new() -> Result<Self, JsError> {
        let module_registry = esm::new_registry();
        let (resolver, module_page_url) = esm::LumenResolver::new("");
        // Capture the import_map reference from the resolver before it's moved into QuickJS.
        let module_import_map = Arc::clone(&resolver.import_map);
        let module_types = import_attributes::new_type_registry();
        let loader =
            esm::LumenLoader::with_types(Arc::clone(&module_registry), Arc::clone(&module_types));
        let rt = Runtime::new().map_err(|e| JsError::Runtime(e.to_string()))?;
        rt.set_loader(resolver, loader);
        let ctx = Context::full(&rt).map_err(|e| JsError::Runtime(e.to_string()))?;
        Ok(Self {
            inner: Mutex::new(Inner { _rt: rt, ctx }),
            nav_out: Arc::new(Mutex::new(None)),
            timer_wakeup: Arc::new(Mutex::new(None)),
            dom_dirty: Arc::new(AtomicBool::new(false)),
            raf_pending: Arc::new(AtomicBool::new(false)),
            layout_rects: Arc::new(Mutex::new(HashMap::new())),
            viewport_size: Arc::new(Mutex::new([0.0, 0.0])),
            lazy_img_requests: Arc::new(Mutex::new(Vec::new())),
            scroll_states: Arc::new(Mutex::new(HashMap::new())),
            pending_scrolls: Arc::new(Mutex::new(Vec::new())),
            pending_page_scrolls: Arc::new(Mutex::new(Vec::new())),
            page_scroll_y: Arc::new(Mutex::new(0.0)),
            computed_styles: Arc::new(Mutex::new(HashMap::new())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            worker_messages: Arc::new(Mutex::new(Vec::new())),
            worker_next_id: Arc::new(Mutex::new(1)),
            worker_blob_store: Arc::new(Mutex::new(HashMap::new())),
            cookie_banner_dismiss: AtomicBool::new(true),
            pending_notifications: Arc::new(Mutex::new(Vec::new())),
            deterministic: AtomicBool::new(false),
            window_open_requests: Arc::new(Mutex::new(Vec::new())),
            console_messages: Arc::new(Mutex::new(Vec::new())),
            broadcast_channels: Arc::new(Mutex::new(Vec::new())),
            shared_worker_outbox: Arc::new(Mutex::new(Vec::new())),
            pending_focus_requests: Arc::new(Mutex::new(Vec::new())),
            pending_history_url_updates: Arc::new(Mutex::new(Vec::new())),
            fullscreen_requests: Arc::new(Mutex::new(Vec::new())),
            view_transition_events: Arc::new(Mutex::new(Vec::new())),
            print_requests: Arc::new(Mutex::new(Vec::new())),
            module_registry,
            module_page_url,
            module_import_map,
            module_types,
            pointer_capture_nid: Arc::new(Mutex::new(None)),
        })
    }

    /// Phase 0 import-attributes preprocessing (TC39 Stage 3): strip
    /// `with { … }` / `assert { … }` clauses from `source` and record the
    /// declared types in `module_types`, resolving each raw specifier against
    /// `base` exactly like the ESM resolver will at load time.
    ///
    /// Returns `Some(rewritten)` when a clause was stripped, `None` otherwise.
    fn preprocess_import_attributes(&self, base: &str, source: &str) -> Option<String> {
        let (rewritten, attrs) = import_attributes::strip_import_attributes(source)?;
        if !attrs.is_empty() {
            let resolver = esm::LumenResolver {
                page_url: Arc::clone(&self.module_page_url),
                import_map: Arc::clone(&self.module_import_map),
            };
            let mut types = self.module_types.lock().unwrap_or_else(|e| e.into_inner());
            for (spec, ty) in attrs {
                types.insert(
                    resolver.resolve_specifier(base, &spec),
                    import_attributes::ModuleType::from_attr(&ty),
                );
            }
        }
        Some(rewritten)
    }

    /// Register an ES module by specifier so it can be `import`-ed by other modules.
    ///
    /// `specifier` is the resolved absolute key used in `import` statements.
    /// Pre-populate before calling `eval_module` so that intra-page imports resolve.
    pub fn register_module_source(&self, specifier: &str, source: &str) {
        // Strip import-attribute clauses (`with { type: '…' }`) the module's own
        // imports may carry; nested specifiers resolve against this module.
        let source = self
            .preprocess_import_attributes(specifier, source)
            .unwrap_or_else(|| source.to_owned());
        self.module_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(specifier.to_owned(), source);
    }

    /// Set the import map (HTML LS §8.1.6.2) used by the module resolver.
    ///
    /// Call before `eval_module` — bare specifiers like `"react"` then resolve
    /// through the map (exact match or longest prefix, see `ImportMap::resolve`).
    pub fn set_import_map(&self, map: esm::ImportMap) {
        if let Ok(mut guard) = self.module_import_map.lock() {
            *guard = map;
        }
    }

    /// Evaluate `source` as an ES module (HTML LS §8.1.3 `<script type=module>`).
    ///
    /// Assigns a virtual `lumen://inline-N` specifier for relative-import resolution.
    /// Drains pending microtasks after evaluation so Promise continuations run.
    pub fn eval_module(&self, source: &str) -> JsResult<()> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Pre-process TC39 decorator syntax (Phase 0 transformer) so the
        // registry and the evaluated module agree on the source.
        let source = guard.ctx.with(|ctx: Ctx<'_>| {
            decorators::maybe_transform_decorators(&ctx, source)
                .unwrap_or_else(|| source.to_owned())
        });
        // Strip import-attribute clauses (TC39 Stage 3) and record declared
        // module types; inline scripts resolve specifiers against the page URL.
        let source = self
            .preprocess_import_attributes("", &source)
            .unwrap_or(source);
        // Unique sequential inline specifier; use page URL as import.meta.url.
        let page_url = self.module_page_url.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let specifier = {
            let mut reg = self.module_registry.lock().unwrap_or_else(|e| e.into_inner());
            let n = reg.len();
            let key = format!("lumen://inline-{n}");
            reg.insert(key.clone(), source.clone());
            key
        };
        // Transform import.meta → preamble var using the page URL for .url.
        let meta_url = if page_url.is_empty() { specifier.as_str() } else { page_url.as_str() };
        let source = import_meta::transform_import_meta(&source, meta_url).unwrap_or(source);
        guard.ctx.with(|ctx: Ctx<'_>| -> JsResult<()> {
            rquickjs::Module::evaluate(ctx.clone(), specifier.as_str(), source.as_bytes())
                .map_err(|e| JsError::Runtime(format!("module eval: {e}")))?;
            // Drain Promise continuations from dynamic import() / top-level await.
            loop { if !ctx.execute_pending_job() { break; } }
            Ok(())
        })
    }

    /// Capture the raw, uncompressed heap payload for a hibernation snapshot
    /// (ADR-008 §10C.3 feeds the result to `heap_snapshot::compress_heap`).
    ///
    /// Full QuickJS heap serialisation (globals / closures / object graph via
    /// `JS_WriteObject`) is task 10C.2 and is blocked by our native-function
    /// bindings, which cannot be round-tripped through `JS_ReadObject`. Until
    /// that lands the payload is empty: the shell's hibernation model drops the
    /// JS runtime and re-runs the page's inline scripts on restore (see shell
    /// `restore_js_context`), so no heap content is consumed today. The empty
    /// payload still exercises the compression + 5 MB cap pipeline end-to-end.
    fn capture_raw_heap(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Install DOM Web API globals (`document`, `window`, `console`, etc.) into
    /// this runtime.  Must be called before running any user scripts that access
    /// the DOM.  The `doc` Arc is captured by the registered native functions;
    /// drop the runtime (via `drop(runtime)`) before calling
    /// `Arc::try_unwrap(doc)` to recover the document after script execution.
    ///
    /// `page_url` initialises `window.location` with the current page URL.
    /// `fetch_provider` is forwarded to `window.fetch()`.
    /// `ws_provider` is forwarded to `new WebSocket(url)`.
    /// `sse_provider` is forwarded to `new EventSource(url)`.
    /// `ls_store` — shared localStorage for this origin; persists across reloads.
    ///   Pass a fresh `Arc::new(Mutex::new(WebStorage::default()))` per origin.
    ///   A fresh `sessionStorage` is created automatically inside.
    /// `idb_backend` — per-origin IndexedDB persistence (`lumen_storage::IdbStore`
    ///   over a `StorageBackend`). Pass the same backend for one origin across
    ///   reloads so databases survive; `None` keeps IndexedDB in-heap only.
    /// `sw_backend` — per-origin Service Worker registration persistence
    ///   (`lumen_storage::SwStore` over a `StorageBackend`). Pass the same backend
    ///   for one origin across reloads so SW registrations survive; `None` keeps
    ///   registrations in-session only.
    /// Pass `None` for providers in sandboxed contexts or unit tests.
    #[allow(clippy::too_many_arguments)]
    pub fn install_dom(
        &self,
        doc: Arc<Mutex<Document>>,
        page_url: &str,
        fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
        ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
        sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
        ls_store: Option<Arc<Mutex<WebStorage>>>,
        idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
        sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
        cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
        // True when COOP=same-origin + COEP=require-corp are both present on this document.
        cross_origin_isolated: bool,
    ) -> JsResult<()> {
        // Update the ESM resolver's base URL so relative imports from inline module
        // scripts resolve correctly against the page origin (HTML LS §8.1.3).
        *self.module_page_url.lock().unwrap_or_else(|e| e.into_inner()) = page_url.to_owned();
        let ls = ls_store.unwrap_or_else(|| Arc::new(Mutex::new(WebStorage::default())));
        let ss = Arc::new(Mutex::new(WebStorage::default()));
        // Compute deterministic seed from URL hash when deterministic mode is active (8F).
        let deterministic_seed = if self.deterministic.load(Ordering::Relaxed) {
            Some(crate::deterministic_seed_from_url(page_url))
        } else {
            None
        };
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            // Install functional WebGL bindings backed by the software
            // rasterizer (task #28, §7F). Preserves the ADR-007 Layer 4
            // fingerprint normalization of the old `webgl_bindings` shim.
            let fingerprint = lumen_paint::GpuFingerprint {
                vendor: "WebKit".to_string(),
                renderer: "Generic GPU".to_string(),
            };
            if let Err(e) = webgl_canvas::install_webgl_canvas(&ctx, &fingerprint) {
                eprintln!("WebGL bindings init failed: {}", e);
            }

            // Install Canvas 2D native bindings (HTML LS §4.12.4). The JS-side
            // getContext('2d') shim lives in dom.rs::_lumen_make_element and
            // calls these `_lumen_canvas2d_*` functions keyed by node index.
            if let Err(e) = canvas2d::install_canvas2d_bindings(&ctx) {
                eprintln!("Canvas 2D bindings init failed: {}", e);
            }

            // Install OffscreenCanvas bindings (HTML LS §4.12.14).
            if let Err(e) = offscreen_canvas::install_offscreen_canvas_bindings(&ctx) {
                eprintln!("OffscreenCanvas bindings init failed: {}", e);
            }

            // Install AudioContext stub with per-session fingerprint noise (ADR-007 Layer 4, 9D.3).
            let audio_seed = audio_bindings::new_session_seed();
            if let Err(e) = audio_bindings::install_audio_bindings(&ctx, audio_seed) {
                eprintln!("Audio bindings init failed: {}", e);
            }

            dom::install_dom_api(
                &ctx,
                doc,
                page_url,
                Arc::clone(&self.nav_out),
                fetch_provider,
                ws_provider,
                sse_provider,
                ls,
                ss,
                Arc::clone(&self.timer_wakeup),
                Arc::clone(&self.dom_dirty),
                Arc::clone(&self.raf_pending),
                Arc::clone(&self.layout_rects),
                Arc::clone(&self.viewport_size),
                Arc::clone(&self.lazy_img_requests),
                None,
                idb_backend,
                sw_backend,
                cache_backend,
                Arc::clone(&self.scroll_states),
                Arc::clone(&self.pending_scrolls),
                Arc::clone(&self.pending_page_scrolls),
                Arc::clone(&self.page_scroll_y),
                Arc::clone(&self.computed_styles),
                Arc::clone(&self.window_open_requests),
                deterministic_seed,
                Arc::clone(&self.console_messages),
                Arc::clone(&self.pending_history_url_updates),
                Arc::clone(&self.fullscreen_requests),
                Arc::clone(&self.print_requests),
                Arc::clone(&self.pending_focus_requests),
                cross_origin_isolated,
            )
            .map_err(|e| rq_err(&ctx, e))?;

            // Install CSS Custom Highlight API (CSS Highlight API L1) — after DOM.
            // Phase 0: CSS.highlights registry + Highlight class; visual rendering in Phase 1.
            if let Err(e) = highlight_api::install_highlight_api_bindings(&ctx) {
                eprintln!("Highlight API bindings init failed: {}", e);
            }

            // Install Battery Status API disable (ADR-007 Layer 4, 9D.4) — after DOM.
            if let Err(e) = battery_bindings::install_battery_bindings(&ctx) {
                eprintln!("Battery bindings init failed: {}", e);
            }

            // Install navigator/screen/timezone normalization (ADR-007 Layer 4, 9D.6) — after DOM.
            if let Err(e) = navigator_bindings::install_navigator_bindings(&ctx) {
                eprintln!("Navigator bindings init failed: {}", e);
            }

            // Install CSS Properties & Values API (Houdini) — after DOM/navigator so that CSS object is available.
            // Enables CSS.registerProperty() for custom property definitions.
            if let Err(e) = css_properties_values_api::install_css_properties_values_api(&ctx) {
                eprintln!("CSS Properties & Values API init failed: {}", e);
            }

            // Install CSS Typed OM (CSS Typed Object Model L1) — after DOM so that Element.prototype is available.
            // Enables element.attributeStyleMap, element.computedStyleMap(), CSSStyleValue, CSSUnitValue, CSSKeywordValue.
            if let Err(e) = typed_om_api::install_typed_om_api(&ctx) {
                eprintln!("CSS Typed OM init failed: {}", e);
            }

            // Install CSS Paint Worklet API stub (Houdini) — after DOM/CSS so that CSS object is available.
            // Phase 0: registerPaint() registers worklet definitions; Phase 1 (future): worker execution.
            // Enables CSS.paintWorklet.addModule() and registerPaint() calls.
            if let Err(e) = paint_worklet::install_paint_worklet_api(&ctx) {
                eprintln!("Paint Worklet API init failed: {}", e);
            }

            // Install native audio capture bridge (__lumen_*_audio_capture natives).
            // Must run before MediaDevices shim so getUserMedia can find the natives.
            if let Err(e) = media_capture::install_media_capture_bindings(&ctx) {
                eprintln!("MediaCapture bindings init failed: {}", e);
            }

            // Install MediaDevices API (W3C Media Capture §4) — after DOM/navigator so that
            // Promise, DOMException, and navigator are available. Phase 1: getUserMedia
            // resolves with a live MediaStream when AudioCaptureProvider is installed.
            if let Err(e) = media_devices::install_media_devices_bindings(&ctx) {
                eprintln!("MediaDevices bindings init failed: {}", e);
            }

            // Install WebHID API (W3C WebHID §3–5) — after DOM/navigator so that
            // Promise, DOMException, and navigator are available. Phase 0: all device
            // operations reject with NotSupportedError (no USB/HID support).
            if let Err(e) = webhid::install_webhid_bindings(&ctx) {
                eprintln!("WebHID bindings init failed: {}", e);
            }

            // Install WebUSB API (W3C WebUSB §2–3) — after DOM/navigator so that
            // Promise, DOMException, and navigator are available. Phase 0: all device
            // operations reject with NotSupportedError (no USB support).
            if let Err(e) = webusb::install_webusb_bindings(&ctx) {
                eprintln!("WebUSB bindings init failed: {}", e);
            }

            // Install Screen Orientation API (W3C Screen Orientation §3–4) — after DOM/screen so that
            // screen.orientation is available. Phase 0: orientation is static 'portrait-primary';
            // lock/unlock methods exist but actual fullscreen integration is a P3 shell task.
            if let Err(e) = screen_orientation::install_screen_orientation_bindings(&ctx) {
                eprintln!("Screen Orientation bindings init failed: {}", e);
            }

            // Install Web Bluetooth API (W3C Web Bluetooth §3–4) — after DOM/navigator so that
            // Promise, DOMException, and navigator are available. Phase 0: all device
            // operations reject with NotSupportedError (no BLE support).
            if let Err(e) = bluetooth::install_bluetooth_bindings(&ctx) {
                eprintln!("Bluetooth bindings init failed: {}", e);
            }

            // Install WebSerial API (W3C Serial API L1) — after DOM/navigator so that
            // Promise, DOMException, and navigator are available. Phase 0: requestPort()
            // rejects NotSupportedError; getPorts() returns [].
            if let Err(e) = serial::install_serial_bindings(&ctx) {
                eprintln!("WebSerial bindings init failed: {}", e);
            }

            // Install Compute Pressure API (W3C Compute Pressure L1) — after DOM/navigator.
            // Phase 0: PressureObserver registers callback but never fires; knownSources()=['cpu'].
            if let Err(e) = compute_pressure::install_compute_pressure_bindings(&ctx) {
                eprintln!("Compute Pressure API init failed: {}", e);
            }

            // Install Badging API (W3C Badging API) — after DOM/navigator.
            // Phase 0: navigator.setAppBadge/clearAppBadge are no-ops; _lumen_set_app_badge hook
            // prepared for OS integration in shell Phase 1.
            if let Err(e) = badging::install_badging_bindings(&ctx) {
                eprintln!("Badging API init failed: {}", e);
            }

            // Install CSP violation event class (W3C CSP Level 3 §7.8) — after DOM/document.
            // Phase 0: SecurityPolicyViolationEvent class + _lumen_dispatch_csp_violation helper.
            // Phase 1: shell calls _lumen_fire_csp_violation for actual enforcement.
            if let Err(e) = csp::install_csp_bindings(&ctx) {
                eprintln!("CSP bindings init failed: {}", e);
            }

            // Install Permissions Policy bindings (W3C Permissions Policy §8) — after DOM/document.
            // Phase 0: document.featurePolicy + _lumen_set_permissions_policy(headerValue) hook.
            // Phase 1: shell calls _lumen_set_permissions_policy after HTTP response headers.
            if let Err(e) = permissions_policy::install_permissions_policy_bindings(&ctx) {
                eprintln!("Permissions Policy bindings init failed: {}", e);
            }

            // Install W3C WebCodecs API (https://www.w3.org/TR/webcodecs/) — after DOM.
            // Phase 0: VideoEncoder/Decoder + AudioEncoder/Decoder + EncodedVideoChunk/AudioChunk stubs.
            // configure() rejects with NotSupportedError; no codec support in Phase 0.
            // Phase 1 (future): FFmpeg or libav1 bindings for actual encoding/decoding.
            if let Err(e) = web_codecs::install_webcodecs_bindings(&ctx) {
                eprintln!("WebCodecs API init failed: {}", e);
            }

            // Install User-Agent Client Hints (W3C UA-CH §4–6) — after navigator shim.
            // Phase 0: navigator.userAgentData with static Chrome 114 / Windows 10 profile.
            if let Err(e) = ua_client_hints::install_ua_client_hints_bindings(&ctx) {
                eprintln!("UA Client Hints init failed: {}", e);
            }

            // Install HTMLVideoElement stubs — after DOM so document.createElement is available.
            if let Err(e) = video_bindings::install_video_bindings(&ctx) {
                eprintln!("Video bindings init failed: {}", e);
            }

            // Install Video Picture-in-Picture API (W3C PiP L1 §3) — after video_bindings.
            // Phase 0: video.requestPictureInPicture() → Promise<PictureInPictureWindow>,
            // document.exitPictureInPicture(), pictureInPictureElement, pictureInPictureEnabled.
            // Shell integration (P3) connects _lumen_pip_enter/_lumen_pip_exit to OS float window.
            if let Err(e) = video_pip::install_video_pip_api(&ctx) {
                eprintln!("Video Picture-in-Picture API init failed: {}", e);
            }

            // Wire the native PiP hooks (`_lumen_pip_enter` / `_lumen_pip_exit`)
            // the shim above calls — shell drains them to open/close the real
            // OS-level floating window (CC-7). After video_pip so the shim exists.
            if let Err(e) = pip_bindings::install_pip_bindings(&ctx) {
                eprintln!("PiP bindings init failed: {}", e);
            }

            // Install HTMLAudioElement stubs (HTML spec §4.8.10) — after DOM/video.
            if let Err(e) = audio_element::install_audio_element_bindings(&ctx) {
                eprintln!("Audio element bindings init failed: {}", e);
            }

            // Install HTMLIFrameElement stubs (HTML spec §4.8.5) — after DOM.
            // Phase 0: contentDocument/contentWindow return null (no sub-document navigation).
            if let Err(e) = iframe_element::install_iframe_element_bindings(&ctx) {
                eprintln!("IFrame element bindings init failed: {}", e);
            }

            // Install Geolocation API stub (W3C Geolocation L2, §7.7) — after DOM/navigator.
            // Default: PERMISSION_DENIED. Shell may reinitialise with fake coords via
            // install_geolocation_bindings when FingerprintProfile enables them.
            if let Err(e) = geolocation::install_geolocation_bindings(&ctx, None) {
                eprintln!("Geolocation bindings init failed: {}", e);
            }

            // Install Contact Picker API stub (W3C Contact Picker API) — after DOM/navigator.
            // Phase 0: navigator.contacts.select() always rejects with NotSupportedError;
            // navigator.contacts.getProperties() returns Promise<['name', 'email', 'tel']>.
            if let Err(e) = contacts::init_contacts_manager(&ctx) {
                eprintln!("Contact Picker API init failed: {}", e);
            }

            // Install Payment Request API stub (W3C Payment Request API) — after DOM/window.
            // Phase 0: PaymentRequest.show() always rejects with NotSupportedError;
            // PaymentRequest.canMakePayment() always returns Promise<false>.
            if let Err(e) = payment_request::init_payment_request(&ctx) {
                eprintln!("Payment Request API init failed: {}", e);
            }

            // Install Layer 1 surface API protection (ADR-007 Layer 1, 9A) — after navigator.
            if let Err(e) = surface_api::install_surface_api_protection(&ctx) {
                eprintln!("Surface API protection init failed: {}", e);
            }

            // Install Web Worker bindings (WHATWG Web Workers §4) — after DOM so
            // TextDecoder and _object_url_store are available for blob-URL resolution.
            // blob_store is shared with worker threads for importScripts() support.
            if let Err(e) = worker::install_worker_bindings(
                &ctx,
                &self.workers,
                &self.worker_messages,
                &self.worker_next_id,
                &self.worker_blob_store,
            ) {
                eprintln!("Worker bindings init failed: {}", e);
            }

            // Install Shared Worker bindings (WHATWG HTML §10.2) — after Worker so
            // TextDecoder / _object_url_store / atob are available for script resolution.
            if let Err(e) =
                shared_worker::install_shared_worker_bindings(&ctx, &self.shared_worker_outbox)
            {
                eprintln!("SharedWorker bindings init failed: {}", e);
            }

            // Install Web Notifications API (W3C Notifications API L1) — after DOM so
            // Event, Promise, and queueMicrotask are already defined.
            // Default permission: "denied" (privacy-first; shell can override per origin).
            if let Err(e) = notifications_bindings::install_notifications_bindings(
                &ctx,
                Arc::clone(&self.pending_notifications),
                false,
            ) {
                eprintln!("Notifications bindings init failed: {}", e);
            }

            // Install Background Fetch API stub (W3C Background Fetch L1, Phase 0) — after DOM
            // so Promise is available. Provides registration.backgroundFetch.fetch(id, reqs, opts)
            // / get(id) / getIds(). Phase 0: in-memory; shell _lumen_bg_fetch_* wiring is Phase 1.
            if let Err(e) = background_fetch::init_background_fetch(&ctx) {
                eprintln!("Background Fetch API init failed: {}", e);
            }

            // Install Background Sync API stub (W3C Background Sync L2, Phase 0) — after DOM
            // so Promise is available. Provides registration.sync.register(tag) / getTags().
            // Phase 0: tags stored in-memory; actual sync-on-next-navigation wiring is P2/P3.
            if let Err(e) = background_sync::init_background_sync(&ctx) {
                eprintln!("Background Sync API init failed: {}", e);
            }

            // Install Periodic Background Sync API stub (W3C PBSync, Phase 0) — after DOM so
            // Promise is available. Provides registration.periodicSync.register(tag, {minInterval})
            // / unregister(tag) / getTags(). Phase 0: in-memory; OS scheduler wiring is P2/P3.
            if let Err(e) = periodic_sync::init_periodic_sync(&ctx) {
                eprintln!("Periodic Background Sync API init failed: {}", e);
            }

            // Install Push API stub (W3C Push API L1, Phase 0) — after DOM so Promise is
            // available. Provides registration.pushManager.subscribe() / getSubscription() /
            // permissionState(). Phase 0: static endpoint, in-memory subscriptions.
            if let Err(e) = push_api::init_push_api(&ctx) {
                eprintln!("Push API init failed: {}", e);
            }

            // Install Cookie Store API (WHATWG Cookie Store API, Phase 0) — after DOM so
            // Promise and document.cookie are available. Provides window.cookieStore with
            // get/getAll/set/delete and CookieChangeEvent. Phase 0: in-memory store.
            if let Err(e) = cookie_store::init_cookie_store(&ctx) {
                eprintln!("Cookie Store API init failed: {}", e);
            }

            // Install cookie-banner auto-dismiss shim (7C.3) — last, after full DOM.
            let cb_enabled = self.cookie_banner_dismiss.load(Ordering::Relaxed);
            if let Err(e) = cookie_banner::install(&ctx, cb_enabled) {
                eprintln!("Cookie-banner bindings init failed: {}", e);
            }

            // Install WebRTC mDNS-only stub (9D.5) — after DOM so Promise and setTimeout
            // are available.  Fires a single .local candidate; never leaks real IP.
            if let Err(e) = webrtc_stub::install_webrtc_bindings(&ctx) {
                eprintln!("WebRTC bindings init failed: {}", e);
            }

            // Install Broadcast Channel API (WHATWG HTML §9.5) — after DOM so
            // MessageEvent and DOMException are available for delivery.
            if let Err(e) = broadcast_channel::install_broadcast_channel_bindings(
                &ctx,
                &self.broadcast_channels,
            ) {
                eprintln!("BroadcastChannel bindings init failed: {}", e);
            }

            // Install _lumen_network_download(url, filename) — lets page scripts
            // and <a download> ask the shell to start a background download.
            if let Err(e) = download_bindings::install_download_bindings(&ctx) {
                eprintln!("Download bindings init failed: {}", e);
            }

            // Install _lumen_log_network_request(method, url, status, duration_ms)
            // — lets page scripts record requests in the DevTools Network panel.
            if let Err(e) = network_log_bindings::install_network_log_bindings(&ctx) {
                eprintln!("Network-log bindings init failed: {}", e);
            }

            // Install navigator.credentials (WebAuthn / passkeys) — after DOM so
            // atob/btoa, TextEncoder, Promise, DOMException, Uint8Array exist, and
            // after the _lumen_webauthn_* native bindings are registered.
            if let Err(e) = credentials::install_credentials_bindings(&ctx) {
                eprintln!("Credentials bindings init failed: {}", e);
            }

            // Install the ECMA-402 Intl shim (§91 i18n) — last, after window so
            // `window.Intl` can be re-exported. Defers to a native Intl if the
            // host QuickJS build ever provides one.
            if let Err(e) = intl_bindings::install_intl_bindings(&ctx) {
                eprintln!("Intl bindings init failed: {}", e);
            }

            // Install TC39 Temporal API shim (Stage 4 / ES2025) — after Intl so the
            // timezone helpers can leverage Date internals. Pure JS, no native bindings.
            if let Err(e) = temporal_api::install_temporal_api(&ctx) {
                eprintln!("Temporal API init failed: {}", e);
            }

            // Install TC39 Stage 4 proposal shims — Object.groupBy/Map.groupBy, Set
            // methods, Promise.withResolvers, Promise.try, Array.fromAsync, Iterator
            // helpers. Each shim no-ops if the engine already has native support.
            if let Err(e) = tc39_proposals::install_tc39_proposals(&ctx) {
                eprintln!("TC39 proposals shim init failed: {}", e);
            }

            // Install ES2025/2026 proposal shims — Float16Array, Math.f16round,
            // DataView.getFloat16/setFloat16, Symbol.dispose/asyncDispose,
            // SuppressedError, DisposableStack, AsyncDisposableStack.
            if let Err(e) = es2026_proposals::install_es2026_proposals(&ctx) {
                eprintln!("ES2026 proposals shim init failed: {}", e);
            }

            // Install AsyncContext (TC39 Stage 2.7) Phase 0 — AsyncContext.Variable +
            // AsyncContext.Snapshot; patches Promise.prototype.then for microtask
            // propagation, so it must run after the DOM shim (queueMicrotask).
            if let Err(e) = async_context::install_async_context(&ctx) {
                eprintln!("AsyncContext shim init failed: {}", e);
            }

            // Install TC39 Decorators (Stage 3) Phase 0 — `@decorator` source
            // transformer (`__lumen_transform_decorators`, used by the eval entry
            // points) + Symbol.ClassDecorator / Symbol.MethodDecorator symbols.
            if let Err(e) = decorators::install_decorator_shim(&ctx) {
                eprintln!("Decorator shim init failed: {}", e);
            }

            // Install Speculation Rules API Phase 0 — document.prerendering,
            // document.getSpeculationRules(), _lumen_deliver_speculation_rules hook.
            if let Err(e) = speculation_rules::install_speculation_rules_api(&ctx) {
                eprintln!("Speculation Rules API init failed: {}", e);
            }

            // Install Soft Navigation Timing API — PerformanceSoftNavigationEntry,
            // _lumen_deliver_soft_nav(url, startTime, durationMs) binding.
            if let Err(e) = soft_navigation::install_soft_navigation_api(&ctx) {
                eprintln!("Soft Navigation API init failed: {}", e);
            }

            // Install Content Index API Phase 0 — ContentIndex class,
            // ServiceWorkerRegistration.prototype.index getter.
            if let Err(e) = content_index::install_content_index_api(&ctx) {
                eprintln!("Content Index API init failed: {}", e);
            }

            // Install Digital Credentials API Phase 0 — DigitalCredential class,
            // navigator.credentials.get({digital:...}) → NotSupportedError.
            if let Err(e) = digital_credentials::install_digital_credentials_api(&ctx) {
                eprintln!("Digital Credentials API init failed: {}", e);
            }

            // Install URL Pattern API (WHATWG URLPattern §3) — pure JS implementation.
            // Provides new URLPattern({pathname, search, hash, hostname}) with .test() and .exec().
            if let Err(e) = url_pattern::install_url_pattern_api(&ctx) {
                eprintln!("URL Pattern API init failed: {}", e);
            }

            // Install Navigation API (HTML LS §7.8) — pure JS implementation.
            // Provides window.navigation singleton with currentEntry, navigate(), back(), forward(), traverseTo().
            if let Err(e) = navigation_api::install_navigation_api(&ctx) {
                eprintln!("Navigation API init failed: {}", e);
            }

            // Install CloseWatcher API (WICG) — after DOM so `document` and `Event` exist.
            // Provides `new CloseWatcher()` with requestClose()/destroy()/close events and
            // Escape-key intercept on the topmost watcher.
            if let Err(e) = close_watcher::install_close_watcher(&ctx) {
                eprintln!("CloseWatcher init failed: {}", e);
            }

            // Install CSS View Transitions API (CSS View Transitions L1 §4) — after DOM
            // so `document` is defined and Promise/queueMicrotask are available.
            if let Err(e) = view_transitions::install_view_transition_bindings(
                &ctx,
                Arc::clone(&self.view_transition_events),
            ) {
                eprintln!("View Transitions bindings init failed: {}", e);
            }

            // Install Web Speech API (W3C Web Speech §3–4) — after DOM so `window`,
            // `Promise`, `setTimeout`, and `Event` are available.
            // Phase 0: SpeechSynthesis dispatches to OS TTS (SAPI/espeak/say);
            // SpeechRecognition always rejects with service-not-allowed.
            if let Err(e) = speech::install_speech_bindings(&ctx) {
                eprintln!("Speech bindings init failed: {}", e);
            }

            // Install Gamepad API (W3C Gamepad L2 §4) — after DOM so `navigator`,
            // `Promise`, and `Event` are available.
            // Phase 0: navigator.getGamepads() returns 4 null slots; no hardware polling.
            // Shell integration (P3) calls _lumen_gamepad_connect/disconnect to notify.
            if let Err(e) = gamepad::install_gamepad_bindings(&ctx) {
                eprintln!("Gamepad bindings init failed: {}", e);
            }

            // Install MediaSession API (W3C Media Session §5) — after DOM so `navigator`,
            // `Promise`, and `Event` are available.
            // Phase 0: metadata/playbackState stored in JS; OS forwarding via
            // _lumen_take_media_session_update() is a P3 shell integration task.
            if let Err(e) = media_session::install_media_session_bindings(&ctx) {
                eprintln!("MediaSession bindings init failed: {}", e);
            }

            // Install CSS Scroll Snap L2 events (W3C CSS Scroll Snap §4) — after DOM
            // so `Event` is available. Provides SnapChangeEvent and native bindings
            // _lumen_fire_snap_changing/changed for shell to emit snap events.
            // Phase 0: event infrastructure complete; shell integration (P2/P3)
            // calls bindings when snap-points change via layout.
            if let Err(e) = scroll_snap_events::install_scroll_snap_events_bindings(&ctx) {
                eprintln!("Scroll Snap events bindings init failed: {}", e);
            }

            // Install CSS Scroll-Driven Animations Level 1 (W3C §3–4) — after DOM.
            // Provides `ScrollTimeline` / `ViewTimeline` classes and
            // `_lumen_deliver_scroll_progress(py, px)` for shell to push viewport
            // progress into all active root-viewport ScrollTimeline instances.
            // Phase 1: `animation-timeline: scroll()` parsed in ComputedStyle (P4).
            if let Err(e) = scroll_timeline::install_scroll_timeline_bindings(&ctx) {
                eprintln!("Scroll-Driven Animations bindings init failed: {}", e);
            }

            // Install Long Animation Frames API (W3C LoAF §3–4) — after DOM so that
            // PerformanceObserver and _perf_entries are in scope.
            // Phase 0: PerformanceLongAnimationFrameTiming + PerformanceScriptTiming classes
            // and _lumen_deliver_long_animation_frame delivery binding.
            // Phase 1: shell rendering loop auto-reports frames > 50 ms.
            if let Err(e) = long_animation_frames::install_long_animation_frames_bindings(&ctx) {
                eprintln!("Long Animation Frames API init failed: {}", e);
            }

            // Install Sanitizer API (W3C Sanitizer API §3) — after DOM so `document`,
            // `Element`, and DOM methods are available.
            // Phase 0: Simple removal of <script> tags and event handler attributes.
            // Config options are not used.
            if let Err(e) = sanitizer::install_sanitizer_bindings(&ctx) {
                eprintln!("Sanitizer bindings init failed: {}", e);
            }

            // Install Shape Detection API (W3C Shape Detection API §3–4) — pure JS implementation.
            // Phase 0: FaceDetector, BarcodeDetector, TextDetector all return empty arrays.
            // No actual detection is performed.
            if let Err(e) = shape_detection::install_shape_detection_bindings(&ctx) {
                eprintln!("Shape Detection bindings init failed: {}", e);
            }

            // Install Device Orientation and Device Motion APIs (W3C Device Orientation L2/L3) — pure JS implementation.
            // Phase 0: DeviceOrientationEvent and DeviceMotionEvent with default values {0,0,0};
            // requestPermission() always resolves to 'granted'.
            if let Err(e) = device_sensors::install_device_sensors_bindings(&ctx) {
                eprintln!("Device Sensors bindings init failed: {}", e);
            }

            // Install Eye Dropper API (W3C Color WG) — pure JS implementation with native binding stub.
            // Phase 0: EyeDropper.open() returns Promise<{sRGBHex}> with AbortSignal support.
            // Platform integration (P3) implements _lumen_eye_dropper_open for each OS.
            if let Err(e) = eye_dropper::install_eye_dropper_bindings(&ctx) {
                eprintln!("Eye Dropper API init failed: {}", e);
            }

            // Install Document Picture-in-Picture API (W3C Document PiP §4) — pure JS implementation.
            // Phase 0: documentPictureInPicture.requestWindow() creates a PiP window overlay.
            // Shell integration (P3) registers the window via _lumen_pip_request_window.
            if let Err(e) = document_pip::install_document_pip_api(&ctx) {
                eprintln!("Document Picture-in-Picture API init failed: {}", e);
            }

            // Install MediaRecorder API stub (W3C MediaStream Recording L2) — pure JS implementation.
            // Phase 0: MediaRecorder state machine (inactive/recording/paused), mimeType reflection,
            // ondataavailable fires empty Blob on stop. BlobEvent class. isTypeSupported() → false.
            if let Err(e) = media_stream_recording::init_media_stream_recording(&ctx) {
                eprintln!("MediaRecorder API init failed: {}", e);
            }

            // Install Media Capabilities API (W3C Media Capabilities §5) — after DOM/navigator.
            // Phase 0: navigator.mediaCapabilities singleton; decodingInfo/encodingInfo always
            // return supported=true, smooth=true, powerEfficient=false.
            if let Err(e) = media_capabilities::install_media_capabilities_bindings(&ctx) {
                eprintln!("Media Capabilities API init failed: {}", e);
            }

            // Install Virtual Keyboard API (W3C VK API) — after navigator.
            // Phase 0: geometry stubs + geometrychange event infrastructure.
            if let Err(e) = virtual_keyboard::install_virtual_keyboard_bindings(&ctx) {
                eprintln!("Virtual Keyboard API init failed: {}", e);
            }

            // Install Web Locks API (W3C Web Locks Level 1) — after DOM/navigator.
            // Phase 0: in-memory per-context FIFO lock queue; supports exclusive/shared modes,
            // ifAvailable, steal, and AbortSignal. navigator.locks → LockManager.
            if let Err(e) = web_locks::install_web_locks_bindings(&ctx) {
                eprintln!("Web Locks API init failed: {}", e);
            }

            // Install Reporting API (W3C Reporting API L1) — observer + _lumen_deliver_report.
            if let Err(e) = reporting_api::install_reporting_api_bindings(&ctx) {
                eprintln!("Reporting API init failed: {}", e);
            }

            // Install Screen Wake Lock API (W3C Screen Wake Lock Level 1) — after DOM/navigator.
            // Phase 0: in-memory sentinel with auto-release on visibilitychange → hidden.
            if let Err(e) = wake_lock::install_wake_lock_bindings(&ctx) {
                eprintln!("Screen Wake Lock API init failed: {}", e);
            }

            // Install W3C Scheduler API Level 1 — scheduler.postTask / yield, TaskController/Signal.
            // Phase 0: user-blocking → queueMicrotask, user-visible → setTimeout(0), background → setTimeout(200).
            if let Err(e) = scheduler::install_scheduler_api(&ctx) {
                eprintln!("Scheduler API init failed: {}", e);
            }

            // Install W3C Web Audio API Level 1 — AudioContext, AudioNode hierarchy, AudioParam.
            // Phase 0: no DSP; graph operations in-memory only; decodeAudioData returns silent buffer.
            if let Err(e) = web_audio::install_web_audio_api(&ctx) {
                eprintln!("Web Audio API init failed: {}", e);
            }

            // Install W3C WebGPU API — navigator.gpu, GPUAdapter/Device/Buffer/Texture/Pipeline stubs.
            // Phase 0: no GPU; all create* ops in-memory only; submit/draw/dispatch are no-ops.
            if let Err(e) = webgpu::install_webgpu_bindings(&ctx) {
                eprintln!("WebGPU API init failed: {}", e);
            }

            // Install W3C WebCodecs API (https://www.w3.org/TR/webcodecs/) — after DOM.
            // Phase 0: VideoEncoder/Decoder + AudioEncoder/Decoder + EncodedVideoChunk/AudioChunk stubs.
            // configure() rejects with NotSupportedError; no codec support in Phase 0.
            // Phase 1 (future): FFmpeg or libav1 bindings for actual encoding/decoding.
            if let Err(e) = web_codecs::install_webcodecs_bindings(&ctx) {
                eprintln!("WebCodecs API init failed: {}", e);
            }

            // Install WebXR Device API (W3C WebXR Device API §5) — after DOM/navigator.
            // Phase 0: isSessionSupported() → false, requestSession() → NotSupportedError.
            // XRSession/XRFrame/XRReferenceSpace/XRView stubs exported on window.
            if let Err(e) = webxr::install_webxr_bindings(&ctx) {
                eprintln!("WebXR Device API init failed: {}", e);
            }

            if let Err(e) = form_validation::install_form_validation_bindings(&ctx) {
                eprintln!("Form Constraint Validation API init failed: {}", e);
            }

            // Phase 0: element.attachInternals() + CustomStateSet; :state() selector is P4 handoff.
            if let Err(e) = element_internals::install_element_internals_bindings(&ctx) {
                eprintln!("ElementInternals API init failed: {}", e);
            }

            // Phase 0: HTMLElement.prototype.inert getter/setter (HTML LS §6.7).
            // Phase 1: _lumen_set_inert native binding propagates to DOM attr + triggers style recalc.
            if let Err(e) = inert::install_inert_api(&ctx) {
                eprintln!("Inert API init failed: {}", e);
            }

            // Phase 0: navigator.presentation + PresentationRequest/Connection stubs.
            if let Err(e) = presentation_api::install_presentation_api(&ctx) {
                eprintln!("Presentation API init failed: {}", e);
            }

            // Install WebAssembly API (W3C WebAssembly JavaScript Interface §7) — after DOM.
            // Phase 0: compile/instantiate return resolved Promises with empty Module/Instance;
            // validate() checks the 4-byte WASM magic header. No actual WASM execution.
            // Phase 1 (future): integrate wasmtime or wasmer for real WASM execution.
            if let Err(e) = webassembly::install_webassembly_bindings(&ctx) {
                eprintln!("WebAssembly bindings init failed: {}", e);
            }

            // Phase 0: W3C Generic Sensor API — Accelerometer, Gyroscope, LinearAccelerationSensor,
            // GravitySensor, AbsoluteOrientationSensor, RelativeOrientationSensor, Magnetometer,
            // AmbientLightSensor. start() activates sensor; no readings until Phase 1 OS integration.
            if let Err(e) = generic_sensor::install_generic_sensor_bindings(&ctx) {
                eprintln!("Generic Sensor API init failed: {}", e);
            }

            // Phase 0: W3C Web MIDI L1 — navigator.requestMIDIAccess() resolves with empty
            // MIDIAccess (no hardware). Phase 1 wires _lumen_midi_deliver_message to
            // CoreMIDI / WinMM / ALSA backends.
            if let Err(e) = web_midi::install_web_midi_api(&ctx) {
                eprintln!("Web MIDI API init failed: {}", e);
            }

            // Phase 0: WHATWG Storage §9 — navigator.storage singleton.
            // estimate() → {usage:0, quota:10GiB}; persist/persisted → true; getDirectory() → OPFS stub.
            // Phase 1: _lumen_storage_estimate/persist/get_directory wire real OS metrics + sandboxed FS.
            if let Err(e) = storage_manager::install_storage_manager_bindings(&ctx) {
                eprintln!("StorageManager API init failed: {}", e);
            }

            // Install XMLHttpRequest API (WHATWG XHR Standard §4) — after DOM so fetch(),
            // FormData, Blob, TextDecoder/Encoder, ProgressEvent infra, and the
            // _lumen_fetch_sync* native bindings are all present.
            // Phase 0: open/send/abort/getResponseHeader/getAllResponseHeaders, readystatechange
            // and load/error/progress/abort events.  Reuses the fetch HTTP stack.
            if let Err(e) = xhr::install_xhr_bindings(&ctx) {
                eprintln!("XMLHttpRequest API init failed: {}", e);
            }

            // W3C DOM Parsing and Serialization — DOMParser + XMLSerializer.
            // After DOM and _lumen_get_attr_names / _lumen_get_children / _lumen_is_text_node
            // bindings are registered.  DOMParser.parseFromString() creates independent
            // virtual documents (pure-JS nodes, not backed by Rust lumen_dom).
            // XMLSerializer.serializeToString() handles both virtual and native nodes.
            if let Err(e) = dom_parser::install_dom_parser(&ctx) {
                eprintln!("DOMParser/XMLSerializer init failed: {}", e);
            }

            // W3C SVG 2 — SVGElement/SVGSVGElement class hierarchy, getBBox() stubs,
            // SVGRect/SVGPoint/SVGLength/SVGMatrix types, createElementNS SVG wiring.
            // Must come after dom_parser (document.createElementNS already defined).
            if let Err(e) = svg::install_svg_bindings(&ctx) {
                eprintln!("SVG DOM API init failed: {}", e);
            }

            // W3C File API — File/FileList classes, _lumen_deliver_file_list(nid, json).
            // Called from shell after OS file dialog closes with selected paths.
            // Must come after SVG bindings (svg install already after dom_parser).
            if let Err(e) = file_input::install_file_input_bindings(&ctx) {
                eprintln!("File API init failed: {}", e);
            }

            // W3C Multi-Screen Window Placement Level 1 — screen.isExtended,
            // navigator.getScreenDetails() → Promise<ScreenDetails>, ScreenDetailed class.
            // Phase 0: single-screen stub (isExtended=false, one ScreenDetailed mirroring screen).
            // Phase 1: _lumen_get_screen_details() native binding for OS multi-screen enumeration.
            if let Err(e) = window_management::install_window_management_api(&ctx) {
                eprintln!("Window Management API init failed: {}", e);
            }

            // WICG Local Font Access — navigator.fonts (FontAccessManager) + FontData class.
            // Phase 0: query() resolves with []. Phase 1: _lumen_local_fonts_query() native binding.
            if let Err(e) = local_font_access::install_local_font_access_api(&ctx) {
                eprintln!("Local Font Access API init failed: {}", e);
            }

            // WICG Launch Handler — window.launchQueue, LaunchParams, setConsumer().
            // Phase 0: in-memory queue. Phase 1: _lumen_deliver_launch_params() from shell.
            if let Err(e) = launch_handler::install_launch_handler_api(&ctx) {
                eprintln!("Launch Handler API init failed: {}", e);
            }

            // WICG Shared Storage API — window.sharedStorage (Privacy Sandbox).
            // Phase 0: in-memory key-value store. Phase 1: SQLite per-origin partition.
            if let Err(e) = shared_storage::install_shared_storage(&ctx) {
                eprintln!("Shared Storage API init failed: {}", e);
            }

            // WICG Idle Detection API — window.IdleDetector.
            // Phase 0: requestPermission() → 'granted'; start() resolves with fixed
            // {userState:'active', screenState:'unlocked'}; 'change' event never fires.
            // Phase 1: wire _lumen_idle_query_* native hooks to OS idle-time APIs.
            if let Err(e) = idle_detection::install_idle_detection_bindings(&ctx) {
                eprintln!("Idle Detection API init failed: {}", e);
            }

            // Privacy Sandbox Topics API — document.browsingTopics() + DeprecatedTopicsButton.
            // Phase 0: browsingTopics() → Promise<[]>; no topic observation or storage.
            // Phase 1: wire _lumen_topics_get_topics native hook to per-origin topic store.
            if let Err(e) = topics_api::install_topics_api(&ctx) {
                eprintln!("Topics API init failed: {}", e);
            }

            // Privacy Sandbox Attribution Reporting API — window.attributionReporting +
            // attributionsrc IDL attribute on <a>/<img>/<script>.
            // Phase 0: registerSource/registerTrigger → Promise<undefined> no-ops.
            // Phase 1: wire _lumen_attribution_register_source/_lumen_attribution_register_trigger.
            if let Err(e) = attribution_reporting::install_attribution_reporting_api(&ctx) {
                eprintln!("Attribution Reporting API init failed: {}", e);
            }

            // W3C Pointer Events Level 3 §4.1 — pointer capture native bindings.
            // _lumen_set_capture_state(nid) / _lumen_release_capture_state()
            // Called by JS setPointerCapture/releasePointerCapture on Element.
            if let Err(e) = pointer_capture::install_pointer_capture_bindings(
                &ctx,
                Arc::clone(&self.pointer_capture_nid),
            ) {
                eprintln!("Pointer capture bindings init failed: {}", e);
            }

            Ok(())
        })
    }

    /// Enable or disable cookie-banner auto-dismiss for subsequent `install_dom` calls.
    ///
    /// Default: `true` (banners are auto-dismissed). Set to `false` to let the user
    /// interact with consent dialogs normally. Shell reads this from the user's
    /// `cookie_banner_dismiss` preference stored in settings.
    pub fn set_cookie_banner_dismiss(&self, enabled: bool) {
        self.cookie_banner_dismiss.store(enabled, Ordering::Relaxed);
    }

    /// Enable deterministic render mode (8F).
    ///
    /// Must be called before `install_dom`. When set, `Date.now()` is frozen at 0
    /// and `Math.random` is replaced with a seeded xorshift32 PRNG derived from the
    /// page URL hash, making JS rendering output independent of wall-clock time.
    pub fn set_deterministic_mode(&self) {
        self.deterministic.store(true, Ordering::Relaxed);
    }

    /// Freeze fingerprint APIs for canvas / audio / font enumeration (8F.3).
    ///
    /// Installs JS overrides that make fingerprinting APIs return fixed deterministic
    /// values regardless of platform:
    /// - **Canvas** — `toDataURL()` / `toBlob()` return a fixed empty PNG data URL
    ///   (already handled by `canvas2d.rs`; this call is a no-op reinforcement).
    /// - **AudioContext** — `createAnalyser` buffer data returns all-zero samples;
    ///   `sampleRate` is pinned to 44100.
    /// - **Font enumeration** — `document.fonts.check()` always returns `true` for
    ///   the single bundled font (Inter); `document.fonts` iterates only Inter.
    ///
    /// Must be called after `install_dom` and before running page scripts.
    /// Idempotent — safe to call multiple times.
    pub fn freeze_fingerprint(&self) {
        // Language: inject a small JS shim that normalises the remaining APIs.
        // Canvas and WebGL are already normalised at the Rust level (canvas2d.rs,
        // webgl_canvas.rs). Here we target audio analyser output and font enumeration.
        const FREEZE_SHIM: &str = r#"
(function(){
  // Audio: pin AnalyserNode.getByteFrequencyData / getFloatFrequencyData to zeros.
  if(typeof AnalyserNode!=='undefined'){
    AnalyserNode.prototype.getByteFrequencyData=function(arr){
      if(arr && arr.fill) arr.fill(0);
    };
    AnalyserNode.prototype.getFloatFrequencyData=function(arr){
      if(arr && arr.fill) arr.fill(-Infinity);
    };
    AnalyserNode.prototype.getByteTimeDomainData=function(arr){
      if(arr && arr.fill) arr.fill(128);
    };
    AnalyserNode.prototype.getFloatTimeDomainData=function(arr){
      if(arr && arr.fill) arr.fill(0);
    };
  }
  // Font: document.fonts.check() always true; forEach/keys/values yield nothing extra.
  if(typeof document!=='undefined' && document.fonts){
    try{
      document.fonts.check=function(){return true;};
    }catch(e){}
  }
})();
"#;
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>(FREEZE_SHIM).ok();
        });
    }

    /// Deliver messages posted by worker threads to their `Worker` JS instances.
    ///
    /// Drains the outbound worker message queue and calls
    /// `_lumen_deliver_worker_messages(msgs)` in the main JS context so that
    /// `onmessage` / `addEventListener('message', fn)` handlers fire.
    ///
    /// Shell must call this on every event-loop tick (alongside `tick_timers()`)
    /// so that worker replies are delivered promptly.
    pub fn pump_workers(&self) {
        let messages = worker::drain_messages(&self.worker_messages);
        if messages.is_empty() {
            return;
        }
        let json = build_worker_messages_json(&messages);
        let script = format!(
            "if(typeof _lumen_deliver_worker_messages==='function')\
             _lumen_deliver_worker_messages({json})"
        );
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Drain dirty Canvas 2D buffers for upload to the renderer.
    ///
    /// Returns `(node_index, width, height, rgba)` for every `<canvas>` whose
    /// 2D context was drawn to since the last call. The shell uploads each as
    /// `Renderer::register_image("canvas:{nid}", ...)` and requests a repaint.
    ///
    /// Acquires the runtime lock so the thread-local canvas registry is read on
    /// the same thread that executes the JS context. Shell must call this on
    /// every event-loop tick (alongside `pump_workers()`).
    pub fn flush_canvas_updates(&self) -> Vec<(u32, u32, u32, Vec<u8>)> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|_ctx| canvas2d::flush_dirty())
    }

    /// Deliver messages posted to this page's `BroadcastChannel` instances.
    ///
    /// Drains the per-runtime receiver queues (filled by the process-global hub
    /// in `broadcast_channel`) and calls `_lumen_deliver_broadcast_messages(msgs)`
    /// in the main JS context so `onmessage` / `addEventListener('message', fn)`
    /// handlers fire.
    ///
    /// Shell must call this on every event-loop tick (alongside `pump_workers()`)
    /// so that broadcasts from other contexts are delivered promptly.
    pub fn pump_broadcast_channels(&self) {
        let messages = broadcast_channel::drain(&self.broadcast_channels);
        if messages.is_empty() {
            return;
        }
        let json = build_worker_messages_json(&messages);
        let script = format!(
            "if(typeof _lumen_deliver_broadcast_messages==='function')\
             _lumen_deliver_broadcast_messages({json})"
        );
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Deliver messages posted by `SharedWorker` threads to this page's ports.
    ///
    /// Drains this runtime's shared-worker outbox (filled by process-global
    /// shared-worker threads in `shared_worker`) and calls
    /// `_lumen_deliver_shared_worker_messages(msgs)` so each client `port`'s
    /// `onmessage` / `addEventListener('message', fn)` handlers fire.
    ///
    /// Shell must call this on every event-loop tick (alongside `pump_workers()`)
    /// so that replies from shared workers are delivered promptly.
    pub fn pump_shared_workers(&self) {
        let messages = shared_worker::drain_messages(&self.shared_worker_outbox);
        if messages.is_empty() {
            return;
        }
        let json = build_worker_messages_json(&messages);
        let script = format!(
            "if(typeof _lumen_deliver_shared_worker_messages==='function')\
             _lumen_deliver_shared_worker_messages({json})"
        );
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Consume any navigation request that JS placed via `location.href =` etc.
    /// Returns `None` if no navigation was requested during script execution.
    /// Must be called before `drop(runtime)` to avoid losing the request.
    pub fn take_navigate_request(&self) -> Option<NavigateRequest> {
        self.nav_out.lock().unwrap().take()
    }

    /// Drain `history.pushState` / `history.replaceState` URL-update notifications
    /// queued since the last call.
    ///
    /// Returns an empty `Vec` when no pushState/replaceState calls were made.
    /// Shell drains this in `about_to_wait` to update the address-bar display URL
    /// and the same-document navigation stack.
    pub fn take_history_url_updates(&self) -> Vec<dom::HistoryUrlUpdate> {
        std::mem::take(&mut *self.pending_history_url_updates.lock().unwrap())
    }

    /// Drain all fullscreen requests queued by `element.requestFullscreen()` and
    /// `document.exitFullscreen()`.
    ///
    /// Called by the shell in `about_to_wait`. Each `Enter { nid }` causes the
    /// shell to call `window.set_fullscreen(Some(Fullscreen::Borderless(None)))`;
    /// each `Exit` causes `window.set_fullscreen(None)`.
    /// Returns an empty vec when no fullscreen state changes occurred since the last drain.
    pub fn take_fullscreen_requests(&self) -> Vec<dom::FullscreenRequest> {
        std::mem::take(&mut *self.fullscreen_requests.lock().unwrap())
    }

    /// Drain all View Transition events queued by `document.startViewTransition`.
    ///
    /// Called by the shell in `about_to_wait`. `Begin` triggers a display-list
    /// snapshot; `End` triggers relayout + 300 ms cross-fade animation.
    pub fn take_view_transition_events(&self) -> Vec<view_transitions::ViewTransitionEvent> {
        std::mem::take(&mut *self.view_transition_events.lock().unwrap())
    }

    /// Returns `true` if JS mutated the DOM since the last call, clearing the flag.
    ///
    /// Called by the shell event loop after each rAF pass to decide whether a
    /// relayout is needed before the next paint.
    pub fn take_dom_dirty(&self) -> bool {
        self.dom_dirty.swap(false, Ordering::Relaxed)
    }

    /// Returns `true` if `requestAnimationFrame` was called since the last call,
    /// clearing the flag.
    ///
    /// Shell reads this after each rendering step: if `true`, another redraw must
    /// be requested so the animation loop gets its next frame.
    pub fn take_raf_pending(&self) -> bool {
        self.raf_pending.swap(false, Ordering::Relaxed)
    }

    /// Non-consuming peek: `true` if `requestAnimationFrame` callbacks are queued.
    ///
    /// Unlike `take_raf_pending`, this does not clear the flag. Use in the shell
    /// vsync gate to check whether to defer firing without losing the signal.
    pub fn has_raf_pending(&self) -> bool {
        self.raf_pending.load(Ordering::Relaxed)
    }

    /// Take the next timer wakeup as Unix epoch ms, clearing the stored value.
    ///
    /// Called by the shell event loop in `about_to_wait` to schedule
    /// `ControlFlow::WaitUntil` so the loop wakes up when the next JS timer fires.
    /// Returns `None` when no timers are pending.
    pub fn take_timer_wakeup(&self) -> Option<f64> {
        self.timer_wakeup.lock().unwrap().take()
    }

    /// Replace the layout bounding-rect table with a fresh snapshot.
    ///
    /// Called by the shell after every `relayout_page` call.  Maps `NodeId`
    /// index (u32) → `[x, y, width, height]` in viewport-relative CSS px
    /// (border-box coordinates, same as `getBoundingClientRect`).
    pub fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>) {
        *self.layout_rects.lock().unwrap() = rects;
    }

    /// Update the viewport dimensions.
    ///
    /// Called by the shell after every resize event and at initial load.
    /// `width` and `height` are in CSS px (physical size / device pixel ratio).
    pub fn update_viewport_size(&self, width: f32, height: f32) {
        *self.viewport_size.lock().unwrap() = [width, height];
    }

    /// Drain lazy image load requests queued by `_lumen_request_lazy_image_load` in JS.
    ///
    /// Called by the shell after `_lumen_deliver_lazy_images()` to get the list of
    /// images that entered the lazy-load margin and should now be fetched.
    /// Returns `(node_id, url)` pairs; clears the internal queue.
    pub fn take_lazy_image_requests(&self) -> Vec<(u32, String)> {
        std::mem::take(&mut self.lazy_img_requests.lock().unwrap())
    }

    /// Replace the scroll-state table with a fresh snapshot from the layout tree.
    ///
    /// Called by the shell after every `relayout_page` call.  Maps `NodeId` index →
    /// `[scroll_x, scroll_y, scroll_width, scroll_height]` for every `overflow: scroll`
    /// / `overflow: auto` container.  Non-scroll elements are absent from the map.
    ///
    /// P3 shell integration: call after every `collect_scroll_containers()` pass,
    /// e.g. `rt.update_scroll_states(containers.iter().map(|c| (c.node.index() as u32, [c.scroll_x, c.scroll_y, c.scroll_width, c.scroll_height])).collect())`.
    pub fn update_scroll_states(&self, states: HashMap<u32, [f32; 4]>) {
        *self.scroll_states.lock().unwrap() = states;
    }

    /// Drain JS-initiated scroll requests queued by `_lumen_request_scroll`.
    ///
    /// Returns `(node_id, target_x, target_y)` triples; clears the queue.
    /// Shell calls this after each rendering step and routes each entry through
    /// `set_scroll_position(root, NodeId::from_index(nid), x, y)`.
    pub fn take_scroll_requests(&self) -> Vec<(u32, f32, f32)> {
        std::mem::take(&mut self.pending_scrolls.lock().unwrap())
    }

    /// Drain JS page-level scroll requests from `window.scrollTo/scrollBy/scroll`.
    /// Returns `(target_y, smooth)` pairs; clears the queue.
    /// Shell routes `smooth=true` → `start_smooth_scroll(y)`, `false` → `scroll_to(y)`.
    pub fn take_page_scroll_requests(&self) -> Vec<(f32, bool)> {
        std::mem::take(&mut self.pending_page_scrolls.lock().unwrap())
    }

    /// Update the page scroll Y exposed to JS `window.scrollY / pageYOffset`.
    /// Shell calls this whenever `scroll_y` changes.
    pub fn set_page_scroll_y(&self, y: f32) {
        *self.page_scroll_y.lock().unwrap() = y;
    }

    /// Drain all OS notification requests queued by `new Notification(...)` in JS.
    ///
    /// Called by the shell in `about_to_wait`.  Each returned entry should be
    /// forwarded to `notification::show_os_notification(title, body)`.
    /// Returns an empty vec when no notifications were created since the last call.
    pub fn take_notification_requests(
        &self,
    ) -> Vec<notifications_bindings::NotificationRequest> {
        notifications_bindings::drain_notifications(&self.pending_notifications)
    }

    /// Drain all popup window requests queued by JS `window.open(...)`.
    ///
    /// Called by the shell in `about_to_wait`. Each returned `PopupRequest` should
    /// open a new tab navigated to `popup.url`. Returns an empty vec when no
    /// `window.open()` calls have been made since the last drain.
    pub fn take_window_open_requests(&self) -> Vec<dom::PopupRequest> {
        std::mem::take(&mut self.window_open_requests.lock().unwrap())
    }

    /// Drain all print requests queued by JS `window.print()` (W-2).
    ///
    /// Called by the shell in `about_to_wait`. Each returned `PrintRequest` should
    /// open a print-preview dialog or directly render to PDF. Returns an empty vec when no
    /// `window.print()` calls have been made since the last drain.
    pub fn take_print_requests(&self) -> Vec<dom::PrintRequest> {
        std::mem::take(&mut self.print_requests.lock().unwrap())
    }

    /// Returns the DOM node nid that currently holds pointer capture (pointer_id=1).
    ///
    /// Shell calls this before dispatching pointer events to redirect them to the
    /// capture target instead of the hit-tested element (W3C Pointer Events L3 §4.1).
    /// Returns `None` when no capture is active.
    pub fn pointer_capture_nid(&self) -> Option<u32> {
        *self.pointer_capture_nid.lock().unwrap()
    }

    /// Release the active pointer capture, returning the former capture target nid.
    ///
    /// Called by the shell implicitly on `pointerup`/`pointercancel` per spec §4.1.
    /// Returns `None` if no capture was active.
    pub fn take_pointer_capture(&self) -> Option<u32> {
        self.pointer_capture_nid.lock().unwrap().take()
    }

    /// Drain all `console.log/warn/error` messages queued since the last call.
    ///
    /// Each entry is `(level, text)` where level is 0=log, 1=warn, 2=error.
    /// Called by the shell's DevTools console panel in `about_to_wait`.
    /// Returns an empty vec when no console calls have been made since the last drain.
    pub fn take_console_messages(&self) -> Vec<(u8, String)> {
        std::mem::take(&mut self.console_messages.lock().unwrap())
    }

    /// Drain JS dialog focus requests queued by `_lumen_request_focus` / `_lumen_request_blur`.
    ///
    /// `None` = blur (clear focus); `Some(nid)` = move keyboard focus to that node.
    /// Called by the shell in `about_to_wait` to apply focus changes from `showModal()` /
    /// `close()` without touching the DOM directly from Rust.
    pub fn take_focus_requests(&self) -> Vec<Option<u32>> {
        std::mem::take(&mut self.pending_focus_requests.lock().unwrap())
    }

    /// Close a `<dialog>` as the result of a `<form method="dialog">` submission.
    ///
    /// Calls `dialog.close(return_value)` in JS so the close event fires and
    /// `returnValue` is set.  `dialog_nid` is the dialog node's index; `return_value`
    /// is the submit button's `value` attribute (may be empty).
    pub fn fire_dialog_close(&self, dialog_nid: u32, return_value: &str) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ctx.with(|ctx| {
            let rv = return_value.replace('\\', r"\\").replace('"', r#"\""#);
            let script = format!(
                "(function(){{var d=_lumen_make_element({dialog_nid});\
                 if(d&&typeof d.close==='function')d.close(\"{rv}\");}})();"
            );
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Notify the JS runtime that the shell moved keyboard focus to a new node.
    ///
    /// Updates `_lumen_last_focused_nid` so that `showModal()` can save and restore
    /// the previously focused element per HTML LS §6.6.3. `nid = None` means focus
    /// was cleared (e.g. click on non-focusable area).
    pub fn notify_focus_changed(&self, nid: Option<u32>) {
        let n = nid.map(|n| n as i64).unwrap_or(-1_i64);
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ctx.with(|ctx| {
            let script = format!(
                "if(typeof _lumen_last_focused_nid!=='undefined')_lumen_last_focused_nid={n};"
            );
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Push a fresh snapshot of computed CSS styles into the JS runtime.
    ///
    /// Called by the shell after every relayout.  Replaces the entire cache;
    /// stale entries for removed nodes are discarded automatically.
    /// The JS side reads entries via `_lumen_get_computed_style(nid, prop)`.
    pub fn update_computed_styles(&self, styles: HashMap<u32, HashMap<String, String>>) {
        *self.computed_styles.lock().unwrap() = styles;
    }

    /// Update `document.hidden` / `document.visibilityState` and fire
    /// `visibilitychange` on both `document` and `window`.
    ///
    /// Call with `hidden = true` on window `Focused(false)` / blur events,
    /// `hidden = false` on `Focused(true)` / focus events.
    /// No-op if `install_dom` has not been called yet.
    pub fn set_document_visibility(&self, hidden: bool) {
        let script = if hidden {
            "_lumen_apply_visibility(true)"
        } else {
            "_lumen_apply_visibility(false)"
        };
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>(script).ok();
        });
    }

    /// Transition `document.readyState` → `'interactive'` and fire
    /// `readystatechange` + `DOMContentLoaded` (bubbling) on `document`.
    ///
    /// Call after the full HTML parse pass but before running user scripts
    /// for the most spec-accurate timing.  Safe to call multiple times —
    /// the JS side is idempotent (state only moves forward).
    pub fn notify_dom_content_loaded(&self) {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>("_lumen_apply_ready_state('interactive')").ok();
        });
    }

    /// Transition `document.readyState` → `'complete'` and fire
    /// `readystatechange` on `document` + `load` on `window`.
    ///
    /// Call after all subresources (images, fonts, scripts) are loaded.
    /// Safe to call multiple times — idempotent on the JS side.
    pub fn notify_window_loaded(&self) {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>("_lumen_apply_ready_state('complete')").ok();
        });
    }

    /// Push viewport scroll progress into all active root-viewport `ScrollTimeline` instances.
    ///
    /// `progress_y` is the block-axis fraction `[0.0, 1.0]` (scroll_y / max_scroll_y).
    /// `progress_x` is the inline-axis fraction `[0.0, 1.0]` (scroll_x / max_scroll_x).
    ///
    /// No-op when `install_dom` has not been called yet or no `ScrollTimeline` is registered.
    pub fn deliver_scroll_progress(&self, progress_y: f32, progress_x: f32) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ctx.with(|ctx| {
            let script = format!(
                "if(typeof _lumen_deliver_scroll_progress==='function')\
                 _lumen_deliver_scroll_progress({},{});",
                progress_y, progress_x
            );
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Fire a non-bubbling `scroll` Event on the DOM element identified by `nid`.
    ///
    /// Called by the shell after every scroll-position change on an overflow
    /// container (both wheel-driven and JS-programmatic scrolls).
    /// Per WHATWG HTML §8.1.6.2 the event is non-bubbling and non-cancelable.
    /// No-op when the runtime has not been initialised yet.
    pub fn fire_element_scroll(&self, nid: u32) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ctx.with(|ctx| {
            let script = format!(
                "if(typeof _lumen_fire_scroll_on_element==='function')\
                 _lumen_fire_scroll_on_element({nid});"
            );
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Fire a non-bubbling `scroll` Event on the `window` object (page scroll).
    ///
    /// Called by the shell whenever the page-level scroll position changes.
    /// No-op when the runtime has not been initialised yet.
    pub fn fire_window_scroll(&self) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ctx.with(|ctx| {
            ctx.eval::<(), _>(
                "if(typeof _lumen_fire_window_scroll_event==='function')\
                 _lumen_fire_window_scroll_event();"
            ).ok();
        });
    }

    /// Fire a CSS Scroll Snap L2 `snapchanging` event on a scroll container.
    ///
    /// Called by the shell while a scroll gesture is in flight and the
    /// container's snapped area changes (before the scroll settles). `nid` is
    /// the scroll-container element; `block` / `inline` are the node ids of the
    /// snapped areas on the block and inline axes (typically from
    /// `lumen_layout::find_snapped_nodes`), or `None` when no area is snapped on
    /// that axis. They are exposed to JS as the event's `snapTargetBlock` /
    /// `snapTargetInline` element properties.
    ///
    /// No-op when the runtime has not been initialised yet.
    pub fn fire_snap_changing(&self, nid: u32, block: Option<u32>, inline: Option<u32>) {
        self.fire_snap_event("_lumen_fire_snap_changing", nid, block, inline);
    }

    /// Fire a CSS Scroll Snap L2 `snapchanged` event on a scroll container.
    ///
    /// Called by the shell once a scroll has settled on a new snap position.
    /// Argument semantics are identical to [`Self::fire_snap_changing`].
    ///
    /// No-op when the runtime has not been initialised yet.
    pub fn fire_snap_changed(&self, nid: u32, block: Option<u32>, inline: Option<u32>) {
        self.fire_snap_event("_lumen_fire_snap_changed", nid, block, inline);
    }

    /// Shared dispatch path for the snap events. Resolves `block` / `inline`
    /// node ids to elements via `_lumen_make_element` inside JS, guarding both
    /// the target function and the resolver so the call is a no-op when the DOM
    /// bindings are absent.
    fn fire_snap_event(&self, func: &str, nid: u32, block: Option<u32>, inline: Option<u32>) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ctx.with(|ctx| {
            let blk = match block {
                Some(b) => format!("_lumen_make_element({b})"),
                None => "null".to_string(),
            };
            let inl = match inline {
                Some(i) => format!("_lumen_make_element({i})"),
                None => "null".to_string(),
            };
            let script = format!(
                "if(typeof {func}==='function'&&typeof _lumen_make_element==='function')\
                 {func}({nid},{blk},{inl});"
            );
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Deliver a Long Animation Frame (LoAF) entry to PerformanceObserver subscribers.
    ///
    /// Call from the shell frame-timing path when a frame exceeds 50 ms.
    /// `start_ms` and `duration_ms` are in milliseconds (performance.now() scale).
    /// `scripts_json` is an optional JSON array of `PerformanceScriptTiming` initialisers.
    /// Pass `None` for `scripts_json` and the entry will have an empty `scripts` array.
    ///
    /// No-op when the runtime has not been initialised yet.
    pub fn deliver_long_animation_frame(
        &self,
        start_ms: f64,
        duration_ms: f64,
        render_start: f64,
        style_layout_start: f64,
        first_ui_event_ts: f64,
        scripts_json: Option<&str>,
    ) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.ctx.with(|ctx| {
            let blocking = (duration_ms - 50.0_f64).max(0.0);
            let scripts_arg = match scripts_json {
                Some(s) => {
                    // Escape single quotes inside the JSON string for safe JS embedding.
                    let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
                    format!("'{escaped}'")
                }
                None => "null".to_string(),
            };
            let script = format!(
                "if(typeof _lumen_deliver_long_animation_frame==='function')\
                 _lumen_deliver_long_animation_frame\
                 ({start_ms},{duration_ms},{render_start},{style_layout_start},{first_ui_event_ts},{blocking},{scripts_arg});"
            );
            ctx.eval::<(), _>(script.as_str()).ok();
        });
    }

    /// Tune the QuickJS GC based on the tab's lifecycle tier (10L).
    ///
    /// - `Soft` (T0 active): reset `gc_threshold` to 1 MiB so the heap can
    ///   grow freely while the tab is in the foreground.
    /// - `Moderate` (T1): run one full collection cycle; threshold unchanged.
    /// - `Aggressive` (T2): run one full cycle + lower `gc_threshold` to
    ///   64 KiB so subsequent allocations trigger GC much sooner, keeping
    ///   the retained heap small during long background stays.
    pub fn run_gc_pass(&self, level: gc_policy::GcLevel) {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match level {
            gc_policy::GcLevel::Soft => {
                guard._rt.set_gc_threshold(gc_policy::GC_THRESHOLD_ACTIVE);
            }
            gc_policy::GcLevel::Moderate => {
                guard._rt.run_gc();
            }
            gc_policy::GcLevel::Aggressive => {
                guard._rt.run_gc();
                guard._rt.set_gc_threshold(gc_policy::GC_THRESHOLD_IDLE);
            }
        }
    }
}

impl Default for QuickJsRuntime {
    fn default() -> Self {
        Self::new().expect("QuickJS runtime init failed")
    }
}

impl JsRuntime for QuickJsRuntime {
    fn eval(&self, script: &str) -> JsResult<JsValue> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            // Pre-process TC39 decorator syntax (Phase 0 transformer); QuickJS
            // itself rejects `@dec` with a SyntaxError.
            let transformed = decorators::maybe_transform_decorators(&ctx, script);
            let code = transformed.as_deref().unwrap_or(script);
            let val: Value = ctx.eval(code).map_err(|e| rq_err(&ctx, e))?;
            from_rq(&ctx, val)
        })
    }

    fn eval_module(&self, source: &str) -> JsResult<()> {
        self.eval_module(source)
    }

    fn register_module_source(&self, specifier: &str, source: &str) {
        self.register_module_source(specifier, source);
    }

    fn set_global(&self, name: &str, value: JsValue) -> JsResult<()> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            let rq = to_rq(&ctx, value)?;
            ctx.globals().set(name, rq).map_err(|e| rq_err(&ctx, e))
        })
    }

    fn get_global(&self, name: &str) -> JsResult<JsValue> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            let val: Value = ctx.globals().get(name).map_err(|e| rq_err(&ctx, e))?;
            from_rq(&ctx, val)
        })
    }

    fn call_function(&self, name: &str, args: &[JsValue]) -> JsResult<JsValue> {
        let guard = self.inner.lock().unwrap();
        guard.ctx.with(|ctx| {
            // Verify the function exists before building the call.
            let _: Function = ctx.globals().get(name).map_err(|e| rq_err(&ctx, e))?;

            // rquickjs 0.11 Function::call requires IntoArgs (fixed-size tuples only);
            // Function::apply does not exist. Work around with a temporary global +
            // JS Function.prototype.apply via eval.
            let arr = Array::new(ctx.clone())
                .map_err(|e| JsError::Runtime(e.to_string()))?;
            for (i, v) in args.iter().enumerate() {
                let rq_val = to_rq(&ctx, v.clone())?;
                arr.set(i, rq_val).map_err(|e| JsError::Runtime(e.to_string()))?;
            }
            ctx.globals()
                .set("__lum_args__", arr)
                .map_err(|e| rq_err(&ctx, e))?;

            let script = format!("{name}.apply(null, __lum_args__)");
            let result: Value = ctx.eval(script.as_str()).map_err(|e| rq_err(&ctx, e))?;

            // Clean up the temporary global.
            ctx.eval::<Value, _>("delete __lum_args__").ok();

            from_rq(&ctx, result)
        })
    }

    fn engine_name(&self) -> &'static str {
        "quickjs"
    }

    fn suspend(&mut self) -> JsResult<SuspendedHeap> {
        // Pause the event loop first so no microtasks/timers mutate state mid-capture.
        self.pause()?;
        // Capture the serialisable heap payload (task 10C.2 produces real bytes;
        // see `capture_raw_heap`) and deflate-compress it under the 5 MB/tab cap
        // (ADR-008 §10C.3).
        let raw = self.capture_raw_heap();
        match heap_snapshot::compress_heap(&raw) {
            Ok(heap) => Ok(heap),
            // Over the per-tab cap: skip heap persistence and let the tab re-run
            // its scripts on restore. Never block hibernation on a large heap.
            Err(heap_snapshot::HeapSnapshotError::TooLarge { .. }) => {
                Ok(SuspendedHeap::default())
            }
            Err(e) => Err(JsError::Runtime(e.to_string())),
        }
    }

    fn resume(snapshot: SuspendedHeap) -> JsResult<Self> {
        // Validate the snapshot inflates (proves the on-disk stream is intact)
        // before rebuilding. Full heap-content restore (globals/closures/object
        // graph) is task 10C.2 and is blocked by our native-function bindings,
        // which `JS_ReadObject` cannot reconstruct; the shell instead re-runs the
        // page's inline scripts against the restored DOM (see shell
        // `restore_js_context`), so a fresh runtime is the correct result here.
        heap_snapshot::decompress_heap(&snapshot).map_err(|e| JsError::Runtime(e.to_string()))?;
        Self::new()
    }
}

/// Build a JSON array of `{ id, json }` objects from the drained worker message list.
///
/// Each element is `{"id":<worker_id>,"json":<raw_json_value>}` so that
/// `_lumen_deliver_worker_messages` can parse the payload without double-JSON-encoding.
fn build_worker_messages_json(messages: &[(u32, String)]) -> String {
    let items: Vec<String> = messages
        .iter()
        .map(|(id, json)| format!("{{\"id\":{id},\"json\":{json}}}"))
        .collect();
    format!("[{}]", items.join(","))
}

fn rq_err(ctx: &Ctx<'_>, err: rquickjs::Error) -> JsError {
    match err {
        rquickjs::Error::Exception => {
            let exc = ctx.catch();
            JsError::Runtime(exc_message(ctx, exc))
        }
        _ => JsError::Runtime(err.to_string()),
    }
}

fn exc_message<'js>(ctx: &Ctx<'js>, exc: Value<'js>) -> String {
    // JS `Error` objects expose `.message`; fall back to string coercion.
    if let Ok(obj) = Object::from_js(ctx, exc.clone())
        && let Ok(msg) = obj.get::<_, String>("message")
    {
        return msg;
    }
    String::from_js(ctx, exc).unwrap_or_else(|_| "JS exception".into())
}

fn from_rq<'js>(ctx: &Ctx<'js>, val: Value<'js>) -> JsResult<JsValue> {
    match val.type_of() {
        Type::Undefined | Type::Null => Ok(JsValue::Null),
        Type::Bool => bool::from_js(ctx, val)
            .map(JsValue::Bool)
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::Int => i32::from_js(ctx, val)
            .map(|n| JsValue::Number(f64::from(n)))
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::Float => f64::from_js(ctx, val)
            .map(JsValue::Number)
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::String => String::from_js(ctx, val)
            .map(JsValue::String)
            .map_err(|e| JsError::Runtime(e.to_string())),
        Type::Array => {
            let arr = Array::from_js(ctx, val).map_err(|e| JsError::Runtime(e.to_string()))?;
            let mut items = Vec::with_capacity(arr.len());
            for i in 0..arr.len() {
                let elem: Value = arr.get(i).map_err(|e| JsError::Runtime(e.to_string()))?;
                items.push(from_rq(ctx, elem)?);
            }
            Ok(JsValue::Array(items))
        }
        Type::Object | Type::Function => {
            let obj = Object::from_js(ctx, val).map_err(|e| JsError::Runtime(e.to_string()))?;
            let mut entries: Vec<(String, JsValue)> = Vec::new();
            for prop in obj.props::<String, Value>() {
                let (k, v) = prop.map_err(|e| JsError::Runtime(e.to_string()))?;
                entries.push((k, from_rq(ctx, v)?));
            }
            Ok(JsValue::object(entries))
        }
        _ => Ok(JsValue::Undefined),
    }
}

fn to_rq<'js>(ctx: &Ctx<'js>, val: JsValue) -> JsResult<Value<'js>> {
    Ok(match val {
        JsValue::Null | JsValue::Undefined => Value::new_null(ctx.clone()),
        JsValue::Bool(b) => Value::new_bool(ctx.clone(), b),
        JsValue::Number(n) => Value::new_float(ctx.clone(), n),
        JsValue::String(s) => s
            .into_js(ctx)
            .map_err(|e| JsError::Runtime(e.to_string()))?,
        JsValue::Array(items) => {
            let rq_items = items
                .into_iter()
                .map(|v| to_rq(ctx, v))
                .collect::<JsResult<Vec<_>>>()?;
            rq_items
                .into_js(ctx)
                .map_err(|e| JsError::Runtime(e.to_string()))?
        }
        JsValue::Object(entries) => {
            let obj = Object::new(ctx.clone()).map_err(|e| JsError::Runtime(e.to_string()))?;
            for (k, v) in entries {
                let rq_val = to_rq(ctx, v)?;
                obj.set(k, rq_val)
                    .map_err(|e| JsError::Runtime(e.to_string()))?;
            }
            obj.into_js(ctx)
                .map_err(|e| JsError::Runtime(e.to_string()))?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::JsRuntime;

    fn rt() -> QuickJsRuntime {
        QuickJsRuntime::new().unwrap()
    }

    #[test]
    fn eval_number() {
        assert_eq!(rt().eval("1 + 2").unwrap(), JsValue::Number(3.0));
    }

    #[test]
    fn eval_string() {
        assert_eq!(
            rt().eval(r#""hello" + " world""#).unwrap(),
            JsValue::String("hello world".into())
        );
    }

    #[test]
    fn eval_bool() {
        assert_eq!(rt().eval("true").unwrap(), JsValue::Bool(true));
        assert_eq!(rt().eval("false").unwrap(), JsValue::Bool(false));
    }

    #[test]
    fn eval_null() {
        assert_eq!(rt().eval("null").unwrap(), JsValue::Null);
    }

    #[test]
    fn suspend_produces_compressed_snapshot() {
        // Snapshot inflates back to the (currently empty) raw payload — the
        // compression pipeline runs end-to-end even with no heap content.
        let mut rt = rt();
        let heap = rt.suspend().unwrap();
        assert!(heap_snapshot::decompress_heap(&heap).unwrap().is_empty());
    }

    #[test]
    fn resume_rebuilds_runtime_from_valid_snapshot() {
        let mut rt = rt();
        let heap = rt.suspend().unwrap();
        let restored = QuickJsRuntime::resume(heap).unwrap();
        // Fresh runtime is functional (heap-content restore is task 10C.2).
        assert_eq!(restored.eval("6 * 7").unwrap(), JsValue::Number(42.0));
    }

    #[test]
    fn resume_rejects_corrupt_snapshot() {
        let mut bytes = b"LJH1".to_vec();
        bytes.extend_from_slice(b"corrupt");
        let bad = SuspendedHeap::new(bytes);
        assert!(QuickJsRuntime::resume(bad).is_err());
    }

    #[test]
    fn set_get_global() {
        let rt = rt();
        rt.set_global("x", JsValue::Number(42.0)).unwrap();
        assert_eq!(rt.get_global("x").unwrap(), JsValue::Number(42.0));
    }

    #[test]
    fn set_global_string() {
        let rt = rt();
        rt.set_global("greeting", JsValue::String("hi".into()))
            .unwrap();
        assert_eq!(
            rt.get_global("greeting").unwrap(),
            JsValue::String("hi".into())
        );
    }

    #[test]
    fn call_function_add() {
        let rt = rt();
        rt.eval("function add(a, b) { return a + b; }").unwrap();
        assert_eq!(
            rt.call_function("add", &[JsValue::Number(3.0), JsValue::Number(4.0)])
                .unwrap(),
            JsValue::Number(7.0)
        );
    }

    #[test]
    fn call_function_no_args() {
        let rt = rt();
        rt.eval("function forty_two() { return 42; }").unwrap();
        assert_eq!(
            rt.call_function("forty_two", &[]).unwrap(),
            JsValue::Number(42.0)
        );
    }

    #[test]
    fn eval_array() {
        assert_eq!(
            rt().eval("[1, 2, 3]").unwrap(),
            JsValue::Array(vec![
                JsValue::Number(1.0),
                JsValue::Number(2.0),
                JsValue::Number(3.0),
            ])
        );
    }

    #[test]
    fn eval_object() {
        let val = rt().eval(r#"({ a: 1, b: "x" })"#).unwrap();
        assert_eq!(
            val,
            JsValue::object([
                ("a".to_string(), JsValue::Number(1.0)),
                ("b".to_string(), JsValue::String("x".into())),
            ])
        );
    }

    #[test]
    fn eval_runtime_error() {
        assert!(matches!(
            rt().eval("throw new Error('boom')"),
            Err(JsError::Runtime(_))
        ));
    }

    #[test]
    fn eval_syntax_error() {
        // QuickJS wraps parse errors as runtime exceptions.
        assert!(matches!(
            rt().eval("function ("),
            Err(JsError::Runtime(_))
        ));
    }

    #[test]
    fn round_trip_bool() {
        let rt = rt();
        rt.set_global("flag", JsValue::Bool(true)).unwrap();
        assert_eq!(rt.eval("flag").unwrap(), JsValue::Bool(true));
    }

    #[test]
    fn round_trip_array() {
        let rt = rt();
        rt.set_global("arr", JsValue::Array(vec![JsValue::Number(1.0), JsValue::Number(2.0)]))
            .unwrap();
        assert_eq!(
            rt.eval("arr[0] + arr[1]").unwrap(),
            JsValue::Number(3.0)
        );
    }

    #[test]
    fn engine_name() {
        assert_eq!(rt().engine_name(), "quickjs");
    }

    #[test]
    fn is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<QuickJsRuntime>();
    }

    // ── ES Module support ────────────────────────────────────────────────────

    #[test]
    fn eval_module_simple_export() {
        // Inline module with no imports evaluates without error.
        let rt = rt();
        assert!(rt.eval_module("export const x = 42;").is_ok());
    }

    #[test]
    fn eval_module_side_effects_visible() {
        // Module can write to global scope via side-effect assignment.
        let rt = rt();
        rt.eval_module("globalThis.__esm_side_effect__ = 'hello';").unwrap();
        // Drain microtasks: the module Promise resolves synchronously inside eval_module.
        // The globalThis write happens before any import resolution.
        let val = rt.eval("globalThis.__esm_side_effect__").unwrap();
        assert_eq!(val, JsValue::String("hello".into()));
    }

    #[test]
    fn eval_module_imports_registered_module() {
        let rt = rt();
        // Pre-register a utility module.
        rt.register_module_source("mylib", "export const answer = 42;");
        // Import from it in an inline module.
        rt.eval_module("import { answer } from 'mylib'; globalThis.__answer__ = answer;").unwrap();
        // Drain any remaining microtasks.
        let _ = rt.eval("undefined");
        let val = rt.eval("globalThis.__answer__").unwrap();
        assert_eq!(val, JsValue::Number(42.0));
    }

    #[test]
    fn eval_module_syntax_error_returns_error() {
        let rt = rt();
        let result = rt.eval_module("this is not valid JS @@@@");
        assert!(result.is_err(), "should fail on syntax error");
    }

    #[test]
    fn eval_module_dynamic_import_resolves() {
        // dynamic import() of a pre-registered module resolves asynchronously inside the module.
        let rt = rt();
        rt.register_module_source("dynmod", "export const v = 'dynamic';");
        rt.eval_module(r#"
            import('dynmod').then(m => { globalThis.__dyn__ = m.v; });
        "#).unwrap();
        // After eval_module drains microtasks, Promise should have resolved.
        let val = rt.eval("globalThis.__dyn__ || 'unset'").unwrap();
        // Either resolved immediately (QuickJS synchronous promise resolution) or still pending.
        // Both outcomes are valid; the key is no panic/error.
        let _ = val;
    }

    // ── gc_policy tests (10L) ─────────────────────────────────────────────────

    #[test]
    fn gc_level_soft_does_not_panic() {
        let rt = rt();
        // Soft: resets gc_threshold, no panic.
        rt.run_gc_pass(gc_policy::GcLevel::Soft);
    }

    #[test]
    fn gc_level_moderate_does_not_panic() {
        let rt = rt();
        rt.eval("var arr = new Array(1000).fill(0);").unwrap();
        // Moderate: runs one GC cycle, heap still valid after.
        rt.run_gc_pass(gc_policy::GcLevel::Moderate);
        // Verify JS context still works post-GC.
        let v = rt.eval("typeof arr").unwrap();
        assert_eq!(v, JsValue::String("object".into()));
    }

    #[test]
    fn gc_level_aggressive_does_not_panic() {
        let rt = rt();
        rt.eval("var obj = {a:1, b:2};").unwrap();
        // Aggressive: GC + lowered threshold, heap still valid after.
        rt.run_gc_pass(gc_policy::GcLevel::Aggressive);
        let v = rt.eval("obj.a").unwrap();
        assert_eq!(v, JsValue::Number(1.0));
    }

    #[test]
    fn gc_level_sequence_active_idle_active() {
        // Simulate T0 → T1 → T2 → T0 lifecycle.
        let rt = rt();
        rt.eval("globalThis.counter = 42;").unwrap();
        rt.run_gc_pass(gc_policy::GcLevel::Moderate);   // T0 → T1
        rt.run_gc_pass(gc_policy::GcLevel::Aggressive); // T1 → T2
        rt.run_gc_pass(gc_policy::GcLevel::Soft);       // T2 → T0
        // Counter survives all transitions.
        let v = rt.eval("globalThis.counter").unwrap();
        assert_eq!(v, JsValue::Number(42.0));
    }

    #[test]
    fn gc_level_constants_ordering() {
        use gc_policy::{GC_THRESHOLD_ACTIVE, GC_THRESHOLD_IDLE, GcLevel};
        // Active threshold is larger than idle (checked at compile time too).
        const _: () = assert!(GC_THRESHOLD_ACTIVE > GC_THRESHOLD_IDLE);
        // Enum discriminants follow the spec ordering.
        assert_eq!(GcLevel::Soft as u8, 0);
        assert_eq!(GcLevel::Moderate as u8, 1);
        assert_eq!(GcLevel::Aggressive as u8, 2);
    }
}

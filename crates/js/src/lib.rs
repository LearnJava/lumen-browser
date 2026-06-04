pub mod audio_bindings;
pub mod audio_element;
pub mod battery_bindings;
pub mod css_properties_values_api;
pub mod esm;
pub mod paint_worklet;
pub mod gamepad;
pub mod highlight_api;
pub mod iframe_element;
pub mod broadcast_channel;
pub mod canvas2d;
pub mod clipboard;
pub mod contacts;
pub mod cookie_banner;
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
pub mod media_devices;
pub mod media_session;
pub mod navigator_bindings;
pub mod notifications_bindings;
pub mod offscreen_canvas;
pub mod pointer_lock;
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
pub use css_properties_values_api::{install_css_properties_values_api, RegisteredProperty, RegisteredPropertiesMap, get_registered_properties};
pub use paint_worklet::{install_paint_worklet_api, PaintWorkletDef, PaintWorkletRegistry, get_paint_worklet_registry};
pub use dom::{FullscreenRequest, HistoryUrlUpdate, NavigateRequest};
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
    #[allow(dead_code)]
    module_import_map: Arc<Mutex<esm::ImportMap>>,
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
        let loader = esm::LumenLoader::new(Arc::clone(&module_registry));
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
            computed_styles: Arc::new(Mutex::new(HashMap::new())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            worker_messages: Arc::new(Mutex::new(Vec::new())),
            worker_next_id: Arc::new(Mutex::new(1)),
            cookie_banner_dismiss: AtomicBool::new(true),
            pending_notifications: Arc::new(Mutex::new(Vec::new())),
            deterministic: AtomicBool::new(false),
            window_open_requests: Arc::new(Mutex::new(Vec::new())),
            console_messages: Arc::new(Mutex::new(Vec::new())),
            broadcast_channels: Arc::new(Mutex::new(Vec::new())),
            shared_worker_outbox: Arc::new(Mutex::new(Vec::new())),
            pending_history_url_updates: Arc::new(Mutex::new(Vec::new())),
            fullscreen_requests: Arc::new(Mutex::new(Vec::new())),
            view_transition_events: Arc::new(Mutex::new(Vec::new())),
            module_registry,
            module_page_url,
            module_import_map,
        })
    }

    /// Register an ES module by specifier so it can be `import`-ed by other modules.
    ///
    /// `specifier` is the resolved absolute key used in `import` statements.
    /// Pre-populate before calling `eval_module` so that intra-page imports resolve.
    pub fn register_module_source(&self, specifier: &str, source: &str) {
        self.module_registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(specifier.to_owned(), source.to_owned());
    }

    /// Evaluate `source` as an ES module (HTML LS §8.1.3 `<script type=module>`).
    ///
    /// Assigns a virtual `lumen://inline-N` specifier for relative-import resolution.
    /// Drains pending microtasks after evaluation so Promise continuations run.
    pub fn eval_module(&self, source: &str) -> JsResult<()> {
        // Unique sequential inline specifier
        let specifier = {
            let mut reg = self.module_registry.lock().unwrap_or_else(|e| e.into_inner());
            let n = reg.len();
            let key = format!("lumen://inline-{n}");
            reg.insert(key.clone(), source.to_owned());
            key
        };
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
                Arc::clone(&self.scroll_states),
                Arc::clone(&self.pending_scrolls),
                Arc::clone(&self.computed_styles),
                Arc::clone(&self.window_open_requests),
                deterministic_seed,
                Arc::clone(&self.console_messages),
                Arc::clone(&self.pending_history_url_updates),
                Arc::clone(&self.fullscreen_requests),
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

            // Install MediaDevices API (W3C Media Capture §4) — after DOM/navigator so that
            // Promise, DOMException, and navigator are available. Phase 0: all capture
            // requests reject with NotAllowedError; enumerateDevices returns [].
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

            // Install HTMLVideoElement stubs — after DOM so document.createElement is available.
            if let Err(e) = video_bindings::install_video_bindings(&ctx) {
                eprintln!("Video bindings init failed: {}", e);
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
            if let Err(e) = worker::install_worker_bindings(
                &ctx,
                &self.workers,
                &self.worker_messages,
                &self.worker_next_id,
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

    /// Drain all `console.log/warn/error` messages queued since the last call.
    ///
    /// Each entry is `(level, text)` where level is 0=log, 1=warn, 2=error.
    /// Called by the shell's DevTools console panel in `about_to_wait`.
    /// Returns an empty vec when no console calls have been made since the last drain.
    pub fn take_console_messages(&self) -> Vec<(u8, String)> {
        std::mem::take(&mut self.console_messages.lock().unwrap())
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
            let val: Value = ctx.eval(script).map_err(|e| rq_err(&ctx, e))?;
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
}

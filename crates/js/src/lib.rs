pub mod audio_bindings;
pub mod audio_element;
pub mod battery_bindings;
pub mod broadcast_channel;
pub mod clipboard;
pub mod cookie_banner;
pub mod dom;
pub mod geolocation;
pub mod navigator_bindings;
pub mod notifications_bindings;
pub mod surface_api;
pub mod video_bindings;
pub mod webgl_bindings;
pub mod webgl_canvas;
pub mod webrtc_stub;
pub mod worker;

use lumen_core::{JsError, JsResult, JsRuntime, JsValue, SuspendedHeap};
use lumen_dom::Document;
use rquickjs::{Array, Context, Ctx, FromJs, Function, IntoJs, Object, Runtime, Type, Value};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub use clipboard::set_clipboard_provider;
pub use dom::NavigateRequest;
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
        let rt = Runtime::new().map_err(|e| JsError::Runtime(e.to_string()))?;
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
        })
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
        ls_store: Option<Arc<Mutex<WebStorage>>>,
        idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
        sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
    ) -> JsResult<()> {
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
            )
            .map_err(|e| rq_err(&ctx, e))?;

            // Install Battery Status API disable (ADR-007 Layer 4, 9D.4) — after DOM.
            if let Err(e) = battery_bindings::install_battery_bindings(&ctx) {
                eprintln!("Battery bindings init failed: {}", e);
            }

            // Install navigator/screen/timezone normalization (ADR-007 Layer 4, 9D.6) — after DOM.
            if let Err(e) = navigator_bindings::install_navigator_bindings(&ctx) {
                eprintln!("Navigator bindings init failed: {}", e);
            }

            // Install HTMLVideoElement stubs — after DOM so document.createElement is available.
            if let Err(e) = video_bindings::install_video_bindings(&ctx) {
                eprintln!("Video bindings init failed: {}", e);
            }

            // Install HTMLAudioElement stubs (HTML spec §4.8.10) — after DOM/video.
            if let Err(e) = audio_element::install_audio_element_bindings(&ctx) {
                eprintln!("Audio element bindings init failed: {}", e);
            }

            // Install Geolocation API stub (W3C Geolocation L2, §7.7) — after DOM/navigator.
            // Default: PERMISSION_DENIED. Shell may reinitialise with fake coords via
            // install_geolocation_bindings when FingerprintProfile enables them.
            if let Err(e) = geolocation::install_geolocation_bindings(&ctx, None) {
                eprintln!("Geolocation bindings init failed: {}", e);
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

    /// Consume any navigation request that JS placed via `location.href =` etc.
    /// Returns `None` if no navigation was requested during script execution.
    /// Must be called before `drop(runtime)` to avoid losing the request.
    pub fn take_navigate_request(&self) -> Option<NavigateRequest> {
        self.nav_out.lock().unwrap().take()
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

    fn resume(_snapshot: SuspendedHeap) -> JsResult<Self> {
        // BUG-023: QuickJS heap snapshots not yet implemented (ADR-008 T2→T3 restore).
        Err(JsError::Runtime("QuickJS resume not implemented".into()))
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
}

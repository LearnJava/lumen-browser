//! Публичная ручка рантайма: [`V8JsRuntime`], её `impl`-блоки и `impl Drop`.
//!
//! Выделено из `v8_runtime.rs` батчем SPLIT-JS7 без изменений поведения.
//! Объявление структуры едет вместе со своими `impl` (прецедент
//! `enum PageSource`/`impl PageSource`, батчи SH-3a/SH-3b): поля приватны, а
//! приватное поле видно только в своём модуле и его потомках.

use super::*;

// ── Public handle ─────────────────────────────────────────────────────────────

/// BUG-341 S7 — outcome of draining the page-side DOM-mutation tracker since
/// the last [`V8JsRuntime::take_dom_touched`] call.
///
/// Feeds [`lumen_layout::style::restyle_root_set_for_node_change`] so the
/// ADR-016 M4 page pipeline (`Lumen::try_relayout_raf_incremental`) can take
/// the incremental-cascade path (`layout_mutation_incremental_restyle`)
/// instead of a full cascade for JS DOM mutations, mirroring
/// `lumen_chrome::bind_model_tracked` (BUG-341 S6) on the chrome side.
#[derive(Debug, Default, Clone)]
pub struct DomTouched {
    /// Nodes whose selector-relevant attribute/class/style value or child
    /// list actually changed via a tracked primitive (`setAttribute`,
    /// `removeAttribute`, `className`/`classList`, inline `style`,
    /// `appendChild`/`removeChild`/`insertBefore`, `textContent`/`innerHTML`).
    pub nodes: HashSet<NodeId>,
    /// `true` when a mutation happened this cycle through a primitive whose
    /// effect on which nodes' selector-relevant state changed cannot be
    /// attributed precisely (`execCommand`, contenteditable text editing,
    /// Selection-driven range deletes, Shadow DOM attachment). When `true`,
    /// `nodes` alone is **not** a safe restyle root-set — the caller must
    /// fall back to a full cascade for this cycle.
    pub unattributed: bool,
}

/// Per-node snapshot of resolved CSS custom properties: node id → the map of
/// `--name` → computed value that node declares or inherits.
///
/// The inner map is shared behind an `Arc` because custom properties inherit —
/// one `:root` declaration is one allocation for every node under it (BUG-732).
/// Produced by `lumen_layout::collect_custom_properties`, consumed by
/// [`V8JsRuntime::update_custom_properties`].
pub type CustomPropertySnapshot = HashMap<u32, Arc<HashMap<String, String>>>;

/// V8-backed JS runtime implementing [`JsRuntime`].
///
/// The isolate lives on a dedicated thread; methods block until the dispatched
/// job completes. Cheap to clone via `Arc` if shared access is needed (but
/// callers typically hold one runtime per tab).
pub struct V8JsRuntime {
    /// Channel to the JS thread.
    pub(super) cmd_tx: SyncSender<V8Command>,
    /// Join handle taken in `Drop` after sending `Shutdown`.
    pub(super) js_thread: Option<JoinHandle<()>>,
    /// Navigation request written by JS via `location.href=`, `location.assign()` etc.
    /// Captured inside `install_dom`; read by [`Self::take_navigate_request`].
    pub(super) nav_out: Arc<Mutex<Option<crate::dom::NavigateRequest>>>,
    /// Next timer wakeup deadline as Unix epoch ms (set by `_lumen_request_wakeup`).
    /// `take_timer_wakeup` atomically clears after reading.
    pub(super) timer_wakeup: Arc<Mutex<Option<f64>>>,
    /// Set to `true` by any DOM-mutating JS binding. Cleared by `take_dom_dirty`.
    pub(super) dom_dirty: Arc<AtomicBool>,
    /// BUG-341 S7: nodes touched by a tracked DOM-mutation primitive since the
    /// last [`Self::take_dom_touched`] call, plus the `unattributed` fallback
    /// flag. See [`DomTouched`].
    pub(super) dom_touched: Arc<Mutex<DomTouched>>,
    /// Set to `true` when JS calls `requestAnimationFrame(fn)`.
    pub(super) raf_pending: Arc<AtomicBool>,
    /// Layout bounding rects updated after each relayout by the shell.
    /// Maps `NodeId` index (u32) → `[x, y, width, height]` in viewport-relative CSS px.
    pub(super) layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Current viewport size `[width, height]` in CSS px.
    pub(super) viewport_size: Arc<Mutex<[f32; 2]>>,
    /// Lazy image load requests queued by `_lumen_request_lazy_image_load` from JS.
    pub(super) lazy_img_requests: Arc<Mutex<Vec<(u32, String)>>>,
    /// Scroll state per scroll-container node, updated after each relayout.
    pub(super) scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Pending scroll requests queued by JS via `_lumen_request_scroll`.
    pub(super) pending_scrolls: Arc<Mutex<Vec<(u32, f32, f32)>>>,
    /// Pending page-level scroll requests from JS `window.scrollTo/scrollBy`.
    pub(super) pending_page_scrolls: Arc<Mutex<Vec<(f32, bool)>>>,
    /// Current page scroll Y exposed to JS `window.scrollY` / `window.pageYOffset`.
    pub(super) page_scroll_y: Arc<Mutex<f32>>,
    /// BUG-822: `true` when the page moved on a rendering update whose scroll
    /// sequence was still in flight, so a `scrollend` is still owed once it
    /// settles. Lives next to [`Self::page_scroll_y`] for the same reason it
    /// does — the debt belongs to the document, not to the shell.
    pub(super) page_scroll_end_pending: Arc<Mutex<bool>>,
    /// Computed CSS styles per node, updated after each relayout by the shell.
    pub(super) computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    /// Resolved CSS custom properties per node (keys carry their `--` prefix),
    /// updated after each relayout by the shell alongside [`Self::computed_styles`].
    /// Kept in a separate map behind an `Arc` because custom properties inherit:
    /// every node under a `:root`-declared set shares one allocation instead of
    /// carrying its own copy of every variable (BUG-732).
    pub(super) custom_properties: Arc<Mutex<CustomPropertySnapshot>>,
    /// Pending popup window requests queued by JS `window.open()`.
    pub(super) window_open_requests: Arc<Mutex<Vec<crate::dom::PopupRequest>>>,
    /// Console messages queued by `console.log/warn/error` calls in JS.
    pub(super) console_messages: Arc<Mutex<Vec<(u8, String)>>>,
    /// `history.pushState` / `history.replaceState` URL-update notifications.
    pub(super) pending_history_url_updates: Arc<Mutex<Vec<crate::dom::HistoryUrlUpdate>>>,
    /// `history.go(n)` / `back` / `forward` traversal deltas.
    pub(super) pending_history_traversals: Arc<Mutex<Vec<i32>>>,
    /// Shell-backed Navigation API state (serialised JSON of nav history + index).
    pub(super) nav_state: Arc<Mutex<String>>,
    /// Queued by `_lumen_navigation_request`; drained by the shell.
    pub(super) pending_navigation_updates: Arc<Mutex<Vec<crate::dom::NavUpdate>>>,
    /// Queued by `_lumen_navigation_report_intercept` during `NavigateEvent` dispatch.
    pub(super) pending_nav_intercepted: Arc<Mutex<Vec<(bool, bool)>>>,
    /// Fullscreen requests emitted by `element.requestFullscreen()` / `document.exitFullscreen()`.
    pub(super) fullscreen_requests: Arc<Mutex<Vec<crate::dom::FullscreenRequest>>>,
    /// CSS View Transitions L1 events emitted by `document.startViewTransition` (Ph3
    /// V8 migration S12b-G5). Mirrors [`crate::QuickJsRuntime`]'s field of the same
    /// name; drained by the shell in `about_to_wait` via `take_view_transition_events()`.
    pub(super) view_transition_events: Arc<Mutex<Vec<crate::view_transitions::ViewTransitionEvent>>>,
    /// Print requests emitted by `window.print()`.
    pub(super) print_requests: Arc<Mutex<Vec<crate::dom::PrintRequest>>>,
    /// Focus requests queued by JS via `_lumen_request_focus` / `_lumen_request_blur`.
    pub(super) pending_focus_requests: Arc<Mutex<Vec<Option<u32>>>>,
    /// Node ID of the current pointer capture target (W3C Pointer Events L3 §4.1),
    /// set via `_lumen_set_capture_state`/`_lumen_release_capture_state`.
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    pub(super) pointer_capture_nid: Arc<Mutex<Option<u32>>>,
    /// Deterministic render mode (8F): when `true`, `Date.now()`/`Math.random` are frozen/seeded.
    pub(super) deterministic: AtomicBool,
    /// DEVX-16: `--rng-seed` override for deterministic mode's `Math.random`
    /// seed. `None` means derive the seed from the page URL hash (previous,
    /// still-default behaviour).
    pub(super) deterministic_rng_seed: Mutex<Option<u64>>,
    /// BUG-480 срез 8: ключ собственного документа в ящиках моста (указатель
    /// Arc, 0 = контекст без документа — до install_dom). Дублирует
    /// `frame_docs.self_key` для чтения без захвата реестра: по нему
    /// [`Self::frame_transport_pending`] отвечает на вопрос шелла «есть ли
    /// неразобранные конверты для МЕНЯ».
    pub(super) self_doc_key: AtomicUsize,
    /// DEVX-16: `--monotonic-clock` — when `true` (and `deterministic` is also
    /// `true`), `Date.now()`/`performance.now()` advance [`Self::deterministic_clock_ms`]
    /// by 1 ms per call instead of staying frozen at 0.
    pub(super) deterministic_monotonic: AtomicBool,
    /// Shared counter backing `deterministic_monotonic`'s clock advance, reset
    /// to 0 by [`Self::set_deterministic_mode`].
    pub(super) deterministic_clock_ms: Arc<AtomicU64>,
    /// Live SW execution threads keyed by `(origin, scope)`.
    pub(super) sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
    /// `sessionStorage` partition of the browsing context this runtime serves
    /// (BUG-836). Session storage is scoped to the *tab*, not the document, so
    /// the owner of the tab hands the same `Arc` to every document's runtime
    /// via [`Self::with_session_storage`]; `None` (tests, headless) means a
    /// fresh, document-local store — which is what every document used to get.
    pub(super) ss_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
    /// `BroadcastChannel` instances created on this page (WHATWG HTML §9.5).
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    pub(super) broadcast_channels: crate::broadcast_channel::BroadcastRegistry,
    /// Pending OS notification requests queued by `new Notification(...)` in JS.
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    pub(super) pending_notifications: crate::notifications_bindings::NotificationQueue,
    /// Live dedicated-`Worker` threads spawned by this page (Ph3 V8 migration
    /// S10). Mirrors [`crate::QuickJsRuntime`]'s `workers` field.
    pub(super) workers: crate::worker::WorkerRegistry,
    /// Outbound queue drained by [`Self::pump_workers`]. Mirrors
    /// [`crate::QuickJsRuntime`]'s `worker_messages` field.
    pub(super) worker_messages: crate::worker::WorkerMessageQueue,
    /// Outbound uncaught-exception report queue drained by [`Self::pump_workers`]
    /// (BUG-591 worker parent-side reporting) — parallel to `worker_messages`
    /// but for `Worker`'s `error` event rather than `message`.
    pub(super) worker_errors: crate::worker::WorkerErrorQueue,
    /// Next `Worker` id to assign. Mirrors [`crate::QuickJsRuntime`]'s
    /// `worker_next_id` field.
    pub(super) worker_next_id: Arc<Mutex<u32>>,
    /// Blob URL → script text, mirrored from `URL.createObjectURL` for
    /// `importScripts()`. Mirrors [`crate::QuickJsRuntime`]'s
    /// `worker_blob_store` field.
    pub(super) worker_blob_store: crate::worker::WorkerBlobStore,
    /// Outbound queue for this page's `SharedWorker` client ports, drained by
    /// [`Self::pump_shared_workers`]. Mirrors [`crate::QuickJsRuntime`]'s
    /// `shared_worker_outbox` field.
    pub(super) shared_worker_outbox: crate::shared_worker::SharedWorkerOutbox,
    /// Outbound uncaught-exception report queue for this page's `SharedWorker`
    /// instances, drained by [`Self::pump_shared_workers`] (BUG-591
    /// SharedWorker parent-side reporting) — parallel to
    /// `shared_worker_outbox` but for the `error` event rather than `message`.
    pub(super) shared_worker_errors: crate::worker::WorkerErrorQueue,
    /// Cookie-banner auto-dismiss (7C.3) enable flag (Ph3 V8 migration S12b-G6,
    /// BUG-548). Defaults to `true`. Shell sets this from the user's
    /// `cookie_banner_dismiss` preference via [`Self::set_cookie_banner_dismiss`].
    /// Mirrors [`crate::QuickJsRuntime`]'s field of the same name.
    pub(super) cookie_banner_dismiss: AtomicBool,
    /// BUG-480 срез 2: реестр под-документов `<iframe>` («хост → Document»),
    /// общий с нативами [`crate::frame_bridge`]. Наполняется
    /// [`Self::register_frame_document`] после загрузки каждого фрейма.
    pub(super) frame_docs: crate::frame_bridge::FrameDocRegistry,
}

/// Разложить плоский `[name, value, name, value, …]` из JS в пары.
///
/// Формат «плоский массив» выбран потому, что мост натива понимает
/// `Vec<String>`, но не массив массивов; непарный хвост (нечётная длина —
/// шим такого не строит) отбрасывается, а не превращается в заголовок с
/// пустым значением.
pub(super) fn pairs_from_flat(flat: Vec<String>) -> Vec<(String, String)> {
    let mut it = flat.into_iter();
    let mut out = Vec::with_capacity(it.len() / 2);
    while let (Some(name), Some(value)) = (it.next(), it.next()) {
        out.push((name, value));
    }
    out
}

impl V8JsRuntime {
    /// Create a new V8 runtime on a dedicated thread.
    pub fn new() -> Result<Self, JsError> {
        let (cmd_tx, cmd_rx) = sync_channel::<V8Command>(V8_CMD_QUEUE_BOUND);
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), JsError>>();
        let js_thread = std::thread::Builder::new()
            .name("lumen-v8".to_string())
            .spawn(move || v8_thread_main(cmd_rx, init_tx))
            .map_err(|e| JsError::Runtime(format!("spawn V8 thread: {e}")))?;
        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(JsError::Runtime("V8 thread died during init".into())),
        }
        Ok(Self {
            cmd_tx,
            js_thread: Some(js_thread),
            nav_out: Arc::new(Mutex::new(None)),
            timer_wakeup: Arc::new(Mutex::new(None)),
            dom_dirty: Arc::new(AtomicBool::new(false)),
            dom_touched: Arc::new(Mutex::new(DomTouched::default())),
            raf_pending: Arc::new(AtomicBool::new(false)),
            layout_rects: Arc::new(Mutex::new(HashMap::new())),
            viewport_size: Arc::new(Mutex::new([0.0, 0.0])),
            lazy_img_requests: Arc::new(Mutex::new(Vec::new())),
            scroll_states: Arc::new(Mutex::new(HashMap::new())),
            pending_scrolls: Arc::new(Mutex::new(Vec::new())),
            pending_page_scrolls: Arc::new(Mutex::new(Vec::new())),
            page_scroll_y: Arc::new(Mutex::new(0.0)),
            page_scroll_end_pending: Arc::new(Mutex::new(false)),
            computed_styles: Arc::new(Mutex::new(HashMap::new())),
            custom_properties: Arc::new(Mutex::new(HashMap::new())),
            window_open_requests: Arc::new(Mutex::new(Vec::new())),
            console_messages: Arc::new(Mutex::new(Vec::new())),
            pending_history_url_updates: Arc::new(Mutex::new(Vec::new())),
            pending_history_traversals: Arc::new(Mutex::new(Vec::new())),
            nav_state: Arc::new(Mutex::new(String::from(r#"{"entries":[],"index":0}"#))),
            pending_navigation_updates: Arc::new(Mutex::new(Vec::new())),
            pending_nav_intercepted: Arc::new(Mutex::new(Vec::new())),
            fullscreen_requests: Arc::new(Mutex::new(Vec::new())),
            view_transition_events: Arc::new(Mutex::new(Vec::new())),
            print_requests: Arc::new(Mutex::new(Vec::new())),
            pending_focus_requests: Arc::new(Mutex::new(Vec::new())),
            pointer_capture_nid: Arc::new(Mutex::new(None)),
            deterministic: AtomicBool::new(false),
            deterministic_rng_seed: Mutex::new(None),
            self_doc_key: AtomicUsize::new(0),
            deterministic_monotonic: AtomicBool::new(false),
            deterministic_clock_ms: Arc::new(AtomicU64::new(0)),
            sw_worker_store: None,
            ss_store: None,
            broadcast_channels: Arc::new(Mutex::new(Vec::new())),
            pending_notifications: Arc::new(Mutex::new(Vec::new())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            worker_messages: Arc::new(Mutex::new(Vec::new())),
            worker_errors: Arc::new(Mutex::new(Vec::new())),
            worker_next_id: Arc::new(Mutex::new(0)),
            worker_blob_store: Arc::new(Mutex::new(HashMap::new())),
            shared_worker_outbox: Arc::new(Mutex::new(Vec::new())),
            shared_worker_errors: Arc::new(Mutex::new(Vec::new())),
            cookie_banner_dismiss: AtomicBool::new(true),
            frame_docs: Arc::new(Mutex::new(crate::frame_bridge::FrameDocSlots::default())),
        })
    }

    /// Set whether the cookie-banner auto-dismiss shim (7C.3) is injected on
    /// the next `install_dom`. Mirrors [`crate::QuickJsRuntime::set_cookie_banner_dismiss`].
    pub fn set_cookie_banner_dismiss(&self, enabled: bool) {
        self.cookie_banner_dismiss.store(enabled, Ordering::Relaxed);
    }

    /// Shared handle to this runtime's `BroadcastChannel` registry, for the
    /// natives registered by [`crate::broadcast_channel::install_broadcast_channel_bindings_v8`].
    pub(crate) fn broadcast_registry(&self) -> crate::broadcast_channel::BroadcastRegistry {
        Arc::clone(&self.broadcast_channels)
    }

    /// Deliver messages posted to this page's `BroadcastChannel` instances.
    /// Mirrors [`crate::QuickJsRuntime::pump_broadcast_channels`].
    pub fn pump_broadcast_channels(&self) {
        let messages = crate::broadcast_channel::drain(&self.broadcast_channels);
        if messages.is_empty() {
            return;
        }
        let json = crate::build_worker_messages_json(&messages);
        let script = format!(
            "if(typeof _lumen_deliver_broadcast_messages==='function')\
             _lumen_deliver_broadcast_messages({json})"
        );
        let _ = self.eval(&script);
    }

    /// Deliver messages posted by worker threads to their `Worker` JS
    /// instances (Ph3 V8 migration S10). Mirrors
    /// [`crate::QuickJsRuntime::pump_workers`].
    pub fn pump_workers(&self) {
        let messages = crate::worker::drain_messages(&self.worker_messages);
        if !messages.is_empty() {
            let json = crate::build_worker_messages_json(&messages);
            let script = format!(
                "if(typeof _lumen_deliver_worker_messages==='function')\
                 _lumen_deliver_worker_messages({json})"
            );
            let _ = self.eval(&script);
        }

        // BUG-591 worker parent-side reporting: deliver uncaught-exception
        // reports (top-level script failure, or a message/timer callback
        // throw) as `Worker`'s `error` event.
        let errors = crate::worker::drain_errors(&self.worker_errors);
        if !errors.is_empty() {
            let json = crate::build_worker_messages_json(&errors);
            let script = format!(
                "if(typeof _lumen_deliver_worker_errors==='function')\
                 _lumen_deliver_worker_errors({json})"
            );
            let _ = self.eval(&script);
        }
    }

    /// Deliver messages posted by `SharedWorker` threads to this page's
    /// ports (Ph3 V8 migration S10). Mirrors
    /// [`crate::QuickJsRuntime::pump_shared_workers`].
    pub fn pump_shared_workers(&self) {
        let messages = crate::shared_worker::drain_messages(&self.shared_worker_outbox);
        if !messages.is_empty() {
            let json = crate::build_worker_messages_json(&messages);
            let script = format!(
                "if(typeof _lumen_deliver_shared_worker_messages==='function')\
                 _lumen_deliver_shared_worker_messages({json})"
            );
            let _ = self.eval(&script);
        }

        // BUG-591 SharedWorker parent-side reporting: deliver uncaught-exception
        // reports broadcast from the shared-worker thread as `SharedWorker`'s
        // `error` event.
        let errors = crate::worker::drain_errors(&self.shared_worker_errors);
        if !errors.is_empty() {
            let json = crate::build_worker_messages_json(&errors);
            let script = format!(
                "if(typeof _lumen_deliver_shared_worker_errors==='function')\
                 _lumen_deliver_shared_worker_errors({json})"
            );
            let _ = self.eval(&script);
        }
    }

    /// Shared handle to this runtime's pending-notifications queue, for the
    /// natives registered by [`crate::notifications_bindings::install_notifications_bindings_v8`].
    pub(crate) fn notification_queue(&self) -> crate::notifications_bindings::NotificationQueue {
        Arc::clone(&self.pending_notifications)
    }

    /// Drain all OS notification requests queued by `new Notification(...)` in JS.
    /// Mirrors [`crate::QuickJsRuntime::take_notification_requests`].
    pub fn take_notification_requests(
        &self,
    ) -> Vec<crate::notifications_bindings::NotificationRequest> {
        crate::notifications_bindings::drain_notifications(&self.pending_notifications)
    }

    /// Consume any navigation request that JS placed via `location.href =` etc.
    /// Returns `None` if no navigation was requested during script execution.
    pub fn take_navigate_request(&self) -> Option<crate::dom::NavigateRequest> {
        self.nav_out
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Take the next timer wakeup as Unix epoch ms, clearing the stored value.
    /// Returns `None` when no timers are pending.
    pub fn take_timer_wakeup(&self) -> Option<f64> {
        self.timer_wakeup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// Drain all print requests queued by `window.print()`.
    pub fn take_print_requests(&self) -> Vec<crate::dom::PrintRequest> {
        std::mem::take(
            &mut *self
                .print_requests
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }

    /// BUG-480 срез 8: есть ли в ящиках моста неразобранные конверты,
    /// адресованные ЭТОМУ контексту. Шелл опрашивает живые рантаймы на каждом
    /// `about_to_wait` и, пока хоть кто-то отвечает «да», держит короткий poll-
    /// дедлайн — иначе конверт после затихания страницы лежит до случайного
    /// пробуждения цикла. До install_dom (ключ 0) всегда `false`.
    pub fn frame_transport_pending(&self) -> bool {
        let key = self.self_doc_key.load(Ordering::Relaxed);
        crate::frame_bridge::frame_transport_has_for((key != 0).then_some(key))
    }

    /// Enable or disable deterministic render mode (8F) before calling `install_dom`.
    ///
    /// `rng_seed` (DEVX-16, `--rng-seed`): overrides the URL-hash-derived
    /// `Math.random` seed when `Some`; ignored when `on` is `false`.
    /// `monotonic_clock` (DEVX-16, `--monotonic-clock`): when `true`,
    /// `Date.now()`/`performance.now()` advance by 1 ms per call instead of
    /// staying frozen at 0; also ignored when `on` is `false`.
    pub fn set_deterministic_mode(&self, on: bool, rng_seed: Option<u64>, monotonic_clock: bool) {
        self.deterministic.store(on, Ordering::Relaxed);
        *self
            .deterministic_rng_seed
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = rng_seed;
        self.deterministic_monotonic
            .store(monotonic_clock, Ordering::Relaxed);
        self.deterministic_clock_ms.store(0, Ordering::Relaxed);
    }

    /// Attach a `SwWorkerStore` so that `_lumen_sw_activate_script` can spawn and
    /// register SW execution threads when pages activate a Service Worker.
    ///
    /// Must be called before `install_dom` to take effect (mirrors
    /// [`crate::QuickJsRuntime::with_sw_worker_store`]).
    pub fn with_sw_worker_store(mut self, store: lumen_core::ext::SwWorkerStore) -> Self {
        self.sw_worker_store = Some(store);
        self
    }

    /// Attach the browsing context's `sessionStorage` partition (BUG-836).
    ///
    /// HTML LS §12.2 binds session storage to the browsing context, so the tab —
    /// not the document — owns the store and hands the same `Arc` to every
    /// document it loads. Without this the runtime builds its own empty store in
    /// `install_dom`, and everything written by the previous document is lost on
    /// navigation. Must be called before `install_dom` to take effect (mirrors
    /// [`Self::with_sw_worker_store`]).
    pub fn with_session_storage(mut self, store: Arc<Mutex<lumen_core::WebStorage>>) -> Self {
        self.ss_store = Some(store);
        self
    }

    /// Returns `true` if JS mutated the DOM since the last call, clearing the flag.
    /// Mirrors [`crate::QuickJsRuntime::take_dom_dirty`].
    pub fn take_dom_dirty(&self) -> bool {
        self.dom_dirty.swap(false, Ordering::Relaxed)
    }

    /// BUG-341 S7: drain the set of nodes touched by tracked DOM-mutation
    /// primitives since the last call, clearing it (and the `unattributed`
    /// flag) for the next cycle. See [`DomTouched`].
    pub fn take_dom_touched(&self) -> DomTouched {
        std::mem::take(&mut *self.dom_touched.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Returns `true` if `requestAnimationFrame` was called since the last call,
    /// clearing the flag. Mirrors [`crate::QuickJsRuntime::take_raf_pending`].
    pub fn take_raf_pending(&self) -> bool {
        self.raf_pending.swap(false, Ordering::Relaxed)
    }

    /// Non-consuming peek: `true` if rAF callbacks are queued.
    /// Mirrors [`crate::QuickJsRuntime::has_raf_pending`].
    pub fn has_raf_pending(&self) -> bool {
        self.raf_pending.load(Ordering::Relaxed)
    }

    /// ADR-016 M2.3: shared, lock-free handle to the rAF-pending flag.
    /// Mirrors [`crate::QuickJsRuntime::raf_pending_flag`].
    pub fn raf_pending_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.raf_pending)
    }

    /// ADR-016 M2.3: shared, lock-free handle to the DOM-dirty flag.
    /// Mirrors [`crate::QuickJsRuntime::dom_dirty_flag`].
    pub fn dom_dirty_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.dom_dirty)
    }

    /// Replace the layout bounding-rect table with a fresh snapshot.
    /// Mirrors [`crate::QuickJsRuntime::update_layout_rects`].
    pub fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>) {
        *self.layout_rects.lock().unwrap_or_else(|e| e.into_inner()) = rects;
    }

    /// Update the current viewport dimensions.
    /// Mirrors [`crate::QuickJsRuntime::update_viewport_size`].
    pub fn update_viewport_size(&self, width: f32, height: f32) {
        *self.viewport_size.lock().unwrap_or_else(|e| e.into_inner()) = [width, height];
    }

    /// Drain lazy image load requests queued by JS.
    /// Mirrors [`crate::QuickJsRuntime::take_lazy_image_requests`].
    pub fn take_lazy_image_requests(&self) -> Vec<(u32, String)> {
        std::mem::take(&mut *self.lazy_img_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Replace the scroll-state table with a fresh snapshot from the layout tree.
    /// Mirrors [`crate::QuickJsRuntime::update_scroll_states`].
    pub fn update_scroll_states(&self, states: HashMap<u32, [f32; 4]>) {
        *self.scroll_states.lock().unwrap_or_else(|e| e.into_inner()) = states;
    }

    /// Drain JS-initiated scroll requests queued by `_lumen_request_scroll`.
    /// Mirrors [`crate::QuickJsRuntime::take_scroll_requests`].
    pub fn take_scroll_requests(&self) -> Vec<(u32, f32, f32)> {
        std::mem::take(&mut *self.pending_scrolls.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain JS page-level scroll requests from `window.scrollTo/scrollBy`.
    /// Mirrors [`crate::QuickJsRuntime::take_page_scroll_requests`].
    pub fn take_page_scroll_requests(&self) -> Vec<(f32, bool)> {
        std::mem::take(&mut *self.pending_page_scrolls.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Update the page scroll Y exposed to JS `window.scrollY`/`pageYOffset`.
    ///
    /// Returns `true` when the position actually moved since the last call, so
    /// the caller can run CSSOM-View §14 «run the scroll steps» for the
    /// viewport (BUG-821). The previous value is kept here, per runtime, i.e.
    /// per document: a navigation builds a fresh runtime whose stored position
    /// is 0, so resetting the shell's `scroll_y` to the top cannot report a
    /// change against the *outgoing* document's position.
    pub fn set_page_scroll_y(&self, y: f32) -> bool {
        let mut cur = self.page_scroll_y.lock().unwrap_or_else(|e| e.into_inner());
        let moved = (*cur - y).abs() > f32::EPSILON;
        *cur = y;
        moved
    }

    /// Fire a non-bubbling `scroll` Event on the DOM element identified by `nid`.
    /// Mirrors [`crate::QuickJsRuntime::fire_element_scroll`].
    pub fn fire_element_scroll(&self, nid: u32) {
        let script = format!(
            "if(typeof _lumen_fire_scroll_on_element==='function')\
             _lumen_fire_scroll_on_element({nid});"
        );
        self.eval(&script).ok();
    }

    /// Fire a non-bubbling `scroll` Event on the `window` object (page scroll).
    /// Mirrors [`crate::QuickJsRuntime::fire_window_scroll`].
    pub fn fire_window_scroll(&self) {
        self.eval(
            "if(typeof _lumen_fire_window_scroll_event==='function')\
             _lumen_fire_window_scroll_event();"
        ).ok();
    }

    /// BUG-822: decide whether this rendering update owes a `scrollend` on the
    /// viewport, and clear the debt when it does.
    ///
    /// `moved` is [`Self::set_page_scroll_y`]'s answer for this update;
    /// `settled` says nothing is still driving the scroll (no smooth animation,
    /// no touch momentum, no scrollbar drag in progress). An instant scroll is
    /// therefore `moved && settled` and gets `scroll` + `scrollend` in the same
    /// frame, which CSSOM-View §14 allows; an animated one accumulates the debt
    /// while it runs and pays it on the update that stops moving the page —
    /// including the case where the last frame of touch momentum happens to
    /// clamp at the edge and moves nothing at all.
    pub fn page_scrollend_due(&self, moved: bool, settled: bool) -> bool {
        let mut pending = self.page_scroll_end_pending.lock().unwrap_or_else(|e| e.into_inner());
        if !settled {
            *pending |= moved;
            return false;
        }
        let due = moved || *pending;
        *pending = false;
        due
    }

    /// Fire a non-bubbling `scrollend` Event on the DOM element identified by `nid`.
    pub fn fire_element_scrollend(&self, nid: u32) {
        let script = format!(
            "if(typeof _lumen_fire_scrollend_on_element==='function')\
             _lumen_fire_scrollend_on_element({nid});"
        );
        self.eval(&script).ok();
    }

    /// Fire a non-bubbling `scrollend` Event on the `window` object (page scroll).
    pub fn fire_window_scrollend(&self) {
        self.eval(
            "if(typeof _lumen_fire_window_scrollend_event==='function')\
             _lumen_fire_window_scrollend_event();"
        ).ok();
    }

    /// Fire a CSS Scroll Snap L2 `snapchanging` event on a scroll container.
    /// Mirrors [`crate::QuickJsRuntime::fire_snap_changing`].
    pub fn fire_snap_changing(&self, nid: u32, block: Option<u32>, inline: Option<u32>) {
        self.fire_snap_event("_lumen_fire_snap_changing", nid, block, inline);
    }

    /// Fire a CSS Scroll Snap L2 `snapchanged` event on a scroll container.
    /// Mirrors [`crate::QuickJsRuntime::fire_snap_changed`].
    pub fn fire_snap_changed(&self, nid: u32, block: Option<u32>, inline: Option<u32>) {
        self.fire_snap_event("_lumen_fire_snap_changed", nid, block, inline);
    }

    /// Shared dispatch path for the snap events.
    /// Mirrors [`crate::QuickJsRuntime::fire_snap_event`].
    fn fire_snap_event(&self, func: &str, nid: u32, block: Option<u32>, inline: Option<u32>) {
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
        self.eval(&script).ok();
    }

    /// Push a fresh snapshot of computed CSS styles into the JS runtime.
    /// Mirrors [`crate::QuickJsRuntime::update_computed_styles`].
    pub fn update_computed_styles(&self, styles: HashMap<u32, HashMap<String, String>>) {
        *self.computed_styles.lock().unwrap_or_else(|e| e.into_inner()) = styles;
    }

    /// Push a fresh snapshot of resolved CSS custom properties into the JS
    /// runtime, feeding `getComputedStyle(el).getPropertyValue('--x')`
    /// (BUG-732). Published from the same places as
    /// [`Self::update_computed_styles`] — a page whose custom properties are
    /// never pushed answers `""` for every variable.
    pub fn update_custom_properties(&self, props: CustomPropertySnapshot) {
        *self
            .custom_properties
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = props;
    }

    /// Update `document.hidden` / `document.visibilityState` and fire
    /// `visibilitychange` on both `document` and `window`.
    /// Mirrors [`crate::QuickJsRuntime::set_document_visibility`].
    pub fn set_document_visibility(&self, hidden: bool) {
        let script = if hidden {
            "_lumen_apply_visibility(true)"
        } else {
            "_lumen_apply_visibility(false)"
        };
        self.eval(script).ok();
    }

    /// Drain all popup window requests queued by JS `window.open(...)`.
    /// Mirrors [`crate::QuickJsRuntime::take_window_open_requests`].
    pub fn take_window_open_requests(&self) -> Vec<PopupRequest> {
        std::mem::take(&mut *self.window_open_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all `console.log/warn/error` messages queued since the last call.
    /// Mirrors [`crate::QuickJsRuntime::take_console_messages`].
    pub fn take_console_messages(&self) -> Vec<(u8, String)> {
        std::mem::take(&mut *self.console_messages.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain `history.pushState`/`history.replaceState` URL-update notifications.
    /// Mirrors [`crate::QuickJsRuntime::take_history_url_updates`].
    pub fn take_history_url_updates(&self) -> Vec<HistoryUrlUpdate> {
        std::mem::take(&mut *self.pending_history_url_updates.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all `history.go(n)`/back/forward traversal deltas queued by JS.
    /// Mirrors [`crate::QuickJsRuntime::take_history_traversals`].
    pub fn take_history_traversals(&self) -> Vec<i32> {
        std::mem::take(&mut *self.pending_history_traversals.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all Navigation API update requests queued by `_lumen_navigation_request`.
    /// Mirrors [`crate::QuickJsRuntime::take_nav_updates`].
    pub fn take_nav_updates(&self) -> Vec<crate::dom::NavUpdate> {
        std::mem::take(&mut *self.pending_navigation_updates.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain `NavigateEvent` intercept results queued during event dispatch.
    /// Mirrors [`crate::QuickJsRuntime::take_nav_intercept_result`].
    pub fn take_nav_intercept_result(&self) -> Vec<(bool, bool)> {
        std::mem::take(&mut *self.pending_nav_intercepted.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain all fullscreen requests queued by `element.requestFullscreen()`/`exitFullscreen()`.
    /// Mirrors [`crate::QuickJsRuntime::take_fullscreen_requests`].
    pub fn take_fullscreen_requests(&self) -> Vec<FullscreenRequest> {
        std::mem::take(&mut *self.fullscreen_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Drain CSS View Transition events queued by `document.startViewTransition()` natives.
    /// Mirrors [`crate::QuickJsRuntime::take_view_transition_events`].
    pub fn take_view_transition_events(&self) -> Vec<crate::view_transitions::ViewTransitionEvent> {
        std::mem::take(
            &mut *self.view_transition_events.lock().unwrap_or_else(|e| e.into_inner()),
        )
    }

    /// Drain JS dialog focus requests queued by `_lumen_request_focus`/`_lumen_request_blur`.
    /// Mirrors [`crate::QuickJsRuntime::take_focus_requests`].
    pub fn take_focus_requests(&self) -> Vec<Option<u32>> {
        std::mem::take(&mut *self.pending_focus_requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Returns the DOM node nid that currently holds pointer capture (pointer_id=1).
    ///
    /// Shell calls this before dispatching pointer events to redirect them to the
    /// capture target instead of the hit-tested element (W3C Pointer Events L3 §4.1).
    /// Returns `None` when no capture is active. Mirrors [`crate::QuickJsRuntime::pointer_capture_nid`].
    pub fn pointer_capture_nid(&self) -> Option<u32> {
        *self.pointer_capture_nid.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Release the active pointer capture, returning the former capture target nid.
    ///
    /// Called by the shell implicitly on `pointerup`/`pointercancel` per spec §4.1.
    /// Returns `None` if no capture was active. Mirrors [`crate::QuickJsRuntime::take_pointer_capture`].
    pub fn take_pointer_capture(&self) -> Option<u32> {
        self.pointer_capture_nid.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// Test-only: run `f` on the JS thread and return its result. Gives module
    /// test suites (e.g. `canvas2d::tests_v8`) a way to inspect `thread_local!`
    /// state that has no JS-visible getter native — the JS thread owns its own
    /// thread-local instance, distinct from the test's calling thread. Mirrors
    /// [`Self::flush_canvas_updates`], generalized.
    #[cfg(test)]
    pub(crate) fn run_for_test<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.run(move |_inner| f())
    }

    /// Drain dirty `<canvas>` 2D buffers for GPU re-upload. Mirrors
    /// [`crate::QuickJsRuntime::flush_canvas_updates`]. Must run on the JS
    /// thread since `canvas2d`'s `CANVASES`/`DIRTY` registries are `thread_local!`.
    pub fn flush_canvas_updates(&self) -> Vec<(u32, u32, u32, Vec<u8>)> {
        self.run(|_inner| crate::canvas2d::flush_dirty())
    }

    /// Register decoded `<img>` bitmaps for canvas `drawImage`, keyed by node id.
    ///
    /// Mirrors [`crate::QuickJsRuntime::register_img_bitmaps`]: the shell calls it
    /// after `fetch_and_decode_images` so `drawImage(imgElement, …)` can read the
    /// decoded pixels out of [`crate::img_bitmap_store`]. The store is
    /// `thread_local!`, so the writes must happen on the JS thread — hence `run`.
    /// The `Arc` is shared with the shell's decode cache (no pixel copy, BUG-272
    /// срез 20); previous contents are cleared first (navigation-scoped).
    pub fn register_img_bitmaps(&self, bitmaps: Vec<(u32, Arc<lumen_image::Image>)>) {
        self.run(move |_inner| {
            crate::img_bitmap_store::clear_img_bitmaps();
            for (nid, image) in bitmaps {
                crate::img_bitmap_store::set_img_bitmap(nid, image);
            }
        });
    }

    /// BUG-480 срез 3: зарегистрировать загруженный под-документ `<iframe>` для доступа из
    /// JS родителя через `contentWindow`/`contentDocument`
    /// ([`crate::frame_bridge`]).
    ///
    /// Вызывается shell-ом после загрузки ребёнка и **до** диспатча trusted
    /// `load` на хосте — обработчики родителя вправе прочитать фасады прямо из
    /// обработчика. `accessible=false` (cross-origin / opaque sandbox)
    /// регистрирует биндинг без доступа к содержимому: `contentWindow`
    /// существует, `contentDocument` и все нативы чтения пусты.
    ///
    /// BUG-480 срез 19: повторный вызов для ТОГО ЖЕ хоста (навигация фрейма)
    /// ЗАМЕЩАЕТ биндинг на месте, а не добавляет второй. Иначе поиск по
    /// `host_nid` — а он берёт первое совпадение (`_lumen_frame_binding`) —
    /// вечно отдавал бы выброшенный документ, `window.length` рос бы на
    /// каждую навигацию, и `window[i]` разъехался бы с порядком документа.
    /// Сохранение ИНДЕКСА здесь и есть содержательная часть: вложенный
    /// browsing context при навигации остаётся тем же, меняется его документ.
    pub fn register_frame_document(
        &self,
        host_nid: u32,
        doc: Arc<Mutex<lumen_dom::Document>>,
        url: String,
        name: Option<String>,
        accessible: bool,
    ) {
        let registry = Arc::clone(&self.frame_docs);
        let idx = self.run(move |_inner| {
            let mut reg = registry.lock().unwrap_or_else(|e| e.into_inner());
            let binding = crate::frame_bridge::FrameDocBinding {
                host_nid,
                doc,
                url,
                name,
                accessible,
            };
            crate::frame_bridge::upsert_binding(&mut reg, binding)
        });
        // Срез 3: индексный (`window[idx]`) + именованный (`window[имя]`)
        // доступники окна фрейма. Порядок регистрации = порядок документа,
        // поэтому idx совпадает со спечным tree order. Ошибки не фатальны:
        // контекст без window (минимальный тестовый изолят) просто пропустит
        // установку внутри шима.
        if let Err(e) =
            self.eval(&format!("typeof _lumen_frame_install_index === 'function' && _lumen_frame_install_index({idx})"))
        {
            eprintln!("v8: _lumen_frame_install_index({idx}) failed: {e}");
        }
    }

    /// BUG-480 срез 3: зарегистрировать документ **родителя** в JS-контексте
    /// фрейма — после этого `window.parent`/`window.top`/`window.frameElement`
    /// внутри фрейма видят фасады предков ([`crate::frame_bridge`]).
    ///
    /// `host_nid` — nid хоста в дереве родителя (для `frameElement`);
    /// `accessible=false` (cross-origin / opaque sandbox) оставляет окно
    /// предка доступным, но скрывает `.document` и содержимое.
    pub fn register_parent_document(
        &self,
        host_nid: u32,
        doc: Arc<Mutex<lumen_dom::Document>>,
        url: String,
        accessible: bool,
    ) {
        let registry = Arc::clone(&self.frame_docs);
        self.run(move |_inner| {
            registry.lock().unwrap_or_else(|e| e.into_inner()).parent =
                Some(crate::frame_bridge::FrameDocBinding {
                    host_nid,
                    doc,
                    url,
                    name: None,
                    accessible,
                });
        });
        // Срез 3: включить геттеры window.parent/top/frameElement/name. Ошибки
        // не фатальны (контекст без window просто пропустит установку).
        if let Err(e) = self.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        ) {
            eprintln!("v8: _lumen_frame_install_hierarchy failed: {e}");
        }
    }

    /// BUG-480 срез 3: зарегистрировать документ **верха** в JS-контексте
    /// фрейма глубины ≥ 2 — `window.top` должен вести в корень, а не в
    /// непосредственного родителя. У фрейма первого уровня не вызывается:
    /// там top разрешается через слот родителя.
    pub fn register_top_document(
        &self,
        doc: Arc<Mutex<lumen_dom::Document>>,
        url: String,
        accessible: bool,
    ) {
        let registry = Arc::clone(&self.frame_docs);
        self.run(move |_inner| {
            registry.lock().unwrap_or_else(|e| e.into_inner()).top =
                Some(crate::frame_bridge::FrameDocBinding {
                    host_nid: 0,
                    doc,
                    url,
                    name: None,
                    accessible,
                });
        });
        // Слот top ставится только у фреймов глубины ≥ 2, у которых parent
        // уже включил иерархию; повторный вызов идемпотентен.
        if let Err(e) = self.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        ) {
            eprintln!("v8: _lumen_frame_install_hierarchy failed: {e}");
        }
    }

    /// V8 isolate heap statistics: `(total_heap_size, used_heap_size)` in bytes.
    ///
    /// BUG-306 diagnostics: `V8JsRuntime` replaced `QuickJsRuntime` as the only
    /// JS engine (S12b), but `debug_js_heap` (`PersistentJs` trait, consumed by
    /// `LUMEN_MEM_REPORT`) was never wired to the V8 isolate — it silently fell
    /// back to the trait default `(-1, -1)`. `get_heap_statistics` requires
    /// running on the JS thread (mutable isolate access), hence `self.run`.
    pub fn debug_heap_stats(&self) -> (i64, i64) {
        self.run(|inner| {
            let stats = inner.isolate.get_heap_statistics();
            (stats.total_heap_size() as i64, stats.used_heap_size() as i64)
        })
    }

    /// Install the import map (HTML LS §8.1.6.2) used to resolve bare module
    /// specifiers such as `"react"`.
    ///
    /// Call before evaluating module scripts. Mirrors
    /// [`crate::QuickJsRuntime::set_import_map`]; the map is stored on the JS
    /// thread because V8's resolve callback can only reach thread-local state.
    pub fn set_import_map(&self, map: crate::esm::ImportMap) {
        self.run(move |_inner| crate::v8_esm::set_import_map(map));
    }

    /// Point this runtime's ESM loader at `base_url` and give it `provider` as
    /// its network bridge.
    ///
    /// `install_dom` does the same two writes for a page runtime; a **worker**
    /// runtime never goes through `install_dom` at all, so a module worker
    /// (BUG-777) has to set them itself or its `import` would resolve against
    /// an empty base and find no fetcher. Both live in `v8_esm`'s thread-local
    /// state — V8's resolve callback is capture-less and can reach nothing
    /// else — hence `self.run`, which lands on the runtime's own JS thread.
    pub(crate) fn set_module_context(
        &self,
        base_url: &str,
        provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
    ) {
        let base_url = base_url.to_owned();
        self.run(move |_inner| {
            crate::v8_esm::set_page_url(&base_url);
            crate::v8_esm::set_fetch_provider(provider);
        });
    }

    /// Dispatch `f` to the JS thread, blocking until it completes.
    ///
    /// # Safety
    /// `f` may borrow from the caller's stack; we block on `rx.recv()` until the
    /// JS thread executes the job, so every borrow stays live. Erasing `'_` to
    /// `'static` is sound for the same reason as in `QuickJsRuntime::run`.
    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
    pub(super) fn run<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut V8Inner) -> R + Send,
        R: Send,
    {
        let (tx, rx) = std::sync::mpsc::channel::<R>();
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + '_> = Box::new(move |inner| {
            let _ = tx.send(f(inner));
        });
        // SAFETY: we block on rx.recv() below until the JS thread has completed
        // the job. Any borrows captured by `f` (e.g. `&str` args) outlive the
        // execution. The two Box types have identical fat-pointer layout; the
        // transmute only adjusts the lifetime annotation.
        let job: Box<dyn FnOnce(&mut V8Inner) + Send + 'static> = unsafe {
            std::mem::transmute::<
                Box<dyn FnOnce(&mut V8Inner) + Send + '_>,
                Box<dyn FnOnce(&mut V8Inner) + Send + 'static>,
            >(job)
        };
        // Исключение из `clippy::panic` (docs/lint-policy.md §10): `run` возвращает
        // `R`, а не `Result<R>`, и все его вызывающие — тоже. Смерть JS-потока
        // означает, что ответа не будет никогда; следующая строка всё равно
        // паникует на `rx.recv()`. Убрать панику можно только сменой сигнатуры
        // `run` на `Result` — это работа владельца крейта, не правка линта.
        #[allow(clippy::panic)]
        if self.cmd_tx.send(V8Command::Run(job)).is_err() {
            panic!("lumen-v8 thread terminated unexpectedly");
        }
        rx.recv().expect("lumen-v8 thread dropped without replying")
    }

    /// Register one already-wrapped native (built via [`crate::v8_compat::into_v8_fn0`]..`fn7`)
    /// as a global JS function `name`.
    ///
    /// Standalone module ports (S5-S7 batch 2+, one file per Web API) that need
    /// `Function::new`-style natives call this instead of duplicating the
    /// scope/store setup that `install_dom`'s mega-closure does inline for the
    /// natives it owns directly.
    pub(crate) fn register_native(
        &self,
        name: &'static str,
        native: Box<dyn crate::v8_compat::V8NativeFn + Send>,
    ) -> JsResult<()> {
        self.run(move |inner| {
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;
            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);
            register_v8_native(scope, ctx, store, name, native)
        })
    }

    /// Register one already-wrapped scoped native (built via a
    /// [`crate::v8_compat::V8NativeFnScoped`] closure) as a global JS function
    /// `name`. Twin of [`Self::register_native`] for natives that need raw
    /// scope/argument access — currently only the WASM host-import bridge
    /// (Ph3 V8 migration S9).
    pub(crate) fn register_native_scoped(
        &self,
        name: &'static str,
        native: Box<dyn crate::v8_compat::V8NativeFnScoped + Send>,
    ) -> JsResult<()> {
        self.run(move |inner| {
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store_scoped;
            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);
            crate::v8_compat::register_v8_native_scoped(scope, ctx, store, name, native)
        })
    }
}

impl Drop for V8JsRuntime {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(V8Command::Shutdown);
        if let Some(handle) = self.js_thread.take() {
            let _ = handle.join();
        }
    }
}

// ── S2: console native registration ──────────────────────────────────────────

impl V8JsRuntime {
    /// Register the three console natives (`_lumen_console_log`,
    /// `_lumen_console_warn`, `_lumen_console_error`) as global JS functions.
    ///
    /// This is the S2 proof-of-concept that typed Rust closures can be
    /// registered via the compat layer and called from JS with auto-converted
    /// arguments.  S3 will extend this to all 184 `install_primitives` natives.
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn install_console_natives(
        &self,
        console_messages: Arc<std::sync::Mutex<Vec<(u8, String)>>>,
    ) -> JsResult<()> {
        self.run(move |inner| {
            // Disjoint field borrows: scope borrows isolate, native_fn_store is separate.
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;

            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);

            // Local `reg!` macro that mirrors the rquickjs original in dom.rs.
            // Arity 0 and 1 shown as proof; higher arities use into_v8_fn2..7.
            macro_rules! reg {
                ($name:expr, move || $body:expr) => {{
                    let native = into_v8_fn0(move || $body);
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, move |$a:ident: $A:ty| $body:expr) => {{
                    let native = into_v8_fn1(move |$a: $A| $body);
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
            }

            // ── console ──────────────────────────────────────────────────────
            {
                let buf_log = Arc::clone(&console_messages);
                reg!("_lumen_console_log", move |msg: String| {
                    eprintln!("[JS] {msg}");
                    buf_log.lock().unwrap().push((0, msg));
                });
                let buf_warn = Arc::clone(&console_messages);
                reg!("_lumen_console_warn", move |msg: String| {
                    eprintln!("[JS warn] {msg}");
                    buf_warn.lock().unwrap().push((1, msg));
                });
                let buf_err = Arc::clone(&console_messages);
                reg!("_lumen_console_error", move |msg: String| {
                    eprintln!("[JS error] {msg}");
                    buf_err.lock().unwrap().push((2, msg));
                });
            }

            Ok(())
        })
    }
}

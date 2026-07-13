//! V8-based JS runtime (slices S1–S2).
//!
//! **S1** — runtime skeleton: `V8JsRuntime` handle, `V8Inner` thread-owned
//! state, `v8_thread_main` loop, `impl JsRuntime`.
//!
//! **S2** — compat layer: `native_fn_store` in `V8Inner` keeps registered
//! closures alive; `install_console_natives` proves typed closures register
//! and call back from JS.  See `crate::v8_compat` for the full compat API.
//!
//! Mirrors the `QuickJsRuntime` thread-dispatch pattern: a dedicated OS thread
//! owns the `v8::OwnedIsolate` (which is `!Send`); the handle exposes
//! `JsRuntime` methods that dispatch jobs to that thread via a bounded
//! `SyncSender`. Each job runs to completion before the caller unblocks
//! (blocking `recv`), so borrows of the caller's stack are sound via the
//! same `transmute`-lifetime trick used by `QuickJsRuntime::run`.
//!
//! Feature-gated: compiled only when `v8-backend` is enabled.

use crate::dom::{
    FullscreenRequest, HistoryUrlUpdate, NavAction, NavigateRequest, PopupRequest, PrintRequest,
};
use crate::v8_compat::{
    OwnedNativeFn, into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4, into_v8_fn5,
    register_v8_native,
};
use lumen_core::ext::{AbortToken, JsSseEvent, JsWsEvent};
use lumen_core::url::Url;
use lumen_core::{JsError, JsResult, JsRuntime, JsValue, SuspendedHeap};
use lumen_dom::{
    DomPosition, Namespace, NodeData, NodeId, QualName, Range as DomRange, Selection,
    ShadowRootMode, node_child_count, node_length, node_text_content, range_text,
};
use lumen_layout::{matches_selector, query_all};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::{
    Once,
    mpsc::{Sender, SyncSender, sync_channel},
};
use std::thread::JoinHandle;

// ── Platform initialization ───────────────────────────────────────────────────

/// Process-global V8 platform, initialized exactly once.
static V8_INIT: Once = Once::new();

/// Initialize the V8 platform for this process.
///
/// Safe to call multiple times — subsequent calls are no-ops. All code that
/// creates a `v8::Isolate` (including the smoke test in `v8_smoke.rs`) must
/// call this first so there is exactly one `initialize_platform` call.
pub fn ensure_v8_platform() {
    V8_INIT.call_once(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

// ── Thread-local state ────────────────────────────────────────────────────────

/// V8 isolate + global context, owned exclusively by the JS thread.
///
/// Both `OwnedIsolate` and the `Global<Context>` are `!Send`; they are
/// created in [`v8_thread_main`] and never leave it.
///
/// Fields are dropped in declaration order (Rust spec §8.1).  `isolate` is
/// first so the isolate is disposed before the closures in `native_fn_store`
/// are freed — no dangling-pointer access by V8 during teardown.
struct V8Inner {
    /// V8 isolate — disposed first on drop.
    isolate: v8::OwnedIsolate,
    /// Persistent handle to the main JS context.
    context: v8::Global<v8::Context>,
    /// Keeps compat-layer native closures alive for the isolate's lifetime.
    ///
    /// Each entry is a `Box::into_raw(Box::new(f) as Box<Box<dyn V8NativeFn +
    /// Send>>)` thin pointer.  Freed after `isolate` drops.
    native_fn_store: Vec<OwnedNativeFn>,
}

// ── Command channel ───────────────────────────────────────────────────────────

/// A unit of work executed on the JS thread against the live [`V8Inner`].
///
/// The caller blocks until the job completes (`rx.recv()`), so even though
/// the box is `'static` (required by `SyncSender`), it may safely capture
/// borrows from the caller's stack for the duration of the call.
type V8Job = Box<dyn FnOnce(&mut V8Inner) + Send + 'static>;

/// Messages the shell sends to the dedicated V8 JS thread.
enum V8Command {
    /// Run a job against the runtime.
    Run(V8Job),
    /// Shut down the thread and drop the isolate.
    Shutdown,
}

/// Bound for the V8 command queue (same value as `QuickJsRuntime`).
const V8_CMD_QUEUE_BOUND: usize = 64;

// ── Thread entry point ────────────────────────────────────────────────────────

/// Entry point of the dedicated V8 thread.
///
/// Initialises the V8 platform (idempotent), creates the isolate and context,
/// signals the caller via `init_tx`, then services [`V8Command`]s until the
/// channel closes or [`V8Command::Shutdown`] arrives.
fn v8_thread_main(
    cmd_rx: std::sync::mpsc::Receiver<V8Command>,
    init_tx: Sender<Result<(), JsError>>,
) {
    ensure_v8_platform();

    let mut isolate = v8::Isolate::new(Default::default());
    // Create the context inside a short-lived HandleScope so the scope's borrow
    // of `isolate` ends before we move `isolate` into `V8Inner`.
    let context = {
        // scope! pins the HandleScope and gives scope: &mut PinnedRef<HandleScope<'_, ()>>
        v8::scope!(let scope, &mut isolate);
        let ctx = v8::Context::new(scope, Default::default());
        // scope deref-coerces to &Isolate via PinnedRef<HandleScope<'_,()>> → Isolate
        v8::Global::new(scope, ctx)
    };

    let mut inner = V8Inner {
        isolate,
        context,
        native_fn_store: Vec::new(),
    };
    let _ = init_tx.send(Ok(()));

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            V8Command::Run(job) => job(&mut inner),
            V8Command::Shutdown => break,
        }
    }
    // `inner` (OwnedIsolate + Global<Context>) drops here, on its owning thread.
}

// ── Public handle ─────────────────────────────────────────────────────────────

/// V8-backed JS runtime implementing [`JsRuntime`].
///
/// The isolate lives on a dedicated thread; methods block until the dispatched
/// job completes. Cheap to clone via `Arc` if shared access is needed (but
/// callers typically hold one runtime per tab).
pub struct V8JsRuntime {
    /// Channel to the JS thread.
    cmd_tx: SyncSender<V8Command>,
    /// Join handle taken in `Drop` after sending `Shutdown`.
    js_thread: Option<JoinHandle<()>>,
    /// Navigation request written by JS via `location.href=`, `location.assign()` etc.
    /// Captured inside `install_dom`; read by [`Self::take_navigate_request`].
    nav_out: Arc<Mutex<Option<crate::dom::NavigateRequest>>>,
    /// Next timer wakeup deadline as Unix epoch ms (set by `_lumen_request_wakeup`).
    /// `take_timer_wakeup` atomically clears after reading.
    timer_wakeup: Arc<Mutex<Option<f64>>>,
    /// Set to `true` by any DOM-mutating JS binding. Cleared by `take_dom_dirty`.
    dom_dirty: Arc<AtomicBool>,
    /// Set to `true` when JS calls `requestAnimationFrame(fn)`.
    raf_pending: Arc<AtomicBool>,
    /// Layout bounding rects updated after each relayout by the shell.
    /// Maps `NodeId` index (u32) → `[x, y, width, height]` in viewport-relative CSS px.
    layout_rects: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Current viewport size `[width, height]` in CSS px.
    viewport_size: Arc<Mutex<[f32; 2]>>,
    /// Lazy image load requests queued by `_lumen_request_lazy_image_load` from JS.
    lazy_img_requests: Arc<Mutex<Vec<(u32, String)>>>,
    /// Scroll state per scroll-container node, updated after each relayout.
    scroll_states: Arc<Mutex<HashMap<u32, [f32; 4]>>>,
    /// Pending scroll requests queued by JS via `_lumen_request_scroll`.
    pending_scrolls: Arc<Mutex<Vec<(u32, f32, f32)>>>,
    /// Pending page-level scroll requests from JS `window.scrollTo/scrollBy`.
    pending_page_scrolls: Arc<Mutex<Vec<(f32, bool)>>>,
    /// Current page scroll Y exposed to JS `window.scrollY` / `window.pageYOffset`.
    page_scroll_y: Arc<Mutex<f32>>,
    /// Computed CSS styles per node, updated after each relayout by the shell.
    computed_styles: Arc<Mutex<HashMap<u32, HashMap<String, String>>>>,
    /// Pending popup window requests queued by JS `window.open()`.
    window_open_requests: Arc<Mutex<Vec<crate::dom::PopupRequest>>>,
    /// Console messages queued by `console.log/warn/error` calls in JS.
    console_messages: Arc<Mutex<Vec<(u8, String)>>>,
    /// `history.pushState` / `history.replaceState` URL-update notifications.
    pending_history_url_updates: Arc<Mutex<Vec<crate::dom::HistoryUrlUpdate>>>,
    /// `history.go(n)` / `back` / `forward` traversal deltas.
    pending_history_traversals: Arc<Mutex<Vec<i32>>>,
    /// Shell-backed Navigation API state (serialised JSON of nav history + index).
    nav_state: Arc<Mutex<String>>,
    /// Queued by `_lumen_navigation_request`; drained by the shell.
    pending_navigation_updates: Arc<Mutex<Vec<crate::dom::NavUpdate>>>,
    /// Queued by `_lumen_navigation_report_intercept` during `NavigateEvent` dispatch.
    pending_nav_intercepted: Arc<Mutex<Vec<(bool, bool)>>>,
    /// Fullscreen requests emitted by `element.requestFullscreen()` / `document.exitFullscreen()`.
    fullscreen_requests: Arc<Mutex<Vec<crate::dom::FullscreenRequest>>>,
    /// Print requests emitted by `window.print()`.
    print_requests: Arc<Mutex<Vec<crate::dom::PrintRequest>>>,
    /// Focus requests queued by JS via `_lumen_request_focus` / `_lumen_request_blur`.
    pending_focus_requests: Arc<Mutex<Vec<Option<u32>>>>,
    /// Deterministic render mode (8F): when `true`, `Date.now()`/`Math.random` are frozen/seeded.
    deterministic: AtomicBool,
    /// Live SW execution threads keyed by `(origin, scope)`.
    sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
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
            raf_pending: Arc::new(AtomicBool::new(false)),
            layout_rects: Arc::new(Mutex::new(HashMap::new())),
            viewport_size: Arc::new(Mutex::new([0.0, 0.0])),
            lazy_img_requests: Arc::new(Mutex::new(Vec::new())),
            scroll_states: Arc::new(Mutex::new(HashMap::new())),
            pending_scrolls: Arc::new(Mutex::new(Vec::new())),
            pending_page_scrolls: Arc::new(Mutex::new(Vec::new())),
            page_scroll_y: Arc::new(Mutex::new(0.0)),
            computed_styles: Arc::new(Mutex::new(HashMap::new())),
            window_open_requests: Arc::new(Mutex::new(Vec::new())),
            console_messages: Arc::new(Mutex::new(Vec::new())),
            pending_history_url_updates: Arc::new(Mutex::new(Vec::new())),
            pending_history_traversals: Arc::new(Mutex::new(Vec::new())),
            nav_state: Arc::new(Mutex::new(String::from(r#"{"entries":[],"index":0}"#))),
            pending_navigation_updates: Arc::new(Mutex::new(Vec::new())),
            pending_nav_intercepted: Arc::new(Mutex::new(Vec::new())),
            fullscreen_requests: Arc::new(Mutex::new(Vec::new())),
            print_requests: Arc::new(Mutex::new(Vec::new())),
            pending_focus_requests: Arc::new(Mutex::new(Vec::new())),
            deterministic: AtomicBool::new(false),
            sw_worker_store: None,
        })
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

    /// Enable or disable deterministic render mode (8F) before calling `install_dom`.
    pub fn set_deterministic_mode(&self, on: bool) {
        self.deterministic
            .store(on, std::sync::atomic::Ordering::Relaxed);
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

    /// Dispatch `f` to the JS thread, blocking until it completes.
    ///
    /// # Safety
    /// `f` may borrow from the caller's stack; we block on `rx.recv()` until the
    /// JS thread executes the job, so every borrow stays live. Erasing `'_` to
    /// `'static` is sound for the same reason as in `QuickJsRuntime::run`.
    fn run<R, F>(&self, f: F) -> R
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
        if self.cmd_tx.send(V8Command::Run(job)).is_err() {
            panic!("lumen-v8 thread terminated unexpectedly");
        }
        rx.recv().expect("lumen-v8 thread dropped without replying")
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

// ── S3: DOM-core native registration ─────────────────────────────────────────
//
// Ports `dom::install_primitives`'s 184 `_lumen_*` natives (rquickjs) to the V8
// compat layer. Scoped to DOM-core only, mirroring `dom::install_dom_api` minus
// the WebGL/Canvas2D/OffscreenCanvas/AudioContext installs that
// `QuickJsRuntime::install_dom` also performs — those, plus Highlight/Battery/
// Navigator-normalization/CSS-Houdini/SubtleCrypto/TrustedTypes, are separate
// future slices that each need their own ctx-taking install fn ported.

impl V8JsRuntime {
    /// Install DOM-core native bindings (`_lumen_*`, 184 functions) and the
    /// `WEB_API_SHIM` JavaScript that builds `document`, `window`, `console`,
    /// `location`, `navigator`, `fetch`, `WebSocket`, `localStorage`, and
    /// `sessionStorage` on top of them.
    ///
    /// Mirrors [`crate::QuickJsRuntime::install_dom`] but scoped to the DOM-core
    /// piece only (`dom::install_dom_api`'s `install_primitives` + shim eval).
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn install_dom(
        &self,
        doc: Arc<Mutex<lumen_dom::Document>>,
        page_url: &str,
        fetch_provider: Option<Arc<dyn lumen_core::ext::JsFetchProvider>>,
        ws_provider: Option<Arc<dyn lumen_core::ext::JsWebSocketProvider>>,
        sse_provider: Option<Arc<dyn lumen_core::ext::JsSseProvider>>,
        ls_store: Option<Arc<Mutex<lumen_core::WebStorage>>>,
        idb_backend: Option<Arc<dyn lumen_core::ext::IdbBackend>>,
        sw_backend: Option<Arc<dyn lumen_core::ext::SwBackend>>,
        cache_backend: Option<Arc<dyn lumen_core::ext::CacheBackend>>,
        sw_worker_store: Option<lumen_core::ext::SwWorkerStore>,
        cross_origin_isolated: bool,
    ) -> JsResult<()> {
        let ls_store =
            ls_store.unwrap_or_else(|| Arc::new(Mutex::new(lumen_core::WebStorage::default())));
        let ss_store: Arc<Mutex<lumen_core::WebStorage>> =
            Arc::new(Mutex::new(lumen_core::WebStorage::default()));
        // PH3-20: an explicit `sw_worker_store` argument takes precedence over a
        // store set earlier via a builder (mirrors `QuickJsRuntime::install_dom`).
        let sw_worker_store = sw_worker_store.or_else(|| self.sw_worker_store.clone());
        // Cookie access is not part of the S3 DOM-core signature; document.cookie
        // reads/writes as empty until a future slice threads a CookieProvider through.
        let cookie_jar: Option<Arc<dyn lumen_core::ext::CookieProvider>> = None;
        let deterministic_seed = if self
            .deterministic
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            Some(crate::deterministic_seed_from_url(page_url))
        } else {
            None
        };
        let page_url = page_url.to_owned();

        self.run(move |inner| {
            // Disjoint field borrows: scope borrows isolate, native_fn_store is separate.
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;

            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);

            // Local `reg!` macro mirroring the rquickjs original's call-site syntax
            // (see `dom.rs::install_primitives`'s `reg!` macro) so the 184 native
            // bodies below could be ported with an (almost) mechanical copy: each
            // arm matches one arity (0-7) in both the implicit-return (`$body:expr`)
            // and explicit-return (`-> $R:ty $body:block`) closure forms, with an
            // optional leading `move` (some natives below capture nothing and were
            // written without it in the rquickjs original).
            macro_rules! reg {
                // arity 0
                ($name:expr, $(move)? || -> $R:ty $body:block) => {{
                    let native = into_v8_fn0(move || -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? || $body:expr) => {{
                    let native = into_v8_fn0(move || { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                // arity 1
                ($name:expr, $(move)? |$a:ident: $A:ty| -> $R:ty $body:block) => {{
                    let native = into_v8_fn1(move |$a: $A| -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? |$a:ident: $A:ty| $body:expr) => {{
                    let native = into_v8_fn1(move |$a: $A| { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                // arity 2
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty| -> $R:ty $body:block) => {{
                    let native = into_v8_fn2(move |$a: $A, $b: $B| -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty| $body:expr) => {{
                    let native = into_v8_fn2(move |$a: $A, $b: $B| { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                // arity 3
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty| -> $R:ty $body:block) => {{
                    let native = into_v8_fn3(move |$a: $A, $b: $B, $c: $C| -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty| $body:expr) => {{
                    let native = into_v8_fn3(move |$a: $A, $b: $B, $c: $C| { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                // arity 4
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty| -> $R:ty $body:block) => {{
                    let native = into_v8_fn4(move |$a: $A, $b: $B, $c: $C, $d: $D| -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty| $body:expr) => {{
                    let native = into_v8_fn4(move |$a: $A, $b: $B, $c: $C, $d: $D| { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                // arity 5
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty| -> $R:ty $body:block) => {{
                    let native = into_v8_fn5(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E| -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty| $body:expr) => {{
                    let native = into_v8_fn5(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E| { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                // arity 6
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty| -> $R:ty $body:block) => {{
                    let native = into_v8_fn6(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G| -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty| $body:expr) => {{
                    let native = into_v8_fn6(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G| { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                // arity 7
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty, $h:ident: $H:ty| -> $R:ty $body:block) => {{
                    let native = into_v8_fn7(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G, $h: $H| -> $R { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
                ($name:expr, $(move)? |$a:ident: $A:ty, $b:ident: $B:ty, $c:ident: $C:ty, $d:ident: $D:ty, $e:ident: $E:ty, $g:ident: $G:ty, $h:ident: $H:ty| $body:expr) => {{
                    let native = into_v8_fn7(move |$a: $A, $b: $B, $c: $C, $d: $D, $e: $E, $g: $G, $h: $H| { $body });
                    register_v8_native(scope, ctx, store, $name, native)?;
                }};
            }

            let nav_out = Arc::clone(&self.nav_out);
            let timer_wakeup = Arc::clone(&self.timer_wakeup);
            let dom_dirty = Arc::clone(&self.dom_dirty);
            let raf_pending = Arc::clone(&self.raf_pending);
            let layout_rects = Arc::clone(&self.layout_rects);
            let viewport_size = Arc::clone(&self.viewport_size);
            let lazy_img_requests = Arc::clone(&self.lazy_img_requests);
            let scroll_states = Arc::clone(&self.scroll_states);
            let pending_scrolls = Arc::clone(&self.pending_scrolls);
            let pending_page_scrolls = Arc::clone(&self.pending_page_scrolls);
            let page_scroll_y = Arc::clone(&self.page_scroll_y);
            let computed_styles = Arc::clone(&self.computed_styles);
            let window_open_requests = Arc::clone(&self.window_open_requests);
            let console_messages = Arc::clone(&self.console_messages);
            let pending_history_url_updates = Arc::clone(&self.pending_history_url_updates);
            let pending_history_traversals = Arc::clone(&self.pending_history_traversals);
            let nav_state = Arc::clone(&self.nav_state);
            let pending_navigation_updates = Arc::clone(&self.pending_navigation_updates);
            let pending_nav_intercepted = Arc::clone(&self.pending_nav_intercepted);
            let fullscreen_requests = Arc::clone(&self.fullscreen_requests);
            let print_requests = Arc::clone(&self.print_requests);
            let pending_focus_requests = Arc::clone(&self.pending_focus_requests);

    // ── console ──────────────────────────────────────────────────────────────
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

    // ── window.print() (W-2) ──────────────────────────────────────────────────
    {
        let pr = Arc::clone(&print_requests);
        reg!("_lumen_print_dialog", move || {
            eprintln!("[window.print()] Opening print preview dialog");
            pr.lock().unwrap().push(PrintRequest::default());
        });
    }

    // ── dialog focus management (HTML LS §6.6.3) ─────────────────────────────
    // `showModal()` calls `_lumen_request_focus(nid)` to focus the first autofocus
    // element (or the dialog itself).  `close()` calls `_lumen_request_focus(prev)`
    // to restore focus to the element that was active before the dialog opened.
    // The shell drains these via `take_focus_requests()` after each JS pump.
    {
        let pfr = Arc::clone(&pending_focus_requests);
        reg!("_lumen_request_focus", move |nid: u32| {
            pfr.lock().unwrap().push(Some(nid));
        });
        let pfr2 = Arc::clone(&pending_focus_requests);
        reg!("_lumen_request_blur", move || {
            pfr2.lock().unwrap().push(None);
        });
    }

    // ── document meta ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_document_root", move || -> u32 {
            let doc = d.lock().unwrap();
            doc.root().index() as u32
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_get_body", move || -> Option<u32> {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "body").map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_get_document_title", move || -> String {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "title")
                .map(|nid| collect_text_content(&doc, nid))
                .unwrap_or_default()
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_set_document_title", move |text: String| {
            let mut doc = d.lock().unwrap();
            if let Some(title_id) = find_element_by_tag(&doc, "title") {
                set_text_content(&mut doc, title_id, &text);
            }
        });
    }

    // ── document.fonts (FontFaceSet) ──────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_size", move || -> u32 {
            let doc = d.lock().unwrap();
            doc.fonts().size() as u32
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_get", move |idx: u32| -> Option<String> {
            let doc = d.lock().unwrap();
            doc.fonts().all().get(idx as usize).map(|face| {
                // Serialize FontFace to JSON manually
                let family_esc = face.family.replace('\\', "\\\\").replace('"', "\\\"");
                let style_esc = face.style.replace('\\', "\\\\").replace('"', "\\\"");
                let weight_esc = face.weight.replace('\\', "\\\\").replace('"', "\\\"");
                let stretch_esc = face.stretch.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let unicode_range_esc = face.unicode_range.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let src_esc = face.src.replace('\\', "\\\\").replace('"', "\\\"");
                let status_str = match face.status {
                    lumen_dom::FontFaceStatus::Unloaded => "unloaded",
                    lumen_dom::FontFaceStatus::Loading => "loading",
                    lumen_dom::FontFaceStatus::Loaded => "loaded",
                    lumen_dom::FontFaceStatus::Error => "error",
                };
                format!(
                    r#"{{"family":"{family_esc}","style":"{style_esc}","weight":"{weight_esc}","stretch":{stretch_json},"unicodeRange":{unicode_json},"src":"{src_esc}","status":"{status_str}"}}"#,
                    stretch_json = if face.stretch.is_some() { format!(r#""{}""#, stretch_esc) } else { "null".to_string() },
                    unicode_json = if face.unicode_range.is_some() { format!(r#""{}""#, unicode_range_esc) } else { "null".to_string() }
                )
            })
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_get_by_family", move |family: String| -> Vec<String> {
            let doc = d.lock().unwrap();
            doc.fonts().get_by_family(&family).iter().map(|face| {
                let family_esc = face.family.replace('\\', "\\\\").replace('"', "\\\"");
                let style_esc = face.style.replace('\\', "\\\\").replace('"', "\\\"");
                let weight_esc = face.weight.replace('\\', "\\\\").replace('"', "\\\"");
                let stretch_esc = face.stretch.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let unicode_range_esc = face.unicode_range.as_ref().map(|s| s.replace('\\', "\\\\").replace('"', "\\\"")).unwrap_or_default();
                let src_esc = face.src.replace('\\', "\\\\").replace('"', "\\\"");
                let status_str = match face.status {
                    lumen_dom::FontFaceStatus::Unloaded => "unloaded",
                    lumen_dom::FontFaceStatus::Loading => "loading",
                    lumen_dom::FontFaceStatus::Loaded => "loaded",
                    lumen_dom::FontFaceStatus::Error => "error",
                };
                format!(
                    r#"{{"family":"{family_esc}","style":"{style_esc}","weight":"{weight_esc}","stretch":{stretch_json},"unicodeRange":{unicode_json},"src":"{src_esc}","status":"{status_str}"}}"#,
                    stretch_json = if face.stretch.is_some() { format!(r#""{}""#, stretch_esc) } else { "null".to_string() },
                    unicode_json = if face.unicode_range.is_some() { format!(r#""{}""#, unicode_range_esc) } else { "null".to_string() }
                )
            }).collect()
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_fonts_has_family", move |family: String| -> bool {
            let doc = d.lock().unwrap();
            doc.fonts().has_family(&family)
        });
    }

    // ── node lookup ──────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_element_by_id", move |id: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            find_first_matching(&doc, doc.root(), &|node| {
                matches!(&node.data, NodeData::Element { .. })
                    && node.get_attr("id") == Some(id.as_str())
            })
            .map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_query_selector", move |sel: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            query_all(&doc, &sel).into_iter().next().map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_query_selector_all",
            move |sel: String| -> Vec<u32> {
                let doc = d.lock().unwrap();
                query_all(&doc, &sel)
                    .into_iter()
                    .map(|n| n.index() as u32)
                    .collect()
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_node_matches_selector",
            move |node_id: u32, sel: String| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches_selector(&doc, nid, &sel)
            }
        );
    }

    // ── node properties ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_tag_name", move |node_id: u32| -> String {
            let doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            match &doc.get(nid).data {
                NodeData::Element { name, .. } => name.local.to_ascii_uppercase(),
                NodeData::Text(_) => "#text".into(),
                NodeData::Document => "#document".into(),
                NodeData::Comment(_) => "#comment".into(),
                NodeData::Doctype { .. } => "html".into(),
                NodeData::ShadowRoot { .. } => "#shadow-root".into(),
                NodeData::DocumentFragment => "#document-fragment".into(),
            }
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_is_text_node",
            move |node_id: u32| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches!(doc.get(nid).data, NodeData::Text(_))
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_attr",
            move |node_id: u32, name: String| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).get_attr(&name).map(|s| s.to_string())
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_attr",
            move |node_id: u32, name: String, value: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_attribute(&mut doc, nid, &name, &value);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_remove_attr", move |node_id: u32, name: String| {
            let mut doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            remove_attribute(&mut doc, nid, &name);
            dirty.store(true, Ordering::Relaxed);
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_attr_names",
            move |node_id: u32| -> Vec<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                match &doc.get(nid).data {
                    NodeData::Element { attrs, .. } => {
                        attrs.iter().map(|a| a.name.local.to_string()).collect()
                    }
                    _ => Vec::new(),
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_text_content",
            move |node_id: u32| -> String {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                collect_text_content(&doc, nid)
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_text_content",
            move |node_id: u32, text: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_text_content(&mut doc, nid, &text);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_inner_html",
            move |node_id: u32| -> String {
                // Phase 0: return text content only (no HTML serialization).
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                collect_text_content(&doc, nid)
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_inner_html",
            move |node_id: u32, html: String| {
                // Phase 0: treat innerHTML as plain text (no fragment parsing).
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_text_content(&mut doc, nid, &html);
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }

    // ── tree navigation ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_children",
            move |node_id: u32| -> Vec<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid)
                    .children
                    .iter()
                    .map(|c| c.index() as u32)
                    .collect()
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_parent",
            move |node_id: u32| -> Option<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).parent.map(|p| p.index() as u32)
            }
        );
    }

    // ── DOM node count ───────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_dom_node_count",
            move || -> u32 {
                d.lock().unwrap().node_count() as u32
            }
        );
    }

    // ── tree mutation ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_create_element",
            move |tag: String| -> u32 {
                let mut doc = d.lock().unwrap();
                // Returns u32::MAX when MAX_DOM_NODES is reached; JS shim handles this.
                match doc.try_create_element(QualName::html(tag.to_ascii_lowercase())) {
                    Ok(nid) => nid.index() as u32,
                    Err(_) => u32::MAX,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_create_element_ns",
            move |ns: String, local: String| -> u32 {
                let mut doc = d.lock().unwrap();
                // Foreign-content namespace selection. SVG keeps the local name's
                // original case (case-sensitive tags like `linearGradient`); all
                // other namespaces fall back to HTML. Returns u32::MAX on overflow.
                let namespace = if ns == "http://www.w3.org/2000/svg" {
                    Namespace::Svg
                } else {
                    Namespace::Html
                };
                match doc.try_create_element(QualName { namespace, local }) {
                    Ok(nid) => nid.index() as u32,
                    Err(_) => u32::MAX,
                }
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_create_text_node",
            move |text: String| -> u32 {
                let mut doc = d.lock().unwrap();
                let nid = doc.create_text(text);
                nid.index() as u32
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_append_child",
            move |parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let parent = NodeId::from_index(parent_id as usize);
                let child = NodeId::from_index(child_id as usize);
                doc.append_child(parent, child);
                dirty.store(true, Ordering::Relaxed);
            }
        );
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_remove_child",
            move |_parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let child = NodeId::from_index(child_id as usize);
                doc.detach(child);
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }

    // ── Service Worker / Cache Storage ───────────────────────────────────────
    {
        // SW registrations: origin+scope+scriptUrl stored in-memory.
        // Key: (origin, scope) → script_url
        type SwMap = std::collections::HashMap<(String, String), String>;
        let sw_regs: Arc<Mutex<SwMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        // Cache storage: origin → cache_name → url → (method, meta_json, body)
        // meta_json: {"method":"GET","status":200,"statusText":"OK","headers":{…}}
        // method is stored separately for O(1) `keys()` without re-parsing meta_json.
        type CacheEntry = (String, String, Vec<u8>);
        type CacheMap = std::collections::HashMap<String, std::collections::HashMap<String, std::collections::HashMap<String, CacheEntry>>>;
        let cache_data: Arc<Mutex<CacheMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_register",
            move |origin: String, scope: String, script_url: String| {
                sw.lock().unwrap().insert((origin, scope), script_url);
            }
        );

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_has_registration",
            move |origin: String| -> bool {
                sw.lock().unwrap().keys().any(|(o, _)| *o == origin)
            }
        );

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_unregister",
            move |origin: String, scope: String| {
                sw.lock().unwrap().remove(&(origin, scope));
            }
        );

        // Persistence bindings — forward to SwBackend when provided.
        let sw_be = sw_backend.clone();
        reg!(
            "_lumen_sw_persist",
            move |_origin: String, snapshot: String| {
                if let Some(ref be) = sw_be {
                    be.save(&snapshot);
                }
            }
        );

        let sw_be2 = sw_backend.clone();
        reg!(
            "_lumen_sw_load",
            move |_origin: String| -> Option<String> {
                sw_be2.as_ref().and_then(|be| be.load())
            }
        );

        // _lumen_sw_activate_script(origin, scope, script_text) — PH3-20: SW fetch interception.
        // Called from the _sw_run_lifecycle JS shim when a SW finishes the activate phase.
        // Spawns a dedicated QuickJS thread for the SW and registers it in sw_worker_store.
        {
            let sws = sw_worker_store.clone();
            let cbe_sw = cache_backend.clone();
            reg!("_lumen_sw_activate_script", move |origin: String, scope: String, text: String| {
                if let (Some(store), Some(cache)) = (sws.as_ref(), cbe_sw.as_ref()) {
                    let handle = crate::sw_worker::spawn_sw_worker(
                        origin.clone(),
                        scope.clone(),
                        text,
                        Arc::clone(cache),
                    );
                    store.lock().unwrap().insert((origin, scope), handle);
                }
            });
        }

        // Dispatch helpers: use SQLite backend when provided, fall back to in-memory map.
        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_put",
            // meta_json: {"method":"GET","status":200,"statusText":"OK","headers":{...}}
            // Grouped into one string to stay within rquickjs 5-arg IntoJsFunc limit.
            move |origin: String, cache_name: String, url: String, meta_json: String, body: Vec<u8>| {
                if let Some(ref be) = cbe {
                    be.cache_put(&origin, &cache_name, &url, &meta_json, &body);
                } else {
                    let method = cache_meta_method(&meta_json);
                    cd.lock()
                        .unwrap()
                        .entry(origin)
                        .or_default()
                        .entry(cache_name)
                        .or_default()
                        .insert(url, (method, meta_json, body));
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match",
            move |origin: String, cache_name: String, url: String| -> Option<Vec<u8>> {
                if let Some(ref be) = cbe {
                    be.cache_match(&origin, &cache_name, &url).map(|(_, body)| body)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .and_then(|cache| cache.get(&url))
                        .map(|(_, _, body)| body.clone())
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match_info",
            // Returns the raw meta_json stored at put time (already JSON-encoded).
            move |origin: String, cache_name: String, url: String| -> Option<String> {
                if let Some(ref be) = cbe {
                    be.cache_match(&origin, &cache_name, &url).map(|(meta, _)| meta)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .and_then(|cache| cache.get(&url))
                        .map(|(_, meta, _)| meta.clone())
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match_any",
            move |origin: String, url: String| -> Option<Vec<u8>> {
                if let Some(ref be) = cbe {
                    be.cache_match_any(&origin, &url).map(|(_, body)| body)
                } else {
                    let guard = cd.lock().unwrap();
                    let caches = guard.get(&origin)?;
                    for cache in caches.values() {
                        if let Some((_, _, body)) = cache.get(&url) {
                            return Some(body.clone());
                        }
                    }
                    None
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match_any_info",
            move |origin: String, url: String| -> Option<String> {
                if let Some(ref be) = cbe {
                    be.cache_match_any(&origin, &url).map(|(meta, _)| meta)
                } else {
                    let guard = cd.lock().unwrap();
                    let caches = guard.get(&origin)?;
                    for cache in caches.values() {
                        if let Some((_, meta, _)) = cache.get(&url) {
                            return Some(meta.clone());
                        }
                    }
                    None
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_delete",
            move |origin: String, cache_name: String, url: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_delete(&origin, &cache_name, &url)
                } else {
                    let mut guard = cd.lock().unwrap();
                    if let Some(caches) = guard.get_mut(&origin)
                        && let Some(cache) = caches.get_mut(&cache_name)
                    {
                        cache.remove(&url).is_some()
                    } else {
                        false
                    }
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_keys",
            move |origin: String, cache_name: String| -> Vec<String> {
                if let Some(ref be) = cbe {
                    be.cache_keys(&origin, &cache_name).into_iter().map(|(u, _)| u).collect()
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .and_then(|caches| caches.get(&cache_name))
                        .map(|cache| cache.keys().cloned().collect())
                        .unwrap_or_default()
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_keys_full",
            move |origin: String, cache_name: String| -> String {
                if let Some(ref be) = cbe {
                    let pairs = be.cache_keys(&origin, &cache_name);
                    let items: Vec<String> = pairs
                        .iter()
                        .map(|(url, method)| format!(r#"{{"url":"{url}","method":"{method}"}}"#))
                        .collect();
                    format!("[{}]", items.join(","))
                } else {
                    let guard = cd.lock().unwrap();
                    match guard.get(&origin).and_then(|c| c.get(&cache_name)) {
                        None => "[]".to_string(),
                        Some(cache) => {
                            let items: Vec<String> = cache
                                .iter()
                                .map(|(url, (method, _, _))| {
                                    format!(r#"{{"url":"{url}","method":"{method}"}}"#)
                                })
                                .collect();
                            format!("[{}]", items.join(","))
                        }
                    }
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_has",
            move |origin: String, cache_name: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_has(&origin, &cache_name)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .map(|caches| caches.contains_key(&cache_name))
                        .unwrap_or(false)
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_delete_cache",
            move |origin: String, cache_name: String| -> bool {
                if let Some(ref be) = cbe {
                    be.cache_delete_cache(&origin, &cache_name)
                } else if let Some(caches) = cd.lock().unwrap().get_mut(&origin) {
                    caches.remove(&cache_name).is_some()
                } else {
                    false
                }
            }
        );

        let cbe = cache_backend.clone();
        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_names",
            move |origin: String| -> Vec<String> {
                if let Some(ref be) = cbe {
                    be.cache_names(&origin)
                } else {
                    cd.lock()
                        .unwrap()
                        .get(&origin)
                        .map(|caches| caches.keys().cloned().collect())
                        .unwrap_or_default()
                }
            }
        );
    }

    // ── history ──────────────────────────────────────────────────────────────
    {
        let hist = Arc::new(Mutex::new(HistoryState::new()));

        let h = Arc::clone(&hist);
        reg!(
            "_lumen_history_push",
            move |state_json: String, url: String| {
                h.lock().unwrap().push(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!(
            "_lumen_history_replace",
            move |state_json: String, url: String| {
                h.lock().unwrap().replace(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!("_lumen_history_go", move |delta: i32| -> bool {
            h.lock().unwrap().go(delta)
        });

        // Queue a real session-history traversal for the shell. `history.go(n)` /
        // `back` / `forward` call this so the shell (single authority) moves its
        // `nav_back`/`nav_fwd` stacks by `delta` and delivers the destination
        // popstate or reload — the JS `HistoryState` above is only a read-cache.
        let t = Arc::clone(&pending_history_traversals);
        reg!("_lumen_history_traverse", move |delta: i32| {
            t.lock().unwrap().push(delta);
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_set_state", move |state_json: String| {
            h.lock().unwrap().set_state(state_json)
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_length", move || -> u32 {
            h.lock().unwrap().length()
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_state_json", move || -> String {
            h.lock().unwrap().state_json().to_string()
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_url", move || -> String {
            h.lock().unwrap().url().to_string()
        });

        // Notify shell of pushState/replaceState URL changes so the address bar
        // can be updated without a page reload.  Called from history.pushState /
        // history.replaceState in WEB_API_SHIM after the JS HistoryState is updated.
        let q = Arc::clone(&pending_history_url_updates);
        reg!(
            "_lumen_history_push_url",
            move |url: String, new_state_json: String| {
                q.lock()
                    .unwrap()
                    .push(HistoryUrlUpdate::Push { url, new_state_json });
            }
        );

        let q = Arc::clone(&pending_history_url_updates);
        reg!(
            "_lumen_history_replace_url",
            move |url: String, new_state_json: String| {
                q.lock()
                    .unwrap()
                    .push(HistoryUrlUpdate::Replace { url, new_state_json });
            }
        );
    }

    // ── Navigation API ──────────────────────────────────────────────────────────
    // Shell-backed Navigation API.  All mutations are queued via
    // `pending_navigation_updates`; the shell drains them in `about_to_wait`
    // and is the single authority for the nav_back / nav_fwd stacks.
    {
        let ns_entries = Arc::clone(&nav_state);
        let ns_index   = Arc::clone(&nav_state);
        let ns_back    = Arc::clone(&nav_state);
        let ns_fwd     = Arc::clone(&nav_state);
        let ns_set     = Arc::clone(&nav_state);
        let q          = Arc::clone(&pending_navigation_updates);
        let pi         = Arc::clone(&pending_nav_intercepted);

        // ── accessors (read nav_state JSON, locked only for copy) ────────────────
        reg!(
            "_lumen_navigation_entries_json",
            move || -> String {
                ns_entries.lock().map(|s| s.clone()).unwrap_or_default()
            }
        );

        reg!(
            "_lumen_navigation_current_index",
            move || -> i32 {
                ns_index.lock()
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .map(|v: serde_json::Value| v.get("index").and_then(|i| i.as_i64()).unwrap_or(0) as i32)
                    .unwrap_or(0)
            }
        );

        reg!(
            "_lumen_navigation_can_go_back",
            move || -> bool {
                ns_back.lock()
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .map(|v: serde_json::Value| {
                        let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                        let len = v.get("entries").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0);
                        idx > 0 && len > 0
                    })
                    .unwrap_or(false)
            }
        );

        reg!(
            "_lumen_navigation_can_go_forward",
            move || -> bool {
                ns_fwd.lock()
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .map(|v: serde_json::Value| {
                        let idx = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                        let len = v.get("entries").and_then(|e| e.as_array()).map(|a| a.len()).unwrap_or(0);
                        idx + 1 < (len as u64)
                    })
                    .unwrap_or(false)
            }
        );

        // ── state setter (called from shell via eval_js) ─────────────────────────
        reg!(
            "_lumen_navigation_set_state",
            move |json: String| {
                *ns_set.lock().unwrap() = json;
            }
        );

        // ── navigation action queue ──────────────────────────────────────────────
        reg!(
            "_lumen_navigation_report_intercept",
            move |intercepted: bool, cancelled: bool| {
                let mut q = pi.lock().unwrap();
                q.push((intercepted, cancelled));
            }
        );

        reg!(
            "_lumen_navigation_request",
            move |action_code: u8, url: String, key: String, data: String| {
                let action = match action_code {
                    0 => NavAction::Push,
                    1 => NavAction::Replace,
                    2 => NavAction::Back,
                    3 => NavAction::Forward,
                    4 => NavAction::TraverseTo,
                    5 => NavAction::Reload,
                    6 => NavAction::InterceptedSuccess,
                    7 => NavAction::InterceptedError,
                    _ => return,
                };
                q.lock().unwrap().push((action, url, key, data));
            }
        );
    }

    // ── navigation (location.href =, assign, replace, reload) ────────────────
    {
        let nav = Arc::clone(&nav_out);
        reg!("_lumen_navigate", move |url: String, replace: bool| {
            *nav.lock().unwrap() = Some(if replace {
                NavigateRequest::Replace(url)
            } else {
                NavigateRequest::Push(url)
            });
        });

        let nav = Arc::clone(&nav_out);
        reg!("_lumen_reload", move || {
            *nav.lock().unwrap() = Some(NavigateRequest::Reload);
        });
    }

    // ── Fetch API ─────────────────────────────────────────────────────────────
    {
        struct FetchCache {
            status: u16,
            status_text: String,
            headers: Vec<String>, // flat: [name, value, name, value, ...]
            body: Vec<u8>,
        }

        let cache: Arc<Mutex<Option<FetchCache>>> = Arc::new(Mutex::new(None));

        let fp2 = fetch_provider.clone();
        let fp_beacon = fetch_provider.clone();
        let fp_cancel = fetch_provider.clone();
        let fp_cancel_body = fetch_provider.clone();
        let c_cancel = Arc::clone(&cache);
        let c_cancel_body = Arc::clone(&cache);
        let fp_async = fetch_provider.clone();
        let c_async = Arc::clone(&cache);
        let (fp, c) = (fetch_provider, Arc::clone(&cache));
        reg!("_lumen_fetch_sync", move |url: String, method: String| -> bool {
            let Some(ref provider) = fp else { return false };
            match provider.fetch_sync(&url, &method) {
                Ok(resp) => {
                    let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                    for (k, v) in resp.headers {
                        flat.push(k);
                        flat.push(v);
                    }
                    *c.lock().unwrap() = Some(FetchCache {
                        status: resp.status,
                        status_text: resp.status_text,
                        headers: flat,
                        body: resp.body,
                    });
                    true
                }
                Err(e) => {
                    eprintln!("fetch error: {e}");
                    false
                }
            }
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_status", move || -> u32 {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or(0, |r| u32::from(r.status))
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_status_text", move || -> String {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(String::new, |r| r.status_text.clone())
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_headers", move || -> Vec<String> {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(Vec::new, |r| r.headers.clone())
        });

        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_get_body", move || -> Vec<u8> {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or_else(Vec::new, |r| r.body.clone())
        });

        // _lumen_fetch_body_length() → u32
        // Returns the byte length of the most recent cached response body.
        // Used by the pull()-based ReadableStream in Response.body to avoid
        // copying the full body into JS memory at construction time.
        let c = Arc::clone(&cache);
        reg!("_lumen_fetch_body_length", move || -> u32 {
            c.lock()
                .unwrap()
                .as_ref()
                .map_or(0, |r| r.body.len() as u32)
        });

        // _lumen_fetch_body_chunk(offset: u32, size: u32) → Vec<u8>
        // Returns bytes [offset .. offset+size] of the cached response body.
        // Called repeatedly by Response.body.pull() to stream large responses
        // without loading the entire body into JS at once (Fetch Standard §2.2).
        let c = Arc::clone(&cache);
        reg!(
            "_lumen_fetch_body_chunk",
            move |offset: u32, size: u32| -> Vec<u8> {
                let guard = c.lock().unwrap();
                let body = guard.as_ref().map_or(&[] as &[u8], |r| r.body.as_slice());
                let start = (offset as usize).min(body.len());
                let end = (start + size as usize).min(body.len());
                body[start..end].to_vec()
            }
        );

        // _lumen_check_sri_integrity(integrity) → bool
        // Verifies the cached response body against the SRI `integrity` string
        // (W3C SRI §3.3.5). Must be called after _lumen_fetch_sync / _lumen_fetch_sync_with_body
        // and before reading the body. Returns true if integrity is empty or passes.
        {
            let c_sri = Arc::clone(&cache);
            reg!("_lumen_check_sri_integrity", move |integrity: String| -> bool {
                let guard = c_sri.lock().unwrap();
                let body = guard.as_ref().map_or(&[] as &[u8], |r| r.body.as_slice());
                crate::sri::check_sri(body, &integrity)
            });
        }

        // _lumen_fetch_sync_with_body(url, method, content_type, body_bytes) → bool
        // Used by fetch() when init.body is present (FormData, string, ArrayBuffer).
        // Shares the same FetchCache slot as _lumen_fetch_sync.
        {
            let fetch_provider2 = fp2;
            let c2 = Arc::clone(&cache);
            reg!(
                "_lumen_fetch_sync_with_body",
                move |url: String, method: String, content_type: String, body: Vec<u8>| -> bool {
                    let Some(ref provider) = fetch_provider2 else {
                        return false;
                    };
                    match provider.fetch_with_body_sync(&url, &method, &content_type, &body) {
                        Ok(resp) => {
                            let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                            for (k, v) in resp.headers {
                                flat.push(k);
                                flat.push(v);
                            }
                            *c2.lock().unwrap() = Some(FetchCache {
                                status: resp.status,
                                status_text: resp.status_text,
                                headers: flat,
                                body: resp.body,
                            });
                            true
                        }
                        Err(e) => {
                            eprintln!("fetch_with_body error: {e}");
                            false
                        }
                    }
                }
            );
        }

        // _lumen_fetch_cancellable(url, method, timeout_ms) → u32
        // In-flight-cancellable GET/HEAD. Returns 0 = ok (body in FetchCache),
        // 1 = network error, 2 = aborted/timed-out. When timeout_ms > 0 a detached
        // deadline thread flips the AbortToken; the network layer tears the socket
        // down, so a `fetch(url, {signal: AbortSignal.timeout(ms)})` against a slow
        // server actually aborts even though the JS thread is parked in the call.
        reg!("_lumen_fetch_cancellable", move |url: String, method: String, timeout_ms: u32| -> u32 {
            let Some(ref provider) = fp_cancel else { return 1 };
            let token = AbortToken::new();
            if timeout_ms > 0 {
                let t = token.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(u64::from(timeout_ms)));
                    t.abort();
                });
            }
            match provider.fetch_cancellable(&url, &method, &token) {
                Ok(resp) => {
                    let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                    for (k, v) in resp.headers { flat.push(k); flat.push(v); }
                    *c_cancel.lock().unwrap() = Some(FetchCache {
                        status: resp.status,
                        status_text: resp.status_text,
                        headers: flat,
                        body: resp.body,
                    });
                    0
                }
                Err(lumen_core::error::Error::Aborted(_)) => 2,
                Err(e) => { eprintln!("fetch error: {e}"); 1 }
            }
        });

        // _lumen_fetch_cancellable_with_body(url, method, content_type, body, timeout_ms) → u32
        // Body-carrying (POST/PUT/...) sibling of _lumen_fetch_cancellable.
        reg!(
            "_lumen_fetch_cancellable_with_body",
            move |url: String, method: String, content_type: String, body: Vec<u8>, timeout_ms: u32| -> u32 {
                let Some(ref provider) = fp_cancel_body else { return 1 };
                let token = AbortToken::new();
                if timeout_ms > 0 {
                    let t = token.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(u64::from(timeout_ms)));
                        t.abort();
                    });
                }
                match provider.fetch_with_body_cancellable(&url, &method, &content_type, &body, &token) {
                    Ok(resp) => {
                        let mut flat = Vec::with_capacity(resp.headers.len() * 2);
                        for (k, v) in resp.headers { flat.push(k); flat.push(v); }
                        *c_cancel_body.lock().unwrap() = Some(FetchCache {
                            status: resp.status,
                            status_text: resp.status_text,
                            headers: flat,
                            body: resp.body,
                        });
                        0
                    }
                    Err(lumen_core::error::Error::Aborted(_)) => 2,
                    Err(e) => { eprintln!("fetch_with_body error: {e}"); 1 }
                }
            }
        );

        // ── Async fetch (in-flight AbortController.abort) ────────────────────────
        // Runs the request on a background thread so a JS `abort()` fired *during*
        // the request (not just a pre-flight/timeout) flips the AbortToken and the
        // network layer tears the socket down. JS fetch() drives a setTimeout poll
        // loop that resolves/rejects once the worker finishes. No shell change: the
        // existing timer pump drives the poll. Mirrors the WS/SSE poll model.
        {
            /// Background fetch result: success payload, or a typed failure.
            enum AsyncOutcome {
                /// Completed response (headers flattened: [name, value, ...]).
                Ok {
                    status: u16,
                    status_text: String,
                    headers: Vec<String>,
                    body: Vec<u8>,
                },
                /// Network/transport error.
                NetError,
                /// Aborted in flight via the AbortToken.
                Aborted,
            }
            /// Per-handle state shared between the worker thread and the JS poll.
            struct AsyncFetchState {
                token: AbortToken,
                outcome: Option<AsyncOutcome>,
            }
            let async_map: Arc<Mutex<HashMap<u32, AsyncFetchState>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let async_next: Arc<AtomicU32> = Arc::new(AtomicU32::new(1));

            // _lumen_fetch_async_start(url, method, content_type, body, has_body) → handle u32 (0 = no provider)
            let am_start = Arc::clone(&async_map);
            reg!(
                "_lumen_fetch_async_start",
                move |url: String, method: String, content_type: String, body: Vec<u8>, has_body: bool| -> u32 {
                    let provider = match fp_async.as_ref() {
                        Some(p) => Arc::clone(p),
                        None => return 0,
                    };
                    let id = async_next.fetch_add(1, Ordering::Relaxed);
                    let token = AbortToken::new();
                    am_start
                        .lock()
                        .unwrap()
                        .insert(id, AsyncFetchState { token: token.clone(), outcome: None });
                    let map = Arc::clone(&am_start);
                    std::thread::spawn(move || {
                        let res = if has_body {
                            provider.fetch_with_body_cancellable(&url, &method, &content_type, &body, &token)
                        } else {
                            provider.fetch_cancellable(&url, &method, &token)
                        };
                        let outcome = match res {
                            Ok(r) => AsyncOutcome::Ok {
                                status: r.status,
                                status_text: r.status_text,
                                headers: r
                                    .headers
                                    .into_iter()
                                    .flat_map(|(k, v)| [k, v])
                                    .collect(),
                                body: r.body,
                            },
                            Err(lumen_core::error::Error::Aborted(_)) => AsyncOutcome::Aborted,
                            Err(_) => AsyncOutcome::NetError,
                        };
                        if let Some(s) = map.lock().unwrap().get_mut(&id) {
                            s.outcome = Some(outcome);
                        }
                    });
                    id
                }
            );

            // _lumen_fetch_async_poll(handle) → 0 pending, 1 ok, 2 net-error, 3 aborted
            let am_poll = Arc::clone(&async_map);
            reg!("_lumen_fetch_async_poll", move |id: u32| -> u32 {
                let map = am_poll.lock().unwrap();
                match map.get(&id) {
                    None => 2,
                    Some(s) => match s.outcome {
                        None => 0,
                        Some(AsyncOutcome::Ok { .. }) => 1,
                        Some(AsyncOutcome::NetError) => 2,
                        Some(AsyncOutcome::Aborted) => 3,
                    },
                }
            });

            // _lumen_fetch_async_abort(handle) → flips the token (worker tears the socket down)
            let am_abort = Arc::clone(&async_map);
            reg!("_lumen_fetch_async_abort", move |id: u32| {
                if let Some(s) = am_abort.lock().unwrap().get(&id) {
                    s.token.abort();
                }
            });

            // _lumen_fetch_async_commit(handle) → moves a completed Ok result into the
            // global FetchCache slot so Response._fromFetchCache reads it. Returns false
            // if the handle is unknown or not in the Ok state.
            let am_commit = Arc::clone(&async_map);
            reg!("_lumen_fetch_async_commit", move |id: u32| -> bool {
                let mut map = am_commit.lock().unwrap();
                match map.get_mut(&id) {
                    None => false,
                    Some(s) => match s.outcome.take() {
                        Some(AsyncOutcome::Ok { status, status_text, headers, body }) => {
                            *c_async.lock().unwrap() = Some(FetchCache {
                                status,
                                status_text,
                                headers,
                                body,
                            });
                            true
                        }
                        other => {
                            s.outcome = other;
                            false
                        }
                    },
                }
            });

            // _lumen_fetch_async_free(handle) → drop the per-handle state
            let am_free = Arc::clone(&async_map);
            reg!("_lumen_fetch_async_free", move |id: u32| {
                am_free.lock().unwrap().remove(&id);
            });
        }

        // ── Per-response stream slots ────────────────────────────────────────────
        // Each call to Response._fromFetchCache() allocates a dedicated slot so the
        // body can be consumed independently of subsequent fetch() calls that would
        // otherwise overwrite the single FetchCache slot.
        //
        // _lumen_stream_alloc()                  → u32  (0 = empty body)
        // _lumen_stream_length(id: u32)          → u32
        // _lumen_stream_chunk(id, offset, size)  → Vec<u8>
        // _lumen_stream_free(id: u32)
        {
            let stream_slots: Arc<Mutex<HashMap<u32, Vec<u8>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let stream_next: Arc<AtomicU32> = Arc::new(AtomicU32::new(1));

            let (ss_alloc, sn, c_sa) = (
                Arc::clone(&stream_slots),
                Arc::clone(&stream_next),
                Arc::clone(&cache),
            );
            reg!("_lumen_stream_alloc", move || -> u32 {
                let body = {
                    let guard = c_sa.lock().unwrap();
                    guard.as_ref().map_or_else(Vec::new, |r| r.body.clone())
                };
                if body.is_empty() {
                    return 0;
                }
                let id = sn.fetch_add(1, Ordering::Relaxed);
                ss_alloc.lock().unwrap().insert(id, body);
                id
            });

            let ss_len = Arc::clone(&stream_slots);
            reg!("_lumen_stream_length", move |id: u32| -> u32 {
                ss_len.lock().unwrap().get(&id).map_or(0, |b| b.len() as u32)
            });

            let ss_chunk = Arc::clone(&stream_slots);
            reg!(
                "_lumen_stream_chunk",
                move |id: u32, offset: u32, size: u32| -> Vec<u8> {
                    let guard = ss_chunk.lock().unwrap();
                    let body = guard.get(&id).map_or(&[] as &[u8], |b| b.as_slice());
                    let start = (offset as usize).min(body.len());
                    let end = (start + size as usize).min(body.len());
                    body[start..end].to_vec()
                }
            );

            let ss_free = Arc::clone(&stream_slots);
            reg!("_lumen_stream_free", move |id: u32| {
                ss_free.lock().unwrap().remove(&id);
            });
        }

        // _lumen_send_beacon(url, body, content_type) → bool
        // Beacon API (W3C Beacon §3): fire-and-forget POST; response is ignored.
        // Returns false if no network provider is available, true if the request was queued.
        // The actual POST runs on a detached background thread so the JS caller is not blocked.
        {
            let fp = fp_beacon;
            reg!(
                "_lumen_send_beacon",
                move |url: String, body: String, content_type: String| -> bool {
                    let Some(ref provider) = fp else { return false };
                    let ct = if content_type.is_empty() {
                        "text/plain;charset=UTF-8".to_string()
                    } else {
                        content_type
                    };
                    let p = Arc::clone(provider);
                    std::thread::spawn(move || {
                        let _ = p.fetch_with_body_sync(&url, "POST", &ct, body.as_bytes());
                    });
                    true
                }
            );
        }
    }

    // ── Clipboard API ─────────────────────────────────────────────────────────
    // _lumen_clipboard_read()      → String (system clipboard plain text, "" if none)
    // _lumen_clipboard_write(text) → void   (replace system clipboard text)
    //
    // Both forward to the process-global clipboard provider installed by the shell
    // (`lumen_js::set_clipboard_provider`). With no provider (tests, dump modes)
    // read returns "" and write is a no-op, so navigator.clipboard still resolves.
    reg!("_lumen_clipboard_read", || -> String {
        crate::clipboard::read_text()
    });
    reg!("_lumen_clipboard_write", |text: String| {
        crate::clipboard::write_text(&text);
    });

    // ── WebAuthn / navigator.credentials ──────────────────────────────────────
    // _lumen_webauthn_create(packed) → JSON   (attestation result or {ok:false})
    // _lumen_webauthn_get(packed)    → JSON   (assertion result or {ok:false})
    // _lumen_webauthn_uvpa()         → bool   (platform authenticator available)
    //
    // `packed` is a `|`-separated string of base64url fields (see crate::credentials).
    // All forward to the process-global CredentialProvider installed by the shell
    // (`lumen_js::set_credential_provider`). With no provider, create/get return
    // {ok:false,error:"NotAllowedError"} so navigator.credentials still resolves.
    reg!("_lumen_webauthn_create", |packed: String| -> String {
        crate::credentials::create(packed)
    });
    reg!("_lumen_webauthn_get", |packed: String| -> String {
        crate::credentials::get(packed)
    });
    reg!("_lumen_webauthn_uvpa", || -> bool {
        crate::credentials::uvpa_available()
    });

    // ── WebSocket API ─────────────────────────────────────────────────────────
    // Phase 0 model: synchronous connect, background recv thread, JS polls.
    // _lumen_ws_connect(url)  → handle u32 (0 = error)
    // _lumen_ws_send(h, text) → bool
    // _lumen_ws_send_bin(h, data) → bool
    // _lumen_ws_close(h, code, reason)
    // _lumen_ws_poll(h) → Option<String> (JSON event or null)
    {
        use std::collections::HashMap;

        // Registry: handle → Box<dyn JsWebSocketSession>
        // Wrapped in Arc<Mutex<>> so each closure captures its own Arc clone.
        let registry: Arc<Mutex<HashMap<u32, Box<dyn lumen_core::ext::JsWebSocketSession>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id: Arc<Mutex<u32>> = Arc::new(Mutex::new(1));

        let (reg_c, nid_c, wp) = (Arc::clone(&registry), Arc::clone(&next_id), ws_provider);
        reg!("_lumen_ws_connect", move |url: String, proto_csv: String| -> u32 {
            let Some(ref provider) = wp else { return 0 };
            let protos: Vec<String> = proto_csv
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            match provider.connect(&url, &protos) {
                Ok(session) => {
                    let id = {
                        let mut n = nid_c.lock().unwrap();
                        let id = *n;
                        *n = n.wrapping_add(1).max(1);
                        id
                    };
                    reg_c.lock().unwrap().insert(id, session);
                    id
                }
                Err(e) => {
                    eprintln!("[JS WebSocket] connect error: {e}");
                    0
                }
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!("_lumen_ws_send", move |handle: u32, text: String| -> bool {
            let mut map = reg_c.lock().unwrap();
            if let Some(sess) = map.get_mut(&handle) {
                sess.send_text(&text).is_ok()
            } else {
                false
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!(
            "_lumen_ws_send_bin",
            move |handle: u32, data: Vec<u8>| -> bool {
                let mut map = reg_c.lock().unwrap();
                if let Some(sess) = map.get_mut(&handle) {
                    sess.send_binary(&data).is_ok()
                } else {
                    false
                }
            }
        );

        let reg_c = Arc::clone(&registry);
        reg!(
            "_lumen_ws_close",
            move |handle: u32, code: u32, reason: String| {
                let mut map = reg_c.lock().unwrap();
                if let Some(sess) = map.get_mut(&handle) {
                    let _ = sess.close(code as u16, &reason);
                }
            }
        );

        let reg_c = Arc::clone(&registry);
        reg!(
            "_lumen_ws_poll",
            move |handle: u32| -> Option<String> {
                let map = reg_c.lock().unwrap();
                let sess = map.get(&handle)?;
                sess.poll().map(|ev| match ev {
                    JsWsEvent::Open => {
                        let proto = sess.protocol().replace('\\', "\\\\").replace('"', "\\\"");
                        format!(r#"{{"t":"open","protocol":"{proto}"}}"#)
                    }
                    JsWsEvent::Message { data, is_binary } => {
                        if is_binary {
                            // Encode binary payload as base64-like hex for Phase 0.
                            let hex: String =
                                data.iter().map(|b| format!("{b:02x}")).collect();
                            format!(r#"{{"t":"msg","bin":true,"data":"{hex}"}}"#)
                        } else {
                            let text = String::from_utf8_lossy(&data);
                            // Minimal JSON-escape: replace \ and " only.
                            let escaped = text
                                .replace('\\', "\\\\")
                                .replace('"', "\\\"")
                                .replace('\n', "\\n")
                                .replace('\r', "\\r");
                            format!(r#"{{"t":"msg","bin":false,"data":"{escaped}"}}"#)
                        }
                    }
                    JsWsEvent::Close { code, reason } => {
                        let c = code.unwrap_or(1000);
                        let r = reason
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"");
                        format!(r#"{{"t":"close","code":{c},"reason":"{r}"}}"#)
                    }
                    JsWsEvent::Error(msg) => {
                        let m = msg
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"");
                        format!(r#"{{"t":"error","msg":"{m}"}}"#)
                    }
                })
            }
        );
    }

    // ── Server-Sent Events API (HTML Living Standard §9.2) ───────────────────
    // Phase 0 model: background recv thread buffers events, JS polls.
    // _lumen_sse_connect(url) → handle u32 (0 = error / no provider)
    // _lumen_sse_poll(handle) → Option<String> (JSON event or null)
    // _lumen_sse_close(handle)
    {
        use std::collections::HashMap;

        /// JSON-escape a string into a quoted JSON string literal (`"..."`).
        ///
        /// Handles the characters that must be escaped per RFC 8259 §7:
        /// `"`, `\`, and the C0 control set (`\n`/`\r`/`\t`/`\b`/`\f` plus `\u00XX`).
        fn json_str(s: &str) -> String {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\u{08}' => out.push_str("\\b"),
                    '\u{0c}' => out.push_str("\\f"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }

        // Registry: handle → Box<dyn JsSseSession>
        let registry: Arc<Mutex<HashMap<u32, Box<dyn lumen_core::ext::JsSseSession>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let next_id: Arc<Mutex<u32>> = Arc::new(Mutex::new(1));

        let (reg_c, nid_c, sp) = (Arc::clone(&registry), Arc::clone(&next_id), sse_provider);
        reg!("_lumen_sse_connect", move |url: String| -> u32 {
            let Some(ref provider) = sp else { return 0 };
            match provider.connect_sse(&url) {
                Ok(session) => {
                    let id = {
                        let mut n = nid_c.lock().unwrap();
                        let id = *n;
                        *n = n.wrapping_add(1).max(1);
                        id
                    };
                    reg_c.lock().unwrap().insert(id, session);
                    id
                }
                Err(e) => {
                    eprintln!("[JS SSE] connect error: {e}");
                    0
                }
            }
        });

        let reg_c = Arc::clone(&registry);
        reg!("_lumen_sse_poll", move |handle: u32| -> Option<String> {
            let map = reg_c.lock().unwrap();
            let sess = map.get(&handle)?;
            sess.poll().map(|ev| match ev {
                JsSseEvent::Open => r#"{"t":"open"}"#.to_string(),
                JsSseEvent::Message {
                    event_type,
                    data,
                    id,
                } => {
                    let id_json = id
                        .as_deref()
                        .map_or_else(|| "null".to_string(), json_str);
                    format!(
                        r#"{{"t":"message","event":{},"data":{},"id":{}}}"#,
                        json_str(&event_type),
                        json_str(&data),
                        id_json
                    )
                }
                JsSseEvent::Retry(ms) => {
                    format!(r#"{{"t":"retry","ms":{ms}}}"#)
                }
                JsSseEvent::Close => r#"{"t":"close"}"#.to_string(),
                JsSseEvent::Error(e) => {
                    format!(r#"{{"t":"error","message":{}}}"#, json_str(&e))
                }
            })
        });

        let reg_c = Arc::clone(&registry);
        reg!("_lumen_sse_close", move |handle: u32| {
            if let Some(mut sess) = reg_c.lock().unwrap().remove(&handle) {
                sess.close();
            }
        });
    }

    // ── localStorage ─────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_length", move || -> u32 { s.lock().unwrap().len() });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_key", move |n: u32| -> Option<String> {
            s.lock().unwrap().key(n).map(|k| k.to_owned())
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_get", move |key: String| -> Option<String> {
            s.lock().unwrap().get_item(&key).map(|v| v.to_owned())
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_set", move |key: String, value: String| {
            s.lock().unwrap().set_item(key, value);
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_remove", move |key: String| {
            s.lock().unwrap().remove_item(&key);
        });
        let s = Arc::clone(&ls_store);
        reg!("_lumen_ls_clear", move || {
            s.lock().unwrap().clear();
        });
    }

    // ── sessionStorage ────────────────────────────────────────────────────────
    {
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_length", move || -> u32 { s.lock().unwrap().len() });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_key", move |n: u32| -> Option<String> {
            s.lock().unwrap().key(n).map(|k| k.to_owned())
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_get", move |key: String| -> Option<String> {
            s.lock().unwrap().get_item(&key).map(|v| v.to_owned())
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_set", move |key: String, value: String| {
            s.lock().unwrap().set_item(key, value);
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_remove", move |key: String| {
            s.lock().unwrap().remove_item(&key);
        });
        let s = Arc::clone(&ss_store);
        reg!("_lumen_ss_clear", move || {
            s.lock().unwrap().clear();
        });
    }

    // ── IndexedDB persistence ─────────────────────────────────────────────────
    // Registered only when a backend is supplied (None in unit tests / sandboxed
    // contexts → the JS shim falls back to in-heap-only databases via its
    // `typeof _lumen_idb_persist === 'function'` guards). The shim serializes the
    // whole per-origin database set into one opaque JSON snapshot; `_lumen_idb_load`
    // restores it on init, `_lumen_idb_persist` writes it after each mutating flush.
    if let Some(idb) = idb_backend {
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_load", move || -> Option<String> { b.load() });
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_persist", move |snapshot: String| {
            b.save(&snapshot);
        });
        // Structured (Phase 3) row-level path. The JS shim keeps the in-heap
        // database authoritative and the opaque snapshot (above) as the lossless
        // restore source; these primitives additionally mirror schema + records
        // into the per-origin SQLite tables so `databases()` and future row-level
        // queries survive a reload. No-op on blob-only backends (default trait impls).
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_schema_op", move |json: String| -> bool {
            match serde_json::from_str::<lumen_core::ext::IdbSchemaOp>(&json) {
                Ok(op) => b.apply_schema(&op).is_ok(),
                Err(_) => false,
            }
        });
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_commit_txn", move |json: String| -> bool {
            match serde_json::from_str::<Vec<lumen_core::ext::IdbRecordOp>>(&json) {
                Ok(ops) => b.commit_txn(&ops).is_ok(),
                Err(_) => false,
            }
        });
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_exec_op", move |json: String| -> Option<String> {
            serde_json::from_str::<lumen_core::ext::IdbRecordOp>(&json)
                .ok()
                .and_then(|op| b.exec_op(&op).ok())
                .and_then(|result| serde_json::to_string(&result).ok())
        });
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_db_version", move |db_name: String| -> i32 {
            b.db_version(&db_name) as i32
        });
        let b = Arc::clone(&idb);
        reg!("_lumen_idb_databases", move || -> String {
            let dbs = b.list_databases();
            serde_json::to_string(
                &dbs.iter()
                    .map(|(name, version)| serde_json::json!({ "name": name, "version": version }))
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".to_string())
        });
    }

    // ── performance.now() — high-resolution timestamp ────────────────────────
    // Returns milliseconds since Unix epoch as f64; JS shim subtracts
    // the time-origin captured at install_dom_api time to give DOMHighResTimeStamp.
    // In deterministic mode (8F) always returns 0 so Date.now()/performance.now()
    // are frozen at the epoch, making rendering output independent of wall-clock time.
    let det_time = deterministic_seed.is_some();
    reg!("_lumen_now_ms", move || -> f64 {
        if det_time {
            0.0
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        }
    });

    // ── timer wakeup notification ─────────────────────────────────────────────
    // Called by _lumen_tick_timers / setTimeout / setInterval JS shims when a
    // timer is scheduled. Stores the earliest pending deadline (Unix epoch ms)
    // so the shell event loop can set ControlFlow::WaitUntil accordingly.
    {
        let tw = Arc::clone(&timer_wakeup);
        reg!("_lumen_request_wakeup", move |deadline_ms: f64| {
            let mut lock = tw.lock().unwrap();
            match *lock {
                None => *lock = Some(deadline_ms),
                Some(prev) if deadline_ms < prev => *lock = Some(deadline_ms),
                _ => {}
            }
        });
    }

    // Called by requestAnimationFrame when a callback is queued.
    // Shell reads this after each rendering step to decide whether to request
    // the next redraw for JS animation loops.
    {
        let raf = Arc::clone(&raf_pending);
        reg!("_lumen_mark_raf_pending", move || {
            raf.store(true, Ordering::Relaxed);
        });
    }

    // ── element geometry (for getBoundingClientRect / ResizeObserver / IntersectionObserver) ──
    // Returns [x, y, width, height] for the given NodeId in viewport-relative CSS px,
    // or undefined if the node has no layout box (display:none, not laid out yet, etc.).
    {
        let lr = Arc::clone(&layout_rects);
        reg!("_lumen_get_bounding_rect", move |nid: u32| -> Option<Vec<f64>> {
            lr.lock()
                .unwrap()
                .get(&nid)
                .map(|r| vec![f64::from(r[0]), f64::from(r[1]), f64::from(r[2]), f64::from(r[3])])
        });
    }

    // Returns [width, height] of the current viewport in CSS px.
    {
        let vs = Arc::clone(&viewport_size);
        reg!("_lumen_get_viewport_size", move || -> Vec<f64> {
            let s = *vs.lock().unwrap();
            vec![f64::from(s[0]), f64::from(s[1])]
        });
    }

    // ── window.matchMedia (CSS Media Queries L4 §4.2) ────────────────────────
    // Parses `query` as a media query and evaluates it against an ad-hoc
    // MediaContext built from the supplied viewport size + user-preference
    // flags. Pure function — no captures: parse_media_query and MediaQuery::matches
    // are stateless. Returns `true` when the query currently matches.
    reg!(
        "_lumen_match_media",
        |query: String, w: f64, h: f64, dark: bool, reduced_motion: bool| -> bool {
            let mq = lumen_css_parser::parse_media_query(&query);
            let ctx = lumen_css_parser::MediaContext {
                media_type: "screen".to_owned(),
                width: w as f32,
                height: h as f32,
                prefers_dark: dark,
                prefers_reduced_motion: reduced_motion,
                forced_colors: false,
                ..Default::default()
            };
            mq.matches(&ctx)
        }
    );

    // ── CSS.supports() backing (CSS Conditional Rules L3 §6) ──────────────────
    // Two-argument form: CSS.supports(property, value) → check property name.
    // Intentionally ignores value in Phase 0 (property-name check is sufficient
    // for the feature-detection patterns real sites use).
    reg!(
        "_lumen_css_supports_prop",
        |prop: String, _value: String| -> bool {
            lumen_css_parser::SUPPORTED_PROPERTIES
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&prop))
        }
    );
    // One-argument form: CSS.supports(conditionText) → parse + evaluate.
    reg!(
        "_lumen_css_supports_cond",
        |condition: String| -> bool {
            lumen_css_parser::parse_supports_condition(&condition)
                .evaluate(lumen_css_parser::SUPPORTED_PROPERTIES)
        }
    );

    // Queues a lazy image load request.  Called by `_lumen_deliver_lazy_images()` in JS
    // when an image registered via `_lumen_init_lazy_images` enters the lazy-load margin.
    // Shell drains via `QuickJsRuntime::take_lazy_image_requests` after each layout.
    {
        let req = Arc::clone(&lazy_img_requests);
        reg!("_lumen_request_lazy_image_load", move |nid: u32, url: String| {
            req.lock().unwrap().push((nid, url));
        });
    }

    // ── scroll state (for scrollTop/scrollLeft/scrollWidth/scrollHeight) ─────────
    // Returns [scroll_x, scroll_y, scroll_width, scroll_height] for an overflow container,
    // or undefined if the node is not a scroll container.
    {
        let ss = Arc::clone(&scroll_states);
        reg!("_lumen_get_scroll_state", move |nid: u32| -> Option<Vec<f64>> {
            ss.lock()
                .unwrap()
                .get(&nid)
                .map(|s| vec![f64::from(s[0]), f64::from(s[1]), f64::from(s[2]), f64::from(s[3])])
        });
    }
    // Queues a programmatic scroll request.  Shell drains via `take_scroll_requests()`.
    {
        let ps = Arc::clone(&pending_scrolls);
        reg!("_lumen_request_scroll", move |nid: u32, x: f64, y: f64| {
            ps.lock().unwrap().push((nid, x as f32, y as f32));
        });
    }
    // Queues a page-level scroll request from window.scrollTo/scrollBy.
    // `smooth=1` → start_smooth_scroll; `smooth=0` → scroll_to (instant).
    {
        let pps = Arc::clone(&pending_page_scrolls);
        reg!("_lumen_request_page_scroll", move |y: f64, smooth: u32| {
            pps.lock().unwrap().push((y as f32, smooth != 0));
        });
    }
    // Returns current page scroll Y for window.scrollY / window.pageYOffset.
    {
        let psy = Arc::clone(&page_scroll_y);
        reg!("_lumen_get_page_scroll_y", move || -> f64 {
            f64::from(*psy.lock().unwrap())
        });
    }

    // ── window.open() popup requests ────────────────────────────────────────────
    // Queues a popup window request. Shell drains via `take_window_open_requests()`.
    // `features` is the raw feature string ("width=800,height=600,..."); we parse
    // `width=` and `height=` here so the shell receives typed values.
    {
        let wor = Arc::clone(&window_open_requests);
        reg!(
            "_lumen_window_open",
            move |url: String, target: String, features: String| {
                let mut width: u32 = 800;
                let mut height: u32 = 600;
                for part in features.split(',') {
                    let part = part.trim();
                    if let Some(v) = part.strip_prefix("width=") {
                        width = v.trim().parse().unwrap_or(800);
                    } else if let Some(v) = part.strip_prefix("height=") {
                        height = v.trim().parse().unwrap_or(600);
                    }
                }
                wor.lock().unwrap().push(PopupRequest { url, target, width, height });
            }
        );
    }

    // ── Fullscreen API (WHATWG Fullscreen §4) ────────────────────────────────────
    // Shell drains via `take_fullscreen_requests()` and calls `window.set_fullscreen()`.
    {
        let fs_req = Arc::clone(&fullscreen_requests);
        reg!("_lumen_fs_enter", move |nid: u32| {
            fs_req.lock().unwrap().push(FullscreenRequest::Enter { nid });
        });
    }
    {
        let fs_req = Arc::clone(&fullscreen_requests);
        reg!("_lumen_fs_exit", move || {
            fs_req.lock().unwrap().push(FullscreenRequest::Exit);
        });
    }

    // ── Pointer Lock API (W3C Pointer Lock L2 §2-4) ────────────────────────────────
    // requestPointerLock(element_nid) — lock pointer to element.
    // Phase 0: in-memory lock. Phase 1: integrate with shell to capture cursor.
    reg!("_lumen_ptr_lock_request", move |nid: u32| {
        crate::pointer_lock::request_pointer_lock(nid);
    });

    // exitPointerLock() — release pointer lock.
    reg!("_lumen_exit_ptr_lock", move || {
        crate::pointer_lock::exit_pointer_lock();
    });

    // pointerLockElement getter — returns locked element or null.
    reg!("_lumen_ptr_lock_element", move || -> Option<u32> {
        crate::pointer_lock::get_locked_element_nid()
    });

    // ── Computed styles (window.getComputedStyle) ────────────────────────────────
    // Returns the resolved CSS value for `prop` on node `nid`, or "" if unknown.
    {
        let cs = Arc::clone(&computed_styles);
        reg!("_lumen_get_computed_style", move |nid: u32, prop: String| -> String {
            cs.lock()
                .unwrap()
                .get(&nid)
                .and_then(|m| m.get(&prop))
                .cloned()
                .unwrap_or_default()
        });
    }

    // ── Shadow DOM ───────────────────────────────────────────────────────────────
    // Attaches a new shadow root to `nid` and returns the shadow root NodeId.
    // `mode`: "open" | "closed".  Triggers layout dirty so the composed tree rebuilds.
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_attach_shadow", move |nid: u32, mode: String| -> u32 {
            let mut doc = d.lock().unwrap();
            let host = NodeId::from_index(nid as usize);
            let m = if mode == "closed" {
                ShadowRootMode::Closed
            } else {
                ShadowRootMode::Open
            };
            let shadow = doc.attach_shadow(host, m);
            dirty.store(true, Ordering::Relaxed);
            shadow.index() as u32
        });
    }
    // Returns the shadow root NodeId for `nid` if the root is Open, else None.
    // Closed roots are intentionally hidden from JS (encapsulation contract).
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_shadow_root", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let host = NodeId::from_index(nid as usize);
            doc.shadow_root_of(host).and_then(|sr| {
                if matches!(
                    doc.get(sr).data,
                    NodeData::ShadowRoot { mode: ShadowRootMode::Open }
                ) {
                    Some(sr.index() as u32)
                } else {
                    None
                }
            })
        });
    }
    // Returns true when `nid` is a shadow-root node (useful for JS wrapper dispatch).
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_is_shadow_root", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            matches!(doc.get(id).data, NodeData::ShadowRoot { .. })
        });
    }
    // Returns true when `nid` is a DocumentFragment node.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_is_document_fragment", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            matches!(doc.get(id).data, NodeData::DocumentFragment)
        });
    }
    // Allocate a new empty DocumentFragment and return its NodeId.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_create_fragment", move || -> u32 {
            let mut doc = d.lock().unwrap();
            doc.create_fragment().index() as u32
        });
    }
    // Return the content DocumentFragment NodeId for a <template> element, or None.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_template_content", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            doc.template_content(id).map(|f| f.index() as u32)
        });
    }
    // Deep-clone a subtree rooted at `nid`. Returns the new root NodeId.
    // `deep`: 1 = deep clone (including children), 0 = shallow (node only).
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_clone_subtree", move |nid: u32, deep: u32| -> u32 {
            let mut doc = d.lock().unwrap();
            let id = NodeId::from_index(nid as usize);
            doc.deep_clone(id, deep != 0).index() as u32
        });
    }
    // Insert `child` immediately before `reference` in `reference`'s parent.
    // Mirrors DOM `insertBefore(child, reference)`.
    {
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_insert_before",
            move |_parent_id: u32, child_id: u32, reference_id: u32| {
                let mut doc = d.lock().unwrap();
                let child = NodeId::from_index(child_id as usize);
                let reference = NodeId::from_index(reference_id as usize);
                doc.insert_before(child, reference);
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }
    // Return the shadow host NodeId for a node inside a shadow tree, or None.
    // Walks ancestors until a ShadowRoot is found, then returns its host.
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_shadow_root_host", move |nid: u32| -> Option<u32> {
            let doc = d.lock().unwrap();
            let mut cur = NodeId::from_index(nid as usize);
            loop {
                let node = doc.get(cur);
                if matches!(node.data, NodeData::ShadowRoot { .. }) {
                    return node.parent.map(|h| h.index() as u32);
                }
                {
                    let p = node.parent?;
                    cur = p
                }
            }
        });
    }

    // ── Selection API (WHATWG Selection API + DOM §4.5) ─────────────────────
    // Exposes document selection state to JavaScript. The Selection object is a
    // singleton per document; Range objects are snapshots of endpoint pairs.
    {
        // Returns [anchor_nid, anchor_offset, focus_nid, focus_offset] or null.
        let d = Arc::clone(&doc);
        reg!("_lumen_get_selection", move || -> Option<Vec<u32>> {
            let doc = d.lock().unwrap();
            let sel = doc.get_selection();
            match (sel.anchor, sel.focus) {
                (Some(a), Some(f)) => Some(vec![
                    a.container.index() as u32,
                    a.offset,
                    f.container.index() as u32,
                    f.offset,
                ]),
                _ => None,
            }
        });
    }
    {
        // Sets selection to [anchor_nid, anchor_offset, focus_nid, focus_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_set_selection",
            move |anchor_nid: u32, anchor_off: u32, focus_nid: u32, focus_off: u32| {
                let mut doc = d.lock().unwrap();
                doc.set_selection(Selection {
                    anchor: Some(DomPosition {
                        container: NodeId::from_index(anchor_nid as usize),
                        offset: anchor_off,
                    }),
                    focus: Some(DomPosition {
                        container: NodeId::from_index(focus_nid as usize),
                        offset: focus_off,
                    }),
                });
                dirty.store(true, Ordering::Relaxed);
            }
        );
    }
    {
        // Clears the current selection.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_clear_selection", move || {
            let mut doc = d.lock().unwrap();
            doc.set_selection(Selection { anchor: None, focus: None });
            dirty.store(true, Ordering::Relaxed);
        });
    }
    {
        // Returns text of the current selection.
        let d = Arc::clone(&doc);
        reg!("_lumen_get_selection_text", move || -> String {
            let doc = d.lock().unwrap();
            match doc.get_selection().get_range() {
                Some(r) => range_text(&doc, &r),
                None => String::new(),
            }
        });
    }
    {
        // Returns text covered by the given range endpoints.
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_range_text",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> String {
                let doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_index(start_nid as usize),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_index(end_nid as usize),
                        offset: end_off,
                    },
                };
                range_text(&doc, &r)
            }
        );
    }
    {
        // Number of direct DOM children (element offset validation).
        let d = Arc::clone(&doc);
        reg!("_lumen_node_child_count", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_child_count(&doc, NodeId::from_index(nid as usize)) as u32
        });
    }
    {
        // DOM-spec "length" of node: char count for text, child count for elements.
        let d = Arc::clone(&doc);
        reg!("_lumen_node_length", move |nid: u32| -> u32 {
            let doc = d.lock().unwrap();
            node_length(&doc, NodeId::from_index(nid as usize)) as u32
        });
    }
    {
        // Text content of a node (node.textContent).
        let d = Arc::clone(&doc);
        reg!("_lumen_node_text_content", move |nid: u32| -> String {
            let doc = d.lock().unwrap();
            node_text_content(&doc, NodeId::from_index(nid as usize))
        });
    }
    {
        // Deletes the contents of range; returns [new_pos_nid, new_pos_offset].
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_range_delete_contents",
            move |start_nid: u32, start_off: u32, end_nid: u32, end_off: u32| -> Vec<u32> {
                let mut doc = d.lock().unwrap();
                let r = DomRange {
                    start: DomPosition {
                        container: NodeId::from_index(start_nid as usize),
                        offset: start_off,
                    },
                    end: DomPosition {
                        container: NodeId::from_index(end_nid as usize),
                        offset: end_off,
                    },
                };
                let pos = lumen_dom::delete_range(&mut doc, &r);
                dirty.store(true, Ordering::Relaxed);
                vec![pos.container.index() as u32, pos.offset]
            }
        );
    }
    // ── contenteditable mutation bindings (Input Events Level 2 §4.1) ─────────
    // These are called by the JS shim's _lumen_handle_contenteditable_key()
    // which fires beforeinput → calls here → fires input.
    {
        // True if nid or any ancestor has contenteditable set to a truthy value.
        let d = Arc::clone(&doc);
        reg!("_lumen_is_contenteditable", move |nid: u32| -> bool {
            let doc = d.lock().unwrap();
            lumen_dom::find_editing_host(&doc, NodeId::from_index(nid as usize)).is_some()
        });
    }
    {
        // Insert `text` at the current selection (or caret) inside contenteditable.
        // Replaces selected content if the selection is non-collapsed.
        // Returns true on success.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_insert_text", move |text: String| -> bool {
            if text.is_empty() { return false; }
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let Some(anchor) = sel.anchor else { return false; };
            let insert_pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else {
                anchor
            };
            let new_pos = lumen_dom::insert_text_at(&mut doc, insert_pos, &text);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster before the caret (Backspace key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_delete_backward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            // Non-collapsed selection: delete it.
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                dirty.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            if anchor.offset == 0 { return false; }
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            // Walk backward one UTF-8 character boundary.
            let off = anchor.offset as usize;
            let mut prev = off.saturating_sub(1);
            while prev > 0 && !text.is_char_boundary(prev) {
                prev -= 1;
            }
            let r = DomRange {
                start: DomPosition { container: anchor.container, offset: prev as u32 },
                end: anchor,
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Delete one grapheme cluster after the caret (Delete key).
        // If the selection is non-collapsed, deletes the selection instead.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_delete_forward", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                let pos = lumen_dom::delete_range(&mut doc, &r);
                doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
                dirty.store(true, Ordering::Relaxed);
                return true;
            }
            let Some(anchor) = sel.anchor else { return false; };
            let text = match &doc.get(anchor.container).data {
                NodeData::Text(s) => s.clone(),
                _ => return false,
            };
            let off = anchor.offset as usize;
            if off >= text.len() { return false; }
            // Walk forward one UTF-8 character boundary.
            let mut next = off + 1;
            while next < text.len() && !text.is_char_boundary(next) {
                next += 1;
            }
            let r = DomRange {
                start: anchor,
                end: DomPosition { container: anchor.container, offset: next as u32 },
            };
            let pos = lumen_dom::delete_range(&mut doc, &r);
            doc.set_selection(Selection { anchor: Some(pos), focus: Some(pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // Split the block at the caret position (Enter key in contenteditable).
        // Finds the editing host, then calls insert_paragraph_break.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_contenteditable_insert_paragraph", move || -> bool {
            let mut doc = d.lock().unwrap();
            let sel = doc.get_selection().clone();
            let pos = if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                lumen_dom::delete_range(&mut doc, &r)
            } else if let Some(p) = sel.anchor {
                p
            } else {
                return false;
            };
            let Some(host) = lumen_dom::find_editing_host(&doc, pos.container) else {
                return false;
            };
            let new_pos = lumen_dom::insert_paragraph_break(&mut doc, pos, host);
            doc.set_selection(Selection { anchor: Some(new_pos), focus: Some(new_pos) });
            dirty.store(true, Ordering::Relaxed);
            true
        });
    }
    {
        // execCommand: bold/italic/underline/insertText/delete/selectAll/copy/cut/paste
        // Returns true if the command was handled.
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!(
            "_lumen_exec_command",
            move |cmd: String, value: String| -> bool {
                let mut doc = d.lock().unwrap();
                let sel = doc.get_selection().clone();
                match cmd.as_str() {
                    "selectAll" => {
                        // Select entire document body text
                        if let Some(body) = find_element_by_tag(&doc, "body") {
                            let children = doc.get(body).children.clone();
                            if !children.is_empty() {
                                let first = *children.first().unwrap();
                                let last = *children.last().unwrap();
                                let last_len = node_length(&doc, last);
                                doc.set_selection(Selection {
                                    anchor: Some(DomPosition { container: first, offset: 0 }),
                                    focus: Some(DomPosition {
                                        container: last,
                                        offset: last_len as u32,
                                    }),
                                });
                                dirty.store(true, Ordering::Relaxed);
                            }
                        }
                        true
                    }
                    "insertText" => {
                        if let Some(pos) = sel.anchor {
                            // Delete selection first if non-collapsed
                            let pos = sel
                                .get_range()
                                .filter(|r| !r.is_collapsed())
                                .map(|r| lumen_dom::delete_range(&mut doc, &r))
                                .unwrap_or(pos);
                            let new_pos = lumen_dom::insert_text_at(&mut doc, pos, &value);
                            doc.set_selection(Selection {
                                anchor: Some(new_pos),
                                focus: Some(new_pos),
                            });
                            dirty.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    "delete" | "forwardDelete" => {
                        if let Some(r) = sel.get_range().filter(|r| !r.is_collapsed()) {
                            let pos = lumen_dom::delete_range(&mut doc, &r);
                            doc.set_selection(Selection {
                                anchor: Some(pos),
                                focus: Some(pos),
                            });
                            dirty.store(true, Ordering::Relaxed);
                        }
                        true
                    }
                    // bold/italic/underline: CSSOM inline style toggling (stub — returns true
                    // so editors know the command is accepted; real inline-style mutation
                    // requires Range wrapping which is Phase 3 contenteditable work).
                    "bold" | "italic" | "underline" | "strikeThrough"
                    | "justifyLeft" | "justifyCenter" | "justifyRight" | "justifyFull"
                    | "indent" | "outdent"
                    | "createLink" | "unlink"
                    | "insertOrderedList" | "insertUnorderedList"
                    | "fontName" | "fontSize" | "foreColor" | "backColor"
                    | "removeFormat" => true,
                    // copy/cut/paste: clipboard interaction is handled by the shell;
                    // returning false lets it fall through to native clipboard handling.
                    "copy" | "cut" | "paste" => false,
                    _ => false,
                }
            }
        );
    }

    // ── document.cookie (RFC 6265 §5.3-5.4) ─────────────────────────────────
    // The getter/setter wrap CookieProvider using host/scheme derived from
    // page_url parsed once at install time. Best-effort: if the URL cannot be
    // parsed (e.g. file://) we skip cookie injection silently.
    {
        let parsed = Url::parse(&page_url).ok();
        let host = parsed.as_ref().map(|u| u.host().to_ascii_lowercase()).unwrap_or_default();
        let is_secure = parsed.as_ref().map(|u| u.scheme() == "https").unwrap_or(false);

        if let Some(jar) = cookie_jar {
            let jar_get = Arc::clone(&jar);
            let host_get = host.clone();
            reg!("_lumen_cookie_get", move || -> String {
                jar_get.get_for_request(&host_get, "/", is_secure, None, false)
            });

            let host_set = host;
            reg!("_lumen_cookie_set", move |cookie_str: String| {
                jar.process_set_cookie(&cookie_str, &host_set, "/", is_secure, None);
            });
        } else {
            reg!("_lumen_cookie_get", move || -> String { String::new() });
            reg!("_lumen_cookie_set", move |_unused: String| {});
        }
    }

    // ── Microtask drain ─────────────────────────────────────────────────────
    // TODO(v8-s3): needs isolate access — draining V8's microtask queue requires
    // `scope.perform_microtask_checkpoint()` on the isolate, which compat-layer
    // closures (JsValue-level only) cannot reach. Stubbed as a no-op so the global
    // exists; V8 auto-runs microtasks after each script/task by default so this
    // primitive (only used to force-flush in QuickJS unit tests) is not required
    // for correctness under V8. Revisit if a future slice needs manual draining.
    reg!("_lumen_drain_microtasks", move || {});

    // ── Web Crypto API ──────────────────────────────────────────────────────
    {
        // Returns `n` cryptographically-random bytes as a Vec<u8> (JS Array of
        // integers 0–255). Capped at 65 536 per call per WebCrypto spec §10.1.3.
        reg!("_lumen_get_random_bytes", |n: u32| -> Vec<u8> {
            let len = (n as usize).min(65_536);
            let mut buf = vec![0u8; len];
            getrandom::getrandom(&mut buf).unwrap_or(());
            buf
        });

        // Computes a SHA digest using the named algorithm.
        // `algo` must be one of "SHA-1", "SHA-256", "SHA-384", "SHA-512".
        // `data` is the raw input bytes.  Returns empty Vec on unknown algo.
        reg!(
            "_lumen_sha_digest",
            |algo: String, data: Vec<u8>| -> Vec<u8> {
                // sha1::Digest trait must be in scope to call sha1::Sha1::digest().
                use sha1::Digest as _;
                match algo.as_str() {
                    "SHA-1" => sha1::Sha1::digest(&data).to_vec(),
                    "SHA-256" => sha2::Sha256::digest(&data).to_vec(),
                    "SHA-384" => sha2::Sha384::digest(&data).to_vec(),
                    "SHA-512" => sha2::Sha512::digest(&data).to_vec(),
                    _ => Vec::new(),
                }
            }
        );

        // Compress `data` using the named format.
        // `format`: "deflate-raw" (raw DEFLATE, RFC 1951), "deflate" (zlib, RFC 1950), "gzip".
        // Returns empty Vec on unknown format or I/O error.
        reg!(
            "_lumen_compress_bytes",
            |data: Vec<u8>, format: String| -> Vec<u8> {
                use flate2::Compression;
                use std::io::Write as _;
                match format.as_str() {
                    "deflate-raw" => {
                        let mut enc =
                            flate2::write::DeflateEncoder::new(Vec::new(), Compression::default());
                        enc.write_all(&data).ok();
                        enc.finish().unwrap_or_default()
                    }
                    "deflate" => {
                        let mut enc =
                            flate2::write::ZlibEncoder::new(Vec::new(), Compression::default());
                        enc.write_all(&data).ok();
                        enc.finish().unwrap_or_default()
                    }
                    "gzip" => {
                        let mut enc =
                            flate2::write::GzEncoder::new(Vec::new(), Compression::default());
                        enc.write_all(&data).ok();
                        enc.finish().unwrap_or_default()
                    }
                    _ => Vec::new(),
                }
            }
        );

        // Decompress `data` using the named format.
        // `format`: "deflate-raw", "deflate", "gzip". Returns empty Vec on error.
        reg!(
            "_lumen_decompress_bytes",
            |data: Vec<u8>, format: String| -> Vec<u8> {
                use std::io::Read as _;
                match format.as_str() {
                    "deflate-raw" => {
                        let mut dec = flate2::read::DeflateDecoder::new(data.as_slice());
                        let mut out = Vec::new();
                        dec.read_to_end(&mut out).ok();
                        out
                    }
                    "deflate" => {
                        let mut dec = flate2::read::ZlibDecoder::new(data.as_slice());
                        let mut out = Vec::new();
                        dec.read_to_end(&mut out).ok();
                        out
                    }
                    "gzip" => {
                        let mut dec = flate2::read::GzDecoder::new(data.as_slice());
                        let mut out = Vec::new();
                        dec.read_to_end(&mut out).ok();
                        out
                    }
                    _ => Vec::new(),
                }
            }
        );
    }

    // SubtleCrypto: generateKey/importKey/exportKey/sign/verify/encrypt/decrypt
    // TODO(v8-s3, out of scope): SubtleCrypto install is rquickjs-ctx-based (crate::subtle_crypto) — separate future slice.

    // Trusted Types API: trustedTypes.createPolicy(), TrustedHTML/Script/ScriptURL
    // TODO(v8-s3, out of scope): Trusted Types install is rquickjs-ctx-based (crate::trusted_types) — separate future slice.

    // D-6: Extension system — chrome.runtime.sendMessage() native binding.
    // Phase 0: no-op; the message is logged to stderr for debugging.
    // Phase 1: shell wires a real IPC channel between content scripts and extension background.
    reg!("_lumen_chrome_runtime_send_message", |msg: String| {
        let _ = msg;
    });

    // CSS Typed OM API: element.attributeStyleMap / computedStyleMap()
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_style_property", move |nid: u32, prop: String| -> String {
            if let Ok(doc) = d.lock() {
                let node = doc.get(NodeId::from_index(nid as usize));
                if let Some(style_attr) = node.get_attr("style") {
                    let parsed = _parse_style_string(style_attr);
                    let kebab_prop = _camel_to_kebab(&prop);
                    return parsed.get(&kebab_prop).cloned().unwrap_or_default();
                }
            }
            String::new()
        });
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_set_style_property", move |nid: u32, prop: String, val: String| {
            if let Ok(mut doc) = d.lock() {
                let node_id = NodeId::from_index(nid as usize);
                let mut parsed = if let Some(style) = doc.get(node_id).get_attr("style") {
                    _parse_style_string(style)
                } else {
                    std::collections::HashMap::new()
                };
                let kebab_prop = _camel_to_kebab(&prop);
                parsed.insert(kebab_prop, val);
                let css_text = _serialize_style_map(&parsed);
                set_attribute(&mut doc, node_id, "style", &css_text);
                dirty.store(true, Ordering::Relaxed);
            }
        });
        let d = Arc::clone(&doc);
        let dirty = Arc::clone(&dom_dirty);
        reg!("_lumen_delete_style_property", move |nid: u32, prop: String| {
            if let Ok(mut doc) = d.lock() {
                let node_id = NodeId::from_index(nid as usize);
                let mut parsed = if let Some(style) = doc.get(node_id).get_attr("style") {
                    _parse_style_string(style)
                } else {
                    std::collections::HashMap::new()
                };
                let kebab_prop = _camel_to_kebab(&prop);
                parsed.remove(&kebab_prop);
                let css_text = _serialize_style_map(&parsed);
                if css_text.is_empty() {
                    remove_attribute(&mut doc, node_id, "style");
                } else {
                    set_attribute(&mut doc, node_id, "style", &css_text);
                }
                dirty.store(true, Ordering::Relaxed);
            }
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_has_style_property", move |nid: u32, prop: String| -> bool {
            if let Ok(doc) = d.lock() {
                let node = doc.get(NodeId::from_index(nid as usize));
                if let Some(style_attr) = node.get_attr("style") {
                    let parsed = _parse_style_string(style_attr);
                    let kebab_prop = _camel_to_kebab(&prop);
                    return parsed.contains_key(&kebab_prop);
                }
            }
            false
        });
        reg!("_lumen_get_style_entries", move |_nid: u32| -> String {
            // Phase 0: return empty object for iteration (stub)
            "[]".to_string()
        });
    }


            // Inject the page URL + cross-origin-isolation state as JS globals so
            // WEB_API_SHIM can initialise `location` and `window.crossOriginIsolated`.
            {
                let key = v8::String::new(scope, "_LUMEN_PAGE_URL")
                    .ok_or_else(|| JsError::Runtime("OOM: key '_LUMEN_PAGE_URL'".into()))?;
                let val = v8::String::new(scope, &page_url)
                    .ok_or_else(|| JsError::Runtime("OOM: page_url value".into()))?;
                ctx.global(scope).set(scope, key.into(), val.into());
            }
            {
                let key = v8::String::new(scope, "_LUMEN_CROSS_ORIGIN_ISOLATED").ok_or_else(
                    || JsError::Runtime("OOM: key '_LUMEN_CROSS_ORIGIN_ISOLATED'".into()),
                )?;
                let val = v8::Boolean::new(scope, cross_origin_isolated);
                ctx.global(scope).set(scope, key.into(), val.into());
            }

            // Evaluate WEB_API_SHIM inline. Cannot call `self.eval(...)` here: the JS
            // thread is already busy running this job (dispatched via `self.run`), and
            // `run` cannot be re-entered from inside its own job closure (it would
            // deadlock waiting on a channel the thread isn't servicing).
            {
                v8::tc_scope!(tc, scope);
                let src = v8::String::new(tc, crate::dom::WEB_API_SHIM)
                    .ok_or_else(|| JsError::Runtime("OOM: WEB_API_SHIM source".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("WEB_API_SHIM compile returned None".into()))?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // Deterministic render mode (8F): override Math.random with a seeded
            // xorshift32 PRNG and freeze Date.now() at 0. Must run after WEB_API_SHIM
            // so Date and Math are fully set up. Same script QuickJS's
            // `dom::install_dom_api` builds (kept byte-for-byte identical).
            if let Some(seed) = deterministic_seed {
                let seed32 = u32::try_from(seed & 0xffff_ffff).unwrap_or(1);
                let seed32 = if seed32 == 0 { 1 } else { seed32 };
                let js = format!(
                    "(function(){{var s={seed32};\
                     Math.random=function(){{s^=s<<13;s^=s>>>17;s^=s<<5;return (s>>>0)/4294967296;}};\
                     Date.now=function(){{return 0;}};\
                     }})()"
                );
                v8::tc_scope!(tc, scope);
                let src = v8::String::new(tc, &js)
                    .ok_or_else(|| JsError::Runtime("OOM: deterministic seed script".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("deterministic seed script compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            Ok(())
        })
    }
}

// ─── DOM helpers (S3) ──────────────────────────────────────────────────────────
//
// Small private duplicates of `dom.rs`'s module-private helpers
// (`find_element_by_tag`, `set_attribute`, `HistoryState`, ...). Kept here
// instead of widening their visibility in `dom.rs`, so the QuickJS code path
// stays untouched apart from the single `WEB_API_SHIM` visibility change.

/// Mirrors `dom::HistoryState` (private there) — per-page JS `history` stack.
struct HistoryState {
    entries: Vec<(String, String)>,
    current: usize,
}

impl HistoryState {
    fn new() -> Self {
        Self {
            entries: vec![(String::from("null"), String::new())],
            current: 0,
        }
    }

    fn push(&mut self, state_json: String, url: String) {
        self.entries.truncate(self.current + 1);
        self.entries.push((state_json, url));
        self.current = self.entries.len() - 1;
    }

    fn replace(&mut self, state_json: String, url: String) {
        if let Some(e) = self.entries.get_mut(self.current) {
            *e = (state_json, url);
        }
    }

    fn set_state(&mut self, state_json: String) {
        if let Some(e) = self.entries.get_mut(self.current) {
            e.0 = state_json;
        }
    }

    fn go(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let new_idx = self.current as i64 + i64::from(delta);
        if new_idx < 0 || new_idx >= self.entries.len() as i64 {
            return false;
        }
        self.current = new_idx as usize;
        true
    }

    fn state_json(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.0.as_str())
            .unwrap_or("null")
    }

    fn url(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.1.as_str())
            .unwrap_or("")
    }

    fn length(&self) -> u32 {
        self.entries.len() as u32
    }
}

/// Mirrors `dom::cache_meta_method` — extract `"method"` from a cache meta JSON string.
fn cache_meta_method(meta_json: &str) -> String {
    if let Some(start) = meta_json.find("\"method\":\"") {
        let rest = &meta_json[start + 10..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    "GET".to_string()
}

/// Mirrors `dom::_parse_style_string` — parse `"color: red; font-size: 12px"` into a map.
fn _parse_style_string(css_text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for decl in css_text.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some((prop, val)) = decl.split_once(':') {
            map.insert(prop.trim().to_string(), val.trim().to_string());
        }
    }
    map
}

/// Mirrors `dom::_serialize_style_map` — serialize a style map back into CSS text.
fn _serialize_style_map(map: &HashMap<String, String>) -> String {
    map.iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Mirrors `dom::_camel_to_kebab` — convert camelCase to kebab-case.
fn _camel_to_kebab(prop: &str) -> String {
    let mut result = String::new();
    for (i, c) in prop.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

/// Mirrors `dom::find_element_by_tag`.
fn find_element_by_tag(doc: &lumen_dom::Document, tag: &str) -> Option<lumen_dom::NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

/// Mirrors `dom::find_first_matching`.
fn find_first_matching(
    doc: &lumen_dom::Document,
    start: lumen_dom::NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
) -> Option<lumen_dom::NodeId> {
    let node = doc.get(start);
    if pred(node) {
        return Some(start);
    }
    for &child in &node.children.clone() {
        if let Some(found) = find_first_matching(doc, child, pred) {
            return Some(found);
        }
    }
    None
}

/// Mirrors `dom::collect_text_content`.
fn collect_text_content(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> String {
    let mut out = String::new();
    collect_text_inner(doc, id, &mut out);
    out
}

/// Mirrors `dom::collect_text_inner`.
fn collect_text_inner(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    let node = doc.get(id);
    if let lumen_dom::NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children.clone() {
        collect_text_inner(doc, child, out);
    }
}

/// Mirrors `dom::set_text_content`.
fn set_text_content(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, text: &str) {
    let children: Vec<lumen_dom::NodeId> = doc.get(id).children.clone();
    for child in children {
        doc.detach(child);
    }
    if !text.is_empty() {
        let text_node = doc.create_text(text);
        doc.append_child(id, text_node);
    }
}

/// Mirrors `dom::set_attribute`.
fn set_attribute(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, name: &str, value: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        if let Some(attr) = attrs
            .iter_mut()
            .find(|a| a.name.local.eq_ignore_ascii_case(name))
        {
            attr.value = value.to_string();
        } else {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName::html(name.to_ascii_lowercase()),
                value: value.to_string(),
            });
        }
    }
}

/// Mirrors `dom::remove_attribute`.
fn remove_attribute(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, name: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        attrs.retain(|a| !a.name.local.eq_ignore_ascii_case(name));
    }
}

// ── JsRuntime impl ────────────────────────────────────────────────────────────

/// Shared scope-setup boilerplate: create pinned HandleScope + ContextScope +
/// pinned TryCatch, then call the provided closure with the TryCatch ref.
///
/// The macro-heavy setup hides the three-step scope dance required by rusty_v8
/// v150 (scope! → ContextScope → tc_scope!) and avoids duplicating it across
/// eval/set_global/get_global/call_function.
macro_rules! with_tc {
    ($inner:expr, |$tc:ident, $ctx:ident| $body:expr) => {{
        // Disjoint field borrows: scope borrows isolate mutably, context immutably.
        let isolate = &mut $inner.isolate;
        let context_global = &$inner.context;
        // scope! pins the HandleScope; scope: &mut PinnedRef<HandleScope<'_, ()>>
        v8::scope!(let scope, isolate);
        // Local<'_, Context> — Copy, usable after ContextScope is created
        let $ctx = v8::Local::new(scope, context_global);
        // ContextScope enters the context; scope: &mut ContextScope<…, HandleScope<…>>
        let scope = &mut v8::ContextScope::new(scope, $ctx);
        // tc_scope! pins TryCatch; $tc: &mut PinnedRef<TryCatch<…, HandleScope<…>>>
        v8::tc_scope!($tc, scope);
        $body
    }};
}

impl JsRuntime for V8JsRuntime {
    fn eval(&self, script: &str) -> JsResult<JsValue> {
        self.run(|inner| {
            with_tc!(inner, |tc, _ctx| {
                let src = v8::String::new(tc, script)
                    .ok_or_else(|| JsError::Runtime("OOM: script string".into()))?;

                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("script compile returned None".into()))?;

                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                match result {
                    Some(val) => from_v8(tc, val),
                    None => Err(JsError::Runtime("script returned no value".into())),
                }
            })
        })
    }

    fn set_global(&self, name: &str, value: JsValue) -> JsResult<()> {
        self.run(|inner| {
            with_tc!(inner, |tc, ctx| {
                let key = v8::String::new(tc, name)
                    .ok_or_else(|| JsError::Runtime(format!("OOM: key '{name}'")))?;
                let val = to_v8(tc, value)?;
                // ctx is Local<Context> (Copy); use it to obtain the global object.
                let global = ctx.global(tc);
                global.set(tc, key.into(), val);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                Ok(())
            })
        })
    }

    fn get_global(&self, name: &str) -> JsResult<JsValue> {
        self.run(|inner| {
            with_tc!(inner, |tc, ctx| {
                let key = v8::String::new(tc, name)
                    .ok_or_else(|| JsError::Runtime(format!("OOM: key '{name}'")))?;
                let global = ctx.global(tc);
                let val = global
                    .get(tc, key.into())
                    .ok_or_else(|| JsError::Runtime(format!("global '{name}' not found")))?;
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                from_v8(tc, val)
            })
        })
    }

    fn call_function(&self, name: &str, args: &[JsValue]) -> JsResult<JsValue> {
        self.run(|inner| {
            with_tc!(inner, |tc, ctx| {
                let key = v8::String::new(tc, name)
                    .ok_or_else(|| JsError::Runtime(format!("OOM: function '{name}'")))?;
                let global = ctx.global(tc);
                let func_val = global
                    .get(tc, key.into())
                    .ok_or_else(|| JsError::Runtime(format!("'{name}' not found in globals")))?;
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let func: v8::Local<v8::Function> = func_val
                    .try_into()
                    .map_err(|_| JsError::Runtime(format!("'{name}' is not a function")))?;
                let mut v8_args: Vec<v8::Local<v8::Value>> = Vec::with_capacity(args.len());
                for a in args.iter().cloned() {
                    v8_args.push(to_v8(tc, a)?);
                }
                let recv = v8::undefined(tc).into();
                let result = func.call(tc, recv, &v8_args);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                match result {
                    Some(val) => from_v8(tc, val),
                    None => Ok(JsValue::Null),
                }
            })
        })
    }

    fn engine_name(&self) -> &'static str {
        "v8"
    }

    fn suspend(&mut self) -> JsResult<SuspendedHeap> {
        // S11 will implement real ValueSerializer-based serialisation.
        Ok(SuspendedHeap::default())
    }

    fn resume(_snapshot: SuspendedHeap) -> JsResult<Self> {
        Self::new()
    }
}

// ── Value converters ──────────────────────────────────────────────────────────

/// Convert a V8 `Local<Value>` to a `JsValue`.
///
/// `scope` must be a `&PinScope<'s, '_>` (= `PinnedRef<HandleScope<'_, Context>>`).
/// Any scope that deref-coerces to one is accepted (e.g. `&mut PinnedRef<TryCatch<…>>`).
fn from_v8<'s>(scope: &v8::PinScope<'s, '_>, val: v8::Local<'s, v8::Value>) -> JsResult<JsValue> {
    if val.is_null() || val.is_undefined() {
        return Ok(JsValue::Null);
    }
    if val.is_boolean() {
        return Ok(JsValue::Bool(val.boolean_value(scope)));
    }
    if val.is_number() {
        return Ok(JsValue::Number(val.number_value(scope).unwrap_or(f64::NAN)));
    }
    if val.is_string() {
        let s = val
            .to_string(scope)
            .ok_or_else(|| JsError::Runtime("string conversion failed".into()))?;
        return Ok(JsValue::String(s.to_rust_string_lossy(scope)));
    }
    if val.is_array() {
        let arr: v8::Local<v8::Array> = val.try_into().unwrap();
        let len = arr.length();
        let mut items = Vec::with_capacity(len as usize);
        for i in 0..len {
            let elem = arr
                .get_index(scope, i)
                .ok_or_else(|| JsError::Runtime(format!("array[{i}] is missing")))?;
            items.push(from_v8(scope, elem)?);
        }
        return Ok(JsValue::Array(items));
    }
    if val.is_object() {
        let obj: v8::Local<v8::Object> = val.try_into().unwrap();
        let own_props = obj
            .get_own_property_names(scope, Default::default())
            .ok_or_else(|| JsError::Runtime("get_own_property_names failed".into()))?;
        let mut entries: Vec<(String, JsValue)> = Vec::new();
        for i in 0..own_props.length() {
            let key = own_props.get_index(scope, i).unwrap();
            let key_str = key
                .to_string(scope)
                .ok_or_else(|| JsError::Runtime("property key to_string failed".into()))?
                .to_rust_string_lossy(scope);
            let prop_val = obj
                .get(scope, key)
                .ok_or_else(|| JsError::Runtime(format!("get '{key_str}' failed")))?;
            entries.push((key_str, from_v8(scope, prop_val)?));
        }
        return Ok(JsValue::object(entries));
    }
    Ok(JsValue::Undefined)
}

/// Convert a `JsValue` to a V8 `Local<Value>`.
fn to_v8<'s>(scope: &v8::PinScope<'s, '_>, val: JsValue) -> JsResult<v8::Local<'s, v8::Value>> {
    Ok(match val {
        JsValue::Null | JsValue::Undefined => v8::null(scope).into(),
        JsValue::Bool(b) => v8::Boolean::new(scope, b).into(),
        JsValue::Number(n) => v8::Number::new(scope, n).into(),
        JsValue::String(s) => v8::String::new(scope, &s)
            .ok_or_else(|| JsError::Runtime("OOM: string allocation".into()))?
            .into(),
        JsValue::Array(items) => {
            let arr = v8::Array::new(scope, items.len() as i32);
            for (i, item) in items.into_iter().enumerate() {
                let v8_item = to_v8(scope, item)?;
                arr.set_index(scope, i as u32, v8_item);
            }
            arr.into()
        }
        JsValue::Object(entries) => {
            let obj = v8::Object::new(scope);
            for (k, v) in entries {
                let key = v8::String::new(scope, &k)
                    .ok_or_else(|| JsError::Runtime("OOM: key allocation".into()))?;
                let v8_val = to_v8(scope, v)?;
                obj.set(scope, key.into(), v8_val);
            }
            obj.into()
        }
    })
}

/// Extract an error message from a V8 exception value.
fn v8_err<'s>(scope: &v8::PinScope<'s, '_>, exc: v8::Local<'s, v8::Value>) -> JsError {
    // Try obj.message first (covers Error instances), fall back to string coercion.
    if let Ok(obj) = v8::Local::<v8::Object>::try_from(exc)
        && let Some(msg_key) = v8::String::new(scope, "message")
        && let Some(msg_val) = obj.get(scope, msg_key.into())
        && msg_val.is_string()
        && let Some(s) = msg_val.to_string(scope)
    {
        return JsError::Runtime(s.to_rust_string_lossy(scope));
    }
    let msg = exc
        .to_string(scope)
        .map(|s| s.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "JS exception".into());
    JsError::Runtime(msg)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::JsRuntime;

    fn rt() -> V8JsRuntime {
        V8JsRuntime::new().unwrap()
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
        assert!(matches!(rt().eval("function ("), Err(JsError::Runtime(_))));
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
        rt.set_global(
            "arr",
            JsValue::Array(vec![JsValue::Number(1.0), JsValue::Number(2.0)]),
        )
        .unwrap();
        assert_eq!(rt.eval("arr[0] + arr[1]").unwrap(), JsValue::Number(3.0));
    }

    #[test]
    fn engine_name() {
        assert_eq!(rt().engine_name(), "v8");
    }

    #[test]
    fn is_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<V8JsRuntime>();
    }

    #[test]
    fn resume_produces_functional_runtime() {
        let fresh = V8JsRuntime::resume(SuspendedHeap::default()).unwrap();
        assert_eq!(fresh.eval("6 * 7").unwrap(), JsValue::Number(42.0));
    }

    // ── S2: compat-layer tests ────────────────────────────────────────────────

    #[test]
    fn console_log_callable_from_js() {
        use std::sync::{Arc, Mutex};
        let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let rt = rt();
        rt.install_console_natives(Arc::clone(&msgs)).unwrap();
        rt.eval("_lumen_console_log('hello')").unwrap();
        let captured = msgs.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0], (0, "hello".to_string()));
    }

    #[test]
    fn console_warn_and_error_callable_from_js() {
        use std::sync::{Arc, Mutex};
        let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let rt = rt();
        rt.install_console_natives(Arc::clone(&msgs)).unwrap();
        rt.eval("_lumen_console_warn('w'); _lumen_console_error('e')")
            .unwrap();
        let captured = msgs.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], (1, "w".to_string()));
        assert_eq!(captured[1], (2, "e".to_string()));
    }

    #[test]
    fn console_log_numeric_arg_coerced_to_string() {
        use std::sync::{Arc, Mutex};
        let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let rt = rt();
        rt.install_console_natives(Arc::clone(&msgs)).unwrap();
        // JS passes 42 (a Number) to a native expecting String — coerced to "42".
        rt.eval("_lumen_console_log(42)").unwrap();
        let captured = msgs.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].1, "42");
    }

    #[test]
    fn native_registered_after_eval_is_accessible() {
        use std::sync::{Arc, Mutex};
        let msgs: Arc<Mutex<Vec<(u8, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let rt = rt();
        rt.install_console_natives(Arc::clone(&msgs)).unwrap();
        // Calling the native inside a JS function defined after registration.
        rt.eval("function f(x) { _lumen_console_log(x); } f('ok')")
            .unwrap();
        assert_eq!(msgs.lock().unwrap()[0].1, "ok");
    }

    // ── S3: install_dom (DOM-core natives + WEB_API_SHIM) ───────────────────────

    /// Builds `html > head > title > "Test Page"`, `html > body > div#main > span.highlight > "Hello"`.
    /// Mirrors `dom::tests::make_doc`.
    fn make_doc() -> Arc<Mutex<lumen_dom::Document>> {
        let mut doc = lumen_dom::Document::new();
        let html = doc.create_element(lumen_dom::QualName::html("html"));
        let head = doc.create_element(lumen_dom::QualName::html("head"));
        let title = doc.create_element(lumen_dom::QualName::html("title"));
        let title_text = doc.create_text("Test Page");
        let body = doc.create_element(lumen_dom::QualName::html("body"));
        let div = doc.create_element(lumen_dom::QualName::html("div"));
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName::html("id"),
                value: "main".into(),
            });
        }
        let span = doc.create_element(lumen_dom::QualName::html("span"));
        if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(span).data {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName::html("class"),
                value: "highlight".into(),
            });
        }
        let text = doc.create_text("Hello");
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, title);
        doc.append_child(title, title_text);
        doc.append_child(html, body);
        doc.append_child(body, div);
        doc.append_child(div, span);
        doc.append_child(span, text);
        Arc::new(Mutex::new(doc))
    }

    /// Mirrors `dom::tests::runtime_with_dom`: a V8 runtime with DOM-core natives
    /// and `WEB_API_SHIM` installed against `doc`, page URL `page_url`.
    fn runtime_with_dom(doc: Arc<Mutex<lumen_dom::Document>>, page_url: &str) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        rt.install_dom(doc, page_url, None, None, None, None, None, None, None, None, false)
            .unwrap();
        rt
    }

    #[test]
    fn query_selector_finds_element_by_id() {
        let rt = runtime_with_dom(make_doc(), "");
        let ok = rt
            .eval("document.querySelector('#main').tagName === 'DIV'")
            .unwrap();
        assert_eq!(ok, JsValue::Bool(true));
    }

    #[test]
    fn query_selector_by_class_reads_text_content() {
        let rt = runtime_with_dom(make_doc(), "");
        let text = rt
            .eval("document.querySelector('.highlight').textContent")
            .unwrap();
        assert_eq!(text, JsValue::String("Hello".into()));
    }

    #[test]
    fn timeout_is_deferred_until_tick() {
        let rt = runtime_with_dom(make_doc(), "");
        // Timer must NOT fire synchronously — deferred to _lumen_tick_timers().
        let result = rt
            .eval("var x = 0; setTimeout(function() { x = 1; }, 0); x")
            .unwrap();
        assert_eq!(result, JsValue::Number(0.0));
    }

    #[test]
    fn timeout_fires_after_tick() {
        let rt = runtime_with_dom(make_doc(), "");
        rt.eval("var x = 0; setTimeout(function() { x = 1; }, 0);")
            .unwrap();
        let result = rt.eval("_lumen_tick_timers(); x").unwrap();
        assert_eq!(result, JsValue::Number(1.0));
    }

    #[test]
    fn location_href_reads_page_url() {
        let rt = runtime_with_dom(make_doc(), "https://example.com/page");
        let href = rt.eval("window.location.href").unwrap();
        assert_eq!(href, JsValue::String("https://example.com/page".into()));
    }

    #[test]
    fn location_href_assignment_queues_navigate_request() {
        let rt = runtime_with_dom(make_doc(), "https://example.com/page");
        rt.eval("window.location.href = 'https://example.com/next'")
            .unwrap();
        match rt.take_navigate_request() {
            Some(crate::dom::NavigateRequest::Push(url)) => {
                assert_eq!(url, "https://example.com/next");
            }
            other => panic!("expected NavigateRequest::Push, got {other:?}"),
        }
        // Consumed — a second read returns None.
        assert!(rt.take_navigate_request().is_none());
    }
}

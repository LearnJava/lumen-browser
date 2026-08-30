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
use crate::heap_snapshot;
use crate::v8_compat::{
    OwnedNativeFn, into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4, into_v8_fn5,
    into_v8_fn6, register_v8_native,
};
use lumen_core::ext::{AbortToken, JsSseEvent, JsWsEvent};
use lumen_core::url::Url;
use lumen_core::{JsError, JsResult, JsRuntime, JsValue, SuspendedHeap};
use lumen_dom::{
    DocumentMode, DomPosition, Namespace, NodeData, NodeId, QualName, Range as DomRange,
    Selection, ShadowRootMode, node_child_count, node_length, node_text_content, range_text,
};
use lumen_layout::{matches_selector, query_all, query_all_scoped, query_all_within};
use std::collections::{HashMap, HashSet};
use v8::{ValueDeserializerHelper, ValueSerializerHelper};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::sync::{
    Once,
    mpsc::{Sender, SyncSender, sync_channel},
};
use std::thread::JoinHandle;

// ─── DOM helpers (S3) ──────────────────────────────────────────────────────────
//
// Small private duplicates of `dom.rs`'s module-private helpers
// (`find_element_by_tag`, `set_attribute`, `HistoryState`, ...). Kept here
// instead of widening their visibility in `dom.rs`, so the QuickJS code path
// stays untouched apart from the single `WEB_API_SHIM` visibility change.
// Вынесены в подмодули батчем SPLIT-JS5; здесь остаются только объявления.

mod dom_helpers;
mod history_state;

use dom_helpers::*;
use history_state::HistoryState;

// ── JsRuntime impl ────────────────────────────────────────────────────────────
//
// Реализация трейта, отчёт об исключениях, кэш байт-кода и конвертеры значений
// вынесены в подмодули батчем SPLIT-JS5.

mod code_cache;
mod eval;
mod value;

use value::*;

// ── install_dom sections ──────────────────────────────────────────────────────
//
// 41 секция-баннер тела `install_dom` вынесена батчем SPLIT-JS6; здесь остаётся
// сама функция — её преамбула, вызовы помощников и хвост. Почему `reg!` уехал с
// контекстом в параметрах, а не «как есть», — в доккомментарии `install`.

mod install;

// ── Голова рантайма: модули SPLIT-JS7 ─────────────────────────────────────────

mod command;
mod named_access;
mod promise_reject;
mod thread;

pub use named_access::ensure_v8_platform;
pub(crate) use command::DOM_EXCEPTION_POLYFILL;
use command::{V8Command, V8Inner, V8_CMD_QUEUE_BOUND};
use named_access::set_named_access_document;
use thread::v8_thread_main;
mod overrides;
mod runtime;

pub use overrides::{
    set_global_timezone_override, set_global_user_agent_override,
    timezone_override_script, user_agent_override_script,
};
pub use runtime::{CustomPropertySnapshot, DomTouched, V8JsRuntime};
// Приватная привязка, чтобы `use super::*;` потомков (в т.ч. `install::net`)
// продолжала видеть помощника под прежним именем.
use overrides::{global_timezone_override, global_user_agent_override};
use runtime::pairs_from_flat;

// ── S3: DOM-core native registration ─────────────────────────────────────────
//
// Ports `dom::install_primitives`'s 184 `_lumen_*` natives (rquickjs) to the V8
// compat layer. Scoped to DOM-core only, mirroring `dom::install_dom_api` minus
// the WebGL/Canvas2D/OffscreenCanvas/AudioContext installs that
// `QuickJsRuntime::install_dom` also performs — those, plus Highlight/Battery/
// Navigator-normalization/CSS-Houdini/SubtleCrypto/TrustedTypes, are separate
// future slices that each need their own ctx-taking install fn ported.

#[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
impl V8JsRuntime {
    /// Install DOM-core native bindings (`_lumen_*`, 184 functions) and the
    /// `WEB_API_SHIM` JavaScript that builds `document`, `window`, `console`,
    /// `location`, `navigator`, `fetch`, `WebSocket`, `localStorage`, and
    /// `sessionStorage` on top of them.
    ///
    /// Mirrors [`crate::QuickJsRuntime::install_dom`] but scoped to the DOM-core
    /// piece only (`dom::install_dom_api`'s `install_primitives` + shim eval).
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    #[allow(clippy::unwrap_used)]  // унаследовано, docs/lint-policy.md §10
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
        // BUG-836: the tab's store when the owner attached one via
        // `with_session_storage`, so `sessionStorage` survives navigation the way
        // HTML LS §12.2 requires; a fresh (document-local) store otherwise.
        let ss_store: Arc<Mutex<lumen_core::WebStorage>> = self
            .ss_store
            .clone()
            .unwrap_or_else(|| Arc::new(Mutex::new(lumen_core::WebStorage::default())));
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
            // DEVX-16: an explicit `--rng-seed` override takes precedence over
            // the URL-hash derivation.
            let override_seed = *self
                .deterministic_rng_seed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            Some(override_seed.unwrap_or_else(|| crate::deterministic_seed_from_url(page_url)))
        } else {
            None
        };
        let monotonic_clock = self.deterministic_monotonic.load(Ordering::Relaxed);
        let deterministic_clock_ms = Arc::clone(&self.deterministic_clock_ms);
        // BUG-371: the file-API grants are bound to this document's origin.
        // Derived here, before `page_url` is moved into the `self.run` closure
        // below, and never taken from a JS argument.
        let page_origin = crate::file_input::origin_for_url(page_url);
        let page_url = page_url.to_owned();
        // BUG-480 срез 4: ключ этого контекста в исходящем ящике кросс-
        // фреймовых postMessage — указатель Arc собственного документа. Тот
        // же инстанс Arc уходит и в реестр родителя (register_iframe_document),
        // поэтому адресата можно найти с обеих сторон. Ставится до run() —
        // doc уезжает в замыкание по значению.
        //
        // Срез 8: тот же ключ дублируется в рантайме — по нему шелл спрашивает
        // «есть ли неразобранные конверты для МЕНЯ» и будит спящий цикл.
        //
        // Срез 10: клон Arc остаётся в реестре — натив обратной доставки
        // ресурсных событий (`_lumen_f_queue_parent_resource`) проверяет по
        // нему nid и берёт `source_doc` конверта.
        {
            let mut reg = self
                .frame_docs
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            reg.self_key = Some(Arc::as_ptr(&doc) as usize);
            reg.self_doc = Some(Arc::clone(&doc));
            reg.self_origin = page_origin.clone();
            self.self_doc_key
                .store(Arc::as_ptr(&doc) as usize, Ordering::Relaxed);
        }
        // BUG-295: session-level `navigator.userAgent` override, if any.
        let ua_override = global_user_agent_override();
        // BUG-295: session-level `Intl`/`Date` timezone override, if any.
        let timezone_override = global_timezone_override();
        // BUG-364: dedicated/shared Worker script fetch reuses the same fetch
        // provider as the Fetch API below. Cloned *before* the `move` closure
        // below takes ownership of `fetch_provider` — Worker/SharedWorker
        // installation happens after that closure returns.
        let fp_worker = fetch_provider.clone();
        let fp_shared_worker = fetch_provider.clone();
        // Тот же провайдер отдаётся загрузчику модулей: `import('./chunk.js')`
        // обязан сходить в сеть, а не искать чанк в заранее зарегистрированных
        // исходниках (иначе code-split приложение не собирается вовсе).
        let fp_esm = fetch_provider.clone();
        // Сеть области сервис-воркера (`importScripts`, `fetch`): клонируется
        // здесь по той же причине, что и предыдущие — ниже провайдер уезжает
        // в замыкание по значению.
        let fp_sw_net = fetch_provider.clone();
        // База IndexedDB воркера — та же, что у страницы: воркер, ведущий свою
        // очередь в `indexedDB`, обязан видеть те же данные.
        let idb_sw = idb_backend.clone();

        self.run(move |inner| {
            // ESM (S12b-23): fallback base URL the module resolver uses for
            // relative imports issued from inline `<script type=module>` bodies,
            // which have no URL of their own. Mirrors the `module_page_url`
            // write in `QuickJsRuntime::install_dom`.
            crate::v8_esm::set_page_url(&page_url);
            crate::v8_esm::set_fetch_provider(fp_esm);
            // BUG-384: point the Window named-properties interceptor (installed
            // on the global object template back at context creation) at this
            // navigation's document. Done here, on the JS thread, because the
            // slot it writes is thread-local to the isolate's owning thread.
            set_named_access_document(&doc);
            // Disjoint field borrows: scope borrows isolate, native_fn_store is separate.
            let isolate = &mut inner.isolate;
            let context_global = &inner.context;
            let store = &mut inner.native_fn_store;

            v8::scope!(let scope, isolate);
            let ctx = v8::Local::new(scope, context_global);
            let scope = &mut v8::ContextScope::new(scope, ctx);

            let nav_out = Arc::clone(&self.nav_out);
            let timer_wakeup = Arc::clone(&self.timer_wakeup);
            let dom_dirty = Arc::clone(&self.dom_dirty);
            let dom_touched = Arc::clone(&self.dom_touched);
            let raf_pending = Arc::clone(&self.raf_pending);
            let layout_rects = Arc::clone(&self.layout_rects);
            let viewport_size = Arc::clone(&self.viewport_size);
            let lazy_img_requests = Arc::clone(&self.lazy_img_requests);
            let scroll_states = Arc::clone(&self.scroll_states);
            let pending_scrolls = Arc::clone(&self.pending_scrolls);
            let pending_page_scrolls = Arc::clone(&self.pending_page_scrolls);
            let page_scroll_y = Arc::clone(&self.page_scroll_y);
            let computed_styles = Arc::clone(&self.computed_styles);
            let custom_properties = Arc::clone(&self.custom_properties);
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

            install::install_console(scope, ctx, store, Arc::clone(&console_messages))?;

            install::install_print(scope, ctx, store, Arc::clone(&print_requests))?;

            install::install_dialog_focus(scope, ctx, store, Arc::clone(&pending_focus_requests))?;

            install::install_document_meta(scope, ctx, store, Arc::clone(&doc))?;

            install::install_document_fonts(scope, ctx, store, Arc::clone(&doc))?;

            install::install_node_lookup(scope, ctx, store, Arc::clone(&doc))?;

            install::install_node_properties(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_tree_navigation(scope, ctx, store, Arc::clone(&doc))?;

            install::install_node_count(scope, ctx, store, Arc::clone(&doc))?;

            install::install_tree_mutation(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_service_worker(
                scope,
                ctx,
                store,
                sw_backend.clone(),
                cache_backend.clone(),
                sw_worker_store.clone(),
                fp_sw_net.clone(),
                idb_sw.clone(),
            )?;

            install::install_history(
                scope,
                ctx,
                store,
                Arc::clone(&pending_history_url_updates),
                Arc::clone(&pending_history_traversals),
            )?;

            install::install_navigation_api(
                scope,
                ctx,
                store,
                Arc::clone(&nav_state),
                Arc::clone(&pending_navigation_updates),
                Arc::clone(&pending_nav_intercepted),
            )?;

            install::install_navigation(scope, ctx, store, Arc::clone(&nav_out))?;

            install::install_fetch(scope, ctx, store, fetch_provider.clone())?;

            install::install_clipboard(scope, ctx, store)?;

            install::install_webauthn(scope, ctx, store)?;

            install::install_websocket(scope, ctx, store, ws_provider.clone())?;

            install::install_text_decoder(scope, ctx, store)?;

            install::install_sse(scope, ctx, store, sse_provider.clone())?;

            install::install_local_storage(scope, ctx, store, Arc::clone(&ls_store))?;

            install::install_session_storage(scope, ctx, store, Arc::clone(&ss_store))?;

            install::install_indexed_db(scope, ctx, store, idb_backend.clone())?;

            install::install_performance_now(
                scope,
                ctx,
                store,
                deterministic_seed,
                monotonic_clock,
                Arc::clone(&deterministic_clock_ms),
            )?;

            install::install_timer_wakeup(
                scope,
                ctx,
                store,
                Arc::clone(&timer_wakeup),
                Arc::clone(&raf_pending),
            )?;

            install::install_element_geometry(
                scope,
                ctx,
                store,
                Arc::clone(&layout_rects),
                Arc::clone(&viewport_size),
            )?;

            install::install_match_media(scope, ctx, store)?;

            install::install_css_supports_and_lazy_images(
                scope,
                ctx,
                store,
                Arc::clone(&lazy_img_requests),
            )?;

            install::install_scroll_state(
                scope,
                ctx,
                store,
                Arc::clone(&scroll_states),
                Arc::clone(&pending_scrolls),
                Arc::clone(&pending_page_scrolls),
                Arc::clone(&page_scroll_y),
            )?;

            install::install_window_open(scope, ctx, store, Arc::clone(&window_open_requests))?;

            install::install_fullscreen(scope, ctx, store, Arc::clone(&fullscreen_requests))?;

            install::install_pointer_lock(scope, ctx, store)?;

            install::install_computed_styles(
                scope,
                ctx,
                store,
                Arc::clone(&computed_styles),
                Arc::clone(&custom_properties),
            )?;

            install::install_shadow_dom(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_selection(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;
            install::install_contenteditable(scope, ctx, store, Arc::clone(&doc))?;
            install::install_design_mode(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
            )?;

            install::install_cookie(scope, ctx, store, page_url.clone(), cookie_jar.clone())?;

            install::install_esm_registry(scope, ctx, store)?;

            install::install_microtask_drain(scope, ctx, store)?;

            install::install_crypto_and_typed_om(
                scope,
                ctx,
                store,
                Arc::clone(&doc),
                Arc::clone(&dom_dirty),
                Arc::clone(&dom_touched),
                Arc::clone(&computed_styles),
                Arc::clone(&custom_properties),
            )?;

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

            // Polyfill `DOMException`: quickjs-ng provides it as a built-in (part of
            // `Context::full()`'s bundled extras), V8 has no web-platform globals at
            // all. `WEB_API_SHIM` and dozens of `install_*` module shims (Ph3 V8
            // migration S5-S7) construct `new DOMException(...)` — without this,
            // `class X extends DOMException` throws `ReferenceError` the moment any
            // such module is evaluated. Shape probed against quickjs-ng's built-in
            // (legacy numeric `code`, full WHATWG constant table, `instanceof Error`).
            {
                v8::tc_scope!(tc, scope);
                let src = v8::String::new(tc, DOM_EXCEPTION_POLYFILL)
                    .ok_or_else(|| JsError::Runtime("OOM: DOM_EXCEPTION_POLYFILL source".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("DOM_EXCEPTION_POLYFILL compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // Evaluate WEB_API_SHIM inline. Cannot call `self.eval(...)` here: the JS
            // thread is already busy running this job (dispatched via `self.run`), and
            // `run` cannot be re-entered from inside its own job closure (it would
            // deadlock waiting on a channel the thread isn't servicing).
            //
            // BUG-378: run it through *indirect* eval rather than as a Script.
            // Both forms create the shim's top-level `var`/`function` bindings as
            // properties of the global object, but with different attributes: a
            // Script's GlobalDeclarationInstantiation passes `D = false`
            // (ECMA-262 §16.1.7), so `_lumen_u2n`, `_lumen_timers`, … come out
            // **non-configurable** — and `enumerable: true → false` is exactly
            // the transition `Object.defineProperty` forbids on a
            // non-configurable property, which left 247 internal names
            // permanently visible to `for (k in window)` no matter what the
            // sealing pass did. Indirect eval's EvalDeclarationInstantiation
            // passes `D = true` (§19.2.1.3), so the same bindings become
            // configurable and `internal_globals::seal_internal_globals_v8` can
            // hide and freeze them at the end of this function.
            //
            // Safe only because the shim has no top-level `let`/`const`/`class`:
            // those are lexical, and eval puts them in a declarative environment
            // that dies with the eval call instead of on the global object — a
            // future top-level `const` would silently vanish. Guarded by
            // `internal_globals`'s `shim_has_no_top_level_lexical_declarations`.
            {
                v8::tc_scope!(tc, scope);
                let shim = crate::dom::web_api_shim();
                let src = v8::String::new(tc, &shim)
                    .ok_or_else(|| JsError::Runtime("OOM: WEB_API_SHIM source".into()))?;
                let wrapper_src = v8::String::new(tc, "(function(s) { (0, eval)(s); })")
                    .ok_or_else(|| JsError::Runtime("OOM: WEB_API_SHIM eval wrapper".into()))?;
                let compiled = v8::Script::compile(tc, wrapper_src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("WEB_API_SHIM compile returned None".into()))?;
                let wrapper = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let wrapper: v8::Local<v8::Function> = wrapper
                    .ok_or_else(|| JsError::Runtime("WEB_API_SHIM eval wrapper is None".into()))?
                    .try_into()
                    .map_err(|_| {
                        JsError::Runtime("WEB_API_SHIM eval wrapper is not a function".into())
                    })?;
                let recv = v8::undefined(tc).into();
                let result = wrapper.call(tc, recv, &[src.into()]);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // Trusted Types API (W3C TT L2, Phase 0): plain JS, no rquickjs-specific API,
            // so the shared shim string is evaluated the same way as WEB_API_SHIM above.
            {
                v8::tc_scope!(tc, scope);
                let src = v8::String::new(tc, crate::trusted_types::TRUSTED_TYPES_SHIM)
                    .ok_or_else(|| JsError::Runtime("OOM: TRUSTED_TYPES_SHIM source".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("TRUSTED_TYPES_SHIM compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // Deterministic render mode (8F): override Math.random with a seeded
            // xorshift32 PRNG and freeze Date.now() at 0. Must run after WEB_API_SHIM
            // so Date and Math are fully set up. Mirrors the script QuickJS's
            // `dom::install_dom_api` builds, except for the DEVX-16 monotonic-clock
            // branch below, which is V8-only (QuickJS is a frozen rollback path,
            // see CLAUDE.md — no new functionality is added there).
            if let Some(seed) = deterministic_seed {
                let seed32 = u32::try_from(seed & 0xffff_ffff).unwrap_or(1);
                let seed32 = if seed32 == 0 { 1 } else { seed32 };
                // DEVX-16: `--monotonic-clock` routes Date.now() through the same
                // `_lumen_now_ms()` native binding performance.now() uses, so both
                // advance in lockstep off one shared counter instead of Date.now()
                // staying frozen at 0.
                let date_now_body = if monotonic_clock {
                    "return _lumen_now_ms();"
                } else {
                    "return 0;"
                };
                let js = format!(
                    "(function(){{var s={seed32};\
                     Math.random=function(){{s^=s<<13;s^=s>>>17;s^=s<<5;return (s>>>0)/4294967296;}};\
                     Date.now=function(){{{date_now_body}}};\
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

            // BUG-295 (`emulation.setUserAgentOverride`): must run after
            // WEB_API_SHIM (same ordering constraint as the deterministic-seed
            // block above) so `navigator` already exists, and before any page
            // `<script>` executes (those run after `install_dom` returns) so
            // even a synchronous top-level read of `navigator.userAgent` sees
            // the override from the very first script.
            if let Some(ua) = ua_override {
                v8::tc_scope!(tc, scope);
                let js = user_agent_override_script(&ua);
                let src = v8::String::new(tc, &js)
                    .ok_or_else(|| JsError::Runtime("OOM: UA override script".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled
                    .ok_or_else(|| JsError::Runtime("UA override script compile returned None".into()))?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            // BUG-295 (`browser.setTimezoneOverride`): a plain global
            // assignment, order-independent w.r.t. the `Intl` shim (installed
            // later, outside this closure — see `intl_bindings::install_intl_bindings_v8`
            // call site) since the shim reads the marker lazily at
            // `DateTimeFormat` construction time, not at shim-install time.
            if let Some(tz) = timezone_override {
                v8::tc_scope!(tc, scope);
                let js = timezone_override_script(&tz);
                let src = v8::String::new(tc, &js)
                    .ok_or_else(|| JsError::Runtime("OOM: timezone override script".into()))?;
                let compiled = v8::Script::compile(tc, src, None);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let compiled = compiled.ok_or_else(|| {
                    JsError::Runtime("timezone override script compile returned None".into())
                })?;
                let result = compiled.run(tc);
                if tc.has_caught() {
                    let exc = tc.exception().unwrap();
                    return Err(v8_err(tc, exc));
                }
                let _ = result;
            }

            Ok(())
        })?;

        // Ph3 V8 migration S5-S7: simple-module batch (~68 modules, plain JS-shim
        // installs with no native `Function::new` registrations, or ≤2 trivial
        // shims) — see docs/tasks/ph3-v8-migration.md for the ported/pending
        // checklist. Called outside the `self.run` dispatch above because each
        // `install_*_v8` helper dispatches its own job via `self.eval` internally.
        // Best-effort like the rquickjs orchestration in `lib.rs::install_dom`
        // (`if let Err(e) = X::install_Y(&ctx) { eprintln!(...) }`): one broken/
        // partial module (Phase 0 stub) must not abort DOM bootstrap for the
        // other 67.
        macro_rules! install_v8 {
            ($module:ident :: $func:ident) => {
                if let Err(e) = crate::$module::$func(self) {
                    eprintln!("v8: {}::{} failed: {e}", stringify!($module), stringify!($func));
                }
            };
        }
        // Ph3 V8 migration S8: canvas2d + webgl_canvas (hand-port, not part of the
        // simple-module S5-S7 batch, but same best-effort orchestration). Mirrors
        // lib.rs::install_dom's ordering (webgl before canvas2d).
        let fingerprint = lumen_paint::GpuFingerprint {
            vendor: "WebKit".to_string(),
            renderer: "Generic GPU".to_string(),
        };
        if let Err(e) = crate::webgl_canvas::install_webgl_canvas_v8(self, &fingerprint) {
            eprintln!("v8: webgl_canvas::install_webgl_canvas_v8 failed: {e}");
        }
        // BUG-454: the canvas fingerprint noise seed is derived from the document
        // origin, so like `file_input`/`filesystem_access` these two take an
        // explicit-arg call rather than `install_v8!`.
        if let Err(e) = crate::canvas2d::install_canvas2d_bindings_v8(self, &page_origin) {
            eprintln!("v8: canvas2d::install_canvas2d_bindings_v8 failed: {e}");
        }
        // P1-imagebitmap: OffscreenCanvas was deferred past S8 (see the note at
        // canvas2d.rs's transferControlToOffscreen V8 port); ported now so
        // createImageBitmap/ImageBitmapRenderingContext work under the default engine.
        if let Err(e) = crate::offscreen_canvas::install_offscreen_canvas_bindings_v8(self, &page_origin) {
            eprintln!("v8: offscreen_canvas::install_offscreen_canvas_bindings_v8 failed: {e}");
        }

        install_v8!(async_context::install_async_context_v8);
        install_v8!(attribution_reporting::install_attribution_reporting_api_v8);
        install_v8!(audio_element::install_audio_element_bindings_v8);
        install_v8!(background_fetch::install_background_fetch_v8);
        install_v8!(background_sync::install_background_sync_v8);
        install_v8!(badging::install_badging_bindings_v8);
        install_v8!(battery_bindings::install_battery_bindings_v8);
        install_v8!(bluetooth::install_bluetooth_bindings_v8);
        install_v8!(broadcast_channel::install_broadcast_channel_bindings_v8);
        install_v8!(close_watcher::install_close_watcher_v8);
        install_v8!(compute_pressure::install_compute_pressure_bindings_v8);
        install_v8!(contacts::install_contacts_manager_v8);
        install_v8!(content_index::install_content_index_api_v8);
        // Default: enabled (mirrors lib.rs::install_dom's `self.cookie_banner_dismiss`
        // AtomicBool read; extra-arg call since the flag lives on `self`, not `&ctx`).
        let cb_enabled = self.cookie_banner_dismiss.load(Ordering::Relaxed);
        if let Err(e) = crate::cookie_banner::install_cookie_banner_bindings_v8(self, cb_enabled) {
            eprintln!("v8: cookie_banner::install_cookie_banner_bindings_v8 failed: {e}");
        }
        install_v8!(cookie_store::install_cookie_store_v8);
        install_v8!(credentials::install_credentials_bindings_v8);
        install_v8!(csp::install_csp_bindings_v8);
        install_v8!(css_properties_values_api::install_css_properties_values_api_v8);
        install_v8!(decorators::install_decorator_shim_v8);
        install_v8!(device_sensors::install_device_sensors_bindings_v8);
        install_v8!(digital_credentials::install_digital_credentials_api_v8);
        install_v8!(document_pip::install_document_pip_api_v8);
        install_v8!(documentpip_bindings::install_docpip_bindings_v8);
        install_v8!(dom_parser::install_dom_parser_v8);
        install_v8!(download_bindings::install_download_bindings_v8);
        install_v8!(element_internals::install_element_internals_bindings_v8);
        install_v8!(es2026_proposals::install_es2026_proposals_v8);
        install_v8!(eye_dropper::install_eye_dropper_bindings_v8);
        // BUG-371: both file APIs take the document origin their grants are
        // bound to, so they get explicit-arg calls rather than `install_v8!`.
        if let Err(e) = crate::file_input::install_file_input_bindings_v8(self, &page_origin) {
            eprintln!("v8: file_input::install_file_input_bindings_v8 failed: {e}");
        }
        if let Err(e) = crate::filesystem_access::install_filesystem_access_v8(self, &page_origin) {
            eprintln!("v8: filesystem_access::install_filesystem_access_v8 failed: {e}");
        }
        // BUG-371 point 1: both shims have now captured the natives they need,
        // so take the whole file-API surface off the global object. Runs even if
        // one of the two installs above failed — a half-installed shim must not
        // leave `__lumen_file_read_text` sitting on `window`.
        if let Err(e) = crate::file_input::seal_file_natives_v8(self) {
            eprintln!("v8: file_input::seal_file_natives_v8 failed: {e}");
        }
        install_v8!(form_validation::install_form_validation_bindings_v8);
        install_v8!(gamepad::install_gamepad_bindings_v8);
        install_v8!(generic_sensor::install_generic_sensor_bindings_v8);
        // Default: PERMISSION_DENIED (mirrors lib.rs::install_dom's hardcoded `None`).
        if let Err(e) = crate::geolocation::install_geolocation_bindings_v8(self, None) {
            eprintln!("v8: geolocation::install_geolocation_bindings_v8 failed: {e}");
        }
        install_v8!(highlight_api::install_highlight_api_bindings_v8);
        install_v8!(idle_detection::install_idle_detection_bindings_v8);
        // BUG-480 срез 2: бридж contentWindow/contentDocument получает реестр
        // рантайма явным аргументом (как geolocation/pointer_capture ниже),
        // а не через install_v8! — нативы `_lumen_f_*` читают тот же Arc,
        // куда пишет register_frame_document. Ставится до iframe_element:
        // его геттеры вызывают функции шима бриджа.
        if let Err(e) =
            crate::frame_bridge::install_frame_bridge_v8(self, Arc::clone(&self.frame_docs))
        {
            eprintln!("v8: frame_bridge::install_frame_bridge_v8 failed: {e}");
        }
        install_v8!(iframe_element::install_iframe_element_bindings_v8);
        install_v8!(inert::install_inert_api_v8);
        install_v8!(intl_bindings::install_intl_bindings_v8);
        install_v8!(launch_handler::install_launch_handler_api_v8);
        install_v8!(local_font_access::install_local_font_access_api_v8);
        install_v8!(long_animation_frames::install_long_animation_frames_bindings_v8);
        install_v8!(media_capabilities::install_media_capabilities_bindings_v8);
        install_v8!(media_capture::install_media_capture_bindings_v8);
        install_v8!(media_devices::install_media_devices_bindings_v8);
        install_v8!(media_session::install_media_session_bindings_v8);
        install_v8!(media_stream_recording::install_media_stream_recording_v8);
        install_v8!(navigation_api::install_navigation_api_v8);
        install_v8!(navigator_bindings::install_navigator_bindings_v8);
        install_v8!(network_log_bindings::install_network_log_bindings_v8);
        // Default permission: "denied" (mirrors lib.rs::install_dom's hardcoded `false`).
        if let Err(e) = crate::notifications_bindings::install_notifications_bindings_v8(self, false)
        {
            eprintln!("v8: notifications_bindings::install_notifications_bindings_v8 failed: {e}");
        }
        install_v8!(paint_worklet::install_paint_worklet_api_v8);
        install_v8!(payment_request::install_payment_request_v8);
        install_v8!(periodic_sync::install_periodic_sync_v8);
        install_v8!(permissions::install_permissions_api_v8);
        install_v8!(permissions_policy::install_permissions_policy_bindings_v8);
        install_v8!(pip_bindings::install_pip_bindings_v8);
        // W3C Pointer Events Level 3 §4.1 — takes `pointer_capture_nid` by ref since the
        // native closures need the runtime-instance Arc, not just `&self` (mirrors the
        // `geolocation`/`shared_worker` extra-arg calls above, not the plain `install_v8!` macro).
        if let Err(e) = crate::pointer_capture::install_pointer_capture_bindings_v8(
            self,
            Arc::clone(&self.pointer_capture_nid),
        ) {
            eprintln!("v8: pointer_capture::install_pointer_capture_bindings_v8 failed: {e}");
        }
        install_v8!(presentation_api::install_presentation_api_v8);
        install_v8!(push_api::install_push_api_v8);
        install_v8!(reporting_api::install_reporting_api_bindings_v8);
        install_v8!(sanitizer::install_sanitizer_bindings_v8);
        install_v8!(scheduler::install_scheduler_api_v8);
        install_v8!(screen_capture::install_screen_capture_bindings_v8);
        install_v8!(screen_orientation::install_screen_orientation_bindings_v8);
        install_v8!(scroll_snap_events::install_scroll_snap_events_bindings_v8);
        install_v8!(scroll_timeline::install_scroll_timeline_bindings_v8);
        install_v8!(serial::install_serial_bindings_v8);
        install_v8!(shape_detection::install_shape_detection_bindings_v8);
        install_v8!(shared_storage::install_shared_storage_v8);
        install_v8!(soft_navigation::install_soft_navigation_api_v8);
        install_v8!(speculation_rules::install_speculation_rules_api_v8);
        install_v8!(speech::install_speech_bindings_v8);
        install_v8!(storage_buckets::install_storage_buckets_v8);
        // BUG-372: `navigator.storage.getDirectory()` opens the OPFS subtree of
        // the installing document's origin, so this one takes the origin too.
        if let Err(e) =
            crate::storage_manager::install_storage_manager_bindings_v8(self, &page_origin)
        {
            eprintln!("v8: storage_manager::install_storage_manager_bindings_v8 failed: {e}");
        }
        install_v8!(surface_api::install_surface_api_protection_v8);
        install_v8!(svg::install_svg_bindings_v8);
        install_v8!(tc39_proposals::install_tc39_proposals_v8);
        install_v8!(temporal_api::install_temporal_api_v8);
        install_v8!(topics_api::install_topics_api_v8);
        install_v8!(typed_om_api::install_typed_om_api_v8);
        install_v8!(ua_client_hints::install_ua_client_hints_bindings_v8);
        install_v8!(url_pattern::install_url_pattern_api_v8);
        install_v8!(video_bindings::install_video_bindings_v8);
        install_v8!(video_pip::install_video_pip_api_v8);
        // CSS View Transitions L1 (BUG-545): takes `view_transition_events` by ref since
        // the native closures need the runtime-instance Arc, not just `&self` (mirrors the
        // `geolocation`/`pointer_capture` extra-arg calls above, not the plain `install_v8!` macro).
        if let Err(e) = crate::view_transitions::install_view_transition_bindings_v8(
            self,
            Arc::clone(&self.view_transition_events),
        ) {
            eprintln!("v8: view_transitions::install_view_transition_bindings_v8 failed: {e}");
        }
        install_v8!(virtual_keyboard::install_virtual_keyboard_bindings_v8);
        install_v8!(wake_lock::install_wake_lock_bindings_v8);
        install_v8!(web_audio::install_web_audio_api_v8);
        install_v8!(webhid::install_webhid_bindings_v8);
        install_v8!(web_locks::install_web_locks_bindings_v8);
        install_v8!(web_midi::install_web_midi_api_v8);
        install_v8!(webrtc_stub::install_webrtc_bindings_v8);
        install_v8!(webusb::install_webusb_bindings_v8);
        install_v8!(webxr::install_webxr_bindings_v8);
        install_v8!(window_management::install_window_management_api_v8);
        install_v8!(xhr::install_xhr_bindings_v8);
        install_v8!(web_codecs::install_webcodecs_bindings_v8);
        // Ph3 V8 migration S9: wasm + webgpu (hand-port, same best-effort
        // orchestration as S8's canvas2d/webgl_canvas above).
        install_v8!(webassembly::install_webassembly_bindings_v8);
        install_v8!(webgpu::install_webgpu_bindings_v8);
        // Ph3 V8 migration S10: worker + shared_worker (hand-port — extra
        // per-runtime state beyond `&ctx`, same shape as S5-S7 batch 3's
        // geolocation/broadcast_channel/notifications_bindings, so called
        // directly rather than through the `install_v8!` macro). Service
        // Worker activation (`_lumen_sw_activate_script`) is already wired
        // above in the S3 core-native block, via `spawn_sw_worker_v8`.
        if let Err(e) = crate::worker::install_worker_bindings_v8(
            self,
            &self.workers,
            &self.worker_messages,
            &self.worker_errors,
            &self.worker_next_id,
            &self.worker_blob_store,
            fp_worker,
        ) {
            eprintln!("v8: worker::install_worker_bindings_v8 failed: {e}");
        }
        if let Err(e) = crate::shared_worker::install_shared_worker_bindings_v8(
            self,
            &self.shared_worker_outbox,
            &self.shared_worker_errors,
            fp_shared_worker,
        ) {
            eprintln!("v8: shared_worker::install_shared_worker_bindings_v8 failed: {e}");
        }
        // BUG-378: must be the LAST install step — it hides every internal
        // `_lumen_*` global from enumeration and freezes the function-valued
        // ones, so anything registering a native or patching one afterwards
        // would either stay visible (register) or fail silently (patch). See
        // `internal_globals`'s module docs.
        if let Err(e) = crate::internal_globals::seal_internal_globals_v8(self) {
            eprintln!("v8: internal_globals::seal_internal_globals_v8 failed: {e}");
        }
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;

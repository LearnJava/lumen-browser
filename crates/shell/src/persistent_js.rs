//! The shell's persistent JS context: the [`PersistentJs`] abstraction and its
//! one implementation, [`V8PersistentJs`].
//!
//! The trait is what keeps a JS context alive *between* renders — its closures
//! hold the same `Arc<Mutex<Document>>` as `LayoutSource::document`, so an
//! event-driven DOM mutation is visible to the next relayout without a page
//! reload. Moved out of `main.rs` by the SPLIT track (batch SH-3a); behaviour
//! and signatures are unchanged.

use crate::*;

/// Shell-local abstraction over a persistent JS context that survives between
/// renders. The JS DOM closures hold a reference to the same
/// `Arc<Mutex<Document>>` as `LayoutSource::document`, so event-driven DOM
/// mutations are visible to the next relayout without a full page reload.
// BUG-171 СЌС‚Р°Рї 2: `Send` РЅСѓР¶РµРЅ, С‡С‚РѕР±С‹ РіРѕС‚РѕРІС‹Р№ JS-С…СЌРЅРґР» (`QuickJsRuntime` вЂ”
// `Send + Sync` РїРѕ ADR-014/B-1), СЃРѕР·РґР°РЅРЅС‹Р№ С„РёРЅР°Р»СЊРЅС‹Рј pipeline РЅР° С„РѕРЅРѕРІРѕРј РїРѕС‚РѕРєРµ,
// РїРµСЂРµСЃС‹Р»Р°Р»СЃСЏ РѕР±СЂР°С‚РЅРѕ РЅР° UI-РїРѕС‚РѕРє РІРЅСѓС‚СЂРё `LoadEvent::RenderDone`.
//
// ADR-016 M2.2c-2b: `Sync` РґРѕР±Р°РІР»РµРЅ, С‡С‚РѕР±С‹ С…СЌРЅРґР» РјРѕР¶РЅРѕ Р±С‹Р»Рѕ РґРµСЂР¶Р°С‚СЊ Р·Р°
// `Arc<dyn PersistentJs>` Рё **СЂР°Р·РґРµР»СЏС‚СЊ** РјРµР¶РґСѓ UI-РїРѕС‚РѕРєРѕРј Рё РґРІРёР¶РєРѕРІС‹Рј РїРѕС‚РѕРєРѕРј РЅР°
// РІСЂРµРјСЏ РјРёРіСЂР°С†РёРё `js_ctx` РЅР° РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє (СЃРј. `EngineJsState`). Р’СЃРµ РјРµС‚РѕРґС‹
// СѓР¶Рµ Р±РµСЂСѓС‚ `&self`, Р° `QuickJsRuntime` вЂ” `Send + Sync` (РІСЃРµ РІС‹Р·РѕРІС‹ С‚СѓРЅРЅРµР»РёСЂСѓСЋС‚СЃСЏ
// РЅР° РІС‹РґРµР»РµРЅРЅС‹Р№ JS-РїРѕС‚РѕРє С‡РµСЂРµР· `SyncSender`, ADR-014), РїРѕСЌС‚РѕРјСѓ `Sync` РґРµСЂР¶РёС‚СЃСЏ
// Р±РµР· `unsafe`. UI-СЃС‚РѕСЂРѕРЅРЅРёР№ `Arc`-РєР»РѕРЅ СѓРґР°Р»СЏРµС‚СЃСЏ РІ M2.2c-2d.
pub(crate) trait PersistentJs: Send + Sync {
    /// Evaluate a JS script (event handler dispatch, rAF tick, etc.).
    fn eval_js(&self, script: &str);
    /// Evaluate `script` and return its result as a JSON string (SDC-1b
    /// `AutomationCommand::Eval` вЂ” unlike `eval_js`, the value is not discarded).
    fn eval_js_value(&self, script: &str) -> Result<String, String>;
    /// Consume any navigation request placed by JS during the last `eval_js`.
    fn take_navigate_request(&self) -> Option<JsNavigateRequest>;
    /// Drain `NavigateEvent` intercept results queued since the last call.
    fn take_nav_intercept_result(&self) -> Vec<(bool, bool)>;
    /// Drain all navigation updates queued by JS since the last call.
    fn take_nav_updates(&self) -> Vec<(u8, String, String, String)>;
    /// Fire `navigatesuccess` event on `window.navigation`.
    fn fire_navigate_success(&self);
    /// Fire `navigateerror` event on `window.navigation`.
    fn fire_navigate_error(&self);
    /// Fire `currententrychange` event on `window.navigation`.
    fn fire_current_entry_change(&self);
    /// Drain all expired JS timers (setTimeout/setInterval).
    ///
    /// Called each `about_to_wait`. Timer callbacks run synchronously inside
    /// the JS context and may themselves schedule further timers or navigation.
    fn tick_timers(&self);
    /// Take the next timer wakeup deadline as Unix epoch ms, clearing the stored
    /// value.  Returns `None` if no timers are pending after the last tick.
    fn take_timer_wakeup(&self) -> Option<f64>;
    /// BUG-480 СЃСЂРµР· 8: РµСЃС‚СЊ Р»Рё РІ СЏС‰РёРєР°С… РјРѕСЃС‚Р° РЅРµСЂР°Р·РѕР±СЂР°РЅРЅС‹Рµ РєРѕРЅРІРµСЂС‚С‹,
    /// Р°РґСЂРµСЃРѕРІР°РЅРЅС‹Рµ Р­РўРћРњРЈ РєРѕРЅС‚РµРєСЃС‚Сѓ (РєСЂРѕСЃСЃ-С„СЂРµР№РјРѕРІС‹Рµ postMessage/СЃРѕР±С‹С‚РёСЏ/
    /// RunScript). РЁРµР»Р» РґРµСЂР¶РёС‚ РєРѕСЂРѕС‚РєРёР№ poll-РґРµРґР»Р°Р№РЅ, РїРѕРєР° С…РѕС‚СЊ РѕРґРёРЅ Р¶РёРІРѕР№
    /// РєРѕРЅС‚РµРєСЃС‚ РѕС‚РІРµС‡Р°РµС‚ В«РґР°В» вЂ” РёРЅР°С‡Рµ РґРѕСЃС‚Р°РІРєР° РїРѕСЃР»Рµ Р·Р°С‚РёС…Р°РЅРёСЏ СЃС‚СЂР°РЅРёС†С‹ Р¶РґС‘С‚
    /// СЃР»СѓС‡Р°Р№РЅРѕРіРѕ РїСЂРѕР±СѓР¶РґРµРЅРёСЏ С†РёРєР»Р°. Default РґР»СЏ РґРІРёР¶РєРѕРІ Р±РµР· РјРѕСЃС‚Р°.
    fn frame_transport_pending(&self) -> bool {
        false
    }
    /// BUG-480 срез 25: забрать (и сбросить) флаг «этот под-документ мутирован
    /// МОСТОМ» — родитель писал в него через `contentDocument`/фасады
    /// (setAttribute/appendChild/…) в СВОЁМ изоляте, поэтому обычный
    /// [`Self::take_dom_dirty`] ребёнка такую мутацию не видит: она прошла
    /// мимо его собственных нативов. Default `false` для движков без моста
    /// (`NullPersistentJs`, минимальные тестовые изоляты).
    fn take_frame_dom_dirty(&self) -> bool {
        false
    }
    /// Returns `true` if JS mutated the DOM since the last call, clearing the flag.
    ///
    /// Called after each rAF pass in `RedrawRequested`; when `true`, a relayout
    /// must happen before the next paint to reflect DOM changes.
    fn take_dom_dirty(&self) -> bool;
    /// BUG-341 S7: drain the page-side DOM-mutation tracker since the last
    /// call вЂ” feeds `lumen_layout::style::restyle_root_set_for_node_change`
    /// so `Lumen::try_relayout_raf_incremental` can take the incremental-
    /// cascade path (`layout_mutation_incremental_restyle`) instead of a full
    /// cascade for JS DOM mutations.
    ///
    /// Default (used by engines without a tracker вЂ” `NullPersistentJs`):
    /// no touched nodes but `unattributed: true`, forcing the caller to fall
    /// back to a full cascade вЂ” preserves those engines' existing behaviour
    /// exactly.
    fn take_dom_touched(&self) -> DomTouchedSummary {
        DomTouchedSummary { nodes: std::collections::HashSet::new(), unattributed: true }
    }
    /// BUG-272/BUG-306 diagnostics: JS engine heap `(total_heap_size,
    /// used_heap_size)` in bytes; `(-1, -1)` when the runtime does not expose
    /// it. `V8PersistentJs` overrides this via `V8JsRuntime::debug_heap_stats`.
    fn debug_js_heap(&self) -> (i64, i64) {
        (-1, -1)
    }
    /// Run all pending `requestAnimationFrame` callbacks with `timestamp_ms`.
    ///
    /// Called in `RedrawRequested` before paint. Callbacks may register new rAF
    /// callbacks (animation loop); use `take_raf_pending` to detect this.
    fn run_animation_frame(&self, timestamp_ms: f64);
    /// Returns `true` if `requestAnimationFrame` was called since the last
    /// `take_raf_pending`, clearing the flag.
    ///
    /// Shell requests another redraw when this returns `true` so animation loops
    /// continue without busy-polling.
    fn take_raf_pending(&self) -> bool;
    /// Non-consuming peek: `true` if rAF callbacks are queued (does not clear).
    ///
    /// Used by the vsync gate: check without consuming so the signal is not lost
    /// when the gate defers firing to the next frame.
    fn has_raf_pending(&self) -> bool;
    /// ADR-016 M2.3: shared, lock-free handle to the rAF-pending flag.
    ///
    /// Returns `None` for runtimes that do not expose it (default). The
    /// `v8` runtime returns a clone of its `Arc<AtomicBool>`, letting the
    /// UI thread read the flag without a blocking engine-thread `query` вЂ” the
    /// scheduling read must never serialize behind an in-flight (long) JS turn.
    fn raf_pending_flag(&self) -> Option<Arc<std::sync::atomic::AtomicBool>> {
        None
    }
    /// ADR-016 M2.3: shared, lock-free handle to the DOM-dirty flag (companion
    /// to [`Self::raf_pending_flag`]). `None` when unsupported (default).
    fn dom_dirty_flag(&self) -> Option<Arc<std::sync::atomic::AtomicBool>> {
        None
    }
    /// Push a fresh snapshot of layout bounding rects into the JS runtime.
    ///
    /// Called after every `relayout_page`. The JS side uses this for
    /// `getBoundingClientRect`, `ResizeObserver`, and `IntersectionObserver`.
    #[allow(dead_code)] // called only from #[cfg(feature = "v8")] blocks
    fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>);
    /// Push a fresh `LayoutBox` tree snapshot for `document.elementFromPoint`/
    /// `elementsFromPoint` (BUG-464/BUG-477).
    ///
    /// Called alongside [`Self::update_layout_rects`], same tree the shell
    /// just built — so a hit test agrees with the geometry `getBoundingClientRect`
    /// already reports.
    #[allow(dead_code)] // called only from #[cfg(feature = "v8")] blocks
    fn update_hit_test_tree(&self, tree: Arc<lumen_layout::LayoutBox>);
    /// Update the current viewport dimensions in the JS runtime.
    ///
    /// Called after every resize and on initial load.
    #[allow(dead_code)] // called only from #[cfg(feature = "v8")] blocks
    fn update_viewport_size(&self, width: f32, height: f32);
    /// Call `_lumen_deliver_resize_observers()` and
    /// `_lumen_deliver_intersection_observers()` in JS.
    ///
    /// Must be called after `update_layout_rects` so that observers read fresh
    /// geometry. Called by the shell after every `relayout_page`.
    #[allow(dead_code)] // called only from #[cfg(feature = "v8")] blocks
    fn deliver_layout_observers(&self);
    /// Register lazy images for deferred IntersectionObserver-style proximity loading.
    ///
    /// Called once after the initial page load with `(node_id, url)` pairs for every
    /// `<img loading="lazy">` element.  Subsequent proximity checks happen via
    /// `deliver_lazy_images()` after each relayout.
    #[allow(dead_code)]
    fn register_lazy_images(&self, pairs: &[(u32, &str)]);
    /// Push decoded `<img>` bitmaps `(nid, Arc<Image>)` into the JS canvas drawImage store.
    ///
    /// Call after `fetch_and_decode_images` so `drawImage(imgElement, вЂ¦)` works.
    /// The `Arc` is shared with the decoded-image cache вЂ” no pixel copy (BUG-272
    /// СЃСЂРµР· 20). Default no-op covers non-QuickJS builds and `NullPersistentJs`.
    #[allow(dead_code)]
    fn register_img_bitmaps(&self, _bitmaps: Vec<(u32, Arc<lumen_image::Image>)>) {}
    /// Check registered lazy images against the current viewport and enqueue load
    /// requests for those within the lazy-load margin (1 viewport ahead of the fold).
    ///
    /// Must be called after `deliver_layout_observers` (fresh rects in JS).
    #[allow(dead_code)]
    fn deliver_lazy_images(&self);
    /// Drain lazy image load requests queued by JS since the last call.
    ///
    /// Returns `(node_id, url)` pairs for images that entered the lazy-load margin.
    #[allow(dead_code)]
    fn take_lazy_image_requests(&self) -> Vec<(u32, String)>;
    /// BUG-480 СЃСЂРµР· 2: Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ Р·Р°РіСЂСѓР¶РµРЅРЅС‹Р№ РїРѕРґ-РґРѕРєСѓРјРµРЅС‚ `<iframe>` РІ
    /// JS-РєРѕРЅС‚РµРєСЃС‚Рµ СЂРѕРґРёС‚РµР»СЏ вЂ” РїРѕСЃР»Рµ СЌС‚РѕРіРѕ `iframe.contentWindow`/
    /// `contentDocument` РёР· СЃРєСЂРёРїС‚РѕРІ СЂРѕРґРёС‚РµР»СЏ РІРёРґСЏС‚ С„Р°СЃР°РґС‹ РїРѕРґ-РґРѕРєСѓРјРµРЅС‚Р°
    /// (`crates/js/src/frame_bridge.rs`).
    ///
    /// Р’С‹Р·С‹РІР°РµС‚СЃСЏ РёР· [`load_frame_sub_documents`] РїРѕСЃР»Рµ РёСЃРїРѕР»РЅРµРЅРёСЏ СЃРєСЂРёРїС‚РѕРІ
    /// СЂРµР±С‘РЅРєР° Рё **РґРѕ** РґРёСЃРїР°С‚С‡Р° trusted `load` РЅР° С…РѕСЃС‚Рµ. `name` вЂ” Р·РЅР°С‡РµРЅРёРµ
    /// Р°С‚СЂРёР±СѓС‚Р° `name` С…РѕСЃС‚Р° (РєР»СЋС‡ РёРјРµРЅРѕРІР°РЅРЅРѕРіРѕ РґРѕСЃС‚СѓРїР° `window[name]`,
    /// СЃСЂРµР· 3). `accessible=false`
    /// (cross-origin / opaque sandbox) СЂРµРіРёСЃС‚СЂРёСЂСѓРµС‚ Р±РёРЅРґРёРЅРі Р±РµР· РґРѕСЃС‚СѓРїР° Рє
    /// СЃРѕРґРµСЂР¶РёРјРѕРјСѓ: `contentWindow` РµСЃС‚СЊ, `contentDocument` вЂ” `null`.
    /// Default no-op РїРѕРєСЂС‹РІР°РµС‚ СЃР±РѕСЂРєРё Р±РµР· v8.
    fn register_iframe_document(
        &self,
        _host_nid: u32,
        _doc: Arc<Mutex<Document>>,
        _url: &str,
        _name: Option<&str>,
        _accessible: bool,
    ) {
    }
    /// BUG-480 СЃСЂРµР· 3: Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ РґРѕРєСѓРјРµРЅС‚ СЂРѕРґРёС‚РµР»СЏ РІ JS-РєРѕРЅС‚РµРєСЃС‚Рµ
    /// С„СЂРµР№РјР° вЂ” РІРЅСѓС‚СЂРё С„СЂРµР№РјР° `window.parent`/`window.frameElement`/`window.name`
    /// РІРёРґСЏС‚ С„Р°СЃР°Рґ СЂРѕРґРёС‚РµР»СЊСЃРєРѕР№ СЃС‚РѕСЂРѕРЅС‹ (`crates/js/src/frame_bridge.rs`).
    ///
    /// Р’С‹Р·С‹РІР°РµС‚СЃСЏ РёР· [`load_frame_sub_documents`] СЃСЂР°Р·Сѓ РїРѕСЃР»Рµ СЃРѕР·РґР°РЅРёСЏ
    /// РєРѕРЅС‚РµРєСЃС‚Р° СЂРµР±С‘РЅРєР° Рё РґРѕ РµРіРѕ DOMContentLoaded/load: РѕР±СЂР°Р±РѕС‚С‡РёРєРё СЂРµР±С‘РЅРєР°
    /// С‡РёС‚Р°СЋС‚ РїСЂРµРґРєРѕРІ РёР· Р»СЋР±РѕРіРѕ СЃРѕР±С‹С‚РёСЏ. `host_nid` вЂ” nid С…РѕСЃС‚Р° РІ РґРµСЂРµРІРµ
    /// СЂРѕРґРёС‚РµР»СЏ. Default no-op РїРѕРєСЂС‹РІР°РµС‚ СЃР±РѕСЂРєРё Р±РµР· v8.
    fn register_parent_document(
        &self,
        _host_nid: u32,
        _doc: Arc<Mutex<Document>>,
        _url: &str,
        _accessible: bool,
    ) {
    }
    /// BUG-480 СЃСЂРµР· 3: Р·Р°СЂРµРіРёСЃС‚СЂРёСЂРѕРІР°С‚СЊ РґРѕРєСѓРјРµРЅС‚ РІРµСЂС…РЅРµРіРѕ РѕРєРЅР° РІ JS-РєРѕРЅС‚РµРєСЃС‚Рµ
    /// С„СЂРµР№РјР° РіР»СѓР±РёРЅС‹ в‰Ґ 2 (`window.top` РІРµРґС‘С‚ РІ РєРѕСЂРµРЅСЊ, Р° РЅРµ РІ РЅРµРїРѕСЃСЂРµРґСЃС‚РІРµРЅРЅРѕРіРѕ
    /// СЂРѕРґРёС‚РµР»СЏ). Р”Р»СЏ С„СЂРµР№РјР° РїРµСЂРІРѕРіРѕ СѓСЂРѕРІРЅСЏ РЅРµ РІС‹Р·С‹РІР°РµС‚СЃСЏ вЂ” С‚Р°Рј top СЂР°Р·СЂРµС€Р°РµС‚СЃСЏ
    /// С‡РµСЂРµР· [`PersistentJs::register_parent_document`]. Default no-op Р±РµР· v8.
    fn register_top_document(&self, _doc: Arc<Mutex<Document>>, _url: &str, _accessible: bool) {}
    /// Deliver a PerformancePaintTiming entry to JS PerformanceObservers.
    ///
    /// `name` is `"first-paint"` or `"first-contentful-paint"`;
    /// `start_ms` is the DOMHighResTimeStamp relative to performance.timeOrigin.
    /// Calls `_lumen_deliver_paint_entry(name, start_ms)` in QuickJS.
    #[allow(dead_code)]
    fn deliver_paint_timing(&self, name: &str, start_ms: f64);
    /// Deliver a PerformanceNavigationTiming entry to JS PerformanceObservers.
    ///
    /// Called after a page load completes. `url` is the navigation URL;
    /// `duration_ms` is total load time (Navigation Timing L2 В§4.2 `duration`).
    /// Calls `_lumen_deliver_perf_entry('navigation', url, 0.0, duration_ms, detail)`.
    fn deliver_nav_timing(&self, url: &str, duration_ms: f64);
    /// Hand a batch of engine-issued subresource loads to the page's Resource
    /// Timing buffer (BUG-839).
    ///
    /// `rows_json` is the array [`crate::resource_timing::rows_to_json`] builds.
    /// A batch rather than one call per load because the loads arrive on
    /// worker threads while this runs once per event-loop step: a page pulling
    /// forty images would otherwise cost forty JS hops in one tick.
    fn deliver_resource_timings(&self, rows_json: &str);
    /// Deliver a LargestContentfulPaint entry to JS PerformanceObservers.
    ///
    /// Called when a large content element (>500pxВІ) is rendered.
    /// `element_id` = NID; `size` = area in pixels; `render_time_ms` = render completion timestamp.
    #[allow(dead_code)]
    fn deliver_lcp_entry(&self, element_id: u32, size: u32, start_ms: f64, render_time_ms: f64);
    /// Deliver a LayoutShift entry to JS PerformanceObservers (CLS metric).
    ///
    /// Called when layout shift is detected during reflow (shift >5px).
    /// `value` = fractional shift distance; `had_input` = whether user input occurred recently.
    #[allow(dead_code)]
    fn deliver_layout_shift(&self, value: f64, had_input: bool);
    /// Push a fresh snapshot of computed CSS styles into the JS runtime.
    ///
    /// Called after every `relayout_page`. The JS side uses this for
    /// `window.getComputedStyle()` and CSS property reads.
    #[allow(dead_code)]
    fn update_computed_styles(&self, styles: HashMap<u32, HashMap<String, String>>);
    /// Push a fresh snapshot of resolved CSS custom properties into the JS
    /// runtime (BUG-732).
    ///
    /// Published from every place that publishes computed styles вЂ” the two
    /// snapshots are the two halves of what `window.getComputedStyle()` can
    /// answer, and a page that gets only the first reports `""` for every
    /// `var()`-declared value.
    #[allow(dead_code)]
    fn update_custom_properties(&self, props: HashMap<u32, Arc<HashMap<String, String>>>);
    /// Advance `document.readyState` to `"interactive"` and fire
    /// `readystatechange` + `DOMContentLoaded` on `document`.
    ///
    /// Call after HTML is fully parsed and inline scripts have run.
    #[allow(dead_code)]
    fn notify_dom_content_loaded(&self);
    /// Advance `document.readyState` to `"complete"` and fire
    /// `readystatechange` on `document` + `load` on `window`.
    ///
    /// Call after all subresources (images, fonts) are decoded and registered.
    #[allow(dead_code)]
    fn notify_window_loaded(&self);
    /// Notify all registered `MediaQueryList` instances that the viewport or
    /// user preferences changed (CSS Media Queries L4 В§4.2). Each MQL whose
    /// `matches` flipped fires a `change` event on its listeners.
    ///
    /// Must be called after `update_viewport_size` so JS reads consistent
    /// dimensions. Shell calls it after every `relayout_page` and any
    /// `prefers-color-scheme` or `prefers-reduced-motion` toggle.
    #[allow(dead_code)]
    fn deliver_media_query_changes(&self, width: f32, height: f32, prefers_dark: bool, reduced_motion: bool);
    /// Poll all live `WebSocket` instances and deliver queued events to JS.
    ///
    /// Must be called on every event-loop step so that `onopen`/`onmessage`/
    /// `onclose`/`onerror` handlers fire promptly. Calls `_lumen_pump_websockets()`
    /// which drains `_lumen_ws_poll()` for every open handle.
    #[allow(dead_code)]
    fn pump_websockets(&self);
    /// Poll all live `EventSource` instances and deliver queued SSE events to JS.
    ///
    /// Must be called on every event-loop step so that `onopen`/`onmessage`/
    /// `onerror` handlers fire promptly. Calls `_lumen_pump_sse()` which drains
    /// `_lumen_sse_poll()` for every open handle (HTML Living Standard В§9.2).
    #[allow(dead_code)]
    fn pump_sse(&self);
    /// Deliver messages posted by Web Worker threads to their `Worker` JS instances.
    ///
    /// Must be called on every event-loop tick alongside `tick_timers()` so that
    /// `onmessage` / `addEventListener('message', fn)` handlers fire promptly.
    #[allow(dead_code)]
    fn pump_workers(&self);
    /// Deliver messages posted to same-origin `BroadcastChannel` instances.
    ///
    /// Must be called on every event-loop tick alongside `pump_workers()` so
    /// that `onmessage` / `addEventListener('message', fn)` handlers fire when
    /// another context (tab/worker) broadcasts on a shared channel name.
    #[allow(dead_code)]
    fn pump_broadcast_channels(&self);
    /// Deliver messages posted by `SharedWorker` threads to this page's ports.
    ///
    /// Must be called on every event-loop tick alongside `pump_workers()` so that
    /// each client `port`'s `onmessage` / `addEventListener('message', fn)` fires
    /// when a shared worker replies (WHATWG HTML В§10.2).
    #[allow(dead_code)]
    fn pump_shared_workers(&self);
    /// BUG-480 СЃСЂРµР· 4: СЂР°Р·РѕР±СЂР°С‚СЊ СЏС‰РёРє РєСЂРѕСЃСЃ-С„СЂРµР№РјРѕРІС‹С… postMessage
    /// (`crates/js/src/frame_bridge.rs`) Рё РґРѕСЃС‚Р°РІРёС‚СЊ Р°РґСЂРµСЃРѕРІР°РЅРЅС‹Рµ СЌС‚РѕРјСѓ
    /// РєРѕРЅС‚РµРєСЃС‚Сѓ СЃРѕРѕР±С‰РµРЅРёСЏ РєР°Рє MessageEvent РІ window.onmessage /
    /// addEventListener('message'). Р’С‹Р·С‹РІР°РµС‚СЃСЏ РЅР° РєР°Р¶РґРѕРј С‚РёРєРµ СЂСЏРґРѕРј СЃ
    /// pump_broadcast_channels вЂ” Рё Сѓ СЃС‚СЂР°РЅРёС†С‹, Рё Сѓ С…СЌРЅРґР»РѕРІ С„СЂРµР№РјРѕРІ.
    #[allow(dead_code)]
    fn pump_frame_messages(&self) {}
    /// Drain OS notification requests queued by `new Notification(...)` in JS.
    ///
    /// Shell calls this in `about_to_wait` and forwards each entry to
    /// `notification::show_os_notification`. Returns an empty vec when no
    /// notifications were created since the last drain.
    #[allow(dead_code)]
    fn take_notification_requests(&self) -> Vec<(String, String)>;
    /// Purge JS-side per-node caches for nodes that have been detached from
    /// the DOM and have zero live JS references.
    ///
    /// Calls `_lumen_gc_collect(nids)` in QuickJS, which removes event-listener
    /// and input-value entries from `_lumen_listeners` / `_input_values` for
    /// the supplied node IDs.  Called by the shell's idle GC tick.
    #[allow(dead_code)]
    fn gc_collect(&self, dead_nids: &[u32]);
    /// Drain popup window requests queued by JS `window.open(...)`.
    ///
    /// Returns `(url, target, width_px, height_px)` tuples. Shell opens a new
    /// tab navigated to `url` for each entry. Returns an empty vec between
    /// `window.open()` calls.
    #[allow(dead_code)]
    fn take_window_open_requests(&self) -> Vec<(String, String, u32, u32)>;
    /// Drain `console.log/warn/error` messages buffered in the JS runtime.
    ///
    /// Each entry is `(level, text)` where level is 0=log, 1=warn, 2=error.
    /// Called by the shell in `about_to_wait` to feed the DevTools console panel.
    /// Returns an empty vec when no console calls have been made since last drain.
    #[allow(dead_code)]
    fn take_console_messages(&self) -> Vec<(u8, String)>;
    /// Push a fresh snapshot of per-node scroll state into the JS runtime.
    ///
    /// Maps NodeId index в†’ `[scroll_x, scroll_y, scroll_width, scroll_height]`.
    /// Called after every `relayout_page` so JS reads `scrollTop`/`scrollLeft`/
    /// `scrollWidth`/`scrollHeight` consistently.
    #[allow(dead_code)]
    fn update_scroll_states(&self, states: HashMap<u32, [f32; 4]>);
    /// Drain programmatic scroll requests queued by JS (`scrollTo`/`scrollBy`/
    /// `scrollIntoView`/`scrollTop=`).
    ///
    /// Returns `(node_id, target_scroll_x, target_scroll_y)` tuples. Shell
    /// applies each via `set_scroll_position()`. Empty when none are pending.
    #[allow(dead_code)]
    fn take_scroll_requests(&self) -> Vec<(u32, f32, f32)>;
    /// Drain `history.pushState` / `history.replaceState` URL-update notifications.
    ///
    /// Each entry is `(is_push, url, new_state_json)` where `is_push = true`
    /// means `pushState` (adds a same-document entry to nav_back) and `false`
    /// means `replaceState` (updates the displayed URL only).
    #[allow(dead_code)]
    fn take_history_url_updates(&self) -> Vec<(bool, String, String)>;
    /// Drain `history.go(n)` / `back` / `forward` traversal deltas queued by JS.
    ///
    /// Each `delta` (negative = back, positive = forward) is applied by the shell
    /// to its real `nav_back`/`nav_fwd` stacks via `Lumen::navigate_by`, which
    /// delivers the destination popstate or reload. The shell is the single
    /// authority for traversal; the JS `HistoryState` is only a read-cache.
    #[allow(dead_code)]
    fn take_history_traversals(&self) -> Vec<i32>;
    /// Fire a `popstate` event in JS for a same-document back/forward navigation.
    ///
    /// `state_json` is the already-serialised state for the destination entry.
    /// `url` is the virtual address-bar URL to restore (may be empty).
    /// Calls `_lumen_deliver_popstate(state_json, url)` via `eval_js`.
    #[allow(dead_code)]
    fn fire_popstate(&self, state_json: &str, url: &str);
    /// Fire a page-lifecycle event (`pageshow` / `pagehide`) on `window`
    /// (HTML Living Standard В§8.6 вЂ” back/forward cache).
    ///
    /// `event` is `"pageshow"` or `"pagehide"` (always a fixed literal supplied
    /// by the shell вЂ” never user data). `persisted` is the `PageTransitionEvent`
    /// `.persisted` flag: `true` when the page was/will be retained in bfcache
    /// (restorable without a reload), `false` for a fresh load or a discarded
    /// page. Delivered via `_lumen_fire_page_lifecycle` in the JS shim.
    #[allow(dead_code)]
    fn fire_page_lifecycle(&self, event: &str, persisted: bool) {
        self.eval_js(&format!(
            "_lumen_fire_page_lifecycle('{event}', {persisted})"
        ));
    }
    /// Run the spec's В«unload a documentВ» steps on the outgoing page
    /// (HTML LS В§7.4.6): `pagehide` в†’ `visibilityState = 'hidden'` в†’ `unload`.
    ///
    /// `persisted` is the `PageTransitionEvent` flag AND the salvageable state:
    /// `true` means the shell retained the document (parked/frozen), so `unload`
    /// must NOT fire; `false` means the document is discarded and `unload` does.
    /// Delivered via `_lumen_unload_document` in the JS shim. BUG-834.
    #[allow(dead_code)]
    fn unload_document(&self, persisted: bool) {
        self.eval_js(&format!("_lumen_unload_document({persisted})"));
    }
    /// Run the spec's В«prompt to unload a documentВ» steps (HTML LS В§7.4.5) вЂ”
    /// dispatch `beforeunload` on the outgoing page.
    ///
    /// The page's answer (В«I asked to stayВ») is deliberately not honoured: that
    /// needs a user-facing confirm dialog, which this engine does not have.
    /// See BUG-834 and `_lumen_fire_beforeunload` in the JS shim.
    #[allow(dead_code)]
    fn fire_beforeunload(&self) {
        self.eval_js("_lumen_fire_beforeunload()");
    }
    /// Drain dirty `<canvas>` 2D pixel buffers for upload to the renderer.
    ///
    /// Returns `(node_index, width, height, rgba)` for every canvas drawn to
    /// since the last drain. Shell registers each as
    /// `Renderer::register_image("canvas:{nid}", ...)` and requests a repaint.
    /// Returns an empty vec when no canvas was drawn (HTML LS В§4.12.4).
    #[allow(dead_code)]
    fn flush_canvas_updates(&self) -> Vec<(u32, u32, u32, Vec<u8>)>;
    /// Drain fullscreen requests queued by `element.requestFullscreen()` and
    /// `document.exitFullscreen()` (WHATWG Fullscreen В§4).
    ///
    /// Each entry is `(enter, nid)`: `enter = true` means enter OS fullscreen
    /// for the element with the given node index; `false` means exit fullscreen
    /// (`nid` is ignored). Shell calls `window.set_fullscreen(Borderless)` /
    /// `window.set_fullscreen(None)` accordingly.
    #[allow(dead_code)]
    fn take_fullscreen_requests(&self) -> Vec<(bool, u32)>;
    /// Drain CSS View Transition events from `document.startViewTransition`.
    ///
    /// Shell drains these in `about_to_wait`: `Begin` captures old display list,
    /// `End` triggers relayout and starts 300 ms cross-fade.
    #[allow(dead_code)]
    fn take_view_transition_events(&self) -> Vec<ViewTransitionEvent>;
    /// Drain print requests emitted by `window.print()` (W-2).
    ///
    /// Shell drains these in `about_to_wait`: each entry triggers print-preview
    /// dialog or direct PDF export.
    #[allow(dead_code)]
    fn take_print_requests(&self) -> Vec<lumen_js::PrintRequest>;
    /// Drain page-level scroll requests from JS `window.scrollTo` / `window.scrollBy`.
    ///
    /// Returns `(target_y, smooth)` pairs where `smooth` indicates whether the scroll
    /// should be animated (CSS Scroll Behavior L1).
    #[allow(dead_code)]
    fn take_page_scroll_requests(&self) -> Vec<(f32, bool)>;
    /// Synchronize the current page scroll position (`window.scrollY`) into the JS runtime.
    ///
    /// Called after scroll updates to keep JS reads of `window.scrollY` accurate.
    ///
    /// Returns `true` when the value actually differs from the one the runtime
    /// held вЂ” i.e. when CSSOM-View В§14 В«run the scroll stepsВ» must fire a
    /// `scroll` event for the viewport this frame (BUG-821). The comparison
    /// lives in the runtime rather than in the shell on purpose: the previous
    /// position must be per-*document*, so a navigation that resets `scroll_y`
    /// hands the fresh runtime its own zero instead of firing a phantom
    /// `scroll` on the new document.
    #[allow(dead_code)]
    fn set_page_scroll_y(&self, y: f32) -> bool;
    /// Adjust the QuickJS GC based on the tab's lifecycle tier (10L).
    ///
    /// `level` encodes aggressiveness: 0 = Soft (active tab, reset threshold),
    /// 1 = Moderate (T1 background, one GC cycle), 2 = Aggressive (T2+ background,
    /// full GC + lowered threshold to keep heap small during long idle).
    #[allow(dead_code)]
    fn run_gc_pass(&self, level: u8);
    /// Push viewport scroll progress into all active root-viewport `ScrollTimeline` instances.
    ///
    /// `progress_y` = block-axis fraction `[0.0, 1.0]` (scroll_y / max_scroll_y).
    /// `progress_x` = inline-axis fraction `[0.0, 1.0]` (scroll_x / max_scroll_x).
    ///
    /// Called after each scroll update in `RedrawRequested` step 1. Drives
    /// CSS Scroll-Driven Animations L1 В§3 (CSS Scroll-Driven Animations Level 1).
    #[allow(dead_code)]
    fn deliver_scroll_progress(&self, progress_y: f32, progress_x: f32);

    /// Fire a non-bubbling `scroll` Event on the element identified by `nid`.
    ///
    /// Called after every overflow-container scroll-position change (both
    /// wheel-driven and JS-programmatic). Per WHATWG HTML В§8.1.6.2.
    #[allow(dead_code)]
    fn fire_element_scroll(&self, nid: u32);

    /// Fire a non-bubbling `scroll` Event on the `window` object.
    ///
    /// Called whenever the page-level scroll position changes.
    /// Per WHATWG HTML В§8.1.6.2.
    #[allow(dead_code)]
    fn fire_window_scroll(&self);

    /// Fire a non-bubbling `scrollend` Event on the element identified by `nid`
    /// (CSSOM-View В§14). Called once an overflow container has *finished*
    /// scrolling; both its scroll paths are instant, so that is the same frame
    /// as [`Self::fire_element_scroll`].
    #[allow(dead_code)]
    fn fire_element_scrollend(&self, nid: u32);

    /// Whether the viewport owes a `scrollend` on this rendering update
    /// (BUG-822). Delegates to the runtime, which holds the debt per document вЂ”
    /// see `V8JsRuntime::page_scrollend_due` for the `moved`/`settled` contract.
    #[allow(dead_code)]
    fn page_scrollend_due(&self, moved: bool, settled: bool) -> bool;

    /// Fire a non-bubbling `scrollend` Event on the `window` object
    /// (CSSOM-View В§14), once page scrolling has come to a stop.
    #[allow(dead_code)]
    fn fire_window_scrollend(&self);

    /// Fire a non-bubbling, non-cancelable `resize` Event on the `window`
    /// object (HTML LS В§7.4.4 В«Firing events using the resize algorithmВ»).
    ///
    /// FRAME-1: called whenever a sub-document's viewport (its `<iframe>`
    /// host box, per HTML LS В§4.8.5) actually changes size вЂ” the frame
    /// counterpart of `WindowEvent::Resized` on the top-level page.
    #[allow(dead_code)]
    fn fire_window_resize(&self);

    /// Deliver a batch of `content-visibility: auto` state changes to JS as
    /// `contentvisibilityautostatechange` events (CSS Contain L2 В§4.1, BUG-852).
    ///
    /// `payload` is a JSON array of `[node_index, skipped]` pairs in tree order.
    #[allow(dead_code)]
    fn deliver_cv_state_changes(&self, payload: &str);

    /// Pause the JS event loop (T0 в†’ T1 lifecycle transition).
    ///
    /// Sets `document.visibilityState = "hidden"`, fires `visibilitychange`.
    /// Called when a tab is sent to background in `switch_tab`.
    /// No-op default for runtimes that don't support it.
    #[allow(dead_code)]
    fn pause_event_loop(&self) {}

    /// Resume the JS event loop (T1 в†’ T0 lifecycle transition).
    ///
    /// Sets `document.visibilityState = "visible"`, fires `visibilitychange`.
    /// Called when a background tab is brought to foreground in `switch_tab`.
    /// No-op default for runtimes that don't support it.
    #[allow(dead_code)]
    fn unpause_event_loop(&self) {}

    /// Drain JS focus requests queued by `_lumen_request_focus` / `_lumen_request_blur`.
    ///
    /// `None` = clear focus (blur); `Some(nid)` = focus that node. Populated by
    /// `showModal()` (focus autofocus descendant or dialog) and `close()` (restore
    /// previous focus). Shell applies each to `self.focused_node` and requests a
    /// relayout so `:focus` / `:focus-within` CSS rules update.
    #[allow(dead_code)]
    fn take_focus_requests(&self) -> Vec<Option<u32>> { Vec::new() }

    /// Close a `<dialog>` as a result of a `<form method="dialog">` submission.
    ///
    /// Calls `dialog.close(return_value)` in the JS runtime so the `close` event
    /// fires and `returnValue` is updated. `dialog_nid` is the dialog's node index;
    /// `return_value` is the submit button's `value` attribute (empty string if none).
    #[allow(dead_code)]
    fn fire_dialog_close(&self, _dialog_nid: u32, _return_value: &str) {}

    /// Notify the JS runtime that the shell moved keyboard focus to a new node.
    ///
    /// Runs the shim's focus-update steps (BUG-381): sets `document.activeElement`,
    /// fires `blur`/`focusout`/`focus`/`focusin` and updates `_lumen_last_focused_nid`
    /// so `showModal()` can record it for restoration when the dialog closes.
    /// `nid = None` means focus was cleared. Idempotent вЂ” echoing a focus the page
    /// itself just requested via `element.focus()` dispatches nothing.
    #[allow(dead_code)]
    fn notify_focus_changed(&self, _nid: Option<u32>) {}

    /// Return the node ID of the current pointer capture target, if any.
    ///
    /// Non-consuming: the capture stays active until `take_pointer_capture()`.
    /// Returns `None` when no capture is active.
    /// Default: no capture support (always `None`).
    #[allow(dead_code)]
    fn pointer_capture_nid(&self) -> Option<u32> { None }

    /// Suspend the JS runtime to a heap snapshot for bfcache freeze.
    ///
    /// Called when navigating away from a page to preserve JS state without
    /// serializing the DOM (DOM is serialized separately via Document::to_bytes).
    /// Returns a compressed heap blob or empty vec if suspension fails/too large.
    #[allow(dead_code)] // called only for bfcache freeze
    fn suspend(&mut self) -> SuspendedHeap { SuspendedHeap::default() }
    /// Whether the page has open realtime connections (`WebSocket`/`EventSource`
    /// in `readyState === OPEN`) or registered `unload`/`beforeunload` handlers.
    ///
    /// Both disqualify a page from the full bfcache freeze per HTML Living
    /// Standard В§8.6 вЂ” [`Lumen::bfcache_eligible`] falls back to the
    /// HTML-snapshot path when this returns `true`. Default `false` (no
    /// blockers) covers runtimes without this introspection.
    #[allow(dead_code)] // called only for bfcache eligibility check
    fn has_bfcache_freeze_blocker(&self) -> bool { false }
    /// Atomically clear and return the current pointer capture target node ID.
    ///
    /// Called by the shell after `pointerup` (implicit release per W3C Pointer Events
    /// L3 В§4.1).  Returns `None` if no capture was active.
    #[allow(dead_code)]
    fn take_pointer_capture(&self) -> Option<u32> { None }
}

/// V8-backed [`PersistentJs`] adapter (Ph3 V8 migration S4).
///
/// Methods backed by state wired in `install_dom` (S3 core DOM) delegate to `V8JsRuntime` accessors;
/// methods for subsystems not yet ported to V8 (view transitions, bfcache
/// heap suspend вЂ” see `docs/tasks/ph3-v8-migration.md` slices S11) use the
/// trait's own default no-op/empty implementation or a local stub, and start
/// returning real data once their slice lands. Workers (dedicated + shared +
/// service) were wired in S10; pointer capture in S12b-20.
#[cfg(feature = "v8")]
pub(crate) struct V8PersistentJs {
    pub(crate) rt: lumen_js::v8_runtime::V8JsRuntime,
}

/// Build the `_lumen_deliver_popstate(...)` call a same-document traversal
/// evaluates (see `PersistentJs::fire_popstate`).
///
/// Split out of `fire_popstate` so the two argument encodings can be asserted
/// without a live runtime вЂ” they are not the same and BUG-829 was exactly that
/// confusion. `state_json` is JSON **text** that the shim parses, so it goes in
/// as a JS string literal; `url` is an ordinary string in single quotes.
#[cfg(feature = "v8")]
pub(crate) fn popstate_eval_source(state_json: &str, url: &str) -> String {
    let escaped = url.replace('\\', "\\\\").replace('\'', "\\'");
    let state_lit =
        serde_json::to_string(state_json).unwrap_or_else(|_| String::from("\"null\""));
    format!("_lumen_deliver_popstate({state_lit}, '{escaped}')")
}

#[cfg(feature = "v8")]
impl PersistentJs for V8PersistentJs {
    fn eval_js(&self, script: &str) {
        use lumen_core::ext::JsRuntime as _;
        if let Err(e) = self.rt.eval(script)
            && !matches!(e, lumen_core::JsError::NotImplemented)
        {
            eprintln!("JS event error: {e}");
        }
    }
    fn eval_js_value(&self, script: &str) -> Result<String, String> {
        use lumen_core::ext::JsRuntime as _;
        self.rt
            .eval(script)
            .map(|v| v.to_json_string())
            .map_err(|e| e.to_string())
    }
    fn take_navigate_request(&self) -> Option<JsNavigateRequest> {
        self.rt.take_navigate_request().map(|r| match r {
            lumen_js::NavigateRequest::Push(u)    => JsNavigateRequest::Push(u),
            lumen_js::NavigateRequest::Replace(u) => JsNavigateRequest::Replace(u),
            lumen_js::NavigateRequest::Reload     => JsNavigateRequest::Reload,
            lumen_js::NavigateRequest::SubmitForm { form, submitter } =>
                JsNavigateRequest::SubmitForm { form, submitter },
        })
    }
    fn take_nav_intercept_result(&self) -> Vec<(bool, bool)> {
        self.rt.take_nav_intercept_result()
    }
    fn take_nav_updates(&self) -> Vec<(u8, String, String, String)> {
        self.rt
            .take_nav_updates()
            .into_iter()
            .map(|(a, url, key, data)| (a as u8, url, key, data))
            .collect()
    }
    fn fire_navigate_success(&self) {
        self.eval_js("if(typeof _lumen_fire_navigate_success==='function')_lumen_fire_navigate_success();");
    }
    fn fire_navigate_error(&self) {
        self.eval_js("if(typeof _lumen_fire_navigate_error==='function')_lumen_fire_navigate_error();");
    }
    fn fire_current_entry_change(&self) {
        self.eval_js("if(typeof _lumen_fire_currententrychange==='function')_lumen_fire_currententrychange();");
    }
    fn tick_timers(&self) {
        self.eval_js("_lumen_tick_timers()");
    }
    fn take_timer_wakeup(&self) -> Option<f64> {
        self.rt.take_timer_wakeup()
    }
    fn frame_transport_pending(&self) -> bool {
        self.rt.frame_transport_pending()
    }
    fn take_dom_dirty(&self) -> bool {
        self.rt.take_dom_dirty()
    }
    fn take_frame_dom_dirty(&self) -> bool {
        self.rt.take_frame_dom_dirty()
    }
    fn take_dom_touched(&self) -> DomTouchedSummary {
        let t = self.rt.take_dom_touched();
        DomTouchedSummary { nodes: t.nodes, unattributed: t.unattributed }
    }
    fn debug_js_heap(&self) -> (i64, i64) {
        self.rt.debug_heap_stats()
    }
    fn run_animation_frame(&self, timestamp_ms: f64) {
        self.eval_js(&format!("_lumen_run_raf_callbacks({timestamp_ms})"));
    }
    fn take_raf_pending(&self) -> bool {
        self.rt.take_raf_pending()
    }
    fn has_raf_pending(&self) -> bool {
        self.rt.has_raf_pending()
    }
    fn raf_pending_flag(&self) -> Option<Arc<std::sync::atomic::AtomicBool>> {
        Some(self.rt.raf_pending_flag())
    }
    fn dom_dirty_flag(&self) -> Option<Arc<std::sync::atomic::AtomicBool>> {
        Some(self.rt.dom_dirty_flag())
    }
    fn update_layout_rects(&self, rects: HashMap<u32, [f32; 4]>) {
        self.rt.update_layout_rects(rects);
    }
    fn update_hit_test_tree(&self, tree: Arc<lumen_layout::LayoutBox>) {
        self.rt.update_hit_test_tree(tree);
    }
    fn update_viewport_size(&self, width: f32, height: f32) {
        self.rt.update_viewport_size(width, height);
    }
    fn deliver_layout_observers(&self) {
        self.eval_js("_lumen_deliver_resize_observers();_lumen_deliver_intersection_observers();_lumen_deliver_canvas_css_resize();");
    }
    fn register_lazy_images(&self, pairs: &[(u32, &str)]) {
        if pairs.is_empty() {
            return;
        }
        let args = pairs
            .iter()
            .map(|(nid, url)| format!("[{nid},{}]", js_string_literal(url)))
            .collect::<Vec<_>>()
            .join(",");
        self.eval_js(&format!("_lumen_init_lazy_images([{args}]);"));
    }
    fn deliver_lazy_images(&self) {
        self.eval_js("_lumen_deliver_lazy_images();");
    }
    // BUG-447: this override was missing, so on the default V8 build the call fell
    // through to the trait's no-op default and the `img_bitmap_store` stayed empty
    // for the whole session вЂ” `drawImage(imgElement, вЂ¦)` silently painted nothing.
    fn register_img_bitmaps(&self, bitmaps: Vec<(u32, Arc<lumen_image::Image>)>) {
        self.rt.register_img_bitmaps(bitmaps);
    }
    fn take_lazy_image_requests(&self) -> Vec<(u32, String)> {
        self.rt.take_lazy_image_requests()
    }
    fn register_iframe_document(
        &self,
        host_nid: u32,
        doc: Arc<Mutex<Document>>,
        url: &str,
        name: Option<&str>,
        accessible: bool,
    ) {
        self.rt.register_frame_document(
            host_nid,
            doc,
            url.to_owned(),
            name.map(str::to_owned),
            accessible,
        );
    }
    fn register_parent_document(
        &self,
        host_nid: u32,
        doc: Arc<Mutex<Document>>,
        url: &str,
        accessible: bool,
    ) {
        self.rt
            .register_parent_document(host_nid, doc, url.to_owned(), accessible);
    }
    fn register_top_document(&self, doc: Arc<Mutex<Document>>, url: &str, accessible: bool) {
        self.rt.register_top_document(doc, url.to_owned(), accessible);
    }
    fn deliver_paint_timing(&self, name: &str, start_ms: f64) {
        self.eval_js(&format!(
            "_lumen_deliver_paint_entry({}, {start_ms})",
            js_string_literal(name),
        ));
    }
    fn deliver_nav_timing(&self, url: &str, duration_ms: f64) {
        self.eval_js(&format!(
            "_lumen_deliver_perf_entry('navigation', {}, 0.0, {duration_ms}, null)",
            js_string_literal(url),
        ));
    }
    fn deliver_resource_timings(&self, rows_json: &str) {
        // The payload crosses the boundary as a JS *string literal* holding
        // JSON text, because the shim runs `JSON.parse` on it. Embedding the
        // JSON bare on the "valid JSON is a valid JS expression" reasoning is
        // what made every `popstate` deliver `state: null` for months
        // (BUG-829) вЂ” the receiver's own reading of the payload decides the
        // encoding, not the payload's syntax.
        self.eval_js(&format!(
            "_lumen_deliver_resource_timings({})",
            js_string_literal(rows_json),
        ));
    }
    fn deliver_lcp_entry(&self, element_id: u32, size: u32, start_ms: f64, render_time_ms: f64) {
        self.eval_js(&format!(
            "_lumen_deliver_lcp_entry({element_id}, {size}, {start_ms}, {render_time_ms})"
        ));
    }
    fn deliver_layout_shift(&self, value: f64, had_input: bool) {
        let had_input_js = if had_input { "true" } else { "false" };
        self.eval_js(&format!(
            "_lumen_deliver_layout_shift({}, 0, {had_input_js})",
            value
        ));
    }
    fn update_computed_styles(&self, styles: HashMap<u32, HashMap<String, String>>) {
        self.rt.update_computed_styles(styles);
    }
    fn update_custom_properties(&self, props: HashMap<u32, Arc<HashMap<String, String>>>) {
        self.rt.update_custom_properties(props);
    }
    fn notify_dom_content_loaded(&self) {
        self.eval_js("_lumen_apply_ready_state('interactive')");
    }
    fn notify_window_loaded(&self) {
        self.eval_js("_lumen_apply_ready_state('complete')");
    }
    fn deliver_media_query_changes(&self, width: f32, height: f32, prefers_dark: bool, reduced_motion: bool) {
        let dark = if prefers_dark { "true" } else { "false" };
        let rm = if reduced_motion { "true" } else { "false" };
        self.eval_js(&format!(
            "if(typeof _lumen_deliver_media_changes==='function')_lumen_deliver_media_changes({width},{height},{dark},{rm});"
        ));
    }
    fn pump_websockets(&self) {
        self.eval_js("if(typeof _lumen_pump_websockets==='function')_lumen_pump_websockets();");
    }
    fn pump_sse(&self) {
        self.eval_js("if(typeof _lumen_pump_sse==='function')_lumen_pump_sse();");
    }
    fn pump_workers(&self) {
        self.rt.pump_workers();
    }
    fn pump_shared_workers(&self) {
        self.rt.pump_shared_workers();
    }
    fn pump_broadcast_channels(&self) {
        self.rt.pump_broadcast_channels();
    }
    fn pump_frame_messages(&self) {
        self.eval_js("if(typeof _lumen_frame_pump_messages==='function')_lumen_frame_pump_messages();");
    }
    fn take_notification_requests(&self) -> Vec<(String, String)> {
        self.rt
            .take_notification_requests()
            .into_iter()
            .map(|r| (r.title, r.body))
            .collect()
    }
    fn gc_collect(&self, dead_nids: &[u32]) {
        if dead_nids.is_empty() {
            return;
        }
        let arr = dead_nids
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.eval_js(&format!(
            "if(typeof _lumen_gc_collect==='function')_lumen_gc_collect([{arr}]);"
        ));
    }
    fn take_window_open_requests(&self) -> Vec<(String, String, u32, u32)> {
        self.rt
            .take_window_open_requests()
            .into_iter()
            .map(|r| (r.url, r.target, r.width, r.height))
            .collect()
    }
    fn take_console_messages(&self) -> Vec<(u8, String)> {
        self.rt.take_console_messages()
    }
    fn update_scroll_states(&self, states: HashMap<u32, [f32; 4]>) {
        self.rt.update_scroll_states(states);
    }
    fn take_scroll_requests(&self) -> Vec<(u32, f32, f32)> {
        self.rt.take_scroll_requests()
    }
    fn take_history_url_updates(&self) -> Vec<(bool, String, String)> {
        self.rt
            .take_history_url_updates()
            .into_iter()
            .map(|u| match u {
                lumen_js::HistoryUrlUpdate::Push { url, new_state_json } => {
                    (true, url, new_state_json)
                }
                lumen_js::HistoryUrlUpdate::Replace { url, new_state_json } => {
                    (false, url, new_state_json)
                }
            })
            .collect()
    }
    fn take_history_traversals(&self) -> Vec<i32> {
        self.rt.take_history_traversals()
    }
    fn fire_popstate(&self, state_json: &str, url: &str) {
        // BUG-829: `state_json` is JSON *text* and `_lumen_deliver_popstate`
        // parses it as such, so it has to reach the shim as a JS string. This
        // used to embed it bare вЂ” on the reasoning that valid JSON is also a
        // valid JS expression вЂ” which handed the shim an object literal
        // instead, whose `JSON.parse` then threw, so every traversal restored
        // `state: null`. Nobody noticed because the one value that survives
        // that round trip unchanged is `null` itself.
        self.eval_js(&popstate_eval_source(state_json, url));
    }
    fn flush_canvas_updates(&self) -> Vec<(u32, u32, u32, Vec<u8>)> {
        self.rt.flush_canvas_updates()
    }
    fn take_fullscreen_requests(&self) -> Vec<(bool, u32)> {
        self.rt
            .take_fullscreen_requests()
            .into_iter()
            .map(|r| match r {
                lumen_js::FullscreenRequest::Enter { nid } => (true, nid),
                lumen_js::FullscreenRequest::Exit => (false, 0),
            })
            .collect()
    }
    fn take_view_transition_events(&self) -> Vec<ViewTransitionEvent> {
        self.rt
            .take_view_transition_events()
            .into_iter()
            .map(|ev| match ev {
                lumen_js::ViewTransitionEvent::Begin => ViewTransitionEvent::Begin,
                lumen_js::ViewTransitionEvent::End => ViewTransitionEvent::End,
                lumen_js::ViewTransitionEvent::Cancel => ViewTransitionEvent::Cancel,
            })
            .collect()
    }
    fn take_print_requests(&self) -> Vec<lumen_js::PrintRequest> {
        self.rt.take_print_requests()
    }
    fn take_page_scroll_requests(&self) -> Vec<(f32, bool)> {
        self.rt.take_page_scroll_requests()
    }
    fn set_page_scroll_y(&self, y: f32) -> bool {
        self.rt.set_page_scroll_y(y)
    }
    fn run_gc_pass(&self, _level: u8) {
        // V8 manages its own generational GC; no manual tuning hook is wired yet.
    }
    fn deliver_scroll_progress(&self, progress_y: f32, progress_x: f32) {
        self.eval_js(&format!(
            "if(typeof _lumen_deliver_scroll_progress==='function')_lumen_deliver_scroll_progress({progress_y},{progress_x});"
        ));
    }
    fn fire_element_scroll(&self, nid: u32) {
        self.eval_js(&format!(
            "if(typeof _lumen_fire_scroll_on_element==='function')_lumen_fire_scroll_on_element({nid});"
        ));
    }
    fn fire_window_scroll(&self) {
        self.eval_js(
            "if(typeof _lumen_fire_window_scroll_event==='function')_lumen_fire_window_scroll_event();"
        );
    }
    fn fire_element_scrollend(&self, nid: u32) {
        self.eval_js(&format!(
            "if(typeof _lumen_fire_scrollend_on_element==='function')_lumen_fire_scrollend_on_element({nid});"
        ));
    }
    fn page_scrollend_due(&self, moved: bool, settled: bool) -> bool {
        self.rt.page_scrollend_due(moved, settled)
    }
    fn fire_window_scrollend(&self) {
        self.eval_js(
            "if(typeof _lumen_fire_window_scrollend_event==='function')_lumen_fire_window_scrollend_event();"
        );
    }
    fn fire_window_resize(&self) {
        self.eval_js(
            "if(typeof _lumen_fire_window_resize_event==='function')_lumen_fire_window_resize_event();"
        );
    }
    fn deliver_cv_state_changes(&self, payload: &str) {
        self.eval_js(&format!(
            "if(typeof _lumen_deliver_cv_state_changes==='function')\
             _lumen_deliver_cv_state_changes({payload});"
        ));
    }
    fn pause_event_loop(&self) {
        self.eval_js("_lumen_apply_visibility(true)");
    }
    fn unpause_event_loop(&self) {
        self.eval_js("_lumen_apply_visibility(false)");
    }
    fn take_focus_requests(&self) -> Vec<Option<u32>> {
        self.rt.take_focus_requests()
    }
    fn fire_dialog_close(&self, dialog_nid: u32, return_value: &str) {
        let rv = return_value.replace('\\', r"\\").replace('"', r#"\""#);
        self.eval_js(&format!(
            "(function(){{var d=_lumen_make_element({dialog_nid});\
             if(d&&typeof d.close==='function')d.close(\"{rv}\");}})();"
        ));
    }
    fn notify_focus_changed(&self, nid: Option<u32>) {
        let n = nid.map(|n| n as i64).unwrap_or(-1_i64);
        self.eval_js(&format!(
            "if(typeof _lumen_focus_update==='function')_lumen_focus_update({n});\
             else if(typeof _lumen_last_focused_nid!=='undefined')_lumen_last_focused_nid={n};"
        ));
    }
    fn pointer_capture_nid(&self) -> Option<u32> {
        self.rt.pointer_capture_nid()
    }
    fn has_bfcache_freeze_blocker(&self) -> bool {
        matches!(self.eval_js_value("_lumen_bfcache_blocked()"), Ok(ref v) if v == "true")
    }
    fn take_pointer_capture(&self) -> Option<u32> {
        self.rt.take_pointer_capture()
    }
}

/// BUG-341 S7: engine-agnostic mirror of `lumen_js::DomTouched`, kept
/// independent of the `v8` feature so [`PersistentJs::take_dom_touched`]'s
/// default (used by no-engine builds, which have no tracker) compiles
/// unconditionally.
///
/// Consumed by [`Lumen::try_relayout_raf_incremental`] (BUG-341 S7 part 2) to
/// derive the DOM-mutation half of `RestyleDelta::dirty_roots` for the
/// incremental-cascade path (`layout_mutation_incremental_restyle`).
#[derive(Debug, Default, Clone)]
pub(crate) struct DomTouchedSummary {
    /// Nodes whose selector-relevant state actually changed via a tracked
    /// mutation primitive. See `lumen_js::DomTouched::nodes`.
    pub(crate) nodes: std::collections::HashSet<lumen_dom::NodeId>,
    /// `true` when `nodes` alone is not a safe restyle root-set this cycle вЂ”
    /// the caller must fall back to a full cascade. See
    /// `lumen_js::DomTouched::unattributed`.
    pub(crate) unattributed: bool,
}

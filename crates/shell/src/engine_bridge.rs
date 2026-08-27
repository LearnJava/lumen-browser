//! The shell's side of the engine thread (ADR-016 M2.2): the concrete commit
//! and persistent-state types [`EngineCommit`]/[`EngineJsState`] the generic
//! [`crate::engine_thread`] executor is instantiated with, and the three
//! routers every UI→JS call goes through.
//!
//! Kept apart from `engine_thread.rs` on purpose: that module is generic over
//! the commit type `C` and the state `S` precisely so it does not depend on
//! layout or JS types, and everything here does.
//!
//! Moved out of `main.rs` by the SPLIT track (batch SH-3c); behaviour and
//! signatures are unchanged.

use crate::*;

/// ADR-016 M2.2: СЂРµР·СѓР»СЊС‚Р°С‚ off-thread relayout, РєРѕС‚РѕСЂС‹Р№ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє
/// РєРѕРјРјРёС‚РёС‚ UI-СЃС‚РѕСЂРѕРЅРµ С‡РµСЂРµР· [`engine_thread::EngineThread`]. Р’СЃС‘ СЃРѕРґРµСЂР¶РёРјРѕРµ вЂ”
/// РІР»Р°РґРµСЋС‰РёРµ Р·РЅР°С‡РµРЅРёСЏ (`Send`); РїРѕСЃР»Рµ РєРѕРјРјРёС‚Р° UI Рё РґРІРёР¶РѕРє РЅРµ РґРµР»СЏС‚ РјСѓС‚Р°Р±РµР»СЊРЅРѕРµ
/// СЃРѕСЃС‚РѕСЏРЅРёРµ (РёРЅРІР°СЂРёР°РЅС‚ 1). РџСЂРёРјРµРЅСЏРµС‚СЃСЏ РЅР° UI-РїРѕС‚РѕРєРµ С‡РµСЂРµР·
/// [`Lumen::apply_relayout_result`] вЂ” С‚Р° Р¶Рµ Р»РѕРіРёРєР°, С‡С‚Рѕ Сѓ СЃРёРЅС…СЂРѕРЅРЅРѕРіРѕ
/// `relayout()`, С‚РѕР»СЊРєРѕ layout СѓР¶Рµ РїРѕСЃС‡РёС‚Р°РЅ РІРЅРµ UI-РїРѕС‚РѕРєР°.
pub(crate) struct EngineCommit {
    /// Р“РѕС‚РѕРІС‹Р№ display-list СЃС‚СЂР°РЅРёС†С‹.
    pub(crate) content: DisplayList,
    /// Layout-РґРµСЂРµРІРѕ, РёР· РєРѕС‚РѕСЂРѕРіРѕ РїРѕСЃС‚СЂРѕРµРЅ `content` вЂ” РЅСѓР¶РЅРѕ UI-СЃС‚РѕСЂРѕРЅРµ РґР»СЏ
    /// transitions / JS-observers / scroll-РєРѕРЅС‚РµР№РЅРµСЂРѕРІ.
    pub(crate) layout_box: lumen_layout::LayoutBox,
    /// CSS layout-viewport, РїРѕРґ РєРѕС‚РѕСЂС‹Р№ РїРѕСЃС‚СЂРѕРµРЅ РєРѕРјРјРёС‚.
    pub(crate) viewport: Size,
    /// РќРѕРјРµСЂ relayout-Р·Р°РґР°РЅРёСЏ (generation-guard РІ [`Lumen::poll_engine_commit`]).
    pub(crate) generation: u64,
    /// Р’СЂРµРјСЏ off-thread РІС‹С‡РёСЃР»РµРЅРёСЏ (style + layout + DL build) РІ РјСЃ вЂ” РґР»СЏ
    /// `ENGINE_SUMMARY` / `[engine] relayout вЂ¦ (off-thread)`.
    pub(crate) compute_ms: f32,
}

/// ADR-016 M2.2c-2b: РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРµ СЃРѕСЃС‚РѕСЏРЅРёРµ РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° `S` вЂ” В«РјРµСЃС‚РѕВ»
/// РґР»СЏ РјСѓС‚Р°Р±РµР»СЊРЅРѕРіРѕ `Document` Рё С…СЌРЅРґР»Р° `lumen-js` (`js_ctx`), РєРѕС‚РѕСЂС‹Рµ M2.2c
/// С†РµР»РёРєРѕРј РїРµСЂРµРЅРѕСЃРёС‚ РЅР° РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє. Р’Р»Р°РґРµРµС‚ РёРј **С‚РѕР»СЊРєРѕ** РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє
/// (С‚РёРї `S` РІ [`engine_thread::EngineThread`]); UI-СЃС‚РѕСЂРѕРЅР° РµРіРѕ РЅРµ СЂР°Р·РґРµР»СЏРµС‚ вЂ”
/// РѕР±С‰Р°РµС‚СЃСЏ С‡РµСЂРµР· [`engine_thread::EngineThread::task`]/`query` (РёРЅРІР°СЂРёР°РЅС‚ 1).
///
/// РЎ M2.2c-2d (21) РїРѕР»Рµ `js` вЂ” **РµРґРёРЅСЃС‚РІРµРЅРЅС‹Р№** РІР»Р°РґРµР»РµС† JS-С…СЌРЅРґР»Р° РїРѕРґ С„Р»Р°РіРѕРј:
/// [`Lumen::set_js_ctx`] РїРµСЂРµРЅРѕСЃРёС‚ `Arc` СЃСЋРґР° С‡РµСЂРµР·
/// [`engine_thread::EngineThread::task`], РѕСЃС‚Р°РІР»СЏСЏ UI-СЃС‚РѕСЂРѕРЅРЅРёР№ `Lumen::js_ctx`
/// РїСѓСЃС‚С‹Рј (`None`). Р’СЃРµ UIв†’JS РІС‹Р·РѕРІС‹ РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓСЋС‚СЃСЏ РЅР° РїРѕС‚РѕРє
/// ([`route_task_js`]/[`route_query_js`]/[`route_eval_js`]) Рё С‡РёС‚Р°СЋС‚ `state.js`.
/// `document` РґРµСЂР¶РёС‚СЃСЏ РєР°Рє Р±СѓРґСѓС‰РµРµ В«СЃРёРґРµРЅСЊРµВ» РґР»СЏ РІР»Р°РґРµРЅРёСЏ DOM; РїРѕРєР° С‡РёС‚Р°РµС‚СЃСЏ
/// С‚РѕР»СЊРєРѕ РєР°Рє Р·РµСЂРєР°Р»СЊРЅС‹Р№ СЃРЅРёРјРѕРє Рё РІ layout-РІС‹С‡РёСЃР»РµРЅРёРё РЅРµ СѓС‡Р°СЃС‚РІСѓРµС‚.
#[derive(Default)]
pub(crate) struct EngineJsState {
    /// Р Р°Р·РґРµР»СЏРµРјС‹Р№ DOM (С‚РѕС‚ Р¶Рµ `Arc`, С‡С‚Рѕ Сѓ `LayoutSource::document`). Р‘СѓРґСѓС‰РµРµ
    /// В«СЃРёРґРµРЅСЊРµВ» РІР»Р°РґРµРЅРёСЏ DOM РґРІРёР¶РєРѕРІС‹Рј РїРѕС‚РѕРєРѕРј (M2.2c-3); РїРѕРєР° вЂ” Р·РµСЂРєР°Р»СЊРЅС‹Р№
    /// СЃРЅРёРјРѕРє РґР»СЏ РіРѕС‚РѕРІРЅРѕСЃС‚Рё РёРЅС„СЂР°СЃС‚СЂСѓРєС‚СѓСЂС‹, layout РµРіРѕ РЅРµ РёСЃРїРѕР»СЊР·СѓРµС‚.
    pub(crate) document: Option<Arc<Mutex<Document>>>,
    /// РҐСЌРЅРґР» РїРµСЂСЃРёСЃС‚РµРЅС‚РЅРѕРіРѕ JS-СЂР°РЅС‚Р°Р№РјР°. РЎ M2.2c-2d (21) РїРѕРґ С„Р»Р°РіРѕРј СЌС‚Рѕ
    /// **РµРґРёРЅСЃС‚РІРµРЅРЅС‹Р№** СЌРєР·РµРјРїР»СЏСЂ `Arc`-С…СЌРЅРґР»Р° (UI-СЃС‚РѕСЂРѕРЅРЅРёР№ `Lumen::js_ctx` вЂ”
    /// `None`); [`Lumen::set_js_ctx`] РєР»Р°РґС‘С‚ РµРіРѕ СЃСЋРґР°, Р° `save_page_snapshot`
    /// РІС‹РЅРёРјР°РµС‚ С‡РµСЂРµР· [`Lumen::take_js_ctx`]. Р‘РµР· С„Р»Р°РіР° РїРѕР»Рµ РЅРµ РёСЃРїРѕР»СЊР·СѓРµС‚СЃСЏ.
    pub(crate) js: Option<Arc<dyn PersistentJs>>,
}

/// ADR-016 M2.2c-2b: РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ fire-and-forget void-РІС‹Р·РѕРІР° `eval_js`.
///
/// РЎРІРѕР±РѕРґРЅР°СЏ С„СѓРЅРєС†РёСЏ (Р° РЅРµ РјРµС‚РѕРґ `Lumen`), С‡С‚РѕР±С‹ РЅР° call-site'Рµ Р·Р°РёРјСЃС‚РІРѕРІР°С‚СЊ РїРѕР»СЏ
/// `engine_thread`/`js_ctx` **СЂР°Р·РґРµР»СЊРЅРѕ** (disjoint borrow) Рё РЅРµ РєРѕРЅС„Р»РёРєС‚РѕРІР°С‚СЊ СЃ
/// СѓР¶Рµ СѓРґРµСЂР¶РёРІР°РµРјС‹РјРё `&mut`-Р·Р°РёРјСЃС‚РІРѕРІР°РЅРёСЏРјРё РґСЂСѓРіРёС… РїРѕР»РµР№ `self`.
///
/// - РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє РµСЃС‚СЊ (`LUMEN_ENGINE_THREAD=1`) в†’ `eval_js` СѓС…РѕРґРёС‚ **off-UI-thread**
///   С‡РµСЂРµР· [`engine_thread::EngineThread::task`] (РЅРµ Р±Р»РѕРєРёСЂСѓРµС‚ UI-РїРѕС‚РѕРє РЅР°
///   JS-round-trip; РёСЃРїРѕР»РЅРёС‚СЃСЏ РїРѕ РїРѕСЂСЏРґРєСѓ СЃСЂРµРґРё РїСЂРѕС‡РёС… `Task`);
/// - РїРѕС‚РѕРєР° РЅРµС‚ (С„Р»Р°Рі РІС‹РєР»СЋС‡РµРЅ, РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) в†’ СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ
///   `js` вЂ” **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ** РїСЂРµР¶РЅРµРјСѓ `js.eval_js(script)`.
///
/// РР·РІРµСЃС‚РЅРѕРµ РѕРіСЂР°РЅРёС‡РµРЅРёРµ РїРѕРґ С„Р»Р°РіРѕРј (РїР°С‚С‚РµСЂРЅ M2.2a): РІС‹Р·РѕРІ СЃС‚Р°РЅРѕРІРёС‚СЃСЏ
/// Р°СЃРёРЅС…СЂРѕРЅРЅС‹Рј, РїРѕСЌС‚РѕРјСѓ СЃРёРЅС…СЂРѕРЅРЅРѕРµ С‡С‚РµРЅРёРµ СЂРµР·СѓР»СЊС‚Р°С‚РѕРІ JS РІ С‚РѕРј Р¶Рµ С‚РёРєРµ (РЅР°РїСЂ.
/// `take_navigate_request`) РјРѕР¶РµС‚ СѓРІРёРґРµС‚СЊ РёС… РЅР° РєР°РґСЂ РїРѕР·Р¶Рµ вЂ” С‚Р°РєРёРµ
/// read-after-eval-С†РµРїРѕС‡РєРё РїРµСЂРµРЅРѕСЃСЏС‚СЃСЏ РЅР° `query`-РїСѓС‚СЊ РІ M2.2c-2c. РџРѕСЌС‚РѕРјСѓ
/// РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓСЋС‚СЃСЏ С‚РѕР»СЊРєРѕ Р·Р°РІРµРґРѕРјРѕ РёР·РѕР»РёСЂРѕРІР°РЅРЅС‹Рµ void-РІС‹Р·РѕРІС‹ Р±РµР· С‡С‚РµРЅРёСЏ СЃР»РµРґРѕРј.
pub(crate) fn route_eval_js(
    engine: Option<&engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    js: Option<&Arc<dyn PersistentJs>>,
    script: String,
) {
    route_task_js(engine, js, move |js| js.eval_js(&script));
}

/// ADR-016 M2.2c-2d: РѕР±РѕР±С‰С‘РЅРЅС‹Р№ РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ fire-and-forget void-*РґРµР№СЃС‚РІРёСЏ*
/// РЅР°Рґ JS-С…СЌРЅРґР»РѕРј.
///
/// РћР±РѕР±С‰Р°РµС‚ [`route_eval_js`] (С‡Р°СЃС‚РЅС‹Р№ СЃР»СѓС‡Р°Р№ `|js| js.eval_js(&script)`) РЅР°
/// Р»СЋР±РѕРµ void-РґРµР№СЃС‚РІРёРµ РЅР°Рґ `&Arc<dyn PersistentJs>` вЂ” РЅСѓР¶РЅРѕ РґР»СЏ Р±Р°С‚С‡Р°
/// per-tick pump-РІС‹Р·РѕРІРѕРІ (`tick_timers`/`pump_websockets`/`pump_sse`/
/// `pump_workers`/`pump_broadcast_channels`/`pump_shared_workers`), РєРѕС‚РѕСЂС‹Рµ РЅРµ
/// СЃРІРѕРґСЏС‚СЃСЏ Рє РѕРґРЅРѕРјСѓ `eval_js` (С‡Р°СЃС‚СЊ вЂ” РїСЂСЏРјС‹Рµ РјРµС‚РѕРґС‹ СЂР°РЅС‚Р°Р№РјР°). РЎРІРѕР±РѕРґРЅР°СЏ
/// С„СѓРЅРєС†РёСЏ (Р° РЅРµ РјРµС‚РѕРґ `Lumen`), С‡С‚РѕР±С‹ РЅР° call-site'Рµ Р·Р°РёРјСЃС‚РІРѕРІР°С‚СЊ РїРѕР»СЏ
/// `engine_thread`/`js_ctx` **СЂР°Р·РґРµР»СЊРЅРѕ** (disjoint borrow).
///
/// - РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє РµСЃС‚СЊ (`LUMEN_ENGINE_THREAD=1`) в†’ РґРµР№СЃС‚РІРёРµ СѓС…РѕРґРёС‚
///   **off-UI-thread** С‡РµСЂРµР· [`engine_thread::EngineThread::task`] (РЅРµ Р±Р»РѕРєРёСЂСѓРµС‚
///   UI-РїРѕС‚РѕРє; РёСЃРїРѕР»РЅРёС‚СЃСЏ РїРѕ РїРѕСЂСЏРґРєСѓ СЃСЂРµРґРё РїСЂРѕС‡РёС… `Task` вЂ” С‚Р°Рє С‡С‚Рѕ РїРѕСЃР»РµРґСѓСЋС‰РёР№
///   `query` РІСЃС‚Р°С‘С‚ РІ РѕС‡РµСЂРµРґСЊ **РїРѕСЃР»Рµ** РЅРµРіРѕ, СЃРѕС…СЂР°РЅСЏСЏ read-after-write РїРѕСЂСЏРґРѕРє);
/// - РїРѕС‚РѕРєР° РЅРµС‚ (С„Р»Р°Рі РІС‹РєР»СЋС‡РµРЅ, РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) в†’ СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ
///   `js` вЂ” **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ** РїСЂРµР¶РЅРёРј РїСЂСЏРјС‹Рј `js.<method>()`.
pub(crate) fn route_task_js(
    engine: Option<&engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    js: Option<&Arc<dyn PersistentJs>>,
    action: impl FnOnce(&Arc<dyn PersistentJs>) + Send + 'static,
) {
    match engine {
        Some(engine) => engine.task(move |state| {
            if let Some(js) = &state.js {
                action(js);
            }
        }),
        None => {
            if let Some(js) = js {
                action(js);
            }
        }
    }
}

/// ADR-016 M2.2c-2c: РјР°СЂС€СЂСѓС‚РёР·Р°С‚РѕСЂ **value-returning** UIв†’JS С‡С‚РµРЅРёСЏ.
///
/// Р”РѕРїРѕР»РЅСЏРµС‚ [`route_eval_js`] (fire-and-forget) РґР»СЏ РІС‹Р·РѕРІРѕРІ, С‡РµР№ СЂРµР·СѓР»СЊС‚Р°С‚ РЅСѓР¶РµРЅ
/// UI-СЃС‚РѕСЂРѕРЅРµ **СЃРµР№С‡Р°СЃ**: `take_dom_dirty` в†’ `bool`, `take_raf_pending` в†’ `bool`,
/// `eval_js_value` в†’ `Result<String, String>`, Р° С‚Р°РєР¶Рµ (РѕСЃС‚Р°С‚РѕРє 2c) nav/timer-С‡С‚РµРЅРёСЏ
/// `take_navigate_request` в†’ `Option<JsNavigateRequest>`, `take_timer_wakeup` в†’
/// `Option<f64>` Рё nav-update drain `take_nav_updates` в†’ `Vec<_>`. РљР°Рє Рё `route_eval_js`
/// вЂ” СЃРІРѕР±РѕРґРЅР°СЏ С„СѓРЅРєС†РёСЏ СЂР°РґРё disjoint-borrow РїРѕР»РµР№ `engine_thread`/`js_ctx`.
///
/// - РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє РµСЃС‚СЊ (`LUMEN_ENGINE_THREAD=1`) в†’ С‡С‚РµРЅРёРµ РёРґС‘С‚ С‡РµСЂРµР·
///   [`engine_thread::EngineThread::query`] (Р±Р»РѕРєРёСЂСѓСЋС‰РёР№ request/reply). Р­С‚Рѕ
///   СЃС‚Р°РІРёС‚ С‡С‚РµРЅРёРµ **РІ РѕС‡РµСЂРµРґСЊ РїРѕСЃР»Рµ** Р»СЋР±РѕРіРѕ СѓР¶Рµ РѕС‚РїСЂР°РІР»РµРЅРЅРѕРіРѕ `task` (РЅР°РїСЂ.
///   РјР°СЂС€СЂСѓС‚РёР·РёСЂРѕРІР°РЅРЅРѕРіРѕ `eval_js`) вЂ” С‚РµРј СЃР°РјС‹Рј РІРѕСЃСЃС‚Р°РЅР°РІР»РёРІР°СЏ read-after-eval
///   РїРѕСЂСЏРґРѕРє, РЅР°РјРµСЂРµРЅРЅРѕ РѕСЃС‚Р°РІР»РµРЅРЅС‹Р№ СЃРёРЅС…СЂРѕРЅРЅС‹Рј РІ M2.2c-2b;
/// - РїРѕС‚РѕРєР° РЅРµС‚ (С„Р»Р°Рі РІС‹РєР»СЋС‡РµРЅ, РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ) в†’ СЃРёРЅС…СЂРѕРЅРЅС‹Р№ РІС‹Р·РѕРІ РїРѕ UI-С…СЌРЅРґР»Сѓ вЂ”
///   **Р±Р°Р№С‚-РёРґРµРЅС‚РёС‡РЅРѕ** РїСЂРµР¶РЅРµРјСѓ РїСЂСЏРјРѕРјСѓ `js.<read>()`.
///
/// Р’РѕР·РІСЂР°С‰Р°РµС‚ `None`, РєРѕРіРґР° JS-РєРѕРЅС‚РµРєСЃС‚Р° РЅРµС‚ РІРѕРІСЃРµ (РЅРµС‚ UI-С…СЌРЅРґР»Р° / СЃРѕСЃС‚РѕСЏРЅРёРµ
/// РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° РµС‰С‘ РЅРµ Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅРѕ / РїРѕС‚РѕРє Р·Р°РІРµСЂС€С‘РЅ РїСЂРё shutdown); РІ СЌС‚РѕРј
/// СЃР»СѓС‡Р°Рµ РІС‹Р·С‹РІР°СЋС‰Р°СЏ СЃС‚РѕСЂРѕРЅР° РїРѕРґСЃС‚Р°РІР»СЏРµС‚ Р·РЅР°С‡РµРЅРёРµ-РїРѕ-СѓРјРѕР»С‡Р°РЅРёСЋ СЃРІРѕРµР№ РІРµС‚РєРё В«Р±РµР·
/// JSВ» (РЅР°РїСЂ. `unwrap_or(false)` РґР»СЏ `take_dom_dirty`) вЂ” РєР°Рє Рё Р±РµР· С„Р»Р°РіР°, РіРґРµ
/// `js_ctx == None` РґР°С‘С‚ С‚Сѓ Р¶Рµ РІРµС‚РєСѓ.
pub(crate) fn route_query_js<R: Send + 'static>(
    engine: Option<&engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    js: Option<&Arc<dyn PersistentJs>>,
    read: impl FnOnce(&Arc<dyn PersistentJs>) -> R + Send + 'static,
) -> Option<R> {
    match engine {
        // `query` РІРµСЂРЅС‘С‚ `Some(inner)`, РіРґРµ `inner` вЂ” СЂРµР·СѓР»СЊС‚Р°С‚ `read`, Р»РёР±Рѕ `None`
        // РµСЃР»Рё С…СЌРЅРґР» РµС‰С‘ РЅРµ Р·РµСЂРєР°Р»РёСЂРѕРІР°РЅ РІ СЃРѕСЃС‚РѕСЏРЅРёРµ; РґРІРѕР№РЅРѕР№ `Option` СЃС…Р»РѕРїС‹РІР°РµРј
        // `flatten`. `query` С†РµР»РёРєРѕРј РІРµСЂРЅС‘С‚ `None` РїСЂРё Р·Р°РІРµСЂС€С‘РЅРЅРѕРј РїРѕС‚РѕРєРµ в†’ С‚РѕР¶Рµ `None`.
        Some(engine) => engine
            .query(move |state| state.js.as_ref().map(read))
            .flatten(),
        None => js.map(read),
    }
}

/// Whether the ADR-016 engine thread should be spawned.
///
/// **ADR-023 (default flip 2026-07-28): now enabled by default.** ADR-016's
/// M0вЂ“M4.1 stages all landed behind `LUMEN_ENGINE_THREAD=1` and each one was
/// accepted as byte-identical with the flag off, so the flag had become a
/// finished-but-unused feature. Leaving it off kept every `relayout()` on the
/// UI thread, which is what makes real sites hang on load: a page with N
/// `@font-face` files pays N serialized full relayouts before its first frame
/// (measured on lenta.ru вЂ” 9 fonts, ~300вЂ“700 ms each, first frame ~6.7 s в†’ ~3.6 s
/// with the thread on; see `bugs/BUG-274-OPEN.md`, СЃСЂРµР· 2026-07-28).
///
/// Rollback (same flag-strategy idiom as ADR-018's V8 cutover and ADR-021's
/// chrome flip): `LUMEN_NO_ENGINE_THREAD=1` вЂ” or `LUMEN_ENGINE_THREAD=0` for
/// callers already setting the historical variable вЂ” restores the fully
/// synchronous UI-thread behaviour.
///
/// Deliberately **not** tied to `--deterministic`: `graphic_tests/run.py`
/// launches with `--deterministic --viewport 1024x720`, so forcing the thread
/// off there would mean the pixel gate never exercises the shipped default.
pub(crate) fn engine_thread_enabled() -> bool {
    let opt_out = std::env::var("LUMEN_NO_ENGINE_THREAD").ok();
    let legacy = std::env::var("LUMEN_ENGINE_THREAD").ok();
    engine_thread_enabled_from(opt_out.as_deref(), legacy.as_deref())
}

/// Pure decision behind [`engine_thread_enabled`], split out so the precedence
/// rules are unit-testable: reading the real environment from a test is
/// process-global and races the rest of the (parallel) test binary.
///
/// `opt_out` is `LUMEN_NO_ENGINE_THREAD`, `legacy` is the historical
/// `LUMEN_ENGINE_THREAD`. The opt-out wins over everything; otherwise only an
/// explicit `LUMEN_ENGINE_THREAD=0` disables the thread. A leftover
/// `LUMEN_ENGINE_THREAD=1` from before the ADR-023 flip keeps working and now
/// simply agrees with the default.
pub(crate) fn engine_thread_enabled_from(opt_out: Option<&str>, legacy: Option<&str>) -> bool {
    if opt_out == Some("1") {
        return false;
    }
    legacy != Some("0")
}

/// ADR-016 M2.2: РїРѕРґРЅРёРјР°РµС‚ РґРІРёР¶РєРѕРІС‹Р№ РїРѕС‚РѕРє, РµСЃР»Рё РѕРЅ РЅРµ РѕС‚РєР»СЋС‡С‘РЅ СЏРІРЅРѕ
/// ([`engine_thread_enabled`] вЂ” СЃ ADR-023 РІРєР»СЋС‡С‘РЅ РїРѕ СѓРјРѕР»С‡Р°РЅРёСЋ). Р’
/// M2.2 С‡РµСЂРµР· РїРѕС‚РѕРє РјР°СЂС€СЂСѓС‚РёР·РёСЂСѓРµС‚СЃСЏ off-thread layout РґР»СЏ async-С‚СЂРёРіРіРµСЂРѕРІ
/// (РїРѕРєР° вЂ” debounce-Р·СѓРј): [`Lumen::submit_relayout_job`] С€Р»С‘С‚ Р·Р°РґР°РЅРёРµ, РїРѕС‚РѕРє
/// СЃС‡РёС‚Р°РµС‚ [`EngineCommit`] Рё РєР»Р°РґС‘С‚ РІ latest-wins СЃР»РѕС‚, РѕС‚РєСѓРґР° РµРіРѕ Р·Р°Р±РёСЂР°РµС‚
/// [`Lumen::poll_engine_commit`]. РџСЂРё СЃР±РѕРµ СЃС‚Р°СЂС‚Р° РїРѕС‚РѕРєР° Р»РѕРіРёСЂСѓРµРј Рё РѕС‚РєР°С‚С‹РІР°РµРјСЃСЏ
/// РЅР° `None` (РєР°Рє РѕР±С‹С‡РЅРѕ, Р±РµР· РґРІРёР¶РєРѕРІРѕРіРѕ РїРѕС‚РѕРєР° вЂ” СЃРёРЅС…СЂРѕРЅРЅС‹Р№ `relayout()`).
pub(crate) fn spawn_engine_thread_if_enabled()
-> Option<engine_thread::EngineThread<EngineCommit, EngineJsState>> {
    if !engine_thread_enabled() {
        return None;
    }
    // ADR-016 M2.2c-2b: РїРѕС‚РѕРє РІР»Р°РґРµРµС‚ `EngineJsState` (Р±СѓРґСѓС‰РµРµ СЃРёРґРµРЅСЊРµ `Document`
    // + `js_ctx`); СЃС‚Р°СЂС‚СѓРµС‚ РїСѓСЃС‚С‹Рј (`EngineJsState::default()` С‡РµСЂРµР· `spawn()`),
    // Р·Р°РїРѕР»РЅСЏРµС‚СЃСЏ `sync_engine_js_state` РїСЂРё РїРµСЂРІРѕР№ Р·Р°РіСЂСѓР·РєРµ СЃС‚СЂР°РЅРёС†С‹.
    match engine_thread::EngineThread::<EngineCommit, EngineJsState>::spawn() {
        Ok(engine) => {
            eprintln!(
                "[engine-thread] Р·Р°РїСѓС‰РµРЅ (ADR-023 РґРµС„РѕР»С‚, M2.2 off-thread layout; \
                 РѕС‚РєР°С‚ вЂ” LUMEN_NO_ENGINE_THREAD=1)"
            );
            Some(engine)
        }
        Err(e) => {
            eprintln!("[engine-thread] РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїСѓСЃС‚РёС‚СЊ: {e}; РїСЂРѕРґРѕР»Р¶Р°РµРј Р±РµР· РЅРµРіРѕ");
            None
        }
    }
}

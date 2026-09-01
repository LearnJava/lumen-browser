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

/// ADR-016 M2.2: результат off-thread relayout, который движковый поток
/// коммитит UI-стороне через [`engine_thread::EngineThread`]. Всё содержимое —
/// владеющие значения (`Send`); после коммита UI и движок не делят мутабельное
/// состояние (инвариант 1). Применяется на UI-потоке через
/// [`Lumen::apply_relayout_result`] — та же логика, что у синхронного
/// `relayout()`, только layout уже посчитан вне UI-потока.
pub(crate) struct EngineCommit {
    /// Готовый display-list страницы.
    pub(crate) content: DisplayList,
    /// Layout-дерево, из которого построен `content` — нужно UI-стороне для
    /// transitions / JS-observers / scroll-контейнеров.
    pub(crate) layout_box: lumen_layout::LayoutBox,
    /// CSS layout-viewport, под который построен коммит.
    pub(crate) viewport: Size,
    /// Номер relayout-задания (generation-guard в [`Lumen::poll_engine_commit`]).
    pub(crate) generation: u64,
    /// Время off-thread вычисления (style + layout + DL build) в мс — для
    /// `ENGINE_SUMMARY` / `[engine] relayout … (off-thread)`.
    pub(crate) compute_ms: f32,
}

/// ADR-016 M2.2c-2b: персистентное состояние движкового потока `S` — «место»
/// для мутабельного `Document` и хэндла `lumen-js` (`js_ctx`), которые M2.2c
/// целиком переносит на движковый поток. Владеет им **только** движковый поток
/// (тип `S` в [`engine_thread::EngineThread`]); UI-сторона его не разделяет —
/// общается через [`engine_thread::EngineThread::task`]/`query` (инвариант 1).
///
/// С M2.2c-2d (21) поле `js` — **единственный** владелец JS-хэндла под флагом:
/// [`Lumen::set_js_ctx`] переносит `Arc` сюда через
/// [`engine_thread::EngineThread::task`], оставляя UI-сторонний `Lumen::js_ctx`
/// пустым (`None`). Все UI→JS вызовы маршрутизируются на поток
/// ([`route_task_js`]/[`route_query_js`]/[`route_eval_js`]) и читают `state.js`.
/// `document` держится как будущее «сиденье» для владения DOM; пока читается
/// только как зеркальный снимок и в layout-вычислении не участвует.
#[derive(Default)]
pub(crate) struct EngineJsState {
    /// Разделяемый DOM (тот же `Arc`, что у `LayoutSource::document`). Будущее
    /// «сиденье» владения DOM движковым потоком (M2.2c-3); пока — зеркальный
    /// снимок для готовности инфраструктуры, layout его не использует.
    pub(crate) document: Option<Arc<Mutex<Document>>>,
    /// Хэндл персистентного JS-рантайма. С M2.2c-2d (21) под флагом это
    /// **единственный** экземпляр `Arc`-хэндла (UI-сторонний `Lumen::js_ctx` —
    /// `None`); [`Lumen::set_js_ctx`] кладёт его сюда, а `save_page_snapshot`
    /// вынимает через [`Lumen::take_js_ctx`]. Без флага поле не используется.
    pub(crate) js: Option<Arc<dyn PersistentJs>>,
}

/// ADR-016 M2.2c-2b: маршрутизатор fire-and-forget void-вызова `eval_js`.
///
/// Свободная функция (а не метод `Lumen`), чтобы на call-site'е заимствовать поля
/// `engine_thread`/`js_ctx` **раздельно** (disjoint borrow) и не конфликтовать с
/// уже удерживаемыми `&mut`-заимствованиями других полей `self`.
///
/// - движковый поток есть (`LUMEN_ENGINE_THREAD=1`) → `eval_js` уходит **off-UI-thread**
///   через [`engine_thread::EngineThread::task`] (не блокирует UI-поток на
///   JS-round-trip; исполнится по порядку среди прочих `Task`);
/// - потока нет (флаг выключен, по умолчанию) → синхронный вызов по UI-хэндлу
///   `js` — **байт-идентично** прежнему `js.eval_js(script)`.
///
/// Известное ограничение под флагом (паттерн M2.2a): вызов становится
/// асинхронным, поэтому синхронное чтение результатов JS в том же тике (напр.
/// `take_navigate_request`) может увидеть их на кадр позже — такие
/// read-after-eval-цепочки переносятся на `query`-путь в M2.2c-2c. Поэтому
/// маршрутизируются только заведомо изолированные void-вызовы без чтения следом.
pub(crate) fn route_eval_js(
    engine: Option<&engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    js: Option<&Arc<dyn PersistentJs>>,
    script: String,
) {
    route_task_js(engine, js, move |js| js.eval_js(&script));
}

/// ADR-016 M2.2c-2d: обобщённый маршрутизатор fire-and-forget void-*действия*
/// над JS-хэндлом.
///
/// Обобщает [`route_eval_js`] (частный случай `|js| js.eval_js(&script)`) на
/// любое void-действие над `&Arc<dyn PersistentJs>` — нужно для батча
/// per-tick pump-вызовов (`tick_timers`/`pump_websockets`/`pump_sse`/
/// `pump_workers`/`pump_broadcast_channels`/`pump_shared_workers`), которые не
/// сводятся к одному `eval_js` (часть — прямые методы рантайма). Свободная
/// функция (а не метод `Lumen`), чтобы на call-site'е заимствовать поля
/// `engine_thread`/`js_ctx` **раздельно** (disjoint borrow).
///
/// - движковый поток есть (`LUMEN_ENGINE_THREAD=1`) → действие уходит
///   **off-UI-thread** через [`engine_thread::EngineThread::task`] (не блокирует
///   UI-поток; исполнится по порядку среди прочих `Task` — так что последующий
///   `query` встаёт в очередь **после** него, сохраняя read-after-write порядок);
/// - потока нет (флаг выключен, по умолчанию) → синхронный вызов по UI-хэндлу
///   `js` — **байт-идентично** прежним прямым `js.<method>()`.
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

/// ADR-016 M2.2c-2c: маршрутизатор **value-returning** UI→JS чтения.
///
/// Дополняет [`route_eval_js`] (fire-and-forget) для вызовов, чей результат нужен
/// UI-стороне **сейчас**: `take_dom_dirty` → `bool`, `take_raf_pending` → `bool`,
/// `eval_js_value` → `Result<String, String>`, а также (остаток 2c) nav/timer-чтения
/// `take_navigate_request` → `Option<JsNavigateRequest>`, `take_timer_wakeup` →
/// `Option<f64>` и nav-update drain `take_nav_updates` → `Vec<_>`. Как и `route_eval_js`
/// — свободная функция ради disjoint-borrow полей `engine_thread`/`js_ctx`.
///
/// - движковый поток есть (`LUMEN_ENGINE_THREAD=1`) → чтение идёт через
///   [`engine_thread::EngineThread::query`] (блокирующий request/reply). Это
///   ставит чтение **в очередь после** любого уже отправленного `task` (напр.
///   маршрутизированного `eval_js`) — тем самым восстанавливая read-after-eval
///   порядок, намеренно оставленный синхронным в M2.2c-2b;
/// - потока нет (флаг выключен, по умолчанию) → синхронный вызов по UI-хэндлу —
///   **байт-идентично** прежнему прямому `js.<read>()`.
///
/// Возвращает `None`, когда JS-контекста нет вовсе (нет UI-хэндла / состояние
/// движкового потока ещё не зеркалировано / поток завершён при shutdown); в этом
/// случае вызывающая сторона подставляет значение-по-умолчанию своей ветки «без
/// JS» (напр. `unwrap_or(false)` для `take_dom_dirty`) — как и без флага, где
/// `js_ctx == None` даёт ту же ветку.
pub(crate) fn route_query_js<R: Send + 'static>(
    engine: Option<&engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    js: Option<&Arc<dyn PersistentJs>>,
    read: impl FnOnce(&Arc<dyn PersistentJs>) -> R + Send + 'static,
) -> Option<R> {
    match engine {
        // `query` вернёт `Some(inner)`, где `inner` — результат `read`, либо `None`
        // если хэндл ещё не зеркалирован в состояние; двойной `Option` схлопываем
        // `flatten`. `query` целиком вернёт `None` при завершённом потоке → тоже `None`.
        Some(engine) => engine
            .query(move |state| state.js.as_ref().map(read))
            .flatten(),
        None => js.map(read),
    }
}

/// Whether the ADR-016 engine thread should be spawned.
///
/// **ADR-023 (default flip 2026-07-28): now enabled by default.** ADR-016's
/// M0–M4.1 stages all landed behind `LUMEN_ENGINE_THREAD=1` and each one was
/// accepted as byte-identical with the flag off, so the flag had become a
/// finished-but-unused feature. Leaving it off kept every `relayout()` on the
/// UI thread, which is what makes real sites hang on load: a page with N
/// `@font-face` files pays N serialized full relayouts before its first frame
/// (measured on lenta.ru — 9 fonts, ~300–700 ms each, first frame ~6.7 s → ~3.6 s
/// with the thread on; see `bugs/BUG-274-OPEN.md`, срез 2026-07-28).
///
/// Rollback (same flag-strategy idiom as ADR-018's V8 cutover and ADR-021's
/// chrome flip): `LUMEN_NO_ENGINE_THREAD=1` — or `LUMEN_ENGINE_THREAD=0` for
/// callers already setting the historical variable — restores the fully
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

/// ADR-016 M2.2: поднимает движковый поток, если он не отключён явно
/// ([`engine_thread_enabled`] — с ADR-023 включён по умолчанию). В
/// M2.2 через поток маршрутизируется off-thread layout для async-триггеров
/// (пока — debounce-зум): [`Lumen::submit_relayout_job`] шлёт задание, поток
/// считает [`EngineCommit`] и кладёт в latest-wins слот, откуда его забирает
/// [`Lumen::poll_engine_commit`]. При сбое старта потока логируем и откатываемся
/// на `None` (как обычно, без движкового потока — синхронный `relayout()`).
pub(crate) fn spawn_engine_thread_if_enabled()
-> Option<engine_thread::EngineThread<EngineCommit, EngineJsState>> {
    if !engine_thread_enabled() {
        return None;
    }
    // ADR-016 M2.2c-2b: поток владеет `EngineJsState` (будущее сиденье `Document`
    // + `js_ctx`); стартует пустым (`EngineJsState::default()` через `spawn()`),
    // заполняется `sync_engine_js_state` при первой загрузке страницы.
    match engine_thread::EngineThread::<EngineCommit, EngineJsState>::spawn() {
        Ok(engine) => {
            eprintln!(
                "[engine-thread] запущен (ADR-023 дефолт, M2.2 off-thread layout; \
                 откат — LUMEN_NO_ENGINE_THREAD=1)"
            );
            Some(engine)
        }
        Err(e) => {
            eprintln!("[engine-thread] не удалось запустить: {e}; продолжаем без него");
            None
        }
    }
}

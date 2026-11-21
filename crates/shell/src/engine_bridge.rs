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
    /// BUG-935 S80: стили полного каскада этого прохода — только под
    /// `LUMEN_BUG935_M4_SWAP`; кладутся в `Lumen::page_prev_cascade_styles`
    /// после применения коммита, чтобы следующий тик инкрементального рестайла
    /// не стартовал с холодного кэша (полный каскад на UI-потоке, 0,6–2 с).
    pub(crate) cascade_styles: Option<lumen_layout::counters::CascadeStyles>,
    /// Интерактивное состояние (hover/focus/active), под которым посчитаны
    /// `cascade_styles` — парное `Lumen::page_prev_interactive`.
    pub(crate) interactive: (Option<lumen_dom::NodeId>, Option<lumen_dom::NodeId>, Option<lumen_dom::NodeId>),
    /// BUG-935 S41: `apply_relayout_result`'s JS-geometry-collection block
    /// (`collect_computed_styles` и Co.), precomputed here on the engine
    /// thread — S37-S40 found it the dominant cost of every commit, already
    /// paid on the UI thread today regardless of routing. `None` when the job
    /// was built without a JS context present (`js_present=false` at
    /// submission time); `apply_relayout_result` then falls back to its own
    /// inline collection, matching pre-S41 behaviour.
    /// PERF-RELAYOUT-JOB: для инкрементальных off-thread заданий — ack
    /// водяного знака acked-читалки затронутых узлов. Вызывается только при
    /// реальном применении коммита: вытесненное задание не сдвигает отметку,
    /// и его DOM-изменения достаются следующему чтению. `None` у полных
    /// проходов и у задач без JS-трекера.
    pub(crate) touched_ack: Option<(std::sync::Arc<dyn Fn(u64) + Send + Sync>, u64)>,
    #[cfg(feature = "v8")]
    pub(crate) precollected: Option<crate::js_precollect::PrecollectedJsData>,
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
///
/// `#[track_caller]` (BUG-935 S23): пробрасывает call site вызывающего сквозь
/// `route_task_js` до `EngineThread::task`, где он попадает в `[engine] task`-лог.
#[track_caller]
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
///
/// `#[track_caller]` (BUG-935 S23): пробрасывает call site вызывающего сквозь
/// `EngineThread::task` до `[engine] task`-лога.
#[track_caller]
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
///
/// `#[track_caller]` (BUG-935 S23): пробрасывает call site вызывающего сквозь
/// `EngineThread::query` до `[engine] task`-лога.
#[track_caller]
pub(crate) fn route_query_js<R: Send + 'static>(
    engine: Option<&engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    js: Option<&Arc<dyn PersistentJs>>,
    read: impl FnOnce(&Arc<dyn PersistentJs>) -> R + Send + 'static,
) -> Option<R> {
    // UX-DIALOGS: JS-поток стоит в модальном диалоге и не ответит; блокирующее
    // чтение повисло бы до `QUERY_TIMEOUT`, не давая UI нарисовать модалку.
    if lumen_js::dialog::js_blocked() {
        return None;
    }
    // COMPAT-HANG: то же для зависшего скрипта — движковый поток стоит за ним,
    // и каждое чтение ждало бы `QUERY_TIMEOUT`, замораживая UI вместе со страницей.
    if lumen_core::script_stall::any_hung() {
        return None;
    }
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

/// PERF-INPUT-1: non-blocking variant of [`route_query_js`] for per-frame
/// polls (`has_raf_pending`, `take_dom_dirty`, `take_css_motion_cmds`, …).
///
/// A blocking `query` behind a busy engine thread waits up to
/// `QUERY_TIMEOUT` (5 s) — measured on market.yandex as whole `[frame] anim`
/// steps of ~5000 ms, freezing the UI window. These polls are pure drains or
/// reads whose skipped tick is retried on the next frame (the drained state
/// stays queued engine-side), so when the engine reports `busy` we return
/// `None` **without enqueueing** — one degraded frame instead of a frozen
/// window. Effectful reads (`eval_js*`, navigate intercepts) must keep using
/// [`route_query_js`]: skipping them would drop the script or mis-pair a
/// drained result with a later event.
pub(crate) fn route_poll_js<R: Send + 'static>(
    engine: Option<&engine_thread::EngineThread<EngineCommit, EngineJsState>>,
    js: Option<&Arc<dyn PersistentJs>>,
    read: impl FnOnce(&Arc<dyn PersistentJs>) -> R + Send + 'static,
) -> Option<R> {
    if let Some(engine) = engine
        && engine.busy().is_some()
    {
        return None;
    }
    route_query_js(engine, js, read)
}

/// THREAD-9: кладёт результат `take_navigate_request` в почтовый ящик
/// (`Lumen::js_nav_inbox`) из движковой задачи. `None` ящик не трогает, чтобы
/// пустое чтение не стёрло ещё не забранный запрос.
pub(crate) fn deposit_js_navigate(
    inbox: &Mutex<Option<crate::JsNavigateRequest>>,
    nav: Option<crate::JsNavigateRequest>,
) {
    if let Some(nav) = nav
        && let Ok(mut slot) = inbox.lock()
    {
        *slot = Some(nav);
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

/// PERF-INPUT-1: отдельный поток только для layout-заданий (`Run`/`Readback`).
/// Раньше `Run` и JS-`Task` делили один FIFO: 5-секундный layout-джоб
/// блокировал вводные JS-события (клики, pointermove) на то же время.
/// Layout-джоб самодостаточен — ему нужны лишь immutable Arc-снимки
/// (`make_relayout_job`), состояние `EngineJsState` не требуется, поэтому
/// достаточно stateless-потока `S = ()`. Коммиты по-прежнему единственные —
/// с этого потока, generation-guard в `poll_engine_commit` не меняется.
pub(crate) fn spawn_layout_thread_if_enabled()
-> Option<engine_thread::EngineThread<EngineCommit>> {
    if !engine_thread_enabled() {
        return None;
    }
    match engine_thread::EngineThread::<EngineCommit>::spawn() {
        Ok(engine) => {
            eprintln!("[layout-thread] запущен (PERF-INPUT-1: layout jobs off the JS engine thread)");
            Some(engine)
        }
        Err(e) => {
            eprintln!("[layout-thread] не удалось запустить: {e}; продолжаем без него");
            None
        }
    }
}

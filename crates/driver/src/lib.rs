//! `lumen-driver` — программный интерфейс к браузерному движку Lumen.
//!
//! Открывает уровни 2–3 тестирования (lumen-plan.md §15):
//! - **Уровень 2** — структурные ассерты: layout snapshot, DOM query, a11y-tree.
//! - **Уровень 3** — in-process snapshot: тот же Rust-процесс, без ffmpeg/gdigrab.
//!
//! # Архитектура
//!
//! ```text
//! BrowserSession (trait)
//!   ├── InProcessSession  ← движок напрямую (headless, без winit/wgpu)
//!   ├── future: WinitSession  ← оконный браузер (lumen-shell клиент)
//!   └── future: WsBiDiSession  ← remote через WebDriver BiDi / MCP
//! ```
//!
//! # Быстрый старт
//!
//! ```rust,no_run
//! use lumen_driver::{BrowserSession, InProcessSession, Target, WaitCondition};
//!
//! let mut session = InProcessSession::new();
//! session.navigate("file:///path/to/page.html").unwrap();
//! let boxes = session.layout_snapshot().unwrap();
//! println!("boxes: {}", boxes.len());
//! ```

mod types;
pub mod automation;
pub mod context;
pub mod determinism;
pub mod explain;
mod font_faces;
pub mod isolation;
pub mod live_session;
pub mod scope;
pub mod session;
pub mod winit_session;
pub mod gpu_session;

pub use types::{
    A11yBounds, A11yNode, A11yState, AxQuery, AutomationCommand, AutomationReply, AutomationDialog, AutomationDownload, BoxModel, ComputedProperties, ConsoleEntry, ConsoleLevel,
    AutomationCookie, ExplainElement, ExplainPage, ExplainPagePhaseTimings, FingerprintProfile, InputCommand, InterceptedRequest,
    InvariantViolationCounts, NavigationSnapshot, NetworkEntry, NodeRef, ScrollDelta, TabInfo, Target, WaitCondition,
};
pub use automation::{AutomationHandle, AutomationRequest};
pub use live_session::LiveWindowSession;
pub use session::InProcessSession;
pub use winit_session::WinitSession;
pub use gpu_session::{GpuSession, RenderedPage, JsNavigateRequest};
pub use isolation::{OriginGroup, OriginIsolationContext};
pub use determinism::ClockMode;
/// Типизированный снимок вычисленных CSS-свойств из lumen-layout.
///
/// Возвращается [`BrowserSession::computed_style_snapshot`]; предпочтительнее
/// [`ComputedProperties`] для структурных ассертов в тестах, потому что поля
/// типизированы, а не сериализованы в строки.
pub use lumen_layout::ComputedStyleSnapshot;

use lumen_core::error::Result;

/// Результат [`BrowserSession::eval_await_promise`] (MCP-TOTAL-S7f): итог
/// ожидания промисного выражения.
#[derive(Debug, PartialEq)]
pub enum AwaitedPromise {
    /// Промис исполнен: `Some` — значение как JSON-строка, `None` — исполнен
    /// с `undefined` (JSON его не переносит).
    Fulfilled(Option<String>),
    /// Промис не осел на микротасках (макротаск/IO) — остался pending.
    Pending,
}

/// Программный интерфейс к браузерному сеансу.
///
/// Разделён на **ресурсы** (read-only снимки текущего состояния) и
/// **инструменты** (команды, изменяющие состояние браузера).
///
/// Реализация может быть headless (in-process движок без UI), оконной
/// (winit/wgpu) или удалённой (BiDi/MCP через сеть).
///
/// Все методы синхронные; async-обёртка появится в рамках задачи 8B (MCP).
pub trait BrowserSession {
    // ── Ресурсы ────────────────────────────────────────────────────────────

    /// Снимок экрана в формате PNG. Для headless (без GPU) — возвращает
    /// `Err` до реализации задачи 8A.5 (tinyskia-cpu-raster).
    fn screenshot(&self) -> Result<Vec<u8>>;

    /// Снимок accessibility-дерева. Опирается на lumen-a11y (задача P1);
    /// до его готовности возвращает дерево с ролями из DOM-тегов.
    fn a11y_tree(&self) -> Result<A11yNode>;

    /// Найти первый узел в accessibility-дереве, совпадающий с запросом [`AxQuery`].
    ///
    /// # Примеры
    /// ```ignore
    /// let button = session.query_a11y(&AxQuery::Role {
    ///     role: "button".into(),
    ///     name: Some("Click".into()),
    /// }).unwrap();
    ///
    /// let any_button = session.query_a11y(&AxQuery::Role {
    ///     role: "button".into(),
    ///     name: None,
    /// }).unwrap();
    /// ```
    fn query_a11y(&self, query: &AxQuery) -> Result<Option<A11yNode>>;

    /// Найти все узлы в accessibility-дереве, совпадающие с запросом [`AxQuery`].
    fn query_a11y_all(&self, query: &AxQuery) -> Result<Vec<A11yNode>>;

    /// Box-model всех layout-блоков текущей страницы в координатах документа.
    fn layout_snapshot(&self) -> Result<Vec<BoxModel>>;

    /// Вычисленные CSS-свойства первого элемента, совпадающего с `selector`.
    /// Возвращает `Ok(None)` если элемент не найден.
    fn computed_style(&self, selector: &str) -> Result<Option<ComputedProperties>>;

    /// Типизированный снимок вычисленных CSS-свойств первого элемента,
    /// совпадающего с `selector`. Использует полный CSS3-движок селекторов.
    ///
    /// В отличие от [`computed_style`](BrowserSession::computed_style), возвращает
    /// [`ComputedStyleSnapshot`] — структуру с типизированными полями, пригодную
    /// для структурных ассертов в тестах.
    ///
    /// Возвращает `Ok(None)` если элемент не найден или не имеет layout-бокса
    /// (инлайн-элементы в Phase 0).
    fn computed_style_snapshot(&self, selector: &str) -> Result<Option<ComputedStyleSnapshot>>;

    /// Box-model первого элемента, совпадающего с `selector`.
    ///
    /// Удобный getter для получения позиции и размера одного элемента без
    /// итерации по всему layout_snapshot(). Эквивалентен поиску в layout_snapshot().
    ///
    /// Возвращает `Ok(None)` если элемент не найден или не имеет layout-бокса.
    fn layout_box_by_selector(&self, selector: &str) -> Result<Option<BoxModel>>;

    /// Все box-модели элементов, совпадающих с `selector`.
    ///
    /// Возвращает пустой вектор, если ни один элемент не совпал.
    fn all_layout_boxes_by_selector(&self, selector: &str) -> Result<Vec<BoxModel>>;

    /// Причинная цепочка для первого элемента, совпадающего с `selector`
    /// (DEVX-10, ADR-024 L1 `x-explain-element`): DOM → style → layout →
    /// size → stacking context → paint → clip → layer. См. [`ExplainElement`].
    ///
    /// Если элемент не найден в DOM, возвращается [`ExplainElement`] со всеми
    /// полями по умолчанию (`in_dom: false`) — не `Ok(None)`, потому что сама
    /// цепочка уже отвечает на вопрос "где она обрывается".
    fn explain_element(&self, selector: &str) -> Result<ExplainElement>;

    /// Page-level aggregate (DEVX-11, ADR-024 L1 `x-explain-page`): invariant-firing
    /// counts by category plus telemetry (box counts, overflow, commands, clip
    /// depth, relayouts, timing). См. [`ExplainPage`].
    fn explain_page(&self) -> Result<ExplainPage>;

    /// Box-model только поддерева первого элемента, совпадающего с `selector`
    /// (DEVX-12, ADR-024 L1 `x-scope-layout`): сам элемент и все его потомки,
    /// в порядке документа. Дополняет [`layout_snapshot`](BrowserSession::layout_snapshot)
    /// (вся страница) для случаев, когда важна только одна её часть.
    ///
    /// Возвращает пустой вектор, если `selector` ничего не находит (совпадает
    /// с [`layout_box_by_selector`](BrowserSession::layout_box_by_selector) —
    /// в т.ч. когда элемент есть в DOM, но не имеет layout-бокса).
    fn layout_snapshot_scoped(&self, selector: &str) -> Result<Vec<BoxModel>>;

    /// PNG-снимок, обрезанный по border-box первого элемента, совпадающего с
    /// `selector` (DEVX-12, ADR-024 L1 `x-scope-screenshot`).
    ///
    /// # Errors
    /// `Err`, если `selector` ничего не находит, элемент не имеет layout-бокса,
    /// либо его прямоугольник целиком вне области снимка — обрезанный снимок
    /// «ничего» не является полезным ответом (в отличие от остальных scoped-
    /// чтений, которые в этом случае просто пусты).
    fn screenshot_scoped(&self, selector: &str) -> Result<Vec<u8>>;

    /// Полный текстовый дамп display list текущей страницы (DEVX-14, ADR-024
    /// L1 `resource://x-display-list`) — то же самое, что печатает CLI
    /// `lumen --dump-display-list`, но через wire-протокол. Дополняет
    /// [`display_list_scoped`](BrowserSession::display_list_scoped)
    /// (одно поддерево) для случаев, когда нужна вся страница целиком.
    ///
    /// # Errors
    /// `Err`, если страница не загружена (`navigate()` ещё не вызывался).
    fn display_list(&self) -> Result<String>;

    /// Команды display list, приписанные (через provenance DEVX-7) поддереву
    /// первого элемента, совпадающего с `selector` (DEVX-12, ADR-024 L1
    /// `x-scope-display-list`): сам элемент и все его потомки, в исходном
    /// порядке display list. Дополняет команд-счётчик одного узла из
    /// `x-explain-element` фактическими командами всего региона страницы.
    ///
    /// # Errors
    /// `Err`, если `selector` ничего не находит.
    fn display_list_scoped(&self, selector: &str) -> Result<String>;

    /// Журнал сетевых запросов с момента последней навигации.
    fn network_log(&self) -> Result<Vec<NetworkEntry>>;

    /// Журнал вызовов console.log/warn/error с момента последней навигации.
    fn console_log(&self) -> Result<Vec<ConsoleEntry>>;

    /// URL текущей страницы (пустая строка если страница не загружена).
    ///
    /// Returns an owned `String` (not `&str`) so implementations that track
    /// the URL behind a lock or a remote round-trip (e.g. [`LiveWindowSession`])
    /// don't need to leak memory to satisfy the borrow.
    fn current_url(&self) -> String;

    // ── Инструменты ────────────────────────────────────────────────────────

    /// Загрузить страницу по URL (поддерживаются `file://` и `http(s)://`).
    /// Блокируется до завершения загрузки и первого layout.
    fn navigate(&mut self, url: &str) -> Result<()>;

    /// Открыть новую вкладку (она становится активной) и загрузить в неё `url`.
    ///
    /// Сессии без таб-стрипа (headless `InProcessSession`) по умолчанию
    /// откатываются к обычному [`BrowserSession::navigate`] в текущем документе.
    fn new_tab(&mut self, url: &str) -> Result<()> {
        self.navigate(url)
    }

    /// Перезагрузить активную вкладку (MCP-TOTAL-S7e, BiDi
    /// `browsingContext.reload`): текущий источник грузится заново;
    /// `ignore_cache` — запрос обхода HTTP-кэша (движок его пока не
    /// различает). Ответ — [`NavigationSnapshot`]: адрес и флаги истории.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn reload(&self, ignore_cache: bool) -> Result<NavigationSnapshot> {
        let _ = ignore_cache;
        Err(lumen_core::error::Error::Other("reload: только для живого окна (--mcp-live-port)".into()))
    }

    /// Шаг по истории сессии (MCP-TOTAL-S7e, BiDi
    /// `browsingContext.traverseHistory`): минус — назад, плюс — вперёд,
    /// `0` — перезагрузка текущей записи (HTML LS §7.8.3). Ответ —
    /// [`NavigationSnapshot`].
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно; у живого
    /// окна — выход за границы истории.
    fn traverse_history(&self, delta: i32) -> Result<NavigationSnapshot> {
        let _ = delta;
        Err(lumen_core::error::Error::Other("traverse_history: только для живого окна (--mcp-live-port)".into()))
    }

    /// Открытые вкладки в порядке таб-стрипа (COMPAT-CORPUS-3-TABS).
    ///
    /// Сессии без таб-стрипа вкладок не различают — по умолчанию `Err`.
    fn list_tabs(&self) -> Result<Vec<TabInfo>> {
        Err(lumen_core::error::Error::Other("list_tabs: сессия без вкладок".into()))
    }

    /// Сделать активной вкладку со стабильным id `id` (из [`BrowserSession::list_tabs`]):
    /// все следующие команды действуют на неё (COMPAT-CORPUS-3-TABS).
    ///
    /// # Errors
    /// `Err`, если вкладки с таким id нет или сессия без вкладок.
    fn switch_tab(&mut self, id: usize) -> Result<()> {
        let _ = id;
        Err(lumen_core::error::Error::Other("switch_tab: сессия без вкладок".into()))
    }

    /// Закрыть вкладку `id` (COMPAT-CORPUS-3-TABS). Последняя вкладка не
    /// закрывается — это завершило бы браузер.
    ///
    /// # Errors
    /// `Err`, если вкладки нет, она последняя или сессия без вкладок.
    fn close_tab(&mut self, id: usize) -> Result<()> {
        let _ = id;
        Err(lumen_core::error::Error::Other("close_tab: сессия без вкладок".into()))
    }

    /// Кликнуть по цели. Для `Target::Selector` берётся центр первого
    /// совпадающего элемента. Для headless — обновляет layout без GPU.
    fn click(&mut self, target: &Target) -> Result<()>;

    /// Ввести текст в поле, совпадающее с `target`. Симулирует посимвольный
    /// ввод через event-loop.
    fn type_text(&mut self, target: &Target, text: &str) -> Result<()>;

    /// Прокрутить содержимое на `delta` логических пикселей.
    fn scroll(&mut self, target: &Target, delta: ScrollDelta) -> Result<()>;

    /// Ожидать выполнения условия `cond`. Блокируется до `timeout_ms` мс;
    /// при превышении — `Err(Error::Other("timeout"))`.
    fn wait(&mut self, cond: WaitCondition, timeout_ms: u64) -> Result<()>;

    /// Выполнить JS-код и вернуть результат как JSON-строку.
    /// Для in-process headless — QuickJS eval (если доступен).
    ///
    /// `&mut self`: in-process headless sessions re-layout after a successful
    /// eval (DEVX-9) so that `layout_snapshot`/`layout_box_by_selector`/
    /// `screenshot` reflect any DOM mutation the script made.
    fn eval(&mut self, js: &str) -> Result<String>;

    /// [`Self::eval`] with an explicit wait budget in milliseconds (BUG-1145).
    ///
    /// Only the live window has something to wait for — its engine thread can
    /// be busy with earlier work — so the default ignores `timeout_ms`.
    fn eval_with_timeout(&mut self, js: &str, timeout_ms: u64) -> Result<String> {
        let _ = timeout_ms;
        self.eval(js)
    }

    /// Ождать промисное выражение (MCP-TOTAL-S7f): выражение, значением
    /// которого является промис, → его осевшее значение. Отклонение — `Err`
    /// с текстом причины («uncaught (in promise): …»).
    ///
    /// `Self::eval` синхронен и снимает значение выражения до опустошения
    /// очереди микротасков, поэтому промисное выражение без ожидания даёт
    /// неосевший промис-объект. V8 при этом выполняет микротаск-чекпоинт в
    /// конце каждого eval, так что хватает двух заходов:
    ///
    /// 1. обернуть выражение в `Promise.resolve(...)` и навесить обработчик
    ///    оседания, записывающий `{state, value|error}` в well-known глобал;
    ///    чекпоинт исполняет обработчик до возврата из первого захода.
    /// 2. прочитать глобал обратно.
    ///
    /// Оседают только промисы на микротасках; промис, ждущий макротаска/IO,
    /// возвращает [`AwaitedPromise::Pending`] — вызывающий отступает к
    /// промис-объекту, как без ожидания.
    fn eval_await_promise(&mut self, expr: &str) -> Result<AwaitedPromise> {
        // Заход 1: исполнить выражение, записать исход оседания в глобал.
        let setup = format!(
            "(function(){{\
                 globalThis.__lumen_await = {{ state: \"pending\" }};\
                 try {{\
                     Promise.resolve(({expr})).then(\
                         function(v){{ globalThis.__lumen_await = (v === undefined) ? {{ state: \"fulfilled\" }} : {{ state: \"fulfilled\", value: v }}; }},\
                         function(e){{ globalThis.__lumen_await = {{ state: \"rejected\", error: (e && e.message !== undefined) ? String(e.message) : String(e) }}; }}\
                     );\
                 }} catch (e) {{\
                     globalThis.__lumen_await = {{ state: \"rejected\", error: (e && e.message !== undefined) ? String(e.message) : String(e) }};\
                 }}\
                 return \"pending\";\
             }})()"
        );
        self.eval(&setup).map_err(|e| lumen_core::error::Error::Other(format!("eval: {e}")))?;

        // Заход 2: прочитать исход, оставленный микротаск-чекпоинтом.
        let read = "(function(){ var r = globalThis.__lumen_await; \
             try { delete globalThis.__lumen_await; } catch (e) {} return r; })()";
        let raw = self.eval(read).map_err(|e| lumen_core::error::Error::Other(format!("eval: {e}")))?;

        let outcome: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| lumen_core::error::Error::Other(format!("eval: {e}")))?;
        match outcome.get("state").and_then(|v| v.as_str()) {
            Some("fulfilled") => Ok(AwaitedPromise::Fulfilled(
                outcome.get("value").map(|v| v.to_string()),
            )),
            Some("rejected") => Err(lumen_core::error::Error::Other(format!(
                "uncaught (in promise): {}",
                outcome.get("error").and_then(|v| v.as_str()).unwrap_or("")
            ))),
            // Ещё pending (ждёт макротаска/IO).
            _ => Ok(AwaitedPromise::Pending),
        }
    }

    /// Найти DOM-узлы по CSS-селектору. Возвращает пустой вектор, если
    /// ни один узел не совпал.
    fn query(&self, selector: &str) -> Result<Vec<NodeRef>>;

    /// Найти DOM-узлы по CSS-селектору `selector` в границах поддерева
    /// первого элемента, совпадающего с `root_selector` (DEVX-12, ADR-024 L1
    /// `x-scope-query`) — эквивалент `Element.querySelectorAll` scoping (DOM
    /// LS §4.2.6), в отличие от [`query`](BrowserSession::query), которая
    /// всегда ищет по всему документу.
    ///
    /// Возвращает пустой вектор, если `root_selector` ничего не находит.
    fn query_scoped(&self, root_selector: &str, selector: &str) -> Result<Vec<NodeRef>>;

    /// Пересчитать layout только для поддерева первого элемента, совпадающего
    /// с `selector` (DEVX-12), используя уже влитую механику
    /// `lumen_layout::incremental` (`DirtyBits`/`mark_dirty`/`lay_out_incremental`,
    /// срезы S16–S27) напрямую на уже существующем дереве — без пересборки из
    /// DOM, в отличие от полного релейаута DEVX-9. Перед пересчётом геометрии
    /// заново вычисляется `ComputedStyle` **только самого целевого узла**
    /// (родительский `.style` — уже корректный inherited-базис), иначе мутация
    /// вроде `el.style.width = '200px'` была бы не видна `lay_out_incremental`,
    /// который использует уже сохранённый в дереве стиль. Полный каскад по
    /// поддереву сознательно не делается — та же ловушка идентичности
    /// anonymous-box/pseudo-element, что и у приостановленного BUG-341's
    /// `layout_mutation_incremental_restyle`, эта задача его не возобновляет.
    ///
    /// **Предусловие (документированное, не проверяется в рантайме — тот же
    /// стиль, что у `lay_out_incremental`'s собственного doc-комментария):**
    /// (1) структура DOM (список детей) внутри `selector` не должна была
    /// измениться с последнего полного layout — добавление/удаление/
    /// перестановка элементов не отражаются; (2) наследуемые свойства,
    /// изменившиеся на `selector`, не каскадируются на потомков (например,
    /// смена `font-size` на самом `selector` не пересчитает em-размеры внутри
    /// него) — только собственный стиль узла. Для структурных изменений или
    /// изменений, которые должны каскадироваться, используйте полный релейаут
    /// (происходит автоматически после `click`/`type_text`/
    /// [`eval`](BrowserSession::eval)). Существует как перф-подсказка, а не
    /// отдельный путь корректности — нарушение предпосылки даёт устаревшую
    /// геометрию, не панику/UB.
    ///
    /// # Errors
    /// `Err`, если `selector` ничего не находит в DOM или в layout-дереве,
    /// либо сессия не инициализирована.
    fn relayout_scoped(&mut self, selector: &str) -> Result<()>;

    /// [`eval`](BrowserSession::eval), но вместо полного релейатута DEVX-9
    /// вызывает [`relayout_scoped`](BrowserSession::relayout_scoped) для
    /// `selector` (DEVX-12) — для вызывающих, которые знают, что `js`
    /// мутирует только одно поддерево, и хотят более дешёвый пересчёт.
    fn eval_scoped(&mut self, selector: &str, js: &str) -> Result<String>;

    // ── Ввод, cookies и preload живого окна ─────────────────────────────────

    /// Переместить указатель вьюпорта в `(x, y)` (CSS px контентной области),
    /// ничего не нажимая (BUG-1194): страница получает `pointermove` и
    /// поднимает `:hover`.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно
    /// (`--mcp-live-port`).
    fn pointer_move(&self, x: f32, y: f32) -> Result<()> {
        let _ = (x, y);
        Err(lumen_core::error::Error::Other("pointer_move: только для живого окна (--mcp-live-port)".into()))
    }

    /// Нажать левую кнопку в `(x, y)` (CSS px вьюпорта) — открывает
    /// text-selection drag, который продолжает `pointer_move` (WPT
    /// `selection-pointer.html`).
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn pointer_down(&self, x: f32, y: f32) -> Result<()> {
        let _ = (x, y);
        Err(lumen_core::error::Error::Other("pointer_down: только для живого окна (--mcp-live-port)".into()))
    }

    /// Отпустить левую кнопку — завершает открытый `pointer_down`-drag.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn pointer_up(&self) -> Result<()> {
        Err(lumen_core::error::Error::Other("pointer_up: только для живого окна (--mcp-live-port)".into()))
    }

    /// Один `keydown` (`down = true`) или `keyup` для нетекстовой клавиши
    /// на фокусном элементе, без клика (BUG-1194). `key` — `Escape`, `Enter`,
    /// `Shift`…; `code` — `KeyA`, `ShiftLeft`…
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn press_key(&self, key: &str, code: &str, down: bool) -> Result<()> {
        let _ = (key, code, down);
        Err(lumen_core::error::Error::Other("press_key: только для живого окна (--mcp-live-port)".into()))
    }

    /// Ввести `text` в фокусный элемент без предварительного клика (BUG-1194).
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn type_focused(&self, text: &str) -> Result<()> {
        let _ = text;
        Err(lumen_core::error::Error::Other("type_focused: только для живого окна (--mcp-live-port)".into()))
    }

    /// Отпустить всё зажатое автоматизацией (MCP-TOTAL-S7d, BiDi
    /// `input.releaseActions`): кнопку `pointer_down` (страница получает
    /// `pointerup`/`mouseup`, `:active` снят) и все удерживаемые модификаторы
    /// `key_down` (каждый получает свой `keyup`). Идемпотентно.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn release_actions(&self) -> Result<()> {
        Err(lumen_core::error::Error::Other("release_actions: только для живого окна (--mcp-live-port)".into()))
    }

    /// Колесо мыши в `(x, y)` (CSS px вьюпорта, MCP-TOTAL-S7d): страница
    /// получает доверенный `wheel` с `delta_x`/`delta_y` (px, плюс —
    /// вправо/вниз), затем прокрутка идёт тем же маршрутом, что у настоящего
    /// `MouseWheel`; `modifiers` — маска из `press_key` (ctrl 1, shift 2,
    /// alt 4, meta 8).
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn wheel(&self, x: f32, y: f32, delta_x: f32, delta_y: f32, modifiers: u8) -> Result<()> {
        let _ = (x, y, delta_x, delta_y, modifiers);
        Err(lumen_core::error::Error::Other("wheel: только для живого окна (--mcp-live-port)".into()))
    }

    /// Неистёкшие cookies активного jar живого окна, `HttpOnly` включительно
    /// (`storage.getCookies`, DEVX-20).
    ///
    /// # Errors
    /// По умолчанию `Err` — jar есть только у живого окна.
    fn get_cookies(&self) -> Result<Vec<AutomationCookie>> {
        Err(lumen_core::error::Error::Other("get_cookies: только для живого окна (--mcp-live-port)".into()))
    }

    /// Сохранить cookie в активный jar живого окна (`storage.setCookie`, DEVX-20).
    ///
    /// # Errors
    /// По умолчанию `Err` — jar есть только у живого окна.
    fn set_cookie(&self, cookie: AutomationCookie) -> Result<()> {
        let _ = cookie;
        Err(lumen_core::error::Error::Other("set_cookie: только для живого окна (--mcp-live-port)".into()))
    }

    /// Удалить одну cookie из активного jar живого окна
    /// (`storage.deleteCookies`, DEVX-20).
    ///
    /// # Errors
    /// По умолчанию `Err` — jar есть только у живого окна.
    fn delete_cookie(&self, domain: &str, path: &str, name: &str) -> Result<()> {
        let _ = (domain, path, name);
        Err(lumen_core::error::Error::Other("delete_cookie: только для живого окна (--mcp-live-port)".into()))
    }

    /// Зарегистрировать preload-скрипт в живом окне
    /// (`script.addPreloadScript`, BUG-1811): исполняется в каждом документе,
    /// созданном после вызова.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn add_preload_script(&self, id: &str, function_declaration: &str) -> Result<()> {
        let _ = (id, function_declaration);
        Err(lumen_core::error::Error::Other("add_preload_script: только для живого окна (--mcp-live-port)".into()))
    }

    /// Удалить preload-скрипт по id (`script.removePreloadScript`, BUG-1811).
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn remove_preload_script(&self, id: &str) -> Result<()> {
        let _ = id;
        Err(lumen_core::error::Error::Other("remove_preload_script: только для живого окна (--mcp-live-port)".into()))
    }

    /// Открытый JS-диалог живого окна (`alert`/`confirm`/`prompt`), если есть
    /// (MCP-TOTAL-S7a, BiDi `browsingContext.userPromptOpened`). Диалог
    /// блокирует JS-поток, поэтому ответ читается из состояния UI, не через
    /// него; `None` — диалога нет.
    ///
    /// # Errors
    /// По умолчанию `Err` — диалоги живого окна доступны только живому окну.
    fn user_prompt(&self) -> Result<Option<AutomationDialog>> {
        Err(lumen_core::error::Error::Other("user_prompt: только для живого окна (--mcp-live-port)".into()))
    }

    /// Ответить на открытый JS-диалог живого окна (MCP-TOTAL-S7a, BiDi
    /// `browsingContext.handleUserPrompt`): `accept` — OK/отмена,
    /// `user_text` — замена введённого текста `prompt`.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn handle_user_prompt(&self, accept: bool, user_text: Option<&str>) -> Result<()> {
        let _ = (accept, user_text);
        Err(lumen_core::error::Error::Other("handle_user_prompt: только для живого окна (--mcp-live-port)".into()))
    }

    /// Заполнить `<input type="file">` по `target` файлами `paths` без
    /// OS-диалога (MCP-TOTAL-S7b, BiDi `input.setFiles`): каждый путь должен
    /// существовать — он регистрируется непрозрачным грантом (JS путей не
    /// видит, BUG-371), доставка и события `input`/`change` — тем же путём,
    /// что после настоящего выбора в диалоге.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn set_files(&self, target: &Target, paths: &[String]) -> Result<()> {
        let _ = (target, paths);
        Err(lumen_core::error::Error::Other("set_files: только для живого окна (--mcp-live-port)".into()))
    }

    /// Загрузки живого окна (MCP-TOTAL-S7c, BiDi `browser.getDownloads`):
    /// каждая запись менеджера загрузок — id, URL, имя файла, состояние,
    /// счётчики байт и путь на диске.
    ///
    /// # Errors
    /// По умолчанию `Err` — менеджер загрузок есть только у живого окна.
    fn list_downloads(&self) -> Result<Vec<AutomationDownload>> {
        Err(lumen_core::error::Error::Other("list_downloads: только для живого окна (--mcp-live-port)".into()))
    }

    /// Начать загрузку `url` (MCP-TOTAL-S7c): файл уходит в папку загрузок
    /// ОС тем же путём, что и загрузка по клику страницы; возвращает id
    /// записи для [`list_downloads`](BrowserSession::list_downloads) /
    /// [`cancel_download`](BrowserSession::cancel_download).
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn start_download(&self, _url: &str) -> Result<u32> {
        Err(lumen_core::error::Error::Other("start_download: только для живого окна (--mcp-live-port)".into()))
    }

    /// Отменить загрузку `id` (MCP-TOTAL-S7c); запись оседает в состоянии
    /// `cancelled`.
    ///
    /// # Errors
    /// По умолчанию `Err` — команду исполняет только живое окно.
    fn cancel_download(&self, _id: u32) -> Result<()> {
        Err(lumen_core::error::Error::Other("cancel_download: только для живого окна (--mcp-live-port)".into()))
    }

    // ── Isolation & Fingerprinting (Phase 1: 8E/8F) ─────────────────────────

    /// Текущий профиль отпечатка браузера (fingerprint profile).
    ///
    /// Возвращает профиль, который был установлен при создании сессии или
    /// последним вызовом [`set_fingerprint_profile`](BrowserSession::set_fingerprint_profile).
    /// По умолчанию: `FingerprintProfile::Standard`.
    fn fingerprint_profile(&self) -> FingerprintProfile;

    /// Установить профиль отпечатка браузера для будущих операций.
    ///
    /// Влияет на User-Agent, TLS cipher ordering (если поддерживается),
    /// HTTP header order, и JS API returns (в Phase 2+).
    /// По ADR-007 §6.
    ///
    /// # Примеры
    /// ```ignore
    /// session.set_fingerprint_profile(FingerprintProfile::Strict)?;
    /// ```
    fn set_fingerprint_profile(&mut self, profile: FingerprintProfile) -> Result<()>;

    /// User-Agent строка для HTTP-запросов и JS `navigator.userAgent`.
    ///
    /// Возвращает установленную строку или default для текущего
    /// [`fingerprint_profile`](BrowserSession::fingerprint_profile).
    fn user_agent(&self) -> String;

    /// Переопределить User-Agent для будущих запросов и JS.
    ///
    /// Если не вызвано, используется default для текущего профиля.
    /// Переопределение сохраняется при смене профиля.
    fn set_user_agent(&mut self, ua: &str) -> Result<()>;

    /// Включить/выключить симуляцию отсутствия сети (WebDriver BiDi
    /// `network.setOfflineStatus`, BUG-295): при `true` все последующие сетевые
    /// запросы (навигация, `fetch()`/XHR, подресурсы) должны падать вместо
    /// похода в сеть.
    ///
    /// Реализация по умолчанию — `Err` (MCP-TOTAL-S1a: headless-сессия без
    /// реального сетевого слоя отвечает честно, а не молчаливым успехом);
    /// [`LiveWindowSession`] переопределяет метод для реального эффекта.
    fn set_offline(&mut self, _offline: bool) -> Result<()> {
        Err(lumen_core::error::Error::Other("set_offline: только для живого окна (--mcp-live-port)".into()))
    }

    /// Override the `Intl`/`Date` timezone (WebDriver BiDi
    /// `browser.setTimezoneOverride`, BUG-295). `Some(id)` is an IANA
    /// timezone identifier (e.g. `"America/New_York"`); `None` restores the
    /// host timezone.
    ///
    /// Default impl is `Err` (MCP-TOTAL-S1a, same rationale as [`set_offline`](
    /// BrowserSession::set_offline)); [`LiveWindowSession`] overrides it for
    /// real effect.
    fn set_timezone(&mut self, _timezone_id: Option<&str>) -> Result<()> {
        Err(lumen_core::error::Error::Other("set_timezone: только для живого окна (--mcp-live-port)".into()))
    }

    /// Set a permission state (WebDriver BiDi `permissions.setPermission`,
    /// BUG-1014). `Ok(false)` = the engine does not know `name`.
    ///
    /// Default impl is `Err` (MCP-TOTAL-S1a: a headless session has no page to
    /// observe the override, so reporting success would be a lie);
    /// [`LiveWindowSession`] overrides it.
    fn set_permission(&mut self, _name: &str, _state: &str) -> Result<bool> {
        Err(lumen_core::error::Error::Other("set_permission: только для живого окна (--mcp-live-port)".into()))
    }

    /// Role and accessible name of the element the selector chain resolves to
    /// (BUG-1014). `Ok(None)` = no such element.
    ///
    /// Default impl is `Err` (MCP-TOTAL-S1a: a headless session has no live
    /// page and no accessibility tree to answer from);
    /// [`LiveWindowSession`] overrides it.
    fn computed_a11y(&mut self, _selectors: &[String]) -> Result<Option<(String, String)>> {
        Err(lumen_core::error::Error::Other("computed_a11y: только для живого окна (--mcp-live-port)".into()))
    }

    /// Register a network intercept rule (WebDriver BiDi `network.addIntercept`,
    /// BUG-295 remainder). A subsequent request whose URL matches
    /// `url_patterns` (exact-string match, per BiDi urlPattern `type: "string"`;
    /// empty = match-all) for one of `phases` pauses at that phase until
    /// [`resolve_intercepted_request`](BrowserSession::resolve_intercepted_request)
    /// decides it or a bounded timeout elapses. [`LiveWindowSession`] pauses on
    /// `"beforeRequestSent"` and `"responseStarted"` (DEVX-20); `"authRequired"`
    /// is not paused on — the BiDi layer rejects it before it gets here.
    ///
    /// Default impl is `Err` (MCP-TOTAL-S1a, same rationale as [`set_offline`](
    /// BrowserSession::set_offline)); [`LiveWindowSession`] overrides it for
    /// real effect.
    fn add_intercept(&mut self, _id: &str, _phases: &[String], _url_patterns: &[String]) -> Result<()> {
        Err(lumen_core::error::Error::Other("add_intercept: только для живого окна (--mcp-live-port)".into()))
    }

    /// Remove a previously registered intercept rule (`network.removeIntercept`).
    ///
    /// Default impl is `Err`, same rationale as [`add_intercept`](BrowserSession::add_intercept).
    fn remove_intercept(&mut self, _id: &str) -> Result<()> {
        Err(lumen_core::error::Error::Other("remove_intercept: только для живого окна (--mcp-live-port)".into()))
    }

    /// Deliver a decision for a paused request (`network.continueRequest`/
    /// `network.continueResponse`/`network.failRequest`, BUG-295 remainder). `continue_request = true` lets
    /// the request proceed unmodified; `false` fails it.
    ///
    /// Returns `true` if `request_id` matched an outstanding pause — an
    /// unknown id is not an error (mirrors the bare-ACK tolerance the BiDi
    /// handlers already had before real bookkeeping existed).
    ///
    /// Default impl is `Err`, same rationale as
    /// [`add_intercept`](BrowserSession::add_intercept).
    fn resolve_intercepted_request(&mut self, _request_id: &str, _continue_request: bool) -> Result<bool> {
        Err(lumen_core::error::Error::Other("resolve_intercepted_request: только для живого окна (--mcp-live-port)".into()))
    }

    /// Requests newly paused by an active intercept since the last call —
    /// data for the `network.beforeRequestSent` / `network.responseStarted`
    /// events a BiDi connection should deliver to its client.
    ///
    /// Default impl is `Err`, same rationale as
    /// [`add_intercept`](BrowserSession::add_intercept).
    fn poll_intercepted_requests(&mut self) -> Result<Vec<InterceptedRequest>> {
        Err(lumen_core::error::Error::Other("poll_intercepted_requests: только для живого окна (--mcp-live-port)".into()))
    }

    // ── Deterministic mode (8F) ──────────────────────────────────────────────

    /// Управление режимом часов `Date.now()` / `performance.now()` (8F.1).
    ///
    /// - `ClockMode::Frozen(ms)` — все вызовы возвращают `ms`.
    /// - `ClockMode::Monotonic { step_ms }` — каждый вызов увеличивает счётчик на `step_ms`.
    /// - `ClockMode::Real` — системные часы (по умолчанию).
    ///
    /// Для InProcessSession: сохраняет режим в `SessionContext`. JS runtime применяет
    /// его при следующей навигации через `set_deterministic_mode()` (задача 8A.7).
    fn set_clock(&mut self, mode: ClockMode) -> Result<()>;

    /// Установить зерно ГПСЧ для воспроизводимого `Math.random()` (8F.2).
    ///
    /// `Some(seed)` — xorshift32 PRNG, сеяный `seed`; одинаковое зерно = одинаковая последовательность.
    /// `None` — восстановить OS entropy.
    ///
    /// Зерно применяется при следующей навигации через `JsRuntime::set_deterministic_mode()`.
    fn set_rng_seed(&mut self, seed: Option<u64>) -> Result<()>;

    /// Зафиксировать профиль отпечатка и заморозить его (8F.3).
    ///
    /// Устанавливает `profile` и запрещает дальнейшие изменения через
    /// `set_fingerprint_profile()`. В JS-слое canvas/WebGL/audio/font API
    /// возвращают детерминированные значения, соответствующие профилю:
    ///
    /// - **Canvas** — `toDataURL()` возвращает фиксированный blank hash.
    /// - **WebGL** — `getParameter(VENDOR/RENDERER)` нормализованы через `GpuFingerprint`.
    /// - **AudioContext** — `sampleRate` зафиксирован в 44100 Гц, сэмплы нулевые.
    /// - **Font enumeration** — `document.fonts` возвращает только bundled Inter.
    ///
    /// Для отмены заморозки используйте `set_fingerprint_profile()` — но если профиль
    /// заморожен, это вернёт `Err`.
    fn freeze_fingerprint(&mut self, profile: FingerprintProfile) -> Result<()>;
}

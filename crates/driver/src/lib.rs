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
pub mod isolation;
pub mod live_session;
pub mod scope;
pub mod session;
pub mod winit_session;
pub mod gpu_session;

pub use types::{
    A11yNode, A11yState, AxQuery, AutomationCommand, AutomationReply, BoxModel, ComputedProperties, ConsoleEntry, ConsoleLevel,
    ExplainElement, ExplainPage, ExplainPagePhaseTimings, FingerprintProfile, InputCommand, InvariantViolationCounts,
    NetworkEntry, NodeRef, ScrollDelta, Target, WaitCondition,
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

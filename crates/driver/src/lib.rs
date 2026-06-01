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
pub mod context;
pub mod isolation;
pub mod session;
pub mod winit_session;
pub mod gpu_session;

pub use types::{
    A11yNode, A11yState, AxQuery, BoxModel, ComputedProperties, ConsoleEntry, ConsoleLevel,
    FingerprintProfile, InputCommand, NetworkEntry, NodeRef, ScrollDelta, Target, WaitCondition,
};
pub use session::InProcessSession;
pub use winit_session::WinitSession;
pub use gpu_session::{GpuSession, RenderedPage, JsNavigateRequest};
pub use isolation::{OriginGroup, OriginIsolationContext};
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

    /// Журнал сетевых запросов с момента последней навигации.
    fn network_log(&self) -> Result<Vec<NetworkEntry>>;

    /// Журнал вызовов console.log/warn/error с момента последней навигации.
    fn console_log(&self) -> Result<Vec<ConsoleEntry>>;

    /// URL текущей страницы (пустая строка если страница не загружена).
    fn current_url(&self) -> &str;

    // ── Инструменты ────────────────────────────────────────────────────────

    /// Загрузить страницу по URL (поддерживаются `file://` и `http(s)://`).
    /// Блокируется до завершения загрузки и первого layout.
    fn navigate(&mut self, url: &str) -> Result<()>;

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
    fn eval(&self, js: &str) -> Result<String>;

    /// Найти DOM-узлы по CSS-селектору. Возвращает пустой вектор, если
    /// ни один узел не совпал.
    fn query(&self, selector: &str) -> Result<Vec<NodeRef>>;

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
}

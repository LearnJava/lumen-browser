//! Вспомогательные типы для [`BrowserSession`](crate::BrowserSession) API.
//!
//! Все типы — независимые value-объекты: не содержат ссылок на внутренние
//! структуры движка, поэтому их можно сериализовать и передавать через сеть
//! (MCP, BiDi, CDP-shim) без изменения ABI.

use lumen_core::geom::Rect;
use serde::{Deserialize, Serialize};

/// Ссылка на DOM-узел, возвращаемая [`BrowserSession::query`].
///
/// `node_id` соответствует [`lumen_dom::NodeId`]; lifetime node-а — до
/// следующей навигации или мутации DOM. Используется как аргумент [`Target`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeRef {
    /// Числовой ID узла в DOM-арене (совпадает с `NodeId::raw()`).
    pub node_id: u32,
    /// Имя тега в нижнем регистре (`"div"`, `"input"`, …). Пусто для
    /// текстовых узлов.
    pub tag_name: String,
    /// Склеенный текстовый контент поддерева.
    pub text_content: String,
    /// Граница border-box узла в координатах документа (логические пиксели).
    pub bounding_rect: Rect,
}

/// Цель для команд [`BrowserSession::click`], [`type_text`](BrowserSession::type_text),
/// [`scroll`](BrowserSession::scroll).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Target {
    /// CSS-селектор: выбирается первый совпадающий элемент.
    Selector(String),
    /// Конкретный узел по ID из [`NodeRef::node_id`].
    NodeId(u32),
    /// Координата в логических пикселях относительно левого верхнего угла документа.
    Point { x: f32, y: f32 },
}

/// Дельта скролла для [`BrowserSession::scroll`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ScrollDelta {
    /// Горизонтальная прокрутка (логические пиксели; положительное — вправо).
    pub x: f32,
    /// Вертикальная прокрутка (логические пиксели; положительное — вниз).
    pub y: f32,
}

/// Условие ожидания для [`BrowserSession::wait`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WaitCondition {
    /// `document.readyState == "complete"`.
    DocumentReady,
    /// Указанный CSS-селектор совпадает с видимым элементом.
    Visible(String),
    /// Layout узла перестал меняться (bounding-box стабилен 50 мс).
    Stable(String),
    /// Нет активных сетевых запросов (кроме SSE/WS).
    NetworkIdle,
    /// JS event loop пуст (нет pending microtask/task/rAF).
    JsIdle,
}

/// Box-model одного узла из [`BrowserSession::layout_snapshot`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoxModel {
    /// ID узла в DOM-арене.
    pub node_id: u32,
    /// CSS-селектор, по которому этот элемент найден (может быть пустым для
    /// анонимных блоков).
    pub tag_name: String,
    /// Border-box в координатах документа: включает padding + border, не включает margin.
    pub border_box: Rect,
    /// Margin-box в координатах документа: включает margin.
    pub margin_box: Rect,
}

/// ARIA state flags for an accessibility node, derived from `lumen-a11y::AXState`.
///
/// Each field mirrors the corresponding WAI-ARIA state or property.
/// All fields are `false` / `None` by default (not applicable or unset).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct A11yState {
    /// `aria-disabled="true"` or HTML `disabled` attribute.
    pub disabled: bool,
    /// `aria-checked` / HTML `checked`. `None` = not a checkable role.
    /// `Some(None)` = mixed/indeterminate. `Some(Some(b))` = checked/unchecked.
    pub checked: Option<Option<bool>>,
    /// `aria-expanded` — disclosure widget open/closed. `None` = not applicable.
    pub expanded: Option<bool>,
    /// `aria-hidden="true"` — node is invisible to assistive technology.
    pub hidden: bool,
    /// `aria-selected`. `None` = not applicable.
    pub selected: Option<bool>,
    /// `aria-pressed` — toggle button state. `None` = not a toggle.
    pub pressed: Option<bool>,
    /// `aria-required="true"` / HTML `required`.
    pub required: bool,
    /// `aria-readonly="true"` / HTML `readonly`.
    pub readonly: bool,
    /// `aria-invalid="true"`.
    pub invalid: bool,
    /// `aria-level` / implicit heading level for `<h1>`–`<h6>`.
    pub level: Option<u32>,
}

/// Узел accessibility-дерева из [`BrowserSession::a11y_tree`].
///
/// Построен из полного `lumen-a11y::AXNode` через `build_ax_tree()`.
/// Вложенные узлы — потомки в accessibility-дереве с учётом Shadow DOM
/// (slot-assigned flat tree).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A11yNode {
    /// DOM NodeId (u32) элемента, соответствующего этому узлу.
    pub node_id: u32,
    /// ARIA-роль: `"button"`, `"link"`, `"heading"`, … `"generic"` для
    /// контейнеров без явной роли.
    pub role: String,
    /// Вычисленное доступное имя (WAI-ARIA Accessible Name §4):
    /// `aria-label` → `aria-labelledby` → `alt` → текстовое содержимое → `title`.
    pub name: String,
    /// Вычисленное описание (`aria-describedby` / `title`).
    #[serde(default)]
    pub description: String,
    /// Placeholder-текст для текстовых полей (`placeholder` attr).
    #[serde(default)]
    pub placeholder: String,
    /// ARIA-состояния и свойства узла.
    #[serde(default)]
    pub state: A11yState,
    /// Дочерние узлы accessibility-дерева.
    pub children: Vec<A11yNode>,
}

/// Запись из сетевого лога [`BrowserSession::network_log`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEntry {
    /// URL запроса.
    pub url: String,
    /// HTTP-метод (`"GET"`, `"POST"`, …).
    pub method: String,
    /// HTTP-статус ответа (0 если запрос не завершён или ошибка сети).
    pub status: u16,
    /// Размер тела ответа в байтах.
    pub size_bytes: usize,
}

/// A network request paused by an active intercept, not yet reported to the
/// BiDi connection that registered it (WebDriver BiDi `network.beforeRequestSent`,
/// BUG-295 remainder). See [`BrowserSession::poll_intercepted_requests`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptedRequest {
    /// Opaque request identifier — the same id `network.continueRequest`/
    /// `network.failRequest` must reference to resolve this pause.
    pub request_id: String,
    /// URL of the paused request.
    pub url: String,
}

/// Запись из консоли [`BrowserSession::console_log`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleEntry {
    /// Уровень сообщения.
    pub level: ConsoleLevel,
    /// Текст сообщения.
    pub message: String,
}

/// Уровень console-сообщения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
}

/// Causal chain answer for [`BrowserSession::explain_element`] (DEVX-10, ADR-024
/// L1 `x-explain-element`): why an element did — or didn't — paint.
///
/// `есть в DOM → стили применились → попал в layout → размер → stacking-контекст
/// → команды → клип → слой` (`docs/tasks/p1-introspection-track.md`). Each stage
/// is only meaningful once the stage before it held, but every stage the tool
/// *could* determine is still filled in — the caller sees exactly where the
/// chain stops instead of a single boolean.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExplainElement {
    /// A DOM node matching the selector exists — checked at the DOM level,
    /// independent of layout (so `display: none` nodes still count).
    pub in_dom: bool,
    /// The CSS cascade ran for this node — a `LayoutBox` exists for it, even
    /// if `in_layout` is `false` (e.g. `display: none` still gets a box,
    /// tagged to be skipped, with its computed style attached).
    pub style_applied: bool,
    /// The node produced a real (non-skipped) layout box.
    pub in_layout: bool,
    /// Border-box `(width, height)` in CSS px, once `in_layout` is `true`.
    pub size: Option<(f32, f32)>,
    /// This box establishes its own CSS stacking context (CSS Positioned
    /// Layout L3 §9.10).
    pub creates_stacking_context: bool,
    /// Number of display-list commands attributed to this box's *own* paint
    /// (its `BoxOrigin`, i.e. not any anonymous wrapper or child box). `0` is
    /// common and legitimate — e.g. a container with no visible background
    /// or border paints nothing of its own.
    pub commands_emitted: usize,
    /// Maximum number of open rect/rounded-rect/path clips across this box's
    /// own paint span(s). `None` when the box produced no span at all —
    /// nothing to anchor a clip-depth reading to.
    pub clip_depth: Option<u16>,
    /// Compositor layer index this box's paint landed in. Currently always
    /// `Some(0)` when `in_layout` is `true` — the in-process compositor has a
    /// single layer (`BasicLayerTree::single_layer`) until multi-layer
    /// compositing lands.
    pub layer: Option<usize>,
    /// Best-effort, human-readable guess at why the element has no visible
    /// paint — **not derived from a diff and must not be read as fact**
    /// (ADR-024's requirement that heuristic output be labelled as such).
    /// `None` when nothing stood out, or when the element did paint.
    pub heuristic: Option<String>,
}

/// Page-level aggregate for [`BrowserSession::explain_page`] (DEVX-11, ADR-024
/// L1 `x-explain-page`): invariant-firing counts by category **plus**
/// telemetry — box counts, overflow, commands, clip depth, relayouts, timing.
///
/// **Design constraint (DEVX-11):** every field here is a machine-readable
/// counter meant for *comparison*, not for standalone human reading — "17
/// overflow elements" means nothing in isolation. Meaning shows up in two
/// modes: diffing two runs of the same page, or profiling across a corpus.
/// This is why `explain_page` and DEVX-13 (structural tree diff) are one
/// ratchet: a counter diff plus a structural diff.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExplainPage {
    /// Total number of `LayoutBox` nodes in the tree, inclusive of the root.
    pub box_count: usize,
    /// Boxes with `BoxRole::AnonymousBlock` or `BoxRole::AnonymousInlineRun`
    /// — synthesised wrappers with no element of their own. Deliberately
    /// excludes `BoxRole::Pseudo` (`::before`/`::after`/…): those are
    /// generated content tied to a real selector match, a different category
    /// from an anonymous flow-fixup wrapper.
    pub anonymous_box_count: usize,
    /// Boxes with `overflow-x` and/or `overflow-y` other than `visible`.
    pub overflow_element_count: usize,
    /// Total display-list commands for the page.
    pub command_count: usize,
    /// Maximum clip-stack depth across every provenance span in the page.
    /// `0` when the page produced no clips at all.
    pub max_clip_depth: u16,
    /// Number of full style+layout passes since the last navigation —
    /// `navigate()`'s initial layout counts as `1`. `None` when the session
    /// backing this query doesn't track the count ([`WinitSession`](crate::WinitSession),
    /// [`LiveWindowSession`](crate::LiveWindowSession) — a documented gap,
    /// same shape as [`ExplainElement::layer`] always being `Some(0)`).
    pub relayout_count: Option<u64>,
    /// DEVX-8a/8b invariant-firing counts by category — see
    /// [`InvariantViolationCounts`]. All-zero on every page this track has
    /// been validated against; a nonzero count here is a real engine bug,
    /// not a false positive (same "don't loosen the check" rule as the
    /// panicking invariants these counts mirror).
    pub invariant_violations: InvariantViolationCounts,
    /// Wall-clock cost of computing this `explain_page` snapshot itself, by
    /// phase — **not** the cost of the original style/layout/paint pass that
    /// built the tree being inspected (that data doesn't exist for an
    /// already-built tree, and re-running the full pipeline here would both
    /// falsify "read-only observability" and double real layout cost). Still
    /// meaningful for the same two comparison modes as every other field:
    /// diff against a previous `explain_page` call on the same page, or
    /// profile across a corpus, to spot an introspection-cost outlier.
    pub phase_ns: ExplainPagePhaseTimings,
}

/// DEVX-8a (`lumen_layout::invariants`) and DEVX-8b (`lumen_paint::invariants`)
/// violation counts, aggregated into [`ExplainPage`]. Narrower than the full
/// DEVX-8a/8b composition — see `docs/tasks/p1-introspection-track.md` §DEVX-11:
/// only the two invariant *modules* with an organized, independently testable
/// counting API are represented as categories here. DEVX-8a's other three
/// sub-checks (unresolved `var()` in `style.rs`, containing-block in
/// `lay_out_inner`, DOM-cycle guard in `lumen-dom`) and paint's `PropertyTrees`
/// reachability check remain pipeline-only `debug_assert!`s with no counting
/// variant — adding one for each would need touching hot-path code outside
/// this track's scope, the same kind of narrowing DEVX-8a/8b/10 each did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvariantViolationCounts {
    /// DEVX-8a: boxes with a non-finite rect or scroll offset.
    pub geometry_non_finite: usize,
    /// DEVX-8a: in-flow block children escaping their parent's border box
    /// horizontally without an overflow/positioning escape hatch.
    pub geometry_containment: usize,
    /// DEVX-8b: display-list commands not covered by exactly one provenance span.
    pub paint_coverage: usize,
    /// DEVX-8b: clip/scroll-layer push/pop imbalance.
    pub paint_clip_balance: usize,
    /// DEVX-8b: provenance spans whose origin node doesn't resolve.
    pub paint_origin_resolution: usize,
    /// DEVX-8b: boxes with visible background/border but no provenance span.
    pub paint_visible_missing_span: usize,
}

/// Per-phase timing breakdown for one [`ExplainPage`] call — see that
/// struct's doc comment for what "phase" means here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainPagePhaseTimings {
    /// Nanoseconds walking the layout tree to build the box/anonymous/overflow
    /// counters and run the DEVX-8a geometry-violation count.
    pub tree_walk_ns: u64,
    /// Nanoseconds building the display list + provenance index (needed for
    /// `command_count`, `max_clip_depth`, and the DEVX-8b paint-violation
    /// count) — the same call `explain_element` makes per-element, here made
    /// once for the whole page.
    pub display_list_build_ns: u64,
}

/// Значения вычисленных CSS-свойств элемента из [`BrowserSession::computed_style`].
///
/// Ключи — lowercase имена CSS-свойств (`"color"`, `"font-size"`, …),
/// значения — строковое представление вычисленного значения.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComputedProperties {
    /// Карта `property → value` для запрошенного элемента.
    pub properties: std::collections::HashMap<String, String>,
}

/// Команда для injection в event-loop браузера с целью создания нативных DOM-событий.
///
/// Используется для реализации [`BrowserSession::click`], [`BrowserSession::type_text`],
/// [`BrowserSession::scroll`] с иSтруsted = true в результирующих DOM-событиях (ADR-006 §8C).
///
/// # Архитектура
///
/// Injected события обрабатываются в WinitSessionHandler event loop точно так же,
/// как OS-события от winit — без обхода через JS `dispatchEvent()`.
#[derive(Debug, Clone)]
pub enum InputCommand {
    /// Клик мышью по координатам документа.
    ///
    /// Параметры: x, y в логических пикселях (document coordinates).
    /// Создаёт mousedown → mouseup → click события на целевом элементе с isTrusted=true.
    MouseClick { x: f32, y: f32 },

    /// Движение мышью на координаты.
    ///
    /// Параметры: x, y в логических пикселях (document coordinates).
    /// Создаёт mousemove событие с isTrusted=true.
    MouseMove { x: f32, y: f32 },

    /// Нажатие кнопки мышью.
    ///
    /// Параметры: x, y в логических пикселях; button (0=left, 1=middle, 2=right).
    MouseDown { x: f32, y: f32, button: u8 },

    /// Отпускание кнопки мышью.
    ///
    /// Параметры: x, y в логических пикселях; button (0=left, 1=middle, 2=right).
    MouseUp { x: f32, y: f32, button: u8 },

    /// Ввод одного символа с клавиатуры.
    ///
    /// Параметр: `char` для Unicode-символа (буквы, цифры, специальные);
    /// используется для посимвольного ввода в текстовые поля.
    /// Создаёт keydown → keypress → keyup → input события с isTrusted=true.
    KeyPress { char: char },

    /// Нажатие специальной клавиши (Backspace, Enter, Tab, etc.).
    ///
    /// Параметр: код клавиши (соответствует `winit::keyboard::KeyCode`);
    /// примеры: "Backspace", "Enter", "Tab", "ArrowDown".
    /// Создаёт keydown → keyup события с isTrusted=true.
    KeyDown { code: String },

    /// Отпускание специальной клавиши.
    ///
    /// Параметр: код клавиши (соответствует `winit::keyboard::KeyCode`).
    KeyUp { code: String },

    /// Скролл на величину в логических пикселях.
    ///
    /// Параметры: delta_x, delta_y (положительное — вправо/вниз).
    /// Обновляет позицию скролла и создаёт scroll событие с isTrusted=true.
    Scroll { delta_x: f32, delta_y: f32 },
}

/// Запрос к accessibility-дереву для [`BrowserSession::query_a11y`] и [`query_a11y_all`](BrowserSession::query_a11y_all).
///
/// Позволяет находить узлы accessibility-дерева по роли и имени (Playwright-стиль getByRole).
/// Роль сравнивается case-insensitive; имя проверяется case-insensitive substring-match.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AxQuery {
    /// Поиск по ARIA-роли и опциональному имени.
    ///
    /// # Примеры
    /// ```ignore
    /// AxQuery::Role { role: "button".to_string(), name: Some("Submit".to_string()) }
    /// AxQuery::Role { role: "link".to_string(), name: None }  // любое имя
    /// ```
    Role {
        /// ARIA-роль (case-insensitive): `"button"`, `"link"`, `"heading"`, etc.
        role: String,
        /// Опциональное имя или его часть (case-insensitive substring match).
        name: Option<String>,
    },
    /// Поиск по подстроке в accessible name (case-insensitive).
    NameContains(String),
}

/// Профиль отпечатка браузера (fingerprint profile) для BrowserSession.
///
/// Определяет уровень приватности и анти-детектирующие меры:
/// - TLS cipher suite ordering (соответствие Chrome, rustls defaults, или Tor Browser).
/// - HTTP header order и User-Agent.
/// - JS API returns (canvas randomization, WebGL strings, etc.) — реализуется в Phase 2.
///
/// По ADR-007 §6, профили распределены:
/// - **Standard** (default) — базовая приватность, выглядит как Chrome.
/// - **Strict** — высокая приватность, похожа на Firefox Strict / Tor Browser.
/// - **Tor** — Tor Browser fingerprint pinning (для будущей интеграции).
///
/// # Примеры
/// ```ignore
/// let mut session = InProcessSession::new();
/// session.set_fingerprint_profile(FingerprintProfile::Strict)?;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FingerprintProfile {
    /// Стандартный профиль (по умолчанию): выглядит как текущий Chrome, не вызывает внимание.
    Standard,
    /// Строгий профиль: высокая приватность, похожа на Firefox/Tor, может быть медленнее из-за дополнительных проверок.
    Strict,
    /// Tor Browser профиль: pinned JA3 + UA + screen + fonts (Phase 3+).
    Tor,
}

impl FingerprintProfile {
    /// Map this session-level profile to the network [`HttpProfile`] that drives
    /// HTTP/1.1 header order, Client Hints handling, and HTTP/2 SETTINGS.
    ///
    /// The TLS profile is derived from the HTTP profile by
    /// [`HttpClient::with_fingerprint_profile`], so callers only need this
    /// single mapping to apply the full fingerprint (task 9F.2):
    /// - `Standard` → `Chrome` (TLS Standard) — current stable Chrome.
    /// - `Strict` → `Strict` (TLS 1.3-only, Client Hints disabled).
    /// - `Tor` → `TorBrowser` (TLS Tor, no HTTP/2 ALPN).
    ///
    /// [`HttpProfile`]: lumen_network::HttpProfile
    /// [`HttpClient::with_fingerprint_profile`]: lumen_network::HttpClient::with_fingerprint_profile
    pub fn to_http_profile(self) -> lumen_network::HttpProfile {
        match self {
            FingerprintProfile::Standard => lumen_network::HttpProfile::Chrome,
            FingerprintProfile::Strict => lumen_network::HttpProfile::Strict,
            FingerprintProfile::Tor => lumen_network::HttpProfile::TorBrowser,
        }
    }
}

/// Подключить общий на процесс HSTS-store (RFC 6797) к клиенту драйвера.
///
/// Драйверные сессии строят `HttpClient` сами, мимо шелловского
/// `config::apply_http` (шелл лежит выше в графе зависимостей), поэтому точка
/// подключения нужна отдельная — но store тот же самый процесс-глобальный
/// объект, что и у шелла: обе половины браузера должны видеть одну
/// HSTS-политику, иначе один и тот же хост апгрейдится в навигации и не
/// апгрейдится в автоматизации ([BUG-402]).
///
/// `Tor`-профиль получает in-memory store — сессия не оставляет на диске
/// следов посещённых хостов; preload-лист работает в обоих режимах.
///
/// [BUG-402]: https://github.com/LearnJava/lumen-browser/blob/main/bugs/BUG-402-FIXED.md
#[must_use]
pub(crate) fn with_shared_hsts(
    client: lumen_network::HttpClient,
    profile: FingerprintProfile,
) -> lumen_network::HttpClient {
    let private = profile == FingerprintProfile::Tor;
    match lumen_storage::shared_hsts_store(private) {
        Some(hsts) => client.with_hsts(hsts),
        None => client,
    }
}

/// Command for automation API — sent to shell via IPC channel (SDC-1a).
///
/// This enum defines the contract between `lumen-driver` and `lumen-shell`.
/// Each variant represents one actionable command that the shell can execute
/// against its live window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AutomationCommand {
    /// Navigate to a URL.
    Navigate(String),
    /// Open a new tab (it becomes active) and navigate it to a URL.
    NewTab(String),
    /// Click at element center (resolved from Target).
    Click(Target),
    /// Type text into an input element.
    Type(Target, String),
    /// Scroll by delta in document coordinates.
    Scroll(ScrollDelta),
    /// Evaluate JavaScript in the active tab.
    Eval(String),
    /// Take a screenshot.
    Screenshot,
    /// Wait for condition.
    Wait(WaitCondition, u64),
    /// Find DOM nodes by CSS selector (SDC-2).
    Query(String),
    /// Snapshot the accessibility tree (SDC-2).
    A11yTree,
    /// Read captured JS console messages since the last `Navigate` (DEVX-1).
    ConsoleLog,
    /// Box-model snapshot of the whole page (DEVX-14, wires `resource://layout`
    /// to the live window).
    LayoutSnapshot,
    /// Network request log since the last `Navigate` (DEVX-14, wires
    /// `resource://network` to the live window).
    NetworkLog,
    /// Enable/disable offline-network simulation (BUG-295, WebDriver BiDi
    /// `network.setOfflineStatus`) on the live window.
    SetOffline(bool),
    /// Override `navigator.userAgent` and the real HTTP `User-Agent` header
    /// for the live window (BUG-295, WebDriver BiDi
    /// `emulation.setUserAgentOverride`). Empty string clears the override.
    SetUserAgent(String),
    /// Override the `Intl`/`Date` timezone reported by the live window
    /// (BUG-295, WebDriver BiDi `browser.setTimezoneOverride`). `None`
    /// clears the override (host timezone); `Some(id)` is an IANA timezone
    /// identifier (e.g. `"America/New_York"`).
    SetTimezone(Option<String>),
    /// Register a network intercept rule on the live window (BUG-295
    /// remainder, WebDriver BiDi `network.addIntercept`).
    AddIntercept {
        /// Opaque intercept identifier (BiDi `intercept`).
        id: String,
        /// Phases at which to intercept (only `"beforeRequestSent"` is
        /// actually paused on — see `lumen_network::intercept`).
        phases: Vec<String>,
        /// URL patterns to match (BiDi urlPattern `type: "string"`; empty = match-all).
        url_patterns: Vec<String>,
    },
    /// Remove a previously registered intercept rule (`network.removeIntercept`).
    RemoveIntercept(String),
    /// Deliver a decision for a paused request (BUG-295 remainder,
    /// `network.continueRequest`/`network.failRequest`). `continue_request =
    /// true` lets the request proceed; `false` fails it.
    ResolveIntercept {
        /// Opaque request identifier to resolve.
        request_id: String,
        /// `true` = continue, `false` = fail.
        continue_request: bool,
    },
    /// Poll requests newly paused by an active intercept since the last call
    /// (BUG-295 remainder, `network.beforeRequestSent` event data).
    PollIntercepts,
}

/// Reply from automation API — returned from shell after command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AutomationReply {
    /// Command acknowledged, no result.
    Ack,
    /// PNG screenshot bytes.
    Screenshot(Vec<u8>),
    /// Eval result as JSON string.
    Eval(String),
    /// Error message.
    Error(String),
    /// `Query` result: matching DOM nodes (SDC-2).
    Query(Vec<NodeRef>),
    /// `A11yTree` result: accessibility tree snapshot (SDC-2).
    A11yTree(Box<A11yNode>),
    /// `ConsoleLog` result: captured JS console messages (DEVX-1).
    ConsoleLog(Vec<ConsoleEntry>),
    /// `LayoutSnapshot` result: whole-page box-model snapshot (DEVX-14).
    LayoutSnapshot(Vec<BoxModel>),
    /// `NetworkLog` result: network request log (DEVX-14).
    NetworkLog(Vec<NetworkEntry>),
    /// `ResolveIntercept` result: whether `request_id` matched a pending pause.
    InterceptResolved(bool),
    /// `PollIntercepts` result: requests newly paused since the last poll.
    Intercepts(Vec<InterceptedRequest>),
}

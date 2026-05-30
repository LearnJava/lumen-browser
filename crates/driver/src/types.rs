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

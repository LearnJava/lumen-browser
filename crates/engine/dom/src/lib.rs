//! Arena-based DOM tree. Build via `Document::create_*` and `append_child`.
//!
//! # Invariant (10B / ADR-008)
//! The entire node graph lives in a **contiguous `Vec<Node>` arena** addressed by
//! `NodeId(u32)`. No `Rc<RefCell<…>>` exists in the graph — children and parents are
//! plain index values. This makes the tree `Send + Sync`, enables O(1) random access,
//! and guarantees that the snapshot serialised by [`Document::to_bytes`] is a flat
//! byte blob with no pointer fixups.

// Catch the most common forms of accidental Rc-in-arena.
#![deny(clippy::rc_buffer)]

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

pub use lumen_core::sandbox::{parse_sandbox_value, SandboxFlags};

pub mod contenteditable;
pub use contenteditable::{CommandHistory, DomCommand, DragData, PasteData, drop_into, paste_into};

/// Error returned by [`Document::to_bytes`] and [`Document::from_bytes`].
#[derive(Debug)]
pub enum DomSnapshotError {
    /// bincode encode failed.
    Encode(bincode::Error),
    /// bincode decode failed.
    Decode(bincode::Error),
}

impl fmt::Display for DomSnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "DOM snapshot encode error: {e}"),
            Self::Decode(e) => write!(f, "DOM snapshot decode error: {e}"),
        }
    }
}

impl std::error::Error for DomSnapshotError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(i: usize) -> Self {
        NodeId(i as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
    Xml,
    XmlNs,
    XLink,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualName {
    pub namespace: Namespace,
    pub local: String,
}

impl QualName {
    pub fn html(local: impl Into<String>) -> Self {
        Self {
            namespace: Namespace::Html,
            local: local.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub name: QualName,
    pub value: String,
}

/// Shadow root mode per Shadow DOM spec §4.2.
///
/// `Open` — JS can access the shadow root via `element.shadowRoot`.
/// `Closed` — `element.shadowRoot` returns `null` (encapsulated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowRootMode {
    Open,
    Closed,
}

impl fmt::Display for ShadowRootMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Closed => f.write_str("closed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeData {
    Document,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    /// Root of a shadow tree attached to a shadow host element.
    ///
    /// Not a regular DOM child — the host stores a pointer via
    /// `Document.shadow_roots`. Contains the shadow subtree as DOM children.
    /// Layout uses this through the composed (flat) tree; see `build_flat_tree`.
    ShadowRoot {
        mode: ShadowRootMode,
    },
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
    },
    Text(String),
    Comment(String),
    /// Inert subtree used as the content container for `<template>` elements.
    ///
    /// DOM Living Standard §4.5: a DocumentFragment has no parent and is not
    /// rendered directly. The `<template>` element stores its content here;
    /// callers clone the fragment into the live tree via `deep_clone`.
    ///
    /// Stored in the arena like any node. The mapping `template → fragment` is
    /// kept in `Document::template_contents`.
    DocumentFragment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

impl Node {
    pub fn element_name(&self) -> Option<&QualName> {
        match &self.data {
            NodeData::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Возвращает значение атрибута по имени (ASCII case-insensitive). На
    /// текстовых узлах и комментариях — `None`.
    pub fn get_attr(&self, name: &str) -> Option<&str> {
        match &self.data {
            NodeData::Element { attrs, .. } => attrs
                .iter()
                .find(|a| a.name.local.eq_ignore_ascii_case(name))
                .map(|a| a.value.as_str()),
            _ => None,
        }
    }

    /// Sandbox-ограничения для `<iframe sandbox="...">` по HTML LS §7.6.5.
    ///
    /// Возвращает `None` для всех не-`iframe` элементов. Для `<iframe>` без
    /// атрибута `sandbox` — `SandboxFlags::empty()` (без ограничений). Для
    /// `<iframe sandbox>` или `<iframe sandbox="">` — `SandboxFlags::all_restrictions()`.
    /// Конкретные `allow-*` keyword-ы снимают соответствующие биты.
    pub fn sandbox_flags(&self) -> Option<SandboxFlags> {
        let name = self.element_name()?;
        if !name.local.eq_ignore_ascii_case("iframe") {
            return None;
        }
        Some(parse_sandbox_value(self.get_attr("sandbox")))
    }

    /// HTML5 form input type для `<input type="...">`. Возвращает None
    /// для всех не-`input` элементов. Для `<input>` без явного `type` —
    /// `InputType::Text` (HTML5 default). Парсинг case-insensitive,
    /// неизвестные имена → `Other(String)` для forward-compat.
    pub fn input_type(&self) -> Option<InputType> {
        let name = self.element_name()?;
        if !name.local.eq_ignore_ascii_case("input") {
            return None;
        }
        let raw = self.get_attr("type").unwrap_or("text");
        Some(InputType::parse(raw))
    }
}

/// HTML5 form input types (HTML Standard §4.10.5). Спека определяет
/// 22 значения; Phase 0 кладёт все известные + `Other(String)` для
/// forward-compat. Тип `text` — default (если атрибут отсутствует или
/// не распознан); прочие неизвестные → `Other` (UI может render-ить как text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputType {
    /// `text` (default) — однострочное текстовое поле.
    Text,
    /// `password` — обфусцированный ввод.
    Password,
    /// `email` — формальная email-валидация.
    Email,
    /// `tel` — номер телефона (нет жёсткого формата).
    Tel,
    /// `url` — URL (формальная валидация).
    Url,
    /// `number` — численный ввод с stepper-ом.
    Number,
    /// `search` — текстовое поле с UI-варьированием (clear button).
    Search,
    /// `date` — date picker.
    Date,
    /// `datetime-local` — date+time picker.
    DateTimeLocal,
    /// `time` — time picker.
    Time,
    /// `month` — month/year picker.
    Month,
    /// `week` — week picker.
    Week,
    /// `color` — color picker.
    Color,
    /// `range` — slider.
    Range,
    /// `checkbox` — boolean checkbox.
    Checkbox,
    /// `radio` — radio button (один из группы по `name`).
    Radio,
    /// `file` — file upload.
    File,
    /// `submit` — submit button.
    Submit,
    /// `reset` — reset-form button.
    Reset,
    /// `button` — generic button (без submit-behavior).
    Button,
    /// `image` — submit button с изображением.
    Image,
    /// `hidden` — невидимое поле для server-side данных.
    Hidden,
    /// Forward-compat для не-описанных типов (или typo в HTML).
    Other(String),
}

impl InputType {
    /// Распарсить значение `type`-атрибута. Case-insensitive по
    /// HTML5 §4.10.5.1.4 «Attribute idioms».
    pub fn parse(s: &str) -> Self {
        let lc = s.trim().to_ascii_lowercase();
        match lc.as_str() {
            "text" | "" => Self::Text,
            "password" => Self::Password,
            "email" => Self::Email,
            "tel" => Self::Tel,
            "url" => Self::Url,
            "number" => Self::Number,
            "search" => Self::Search,
            "date" => Self::Date,
            "datetime-local" => Self::DateTimeLocal,
            "time" => Self::Time,
            "month" => Self::Month,
            "week" => Self::Week,
            "color" => Self::Color,
            "range" => Self::Range,
            "checkbox" => Self::Checkbox,
            "radio" => Self::Radio,
            "file" => Self::File,
            "submit" => Self::Submit,
            "reset" => Self::Reset,
            "button" => Self::Button,
            "image" => Self::Image,
            "hidden" => Self::Hidden,
            _ => Self::Other(lc),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Text => "text",
            Self::Password => "password",
            Self::Email => "email",
            Self::Tel => "tel",
            Self::Url => "url",
            Self::Number => "number",
            Self::Search => "search",
            Self::Date => "date",
            Self::DateTimeLocal => "datetime-local",
            Self::Time => "time",
            Self::Month => "month",
            Self::Week => "week",
            Self::Color => "color",
            Self::Range => "range",
            Self::Checkbox => "checkbox",
            Self::Radio => "radio",
            Self::File => "file",
            Self::Submit => "submit",
            Self::Reset => "reset",
            Self::Button => "button",
            Self::Image => "image",
            Self::Hidden => "hidden",
            Self::Other(s) => s.as_str(),
        }
    }

    /// Текстовая семантика — поле с буквенным контентом, на котором
    /// можно делать text selection, IME, и т.д. Включает text/password/
    /// email/tel/url/number/search.
    pub fn is_textual(&self) -> bool {
        matches!(
            self,
            Self::Text | Self::Password | Self::Email | Self::Tel
                | Self::Url | Self::Number | Self::Search
        )
    }

    /// Кнопочная семантика — submit/reset/button/image, рендерится
    /// как button.
    pub fn is_button_like(&self) -> bool {
        matches!(
            self,
            Self::Submit | Self::Reset | Self::Button | Self::Image
        )
    }
}

/// Данные `<form>` элемента — URL назначения, метод и число полей ввода.
#[derive(Debug, Clone)]
pub struct FormInfo {
    /// Значение атрибута `action` (пустая строка если отсутствует).
    pub action: String,
    /// Значение атрибута `method` в нижнем регистре, по умолчанию `"get"`.
    pub method: String,
    /// Число дочерних элементов-потомков типа input/select/textarea/button.
    pub field_count: usize,
}

/// Парсинг-режим документа по HTML5 §13.2.6.2 «The insertion mode».
///
/// Решается tree builder-ом по DOCTYPE-токену (см. §13.2.5.1
/// «The initial insertion mode»). На один Document приходится один режим
/// — он фиксируется в момент обработки первого DOCTYPE и больше не
/// меняется. Используется hot-path-ами layout/cascade для переключения
/// десятков legacy CSS-поведений (table sizing, body-background
/// propagation, font-size в `<table>`, и т.д.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DocumentMode {
    /// Standards / no-quirks mode — действуют современные правила.
    /// Сюда попадают `<!DOCTYPE html>` и большинство XHTML DOCTYPE.
    #[default]
    NoQuirks,
    /// Quirks mode — legacy-режим без DOCTYPE или с устаревшими
    /// PUBLIC IDs (HTML 2.0/3.x, HTML 4.x не-Strict без system_id).
    Quirks,
    /// Limited-quirks mode — узкий промежуточный режим для HTML 4.0/4.01
    /// Frameset / Transitional с правильным system_id и XHTML 1.0
    /// Frameset / Transitional. Большинство правил совпадает с
    /// no-quirks, но несколько (например, table cellpadding) — quirks.
    LimitedQuirks,
}

// ── Selection / Range ─────────────────────────────────────────────────────────

/// A position within the document (WHATWG DOM §4.4).
///
/// For `NodeData::Text` nodes `offset` is a UTF-8 byte offset within the
/// text content. For element nodes it is a child index. Use
/// [`Document::get_selection`] / [`Document::set_selection`] to persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomPosition {
    /// The node that contains this position.
    pub container: NodeId,
    /// Byte offset within the text content (text nodes) or child index
    /// (element nodes).
    pub offset: u32,
}

/// A contiguous range of document content (WHATWG DOM §4.5).
///
/// `start` must precede `end` in tree order. For a collapsed range
/// `start == end`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// First position (inclusive).
    pub start: DomPosition,
    /// Last position (exclusive).
    pub end: DomPosition,
}

impl Range {
    /// Collapsed range: both endpoints at `pos`.
    pub fn collapsed(pos: DomPosition) -> Self {
        Self { start: pos, end: pos }
    }

    /// True when start and end are the same position.
    pub fn is_collapsed(&self) -> bool {
        self.start == self.end
    }
}

/// The current document text selection (WHATWG Selection API).
///
/// Tracks anchor (mousedown) and focus (mousemove/mouseup). The selection
/// range is `min(anchor, focus)..=max(anchor, focus)` in document order.
///
/// `anchor` and `focus` are `None` when there is no active selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// Fixed start of the selection (where the user pressed the mouse button).
    pub anchor: Option<DomPosition>,
    /// Moving end of the selection (where the user released / dragged to).
    pub focus: Option<DomPosition>,
}

impl Selection {
    /// True when anchor == focus (or no selection).
    pub fn is_collapsed(&self) -> bool {
        match (&self.anchor, &self.focus) {
            (Some(a), Some(f)) => a == f,
            _ => true,
        }
    }

    /// The selection as a normalised Range (start ≤ end in node order).
    /// Returns `None` when there is no selection.
    pub fn get_range(&self) -> Option<Range> {
        let a = self.anchor?;
        let f = self.focus?;
        // Normalise so start is the position with the lower container index
        // or lower offset within the same container.
        if a.container.index() < f.container.index()
            || (a.container == f.container && a.offset <= f.offset)
        {
            Some(Range { start: a, end: f })
        } else {
            Some(Range { start: f, end: a })
        }
    }

    /// Collapse the selection to a single point.
    pub fn collapse(&mut self, pos: DomPosition) {
        self.anchor = Some(pos);
        self.focus = Some(pos);
    }

    /// Extend the focus end to `pos` (anchor stays fixed).
    pub fn extend_focus(&mut self, pos: DomPosition) {
        self.focus = Some(pos);
    }

    /// Remove the selection entirely.
    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
    }
}

/// Tracks the current IME composition session.
///
/// When an IME begins composing, this stores the target node and interim text
/// until the composition ends and the final text is committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionState {
    /// The editable element (contenteditable/input/textarea) receiving IME input.
    pub node: NodeId,
    /// The interim (preedit) composition text being edited by the IME.
    pub text: String,
    /// BCP 47 language tag (e.g., "ja", "zh-Hans"). `None` if not available.
    pub locale: Option<String>,
    /// Selection range in UTF-16 code units (start offset, length).
    /// Allows JS to highlight the composition range as the user types.
    pub selection: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
    mode: DocumentMode,
    target_id: Option<String>,
    /// Maps each shadow host `NodeId` to its shadow root `NodeId`.
    ///
    /// Shadow roots are stored in the arena like regular nodes but are not
    /// DOM children of the host. The flat tree (see `build_flat_tree`) uses
    /// this map to route layout traversal through shadow trees.
    shadow_roots: HashMap<NodeId, NodeId>,
    /// Maps each `<template>` element `NodeId` to its content `DocumentFragment` `NodeId`.
    ///
    /// The fragment is stored in the arena but is not a DOM child of the
    /// template element — `template.children` is always empty. Callers access
    /// template content via [`Document::template_content`].
    template_contents: HashMap<NodeId, NodeId>,
    /// The current text selection. Updated by the shell on mouse events;
    /// read by layout for `selection_rects` and by JS via `window.getSelection()`.
    selection: Selection,
    /// The active IME composition session (if any).
    /// Tracks preedit text and range while the user is composing via an IME.
    /// Cleared when composition ends.
    composition: Option<CompositionState>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let root_node = Node {
            parent: None,
            children: Vec::new(),
            data: NodeData::Document,
        };
        Self {
            nodes: vec![root_node],
            root: NodeId(0),
            mode: DocumentMode::default(),
            target_id: None,
            shadow_roots: HashMap::new(),
            template_contents: HashMap::new(),
            selection: Selection::default(),
            composition: None,
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Текущий парсинг-режим. Tree builder выставляет его при
    /// обработке DOCTYPE (или его отсутствии в конце потока) — для
    /// программно созданных документов и Document::new()-результата по
    /// умолчанию NoQuirks.
    pub fn mode(&self) -> DocumentMode {
        self.mode
    }

    /// Установить режим. Использует tree builder при инициализации
    /// документа — пользовательский код вызывает редко.
    pub fn set_mode(&mut self, mode: DocumentMode) {
        self.mode = mode;
    }

    /// Current selection. The shell updates this on mouse events; JS reads it
    /// via `window.getSelection()`.
    pub fn get_selection(&self) -> &Selection {
        &self.selection
    }

    /// Replace the current selection.
    pub fn set_selection(&mut self, sel: Selection) {
        self.selection = sel;
    }

    /// Clear the selection.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Текущий target — id из URL fragment (без ведущего `#`), к которому
    /// привязан `:target` pseudo-class (CSS Selectors L4 §9.6, HTML LS
    /// §7.10.6 «the indicated part of the document»). `None`, если URL без
    /// fragment-а либо fragment пустой / не указывает на существующий
    /// element с этим id. Сравнение `:target` matcher-а case-sensitive
    /// (HTML id attribute case-sensitive per HTML LS §3.2.6).
    ///
    /// Phase 0: значение здесь не выставляется автоматически — это shell-
    /// интеграция (P3): при загрузке URL парсить fragment и звать
    /// [`Document::set_target`] до style cascade, чтобы matcher имел
    /// корректное значение к моменту layout.
    pub fn target(&self) -> Option<&str> {
        self.target_id.as_deref()
    }

    /// Установить current target (id без `#`). `None` — нет fragment-а в URL.
    /// Caller отвечает за rerun style cascade: пересчёт `:target` matcher-а
    /// не вызывается отсюда.
    pub fn set_target<S: Into<String>>(&mut self, id: Option<S>) {
        self.target_id = id.map(Into::into).filter(|s| !s.is_empty());
    }

    /// Attach a shadow root to `host` and return its `NodeId`.
    ///
    /// The shadow root is allocated in the arena but is **not** a DOM child of
    /// `host`. Children appended to the shadow root form the shadow tree.
    /// Calling twice on the same host replaces the old shadow root (old root
    /// remains in the arena as an orphan — no automatic cleanup in Phase 0).
    ///
    /// Shadow DOM spec §4.2 «Attaching a shadow root».
    pub fn attach_shadow(&mut self, host: NodeId, mode: ShadowRootMode) -> NodeId {
        let sr = self.alloc(NodeData::ShadowRoot { mode });
        self.shadow_roots.insert(host, sr);
        sr
    }

    /// Return the shadow root attached to `host`, or `None` if not a shadow host.
    pub fn shadow_root_of(&self, host: NodeId) -> Option<NodeId> {
        self.shadow_roots.get(&host).copied()
    }

    /// Whether `id` is a shadow host (has an attached shadow root).
    pub fn is_shadow_host(&self, id: NodeId) -> bool {
        self.shadow_roots.contains_key(&id)
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// HTML5 §4.2.3 — найти первый `<base href="...">` в документе и
    /// вернуть значение атрибута `href`. Используется для resolve
    /// относительных URL (`<a>`, `<img>`, `<link>`, `<script>`). Если
    /// нет `<base>` или нет атрибута href — `None`.
    ///
    /// Поиск в pre-order обходе (depth-first, элементы по порядку
    /// исходного HTML). Имена тегов и атрибутов в HTML lowercase'нуты
    /// парсером.
    pub fn base_href(&self) -> Option<&str> {
        self.find_first_element(|node| {
            node.element_name()
                .map(|n| n.local == "base")
                .unwrap_or(false)
        })
        .and_then(|n| n.get_attr("href"))
    }

    /// Returns the `<body>` element's `NodeId`, walking root → `<html>` → `<body>`.
    /// Returns `None` for documents that have no `<html>` or no `<body>` child.
    pub fn body(&self) -> Option<NodeId> {
        let html = self.get(self.root).children.iter().copied().find(|&c| {
            matches!(&self.get(c).data, NodeData::Element { name, .. } if name.local == "html")
        })?;
        self.get(html).children.iter().copied().find(|&c| {
            matches!(&self.get(c).data, NodeData::Element { name, .. } if name.local == "body")
        })
    }

    /// Найти первый элемент, удовлетворяющий предикату. Pre-order обход
    /// от root. Используется для `base_href` и подобных «глобальных»
    /// HTML-помощников.
    pub fn find_first_element(&self, predicate: impl Fn(&Node) -> bool) -> Option<&Node> {
        let mut stack: Vec<NodeId> = vec![self.root];
        while let Some(id) = stack.pop() {
            let node = self.get(id);
            if matches!(node.data, NodeData::Element { .. }) && predicate(node) {
                return Some(node);
            }
            // Push children в обратном порядке, чтобы pop возвращал в
            // прямом source-order.
            for &child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        None
    }

    /// Find a node by its `id` attribute (case-sensitive, per HTML spec).
    ///
    /// Returns the `NodeId` of the first element with matching `id`, or `None` if not found.
    /// Used by accessibility tree and ARIA relationship attributes (aria-labelledby, aria-controls, etc.)
    /// to resolve references.
    pub fn find_by_id(&self, id: &str) -> Option<NodeId> {
        let mut stack: Vec<NodeId> = vec![self.root];
        while let Some(node_id) = stack.pop() {
            let node = self.get(node_id);
            if matches!(node.data, NodeData::Element { .. })
                && node.get_attr("id").is_some_and(|attr_id| attr_id == id)
            {
                return Some(node_id);
            }
            // Push children в обратном порядке для source-order traversal
            for &child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        None
    }

    fn alloc(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            parent: None,
            children: Vec::new(),
            data,
        });
        id
    }

    pub fn create_element(&mut self, name: QualName) -> NodeId {
        self.alloc(NodeData::Element {
            name,
            attrs: Vec::new(),
        })
    }

    pub fn create_text(&mut self, content: impl Into<String>) -> NodeId {
        self.alloc(NodeData::Text(content.into()))
    }

    pub fn create_comment(&mut self, content: impl Into<String>) -> NodeId {
        self.alloc(NodeData::Comment(content.into()))
    }

    /// Allocate a `DocumentFragment` node in the arena.
    ///
    /// Used by the tree builder to hold `<template>` content. The fragment is
    /// an inert container: it is never a DOM child of any node and is not
    /// rendered. Register it as a template's content via
    /// [`set_template_content`][Self::set_template_content].
    pub fn create_fragment(&mut self) -> NodeId {
        self.alloc(NodeData::DocumentFragment)
    }

    /// Register `fragment` as the content container for `template`.
    ///
    /// Overwrites any previous mapping. Caller must ensure `fragment` was
    /// created with [`create_fragment`][Self::create_fragment].
    pub fn set_template_content(&mut self, template: NodeId, fragment: NodeId) {
        self.template_contents.insert(template, fragment);
    }

    /// Return the content `DocumentFragment` for a `<template>` element, or
    /// `None` if `template` has no associated content (not a template element).
    pub fn template_content(&self, template: NodeId) -> Option<NodeId> {
        self.template_contents.get(&template).copied()
    }

    pub fn create_doctype(
        &mut self,
        name: impl Into<String>,
        public_id: impl Into<String>,
        system_id: impl Into<String>,
    ) -> NodeId {
        self.alloc(NodeData::Doctype {
            name: name.into(),
            public_id: public_id.into(),
            system_id: system_id.into(),
        })
    }

    /// Append `child` as the last child of `parent`. If `child` already has a parent, it is detached first.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        debug_assert!(parent != child, "cannot append a node to itself");
        self.detach(child);
        self.nodes[child.index()].parent = Some(parent);
        self.nodes[parent.index()].children.push(child);
    }

    /// Insert `new_node` immediately after `reference` in their shared parent.
    ///
    /// If `reference` has no parent, `new_node` is left without a parent (no-op
    /// other than detaching any previous parent of `new_node`). If `reference` is
    /// the last child, `new_node` is appended.
    pub fn insert_after(&mut self, reference: NodeId, new_node: NodeId) {
        self.detach(new_node);
        let parent = match self.nodes[reference.index()].parent {
            Some(p) => p,
            None => return,
        };
        let siblings = &mut self.nodes[parent.index()].children;
        let pos = siblings.iter().position(|&n| n == reference).unwrap_or(siblings.len() - 1);
        siblings.insert(pos + 1, new_node);
        self.nodes[new_node.index()].parent = Some(parent);
    }

    /// Remove `node` from its current parent. The node itself stays in the arena and can be re-attached.
    pub fn detach(&mut self, node: NodeId) {
        let parent = self.nodes[node.index()].parent.take();
        if let Some(parent) = parent {
            let siblings = &mut self.nodes[parent.index()].children;
            if let Some(pos) = siblings.iter().position(|&n| n == node) {
                siblings.remove(pos);
            }
        }
    }

    // ── IME Composition state management ──────────────────────────────────────

    /// Begin a new IME composition session in the given editable element.
    ///
    /// Overwrites any existing composition. Called by P3 when `compositionstart`
    /// is received from winit.
    ///
    /// # Arguments
    /// * `node` — the contenteditable/input/textarea receiving IME input
    /// * `text` — initial composition text (may be empty)
    /// * `locale` — BCP 47 language tag, or `None` if not available
    pub fn begin_composition(&mut self, node: NodeId, text: String, locale: Option<String>) {
        self.composition = Some(CompositionState {
            node,
            text,
            locale,
            selection: None,
        });
    }

    /// Update the active composition with new preedit text and selection range.
    ///
    /// Called by P3 when `compositionupdate` is received from winit.
    /// No-op if no composition is active.
    ///
    /// # Arguments
    /// * `text` — new interim composition text
    /// * `selection` — UTF-16 offset and length of the composition range shown to user
    pub fn update_composition(&mut self, text: String, selection: Option<(u32, u32)>) {
        if let Some(comp) = &mut self.composition {
            comp.text = text;
            comp.selection = selection;
        }
    }

    /// End the active composition and return its final state.
    ///
    /// Called by P3 when `compositionend` is received from winit. Returns the
    /// composition state so P3 can construct the final text event (with the
    /// committed text from the IME).
    ///
    /// Returns `None` if no composition is active.
    pub fn end_composition(&mut self) -> Option<CompositionState> {
        self.composition.take()
    }

    /// Get the current composition state without removing it.
    ///
    /// Used by layout and JS to inspect the active composition (e.g., for
    /// drawing selection underlines in the preedit range).
    ///
    /// Returns `None` if no composition is active.
    pub fn get_composition(&self) -> Option<&CompositionState> {
        self.composition.as_ref()
    }

    /// Check if an IME composition is currently active.
    ///
    /// Returns `true` if `begin_composition` has been called and
    /// `end_composition` has not yet been called.
    pub fn is_composing(&self) -> bool {
        self.composition.is_some()
    }

    /// Get the composition range (offset and length) if composition is active.
    ///
    /// Returns `None` if no composition is active or if the selection range
    /// has not been set yet.
    pub fn get_composition_range(&self) -> Option<(u32, u32)> {
        self.composition.as_ref().and_then(|comp| comp.selection)
    }

    /// Get the target node that is receiving composition input.
    ///
    /// Returns `None` if no composition is active. This is the element that
    /// should dispatch composition events (compositionstart/update/end).
    pub fn get_composition_target(&self) -> Option<NodeId> {
        self.composition.as_ref().map(|comp| comp.node)
    }

    // ── T3 hibernation snapshot (ADR-008) ─────────────────────────────────────

    /// Serialise the entire document to a compact binary blob (bincode).
    ///
    /// Used for **T3 hibernation**: when a tab is suspended, the shell calls
    /// `to_bytes()`, stores the blob on disk, and frees the in-memory tree.
    /// On restore the shell calls [`from_bytes`] to reconstruct the tree
    /// without re-parsing HTML. The blob is self-contained — no pointer fixups
    /// are needed because every node reference is a `NodeId(u32)` offset.
    pub fn to_bytes(&self) -> Result<Vec<u8>, DomSnapshotError> {
        bincode::serialize(self).map_err(DomSnapshotError::Encode)
    }

    /// Deserialise a document from a binary blob produced by [`to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DomSnapshotError> {
        bincode::deserialize(bytes).map_err(DomSnapshotError::Decode)
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_tree(self, self.root, 0, f)
    }
}

fn write_tree(doc: &Document, id: NodeId, depth: usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let node = doc.get(id);
    for _ in 0..depth {
        f.write_str("  ")?;
    }
    match &node.data {
        NodeData::Document => writeln!(f, "#document")?,
        NodeData::Doctype { name, .. } => writeln!(f, "<!DOCTYPE {name}>")?,
        NodeData::ShadowRoot { mode } => writeln!(f, "#shadow-root ({mode})")?,
        NodeData::DocumentFragment => writeln!(f, "#document-fragment")?,
        NodeData::Element { name, attrs } => {
            write!(f, "<{}", name.local)?;
            for a in attrs {
                write!(f, " {}=\"{}\"", a.name.local, a.value)?;
            }
            writeln!(f, ">")?;
        }
        NodeData::Text(s) => writeln!(f, "\"{}\"", s.replace('\n', "\\n"))?,
        NodeData::Comment(s) => writeln!(f, "<!--{s}-->")?,
    }
    for &child in &node.children {
        write_tree(doc, child, depth + 1, f)?;
    }
    // Shadow roots are not DOM children — print them after light-tree children.
    if let Some(sr) = doc.shadow_root_of(id) {
        write_tree(doc, sr, depth + 1, f)?;
    }
    // Template content fragments are not DOM children — print inline for debugging.
    if let Some(frag) = doc.template_content(id) {
        write_tree(doc, frag, depth + 1, f)?;
    }
    Ok(())
}

fn count_form_controls(doc: &Document, id: NodeId) -> usize {
    let mut count = 0;
    for &child in &doc.get(id).children.clone() {
        if doc
            .get(child)
            .element_name()
            .map(|n| {
                matches!(
                    n.local.to_ascii_lowercase().as_str(),
                    "input" | "select" | "textarea" | "button"
                )
            })
            .unwrap_or(false)
        {
            count += 1;
        }
        count += count_form_controls(doc, child);
    }
    count
}

fn collect_forms(doc: &Document, id: NodeId, out: &mut Vec<FormInfo>) {
    let node = doc.get(id);
    if node
        .element_name()
        .map(|n| n.local.eq_ignore_ascii_case("form"))
        .unwrap_or(false)
    {
        let action = node.get_attr("action").unwrap_or("").to_string();
        let method = node
            .get_attr("method")
            .unwrap_or("get")
            .to_ascii_lowercase();
        let field_count = count_form_controls(doc, id);
        out.push(FormInfo {
            action,
            method,
            field_count,
        });
        return;
    }
    for &child in &node.children.clone() {
        collect_forms(doc, child, out);
    }
}

/// Гейт отправки форм по sandbox-флагу HTML §7.6.5.
///
/// Если `sandbox` содержит [`SandboxFlags::FORMS`] — отправка заблокирована;
/// функция логирует число заблокированных форм и возвращает его.
/// Если флаг не установлен — возвращает 0. В Phase 0 реальной отправки
/// нет; вызов устанавливает инфраструктуру для будущего FormRuntime.
pub fn check_form_gate(doc: &Document, sandbox: SandboxFlags) -> usize {
    let mut forms = Vec::new();
    collect_forms(doc, doc.root(), &mut forms);
    if forms.is_empty() {
        return 0;
    }
    if sandbox.contains(SandboxFlags::FORMS) {
        eprintln!(
            "sandbox: заблокировано {} форм(ы) (sandbox=forms)",
            forms.len()
        );
        return forms.len();
    }
    0
}

/// Найти ближайший предок `<form>` для узла `node`.
///
/// Реализует шаг «find the form owner» из HTML LS §form-associated elements:
/// поднимаемся вверх по цепочке родителей до первого элемента с тегом `form`.
/// Возвращает `None` если узел не вложен ни в какую форму.
pub fn find_ancestor_form(doc: &Document, mut node: NodeId) -> Option<NodeId> {
    while let Some(parent) = doc.get(node).parent {
        if doc.get(parent).element_name()
            .map(|q| q.local.eq_ignore_ascii_case("form"))
            .unwrap_or(false)
        {
            return Some(parent);
        }
        node = parent;
    }
    None
}

/// Собрать имена и значения submittable-контролов формы из DOM-атрибутов.
///
/// Обходит потомков `form_id` depth-first и возвращает `(name, value)` для
/// каждого `<input>`, `<textarea>`, `<select>` у которых есть атрибут `name`
/// и который не является disabled. `<input type="submit">` и `<input type="reset">`
/// не включаются в набор данных (они не submittable в смысле HTML LS).
///
/// Значения берутся из DOM-атрибута `value`. Для актуальных runtime-значений
/// (что пользователь набрал) — вызывающий код в shell должен наложить
/// `FormState` поверх результата.
pub fn collect_dom_form_fields(doc: &Document, form_id: NodeId) -> Vec<(String, String)> {
    let mut out = Vec::new();
    collect_fields_in(doc, form_id, form_id, &mut out);
    out
}

fn collect_fields_in(doc: &Document, id: NodeId, form_id: NodeId, out: &mut Vec<(String, String)>) {
    let node = doc.get(id);
    let tag = node.element_name().map(|q| q.local.as_str()).unwrap_or("");
    match tag {
        "input" => {
            let itype = node
                .get_attr("type")
                .unwrap_or("text")
                .to_ascii_lowercase();
            // submit/reset/button/image не включаются в набор данных.
            if matches!(itype.as_str(), "submit" | "reset" | "button" | "image") {
                return;
            }
            if node.get_attr("disabled").is_some() {
                return;
            }
            if let Some(name) = node.get_attr("name").filter(|n| !n.is_empty()) {
                // checkbox и radio включаются только если checked.
                if matches!(itype.as_str(), "checkbox" | "radio") {
                    if node.get_attr("checked").is_none() {
                        return;
                    }
                    let value = node.get_attr("value").unwrap_or("on").to_string();
                    out.push((name.to_string(), value));
                } else {
                    let value = node.get_attr("value").unwrap_or("").to_string();
                    out.push((name.to_string(), value));
                }
            }
        }
        "textarea" => {
            if node.get_attr("disabled").is_some() {
                return;
            }
            if let Some(name) = node.get_attr("name").filter(|n| !n.is_empty()) {
                let value = node.get_attr("value").unwrap_or("").to_string();
                out.push((name.to_string(), value));
            }
        }
        "select" => {
            if node.get_attr("disabled").is_some() {
                return;
            }
            if let Some(name) = node.get_attr("name").filter(|n| !n.is_empty()) {
                // Ищем первый выбранный <option>; если нет — первый <option>.
                let selected = find_selected_option(doc, id);
                out.push((name.to_string(), selected));
            }
        }
        // Не рекурсируем внутрь вложенных форм (HTML LS не поддерживает
        // nested forms, но такие страницы встречаются).
        "form" if id != form_id => return,
        _ => {}
    }
    for &child in &node.children.clone() {
        collect_fields_in(doc, child, form_id, out);
    }
}

fn find_selected_option(doc: &Document, select_id: NodeId) -> String {
    let node = doc.get(select_id);
    let mut first_value = String::new();
    for &child in &node.children.clone() {
        let ch = doc.get(child);
        if ch.element_name().map(|q| q.local.eq_ignore_ascii_case("option")).unwrap_or(false) {
            let val = ch.get_attr("value")
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    ch.children.first().and_then(|&t| {
                        if let NodeData::Text(data) = &doc.get(t).data {
                            Some(data.clone())
                        } else {
                            None
                        }
                    }).unwrap_or_default()
                });
            if first_value.is_empty() {
                first_value = val.clone();
            }
            if ch.get_attr("selected").is_some() {
                return val;
            }
        }
    }
    first_value
}

// ──────────────────────────────────────────────────────────────────────────────
// HTML5 Constraint Validation API (§4.10.21)
// ──────────────────────────────────────────────────────────────────────────────

/// Validity state for a form control — HTML5 §4.10.21.1 `ValidityState` interface.
///
/// Phase 0: `pattern_mismatch`, `step_mismatch`, `bad_input`, `custom_error`
/// are always `false` (require runtime state or regex engine).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidityState {
    /// Required field with no value / unchecked checkbox.
    pub value_missing: bool,
    /// `type=email` or `type=url` with syntactically wrong value.
    pub type_mismatch: bool,
    /// `pattern` attribute not matched — Phase 0: always false.
    pub pattern_mismatch: bool,
    /// Value length exceeds `maxlength`.
    pub too_long: bool,
    /// Value length is less than `minlength`.
    pub too_short: bool,
    /// Numeric value is less than `min`.
    pub range_underflow: bool,
    /// Numeric value is greater than `max`.
    pub range_overflow: bool,
    /// Value doesn't match `step` — Phase 0: always false.
    pub step_mismatch: bool,
    /// User agent can't convert the input — Phase 0: always false.
    pub bad_input: bool,
    /// `setCustomValidity("")` was called with non-empty string — Phase 0: always false.
    pub custom_error: bool,
}

impl ValidityState {
    /// Returns `true` when all flags are `false` (element satisfies all constraints).
    pub fn valid(&self) -> bool {
        !self.value_missing
            && !self.type_mismatch
            && !self.pattern_mismatch
            && !self.too_long
            && !self.too_short
            && !self.range_underflow
            && !self.range_overflow
            && !self.step_mismatch
            && !self.bad_input
            && !self.custom_error
    }
}

/// Returns the validity state for `node`, or `None` if the node is not a
/// form-associated element subject to constraint validation (HTML5 §4.10.21.2).
///
/// "Barred" conditions (return `None`):
///   - Not an `<input>`, `<select>`, or `<textarea>`.
///   - `<input type="hidden|button|submit|reset|image">`.
///   - Any element with the `disabled` attribute.
pub fn element_validity(doc: &Document, node: NodeId) -> Option<ValidityState> {
    let node_ref = doc.get(node);
    let tag = node_ref.element_name()?.local.as_str().to_ascii_lowercase();
    let tag = tag.as_str();

    let t_lower;
    let (is_input, itype) = match tag {
        "input" => {
            t_lower = node_ref
                .get_attr("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "text".to_string());
            let t = t_lower.as_str();
            // Barred input types per HTML5 §4.10.21.2.
            if matches!(t, "hidden" | "button" | "submit" | "reset" | "image") {
                return None;
            }
            (true, t)
        }
        "select" | "textarea" => (false, tag),
        _ => return None,
    };

    if node_ref.get_attr("disabled").is_some() {
        return None;
    }

    let mut vs = ValidityState::default();

    // --- valueMissing (HTML5 §4.10.21.4.1) ---
    if node_ref.get_attr("required").is_some() {
        let missing = if is_input {
            match itype {
                "checkbox" | "radio" => node_ref.get_attr("checked").is_none(),
                _ => node_ref.get_attr("value").unwrap_or("").trim().is_empty(),
            }
        } else if tag == "textarea" {
            dom_text_content(doc, node).trim().is_empty()
        } else {
            // select: simplified — checks for non-empty selected value.
            node_ref.get_attr("value").unwrap_or("").trim().is_empty()
        };
        vs.value_missing = missing;
    }

    if is_input {
        let value = node_ref.get_attr("value").unwrap_or("");

        // --- typeMismatch (HTML5 §4.10.21.4.2) ---
        if !value.is_empty() {
            if itype == "email" {
                vs.type_mismatch = !is_valid_email_dom(value);
            } else if itype == "url" {
                vs.type_mismatch = !is_valid_url_dom(value);
            }
        }

        // --- rangeUnderflow / rangeOverflow (HTML5 §4.10.21.4.6-7) ---
        let supports_range = matches!(itype, "number" | "range" | "date" | "time");
        if supports_range {
            if let Some(val_num) = parse_html_float(value) {
                let min_num = node_ref.get_attr("min").and_then(parse_html_float);
                let max_num = node_ref.get_attr("max").and_then(parse_html_float);
                if let Some(min) = min_num {
                    vs.range_underflow = val_num < min;
                }
                if let Some(max) = max_num {
                    vs.range_overflow = val_num > max;
                }
            } else if itype == "range" {
                // range with no/invalid value uses default mid-point — never under/overflow.
            }
        }

        // --- tooLong (HTML5 §4.10.21.4.8) ---
        if let Some(max_len) = node_ref.get_attr("maxlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_long = value.chars().count() > max_len;
        }

        // --- tooShort (HTML5 §4.10.21.4.9): only when field has a value ---
        if let Some(min_len) = node_ref.get_attr("minlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_short = !value.is_empty() && value.chars().count() < min_len;
        }
    } else if tag == "textarea" {
        let value = dom_text_content(doc, node);
        if let Some(max_len) = node_ref.get_attr("maxlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_long = value.chars().count() > max_len;
        }
        if let Some(min_len) = node_ref.get_attr("minlength").and_then(|v| v.trim().parse::<usize>().ok()) {
            vs.too_short = !value.is_empty() && value.chars().count() < min_len;
        }
    }

    Some(vs)
}

/// Returns `true` if all submittable controls in `form_id` satisfy their
/// constraints (HTML5 §4.10.22.3 «statically validate the constraints»).
///
/// Returns `false` as soon as one invalid control is found.
/// All controls are barred controls — `check_validity_form` returns `true`
/// (vacuously valid — HTML5: «an element satisfies its constraints» when barred).
pub fn check_validity_form(doc: &Document, form_id: NodeId) -> bool {
    let mut all_valid = true;
    collect_validity_in(doc, form_id, form_id, &mut all_valid);
    all_valid
}

/// Returns the `NodeId`s of all invalid (failing constraint validation) controls
/// inside `form_id`, in DOM order.
pub fn invalid_controls_in_form(doc: &Document, form_id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    collect_invalid_in(doc, form_id, form_id, &mut out);
    out
}

fn collect_validity_in(doc: &Document, id: NodeId, form_id: NodeId, all_valid: &mut bool) {
    if !*all_valid {
        return; // early exit on first failure
    }
    let tag = doc.get(id).element_name().map(|q| q.local.as_str().to_ascii_lowercase()).unwrap_or_default();
    if matches!(tag.as_str(), "input" | "select" | "textarea")
        && element_validity(doc, id).is_some_and(|vs| !vs.valid())
    {
        *all_valid = false;
        return;
    }
    if tag == "form" && id != form_id {
        return; // don't cross into nested forms
    }
    for &child in &doc.get(id).children.clone() {
        collect_validity_in(doc, child, form_id, all_valid);
        if !*all_valid {
            return;
        }
    }
}

fn collect_invalid_in(doc: &Document, id: NodeId, form_id: NodeId, out: &mut Vec<NodeId>) {
    let tag = doc.get(id).element_name().map(|q| q.local.as_str().to_ascii_lowercase()).unwrap_or_default();
    if matches!(tag.as_str(), "input" | "select" | "textarea")
        && element_validity(doc, id).is_some_and(|vs| !vs.valid())
    {
        out.push(id);
    }
    if tag == "form" && id != form_id {
        return;
    }
    for &child in &doc.get(id).children.clone() {
        collect_invalid_in(doc, child, form_id, out);
    }
}

/// Collects all text content of an element (all Text descendants in DOM order).
fn dom_text_content(doc: &Document, node: NodeId) -> String {
    let mut out = String::new();
    dom_collect_text(doc, node, &mut out);
    out
}

fn dom_collect_text(doc: &Document, node: NodeId, out: &mut String) {
    for &child in &doc.get(node).children {
        match &doc.get(child).data {
            NodeData::Text(s) => out.push_str(s),
            NodeData::Element { .. } => dom_collect_text(doc, child, out),
            _ => {}
        }
    }
}

/// Parses an HTML5 valid floating-point number (§2.5.5).
/// Rejects leading `+`, NaN, and ±∞.
fn parse_html_float(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s.starts_with('+') {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    if v.is_finite() { Some(v) } else { None }
}

/// Basic email syntax check (HTML5 §4.10.5.1.5 «valid e-mail address»).
/// Phase 0: non-empty local-part + `@` + domain with at least one `.`.
fn is_valid_email_dom(value: &str) -> bool {
    let value = value.trim();
    let Some(at_pos) = value.rfind('@') else { return false; };
    let local = &value[..at_pos];
    let domain = &value[at_pos + 1..];
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    let parts: Vec<&str> = domain.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty())
}

/// Basic URL syntax check (HTML5 §4.10.5.1.15 «valid URL»).
/// Phase 0: presence of `<scheme>://` or known schemeless URIs.
fn is_valid_url_dom(value: &str) -> bool {
    let value = value.trim();
    if let Some(pos) = value.find("://") {
        let scheme = &value[..pos];
        return !scheme.is_empty()
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    }
    if let Some(pos) = value.find(':') {
        let scheme = &value[..pos];
        if matches!(scheme, "data" | "mailto" | "tel") {
            return !value[pos + 1..].is_empty();
        }
    }
    false
}

/// Информация об якорной ссылке (`<a href>`), найденной в документе.
pub struct AnchorInfo {
    /// Значение атрибута `href`.
    pub href: String,
}

fn collect_anchors(doc: &Document, id: NodeId, out: &mut Vec<AnchorInfo>) {
    let node = doc.get(id);
    if node
        .element_name()
        .map(|n| n.local.eq_ignore_ascii_case("a"))
        .unwrap_or(false)
        && let Some(href) = node.get_attr("href").filter(|h| !h.is_empty())
    {
        out.push(AnchorInfo {
            href: href.to_string(),
        });
    }
    for &child in &node.children.clone() {
        collect_anchors(doc, child, out);
    }
}

// ──────── Shadow DOM: composed (flat) tree ────────

/// Pre-computed composed tree (flat tree) for Shadow DOM layout traversal.
///
/// Shadow DOM spec §8.2: the flat tree replaces the DOM tree for rendering.
/// Shadow hosts are replaced by their shadow subtrees and `<slot>` elements
/// are replaced by their assigned light-tree nodes.
///
/// For documents without Shadow DOM `overrides` is empty, so every lookup
/// falls through to DOM children — zero allocation overhead.
#[derive(Debug, Default)]
pub struct FlatTree {
    /// Nodes whose composed-tree children differ from their DOM children.
    overrides: HashMap<NodeId, Vec<NodeId>>,
}

impl FlatTree {
    /// Composed-tree children of `id`.
    ///
    /// Returns DOM children when no shadow override exists (fast path for
    /// ordinary elements in non-shadow documents).
    pub fn children_of<'a>(&'a self, doc: &'a Document, id: NodeId) -> &'a [NodeId] {
        self.overrides
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_else(|| doc.get(id).children.as_slice())
    }
}

/// Build the composed (flat) tree for the document.
///
/// Shadow DOM spec §8.2. Layout calls this once before `build_box` so that
/// the tree traversal follows shadow boundaries without per-node branching.
///
/// Fast path: if the document has no shadow hosts, returns an empty `FlatTree`
/// (every `children_of` call falls through to DOM children).
pub fn build_flat_tree(doc: &Document) -> FlatTree {
    if doc.shadow_roots.is_empty() {
        return FlatTree::default();
    }

    let mut overrides: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for i in 0..doc.len() {
        let id = NodeId::from_index(i);
        if !doc.is_shadow_host(id) {
            continue;
        }
        let sr = doc.shadow_root_of(id).expect("shadow host has no root");

        // Shadow host's composed children = shadow root's DOM children.
        overrides.insert(id, doc.get(sr).children.clone());

        // Distribute light-tree children into matching <slot> elements.
        let slot_map = compute_slot_assignments(doc, id, sr);
        wire_slot_overrides(doc, sr, &slot_map, &mut overrides);
    }

    FlatTree { overrides }
}

/// Maps each `<slot>` NodeId to its assigned light-tree nodes.
type SlotAssignments = HashMap<NodeId, Vec<NodeId>>;

/// Compute slot assignments for `host`'s shadow tree rooted at `sr`.
///
/// Each light-tree child of `host` whose `slot=""` attribute matches a
/// `<slot name="">` in the shadow tree is assigned to that slot. Unmatched
/// children are dropped (they don't appear in the flat tree).
fn compute_slot_assignments(doc: &Document, host: NodeId, sr: NodeId) -> SlotAssignments {
    let mut slots: Vec<(NodeId, String)> = Vec::new();
    collect_slots(doc, sr, &mut slots);

    let mut map: SlotAssignments = HashMap::new();
    for &(slot_id, _) in &slots {
        map.insert(slot_id, Vec::new());
    }

    for &child in &doc.get(host).children {
        let wanted = doc.get(child).get_attr("slot").unwrap_or("").to_string();
        if let Some(&(slot_id, _)) = slots.iter().find(|(_, name)| *name == wanted) {
            map.get_mut(&slot_id).expect("slot in map").push(child);
        }
        // Children with no matching slot are not rendered in the flat tree.
    }

    map
}

fn collect_slots(doc: &Document, id: NodeId, out: &mut Vec<(NodeId, String)>) {
    if let NodeData::Element { name, .. } = &doc.get(id).data
        && name.local == "slot"
    {
        let slot_name = doc.get(id).get_attr("name").unwrap_or("").to_string();
        out.push((id, slot_name));
    }
    for &child in &doc.get(id).children {
        collect_slots(doc, child, out);
    }
}

/// Override each `<slot>` in the shadow tree with its assigned light-tree nodes.
///
/// A slot with assigned nodes gets an override (composed children = assigned).
/// A slot with no assigned nodes keeps its DOM children as fallback content.
fn wire_slot_overrides(
    doc: &Document,
    id: NodeId,
    slot_map: &SlotAssignments,
    overrides: &mut HashMap<NodeId, Vec<NodeId>>,
) {
    if let NodeData::Element { name, .. } = &doc.get(id).data
        && name.local == "slot"
        && let Some(assigned) = slot_map.get(&id)
        && !assigned.is_empty()
    {
        overrides.insert(id, assigned.clone());
        // Empty assignment → no override; slot's DOM children are the fallback.
    }
    for &child in &doc.get(id).children {
        wire_slot_overrides(doc, child, slot_map, overrides);
    }
}

/// Гейт навигации по sandbox-флагу HTML §7.6.5.
///
/// Если `sandbox` содержит [`SandboxFlags::NAVIGATION`] — навигация
/// из sandboxed-документа заблокирована; функция логирует число
/// заблокированных ссылок и возвращает его.
/// Если флаг не установлен — возвращает 0. В Phase 0 реальной навигации
/// нет; вызов устанавливает инфраструктуру для будущего NavigationRuntime.
pub fn check_navigation_gate(doc: &Document, sandbox: SandboxFlags) -> usize {
    let mut anchors = Vec::new();
    collect_anchors(doc, doc.root(), &mut anchors);
    if anchors.is_empty() {
        return 0;
    }
    if sandbox.contains(SandboxFlags::NAVIGATION) {
        eprintln!(
            "sandbox: заблокировано {} ссылок(и) (sandbox=navigation)",
            anchors.len()
        );
        return anchors.len();
    }
    0
}

// ──────────────────────────────────────────────────────────────────────────────
// iframe sandbox
// ──────────────────────────────────────────────────────────────────────────────

/// Данные `<iframe>` элемента — URL содержимого и sandbox-ограничения.
///
/// `is_sandboxed` — `true` если у элемента есть атрибут `sandbox` (даже пустой).
/// `sandbox` содержит распарсенные флаги (пустые = нет ограничений, все = максимум).
pub struct IframeInfo {
    /// Значение атрибута `src`, если задан.
    pub src: Option<String>,
    /// Sandbox-флаги согласно HTML §7.6.5. `SandboxFlags::empty()` если атрибута нет.
    pub sandbox: SandboxFlags,
    /// `true` если у элемента есть атрибут `sandbox` (независимо от значения).
    pub is_sandboxed: bool,
}

fn collect_iframes_inner(doc: &Document, id: NodeId, out: &mut Vec<IframeInfo>) {
    let node = doc.get(id);
    if node
        .element_name()
        .map(|n| n.local.eq_ignore_ascii_case("iframe"))
        .unwrap_or(false)
    {
        let src = node.get_attr("src").filter(|s| !s.is_empty()).map(str::to_owned);
        let is_sandboxed = node.get_attr("sandbox").is_some();
        let sandbox = node.sandbox_flags().unwrap_or_else(SandboxFlags::empty);
        out.push(IframeInfo { src, sandbox, is_sandboxed });
    }
    for &child in &node.children.clone() {
        collect_iframes_inner(doc, child, out);
    }
}

/// Собрать все `<iframe>` элементы документа с их sandbox-ограничениями.
///
/// Каждый `<iframe>` — один `IframeInfo`. Элементы без атрибута `sandbox`
/// включаются с `is_sandboxed = false` и `sandbox = SandboxFlags::empty()`.
/// Порядок — depth-first обход дерева.
pub fn collect_iframes(doc: &Document) -> Vec<IframeInfo> {
    let mut out = Vec::new();
    collect_iframes_inner(doc, doc.root(), &mut out);
    out
}

/// Гейт открытия popup-ов (`window.open()`, `target="_blank"`) по sandbox HTML §7.6.5.
///
/// Возвращает `true` если `sandbox` содержит [`SandboxFlags::AUXILIARY_NAVIGATION`]
/// (т.е. `allow-popups` не указан) — popup запрещён.
/// `false` — popup разрешён (флаг снят или sandbox не активен).
pub fn check_popup_gate(sandbox: SandboxFlags) -> bool {
    if sandbox.contains(SandboxFlags::AUXILIARY_NAVIGATION) {
        eprintln!("sandbox: заблокирован popup (sandbox=auxiliary-navigation, нет allow-popups)");
        return true;
    }
    false
}

// ──────────────────────────────────────────────────────────────────────────────
// contenteditable: Input Events Level 2 (P1 part)
// ──────────────────────────────────────────────────────────────────────────────

/// Input event type per Input Events Level 2 §4.1.3.
///
/// Each variant corresponds to one `inputType` string value that is dispatched
/// with `beforeinput`/`input` events on a `contenteditable` host. P3 maps
/// keyboard events to these variants; P1 uses them only as data types for the
/// DOM mutation API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditInputType {
    /// A printable character was typed (regular key press in contenteditable).
    InsertText,
    /// Enter key — split the current block into two paragraphs.
    InsertParagraph,
    /// Shift+Enter — insert a `<br>` line break without splitting a block.
    InsertLineBreak,
    /// Backspace — delete one character/grapheme cluster before the caret.
    DeleteContentBackward,
    /// Delete — delete one character/grapheme cluster after the caret.
    DeleteContentForward,
    /// Ctrl+Backspace — delete one word before the caret.
    DeleteWordBackward,
    /// Ctrl+Delete — delete one word after the caret.
    DeleteWordForward,
    /// Ctrl+V / drag-and-drop — insert content from the clipboard.
    InsertFromPaste,
    /// Ctrl+X — delete selected content and copy to clipboard.
    DeleteByCut,
    /// Ctrl+A — select all content (no DOM mutation).
    SelectAll,
    /// Ctrl+Z — undo the previous edit (managed by P3 undo stack).
    HistoryUndo,
    /// Ctrl+Y / Ctrl+Shift+Z — redo (managed by P3 undo stack).
    HistoryRedo,
}

impl EditInputType {
    /// The canonical `inputType` string for the `InputEvent` interface.
    ///
    /// Values match the Input Events Level 2 §4.1.3 enumeration.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InsertText => "insertText",
            Self::InsertParagraph => "insertParagraph",
            Self::InsertLineBreak => "insertLineBreak",
            Self::DeleteContentBackward => "deleteContentBackward",
            Self::DeleteContentForward => "deleteContentForward",
            Self::DeleteWordBackward => "deleteWordBackward",
            Self::DeleteWordForward => "deleteWordForward",
            Self::InsertFromPaste => "insertFromPaste",
            Self::DeleteByCut => "deleteByCut",
            Self::SelectAll => "selectAll",
            Self::HistoryUndo => "historyUndo",
            Self::HistoryRedo => "historyRedo",
        }
    }
}

/// Data for a `beforeinput` or `input` DOM event (Input Events Level 2 §4.1).
///
/// P3 constructs this from keyboard / clipboard events and dispatches it to the
/// JS runtime and the mutation functions below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEvent {
    /// The kind of edit operation that caused this event.
    pub input_type: EditInputType,
    /// Text that was inserted, or `None` for deletion/selection operations.
    pub data: Option<String>,
    /// `true` while an IME composition is in progress (not yet committed).
    pub is_composing: bool,
}

// ──────────────────────────────────────────────────────────────────────────────
// IME Composition Events (UI Events §5.2.5 — for CJK/Cyrillic input)
// ──────────────────────────────────────────────────────────────────────────────

/// Type of IME composition event (UI Events §5.2.5).
///
/// While an IME is composing (e.g., Japanese input), the sequence is:
/// 1. `compositionstart` — IME began capturing input
/// 2. Zero or more `compositionupdate` — interim composition text shown to user
/// 3. `compositionend` — IME committed the final text (may differ from last update)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositionEventType {
    /// IME composition began.
    Start,
    /// Interim composition text changed.
    Update,
    /// IME composition finished and text was committed.
    End,
}

impl CompositionEventType {
    /// The canonical DOM event name per UI Events §5.2.5.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "compositionstart",
            Self::Update => "compositionupdate",
            Self::End => "compositionend",
        }
    }
}

/// Data for a `compositionstart` / `compositionupdate` / `compositionend` event.
///
/// P3 constructs this from winit IME callbacks (Ime::Preedit, Ime::Commit) and
/// dispatches to the JS runtime. P1 maintains composition state and ranges.
#[derive(Debug, Clone)]
pub struct CompositionData {
    /// The interim (preedit) or final (committed) composition text.
    /// Empty string for events where only metadata changed.
    pub data: String,
    /// BCP 47 language tag (e.g., "ja", "zh-Hans", "ru"). `None` if unknown.
    pub locale: Option<String>,
    /// Composition range in the contenteditable host (UTF-16 code units).
    /// `None` if the range cannot be determined.
    /// `(offset, length)` where offset is the start position and length is the number
    /// of characters in the composition range.
    pub range: Option<(u32, u32)>,
}

/// An IME composition event (compositionstart / update / end).
///
/// Dispatched by P3 from winit IME callbacks. The DOM tracks composition state
/// per node for virtual keyboard interaction and text alternatives.
#[derive(Debug, Clone)]
pub struct CompositionEvent {
    /// The kind of composition event.
    pub event_type: CompositionEventType,
    /// Composition data (text, language, selection range).
    pub data: CompositionData,
}

impl CompositionEvent {
    /// Create a new composition event.
    pub fn new(event_type: CompositionEventType, data: CompositionData) -> Self {
        Self { event_type, data }
    }

    /// Create a `compositionstart` event with initial IME text.
    pub fn start(data: String, locale: Option<String>) -> Self {
        Self {
            event_type: CompositionEventType::Start,
            data: CompositionData {
                data,
                locale,
                range: None,
            },
        }
    }

    /// Create a `compositionupdate` event for interim preedit text.
    pub fn update(data: String, range: Option<(u32, u32)>) -> Self {
        Self {
            event_type: CompositionEventType::Update,
            data: CompositionData {
                data,
                locale: None,
                range,
            },
        }
    }

    /// Create a `compositionend` event for final committed text.
    pub fn end(data: String) -> Self {
        Self {
            event_type: CompositionEventType::End,
            data: CompositionData {
                data,
                locale: None,
                range: None,
            },
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// DOM mutation helpers for contenteditable editing
// ──────────────────────────────────────────────────────────────────────────────

/// Split a text node at `byte_offset`, creating a second text node with the
/// suffix `[byte_offset..]` and inserting it immediately after the original.
///
/// Returns the `NodeId` of the newly created second node.
/// If `byte_offset == 0` the first node becomes empty and all content moves to
/// the second. If `byte_offset >= content.len()` the first node is unchanged
/// and the second node is empty.
///
/// The caller is responsible for ensuring that `byte_offset` falls on a UTF-8
/// character boundary; if not, the offset is rounded down to the nearest
/// boundary to avoid producing invalid UTF-8.
pub fn split_text_node(doc: &mut Document, node: NodeId, byte_offset: u32) -> NodeId {
    let content = match &doc.get(node).data {
        NodeData::Text(s) => s.clone(),
        _ => return node, // not a text node — no-op, return self
    };

    // Clamp to a valid UTF-8 boundary.
    let offset = byte_offset as usize;
    let offset = if offset >= content.len() {
        content.len()
    } else {
        // Walk back to a char boundary.
        let mut b = offset;
        while b > 0 && !content.is_char_boundary(b) {
            b -= 1;
        }
        b
    };

    let first_part = content[..offset].to_string();
    let second_part = content[offset..].to_string();

    // Mutate the first (original) node in-place.
    if let NodeData::Text(s) = &mut doc.get_mut(node).data {
        *s = first_part;
    }

    // Allocate the second node and wire it into the parent.
    let second = doc.create_text(second_part);
    doc.insert_after(node, second);
    second
}

/// Insert `text` into the text node at `pos`, returning the caret position
/// immediately after the inserted text.
///
/// `pos.container` must point to a `NodeData::Text` node. If it points to an
/// element instead, the function tries to use the first text-node child; if
/// none exists it creates one and appends it.
///
/// `pos.offset` is a UTF-8 byte offset within the text content. If it exceeds
/// the content length it is clamped to the end.
pub fn insert_text_at(doc: &mut Document, pos: DomPosition, text: &str) -> DomPosition {
    if text.is_empty() {
        return pos;
    }

    // Resolve container to a text node.
    let text_node = match &doc.get(pos.container).data {
        NodeData::Text(_) => pos.container,
        NodeData::Element { .. } | NodeData::DocumentFragment => {
            // Find existing first text child or create one.
            let first_text = doc.get(pos.container).children.iter().copied().find(|&c| {
                matches!(doc.get(c).data, NodeData::Text(_))
            });
            match first_text {
                Some(id) => id,
                None => {
                    let new_text = doc.create_text("");
                    doc.append_child(pos.container, new_text);
                    new_text
                }
            }
        }
        _ => return pos,
    };

    let content = match &doc.get(text_node).data {
        NodeData::Text(s) => s.clone(),
        _ => return pos,
    };

    let offset = pos.offset as usize;
    let offset = offset.min(content.len());
    // Snap to UTF-8 boundary.
    let mut byte_off = offset;
    while byte_off > 0 && !content.is_char_boundary(byte_off) {
        byte_off -= 1;
    }

    let mut new_content = String::with_capacity(content.len() + text.len());
    new_content.push_str(&content[..byte_off]);
    new_content.push_str(text);
    new_content.push_str(&content[byte_off..]);

    let new_offset = (byte_off + text.len()) as u32;
    if let NodeData::Text(s) = &mut doc.get_mut(text_node).data {
        *s = new_content;
    }

    DomPosition { container: text_node, offset: new_offset }
}

/// Delete the content of `range` from the document, returning a collapsed
/// `DomPosition` at the start of the deleted range.
///
/// Only same-container deletions are supported (both endpoints in the same
/// text node). If `range.is_collapsed()` the function is a no-op.
/// Cross-node ranges are not yet implemented and return the start position
/// unchanged.
pub fn delete_range(doc: &mut Document, range: &Range) -> DomPosition {
    if range.is_collapsed() {
        return range.start;
    }

    // Only handle same-container for now.
    if range.start.container != range.end.container {
        return range.start;
    }

    let container = range.start.container;
    let content = match &doc.get(container).data {
        NodeData::Text(s) => s.clone(),
        _ => return range.start,
    };

    let start = (range.start.offset as usize).min(content.len());
    let end = (range.end.offset as usize).min(content.len());
    let (start, end) = if start <= end { (start, end) } else { (end, start) };

    // Snap both offsets to UTF-8 boundaries.
    let mut s = start;
    while s > 0 && !content.is_char_boundary(s) {
        s -= 1;
    }
    let mut e = end;
    while e > 0 && !content.is_char_boundary(e) {
        e -= 1;
    }

    let mut new_content = String::with_capacity(content.len() - (e - s));
    new_content.push_str(&content[..s]);
    new_content.push_str(&content[e..]);

    if let NodeData::Text(c) = &mut doc.get_mut(container).data {
        *c = new_content;
    }

    DomPosition { container, offset: s as u32 }
}

/// Insert a paragraph break (Enter key) at `pos` inside the `host`
/// contenteditable element.
///
/// Splits the text node at `pos` and inserts a `<br>` element immediately after
/// the split point. Returns a `DomPosition` at the start of the content after
/// the break (i.e. offset 0 into the second part of the split text node).
///
/// If `pos.container` is not a text node, a `<br>` is appended to `host`
/// directly and the position returned points to an empty text node after it.
///
/// `host` — the `contenteditable` root element (used as the insertion
/// container when `pos.container` has no parent or is not a text node).
// CSS: line-height, block formatting context for <p> splitting
pub fn insert_paragraph_break(doc: &mut Document, pos: DomPosition, host: NodeId) -> DomPosition {
    let is_text = matches!(doc.get(pos.container).data, NodeData::Text(_));

    if is_text {
        // Split text node at pos.
        let second = split_text_node(doc, pos.container, pos.offset);

        // Insert <br> between the two halves.
        let br = doc.create_element(QualName::html("br"));
        doc.insert_after(pos.container, br);
        // Move second text node after <br>.
        doc.insert_after(br, second);

        DomPosition { container: second, offset: 0 }
    } else {
        // Fallback: just append a <br> and an empty text node to host.
        let br = doc.create_element(QualName::html("br"));
        doc.append_child(host, br);
        let empty = doc.create_text("");
        doc.append_child(host, empty);
        DomPosition { container: empty, offset: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_document_has_root() {
        let doc = Document::new();
        assert_eq!(doc.len(), 1);
        assert!(matches!(doc.get(doc.root()).data, NodeData::Document));
    }

    #[test]
    fn build_simple_tree() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let h1 = doc.create_element(QualName::html("h1"));
        let text = doc.create_text("Hello");

        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, h1);
        doc.append_child(h1, text);

        assert_eq!(doc.len(), 5);
        assert_eq!(doc.get(html).children, vec![body]);
        assert_eq!(doc.get(body).children, vec![h1]);
        assert_eq!(doc.get(h1).children, vec![text]);
        assert_eq!(doc.get(text).parent, Some(h1));
    }

    #[test]
    fn detach_removes_from_parent_but_keeps_node() {
        let mut doc = Document::new();
        let a = doc.create_element(QualName::html("a"));
        let b = doc.create_element(QualName::html("b"));
        doc.append_child(doc.root(), a);
        doc.append_child(a, b);

        doc.detach(b);

        assert!(doc.get(a).children.is_empty());
        assert_eq!(doc.get(b).parent, None);
        assert_eq!(doc.len(), 3);
    }

    #[test]
    fn append_moves_existing_node() {
        let mut doc = Document::new();
        let a = doc.create_element(QualName::html("a"));
        let b = doc.create_element(QualName::html("b"));
        let c = doc.create_element(QualName::html("c"));
        doc.append_child(doc.root(), a);
        doc.append_child(doc.root(), b);
        doc.append_child(a, c);

        doc.append_child(b, c);

        assert!(doc.get(a).children.is_empty());
        assert_eq!(doc.get(b).children, vec![c]);
        assert_eq!(doc.get(c).parent, Some(b));
    }

    #[test]
    fn cyrillic_text_node() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let h1 = doc.create_element(QualName::html("h1"));
        let text = doc.create_text("Привет, мир! Ёжик");

        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, h1);
        doc.append_child(h1, text);

        match &doc.get(text).data {
            NodeData::Text(s) => {
                assert_eq!(s, "Привет, мир! Ёжик");
                // Cyrillic is 2 bytes per char in UTF-8, so byte length must exceed char count.
                assert!(s.len() > s.chars().count());
            }
            _ => panic!("expected text node"),
        }

        let printed = doc.to_string();
        assert!(printed.contains("Привет"));
        assert!(printed.contains("Ёжик"));
    }

    #[test]
    fn cyrillic_attribute_value() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));

        let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data else {
            unreachable!();
        };
        attrs.push(Attribute {
            name: QualName::html("title"),
            value: "Привет, кириллица".to_string(),
        });

        doc.append_child(doc.root(), div);

        let s = doc.to_string();
        assert!(s.contains("title=\"Привет, кириллица\""));
    }

    #[test]
    fn display_format() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let h1 = doc.create_element(QualName::html("h1"));
        let text = doc.create_text("Hello");

        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, h1);
        doc.append_child(h1, text);

        let s = doc.to_string();
        assert!(s.contains("#document"));
        assert!(s.contains("<html>"));
        assert!(s.contains("\"Hello\""));
    }

    // ──────── base_href / find_first_element ────────

    fn build_doc_with_base(href: &str) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let base = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base).data {
            attrs.push(Attribute {
                name: QualName::html("href"),
                value: href.to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, base);
        doc
    }

    #[test]
    fn base_href_extracts_attribute() {
        let doc = build_doc_with_base("https://example.com/path/");
        assert_eq!(doc.base_href(), Some("https://example.com/path/"));
    }

    #[test]
    fn base_href_returns_none_without_base() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        doc.append_child(doc.root(), html);
        assert_eq!(doc.base_href(), None);
    }

    #[test]
    fn base_href_returns_none_when_base_has_no_href() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let base = doc.create_element(QualName::html("base"));  // без href
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, base);
        assert_eq!(doc.base_href(), None);
    }

    #[test]
    fn base_href_finds_first_in_document_order() {
        // Два <base> элемента — берём первый в pre-order.
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let base1 = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base1).data {
            attrs.push(Attribute {
                name: QualName::html("href"),
                value: "first".to_string(),
            });
        }
        let base2 = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base2).data {
            attrs.push(Attribute {
                name: QualName::html("href"),
                value: "second".to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, base1);
        doc.append_child(head, base2);
        assert_eq!(doc.base_href(), Some("first"));
    }

    #[test]
    fn base_href_case_insensitive_attribute() {
        // HTML парсер lower-case-ит, но если что-то попало в HREF — get_attr
        // должен находить.
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let base = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base).data {
            attrs.push(Attribute {
                name: QualName::html("HREF"),
                value: "x.com".to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, base);
        assert_eq!(doc.base_href(), Some("x.com"));
    }

    #[test]
    fn find_first_element_returns_none_when_no_match() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        doc.append_child(doc.root(), html);
        let found = doc.find_first_element(|n| {
            n.element_name().map(|q| q.local == "nonexistent").unwrap_or(false)
        });
        assert!(found.is_none());
    }

    // ──────── InputType ────────

    fn build_input(input_type: Option<&str>) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let input = doc.create_element(QualName::html("input"));
        if let Some(t) = input_type
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data
        {
            attrs.push(Attribute {
                name: QualName::html("type"),
                value: t.to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, input);
        doc
    }

    fn input_node(doc: &Document) -> &Node {
        // root → html → input.
        let html_id = doc.get(doc.root()).children[0];
        let input_id = doc.get(html_id).children[0];
        doc.get(input_id)
    }

    #[test]
    fn input_type_default_is_text() {
        let doc = build_input(None);
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Text));
    }

    #[test]
    fn input_type_explicit_text() {
        let doc = build_input(Some("text"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Text));
    }

    #[test]
    fn input_type_password() {
        let doc = build_input(Some("password"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Password));
    }

    #[test]
    fn input_type_email() {
        let doc = build_input(Some("email"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Email));
    }

    #[test]
    fn input_type_all_standard() {
        // Все 22 стандартных значения.
        for (s, expected) in [
            ("tel", InputType::Tel),
            ("url", InputType::Url),
            ("number", InputType::Number),
            ("search", InputType::Search),
            ("date", InputType::Date),
            ("datetime-local", InputType::DateTimeLocal),
            ("time", InputType::Time),
            ("month", InputType::Month),
            ("week", InputType::Week),
            ("color", InputType::Color),
            ("range", InputType::Range),
            ("checkbox", InputType::Checkbox),
            ("radio", InputType::Radio),
            ("file", InputType::File),
            ("submit", InputType::Submit),
            ("reset", InputType::Reset),
            ("button", InputType::Button),
            ("image", InputType::Image),
            ("hidden", InputType::Hidden),
        ] {
            let doc = build_input(Some(s));
            assert_eq!(input_node(&doc).input_type(), Some(expected), "type={s}");
        }
    }

    #[test]
    fn input_type_case_insensitive() {
        let doc = build_input(Some("EMAIL"));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Email));
        let doc2 = build_input(Some("Checkbox"));
        assert_eq!(input_node(&doc2).input_type(), Some(InputType::Checkbox));
    }

    #[test]
    fn input_type_unknown_becomes_other() {
        let doc = build_input(Some("future-feature"));
        assert_eq!(
            input_node(&doc).input_type(),
            Some(InputType::Other("future-feature".to_string()))
        );
    }

    #[test]
    fn input_type_empty_string_treated_as_text() {
        let doc = build_input(Some(""));
        assert_eq!(input_node(&doc).input_type(), Some(InputType::Text));
    }

    #[test]
    fn input_type_none_for_non_input_element() {
        let mut doc = Document::new();
        let p = doc.create_element(QualName::html("p"));
        doc.append_child(doc.root(), p);
        let p_id = doc.get(doc.root()).children[0];
        assert_eq!(doc.get(p_id).input_type(), None);
    }

    #[test]
    fn input_type_round_trip_via_as_str() {
        for t in [
            InputType::Text,
            InputType::Password,
            InputType::Email,
            InputType::Tel,
            InputType::Url,
            InputType::Number,
            InputType::Search,
            InputType::Date,
            InputType::DateTimeLocal,
            InputType::Time,
            InputType::Month,
            InputType::Week,
            InputType::Color,
            InputType::Range,
            InputType::Checkbox,
            InputType::Radio,
            InputType::File,
            InputType::Submit,
            InputType::Reset,
            InputType::Button,
            InputType::Image,
            InputType::Hidden,
            InputType::Other("custom".into()),
        ] {
            assert_eq!(InputType::parse(t.as_str()), t);
        }
    }

    #[test]
    fn input_type_is_textual_classification() {
        assert!(InputType::Text.is_textual());
        assert!(InputType::Email.is_textual());
        assert!(InputType::Password.is_textual());
        assert!(InputType::Number.is_textual());
        assert!(!InputType::Checkbox.is_textual());
        assert!(!InputType::File.is_textual());
    }

    #[test]
    fn input_type_is_button_like() {
        assert!(InputType::Submit.is_button_like());
        assert!(InputType::Reset.is_button_like());
        assert!(InputType::Button.is_button_like());
        assert!(InputType::Image.is_button_like());
        assert!(!InputType::Text.is_button_like());
        assert!(!InputType::Checkbox.is_button_like());
    }

    // ──────── DocumentMode ────────

    #[test]
    fn document_default_mode_is_no_quirks() {
        let doc = Document::new();
        assert_eq!(doc.mode(), DocumentMode::NoQuirks);
    }

    #[test]
    fn document_mode_can_be_set() {
        let mut doc = Document::new();
        doc.set_mode(DocumentMode::Quirks);
        assert_eq!(doc.mode(), DocumentMode::Quirks);
        doc.set_mode(DocumentMode::LimitedQuirks);
        assert_eq!(doc.mode(), DocumentMode::LimitedQuirks);
        doc.set_mode(DocumentMode::NoQuirks);
        assert_eq!(doc.mode(), DocumentMode::NoQuirks);
    }

    // ──────── target_id ────────

    #[test]
    fn document_default_target_is_none() {
        let doc = Document::new();
        assert_eq!(doc.target(), None);
    }

    #[test]
    fn document_target_round_trips_set_get() {
        let mut doc = Document::new();
        doc.set_target(Some("intro"));
        assert_eq!(doc.target(), Some("intro"));
        doc.set_target::<String>(None);
        assert_eq!(doc.target(), None);
    }

    #[test]
    fn document_set_target_empty_becomes_none() {
        // Empty fragment («#» в URL) трактуется как «нет target-а»: страница
        // не должна никого подсвечивать. Совпадает с поведением major-браузеров.
        let mut doc = Document::new();
        doc.set_target(Some(""));
        assert_eq!(doc.target(), None);
    }

    // ──────── sandbox_flags ────────

    fn build_iframe(sandbox: Option<&str>) -> (Document, NodeId) {
        let mut doc = Document::new();
        let iframe = doc.create_element(QualName::html("iframe"));
        if let Some(val) = sandbox
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe).data
        {
            attrs.push(Attribute {
                name: QualName::html("sandbox"),
                value: val.to_string(),
            });
        }
        doc.append_child(doc.root(), iframe);
        (doc, iframe)
    }

    #[test]
    fn sandbox_flags_none_for_non_iframe() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        assert_eq!(doc.get(div).sandbox_flags(), None);
    }

    #[test]
    fn sandbox_flags_iframe_without_attribute_is_empty() {
        let (doc, iframe) = build_iframe(None);
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(flags.is_empty());
    }

    #[test]
    fn sandbox_flags_iframe_empty_attribute_all_restrictions() {
        let (doc, iframe) = build_iframe(Some(""));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert_eq!(flags, SandboxFlags::all_restrictions());
    }

    #[test]
    fn sandbox_flags_allow_scripts_lifts_scripts() {
        let (doc, iframe) = build_iframe(Some("allow-scripts"));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(!flags.contains(SandboxFlags::SCRIPTS));
        assert!(flags.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn sandbox_flags_allow_forms_and_scripts() {
        let (doc, iframe) = build_iframe(Some("allow-scripts allow-forms"));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(!flags.contains(SandboxFlags::SCRIPTS));
        assert!(!flags.contains(SandboxFlags::FORMS));
        assert!(flags.contains(SandboxFlags::ORIGIN));
    }

    #[test]
    fn sandbox_flags_allow_same_origin() {
        let (doc, iframe) = build_iframe(Some("allow-same-origin"));
        let flags = doc.get(iframe).sandbox_flags().unwrap();
        assert!(!flags.contains(SandboxFlags::ORIGIN));
        assert!(flags.contains(SandboxFlags::SCRIPTS));
    }

    // ──────── collect_forms / check_form_gate ────────

    fn build_doc_with_form(
        action: Option<&str>,
        method: Option<&str>,
        controls: &[&str],
    ) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        let form = doc.create_element(QualName::html("form"));
        if let Some(a) = action
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data
        {
            attrs.push(Attribute {
                name: QualName::html("action"),
                value: a.to_string(),
            });
        }
        if let Some(m) = method
            && let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data
        {
            attrs.push(Attribute {
                name: QualName::html("method"),
                value: m.to_string(),
            });
        }
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        doc.append_child(body, form);
        for &tag in controls {
            let el = doc.create_element(QualName::html(tag));
            doc.append_child(form, el);
        }
        doc
    }

    #[test]
    fn collect_forms_finds_form_with_action_and_method() {
        let doc = build_doc_with_form(Some("/submit"), Some("post"), &["input"]);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].action, "/submit");
        assert_eq!(forms[0].method, "post");
        assert_eq!(forms[0].field_count, 1);
    }

    #[test]
    fn collect_forms_defaults_action_and_method() {
        let doc = build_doc_with_form(None, None, &[]);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].action, "");
        assert_eq!(forms[0].method, "get");
        assert_eq!(forms[0].field_count, 0);
    }

    #[test]
    fn collect_forms_counts_all_control_types() {
        let doc =
            build_doc_with_form(None, None, &["input", "select", "textarea", "button"]);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert_eq!(forms[0].field_count, 4);
    }

    #[test]
    fn collect_forms_skips_non_form_elements() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let mut forms = Vec::new();
        collect_forms(&doc, doc.root(), &mut forms);
        assert!(forms.is_empty());
    }

    #[test]
    fn check_form_gate_no_forms_returns_zero() {
        let doc = Document::new();
        assert_eq!(check_form_gate(&doc, SandboxFlags::empty()), 0);
        assert_eq!(check_form_gate(&doc, SandboxFlags::FORMS), 0);
    }

    #[test]
    fn check_form_gate_blocked_by_sandbox_returns_count() {
        let doc = build_doc_with_form(Some("/login"), None, &["input"]);
        assert_eq!(check_form_gate(&doc, SandboxFlags::FORMS), 1);
    }

    #[test]
    fn check_form_gate_allowed_returns_zero() {
        let doc = build_doc_with_form(Some("/login"), None, &["input"]);
        assert_eq!(check_form_gate(&doc, SandboxFlags::empty()), 0);
    }

    // ──────── collect_anchors / check_navigation_gate ────────

    fn build_doc_with_anchors(hrefs: &[&str]) -> Document {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);
        for &href in hrefs {
            let a = doc.create_element(QualName::html("a"));
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(a).data {
                attrs.push(Attribute {
                    name: QualName::html("href"),
                    value: href.to_string(),
                });
            }
            doc.append_child(body, a);
        }
        doc
    }

    #[test]
    fn collect_anchors_finds_href_links() {
        let doc = build_doc_with_anchors(&["/page1", "/page2"]);
        let mut anchors = Vec::new();
        collect_anchors(&doc, doc.root(), &mut anchors);
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].href, "/page1");
        assert_eq!(anchors[1].href, "/page2");
    }

    #[test]
    fn collect_anchors_skips_empty_href() {
        let doc = build_doc_with_anchors(&[""]);
        let mut anchors = Vec::new();
        collect_anchors(&doc, doc.root(), &mut anchors);
        assert!(anchors.is_empty());
    }

    #[test]
    fn collect_anchors_skips_anchor_without_href() {
        let mut doc = Document::new();
        let a = doc.create_element(QualName::html("a"));
        doc.append_child(doc.root(), a);
        let mut anchors = Vec::new();
        collect_anchors(&doc, doc.root(), &mut anchors);
        assert!(anchors.is_empty());
    }

    #[test]
    fn check_navigation_gate_no_anchors_returns_zero() {
        let doc = Document::new();
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::empty()), 0);
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::NAVIGATION), 0);
    }

    #[test]
    fn check_navigation_gate_blocked_by_sandbox_returns_count() {
        let doc = build_doc_with_anchors(&["/a", "/b"]);
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::NAVIGATION), 2);
    }

    #[test]
    fn check_navigation_gate_allowed_returns_zero() {
        let doc = build_doc_with_anchors(&["/a"]);
        assert_eq!(check_navigation_gate(&doc, SandboxFlags::empty()), 0);
    }

    // ──────── Shadow DOM ────────

    fn build_shadow_host() -> (Document, NodeId, NodeId) {
        // <div id="host">  ← shadow host
        //   #shadow-root(open)
        //     <span>shadow</span>
        //   <p>light</p>   ← light-tree child (no slot match → not in flat tree)
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);

        let sr = doc.attach_shadow(host, ShadowRootMode::Open);
        let span = doc.create_element(QualName::html("span"));
        let text = doc.create_text("shadow");
        doc.append_child(sr, span);
        doc.append_child(span, text);

        let light_p = doc.create_element(QualName::html("p"));
        doc.append_child(host, light_p);

        (doc, host, sr)
    }

    #[test]
    fn attach_shadow_registers_host() {
        let (doc, host, sr) = build_shadow_host();
        assert!(doc.is_shadow_host(host));
        assert_eq!(doc.shadow_root_of(host), Some(sr));
    }

    #[test]
    fn shadow_root_node_data_variant() {
        let (doc, _, sr) = build_shadow_host();
        assert!(matches!(
            doc.get(sr).data,
            NodeData::ShadowRoot { mode: ShadowRootMode::Open }
        ));
    }

    #[test]
    fn shadow_root_mode_display() {
        assert_eq!(ShadowRootMode::Open.to_string(), "open");
        assert_eq!(ShadowRootMode::Closed.to_string(), "closed");
    }

    #[test]
    fn flat_tree_no_shadow_is_zero_alloc() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), html);
        doc.append_child(html, body);

        let flat = build_flat_tree(&doc);
        // No overrides — fast path, HashMap is empty.
        assert!(flat.overrides.is_empty());
        // children_of falls through to DOM children.
        assert_eq!(flat.children_of(&doc, html), &[body]);
    }

    #[test]
    fn flat_tree_host_children_are_shadow_root_children() {
        let (doc, host, sr) = build_shadow_host();
        let flat = build_flat_tree(&doc);

        // Host's composed children = shadow root's DOM children (the <span>).
        let sr_children = doc.get(sr).children.clone();
        assert_eq!(flat.children_of(&doc, host), sr_children.as_slice());
    }

    #[test]
    fn flat_tree_slot_distributes_light_children() {
        // Shadow: <slot name="x"> … </slot>
        // Light:  <p slot="x">light</p>
        // After flat tree: slot's composed children = [<p>]
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);

        let sr = doc.attach_shadow(host, ShadowRootMode::Open);

        let slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(slot).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "x".into() });
        }
        let fallback = doc.create_text("fallback");
        doc.append_child(sr, slot);
        doc.append_child(slot, fallback);

        let light_p = doc.create_element(QualName::html("p"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(light_p).data {
            attrs.push(Attribute { name: QualName::html("slot"), value: "x".into() });
        }
        doc.append_child(host, light_p);

        let flat = build_flat_tree(&doc);

        // Slot is overridden with assigned light node, not fallback.
        assert_eq!(flat.children_of(&doc, slot), &[light_p]);
    }

    #[test]
    fn flat_tree_slot_fallback_when_no_assigned_nodes() {
        // Slot with name "x" but no light-tree child with slot="x".
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);

        let sr = doc.attach_shadow(host, ShadowRootMode::Open);
        let slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(slot).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "y".into() });
        }
        let fallback = doc.create_text("fallback");
        doc.append_child(sr, slot);
        doc.append_child(slot, fallback);

        let flat = build_flat_tree(&doc);
        // No assignment → no override → slot keeps its DOM children (fallback).
        assert_eq!(flat.children_of(&doc, slot), &[fallback]);
    }

    #[test]
    fn flat_tree_nested_shadow_with_slot_delegation() {
        // Scenario:
        // <custom-component>
        //   #shadow-root(open)
        //     <slot name="item"></slot>
        //   <custom-item slot="item">
        //     #shadow-root(open)
        //       <div>Item content</div>
        //   </custom-item>
        //
        // Expected flat tree:
        // - custom-component's composed children = [custom-item (from shadow root)]
        // - slot's composed children = [custom-item (from light tree assignment)]
        // - custom-item's composed children = [<div>Item content</div> (from its shadow root)]

        let mut doc = Document::new();

        // Create outer component with shadow tree
        let outer_host = doc.create_element(QualName::html("custom-component"));
        doc.append_child(doc.root(), outer_host);

        let outer_shadow = doc.attach_shadow(outer_host, ShadowRootMode::Open);
        let outer_slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(outer_slot).data {
            attrs.push(Attribute {
                name: QualName::html("name"),
                value: "item".into(),
            });
        }
        doc.append_child(outer_shadow, outer_slot);

        // Create inner component with shadow tree and slot attribute
        let inner_host = doc.create_element(QualName::html("custom-item"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inner_host).data {
            attrs.push(Attribute {
                name: QualName::html("slot"),
                value: "item".into(),
            });
        }
        doc.append_child(outer_host, inner_host); // Light tree child of outer

        let inner_shadow = doc.attach_shadow(inner_host, ShadowRootMode::Open);
        let inner_content = doc.create_element(QualName::html("div"));
        doc.append_child(inner_shadow, inner_content);

        let flat = build_flat_tree(&doc);

        // Outer host should have shadow root children (which includes slot)
        assert_eq!(flat.children_of(&doc, outer_host), &[outer_slot]);

        // Outer slot should have inner_host as its assigned child
        assert_eq!(flat.children_of(&doc, outer_slot), &[inner_host]);

        // Inner host should have inner_content as its composed child (from its shadow root)
        assert_eq!(flat.children_of(&doc, inner_host), &[inner_content]);
    }

    #[test]
    fn flat_tree_nested_slot_fallback() {
        // Scenario:
        // <outer-component>
        //   #shadow-root(open)
        //     <slot name="header">
        //       <default-header></default-header>
        //     </slot>
        //   <!-- light tree: no child with slot="header", so fallback is used -->
        //
        // Expected: slot should have its DOM child (default-header) as composed children.

        let mut doc = Document::new();

        let outer_host = doc.create_element(QualName::html("outer-component"));
        doc.append_child(doc.root(), outer_host);

        let outer_shadow = doc.attach_shadow(outer_host, ShadowRootMode::Open);
        let slot = doc.create_element(QualName::html("slot"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(slot).data {
            attrs.push(Attribute {
                name: QualName::html("name"),
                value: "header".into(),
            });
        }
        doc.append_child(outer_shadow, slot);

        let fallback = doc.create_element(QualName::html("default-header"));
        doc.append_child(slot, fallback);

        // No light-tree children with slot="header", so fallback should be used.

        let flat = build_flat_tree(&doc);

        // Slot should have fallback as its composed children (no assignment).
        assert_eq!(flat.children_of(&doc, slot), &[fallback]);
    }

    #[test]
    fn shadow_root_printed_in_display() {
        let (doc, _, _) = build_shadow_host();
        let s = doc.to_string();
        assert!(s.contains("#shadow-root (open)"));
    }

    // ── form submission helpers ──────────────────────────────────────────────

    fn make_form_doc() -> (Document, NodeId, NodeId, NodeId) {
        // <form action="/send" method="post">
        //   <input name="user" value="alice">
        //   <input type="submit">
        // </form>
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
            attrs.push(Attribute { name: QualName::html("action"), value: "/send".into() });
            attrs.push(Attribute { name: QualName::html("method"), value: "post".into() });
        }
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "user".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "alice".into() });
        }
        let submit = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, input);
        doc.append_child(form, submit);
        (doc, form, input, submit)
    }

    #[test]
    fn find_ancestor_form_direct_child() {
        let (doc, form, input, _) = make_form_doc();
        assert_eq!(find_ancestor_form(&doc, input), Some(form));
    }

    #[test]
    fn find_ancestor_form_nested() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let div = doc.create_element(QualName::html("div"));
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), form);
        doc.append_child(form, div);
        doc.append_child(div, input);
        assert_eq!(find_ancestor_form(&doc, input), Some(form));
    }

    #[test]
    fn find_ancestor_form_no_form() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), div);
        doc.append_child(div, input);
        assert_eq!(find_ancestor_form(&doc, input), None);
    }

    #[test]
    fn collect_dom_form_fields_basic() {
        let (doc, form, _, _) = make_form_doc();
        let fields = collect_dom_form_fields(&doc, form);
        // submit input должен быть исключён; только "user" должен попасть
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "user");
        assert_eq!(fields[0].1, "alice");
    }

    #[test]
    fn collect_dom_form_fields_skips_disabled() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "x".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "v".into() });
            attrs.push(Attribute { name: QualName::html("disabled"), value: String::new() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, input);
        let fields = collect_dom_form_fields(&doc, form);
        assert!(fields.is_empty());
    }

    #[test]
    fn collect_dom_form_fields_skips_nameless() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let input = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "v".into() });
            // no "name" attribute
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, input);
        let fields = collect_dom_form_fields(&doc, form);
        assert!(fields.is_empty());
    }

    #[test]
    fn collect_dom_form_fields_unchecked_checkbox_excluded() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let cb = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "checkbox".into() });
            attrs.push(Attribute { name: QualName::html("name"), value: "agree".into() });
            // no "checked" attribute — не отмечен
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, cb);
        let fields = collect_dom_form_fields(&doc, form);
        assert!(fields.is_empty());
    }

    #[test]
    fn collect_dom_form_fields_checked_checkbox_included() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let cb = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(cb).data {
            attrs.push(Attribute { name: QualName::html("type"), value: "checkbox".into() });
            attrs.push(Attribute { name: QualName::html("name"), value: "agree".into() });
            attrs.push(Attribute { name: QualName::html("checked"), value: String::new() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, cb);
        let fields = collect_dom_form_fields(&doc, form);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, "agree");
        assert_eq!(fields[0].1, "on"); // default checkbox value
    }

    #[test]
    fn collect_dom_form_fields_textarea() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let ta = doc.create_element(QualName::html("textarea"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(ta).data {
            attrs.push(Attribute { name: QualName::html("name"), value: "msg".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "hello".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, ta);
        let fields = collect_dom_form_fields(&doc, form);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0], ("msg".into(), "hello".into()));
    }

    #[test]
    fn collect_dom_form_fields_multiple() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        for (name, val) in [("a", "1"), ("b", "2"), ("c", "3")] {
            let inp = doc.create_element(QualName::html("input"));
            if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
                attrs.push(Attribute { name: QualName::html("name"), value: name.into() });
                attrs.push(Attribute { name: QualName::html("value"), value: val.into() });
            }
            doc.append_child(form, inp);
        }
        doc.append_child(doc.root(), form);
        let fields = collect_dom_form_fields(&doc, form);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], ("a".into(), "1".into()));
        assert_eq!(fields[1], ("b".into(), "2".into()));
        assert_eq!(fields[2], ("c".into(), "3".into()));
    }

    // ──────────────────────────────────────────────────────────────────────────
    // ValidityState tests
    // ──────────────────────────────────────────────────────────────────────────

    fn make_input(attrs: &[(&str, &str)]) -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs: a, .. } = &mut doc.get_mut(inp).data {
            for &(name, val) in attrs {
                a.push(Attribute { name: QualName::html(name), value: val.into() });
            }
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);
        (doc, form, inp)
    }

    #[test]
    fn validity_non_form_element_returns_none() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        assert_eq!(element_validity(&doc, div), None);
    }

    #[test]
    fn validity_hidden_input_returns_none() {
        let (doc, _, inp) = make_input(&[("type", "hidden"), ("required", "")]);
        assert_eq!(element_validity(&doc, inp), None);
    }

    #[test]
    fn validity_submit_input_returns_none() {
        let (doc, _, inp) = make_input(&[("type", "submit")]);
        assert_eq!(element_validity(&doc, inp), None);
    }

    #[test]
    fn validity_disabled_input_returns_none() {
        let (doc, _, inp) = make_input(&[("required", ""), ("disabled", "")]);
        assert_eq!(element_validity(&doc, inp), None);
    }

    #[test]
    fn validity_required_empty_value_missing() {
        let (doc, _, inp) = make_input(&[("required", ""), ("value", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.value_missing);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_required_with_value_not_missing() {
        let (doc, _, inp) = make_input(&[("required", ""), ("value", "alice")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.value_missing);
        assert!(vs.valid());
    }

    #[test]
    fn validity_required_checkbox_unchecked_missing() {
        let (doc, _, inp) = make_input(&[("type", "checkbox"), ("required", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.value_missing);
    }

    #[test]
    fn validity_required_checkbox_checked_ok() {
        let (doc, _, inp) = make_input(&[("type", "checkbox"), ("required", ""), ("checked", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.value_missing);
        assert!(vs.valid());
    }

    #[test]
    fn validity_email_type_mismatch() {
        let (doc, _, inp) = make_input(&[("type", "email"), ("value", "notanemail")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.type_mismatch);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_email_valid() {
        let (doc, _, inp) = make_input(&[("type", "email"), ("value", "user@example.com")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.type_mismatch);
        assert!(vs.valid());
    }

    #[test]
    fn validity_url_type_mismatch() {
        let (doc, _, inp) = make_input(&[("type", "url"), ("value", "notaurl")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.type_mismatch);
    }

    #[test]
    fn validity_url_valid() {
        let (doc, _, inp) = make_input(&[("type", "url"), ("value", "https://example.com")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.type_mismatch);
        assert!(vs.valid());
    }

    #[test]
    fn validity_range_underflow() {
        let (doc, _, inp) = make_input(&[("type", "number"), ("min", "10"), ("value", "5")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.range_underflow);
        assert!(!vs.range_overflow);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_range_overflow() {
        let (doc, _, inp) = make_input(&[("type", "number"), ("max", "10"), ("value", "20")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.range_overflow);
        assert!(!vs.range_underflow);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_number_in_range() {
        let (doc, _, inp) = make_input(&[("type", "number"), ("min", "0"), ("max", "100"), ("value", "50")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.range_underflow);
        assert!(!vs.range_overflow);
        assert!(vs.valid());
    }

    #[test]
    fn validity_too_long() {
        let (doc, _, inp) = make_input(&[("maxlength", "3"), ("value", "hello")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.too_long);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_too_short() {
        let (doc, _, inp) = make_input(&[("minlength", "5"), ("value", "hi")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(vs.too_short);
        assert!(!vs.valid());
    }

    #[test]
    fn validity_length_ok() {
        let (doc, _, inp) = make_input(&[("minlength", "2"), ("maxlength", "10"), ("value", "hello")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.too_short);
        assert!(!vs.too_long);
        assert!(vs.valid());
    }

    #[test]
    fn validity_empty_value_not_too_short() {
        // tooShort only applies when field has a value; empty is valueMissing territory.
        let (doc, _, inp) = make_input(&[("minlength", "5"), ("value", "")]);
        let vs = element_validity(&doc, inp).unwrap();
        assert!(!vs.too_short);
    }

    #[test]
    fn check_validity_form_all_valid() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "filled".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp);
        assert!(check_validity_form(&doc, form));
    }

    #[test]
    fn check_validity_form_one_invalid() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        // valid input
        let inp1 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "ok".into() });
        }
        // invalid input: required but empty
        let inp2 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp1);
        doc.append_child(form, inp2);
        assert!(!check_validity_form(&doc, form));
    }

    #[test]
    fn invalid_controls_in_form_finds_them() {
        let mut doc = Document::new();
        let form = doc.create_element(QualName::html("form"));
        let inp1 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
            attrs.push(Attribute { name: QualName::html("value"), value: "ok".into() });
        }
        let inp2 = doc.create_element(QualName::html("input"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
            attrs.push(Attribute { name: QualName::html("required"), value: "".into() });
            attrs.push(Attribute { name: QualName::html("value"), value: "".into() });
        }
        doc.append_child(doc.root(), form);
        doc.append_child(form, inp1);
        doc.append_child(form, inp2);
        let invalid = invalid_controls_in_form(&doc, form);
        assert_eq!(invalid.len(), 1);
        assert_eq!(invalid[0], inp2);
    }

    // ──────── EditInputType ────────

    #[test]
    fn edit_input_type_as_str_round_trip() {
        let cases = [
            (EditInputType::InsertText, "insertText"),
            (EditInputType::InsertParagraph, "insertParagraph"),
            (EditInputType::InsertLineBreak, "insertLineBreak"),
            (EditInputType::DeleteContentBackward, "deleteContentBackward"),
            (EditInputType::DeleteContentForward, "deleteContentForward"),
            (EditInputType::DeleteWordBackward, "deleteWordBackward"),
            (EditInputType::DeleteWordForward, "deleteWordForward"),
            (EditInputType::InsertFromPaste, "insertFromPaste"),
            (EditInputType::DeleteByCut, "deleteByCut"),
            (EditInputType::SelectAll, "selectAll"),
            (EditInputType::HistoryUndo, "historyUndo"),
            (EditInputType::HistoryRedo, "historyRedo"),
        ];
        for (variant, expected) in cases {
            assert_eq!(variant.as_str(), expected, "mismatch for {:?}", variant);
        }
    }

    // ──────── insert_text_at ────────

    fn make_text_doc(content: &str) -> (Document, NodeId, NodeId) {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        let text = doc.create_text(content);
        doc.append_child(doc.root(), div);
        doc.append_child(div, text);
        (doc, div, text)
    }

    #[test]
    fn insert_text_at_start() {
        let (mut doc, _, text) = make_text_doc("world");
        let pos = DomPosition { container: text, offset: 0 };
        let new_pos = insert_text_at(&mut doc, pos, "Hello ");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello world"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.container, text);
        assert_eq!(new_pos.offset, 6);
    }

    #[test]
    fn insert_text_at_end() {
        let (mut doc, _, text) = make_text_doc("Hello");
        let pos = DomPosition { container: text, offset: 5 };
        let new_pos = insert_text_at(&mut doc, pos, " world");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello world"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.offset, 11);
    }

    #[test]
    fn insert_text_at_mid() {
        let (mut doc, _, text) = make_text_doc("Helo");
        let pos = DomPosition { container: text, offset: 3 };
        let new_pos = insert_text_at(&mut doc, pos, "l");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.offset, 4);
    }

    #[test]
    fn insert_text_at_empty_node() {
        let (mut doc, _, text) = make_text_doc("");
        let pos = DomPosition { container: text, offset: 0 };
        let new_pos = insert_text_at(&mut doc, pos, "Hi");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hi"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos.offset, 2);
    }

    #[test]
    fn insert_text_at_element_creates_text_child() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let pos = DomPosition { container: div, offset: 0 };
        let new_pos = insert_text_at(&mut doc, pos, "abc");
        // A text child was created.
        let children = &doc.get(div).children;
        assert_eq!(children.len(), 1);
        match &doc.get(children[0]).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!("no text child created"),
        }
        assert_eq!(new_pos.offset, 3);
    }

    #[test]
    fn insert_text_at_multibyte_utf8() {
        // "Привет" — each Cyrillic char is 2 bytes in UTF-8.
        // Char boundaries: П=0, р=2, и=4, в=6, е=8, т=10, end=12.
        // offset 4 is exactly the start of "и" — insert X before "и".
        let (mut doc, _, text) = make_text_doc("Привет");
        let pos = DomPosition { container: text, offset: 4 };
        let new_pos = insert_text_at(&mut doc, pos, "X");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "ПрXивет"),
            _ => panic!("not a text node"),
        }
        // 4 bytes before + 1 byte "X" = offset 5.
        assert_eq!(new_pos.offset, 5);
    }

    #[test]
    fn insert_text_noop_when_empty_string() {
        let (mut doc, _, text) = make_text_doc("abc");
        let pos = DomPosition { container: text, offset: 1 };
        let new_pos = insert_text_at(&mut doc, pos, "");
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!("not a text node"),
        }
        assert_eq!(new_pos, pos);
    }

    // ──────── delete_range ────────

    #[test]
    fn delete_range_same_node_full() {
        let (mut doc, _, text) = make_text_doc("Hello");
        let range = Range {
            start: DomPosition { container: text, offset: 0 },
            end:   DomPosition { container: text, offset: 5 },
        };
        let pos = delete_range(&mut doc, &range);
        match &doc.get(text).data {
            NodeData::Text(s) => assert!(s.is_empty()),
            _ => panic!("not a text node"),
        }
        assert_eq!(pos.offset, 0);
    }

    #[test]
    fn delete_range_same_node_partial() {
        let (mut doc, _, text) = make_text_doc("Hello world");
        let range = Range {
            start: DomPosition { container: text, offset: 5 },
            end:   DomPosition { container: text, offset: 11 },
        };
        let pos = delete_range(&mut doc, &range);
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!("not a text node"),
        }
        assert_eq!(pos.offset, 5);
    }

    #[test]
    fn delete_range_collapsed_noop() {
        let (mut doc, _, text) = make_text_doc("abc");
        let range = Range::collapsed(DomPosition { container: text, offset: 1 });
        let pos = delete_range(&mut doc, &range);
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!("not a text node"),
        }
        assert_eq!(pos.offset, 1);
    }

    // ──────── split_text_node ────────

    #[test]
    fn split_text_node_basic() {
        let (mut doc, div, text) = make_text_doc("Hello world");
        let second = split_text_node(&mut doc, text, 5);
        // First node: "Hello"
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!(),
        }
        // Second node: " world"
        match &doc.get(second).data {
            NodeData::Text(s) => assert_eq!(s, " world"),
            _ => panic!(),
        }
        // Parent has two text children in correct order.
        let children = &doc.get(div).children;
        assert_eq!(children, &[text, second]);
    }

    #[test]
    fn split_text_node_at_start() {
        let (mut doc, div, text) = make_text_doc("abc");
        let second = split_text_node(&mut doc, text, 0);
        match &doc.get(text).data {
            NodeData::Text(s) => assert!(s.is_empty()),
            _ => panic!(),
        }
        match &doc.get(second).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!(),
        }
        assert_eq!(doc.get(div).children, vec![text, second]);
    }

    #[test]
    fn split_text_node_at_end() {
        let (mut doc, div, text) = make_text_doc("abc");
        let second = split_text_node(&mut doc, text, 3);
        match &doc.get(text).data {
            NodeData::Text(s) => assert_eq!(s, "abc"),
            _ => panic!(),
        }
        match &doc.get(second).data {
            NodeData::Text(s) => assert!(s.is_empty()),
            _ => panic!(),
        }
        assert_eq!(doc.get(div).children, vec![text, second]);
    }

    // ──────── insert_paragraph_break ────────

    #[test]
    fn insert_paragraph_break_creates_br() {
        let (mut doc, div, text) = make_text_doc("Hello world");
        let pos = DomPosition { container: text, offset: 5 };
        let new_pos = insert_paragraph_break(&mut doc, pos, div);

        // The div should now have: [text("Hello"), br, text(" world")]
        let children = doc.get(div).children.clone();
        assert_eq!(children.len(), 3);

        match &doc.get(children[0]).data {
            NodeData::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!("expected first text node"),
        }
        match &doc.get(children[1]).data {
            NodeData::Element { name, .. } => assert_eq!(name.local, "br"),
            _ => panic!("expected br element"),
        }
        match &doc.get(children[2]).data {
            NodeData::Text(s) => assert_eq!(s, " world"),
            _ => panic!("expected second text node"),
        }
        // New caret position is at the start of the second text node.
        assert_eq!(new_pos.offset, 0);
        assert_eq!(new_pos.container, children[2]);
    }

    #[test]
    fn insert_paragraph_break_on_element_appends_br() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        let pos = DomPosition { container: div, offset: 0 };
        let new_pos = insert_paragraph_break(&mut doc, pos, div);
        let children = doc.get(div).children.clone();
        // Should have a <br> and an empty text node.
        assert_eq!(children.len(), 2);
        match &doc.get(children[0]).data {
            NodeData::Element { name, .. } => assert_eq!(name.local, "br"),
            _ => panic!("expected br"),
        }
        assert_eq!(new_pos.offset, 0);
    }

    // ── collect_iframes ───────────────────────────────────────────────────────

    fn make_iframe(sandbox: Option<&str>, src: Option<&str>) -> Document {
        let mut doc = Document::new();
        let iframe = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(iframe).data {
            if let Some(s) = sandbox {
                attrs.push(Attribute { name: QualName::html("sandbox"), value: s.to_string() });
            }
            if let Some(s) = src {
                attrs.push(Attribute { name: QualName::html("src"), value: s.to_string() });
            }
        }
        doc.append_child(doc.root(), iframe);
        doc
    }

    #[test]
    fn collect_iframes_empty_document() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), div);
        assert!(collect_iframes(&doc).is_empty());
    }

    #[test]
    fn collect_iframes_finds_iframe_without_sandbox() {
        let doc = make_iframe(None, Some("https://example.com"));
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].src.as_deref(), Some("https://example.com"));
        assert!(!frames[0].is_sandboxed);
        assert!(frames[0].sandbox.is_empty());
    }

    #[test]
    fn collect_iframes_sandboxed_empty_attr_all_restrictions() {
        let doc = make_iframe(Some(""), Some("page.html"));
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_sandboxed);
        assert_eq!(frames[0].sandbox, SandboxFlags::all_restrictions());
        assert!(frames[0].sandbox.contains(SandboxFlags::SCRIPTS));
        assert!(frames[0].sandbox.contains(SandboxFlags::FORMS));
        assert!(frames[0].sandbox.contains(SandboxFlags::AUXILIARY_NAVIGATION));
    }

    #[test]
    fn collect_iframes_allow_scripts_lifts_scripts_flag() {
        let doc = make_iframe(Some("allow-scripts"), Some("a.html"));
        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_sandboxed);
        assert!(!frames[0].sandbox.contains(SandboxFlags::SCRIPTS));
        assert!(frames[0].sandbox.contains(SandboxFlags::FORMS));
    }

    #[test]
    fn collect_iframes_multiple_iframes() {
        let mut doc = Document::new();
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), body);

        // iframe 1: no sandbox
        let f1 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f1).data {
            attrs.push(Attribute { name: QualName::html("src"), value: "a.html".to_string() });
        }
        doc.append_child(body, f1);

        // iframe 2: allow-scripts allow-forms
        let f2 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f2).data {
            attrs.push(Attribute {
                name: QualName::html("sandbox"),
                value: "allow-scripts allow-forms".to_string(),
            });
            attrs.push(Attribute { name: QualName::html("src"), value: "b.html".to_string() });
        }
        doc.append_child(body, f2);

        // iframe 3: sandbox="" (all restrictions)
        let f3 = doc.create_element(QualName::html("iframe"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(f3).data {
            attrs.push(Attribute { name: QualName::html("sandbox"), value: String::new() });
            attrs.push(Attribute { name: QualName::html("src"), value: "c.html".to_string() });
        }
        doc.append_child(body, f3);

        let frames = collect_iframes(&doc);
        assert_eq!(frames.len(), 3);
        assert!(!frames[0].is_sandboxed);
        assert!(frames[1].is_sandboxed);
        assert!(!frames[1].sandbox.contains(SandboxFlags::SCRIPTS));
        assert!(!frames[1].sandbox.contains(SandboxFlags::FORMS));
        assert!(frames[2].is_sandboxed);
        assert_eq!(frames[2].sandbox, SandboxFlags::all_restrictions());
    }

    // ── check_popup_gate ──────────────────────────────────────────────────────

    #[test]
    fn popup_gate_blocked_when_auxiliary_navigation_set() {
        assert!(check_popup_gate(SandboxFlags::AUXILIARY_NAVIGATION));
    }

    #[test]
    fn popup_gate_allowed_when_flag_not_set() {
        assert!(!check_popup_gate(SandboxFlags::empty()));
    }

    #[test]
    fn popup_gate_blocked_when_all_restrictions() {
        assert!(check_popup_gate(SandboxFlags::all_restrictions()));
    }

    #[test]
    fn popup_gate_allowed_after_allow_popups() {
        let flags = lumen_core::parse_sandbox_value(Some("allow-popups"));
        assert!(!check_popup_gate(flags));
    }

    // ── DOM snapshot (T3 hibernation) ─────────────────────────────────────────

    #[test]
    fn snapshot_empty_document_roundtrip() {
        let doc = Document::new();
        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");
        assert_eq!(restored.mode(), doc.mode());
        assert_eq!(restored.root(), doc.root());
    }

    #[test]
    fn snapshot_document_with_elements_roundtrip() {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        doc.append_child(doc.root(), html);
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(html, body);
        let text = doc.create_text("hello world");
        doc.append_child(body, text);
        doc.set_mode(DocumentMode::NoQuirks);

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        assert_eq!(restored.mode(), DocumentMode::NoQuirks);
        let root_children = restored.get(restored.root()).children.clone();
        assert_eq!(root_children.len(), 1);
        let html_id = root_children[0];
        let html_children = restored.get(html_id).children.clone();
        assert_eq!(html_children.len(), 1);
        let body_id = html_children[0];
        let body_children = restored.get(body_id).children.clone();
        assert_eq!(body_children.len(), 1);
        let text_id = body_children[0];
        assert!(matches!(&restored.get(text_id).data, NodeData::Text(s) if s == "hello world"));
    }

    #[test]
    fn snapshot_document_with_attributes_roundtrip() {
        let mut doc = Document::new();
        let div = doc.create_element(QualName::html("div"));
        // Manually push attributes via NodeData::Element.
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(Attribute { name: QualName::html("class"), value: "container".into() });
            attrs.push(Attribute { name: QualName::html("id"), value: "main".into() });
        }
        doc.append_child(doc.root(), div);

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        let div_id = restored.get(restored.root()).children[0];
        assert_eq!(restored.get(div_id).get_attr("class"), Some("container"));
        assert_eq!(restored.get(div_id).get_attr("id"), Some("main"));
    }

    #[test]
    fn snapshot_document_with_shadow_root_roundtrip() {
        let mut doc = Document::new();
        let host = doc.create_element(QualName::html("div"));
        doc.append_child(doc.root(), host);
        let shadow = doc.attach_shadow(host, ShadowRootMode::Open);
        let span = doc.create_element(QualName::html("span"));
        doc.append_child(shadow, span);

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        let host_id = restored.get(restored.root()).children[0];
        let shadow_id = restored.shadow_root_of(host_id).expect("shadow root");
        let shadow_children = &restored.get(shadow_id).children;
        assert_eq!(shadow_children.len(), 1);
    }

    #[test]
    fn snapshot_quirks_mode_preserved() {
        let mut doc = Document::new();
        doc.set_mode(DocumentMode::Quirks);
        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");
        assert_eq!(restored.mode(), DocumentMode::Quirks);
    }

    #[test]
    fn snapshot_selection_preserved() {
        let mut doc = Document::new();
        let text = doc.create_text("abcdef");
        doc.append_child(doc.root(), text);
        let sel = Selection {
            anchor: Some(DomPosition { container: text, offset: 0 }),
            focus: Some(DomPosition { container: text, offset: 3 }),
        };
        doc.set_selection(sel.clone());

        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");
        assert_eq!(restored.get_selection(), &sel);
    }

    #[test]
    fn snapshot_blob_is_compact() {
        // Ensure the snapshot is a reasonable size (not accidentally inflated).
        let mut doc = Document::new();
        let body = doc.create_element(QualName::html("body"));
        doc.append_child(doc.root(), body);
        let text = doc.create_text("hello");
        doc.append_child(body, text);
        let bytes = doc.to_bytes().expect("encode");
        // A 3-node tree should serialize well under 1 KB.
        assert!(bytes.len() < 1024, "snapshot too large: {} bytes", bytes.len());
    }

    // ──────────────────────────────────────────────────────────────────────────
    // IME Composition Events tests
    // ──────────────────────────────────────────────────────────────────────────

    #[test]
    fn composition_event_type_as_str() {
        assert_eq!(CompositionEventType::Start.as_str(), "compositionstart");
        assert_eq!(CompositionEventType::Update.as_str(), "compositionupdate");
        assert_eq!(CompositionEventType::End.as_str(), "compositionend");
    }

    #[test]
    fn composition_event_constructors() {
        let start = CompositionEvent::start("あ".to_string(), Some("ja".to_string()));
        assert_eq!(start.event_type, CompositionEventType::Start);
        assert_eq!(start.data.data, "あ");
        assert_eq!(start.data.locale, Some("ja".to_string()));
        assert_eq!(start.data.range, None);

        let update = CompositionEvent::update("あい".to_string(), Some((0, 2)));
        assert_eq!(update.event_type, CompositionEventType::Update);
        assert_eq!(update.data.data, "あい");
        assert_eq!(update.data.locale, None);
        assert_eq!(update.data.range, Some((0, 2)));

        let end = CompositionEvent::end("あいう".to_string());
        assert_eq!(end.event_type, CompositionEventType::End);
        assert_eq!(end.data.data, "あいう");
        assert_eq!(end.data.locale, None);
        assert_eq!(end.data.range, None);
    }

    #[test]
    fn document_begin_composition() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), input);

        // No composition initially
        assert!(doc.get_composition().is_none());

        // Begin composition
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));
        let comp = doc.get_composition();
        assert!(comp.is_some());
        let comp = comp.unwrap();
        assert_eq!(comp.node, input);
        assert_eq!(comp.text, "あ");
        assert_eq!(comp.locale, Some("ja".to_string()));
        assert_eq!(comp.selection, None);
    }

    #[test]
    fn document_update_composition() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));

        // Update with new preedit and selection
        doc.update_composition("あい".to_string(), Some((0, 2)));
        let comp = doc.get_composition().unwrap();
        assert_eq!(comp.text, "あい");
        assert_eq!(comp.selection, Some((0, 2)));
        // Locale should remain unchanged
        assert_eq!(comp.locale, Some("ja".to_string()));
    }

    #[test]
    fn document_update_composition_no_active() {
        let mut doc = Document::new();
        // Updating without active composition should be a no-op
        doc.update_composition("text".to_string(), Some((0, 4)));
        assert!(doc.get_composition().is_none());
    }

    #[test]
    fn document_end_composition() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));

        // End composition returns the state
        let ended = doc.end_composition();
        assert!(ended.is_some());
        let ended = ended.unwrap();
        assert_eq!(ended.node, input);
        assert_eq!(ended.text, "あ");

        // Composition should now be None
        assert!(doc.get_composition().is_none());
    }

    #[test]
    fn document_end_composition_no_active() {
        let mut doc = Document::new();
        // Ending without active composition should return None
        assert!(doc.end_composition().is_none());
    }

    #[test]
    fn document_composition_sequence() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));

        // Simulates a full IME composition sequence (Japanese input).
        // User wants to type "こんにちは" (konnichiha).

        // 1. Start: User types first key
        doc.begin_composition(input, "こ".to_string(), Some("ja".to_string()));
        assert_eq!(doc.get_composition().unwrap().text, "こ");

        // 2. Update: User continues typing
        doc.update_composition("こん".to_string(), Some((0, 2)));
        assert_eq!(doc.get_composition().unwrap().text, "こん");

        doc.update_composition("こんに".to_string(), Some((0, 3)));
        assert_eq!(doc.get_composition().unwrap().text, "こんに");

        // 3. End: User commits the input
        let final_state = doc.end_composition();
        assert!(final_state.is_some());
        assert_eq!(final_state.unwrap().text, "こんに");

        // Composition is now cleared
        assert!(doc.get_composition().is_none());
    }

    #[test]
    fn composition_state_snapshot_roundtrip() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));
        doc.append_child(doc.root(), input);
        doc.begin_composition(input, "test".to_string(), Some("en".to_string()));

        // Serialize and deserialize
        let bytes = doc.to_bytes().expect("encode");
        let restored = Document::from_bytes(&bytes).expect("decode");

        // Composition state should be preserved
        let restored_comp = restored.get_composition();
        assert!(restored_comp.is_some());
        let restored_comp = restored_comp.unwrap();
        assert_eq!(restored_comp.text, "test");
        assert_eq!(restored_comp.locale, Some("en".to_string()));
    }

    #[test]
    fn composition_helper_is_composing() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));

        // Not composing initially
        assert!(!doc.is_composing());

        // Begin composition
        doc.begin_composition(input, "あ".to_string(), Some("ja".to_string()));
        assert!(doc.is_composing());

        // End composition
        doc.end_composition();
        assert!(!doc.is_composing());
    }

    #[test]
    fn composition_helper_get_range() {
        let mut doc = Document::new();
        let input = doc.create_element(QualName::html("input"));

        // No range initially
        assert!(doc.get_composition_range().is_none());

        // Begin composition without range
        doc.begin_composition(input, "a".to_string(), None);
        assert!(doc.get_composition_range().is_none());

        // Update with range
        doc.update_composition("ab".to_string(), Some((0, 2)));
        assert_eq!(doc.get_composition_range(), Some((0, 2)));

        // Update with different range
        doc.update_composition("abc".to_string(), Some((0, 3)));
        assert_eq!(doc.get_composition_range(), Some((0, 3)));

        // End composition clears range
        doc.end_composition();
        assert!(doc.get_composition_range().is_none());
    }

    #[test]
    fn composition_helper_get_target() {
        let mut doc = Document::new();
        let input1 = doc.create_element(QualName::html("input"));
        let input2 = doc.create_element(QualName::html("textarea"));

        // No target initially
        assert!(doc.get_composition_target().is_none());

        // Begin composition on input1
        doc.begin_composition(input1, "text".to_string(), None);
        assert_eq!(doc.get_composition_target(), Some(input1));

        // End and start on input2
        doc.end_composition();
        doc.begin_composition(input2, "more".to_string(), None);
        assert_eq!(doc.get_composition_target(), Some(input2));

        // End composition clears target
        doc.end_composition();
        assert!(doc.get_composition_target().is_none());
    }

    #[test]
    fn composition_helpers_with_ranges() {
        let mut doc = Document::new();
        let contenteditable = doc.create_element(QualName::html("div"));

        // Simulate IME input with range tracking (UI Events §5.2.5)
        doc.begin_composition(contenteditable, "c".to_string(), Some("ru".to_string()));
        assert!(doc.is_composing());
        assert_eq!(doc.get_composition_target(), Some(contenteditable));

        // User updates composition
        doc.update_composition("ч".to_string(), Some((0, 1)));
        assert_eq!(doc.get_composition_range(), Some((0, 1)));

        doc.update_composition("чт".to_string(), Some((0, 2)));
        assert_eq!(doc.get_composition_range(), Some((0, 2)));

        // Final commit
        let final_state = doc.end_composition();
        assert!(!doc.is_composing());
        assert!(final_state.is_some());
        assert_eq!(final_state.unwrap().text, "чт");
    }

    #[test]
    fn composition_event_dispatching_ready() {
        // Test CompositionEvent readiness for P3 dispatch (UI Events §5.2.5)
        // P3 will serialize these events to JS runtime

        // compositionstart event
        let start_evt = CompositionEvent::start("初".to_string(), Some("zh".to_string()));
        assert_eq!(start_evt.event_type, CompositionEventType::Start);
        assert_eq!(start_evt.event_type.as_str(), "compositionstart");
        assert_eq!(start_evt.data.data, "初");
        assert_eq!(start_evt.data.locale, Some("zh".to_string()));

        // compositionupdate events track user edits and cursor position
        let update1 = CompositionEvent::update("初".to_string(), Some((0, 1)));
        assert_eq!(update1.event_type.as_str(), "compositionupdate");
        assert_eq!(update1.data.range, Some((0, 1))); // cursor at offset 0, length 1

        let update2 = CompositionEvent::update("初中".to_string(), Some((0, 2)));
        assert_eq!(update2.data.data, "初中");
        assert_eq!(update2.data.range, Some((0, 2))); // preedit text spans 2 characters

        // compositionend event with final committed text
        let end_evt = CompositionEvent::end("初中文".to_string());
        assert_eq!(end_evt.event_type.as_str(), "compositionend");
        assert_eq!(end_evt.data.data, "初中文");
        assert_eq!(end_evt.data.range, None); // no range on final commit
    }

    #[test]
    fn composition_event_empty_data() {
        // Edge case: some IMEs send compositionstart with empty data
        let start_empty = CompositionEvent::start("".to_string(), Some("ja".to_string()));
        assert_eq!(start_empty.data.data, "");
        assert_eq!(start_empty.data.locale, Some("ja".to_string()));

        // compositionupdate may not have locale info
        let update = CompositionEvent::update("text".to_string(), Some((0, 4)));
        assert_eq!(update.data.locale, None);

        // compositionend may have empty data (commit cleared by IME)
        let end_empty = CompositionEvent::end("".to_string());
        assert_eq!(end_empty.data.data, "");
        assert_eq!(end_empty.data.range, None);
    }

    #[test]
    fn composition_multi_codepoint_range() {
        // Test range handling with multi-byte UTF-16 characters
        // Some characters (emoji, etc.) are 2 UTF-16 code units

        // surrogate pair emoji: 👍 = 2 UTF-16 code units
        let emoji_composition = CompositionEvent::update("👍".to_string(), Some((0, 2)));
        assert_eq!(emoji_composition.data.range, Some((0, 2)));

        // More complex: multiple characters with mixed widths
        let mixed = CompositionEvent::update("😀text😀".to_string(), Some((0, 6)));
        // emoji(2) + t(1) + e(1) + x(1) + t(1) + emoji(2) = 8 UTF-16 units
        assert_eq!(mixed.data.range, Some((0, 6)));
    }
}

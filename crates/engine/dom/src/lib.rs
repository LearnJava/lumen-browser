//! Arena-based DOM tree. Build via `Document::create_*` and `append_child`.

use std::collections::HashMap;
use std::fmt;

pub use lumen_core::sandbox::{parse_sandbox_value, SandboxFlags};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(i: usize) -> Self {
        NodeId(i as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
    Xml,
    XmlNs,
    XLink,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: QualName,
    pub value: String,
}

/// Shadow root mode per Shadow DOM spec §4.2.
///
/// `Open` — JS can access the shadow root via `element.shadowRoot`.
/// `Closed` — `element.shadowRoot` returns `null` (encapsulated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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

#[derive(Debug, Clone)]
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
    fn shadow_root_printed_in_display() {
        let (doc, _, _) = build_shadow_host();
        let s = doc.to_string();
        assert!(s.contains("#shadow-root (open)"));
    }
}

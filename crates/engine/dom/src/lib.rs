//! Arena-based DOM tree. Build via `Document::create_*` and `append_child`.

use std::fmt;

pub use lumen_core::sandbox::{parse_sandbox_value, SandboxFlags};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
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

#[derive(Debug, Clone)]
pub enum NodeData {
    Document,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
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

#[derive(Debug)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
    mode: DocumentMode,
    target_id: Option<String>,
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
    Ok(())
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
}

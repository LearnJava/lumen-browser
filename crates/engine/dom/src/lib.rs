//! Arena-based DOM tree. Build via `Document::create_*` and `append_child`.

use std::fmt;

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
}

#[derive(Debug)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
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
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
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
}

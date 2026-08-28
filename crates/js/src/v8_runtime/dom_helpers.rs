//! Приватные копии модуль-приватных помощников `dom.rs` (S3):
//! `find_element_by_tag`, `set_attribute`, сериализация/разбор фрагментов
//! HTML для `innerHTML`/`outerHTML`/`insertAdjacentHTML` (BUG-368, BUG-351),
//! Typed OM-ключи и трекер мутаций DOM (BUG-341 S7).
//!
//! Держим их здесь, а не расширяем видимость в `dom.rs`. Вынесено из
//! `v8_runtime.rs` батчем SPLIT-JS5; тип `HistoryState`, объявленный тем же
//! баннером, — в соседнем [`super::history_state`].

use super::*;

/// Mirrors `dom::cache_meta_method` — extract `"method"` from a cache meta JSON string.
pub(super) fn cache_meta_method(meta_json: &str) -> String {
    if let Some(start) = meta_json.find("\"method\":\"") {
        let rest = &meta_json[start + 10..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    "GET".to_string()
}

/// Mirrors `dom::_parse_style_string` — parse `"color: red; font-size: 12px"` into a map.
pub(super) fn _parse_style_string(css_text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for decl in css_text.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some((prop, val)) = decl.split_once(':') {
            map.insert(prop.trim().to_string(), val.trim().to_string());
        }
    }
    map
}

/// Mirrors `dom::_serialize_style_map` — serialize a style map back into CSS text.
pub(super) fn _serialize_style_map(map: &HashMap<String, String>) -> String {
    map.iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Mirrors `dom::_camel_to_kebab` — convert camelCase to kebab-case.
pub(super) fn _camel_to_kebab(prop: &str) -> String {
    let mut result = String::new();
    for (i, c) in prop.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

/// Property name a Typed OM / CSSOM lookup uses as the map key.
///
/// A CSS custom property (`--`-prefixed) is **case-sensitive** and is never
/// spelled camelCase, so it must reach the map verbatim: running it through
/// [`_camel_to_kebab`] turns `--Foo` into `---foo` and loses the declaration
/// (BUG-387). Everything else is an ASCII CSS property name that the Typed OM
/// accepts in either spelling, so it is folded to kebab-case.
pub(super) fn _css_property_key(prop: &str) -> String {
    if prop.starts_with("--") { prop.to_string() } else { _camel_to_kebab(prop) }
}

/// Serialises `[property, value]` pairs into the JSON array the Typed OM
/// iteration bindings (`_lumen_get_style_entries`,
/// `_lumen_get_computed_style_entries`) hand back to the JS shim.
///
/// Sorted by property name: both sources are `HashMap`s, so without this the
/// iteration order of `attributeStyleMap` / `computedStyleMap()` would differ
/// between runs of the same page.
pub(super) fn _style_entries_to_json(mut pairs: Vec<(String, String)>) -> String {
    pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    serde_json::to_string(&pairs).unwrap_or_else(|_| "[]".to_string())
}

/// Mirrors `dom::find_element_by_tag`.
pub(super) fn find_element_by_tag(doc: &lumen_dom::Document, tag: &str) -> Option<lumen_dom::NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

/// Mirrors `dom::namespace_uri`. DOM LS §4.9.1 `Node.namespaceURI` value for a
/// given `Namespace`. Backs `_lumen_get_namespace_uri` (BUG-281). `None` means
/// "no namespace" (`Namespace::None`, BUG-328) — callers must surface that as
/// JS `null`, not the empty string.
pub(super) fn namespace_uri(ns: Namespace) -> Option<&'static str> {
    match ns {
        Namespace::Html => Some("http://www.w3.org/1999/xhtml"),
        Namespace::Svg => Some("http://www.w3.org/2000/svg"),
        Namespace::MathMl => Some("http://www.w3.org/1998/Math/MathML"),
        Namespace::Xml => Some("http://www.w3.org/XML/1998/namespace"),
        Namespace::XmlNs => Some("http://www.w3.org/2000/xmlns/"),
        Namespace::XLink => Some("http://www.w3.org/1999/xlink"),
        Namespace::None => None,
    }
}

/// Mirrors `dom::find_first_matching`.
pub(super) fn find_first_matching(
    doc: &lumen_dom::Document,
    start: lumen_dom::NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
) -> Option<lumen_dom::NodeId> {
    let node = doc.get(start);
    if pred(node) {
        return Some(start);
    }
    for &child in &node.children.clone() {
        if let Some(found) = find_first_matching(doc, child, pred) {
            return Some(found);
        }
    }
    None
}

/// Mirrors `dom::collect_text_content`.
///
/// DOM §4.10 `CharacterData.data`/`Node.textContent` on a Comment node return
/// that node's own string verbatim, not a recursive descendant-Text
/// concatenation (a leaf Comment has no children anyway, but its own text
/// lives in `NodeData::Comment`, which `collect_text_inner` deliberately does
/// not match — `Node.textContent` on an *ancestor* element must skip comment
/// descendants entirely per spec, so that exclusion has to stay narrow to the
/// recursive case only).
pub(super) fn collect_text_content(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> String {
    if let lumen_dom::NodeData::Comment(s) = &doc.get(id).data {
        return s.clone();
    }
    let mut out = String::new();
    collect_text_inner(doc, id, &mut out);
    out
}

/// Mirrors `dom::collect_text_inner`.
pub(super) fn collect_text_inner(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    let node = doc.get(id);
    if let lumen_dom::NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children.clone() {
        collect_text_inner(doc, child, out);
    }
}

/// Является ли `nid` элементом `<template>` (HTML LS §4.12.3).
///
/// Отдельная проверка, а не наличие content-фрагмента: фрагмент у шаблона,
/// созданного из JS, появляется лениво, и «нет фрагмента» ещё не значит
/// «не шаблон».
pub(super) fn is_template_element(doc: &lumen_dom::Document, nid: NodeId) -> bool {
    matches!(&doc.get(nid).data, lumen_dom::NodeData::Element { name, .. } if name.local == "template")
}

/// BUG-341 S7: record `nid` as touched by a tracked DOM-mutation primitive.
pub(super) fn record_dom_touch(tracker: &Mutex<DomTouched>, nid: NodeId) {
    tracker.lock().unwrap_or_else(|e| e.into_inner()).nodes.insert(nid);
}

/// BUG-341 S7: mark this cycle's DOM mutations as unattributable — a mutation
/// happened through a primitive (`execCommand`, contenteditable editing,
/// Selection-driven range edits, Shadow DOM attachment) whose effect on which
/// nodes' selector-relevant state changed cannot be precisely determined.
/// Forces the page pipeline to fall back to a full cascade this cycle.
pub(super) fn record_dom_touch_unattributed(tracker: &Mutex<DomTouched>) {
    tracker.lock().unwrap_or_else(|e| e.into_inner()).unattributed = true;
}

/// Mirrors `dom::set_text_content`.
///
/// DOM §4.10 CharacterData nodes (Text/Comment) have no children, so setting
/// `.data`/`.textContent` must overwrite their own string in place. The
/// previous implementation always applied Element/Document "replace all
/// children with one Text node" semantics even when `id` itself was a leaf
/// Text/Comment node: it detached the (empty) children, then appended a
/// *new child* text node under `id` — leaving `id`'s own string untouched and
/// corrupting subsequent reads (`get_text_content` would return the stale
/// original string concatenated with the new child's). CharacterData.appendData
/// et al (WEB_API_SHIM `CharacterData.prototype`) all bottom out in this
/// setter via the `data` accessor, so this bug silently broke every write to
/// a native Text/Comment node's data.
pub(super) fn set_text_content(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, text: &str) {
    match &mut doc.get_mut(id).data {
        lumen_dom::NodeData::Text(s) | lumen_dom::NodeData::Comment(s) => {
            *s = text.to_string();
            return;
        }
        _ => {}
    }
    let children: Vec<lumen_dom::NodeId> = doc.get(id).children.clone();
    for child in children {
        doc.detach(child);
    }
    if !text.is_empty() {
        let text_node = doc.create_text(text);
        doc.append_child(id, text_node);
    }
}

/// Mirrors `dom::set_attribute`.
pub(super) fn set_attribute(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, name: &str, value: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        if let Some(attr) = attrs
            .iter_mut()
            .find(|a| a.name.local.eq_ignore_ascii_case(name))
        {
            attr.value = value.to_string();
        } else {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName::html(name.to_ascii_lowercase()),
                value: value.to_string(),
            });
        }
    }
}

/// Mirrors `dom::remove_attribute`.
pub(super) fn remove_attribute(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, name: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        attrs.retain(|a| !a.name.local.eq_ignore_ascii_case(name));
    }
}

// ── innerHTML/outerHTML/insertAdjacentHTML (BUG-368, BUG-351) ─────────────────

/// HTML LS §13.1.2 void elements — no content model, no closing tag when serialized.
pub(super) const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
    "source", "track", "wbr",
];

/// DOM Parsing §2.6 "fragment serializing algorithm" escaping for text nodes:
/// `&` and `<` (`>` is also escaped — mirrors the existing `_nativeSerializeNode`
/// JS-side convention in `dom_parser.rs`, harmless and slightly more defensive
/// than the spec's `&`/`<`/non-breaking-space-only minimum).
pub(super) fn escape_html_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// DOM Parsing §2.6 escaping for a double-quoted attribute value: `&` and `"`.
pub(super) fn escape_html_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

/// Serializes `id` itself — element open tag + attributes + children + close tag,
/// or the escaped data for a text/comment node. Mirrors HTML LS §13.3 "serializing
/// HTML fragments" run on a single node (used for `outerHTML`, BUG-351).
pub(super) fn serialize_node(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    match &doc.get(id).data {
        lumen_dom::NodeData::Text(s) => out.push_str(&escape_html_text(s)),
        lumen_dom::NodeData::Comment(s) => {
            out.push_str("<!--");
            out.push_str(s);
            out.push_str("-->");
        }
        lumen_dom::NodeData::Element { name, attrs } => {
            let tag = name.local.to_ascii_lowercase();
            out.push('<');
            out.push_str(&tag);
            for a in attrs {
                out.push(' ');
                out.push_str(&a.name.local);
                out.push_str("=\"");
                out.push_str(&escape_html_attr(&a.value));
                out.push('"');
            }
            out.push('>');
            if VOID_ELEMENTS.contains(&tag.as_str()) {
                return;
            }
            serialize_children(doc, id, out);
            out.push_str("</");
            out.push_str(&tag);
            out.push('>');
        }
        // Document/Doctype/ShadowRoot/DocumentFragment never appear as a regular
        // DOM child reachable from `innerHTML`/`outerHTML` — nothing to emit.
        _ => {}
    }
}

/// Serializes `id`'s children in tree order (used for `innerHTML`, BUG-368).
pub(super) fn serialize_children(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    for &child in &doc.get(id).children.clone() {
        serialize_node(doc, child, out);
    }
}

/// Recursively re-creates `src_id` (and its descendants) from the throwaway `src`
/// `Document` produced by `lumen_html_parser::parse` into the live `dst` document,
/// returning the new, still-detached node id. Node arenas are per-`Document`, so a
/// `NodeId` from `src` cannot simply be reused in `dst` — every node must be
/// recreated via `dst`'s own `create_*` calls.
pub(super) fn import_node(
    dst: &mut lumen_dom::Document,
    src: &lumen_dom::Document,
    src_id: lumen_dom::NodeId,
) -> lumen_dom::NodeId {
    let new_id = match &src.get(src_id).data {
        lumen_dom::NodeData::Element { name, attrs } => {
            let id = dst.create_element(name.clone());
            if let lumen_dom::NodeData::Element { attrs: dst_attrs, .. } = &mut dst.get_mut(id).data {
                *dst_attrs = attrs.clone();
            }
            id
        }
        lumen_dom::NodeData::Text(s) => dst.create_text(s.clone()),
        lumen_dom::NodeData::Comment(s) => dst.create_comment(s.clone()),
        // Doctype/Document/ShadowRoot/DocumentFragment cannot occur among a
        // parsed fragment's `<body>` children — fall back to an inert, unused
        // fragment node rather than panicking on an unreachable shape.
        _ => return dst.create_fragment(),
    };
    for &child in &src.get(src_id).children.clone() {
        let new_child = import_node(dst, src, child);
        dst.append_child(new_id, new_child);
    }
    new_id
}

/// Parses `html` as an HTML fragment and imports the result into `doc`, returning
/// the new, still-detached top-level node ids (the parsed document's `<body>`'s
/// direct children — `lumen_html_parser::parse` only exposes a full-document
/// parser, HTML LS §13.4 fragment-context tree-construction adjustments are not
/// implemented, matching the existing `Foreign content is not supported` gap noted
/// in `tree_builder.rs`, BUG-685).
pub(super) fn parse_html_fragment(doc: &mut lumen_dom::Document, html: &str) -> Vec<lumen_dom::NodeId> {
    let temp = lumen_html_parser::parse(html);
    let root = temp.body().unwrap_or_else(|| temp.root());
    temp.get(root)
        .children
        .clone()
        .into_iter()
        .map(|c| import_node(doc, &temp, c))
        .collect()
}

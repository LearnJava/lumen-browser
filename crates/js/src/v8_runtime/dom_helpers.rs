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
/// JS `null`, not the empty string. Owned, not `&'static str` (GAP-XMLDOC
/// срез 36): `Namespace::Other` carries an arbitrary URI, so this can no
/// longer borrow from a fixed set of string literals.
pub(super) fn namespace_uri(ns: &Namespace) -> Option<String> {
    ns.uri().map(str::to_string)
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
    match &doc.get(id).data {
        lumen_dom::NodeData::Comment(s) => return s.clone(),
        // GAP-XMLDOC срез 23: PI's `data`/`nodeValue`/`textContent` are all the
        // same field (DOM §4.5), same "own string verbatim" rule as Comment.
        lumen_dom::NodeData::ProcessingInstruction { data, .. } => return data.clone(),
        _ => {}
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
    let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
    t.nodes.insert(nid);
    t.epoch = t.epoch.wrapping_add(1);
}

/// BUG-341 S7: mark this cycle's DOM mutations as unattributable — a mutation
/// happened through a primitive (`execCommand`, contenteditable editing,
/// Selection-driven range edits, Shadow DOM attachment) whose effect on which
/// nodes' selector-relevant state changed cannot be precisely determined.
/// Forces the page pipeline to fall back to a full cascade this cycle.
pub(super) fn record_dom_touch_unattributed(tracker: &Mutex<DomTouched>) {
    let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
    t.unattributed = true;
    t.epoch = t.epoch.wrapping_add(1);
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
        lumen_dom::NodeData::ProcessingInstruction { data, .. } => {
            *data = text.to_string();
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

/// DOM §4.5 "validate and extract" namespace resolution, attribute-namespacing
/// slice (GAP-XMLDOC срез 10, BUG-685): the reverse of [`namespace_uri`],
/// restricted to the namespaces Lumen's closed `Namespace` enum can actually
/// represent. `None` means "not one of the namespaces Lumen tracks for
/// attributes" — distinct from `Namespace::Html`'s "definitely no namespace",
/// since a caller-supplied URI Lumen has no representation for is neither
/// (same BUG-830 "no general namespace registry yet" limitation as
/// `_lumen_create_element_ns`, applied to attributes rather than elements).
fn known_attribute_namespace(ns: Option<&str>) -> Option<lumen_dom::Namespace> {
    match ns? {
        "http://www.w3.org/1999/xlink" => Some(lumen_dom::Namespace::XLink),
        "http://www.w3.org/XML/1998/namespace" => Some(lumen_dom::Namespace::Xml),
        "http://www.w3.org/2000/xmlns/" => Some(lumen_dom::Namespace::XmlNs),
        "http://www.w3.org/2000/svg" => Some(lumen_dom::Namespace::Svg),
        "http://www.w3.org/1998/Math/MathML" => Some(lumen_dom::Namespace::MathMl),
        _ => None,
    }
}

/// `setAttributeNS`'s namespace resolution — like [`known_attribute_namespace`],
/// but `null`/empty falls back to `Namespace::Html` instead of "unknown",
/// matching every already-existing plain attribute (a deliberate deviation
/// from DOM §4.5, which would say "no namespace" — kept for compatibility
/// with the rest of the plain-attribute model, same tradeoff `known_attribute_namespace`
/// documents). An unrecognized but non-empty `ns`, previously collapsed into
/// that same `Html` fallback, now round-trips verbatim via [`lumen_dom::Namespace::Other`]
/// instead of being silently discarded (GAP-XMLDOC срез 37, BUG-685/BUG-830) —
/// the attribute-side half of the element-creation fix `_lumen_create_element_ns`
/// already got in срез 36.
pub(super) fn resolve_attribute_namespace(ns: Option<&str>) -> lumen_dom::Namespace {
    match ns {
        None | Some("") => lumen_dom::Namespace::Html,
        Some(uri) => lumen_dom::Namespace::from_uri(Some(uri)),
    }
}

/// `getAttributeNS`/`hasAttributeNS`/`removeAttributeNS` (GAP-XMLDOC срез 10,
/// BUG-685, BUG-309): finds the stored qualified name of the attribute whose
/// namespace URI is `ns` and whose local name (the qualified name's suffix
/// after the last `:`, or the whole name if there is none) is `local_name`.
/// `ns` of `None`/empty/unrecognized falls back to a plain by-name lookup —
/// the DOM standard's "no namespace" case for the first two, and (BUG-309,
/// BUG-830) the best Lumen can do for a namespace URI it has no
/// representation for, matching pre-срез-10 behavior for that case rather
/// than newly reporting "not found" for every attribute set through it.
pub(super) fn find_attr_by_namespace(
    doc: &lumen_dom::Document,
    id: lumen_dom::NodeId,
    ns: Option<&str>,
    local_name: &str,
) -> Option<String> {
    let lumen_dom::NodeData::Element { attrs, .. } = &doc.get(id).data else {
        return None;
    };
    match known_attribute_namespace(ns) {
        None => attrs
            .iter()
            .find(|a| a.name.local == local_name)
            .map(|a| a.name.local.clone()),
        Some(known) => attrs
            .iter()
            .find(|a| {
                a.name.namespace == known && a.name.local.rsplit(':').next() == Some(local_name)
            })
            .map(|a| a.name.local.clone()),
    }
}

/// `setAttributeNS(namespace, qualifiedName, value)` (GAP-XMLDOC срез 10,
/// BUG-685, BUG-309): unlike [`set_attribute`], tags the attribute with the
/// namespace resolved from `ns` instead of always `Html`. An existing
/// attribute is matched by its stored qualified name (same identity model as
/// every other attribute accessor here — Lumen has no separate prefix field),
/// and has its namespace corrected too, since the whole point of calling the
/// `NS` form is to declare one.
pub(super) fn set_attribute_ns(
    doc: &mut lumen_dom::Document,
    id: lumen_dom::NodeId,
    ns: Option<&str>,
    qualified_name: &str,
    value: &str,
) {
    let namespace = resolve_attribute_namespace(ns);
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        if let Some(attr) = attrs
            .iter_mut()
            .find(|a| a.name.local.eq_ignore_ascii_case(qualified_name))
        {
            attr.name.namespace = namespace;
            attr.value = value.to_string();
        } else {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName {
                    namespace,
                    local: qualified_name.to_string(),
                },
                value: value.to_string(),
            });
        }
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

/// HTML LS §13.3 "serializing HTML fragments": a text node whose parent is one of
/// these HTML elements is emitted literally, not escaped. `noscript` is included
/// because scripting is always enabled in Lumen. BUG-1132: pages that stash inline
/// script bodies in `<script type=text/…>` and re-run them via `innerHTML` got
/// `&amp;&amp;`/`&lt;` back and hit a SyntaxError.
const RAW_TEXT_PARENTS: &[&str] = &[
    "style", "script", "xmp", "iframe", "noembed", "noframes", "plaintext", "noscript",
];

fn parent_is_raw_text(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> bool {
    doc.get(id).parent.is_some_and(|p| match &doc.get(p).data {
        lumen_dom::NodeData::Element { name, .. } => {
            name.namespace == lumen_dom::Namespace::Html
                && RAW_TEXT_PARENTS.iter().any(|t| name.local.eq_ignore_ascii_case(t))
        }
        _ => false,
    })
}

/// Serializes `id` itself — element open tag + attributes + children + close tag,
/// or the escaped data for a text/comment node. Mirrors HTML LS §13.3 "serializing
/// HTML fragments" run on a single node (used for `outerHTML`, BUG-351).
///
/// BUG-1028: explicit heap stack instead of native recursion — same safe
/// mechanical class LAYOUT-1/2 converted, but two-phase (an element's close
/// tag must be emitted only after all of its descendants), so each opened
/// element pushes its own `Frame::Close` marker *before* its children, popped
/// once every descendant has already been emitted (LIFO, children pushed in
/// reverse to preserve document order).
pub(super) fn serialize_node(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    enum Frame {
        Open(lumen_dom::NodeId),
        Close(String),
    }
    let mut stack = vec![Frame::Open(id)];
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Open(id) => match &doc.get(id).data {
                lumen_dom::NodeData::Text(s) if parent_is_raw_text(doc, id) => out.push_str(s),
                lumen_dom::NodeData::Text(s) => out.push_str(&escape_html_text(s)),
                lumen_dom::NodeData::Comment(s) => {
                    out.push_str("<!--");
                    out.push_str(s);
                    out.push_str("-->");
                }
                lumen_dom::NodeData::ProcessingInstruction { target, data } => {
                    out.push_str("<?");
                    out.push_str(target);
                    if !data.is_empty() {
                        out.push(' ');
                        out.push_str(data);
                    }
                    out.push('?');
                    out.push('>');
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
                        continue;
                    }
                    stack.push(Frame::Close(tag));
                    for &child in doc.get(id).children.iter().rev() {
                        stack.push(Frame::Open(child));
                    }
                }
                // Document/Doctype/ShadowRoot/DocumentFragment never appear as a
                // regular DOM child reachable from `innerHTML`/`outerHTML` —
                // nothing to emit.
                _ => {}
            },
            Frame::Close(tag) => {
                out.push_str("</");
                out.push_str(&tag);
                out.push('>');
            }
        }
    }
}

/// Serializes `id`'s children in tree order (used for `innerHTML`, BUG-368).
pub(super) fn serialize_children(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
    for &child in &doc.get(id).children.clone() {
        serialize_node(doc, child, out);
    }
}

/// Re-creates `src_id` (and its descendants) from the throwaway `src`
/// `Document` produced by `lumen_html_parser::parse` into the live `dst` document,
/// returning the new, still-detached node id. Node arenas are per-`Document`, so a
/// `NodeId` from `src` cannot simply be reused in `dst` — every node must be
/// recreated via `dst`'s own `create_*` calls.
///
/// BUG-1028: explicit heap stack instead of native recursion — one frame per
/// level of the fragment being assigned used to be one `import_node` call on
/// the native stack, unbounded by anything the caller controls (a
/// `<script>`-built `innerHTML` fragment can nest arbitrarily deep). Pure
/// pre-order with nothing read back from a child beyond its own new id (which
/// goes straight into `append_child`) — the same safe mechanical class
/// LAYOUT-1 converted. `clone_one` creates a node without attaching it;
/// the stack carries `(src_id, dst_parent)` pairs, LIFO with children pushed
/// in reverse to preserve document order.
pub(super) fn import_node(
    dst: &mut lumen_dom::Document,
    src: &lumen_dom::Document,
    src_id: lumen_dom::NodeId,
) -> lumen_dom::NodeId {
    // `(new_id, has_children_to_import)` — the fallback arm mirrors the old
    // early `return` for a node type that cannot carry importable children
    // (Doctype/Document/ShadowRoot/DocumentFragment): its own descendants,
    // if any, must not be walked into the stack below.
    fn clone_one(
        dst: &mut lumen_dom::Document,
        src: &lumen_dom::Document,
        src_id: lumen_dom::NodeId,
    ) -> (lumen_dom::NodeId, bool) {
        match &src.get(src_id).data {
            lumen_dom::NodeData::Element { name, attrs } => {
                let id = dst.create_element(name.clone());
                if let lumen_dom::NodeData::Element { attrs: dst_attrs, .. } = &mut dst.get_mut(id).data {
                    *dst_attrs = attrs.clone();
                }
                (id, true)
            }
            lumen_dom::NodeData::Text(s) => (dst.create_text(s.clone()), true),
            lumen_dom::NodeData::Comment(s) => (dst.create_comment(s.clone()), true),
            lumen_dom::NodeData::ProcessingInstruction { target, data } => {
                (dst.create_processing_instruction(target.clone(), data.clone()), true)
            }
            // Doctype/Document/ShadowRoot/DocumentFragment cannot occur among a
            // parsed fragment's top-level children (`in body` ignores a DOCTYPE
            // token) — fall back to an inert, unused fragment node rather than
            // panicking on an unreachable shape.
            _ => (dst.create_fragment(), false),
        }
    }

    let (root_new_id, root_has_children) = clone_one(dst, src, src_id);
    let mut stack: Vec<(lumen_dom::NodeId, lumen_dom::NodeId)> = if root_has_children {
        src.get(src_id)
            .children
            .iter()
            .rev()
            .map(|&child| (child, root_new_id))
            .collect()
    } else {
        Vec::new()
    };
    while let Some((id, new_parent)) = stack.pop() {
        let (new_id, has_children) = clone_one(dst, src, id);
        dst.append_child(new_parent, new_id);
        if has_children {
            stack.extend(src.get(id).children.iter().rev().map(|&child| (child, new_id)));
        }
    }
    root_new_id
}

/// Parses `html` as an HTML fragment and imports the result into `doc`, returning
/// the new, still-detached top-level node ids.
///
/// BUG-982: goes through `lumen_html_parser::parse_fragment` (HTML LS §13.4
/// entry point), not the document parser plus «take `<body>`'s children». The
/// document parser starts in `initial`, where §13.2.6.4.1–4 *must* drop a
/// leading whitespace run and *must* put a leading comment on the `Document`
/// itself — both then fell outside `<body>` and were silently lost, which is
/// what broke React 18's `<!--$-->` Suspense markers. Context element support
/// (a bare `<td>`, an SVG subtree) is [`parse_html_fragment_with_context`] —
/// this always parses at body level, same as before GAP-XMLDOC срез 14.
pub(super) fn parse_html_fragment(doc: &mut lumen_dom::Document, html: &str) -> Vec<lumen_dom::NodeId> {
    parse_html_fragment_with_context(doc, html, None)
}

/// Same as [`parse_html_fragment`], but with a real HTML LS §13.4 context
/// element (GAP-XMLDOC срез 14, BUG-685) — `context_nid`'s namespace/local
/// name/attributes decide the "adjusted current node" the fragment parser
/// uses for foreign-content routing and the CDATA-allowed flag while its
/// synthetic root is still the only element on the stack. `_lumen_set_inner_html`
/// is the one caller that has a real context element to offer
/// (`Element.innerHTML=`'s `this`); `outerHTML`/`insertAdjacentHTML` don't
/// (their context is the target's *parent*, not measured yet), so they keep
/// going through the `None` path above.
pub(super) fn parse_html_fragment_with_context(
    doc: &mut lumen_dom::Document,
    html: &str,
    context_nid: Option<lumen_dom::NodeId>,
) -> Vec<lumen_dom::NodeId> {
    let context = context_nid.and_then(|nid| match &doc.get(nid).data {
        lumen_dom::NodeData::Element { name, attrs } => Some(lumen_html_parser::FragmentContext {
            namespace: name.namespace.clone(),
            local: name.local.clone(),
            attrs: attrs.iter().map(|a| (a.name.local.clone(), a.value.clone())).collect(),
            // GAP-XMLDOC срез 39 (BUG-685): the context element's own `xmlns`
            // (if any) or its nearest real ancestor's — the fragment parser's
            // own ancestor walk can never see past its synthetic root into
            // *this* `doc`, since the context element never joins the
            // fragment's tree (see `FragmentContext`'s struct doc).
            default_namespace: doc.nearest_xmlns_default(nid),
        }),
        _ => None,
    });
    let (temp, root) = lumen_html_parser::parse_fragment_with_context(html, context);
    temp.get(root)
        .children
        .clone()
        .into_iter()
        .map(|c| import_node(doc, &temp, c))
        .collect()
}

#[cfg(test)]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]

    use super::*;

    // BUG-1028: `import_node`/`serialize_node` used to cost one native stack
    // frame per level of DOM depth — the same safe mechanical class LAYOUT-1
    // converted (`selector_query.rs::find_first_dom_node_by_selector_deep_chain_does_not_overflow_the_stack`),
    // which uses 200_000 for its own chain. Deliberately smaller here: this
    // module's chain construction goes through `Document::append_child`,
    // whose `is_self_or_ancestor` cycle-check debug_assert walks every
    // ancestor on each call, making chain-building O(depth²) under the
    // `debug_assertions` a normal `cargo test` build carries. 20_000 is
    // still 2x the
    // ~10_000-level ceiling BUG-1028 itself measured live on the (much
    // larger-framed) `lumen-v8` thread post-BUG-1027, comfortably beyond
    // what a bare recursive Rust function could reach on any real thread
    // stack — while keeping this pair of tests in the single-digit seconds
    // instead of the ~15 minutes 200_000 measured at here. Built directly
    // via `Document`'s arena API, not `lumen_html_parser::parse`, since
    // parsing a matching HTML string would additionally exercise the
    // parser's own recursion, outside this test's scope.
    const DEEP_CHAIN_DEPTH: usize = 20_000;

    #[test]
    fn import_node_deep_chain_does_not_overflow_the_stack() {
        let mut src = lumen_dom::Document::new();
        let mut parent = src.root();
        for _ in 0..DEEP_CHAIN_DEPTH {
            let div = src.create_element(lumen_dom::QualName::html("div"));
            src.append_child(parent, div);
            parent = div;
        }
        let leaf = src.create_element(lumen_dom::QualName::html("span"));
        src.append_child(parent, leaf);

        let root_children = src.get(src.root()).children.clone();
        assert_eq!(root_children.len(), 1);

        let mut dst = lumen_dom::Document::new();
        let new_root = import_node(&mut dst, &src, root_children[0]);

        // Walk the imported chain iteratively (no native recursion in the
        // test itself) to confirm every level made it across intact.
        let mut cur = new_root;
        let mut div_count = 0usize;
        loop {
            match &dst.get(cur).data {
                lumen_dom::NodeData::Element { name, .. } if name.local == "div" => {
                    div_count += 1;
                }
                lumen_dom::NodeData::Element { name, .. } if name.local == "span" => break,
                other => panic!("unexpected node in imported chain: {other:?}"),
            }
            let children = dst.get(cur).children.clone();
            assert_eq!(children.len(), 1, "expected a single-child chain");
            cur = children[0];
        }
        assert_eq!(div_count, DEEP_CHAIN_DEPTH);
    }

    #[test]
    fn serialize_node_deep_chain_does_not_overflow_the_stack() {
        let mut doc = lumen_dom::Document::new();
        let mut parent = doc.root();
        for _ in 0..DEEP_CHAIN_DEPTH {
            let div = doc.create_element(lumen_dom::QualName::html("div"));
            doc.append_child(parent, div);
            parent = div;
        }
        let root_children = doc.get(doc.root()).children.clone();
        assert_eq!(root_children.len(), 1);

        let mut out = String::new();
        serialize_node(&doc, root_children[0], &mut out);

        assert_eq!(out.matches("<div>").count(), DEEP_CHAIN_DEPTH);
        assert_eq!(out.matches("</div>").count(), DEEP_CHAIN_DEPTH);
    }

    // BUG-1132: текст внутри `<script>`/`<style>` сериализуется как есть,
    // в обычном элементе и в SVG-`<style>` — экранируется.
    #[test]
    fn serialize_raw_text_parents_emit_text_verbatim() {
        let mut doc = lumen_dom::Document::new();
        let root = doc.root();
        let body = "if (a < 3 && b > 1) x = '&amp;';";
        let mut ser = |parent: lumen_dom::QualName| {
            let el = doc.create_element(parent);
            doc.append_child(root, el);
            let t = doc.create_text(body.to_string());
            doc.append_child(el, t);
            let mut out = String::new();
            serialize_children(&doc, el, &mut out);
            out
        };
        assert_eq!(ser(lumen_dom::QualName::html("script")), body);
        assert_eq!(ser(lumen_dom::QualName::html("STYLE")), body);
        assert_eq!(ser(lumen_dom::QualName::html("noscript")), body);
        assert_eq!(
            ser(lumen_dom::QualName::html("div")),
            "if (a &lt; 3 &amp;&amp; b &gt; 1) x = '&amp;amp;';"
        );
        let svg_style = lumen_dom::QualName {
            namespace: lumen_dom::Namespace::Svg,
            local: "style".into(),
        };
        assert_ne!(ser(svg_style), body);
    }

    /// Верхний уровень результата `parse_html_fragment` в компактной записи —
    /// проверяем не только форму дерева парсера, но и то, что перенос в живой
    /// документ ничего не роняет по дороге.
    fn imported(html: &str) -> Vec<String> {
        let mut doc = lumen_dom::Document::new();
        let ids = parse_html_fragment(&mut doc, html);
        ids.into_iter()
            .map(|id| match &doc.get(id).data {
                lumen_dom::NodeData::Element { name, .. } => format!("<{}>", name.local),
                lumen_dom::NodeData::Text(s) => format!("#text{s:?}"),
                lumen_dom::NodeData::Comment(s) => format!("#comment{s:?}"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    // BUG-982: ведущий пробел и ведущий комментарий фрагмента доходят до
    // живого документа. Раньше `parse_html_fragment` брал детей `<body>`
    // документного разбора, а туда ни тот ни другой узел не попадал:
    // whitespace-only токен в `initial` игнорируется, comment-токен уходит
    // на сам `Document`. На втором держалась гидрация React 18 — его маркеры
    // Suspense `<!--$-->` стоят в начале фрагмента.
    #[test]
    fn parse_html_fragment_keeps_leading_whitespace_and_comments() {
        assert_eq!(imported(" abc"), ["#text\" abc\""]);
        assert_eq!(imported(" "), ["#text\" \""]);
        assert_eq!(imported("<!--$-->x"), ["#comment\"$\"", "#text\"x\""]);
        assert_eq!(imported("<div>d</div>"), ["<div>"]);
    }
}

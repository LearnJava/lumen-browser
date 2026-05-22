//! JS↔DOM bridge for lumen-js.
//!
//! Registers `_lumen_*` native Rust functions in a QuickJS context, then
//! evaluates the `WEB_API_SHIM` JavaScript that builds standard `document`,
//! `window`, `console` globals on top of those primitives.
//!
//! Phase 0 selector support: `#id`, `.class`, `tagname`, `*`.
//! Compound selectors (e.g. `div.foo`) are not supported in Phase 0.

use std::sync::{Arc, Mutex};

use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};
use rquickjs::{Ctx, Function, Result as QjResult};

// ─── history state ───────────────────────────────────────────────────────────

struct HistoryEntry {
    state_json: String,
    url: String,
}

struct HistoryState {
    entries: Vec<HistoryEntry>,
    current: usize,
}

impl HistoryState {
    fn new() -> Self {
        Self {
            entries: vec![HistoryEntry {
                state_json: "null".into(),
                url: String::new(),
            }],
            current: 0,
        }
    }

    fn push(&mut self, state_json: String, url: String) {
        self.entries.truncate(self.current + 1);
        self.entries.push(HistoryEntry { state_json, url });
        self.current = self.entries.len() - 1;
    }

    fn replace(&mut self, state_json: String, url: String) {
        if let Some(e) = self.entries.get_mut(self.current) {
            e.state_json = state_json;
            e.url = url;
        }
    }

    // Returns false when delta is 0 (Phase 0: reload not implemented) or out of bounds.
    fn go(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let new_idx = self.current as i64 + i64::from(delta);
        if new_idx < 0 || new_idx >= self.entries.len() as i64 {
            return false;
        }
        self.current = new_idx as usize;
        true
    }

    fn state_json(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.state_json.as_str())
            .unwrap_or("null")
    }

    fn url(&self) -> &str {
        self.entries
            .get(self.current)
            .map(|e| e.url.as_str())
            .unwrap_or("")
    }

    fn length(&self) -> u32 {
        self.entries.len() as u32
    }
}

// ─── public entry point ───────────────────────────────────────────────────────

/// Install DOM primitives (`_lumen_*`) and the Web API shim into `ctx`.
///
/// After this call the context exposes `console`, `document`, `window`,
/// `location`, `navigator`, and `alert`.
pub fn install_dom_api(ctx: &Ctx<'_>, doc: Arc<Mutex<Document>>) -> QjResult<()> {
    install_primitives(ctx, Arc::clone(&doc))?;
    ctx.eval::<(), _>(WEB_API_SHIM)?;
    Ok(())
}

// ─── primitive registrations ──────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn install_primitives(ctx: &Ctx<'_>, doc: Arc<Mutex<Document>>) -> QjResult<()> {
    macro_rules! reg {
        ($name:expr, $f:expr) => {
            ctx.globals()
                .set($name, Function::new(ctx.clone(), $f)?)?;
        };
    }

    // ── console ──────────────────────────────────────────────────────────────
    {
        reg!("_lumen_console_log", |msg: String| {
            eprintln!("[JS] {msg}");
        });
        reg!("_lumen_console_warn", |msg: String| {
            eprintln!("[JS warn] {msg}");
        });
        reg!("_lumen_console_error", |msg: String| {
            eprintln!("[JS error] {msg}");
        });
    }

    // ── document meta ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_document_root", move || -> u32 {
            let doc = d.lock().unwrap();
            doc.root().index() as u32
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_get_body", move || -> Option<u32> {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "body").map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_get_document_title", move || -> String {
            let doc = d.lock().unwrap();
            find_element_by_tag(&doc, "title")
                .map(|nid| collect_text_content(&doc, nid))
                .unwrap_or_default()
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_set_document_title", move |text: String| {
            let mut doc = d.lock().unwrap();
            if let Some(title_id) = find_element_by_tag(&doc, "title") {
                set_text_content(&mut doc, title_id, &text);
            }
        });
    }

    // ── node lookup ──────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_element_by_id", move |id: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            find_first_matching(&doc, doc.root(), &|node| {
                matches!(&node.data, NodeData::Element { .. })
                    && node.get_attr("id") == Some(id.as_str())
            })
            .map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!("_lumen_query_selector", move |sel: String| -> Option<u32> {
            let doc = d.lock().unwrap();
            find_first_matching(&doc, doc.root(), &|node| selector_matches(node, &sel))
                .map(|n| n.index() as u32)
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_query_selector_all",
            move |sel: String| -> Vec<u32> {
                let doc = d.lock().unwrap();
                let mut out = Vec::new();
                collect_matching(&doc, doc.root(), &|node| selector_matches(node, &sel), &mut out);
                out.into_iter().map(|n| n.index() as u32).collect()
            }
        );
    }

    // ── node properties ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!("_lumen_get_tag_name", move |node_id: u32| -> String {
            let doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            match &doc.get(nid).data {
                NodeData::Element { name, .. } => name.local.to_ascii_uppercase(),
                NodeData::Text(_) => "#text".into(),
                NodeData::Document => "#document".into(),
                NodeData::Comment(_) => "#comment".into(),
                NodeData::Doctype { .. } => "html".into(),
            }
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_is_text_node",
            move |node_id: u32| -> bool {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                matches!(doc.get(nid).data, NodeData::Text(_))
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_attr",
            move |node_id: u32, name: String| -> Option<String> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).get_attr(&name).map(|s| s.to_string())
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_set_attr",
            move |node_id: u32, name: String, value: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_attribute(&mut doc, nid, &name, &value);
            }
        );
        let d = Arc::clone(&doc);
        reg!("_lumen_remove_attr", move |node_id: u32, name: String| {
            let mut doc = d.lock().unwrap();
            let nid = NodeId::from_index(node_id as usize);
            remove_attribute(&mut doc, nid, &name);
        });
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_text_content",
            move |node_id: u32| -> String {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                collect_text_content(&doc, nid)
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_set_text_content",
            move |node_id: u32, text: String| {
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_text_content(&mut doc, nid, &text);
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_inner_html",
            move |node_id: u32| -> String {
                // Phase 0: return text content only (no HTML serialization).
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                collect_text_content(&doc, nid)
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_set_inner_html",
            move |node_id: u32, html: String| {
                // Phase 0: treat innerHTML as plain text (no fragment parsing).
                let mut doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                set_text_content(&mut doc, nid, &html);
            }
        );
    }

    // ── tree navigation ──────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_children",
            move |node_id: u32| -> Vec<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid)
                    .children
                    .iter()
                    .map(|c| c.index() as u32)
                    .collect()
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_get_parent",
            move |node_id: u32| -> Option<u32> {
                let doc = d.lock().unwrap();
                let nid = NodeId::from_index(node_id as usize);
                doc.get(nid).parent.map(|p| p.index() as u32)
            }
        );
    }

    // ── tree mutation ────────────────────────────────────────────────────────
    {
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_create_element",
            move |tag: String| -> u32 {
                let mut doc = d.lock().unwrap();
                let nid = doc.create_element(QualName::html(tag.to_ascii_lowercase()));
                nid.index() as u32
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_create_text_node",
            move |text: String| -> u32 {
                let mut doc = d.lock().unwrap();
                let nid = doc.create_text(text);
                nid.index() as u32
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_append_child",
            move |parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let parent = NodeId::from_index(parent_id as usize);
                let child = NodeId::from_index(child_id as usize);
                doc.append_child(parent, child);
            }
        );
        let d = Arc::clone(&doc);
        reg!(
            "_lumen_remove_child",
            move |_parent_id: u32, child_id: u32| {
                let mut doc = d.lock().unwrap();
                let child = NodeId::from_index(child_id as usize);
                doc.detach(child);
            }
        );
    }

    // ── Service Worker / Cache Storage (in-memory scaffold) ─────────────────
    {
        // SW registrations: origin+scope+scriptUrl stored in-memory.
        // Key: (origin, scope) → script_url
        type SwMap = std::collections::HashMap<(String, String), String>;
        let sw_regs: Arc<Mutex<SwMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        // Cache storage: origin → cache_name → url → body (Vec<u8>)
        type CacheMap = std::collections::HashMap<String, std::collections::HashMap<String, std::collections::HashMap<String, Vec<u8>>>>;
        let cache_data: Arc<Mutex<CacheMap>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_register",
            move |origin: String, scope: String, script_url: String| {
                sw.lock().unwrap().insert((origin, scope), script_url);
            }
        );

        let sw = Arc::clone(&sw_regs);
        reg!(
            "_lumen_sw_has_registration",
            move |origin: String| -> bool {
                sw.lock().unwrap().keys().any(|(o, _)| *o == origin)
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_put",
            move |origin: String, cache_name: String, url: String, body: Vec<u8>| {
                cd.lock()
                    .unwrap()
                    .entry(origin)
                    .or_default()
                    .entry(cache_name)
                    .or_default()
                    .insert(url, body);
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match",
            move |origin: String, cache_name: String, url: String| -> Option<Vec<u8>> {
                cd.lock()
                    .unwrap()
                    .get(&origin)
                    .and_then(|caches| caches.get(&cache_name))
                    .and_then(|cache| cache.get(&url))
                    .cloned()
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_match_any",
            move |origin: String, url: String| -> Option<Vec<u8>> {
                let guard = cd.lock().unwrap();
                let caches = guard.get(&origin)?;
                for cache in caches.values() {
                    if let Some(body) = cache.get(&url) {
                        return Some(body.clone());
                    }
                }
                None
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_delete",
            move |origin: String, cache_name: String, url: String| {
                if let Some(caches) = cd.lock().unwrap().get_mut(&origin)
                    && let Some(cache) = caches.get_mut(&cache_name)
                {
                    cache.remove(&url);
                }
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_keys",
            move |origin: String, cache_name: String| -> Vec<String> {
                cd.lock()
                    .unwrap()
                    .get(&origin)
                    .and_then(|caches| caches.get(&cache_name))
                    .map(|cache| cache.keys().cloned().collect())
                    .unwrap_or_default()
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_has",
            move |origin: String, cache_name: String| -> bool {
                cd.lock()
                    .unwrap()
                    .get(&origin)
                    .map(|caches| caches.contains_key(&cache_name))
                    .unwrap_or(false)
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_delete_cache",
            move |origin: String, cache_name: String| {
                if let Some(caches) = cd.lock().unwrap().get_mut(&origin) {
                    caches.remove(&cache_name);
                }
            }
        );

        let cd = Arc::clone(&cache_data);
        reg!(
            "_lumen_cache_names",
            move |origin: String| -> Vec<String> {
                cd.lock()
                    .unwrap()
                    .get(&origin)
                    .map(|caches| caches.keys().cloned().collect())
                    .unwrap_or_default()
            }
        );
    }

    // ── history ──────────────────────────────────────────────────────────────
    {
        let hist = Arc::new(Mutex::new(HistoryState::new()));

        let h = Arc::clone(&hist);
        reg!(
            "_lumen_history_push",
            move |state_json: String, url: String| {
                h.lock().unwrap().push(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!(
            "_lumen_history_replace",
            move |state_json: String, url: String| {
                h.lock().unwrap().replace(state_json, url);
            }
        );

        let h = Arc::clone(&hist);
        reg!("_lumen_history_go", move |delta: i32| -> bool {
            h.lock().unwrap().go(delta)
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_length", move || -> u32 {
            h.lock().unwrap().length()
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_state_json", move || -> String {
            h.lock().unwrap().state_json().to_string()
        });

        let h = Arc::clone(&hist);
        reg!("_lumen_history_url", move || -> String {
            h.lock().unwrap().url().to_string()
        });
    }

    Ok(())
}

// ─── DOM helpers ──────────────────────────────────────────────────────────────

fn find_element_by_tag(doc: &Document, tag: &str) -> Option<NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

fn find_first_matching(
    doc: &Document,
    start: NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
) -> Option<NodeId> {
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

fn collect_matching(
    doc: &Document,
    start: NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
    out: &mut Vec<NodeId>,
) {
    let node = doc.get(start);
    if pred(node) {
        out.push(start);
    }
    for &child in &node.children.clone() {
        collect_matching(doc, child, pred, out);
    }
}

/// Phase 0 selector matching: `#id`, `.class`, `tagname`, `*`.
fn selector_matches(node: &lumen_dom::Node, selector: &str) -> bool {
    let NodeData::Element { name, .. } = &node.data else {
        return false;
    };
    let sel = selector.trim();
    if let Some(id) = sel.strip_prefix('#') {
        node.get_attr("id") == Some(id)
    } else if let Some(cls) = sel.strip_prefix('.') {
        has_class(node, cls)
    } else if sel == "*" {
        true
    } else {
        name.local.eq_ignore_ascii_case(sel)
    }
}

fn has_class(node: &lumen_dom::Node, cls: &str) -> bool {
    node.get_attr("class")
        .map(|c| c.split_ascii_whitespace().any(|t| t == cls))
        .unwrap_or(false)
}

fn collect_text_content(doc: &Document, id: NodeId) -> String {
    let mut out = String::new();
    collect_text_inner(doc, id, &mut out);
    out
}

fn collect_text_inner(doc: &Document, id: NodeId, out: &mut String) {
    let node = doc.get(id);
    if let NodeData::Text(s) = &node.data {
        out.push_str(s);
    }
    for &child in &node.children.clone() {
        collect_text_inner(doc, child, out);
    }
}

fn set_text_content(doc: &mut Document, id: NodeId, text: &str) {
    let children: Vec<NodeId> = doc.get(id).children.clone();
    for child in children {
        doc.detach(child);
    }
    if !text.is_empty() {
        let text_node = doc.create_text(text);
        doc.append_child(id, text_node);
    }
}

fn set_attribute(doc: &mut Document, id: NodeId, name: &str, value: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        if let Some(attr) = attrs
            .iter_mut()
            .find(|a| a.name.local.eq_ignore_ascii_case(name))
        {
            attr.value = value.to_string();
        } else {
            attrs.push(Attribute {
                name: QualName::html(name.to_ascii_lowercase()),
                value: value.to_string(),
            });
        }
    }
}

fn remove_attribute(doc: &mut Document, id: NodeId, name: &str) {
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        attrs.retain(|a| !a.name.local.eq_ignore_ascii_case(name));
    }
}

// ─── JavaScript Web API shim ──────────────────────────────────────────────────

/// Evaluated once after primitives are registered; builds standard Web API globals.
///
/// Uses top-level `var` so declarations land on the global object in QuickJS's
/// script eval. No IIFE — avoids strict-mode `this`-is-undefined edge cases.
///
/// `parentElement` and `children` are defined as non-enumerable via
/// `Object.defineProperty` to prevent `from_rq` from calling them during
/// object serialization — they can cause parent↔child infinite recursion.
/// Evaluated once after primitives are registered; builds standard Web API globals.
///
/// `Option<T>` in rquickjs maps `None → undefined`, not `null`. All places
/// where the Web API spec requires `null` use `_lumen_u2n` (undefined-to-null).
///
/// `parentElement` and `children` are non-enumerable to prevent infinite
/// recursion when `from_rq` serializes the returned object (parent↔child cycles).
const WEB_API_SHIM: &str = "
function _lumen_u2n(v) { return v !== undefined ? v : null; }

// ── Event / CustomEvent constructors ─────────────────────────────────────────

function Event(type, init) {
    this.type             = String(type || '');
    this.bubbles          = !!(init && init.bubbles);
    this.cancelable       = !!(init && init.cancelable);
    this.defaultPrevented = false;
    this.target           = null;
    this.currentTarget    = null;
    this.timeStamp        = Date.now ? Date.now() : 0;
    this._stopImmediate   = false;
}
Event.prototype.preventDefault = function() {
    if (this.cancelable) this.defaultPrevented = true;
};
Event.prototype.stopPropagation = function() {};
Event.prototype.stopImmediatePropagation = function() { this._stopImmediate = true; };

function CustomEvent(type, init) {
    Event.call(this, type, init);
    this.detail = (init && init.detail !== undefined) ? init.detail : null;
}
CustomEvent.prototype = Object.create(Event.prototype);
CustomEvent.prototype.constructor = CustomEvent;

// ── Per-element event listener store ─────────────────────────────────────────
// Key: String(nid) + ':' + type  →  Array of handler functions.

var _lumen_listeners = {};

function _lumen_add_listener(nid, type, fn) {
    if (typeof fn !== 'function') return;
    var key = String(nid) + ':' + String(type);
    if (!_lumen_listeners[key]) _lumen_listeners[key] = [];
    _lumen_listeners[key].push(fn);
}
function _lumen_rm_listener(nid, type, fn) {
    var key = String(nid) + ':' + String(type);
    var arr = _lumen_listeners[key];
    if (!arr) return;
    var idx = arr.indexOf(fn);
    if (idx >= 0) arr.splice(idx, 1);
}
function _lumen_dispatch(nid, event) {
    var key = String(nid) + ':' + event.type;
    var arr = _lumen_listeners[key];
    if (!arr || arr.length === 0) return !event.defaultPrevented;
    var copy = arr.slice(); // snapshot in case a handler mutates the list
    for (var i = 0; i < copy.length; i++) {
        try { copy[i].call(null, event); } catch(e) {}
        if (event._stopImmediate) break;
    }
    return !event.defaultPrevented;
}

// ── DOMTokenList (classList) ──────────────────────────────────────────────────

function _lumen_make_class_list(nid) {
    function getArr() {
        var c = _lumen_get_attr(nid, 'class');
        return (c && c.length > 0)
            ? c.split(/\\s+/).filter(function(t) { return t.length > 0; })
            : [];
    }
    function setArr(arr) { _lumen_set_attr(nid, 'class', arr.join(' ')); }
    var cl = {
        contains: function(cls) { return getArr().indexOf(String(cls)) >= 0; },
        add: function() {
            var arr = getArr();
            for (var i = 0; i < arguments.length; i++) {
                var cls = String(arguments[i]);
                if (arr.indexOf(cls) < 0) arr.push(cls);
            }
            setArr(arr);
        },
        remove: function() {
            var arr = getArr();
            for (var i = 0; i < arguments.length; i++) {
                var cls = String(arguments[i]);
                var idx = arr.indexOf(cls);
                if (idx >= 0) arr.splice(idx, 1);
            }
            setArr(arr);
        },
        toggle: function(cls, force) {
            cls = String(cls);
            var arr = getArr();
            var idx = arr.indexOf(cls);
            if (force !== undefined) {
                if (force && idx < 0)   { arr.push(cls); setArr(arr); return true; }
                if (!force && idx >= 0) { arr.splice(idx, 1); setArr(arr); return false; }
                return !!force;
            }
            if (idx >= 0) { arr.splice(idx, 1); setArr(arr); return false; }
            arr.push(cls); setArr(arr); return true;
        },
        replace: function(oldCls, newCls) {
            var arr = getArr();
            var idx = arr.indexOf(String(oldCls));
            if (idx < 0) return false;
            arr[idx] = String(newCls); setArr(arr); return true;
        },
        item: function(i) { var arr = getArr(); return arr[i] !== undefined ? arr[i] : null; },
        forEach: function(fn, thisArg) { getArr().forEach(fn, thisArg); },
        toString: function() { return getArr().join(' '); },
    };
    Object.defineProperty(cl, 'length', {
        get: function() { return getArr().length; },
        enumerable: true, configurable: true,
    });
    return cl;
}

// ── CSSStyleDeclaration (inline style) ───────────────────────────────────────

function _lumen_parse_style(s) {
    var obj = {};
    if (!s) return obj;
    s.split(';').forEach(function(decl) {
        var idx = decl.indexOf(':');
        if (idx < 0) return;
        var prop = decl.slice(0, idx).trim();
        var val  = decl.slice(idx + 1).trim();
        if (prop) obj[prop] = val;
    });
    return obj;
}
function _lumen_serialize_style(obj) {
    return Object.keys(obj).map(function(k) { return k + ': ' + obj[k]; }).join('; ');
}
function _lumen_camel_to_kebab(prop) {
    return prop.replace(/([A-Z])/g, function(m) { return '-' + m.toLowerCase(); });
}

function _lumen_make_style(nid) {
    function getParsed() {
        var s = _lumen_get_attr(nid, 'style');
        return _lumen_parse_style(s !== undefined ? s : '');
    }
    function setParsed(obj) { _lumen_set_attr(nid, 'style', _lumen_serialize_style(obj)); }
    var handler = {
        getPropertyValue: function(prop) {
            return getParsed()[_lumen_camel_to_kebab(String(prop))] || '';
        },
        setProperty: function(prop, val) {
            var obj = getParsed();
            obj[_lumen_camel_to_kebab(String(prop))] = String(val);
            setParsed(obj);
        },
        removeProperty: function(prop) {
            var obj = getParsed();
            var key = _lumen_camel_to_kebab(String(prop));
            var old = obj[key] || '';
            delete obj[key]; setParsed(obj); return old;
        },
    };
    Object.defineProperty(handler, 'cssText', {
        get: function() { var s = _lumen_get_attr(nid, 'style'); return s !== undefined ? s : ''; },
        set: function(v) { _lumen_set_attr(nid, 'style', String(v)); },
        enumerable: true, configurable: true,
    });
    return new Proxy(handler, {
        get: function(target, prop) {
            if (prop in target) return target[prop];
            return target.getPropertyValue(_lumen_camel_to_kebab(String(prop)));
        },
        set: function(target, prop, value) {
            if (prop in target) { target[prop] = value; return true; }
            target.setProperty(_lumen_camel_to_kebab(String(prop)), value);
            return true;
        },
    });
}

// ── Element factory ───────────────────────────────────────────────────────────

function _lumen_make_element(nid) {
    if (nid === null || nid === undefined) return null;
    var _classList = _lumen_make_class_list(nid);
    var _style     = _lumen_make_style(nid);
    var _obj = {
        __nid__: nid,
        get tagName()        { return _lumen_get_tag_name(nid); },
        get nodeName()       { return _lumen_get_tag_name(nid); },
        get nodeType()       { return _lumen_is_text_node(nid) ? 3 : 1; },
        get id()             { var v = _lumen_u2n(_lumen_get_attr(nid, 'id'));    return v !== null ? v : ''; },
        set id(v)            { _lumen_set_attr(nid, 'id', String(v)); },
        get className()      { var v = _lumen_u2n(_lumen_get_attr(nid, 'class')); return v !== null ? v : ''; },
        set className(v)     { _lumen_set_attr(nid, 'class', String(v)); },
        get classList()      { return _classList; },
        get style()          { return _style; },
        get textContent()    { return _lumen_get_text_content(nid); },
        set textContent(v)   { _lumen_set_text_content(nid, String(v)); },
        get innerHTML()      { return _lumen_get_inner_html(nid); },
        set innerHTML(v)     { _lumen_set_inner_html(nid, String(v)); },
        getAttribute:    function(n)    { return _lumen_u2n(_lumen_get_attr(nid, String(n))); },
        setAttribute:    function(n, v) { _lumen_set_attr(nid, String(n), String(v)); },
        removeAttribute: function(n)    { _lumen_remove_attr(nid, String(n)); },
        hasAttribute:    function(n)    { return _lumen_get_attr(nid, String(n)) !== undefined; },
        appendChild:     function(c) {
            if (c && c.__nid__ !== undefined) _lumen_append_child(nid, c.__nid__);
            return c;
        },
        removeChild:     function(c) {
            if (c && c.__nid__ !== undefined) _lumen_remove_child(nid, c.__nid__);
            return c;
        },
        querySelector:    function(sel) {
            var n = _lumen_u2n(_lumen_query_selector(String(sel)));
            return n !== null ? _lumen_make_element(n) : null;
        },
        querySelectorAll: function(sel) {
            return _lumen_query_selector_all(String(sel)).map(_lumen_make_element);
        },
        matches: function(sel) {
            // Phase 0: query the DOM and check if the result matches this nid.
            var n = _lumen_u2n(_lumen_query_selector(String(sel)));
            return n !== null && n === nid;
        },
        addEventListener:    function(type, fn) { _lumen_add_listener(nid, type, fn); },
        removeEventListener: function(type, fn) { _lumen_rm_listener(nid, type, fn); },
        dispatchEvent:       function(evt) {
            if (!evt) return true;
            evt.target = this; evt.currentTarget = this;
            return _lumen_dispatch(nid, evt);
        },
        closest: function(sel) {
            var cur = nid;
            while (cur !== undefined && cur !== null) {
                var n = _lumen_u2n(_lumen_query_selector(String(sel)));
                if (n !== null && n === cur) return _lumen_make_element(cur);
                var pid = _lumen_u2n(_lumen_get_parent(cur));
                cur = pid !== null ? pid : null;
            }
            return null;
        },
    };
    Object.defineProperty(_obj, 'parentElement', {
        get: function() {
            var pid = _lumen_u2n(_lumen_get_parent(nid));
            return pid !== null ? _lumen_make_element(pid) : null;
        },
        enumerable: false, configurable: true,
    });
    Object.defineProperty(_obj, 'children', {
        get: function() { return _lumen_get_children(nid).map(_lumen_make_element); },
        enumerable: false, configurable: true,
    });
    return _obj;
}

var _lumen_root_nid = _lumen_get_document_root();

var console = {
    log:   function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
    warn:  function() { _lumen_console_warn( Array.prototype.join.call(arguments, ' ')); },
    error: function() { _lumen_console_error(Array.prototype.join.call(arguments, ' ')); },
    info:  function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
    debug: function() { _lumen_console_log(  Array.prototype.join.call(arguments, ' ')); },
};

var document = {
    get title()  { return _lumen_get_document_title(); },
    set title(v) { _lumen_set_document_title(String(v)); },
    get body()   {
        var bid = _lumen_u2n(_lumen_get_body());
        return bid !== null ? _lumen_make_element(bid) : null;
    },
    get documentElement() { return _lumen_make_element(_lumen_root_nid); },
    getElementById:    function(id)  {
        var n = _lumen_u2n(_lumen_get_element_by_id(String(id)));
        return n !== null ? _lumen_make_element(n) : null;
    },
    querySelector:     function(sel) {
        var n = _lumen_u2n(_lumen_query_selector(String(sel)));
        return n !== null ? _lumen_make_element(n) : null;
    },
    querySelectorAll:  function(sel) {
        return _lumen_query_selector_all(String(sel)).map(_lumen_make_element);
    },
    createElement:     function(tag) {
        return _lumen_make_element(_lumen_create_element(String(tag).toLowerCase()));
    },
    createTextNode:    function(t)   { return _lumen_make_element(_lumen_create_text_node(String(t))); },
    createComment:     function()    { return _lumen_make_element(_lumen_create_text_node('')); },
    appendChild:       function(c)   {
        if (c && c.__nid__ !== undefined) _lumen_append_child(_lumen_root_nid, c.__nid__);
        return c;
    },
    addEventListener:    function() {},
    removeEventListener: function() {},
    dispatchEvent:       function() { return true; },
};

var alert    = function(m) { _lumen_console_log('[alert] ' + String(m)); };
var confirm  = function()  { return false; };
var prompt   = function()  { return null; };
var location = { href: '', protocol: 'file:', hostname: '', host: '', pathname: '', search: '', hash: '' };
// ── Service Worker API ────────────────────────────────────────────────────────

function _lumen_build_cache_object(origin, cacheName) {
    return {
        put: function(request, response) {
            var url = (typeof request === 'string') ? request : request.url;
            return response.arrayBuffer().then(function(buf) {
                _lumen_cache_put(origin, cacheName, url, new Uint8Array(buf));
                return undefined;
            });
        },
        match: function(request) {
            var url = (typeof request === 'string') ? request : request.url;
            var body = _lumen_cache_match(origin, cacheName, url);
            if (body === undefined || body === null) return Promise.resolve(undefined);
            return Promise.resolve(new Response(body));
        },
        delete: function(request) {
            var url = (typeof request === 'string') ? request : request.url;
            _lumen_cache_delete(origin, cacheName, url);
            return Promise.resolve(true);
        },
        keys: function() {
            return Promise.resolve(
                _lumen_cache_keys(origin, cacheName).map(function(u) { return new Request(u); })
            );
        },
        addAll: function(urls) {
            return Promise.all(urls.map(function(u) {
                return fetch(u).then(function(r) {
                    _lumen_cache_put(origin, cacheName, u, []);
                    return r;
                });
            }));
        },
    };
}

var _sw_origin = (typeof location !== 'undefined') ? (location.protocol + '//' + location.host) : '';

var caches = {
    open: function(name) {
        return Promise.resolve(_lumen_build_cache_object(_sw_origin, String(name)));
    },
    match: function(request) {
        var url = (typeof request === 'string') ? request : request.url;
        var body = _lumen_cache_match_any(_sw_origin, url);
        if (body === undefined || body === null) return Promise.resolve(undefined);
        return Promise.resolve(new Response(body));
    },
    has: function(name) {
        return Promise.resolve(_lumen_cache_has(_sw_origin, String(name)));
    },
    delete: function(name) {
        _lumen_cache_delete_cache(_sw_origin, String(name));
        return Promise.resolve(true);
    },
    keys: function() {
        return Promise.resolve(_lumen_cache_names(_sw_origin));
    },
};

var _sw_registrations = {};
var _sw_container = {
    register: function(scriptUrl, options) {
        var scope = (options && options.scope) ? String(options.scope) : '/';
        _lumen_sw_register(_sw_origin, scope, String(scriptUrl));
        var reg = {
            scope: scope,
            scriptURL: String(scriptUrl),
            active: null,
            installing: null,
            waiting: null,
            update: function() { return Promise.resolve(); },
            unregister: function() { return Promise.resolve(true); },
        };
        _sw_registrations[scope] = reg;
        return Promise.resolve(reg);
    },
    getRegistration: function(url) {
        var u = url || _sw_origin + '/';
        for (var scope in _sw_registrations) {
            if (String(u).indexOf(scope) === 0) return Promise.resolve(_sw_registrations[scope]);
        }
        return Promise.resolve(undefined);
    },
    getRegistrations: function() {
        return Promise.resolve(Object.values(_sw_registrations));
    },
    ready: Promise.resolve({ scope: '/', active: null }),
    controller: null,
    oncontrollerchange: null,
    onmessage: null,
};

var navigator = {
    userAgent: 'Lumen/0.0',
    language: 'en-US',
    onLine: false,
    serviceWorker: _sw_container,
};
var setTimeout  = function(fn) { try { fn(); } catch(e) {} return 0; };
var setInterval = function()   { return 0; };
var clearTimeout  = function() {};
var clearInterval = function() {};
var requestAnimationFrame = function() { return 0; };

var _popstate_listeners = [];

var history = {
    get length()  { return _lumen_history_length(); },
    get state()   {
        try { return JSON.parse(_lumen_history_state_json()); } catch(e) { return null; }
    },
    pushState:    function(state, title, url) {
        _lumen_history_push(
            JSON.stringify(state !== undefined ? state : null),
            String(url || '')
        );
    },
    replaceState: function(state, title, url) {
        _lumen_history_replace(
            JSON.stringify(state !== undefined ? state : null),
            String(url || '')
        );
    },
    back:    function() { history.go(-1); },
    forward: function() { history.go(1); },
    go: function(delta) {
        var ok = _lumen_history_go((delta | 0));
        if (ok) {
            var s;
            try { s = JSON.parse(_lumen_history_state_json()); } catch(e) { s = null; }
            var ev = { type: 'popstate', state: s };
            if (typeof window.onpopstate === 'function') {
                try { window.onpopstate(ev); } catch(e) {}
            }
            for (var i = 0; i < _popstate_listeners.length; i++) {
                try { _popstate_listeners[i](ev); } catch(e) {}
            }
        }
    },
};

function EventSource(url) {
    this.url = String(url || '');
    this.readyState = 0;
    this.onopen = null;
    this.onmessage = null;
    this.onerror = null;
    this._listeners = {};
}
EventSource.prototype.addEventListener = function(type, fn) {
    if (!this._listeners[type]) this._listeners[type] = [];
    this._listeners[type].push(fn);
};
EventSource.prototype.removeEventListener = function(type, fn) {
    if (!this._listeners[type]) return;
    var idx = this._listeners[type].indexOf(fn);
    if (idx >= 0) this._listeners[type].splice(idx, 1);
};
EventSource.prototype.close = function() { this.readyState = 2; };
EventSource.CONNECTING = 0;
EventSource.OPEN = 1;
EventSource.CLOSED = 2;

// ── IME Composition events (UI Events Specification §5.3) ─────────────────────
// Слушатели compositionstart/compositionupdate/compositionend:
// страница регистрирует их через addEventListener на нужном элементе.
// _lumen_dispatch_composition вызывается Rust-сторона после получения
// Ime::Preedit / Ime::Commit от winit. Диспатч идёт на document.activeElement
// (или document.body как fallback).
var _ime_active_element = null;

function _lumen_set_ime_target(el) {
    _ime_active_element = el || null;
}

function _lumen_dispatch_composition(type, data) {
    var target = _ime_active_element || (typeof document !== 'undefined' && document.body) || null;
    if (!target) return;
    var nid = target.__nid__;
    if (nid === undefined) return;
    var evt = new Event(type);
    evt.data = String(data);
    evt.locale = '';
    _lumen_dispatch(nid, evt);
}

// ── Page lifecycle events: pageshow / pagehide (HTML Living Standard §8.6) ───
// _lumen_bfcache_persisted is set to true by an injected init script when the
// shell restores a page from bfcache. Pages can read event.persisted to detect
// this case and skip expensive re-initialisation.
var _lumen_bfcache_persisted = false;
var _pageshow_listeners = [];
var _pagehide_listeners = [];

function _lumen_fire_page_lifecycle(type, persisted) {
    var evt = new Event(type);
    evt.persisted = !!persisted;
    var listeners = type === 'pageshow' ? _pageshow_listeners : _pagehide_listeners;
    for (var i = 0; i < listeners.length; i++) {
        try { listeners[i](evt); } catch(e) {}
    }
    var handler = type === 'pageshow' ? window.onpageshow : window.onpagehide;
    if (typeof handler === 'function') {
        try { handler(evt); } catch(e) {}
    }
}

var window = {
    history: history,
    onpopstate: null,
    onpageshow: null,
    onpagehide: null,
    location: location,
    navigator: navigator,
    alert: alert,
    confirm: confirm,
    prompt: prompt,
    setTimeout: setTimeout,
    setInterval: setInterval,
    clearTimeout: clearTimeout,
    clearInterval: clearInterval,
    requestAnimationFrame: requestAnimationFrame,
    EventSource: EventSource,
    caches: caches,
    document: document,
    console: console,
    _lumen_dispatch_composition: _lumen_dispatch_composition,
    _lumen_set_ime_target: _lumen_set_ime_target,
    _lumen_fire_page_lifecycle: _lumen_fire_page_lifecycle,
    addEventListener: function(type, fn) {
        if (typeof fn !== 'function') return;
        if (type === 'popstate') {
            _popstate_listeners.push(fn);
        } else if (type === 'pageshow') {
            _pageshow_listeners.push(fn);
        } else if (type === 'pagehide') {
            _pagehide_listeners.push(fn);
        }
    },
    removeEventListener: function(type, fn) {
        var arr;
        if (type === 'popstate') arr = _popstate_listeners;
        else if (type === 'pageshow') arr = _pageshow_listeners;
        else if (type === 'pagehide') arr = _pagehide_listeners;
        else return;
        var idx = arr.indexOf(fn);
        if (idx >= 0) arr.splice(idx, 1);
    },
    dispatchEvent: function() { return true; },
};
";

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::QuickJsRuntime;
    use lumen_core::JsRuntime;
    use lumen_dom::{Document, NodeData, QualName};

    fn make_doc() -> Arc<Mutex<Document>> {
        let mut doc = Document::new();
        let html = doc.create_element(QualName::html("html"));
        let head = doc.create_element(QualName::html("head"));
        let title = doc.create_element(QualName::html("title"));
        let title_text = doc.create_text("Test Page");
        let body = doc.create_element(QualName::html("body"));
        let div = doc.create_element(QualName::html("div"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(div).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("id"),
                value: "main".into(),
            });
        }
        let span = doc.create_element(QualName::html("span"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(span).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("class"),
                value: "highlight".into(),
            });
        }
        let text = doc.create_text("Hello");
        doc.append_child(doc.root(), html);
        doc.append_child(html, head);
        doc.append_child(head, title);
        doc.append_child(title, title_text);
        doc.append_child(html, body);
        doc.append_child(body, div);
        doc.append_child(div, span);
        doc.append_child(span, text);
        Arc::new(Mutex::new(doc))
    }

    fn runtime_with_dom(doc: Arc<Mutex<Document>>) -> QuickJsRuntime {
        let rt = QuickJsRuntime::new().unwrap();
        rt.install_dom(doc).unwrap();
        rt
    }

    #[test]
    fn console_log_does_not_crash() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("console.log('hello from test')").unwrap();
    }

    #[test]
    fn get_element_by_id_found() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_element_by_id_not_found() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('nonexistent') === null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn get_element_by_id_tag_name() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').tagName")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("DIV".into()));
    }

    #[test]
    fn query_selector_by_id() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('#main') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_by_class() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight') !== null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn query_selector_by_tag() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("document.querySelector('span') !== null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn text_content_get() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').textContent")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("Hello".into()));
    }

    #[test]
    fn text_content_set_mutates_dom() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval("document.getElementById('main').textContent = 'World';")
            .unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        // The div#main should now have a single text child "World".
        let body_id = find_element_by_tag(&doc, "body").unwrap();
        let div_id = doc.get(body_id).children[0];
        let text = collect_text_content(&doc, div_id);
        assert_eq!(text, "World");
    }

    #[test]
    fn set_attribute_mutates_dom() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval("document.getElementById('main').setAttribute('data-x', '42');")
            .unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        let body_id = find_element_by_tag(&doc, "body").unwrap();
        let div_id = doc.get(body_id).children[0];
        assert_eq!(doc.get(div_id).get_attr("data-x"), Some("42"));
    }

    #[test]
    fn get_attribute_returns_value() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').getAttribute('id')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("main".into()));
    }

    #[test]
    fn get_attribute_returns_null_for_missing() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').getAttribute('data-missing') === null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn document_title_get() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("document.title").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("Test Page".into()));
    }

    #[test]
    fn document_title_set() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval("document.title = 'New Title';").unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        let title_text = find_element_by_tag(&doc, "title")
            .map(|nid| collect_text_content(&doc, nid))
            .unwrap_or_default();
        assert_eq!(title_text, "New Title");
    }

    #[test]
    fn document_body_not_null() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("document.body !== null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn create_element_and_append() {
        let doc = make_doc();
        let rt = runtime_with_dom(Arc::clone(&doc));
        rt.eval(
            "var p = document.createElement('p'); \
             p.textContent = 'new paragraph'; \
             document.body.appendChild(p);",
        )
        .unwrap();
        drop(rt);
        let doc = Arc::try_unwrap(doc).unwrap().into_inner().unwrap();
        let body_id = find_element_by_tag(&doc, "body").unwrap();
        let body = doc.get(body_id);
        // body should now have 2 children: the original div + the new <p>
        assert_eq!(body.children.len(), 2);
        let p_id = body.children[1];
        assert_eq!(
            doc.get(p_id)
                .element_name()
                .map(|n| n.local.as_str()),
            Some("p")
        );
        assert_eq!(collect_text_content(&doc, p_id), "new paragraph");
    }

    #[test]
    fn query_selector_all_returns_array() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelectorAll('span').length")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn alert_does_not_crash() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("alert('test')").unwrap();
    }

    #[test]
    fn timeout_calls_function() {
        let rt = runtime_with_dom(make_doc());
        // setTimeout in our shim calls the callback synchronously.
        let result = rt
            .eval(
                "var x = 0; \
                 setTimeout(function() { x = 1; }, 0); \
                 x",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn history_initial_length_is_one() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("history.length").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn history_initial_state_is_null() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("history.state === null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn history_push_state_increments_length() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("history.pushState({page: 1}, '', '/page1');").unwrap();
        rt.eval("history.pushState({page: 2}, '', '/page2');").unwrap();
        let result = rt.eval("history.length").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(3.0));
    }

    #[test]
    fn history_state_after_push_returns_state() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("history.pushState({x: 42}, '', '/p');").unwrap();
        let result = rt.eval("history.state.x").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(42.0));
    }

    #[test]
    fn history_replace_state_keeps_length() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("history.pushState({n: 1}, '', '/a');").unwrap();
        rt.eval("history.replaceState({n: 99}, '', '/a2');").unwrap();
        let len = rt.eval("history.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(2.0));
        let state = rt.eval("history.state.n").unwrap();
        assert_eq!(state, lumen_core::JsValue::Number(99.0));
    }

    #[test]
    fn history_back_fires_popstate_with_previous_state() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var events = []; \
             window.addEventListener('popstate', function(e) { events.push(e.state); }); \
             history.pushState({page: 1}, '', '/p1'); \
             history.pushState({page: 2}, '', '/p2'); \
             history.back();",
        )
        .unwrap();
        let len = rt.eval("events.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(1.0));
        let page = rt.eval("events[0].page").unwrap();
        assert_eq!(page, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn history_forward_after_back() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "history.pushState({n: 1}, '', '/p1'); \
             history.pushState({n: 2}, '', '/p2'); \
             history.back();",
        )
        .unwrap();
        let state_after_back = rt.eval("history.state.n").unwrap();
        assert_eq!(state_after_back, lumen_core::JsValue::Number(1.0));

        rt.eval("history.forward();").unwrap();
        let state_after_fwd = rt.eval("history.state.n").unwrap();
        assert_eq!(state_after_fwd, lumen_core::JsValue::Number(2.0));
    }

    #[test]
    fn history_go_beyond_bounds_does_not_fire_popstate() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var fired = false; \
             window.addEventListener('popstate', function() { fired = true; }); \
             history.go(-5);",
        )
        .unwrap();
        let result = rt.eval("fired").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn window_onpopstate_fires_on_back() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var captured = null; \
             window.onpopstate = function(e) { captured = e.state; }; \
             history.pushState({v: 7}, '', '/p'); \
             history.back();",
        )
        .unwrap();
        let result = rt.eval("captured === null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true)); // initial state is null
    }

    #[test]
    fn history_push_drops_forward_entries() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "history.pushState({n: 1}, '', '/p1'); \
             history.pushState({n: 2}, '', '/p2'); \
             history.back(); \
             history.pushState({n: 3}, '', '/p3');",
        )
        .unwrap();
        // After back + push, forward entries are dropped: entries = [init, {n:1}, {n:3}]
        let len = rt.eval("history.length").unwrap();
        assert_eq!(len, lumen_core::JsValue::Number(3.0));
        let state = rt.eval("history.state.n").unwrap();
        assert_eq!(state, lumen_core::JsValue::Number(3.0));
    }

    #[test]
    fn window_object_exposes_history() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("window.history === history").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn window_remove_event_listener_stops_popstate() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var count = 0; \
             function handler(e) { count++; } \
             window.addEventListener('popstate', handler); \
             history.pushState({}, '', '/p'); \
             history.back(); \
             window.removeEventListener('popstate', handler); \
             history.forward(); \
             history.back();",
        )
        .unwrap();
        // handler fires once (on first back), then is removed
        let result = rt.eval("count").unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    // ── classList ────────────────────────────────────────────────────────────

    #[test]
    fn classlist_contains_true() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.contains('highlight')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_contains_false() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.contains('missing')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn classlist_add_and_contains() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').classList.add('active');").unwrap();
        let result = rt
            .eval("document.getElementById('main').classList.contains('active')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_remove() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "document.querySelector('.highlight').classList.remove('highlight');",
        )
        .unwrap();
        let result = rt
            .eval("document.querySelector('.highlight') === null")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_toggle_adds_when_absent() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.getElementById('main').classList.toggle('open')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
        let has = rt
            .eval("document.getElementById('main').classList.contains('open')")
            .unwrap();
        assert_eq!(has, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_toggle_removes_when_present() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.querySelector('.highlight').classList.toggle('highlight');").unwrap();
        let has = rt
            .eval("document.querySelector('.highlight') === null")
            .unwrap();
        assert_eq!(has, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_replace() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "document.querySelector('.highlight').classList.replace('highlight', 'selected');",
        )
        .unwrap();
        let old = rt
            .eval("document.querySelector('.highlight') === null")
            .unwrap();
        assert_eq!(old, lumen_core::JsValue::Bool(true));
        let new = rt
            .eval("document.querySelector('.selected') !== null")
            .unwrap();
        assert_eq!(new, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn classlist_length() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.length")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn classlist_to_string() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("document.querySelector('.highlight').classList.toString()")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("highlight".into()));
    }

    // ── style / CSSStyleDeclaration ──────────────────────────────────────────

    #[test]
    fn style_set_and_get_property() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').style.setProperty('color', 'red');")
            .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.getPropertyValue('color')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("red".into()));
    }

    #[test]
    fn style_assignment_via_property_name() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').style.color = 'blue';")
            .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.color")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("blue".into()));
    }

    #[test]
    fn style_camel_case_to_kebab() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("document.getElementById('main').style.backgroundColor = 'green';")
            .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.getPropertyValue('background-color')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("green".into()));
    }

    #[test]
    fn style_remove_property() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var el = document.getElementById('main'); \
             el.style.color = 'red'; \
             el.style.removeProperty('color');",
        )
        .unwrap();
        let result = rt
            .eval("document.getElementById('main').style.getPropertyValue('color')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("".into()));
    }

    #[test]
    fn style_css_text_roundtrip() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "document.getElementById('main').style.cssText = 'color: red; font-size: 12px';",
        )
        .unwrap();
        let color = rt
            .eval("document.getElementById('main').style.getPropertyValue('color')")
            .unwrap();
        assert_eq!(color, lumen_core::JsValue::String("red".into()));
        let size = rt
            .eval("document.getElementById('main').style.getPropertyValue('font-size')")
            .unwrap();
        assert_eq!(size, lumen_core::JsValue::String("12px".into()));
    }

    // ── addEventListener / dispatchEvent on elements ─────────────────────────

    #[test]
    fn element_add_event_listener_and_dispatch() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var received = null; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('click', function(e) { received = e.type; }); \
                 el.dispatchEvent(new Event('click')); \
                 received",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::String("click".into()));
    }

    #[test]
    fn element_remove_event_listener_stops_dispatch() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var count = 0; \
                 var el = document.getElementById('main'); \
                 function h() { count++; } \
                 el.addEventListener('click', h); \
                 el.dispatchEvent(new Event('click')); \
                 el.removeEventListener('click', h); \
                 el.dispatchEvent(new Event('click')); \
                 count",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    #[test]
    fn custom_event_detail_accessible_in_handler() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var got = null; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('myevent', function(e) { got = e.detail; }); \
                 el.dispatchEvent(new CustomEvent('myevent', { detail: 42 })); \
                 got",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(42.0));
    }

    #[test]
    fn event_prevent_default() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var el = document.getElementById('main'); \
                 el.addEventListener('submit', function(e) { e.preventDefault(); }); \
                 var ev = new Event('submit', { cancelable: true }); \
                 var ret = el.dispatchEvent(ev); \
                 ret",
            )
            .unwrap();
        // dispatchEvent returns false when defaultPrevented
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn stop_immediate_propagation_stops_subsequent_listeners() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                "var calls = 0; \
                 var el = document.getElementById('main'); \
                 el.addEventListener('x', function(e) { calls++; e.stopImmediatePropagation(); }); \
                 el.addEventListener('x', function(e) { calls++; }); \
                 el.dispatchEvent(new Event('x')); \
                 calls",
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Number(1.0));
    }

    // ── Event / CustomEvent constructors ─────────────────────────────────────

    #[test]
    fn event_constructor_sets_type() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("new Event('load').type").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("load".into()));
    }

    #[test]
    fn event_bubbles_cancelable_defaults_false() {
        let rt = runtime_with_dom(make_doc());
        let bubbles = rt.eval("new Event('x').bubbles").unwrap();
        assert_eq!(bubbles, lumen_core::JsValue::Bool(false));
        let cancelable = rt.eval("new Event('x').cancelable").unwrap();
        assert_eq!(cancelable, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn custom_event_detail_null_by_default() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("new CustomEvent('x').detail === null").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── navigator.serviceWorker ───────────────────────────────────────────────

    #[test]
    fn navigator_has_service_worker() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof navigator.serviceWorker === 'object'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_register_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval(
                r#"
                var p = navigator.serviceWorker.register('/sw.js', { scope: '/app/' });
                typeof p.then === 'function'
                "#,
            )
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn sw_register_calls_lumen_primitive() {
        let rt = runtime_with_dom(make_doc());
        // _sw_origin = location.protocol + '//' + location.host = 'file://'
        rt.eval("navigator.serviceWorker.register('/sw.js', { scope: '/' });")
            .unwrap();
        // проверяем через примитив — origin 'file://'
        let result = rt.eval("_lumen_sw_has_registration('file://')").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── caches API ────────────────────────────────────────────────────────────

    #[test]
    fn caches_object_exists() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("typeof caches === 'object'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn caches_open_returns_promise() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof caches.open('v1').then === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_has_returns_false_for_unknown() {
        let rt = runtime_with_dom(make_doc());
        // has() returns promise; we check the primitive directly.
        let result = rt
            .eval("_lumen_cache_has('', 'nonexistent')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn cache_put_and_match_roundtrip() {
        let rt = runtime_with_dom(make_doc());
        // Put raw bytes via primitive, then match.
        rt.eval("_lumen_cache_put('', 'v1', 'https://x.com/a', [72, 101, 108, 108, 111]);")
            .unwrap();
        let result = rt
            .eval("_lumen_cache_has('', 'v1')")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
        let keys = rt.eval("_lumen_cache_keys('', 'v1')").unwrap();
        assert_eq!(
            keys,
            lumen_core::JsValue::Array(vec![lumen_core::JsValue::String(
                "https://x.com/a".into()
            )])
        );
    }

    #[test]
    fn cache_match_any_returns_none_on_miss() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("_lumen_cache_match_any('', 'https://x.com/missing') === undefined")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn cache_delete_removes_entry() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("_lumen_cache_put('', 'v1', 'https://x.com/b', []);")
            .unwrap();
        rt.eval("_lumen_cache_delete('', 'v1', 'https://x.com/b');")
            .unwrap();
        let keys = rt.eval("_lumen_cache_keys('', 'v1')").unwrap();
        assert_eq!(keys, lumen_core::JsValue::Array(vec![]));
    }

    #[test]
    fn cache_names_lists_opened_caches() {
        let rt = runtime_with_dom(make_doc());
        rt.eval("_lumen_cache_put('', 'alpha', 'https://x.com/r', []);")
            .unwrap();
        rt.eval("_lumen_cache_put('', 'beta', 'https://x.com/s', []);")
            .unwrap();
        let mut names = match rt.eval("_lumen_cache_names('')").unwrap() {
            lumen_core::JsValue::Array(a) => a
                .into_iter()
                .filter_map(|v| {
                    if let lumen_core::JsValue::String(s) = v {
                        Some(s)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        };
        names.sort();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn window_has_caches() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("typeof window.caches === 'object'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── IME composition API ───────────────────────────────────────────────────

    #[test]
    fn dispatch_composition_function_exists() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof _lumen_dispatch_composition === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn set_ime_target_function_exists() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof _lumen_set_ime_target === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn dispatch_composition_on_element_fires_listener() {
        let rt = runtime_with_dom(make_doc());
        // Регистрируем слушатель compositionstart на main div.
        // При диспатче он должен сохранить data в глобальной переменной.
        rt.eval(r#"
            var _got_composition = null;
            var el = document.getElementById('main');
            el.addEventListener('compositionstart', function(e) {
                _got_composition = e.type;
            });
            _lumen_set_ime_target(el);
            _lumen_dispatch_composition('compositionstart', '');
        "#).unwrap();
        let result = rt.eval("_got_composition").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("compositionstart".into()));
    }

    #[test]
    fn dispatch_composition_update_carries_data() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(r#"
            var _comp_data = null;
            var el = document.getElementById('main');
            el.addEventListener('compositionupdate', function(e) {
                _comp_data = e.data;
            });
            _lumen_set_ime_target(el);
            _lumen_dispatch_composition('compositionupdate', 'あい');
        "#).unwrap();
        let result = rt.eval("_comp_data").unwrap();
        assert_eq!(result, lumen_core::JsValue::String("あい".into()));
    }

    #[test]
    fn dispatch_composition_without_target_does_not_crash() {
        let rt = runtime_with_dom(make_doc());
        // Нет target — должен молча ничего не сделать.
        rt.eval("_lumen_set_ime_target(null); _lumen_dispatch_composition('compositionstart', '');")
            .unwrap();
    }

    #[test]
    fn window_has_dispatch_composition() {
        let rt = runtime_with_dom(make_doc());
        let result = rt
            .eval("typeof window._lumen_dispatch_composition === 'function'")
            .unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }

    // ── bfcache / pageshow / pagehide ────────────────────────────────────────

    #[test]
    fn window_has_pageshow_pagehide_handlers() {
        let rt = runtime_with_dom(make_doc());
        // onpageshow and onpagehide should be null (not set) initially.
        let r1 = rt.eval("window.onpageshow === null").unwrap();
        let r2 = rt.eval("window.onpagehide === null").unwrap();
        assert_eq!(r1, lumen_core::JsValue::Bool(true));
        assert_eq!(r2, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pageshow_listener_receives_event_with_persisted_false() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var saw = false; var persistedFlag = null;
             window.addEventListener('pageshow', function(e) { saw = true; persistedFlag = e.persisted; });
             _lumen_fire_page_lifecycle('pageshow', false);",
        ).unwrap();
        let saw = rt.eval("saw").unwrap();
        let persisted = rt.eval("persistedFlag").unwrap();
        assert_eq!(saw, lumen_core::JsValue::Bool(true));
        assert_eq!(persisted, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn pageshow_listener_receives_persisted_true_from_bfcache() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var persistedFlag = null;
             window.addEventListener('pageshow', function(e) { persistedFlag = e.persisted; });
             _lumen_fire_page_lifecycle('pageshow', true);",
        ).unwrap();
        let persisted = rt.eval("persistedFlag").unwrap();
        assert_eq!(persisted, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn pagehide_listener_fires() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var fired = false;
             window.addEventListener('pagehide', function(e) { fired = true; });
             _lumen_fire_page_lifecycle('pagehide', false);",
        ).unwrap();
        let fired = rt.eval("fired").unwrap();
        assert_eq!(fired, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn onpageshow_handler_fires() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var saw = false;
             window.onpageshow = function(e) { saw = true; };
             _lumen_fire_page_lifecycle('pageshow', false);",
        ).unwrap();
        let saw = rt.eval("saw").unwrap();
        assert_eq!(saw, lumen_core::JsValue::Bool(true));
    }

    #[test]
    fn remove_pageshow_listener_stops_it_firing() {
        let rt = runtime_with_dom(make_doc());
        rt.eval(
            "var count = 0;
             var fn1 = function() { count++; };
             window.addEventListener('pageshow', fn1);
             window.removeEventListener('pageshow', fn1);
             _lumen_fire_page_lifecycle('pageshow', false);",
        ).unwrap();
        let count = rt.eval("count").unwrap();
        assert_eq!(count, lumen_core::JsValue::Number(0.0));
    }

    #[test]
    fn lumen_bfcache_persisted_default_false() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("_lumen_bfcache_persisted").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(false));
    }

    #[test]
    fn lumen_fire_page_lifecycle_exported_on_window() {
        let rt = runtime_with_dom(make_doc());
        let result = rt.eval("typeof window._lumen_fire_page_lifecycle === 'function'").unwrap();
        assert_eq!(result, lumen_core::JsValue::Bool(true));
    }
}

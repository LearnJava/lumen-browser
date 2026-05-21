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

function _lumen_make_element(nid) {
    if (nid === null || nid === undefined) return null;
    var _style = { setProperty: function() {} };
    var _obj = {
        __nid__: nid,
        get tagName()        { return _lumen_get_tag_name(nid); },
        get nodeName()       { return _lumen_get_tag_name(nid); },
        get nodeType()       { return _lumen_is_text_node(nid) ? 3 : 1; },
        get id()             { var v = _lumen_u2n(_lumen_get_attr(nid, 'id'));    return v !== null ? v : ''; },
        set id(v)            { _lumen_set_attr(nid, 'id', String(v)); },
        get className()      { var v = _lumen_u2n(_lumen_get_attr(nid, 'class')); return v !== null ? v : ''; },
        set className(v)     { _lumen_set_attr(nid, 'class', String(v)); },
        get textContent()    { return _lumen_get_text_content(nid); },
        set textContent(v)   { _lumen_set_text_content(nid, String(v)); },
        get innerHTML()      { return _lumen_get_inner_html(nid); },
        set innerHTML(v)     { _lumen_set_inner_html(nid, String(v)); },
        get style()          { return _style; },
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
        querySelector:    function() { return null; },
        querySelectorAll: function() { return []; },
        addEventListener: function() {},
        removeEventListener: function() {},
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
var location = { href: '', protocol: 'file:', hostname: '', pathname: '', search: '', hash: '' };
var navigator = { userAgent: 'Lumen/0.0', language: 'en-US', onLine: false };
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

var window = {
    history: history,
    onpopstate: null,
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
    document: document,
    console: console,
    addEventListener: function(type, fn) {
        if (type === 'popstate' && typeof fn === 'function') {
            _popstate_listeners.push(fn);
        }
    },
    removeEventListener: function(type, fn) {
        if (type === 'popstate') {
            var idx = _popstate_listeners.indexOf(fn);
            if (idx >= 0) _popstate_listeners.splice(idx, 1);
        }
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
}

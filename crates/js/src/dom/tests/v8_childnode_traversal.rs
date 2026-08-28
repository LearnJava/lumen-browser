//! V8 port of the "ChildNode/ParentNode/ElementTraversal + TreeWalker/NodeIterator"
//! test row (S12b-24-childnode-traversal), the sixth porting slice of the `dom.rs`
//! test-monolith migration described in `docs/tasks/ph3-v8-migration.md`: the
//! ChildNode/ParentNode mixin (`remove`/`before`/`after`/`replaceWith`/`prepend`/
//! `replaceChildren`), ElementTraversal (`childElementCount`/`firstElementChild`/
//! `lastElementChild`/`nextElementSibling`/`previousElementSibling`), the live
//! `HTMLCollection` returned by `.children`, `Node.isConnected`, TreeWalker/
//! NodeIterator (+ `NodeFilter`), `document.adoptNode`/`importNode`, and the raw
//! `_lumen_get_bounding_rect`/`_lumen_get_viewport_size` bindings.
//!
//! The 27 tests moved here verbatim from the QuickJS monolith above — bodies are
//! `rt.eval(...)` plus `update_layout_rects`/`update_viewport_size`, which
//! `V8JsRuntime` already mirrors — so the only edit was which runtime the fixture
//! builds. No engine divergence found.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`]: same fixture document, same
/// `install_dom` argument list, same `_LUMEN_EXTENSION_ACTIVE` pre-eval.
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// ── ChildNode / ParentNode mixin tests ───────────────────────────────────

#[test]
fn element_remove_detaches_from_parent() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _container = document.createElement('div');
                document.body.appendChild(_container);
                var _div = document.createElement('span');
                _container.appendChild(_div);
                _div.remove();
            "#).unwrap();
    let count = rt.eval("_container.children.length").unwrap();
    assert_eq!(count, lumen_core::JsValue::Number(0.0));
}

// ── ElementTraversal / ParentNode.children (BUG-310) ──────────────────────

// Appends three element children (`a`,`b`,`c`) interleaved with text nodes
// to a fresh `div` bound to the JS global `varname`. innerHTML is NOT parsed
// by the bare unit-test runtime, so the subtree is built via DOM APIs.
fn build_mixed_children(rt: &V8JsRuntime, varname: &str) {
    rt.eval(&format!(r#"
                var {v} = document.createElement('div');
                document.body.appendChild({v});
                {v}.appendChild(document.createTextNode('x'));
                var _s0 = document.createElement('span'); _s0.id = 'a'; {v}.appendChild(_s0);
                {v}.appendChild(document.createTextNode(' '));
                var _s1 = document.createElement('b');    _s1.id = 'b'; {v}.appendChild(_s1);
                {v}.appendChild(document.createTextNode(' '));
                var _s2 = document.createElement('span'); _s2.id = 'c'; {v}.appendChild(_s2);
                {v}.appendChild(document.createTextNode('y'));
            "#, v = varname)).unwrap();
}

#[test]
fn element_traversal_child_element_count_skips_text() {
    let rt = v8_runtime_with_dom(make_doc());
    build_mixed_children(&rt, "_t");
    // Text nodes ('x', whitespace, 'y') must not be counted.
    let n = rt.eval("_t.childElementCount").unwrap();
    assert_eq!(n, lumen_core::JsValue::Number(3.0));
}

#[test]
fn element_traversal_first_last_element_child() {
    let rt = v8_runtime_with_dom(make_doc());
    build_mixed_children(&rt, "_t");
    let first = rt.eval("_t.firstElementChild.id").unwrap();
    let last = rt.eval("_t.lastElementChild.id").unwrap();
    let ntype = rt.eval("_t.firstElementChild.nodeType").unwrap();
    assert_eq!(first, lumen_core::JsValue::String("a".into()));
    assert_eq!(last, lumen_core::JsValue::String("c".into()));
    assert_eq!(ntype, lumen_core::JsValue::Number(1.0));
}

#[test]
fn element_traversal_sibling_navigation_skips_text() {
    let rt = v8_runtime_with_dom(make_doc());
    build_mixed_children(&rt, "_t");
    // 'a' → next element 'b' and 'c' → prev element 'b' skip whitespace text.
    let next = rt.eval("document.getElementById('a').nextElementSibling.id").unwrap();
    let prev = rt.eval("document.getElementById('c').previousElementSibling.id").unwrap();
    assert_eq!(next, lumen_core::JsValue::String("b".into()));
    assert_eq!(prev, lumen_core::JsValue::String("b".into()));
}

#[test]
fn element_traversal_null_edges() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _empty = document.createElement('div');
                document.body.appendChild(_empty);
                _empty.appendChild(document.createTextNode('just text, no elements'));
                var _solo = document.createElement('div');
                document.body.appendChild(_solo);
                var _only = document.createElement('span'); _only.id = 'solo';
                _solo.appendChild(_only);
            "#).unwrap();
    let no_first = rt.eval("_empty.firstElementChild === null").unwrap();
    let no_last = rt.eval("_empty.lastElementChild === null").unwrap();
    let count0 = rt.eval("_empty.childElementCount").unwrap();
    let no_next = rt.eval("document.getElementById('solo').nextElementSibling === null").unwrap();
    let no_prev = rt.eval("document.getElementById('solo').previousElementSibling === null").unwrap();
    assert_eq!(no_first, lumen_core::JsValue::Bool(true));
    assert_eq!(no_last, lumen_core::JsValue::Bool(true));
    assert_eq!(count0, lumen_core::JsValue::Number(0.0));
    assert_eq!(no_next, lumen_core::JsValue::Bool(true));
    assert_eq!(no_prev, lumen_core::JsValue::Bool(true));
}

#[test]
fn children_is_live_html_collection() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _p = document.createElement('ul');
                document.body.appendChild(_p);
                _p.appendChild(document.createElement('li'));
                _p.appendChild(document.createElement('li'));
                _p.appendChild(document.createElement('li'));
            "#).unwrap();
    let is_coll = rt.eval("_p.children instanceof HTMLCollection").unwrap();
    assert_eq!(is_coll, lumen_core::JsValue::Bool(true));
    // A cached reference must reflect later mutations (live collection).
    let live = rt.eval(r#"
                var _c = _p.children;
                var before = _c.length;
                _p.appendChild(document.createElement('li'));
                var after = _c.length;
                before === 3 && after === 4
            "#).unwrap();
    assert_eq!(live, lumen_core::JsValue::Bool(true));
}

#[test]
fn html_collection_supports_enumeration() {
    // BUG-323: HTMLCollection's Proxy had no `ownKeys`/`getOwnPropertyDescriptor`
    // traps, so `for-in`, `Object.getOwnPropertyNames` and `hasOwnProperty` all
    // saw an empty plain object instead of the live indices/named keys.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _p = document.createElement('ul');
                document.body.appendChild(_p);
                var _a = document.createElement('li'); _a.id = 'alpha'; _p.appendChild(_a);
                var _b = document.createElement('li'); _b.setAttribute('name', 'beta'); _p.appendChild(_b);
                _p.appendChild(document.createElement('li'));
            "#).unwrap();
    // for-in yields only the enumerable indexed keys.
    let for_in = rt.eval(r#"
                var keys = [];
                for (var p in _p.children) keys.push(p);
                keys.join(',')
            "#).unwrap();
    assert_eq!(for_in, lumen_core::JsValue::String("0,1,2".into()));
    // Object.getOwnPropertyNames sees indices AND named keys (non-enumerable).
    let own_names = rt.eval("Object.getOwnPropertyNames(_p.children).join(',')").unwrap();
    assert_eq!(own_names, lumen_core::JsValue::String("0,1,2,alpha,beta".into()));
    // hasOwnProperty is true for both indices and named keys.
    let has_own = rt.eval(r#"
                var c = _p.children;
                c.hasOwnProperty('0') && c.hasOwnProperty('alpha') && c.hasOwnProperty('beta')
                    && !c.hasOwnProperty('missing')
            "#).unwrap();
    assert_eq!(has_own, lumen_core::JsValue::Bool(true));
}

#[test]
fn bug_328_html_collection_name_exposure_requires_html_namespace() {
    // BUG-328: `createElementNS(null-or-"", ...)` builds an element with NO
    // namespace, previously collapsed into `Namespace::Html` on the Rust
    // side — indistinguishable from a real HTML element. DOM §4.2.10.2
    // only exposes an element's `name` attribute as an HTMLCollection
    // property when the element is in the HTML namespace; `id` exposure is
    // namespace-blind, and `ownKeys`'s order is a single tree-order pass
    // (id-then-name per element), not an id-pass over every element
    // followed by a separate name-pass. Mirrors
    // `dom/nodes/Element-children.html` subtest 2 (`ownKeys`/enumeration
    // order) exactly: `<img> <img id=foo> <img id=foo> <img name=bar>`
    // plus two `createElementNS("", "img")` elements (id=baz, name=qux).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _p = document.createElement('div');
                document.body.appendChild(_p);
                _p.appendChild(document.createElement('img'));
                _p.appendChild(document.createElement('img')).id = 'foo';
                _p.appendChild(document.createElement('img')).id = 'foo';
                _p.appendChild(document.createElement('img')).setAttribute('name', 'bar');
                var _baz = document.createElementNS('', 'img');
                _baz.setAttribute('id', 'baz');
                _p.appendChild(_baz);
                var _qux = document.createElementNS('', 'img');
                _qux.setAttribute('name', 'qux');
                _p.appendChild(_qux);
            "#).unwrap();
    // `namespaceURI` of a no-namespace element is `null`, not the HTML
    // namespace and not the empty string.
    let ns_uri = rt.eval("_baz.namespaceURI").unwrap();
    assert_eq!(ns_uri, lumen_core::JsValue::Null);
    // `name` on a no-namespace element (`qux`) must not leak into
    // `ownKeys`, and `id`-then-name per-element order must put `bar`
    // (index 3) before `baz` (index 4), matching the WPT expectation.
    let own_names = rt.eval("Object.getOwnPropertyNames(_p.children).join(',')").unwrap();
    assert_eq!(
        own_names,
        lumen_core::JsValue::String("0,1,2,3,4,5,foo,bar,baz".into())
    );
    // `namedItem`/named `get` must not find the no-namespace element by
    // its `name` attribute, but must still find it by `id`.
    let named_by_name = rt.eval("_p.children.namedItem('qux')").unwrap();
    assert_eq!(named_by_name, lumen_core::JsValue::Null);
    let named_by_id = rt.eval("_p.children.namedItem('baz') === _baz").unwrap();
    assert_eq!(named_by_id, lumen_core::JsValue::Bool(true));
}

#[test]
fn parent_node_children_is_reachable() {
    // Regression for ParentNode-children.html: `node.parentNode.children`
    // must resolve (parentNode was missing on element wrappers before BUG-310).
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _ul = document.createElement('ul');
                document.body.appendChild(_ul);
                var _li = document.createElement('li'); _li.id = 'li0'; _ul.appendChild(_li);
                _ul.appendChild(document.createElement('li'));
            "#).unwrap();
    let via_parent = rt.eval("document.getElementById('li0').parentNode.children.length").unwrap();
    assert_eq!(via_parent, lumen_core::JsValue::Number(2.0));
    let parent_tag = rt.eval("document.getElementById('li0').parentNode.tagName.toLowerCase()").unwrap();
    assert_eq!(parent_tag, lumen_core::JsValue::String("ul".into()));
}

#[test]
fn node_is_connected_reflects_document_attachment() {
    // BUG-311: Node.isConnected — true once the node is in the document tree,
    // false for a freshly-created (detached) node and after removal.
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _det = document.createElement('div'); _det.id = 'det';
                var _child = document.createElement('span'); _det.appendChild(_child);
                var _att = document.createElement('div'); _att.id = 'att';
                document.body.appendChild(_att);
            "#).unwrap();
    // Detached element and its detached child: false.
    let det = rt.eval("document.getElementById('det') === null && _det.isConnected").unwrap();
    assert_eq!(det, lumen_core::JsValue::Bool(false));
    let det_child = rt.eval("_child.isConnected").unwrap();
    assert_eq!(det_child, lumen_core::JsValue::Bool(false));
    // Attached element: true; documentElement itself: true.
    let att = rt.eval("_att.isConnected").unwrap();
    assert_eq!(att, lumen_core::JsValue::Bool(true));
    let html = rt.eval("document.documentElement.isConnected").unwrap();
    assert_eq!(html, lumen_core::JsValue::Bool(true));
    // After removal the node reports disconnected again.
    let removed = rt.eval("_att.remove(); _att.isConnected").unwrap();
    assert_eq!(removed, lumen_core::JsValue::Bool(false));
}

#[test]
fn children_collection_item_named_and_index() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _q = document.createElement('div');
                document.body.appendChild(_q);
                _q.appendChild(document.createElement('span'));
                var _mid = document.createElement('span'); _mid.id = 'mid'; _q.appendChild(_mid);
                _q.appendChild(document.createElement('span'));
            "#).unwrap();
    let item = rt.eval("_q.children.item(1).id").unwrap();
    let named = rt.eval("_q.children.namedItem('mid').id").unwrap();
    let named_null = rt.eval("_q.children.namedItem('nope') === null").unwrap();
    let indexed = rt.eval("_q.children[0].tagName.toLowerCase()").unwrap();
    assert_eq!(item, lumen_core::JsValue::String("mid".into()));
    assert_eq!(named, lumen_core::JsValue::String("mid".into()));
    assert_eq!(named_null, lumen_core::JsValue::Bool(true));
    assert_eq!(indexed, lumen_core::JsValue::String("span".into()));
}

#[test]
fn element_before_inserts_node_before_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _cont = document.createElement('div');
                document.body.appendChild(_cont);
                var _a = document.createElement('span');
                var _b = document.createElement('div');
                _a.id = 'A'; _b.id = 'B';
                _cont.appendChild(_b);
                _b.before(_a);
            "#).unwrap();
    let first_id = rt.eval("_cont.children[0].id").unwrap();
    assert_eq!(first_id, lumen_core::JsValue::String("A".into()));
}

#[test]
fn element_after_inserts_node_after_target() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _cont2 = document.createElement('div');
                document.body.appendChild(_cont2);
                var _x = document.createElement('span');
                var _y = document.createElement('em');
                _x.id = 'X'; _y.id = 'Y';
                _cont2.appendChild(_x);
                _x.after(_y);
            "#).unwrap();
    let second_id = rt.eval("_cont2.children[1].id").unwrap();
    assert_eq!(second_id, lumen_core::JsValue::String("Y".into()));
}

#[test]
fn element_replace_with_swaps_element() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _cont3 = document.createElement('div');
                document.body.appendChild(_cont3);
                var _old = document.createElement('p');
                var _new = document.createElement('section');
                _old.id = 'OLD'; _new.id = 'NEW';
                _cont3.appendChild(_old);
                _old.replaceWith(_new);
            "#).unwrap();
    let tag = rt.eval("_cont3.children[0].id").unwrap();
    assert_eq!(tag, lumen_core::JsValue::String("NEW".into()));
}

#[test]
fn element_prepend_inserts_before_first_child() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _pcont = document.createElement('div');
                document.body.appendChild(_pcont);
                var _first = document.createElement('div');
                var _second = document.createElement('span');
                _first.id = 'FIRST'; _second.id = 'SECOND';
                _pcont.appendChild(_second);
                _pcont.prepend(_first);
            "#).unwrap();
    let first_id = rt.eval("_pcont.children[0].id").unwrap();
    assert_eq!(first_id, lumen_core::JsValue::String("FIRST".into()));
}

#[test]
fn element_replace_children_clears_and_sets() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _rccont = document.createElement('div');
                document.body.appendChild(_rccont);
                var _c1 = document.createElement('div');
                var _c2 = document.createElement('span');
                var _c3 = document.createElement('p');
                _rccont.appendChild(_c1);
                _rccont.appendChild(_c2);
                _rccont.replaceChildren(_c3);
            "#).unwrap();
    let count = rt.eval("_rccont.children.length").unwrap();
    assert_eq!(count, lumen_core::JsValue::Number(1.0));
    let tag = rt.eval("_rccont.children[0].tagName.toLowerCase()").unwrap();
    assert_eq!(tag, lumen_core::JsValue::String("p".into()));
}

// ── TreeWalker / NodeIterator tests ──────────────────────────────────────

#[test]
fn node_filter_constants_available() {
    let rt = v8_runtime_with_dom(make_doc());
    let accept = rt.eval("NodeFilter.FILTER_ACCEPT").unwrap();
    assert_eq!(accept, lumen_core::JsValue::Number(1.0));
    let show_all = rt.eval("NodeFilter.SHOW_ALL").unwrap();
    assert_eq!(show_all, lumen_core::JsValue::Number(0xFFFFFFFF_u32 as f64));
    let show_element = rt.eval("NodeFilter.SHOW_ELEMENT").unwrap();
    assert_eq!(show_element, lumen_core::JsValue::Number(1.0));
}

#[test]
fn tree_walker_exists_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt.eval("typeof window.TreeWalker === 'function'").unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn create_tree_walker_returns_walker_with_root() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _twroot = document.createElement('section');
                document.body.appendChild(_twroot);
                var _tw = document.createTreeWalker(_twroot, NodeFilter.SHOW_ELEMENT);
            "#).unwrap();
    let root_tag = rt.eval("_tw.root.tagName.toLowerCase()").unwrap();
    assert_eq!(root_tag, lumen_core::JsValue::String("section".into()));
}

#[test]
fn tree_walker_next_node_traverses_children() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _twc = document.createElement('section');
                document.body.appendChild(_twc);
                var _d1 = document.createElement('div');
                var _d2 = document.createElement('span');
                _d1.id = 'D1'; _d2.id = 'D2';
                _twc.appendChild(_d1);
                _twc.appendChild(_d2);
                var _tw2 = document.createTreeWalker(_twc, NodeFilter.SHOW_ELEMENT);
                var _n1 = _tw2.nextNode(); // D1
                var _n2 = _tw2.nextNode(); // D2
            "#).unwrap();
    let id1 = rt.eval("_n1 && _n1.id").unwrap();
    assert_eq!(id1, lumen_core::JsValue::String("D1".into()));
    let id2 = rt.eval("_n2 && _n2.id").unwrap();
    assert_eq!(id2, lumen_core::JsValue::String("D2".into()));
}

#[test]
fn tree_walker_previous_node_goes_back() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _twpc = document.createElement('article');
                document.body.appendChild(_twpc);
                var _da = document.createElement('div');
                var _db = document.createElement('span');
                _da.id = 'DA'; _db.id = 'DB';
                _twpc.appendChild(_da);
                _twpc.appendChild(_db);
                var _tw3 = document.createTreeWalker(_twpc, NodeFilter.SHOW_ELEMENT);
                _tw3.nextNode(); // DA
                _tw3.nextNode(); // DB
                var _prev = _tw3.previousNode(); // back to DA
            "#).unwrap();
    let id = rt.eval("_prev && _prev.id").unwrap();
    assert_eq!(id, lumen_core::JsValue::String("DA".into()));
}

#[test]
fn tree_walker_with_filter_function() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _twfc = document.createElement('aside');
                document.body.appendChild(_twfc);
                var _s1 = document.createElement('span');
                var _s2 = document.createElement('div');
                var _s3 = document.createElement('span');
                _s1.id = 'S1'; _s2.id = 'S2'; _s3.id = 'S3';
                _twfc.appendChild(_s1);
                _twfc.appendChild(_s2);
                _twfc.appendChild(_s3);
                var _tw4 = document.createTreeWalker(_twfc, NodeFilter.SHOW_ELEMENT, function(node) {
                    return node.tagName.toLowerCase() === 'span'
                        ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP;
                });
                var _fn1 = _tw4.nextNode(); // S1
                var _fn2 = _tw4.nextNode(); // S3 (S2=div skipped)
            "#).unwrap();
    let id1 = rt.eval("_fn1 && _fn1.id").unwrap();
    assert_eq!(id1, lumen_core::JsValue::String("S1".into()));
    let id2 = rt.eval("_fn2 && _fn2.id").unwrap();
    assert_eq!(id2, lumen_core::JsValue::String("S3".into()));
}

#[test]
fn node_iterator_exists_on_window() {
    let rt = v8_runtime_with_dom(make_doc());
    let ok = rt.eval("typeof window.NodeIterator === 'function'").unwrap();
    assert_eq!(ok, lumen_core::JsValue::Bool(true));
}

#[test]
fn node_iterator_next_node_and_previous_node() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _nic = document.createElement('nav');
                document.body.appendChild(_nic);
                var _ni_a = document.createElement('div');
                var _ni_b = document.createElement('span');
                _ni_a.id = 'NIA'; _ni_b.id = 'NIB';
                _nic.appendChild(_ni_a);
                _nic.appendChild(_ni_b);
                var _ni = document.createNodeIterator(_nic, NodeFilter.SHOW_ELEMENT);
                var _ni_n1 = _ni.nextNode(); // _nic itself
                var _ni_n2 = _ni.nextNode(); // NIA
                var _ni_n3 = _ni.nextNode(); // NIB
                var _ni_p1 = _ni.previousNode(); // back to NIA
            "#).unwrap();
    let n2_id = rt.eval("_ni_n2 && _ni_n2.id").unwrap();
    assert_eq!(n2_id, lumen_core::JsValue::String("NIA".into()));
    let p1_id = rt.eval("_ni_p1 && _ni_p1.id").unwrap();
    assert_eq!(p1_id, lumen_core::JsValue::String("NIA".into()));
}

#[test]
fn document_adopt_node_returns_node() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _orig = document.createElement('div');
                _orig.id = 'ADO';
                var _adopted = document.adoptNode(_orig);
            "#).unwrap();
    let id = rt.eval("_adopted && _adopted.id").unwrap();
    assert_eq!(id, lumen_core::JsValue::String("ADO".into()));
}

#[test]
fn document_import_node_returns_clone() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var _tmpl = document.createElement('p');
                _tmpl.id = 'IMP';
                var _imported = document.importNode(_tmpl, false);
            "#).unwrap();
    let id = rt.eval("_imported && _imported.id").unwrap();
    assert_eq!(id, lumen_core::JsValue::String("IMP".into()));
}

#[test]
fn get_bounding_rect_returns_values_from_runtime() {
    let rt = v8_runtime_with_dom(make_doc());
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        let body_id = super::find_element_by_tag(&doc, "body").unwrap();
        body_id.index() as u32
    };
    rt.update_layout_rects([(nid, [10.0, 20.0, 300.0, 150.0])].into_iter().collect());
    // The JS body element's __nid__ should match nid
    let rect_val = rt.eval(&format!("_lumen_get_bounding_rect({nid})")).unwrap();
    match rect_val {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(10.0));
            assert_eq!(arr[1], lumen_core::JsValue::Number(20.0));
            assert_eq!(arr[2], lumen_core::JsValue::Number(300.0));
            assert_eq!(arr[3], lumen_core::JsValue::Number(150.0));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn get_viewport_size_returns_updated_values() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.update_viewport_size(1920.0, 1080.0);
    let vp = rt.eval("_lumen_get_viewport_size()").unwrap();
    match vp {
        lumen_core::JsValue::Array(arr) => {
            assert_eq!(arr[0], lumen_core::JsValue::Number(1920.0));
            assert_eq!(arr[1], lumen_core::JsValue::Number(1080.0));
        }
        other => panic!("expected Array, got {other:?}"),
    }
}

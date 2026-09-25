//! BUG-1130 — a `ShadowRoot` lacked most of `Node`/`ParentNode`/
//! `DocumentOrShadowRoot`/`ShadowRoot` (DOM §4.2.2, §4.8; HTML §6.6.3):
//! `typeof sr.insertBefore === 'undefined'`, so Lit's first template render on
//! archive.org threw `t.insertBefore is not a function` and left the page empty.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true")
        .unwrap();
    rt.install_dom(
        doc,
        "https://example.test/",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    rt
}

/// Evaluates `expr`; an exception comes back as `THROW:<name>:<message>`.
fn s(rt: &V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.name + ':' + e.message; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

const SETUP: &str = "var host = document.createElement('div'); document.body.appendChild(host); \
                     var sr = host.attachShadow({mode:'open'}); ";

/// The report's own list (`shadow_members.html`): every name must be present.
#[test]
fn shadow_root_has_every_member_of_its_interfaces() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    let missing = s(
        &rt,
        "['insertBefore','replaceChild','appendChild','removeChild','contains','hasChildNodes',\
          'cloneNode','normalize','isEqualNode','isSameNode','compareDocumentPosition',\
          'lookupNamespaceURI','childNodes','firstChild','lastChild','parentNode','nodeType',\
          'nodeName','ownerDocument','isConnected','textContent','getElementById','querySelector',\
          'append','prepend','replaceChildren','moveBefore','firstElementChild','lastElementChild',\
          'childElementCount','elementFromPoint','activeElement','getAnimations','styleSheets',\
          'delegatesFocus','slotAssignment','clonable','serializable','onslotchange']\
          .filter(function(n){ return !(n in sr) || (sr[n] === undefined); }).join(',')",
    );
    assert_eq!(missing, "");
}

/// Lit's first render: a marker comment goes in through `insertBefore`.
#[test]
fn insert_before_and_tree_links_work_on_a_shadow_root() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    assert_eq!(
        s(
            &rt,
            "(function(){ var c = document.createComment('lit'); \
              var r = sr.insertBefore(c, sr.firstChild); \
              var a = document.createElement('a'); sr.insertBefore(a, c); \
              var b = document.createElement('b'); sr.replaceChild(b, a); \
              return [r === c, sr.childNodes.length, sr.firstChild === b, sr.lastChild === c, \
                      c.parentNode === sr, sr.firstElementChild === b, sr.lastElementChild === b, \
                      sr.childElementCount, sr.contains(c), sr.hasChildNodes()].join(); })()"
        ),
        "true,2,true,true,true,true,true,1,true,true"
    );
}

/// `Node` attributes answer as a fragment in a connected shadow tree.
#[test]
fn node_attributes_of_a_shadow_root() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    assert_eq!(
        s(
            &rt,
            "[sr.nodeType, sr.nodeName, sr.parentNode, sr.parentElement, sr.ownerDocument === document, \
              sr.isConnected, sr.nextSibling, sr.previousSibling, sr.isSameNode(host.shadowRoot), \
              sr instanceof Node, sr instanceof DocumentFragment, \
              Object.prototype.toString.call(sr)].join()"
        ),
        "11,#document-fragment,,,true,true,,,true,true,true,[object ShadowRoot]"
    );
    rt.eval("document.body.removeChild(host)").unwrap();
    assert_eq!(s(&rt, "sr.isConnected"), "false");
}

/// `ShadowRoot`'s own attributes (DOM §4.8) reflect the `attachShadow` init.
#[test]
fn shadow_root_init_attributes() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(
        s(
            &rt,
            "(function(){ var h = document.createElement('div'); \
              var r = h.attachShadow({mode:'closed', delegatesFocus:true, slotAssignment:'manual', \
                                      clonable:true, serializable:true}); \
              var h2 = document.createElement('span'); var r2 = h2.attachShadow({mode:'open'}); \
              return [r.mode, r.delegatesFocus, r.slotAssignment, r.clonable, r.serializable, \
                      r2.delegatesFocus, r2.slotAssignment, r2.clonable, r2.serializable, \
                      r2.onslotchange].join(); })()"
        ),
        "closed,true,manual,true,true,false,named,false,false,"
    );
}

/// `DocumentOrShadowRoot.activeElement` (HTML §6.6.3): the focused element
/// retargeted against the root, `null` when focus is outside its tree.
#[test]
fn shadow_root_active_element_is_retargeted() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    assert_eq!(
        s(
            &rt,
            "(function(){ var inp = document.createElement('input'); sr.appendChild(inp); \
              var inner = document.createElement('div'); sr.appendChild(inner); \
              var sr2 = inner.attachShadow({mode:'open'}); \
              var deep = document.createElement('input'); sr2.appendChild(deep); \
              var out = document.createElement('input'); document.body.appendChild(out); \
              var before = sr.activeElement; inp.focus(); var a = sr.activeElement === inp; \
              deep.focus(); var b = sr.activeElement === inner, c = sr2.activeElement === deep; \
              out.focus(); \
              return [before, a, b, c, sr.activeElement, sr2.activeElement].join(); })()"
        ),
        ",true,true,true,,"
    );
}

/// `DocumentOrShadowRoot.styleSheets` (CSSOM §6.3): only the registry's sheets
/// whose owner node is in this shadow tree.
#[test]
fn shadow_root_style_sheets_are_its_own() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    let nid = |expr: &str| match rt.eval(expr).unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("{other:?}"),
    };
    let head_style =
        nid("var hs = document.createElement('style'); document.head.appendChild(hs); hs.__nid__");
    let shadow_style =
        nid("var ss = document.createElement('style'); sr.appendChild(ss); ss.__nid__");
    let entry = |node| lumen_css_parser::StylesheetNodeEntry {
        node,
        sheet: Arc::new(lumen_css_parser::parse("p { color: red; }")),
        disabled: false,
    };
    rt.update_stylesheet_nodes(vec![entry(head_style), entry(shadow_style)]);
    assert_eq!(
        s(
            &rt,
            "(function(){ var other = document.createElement('p').attachShadow({mode:'open'}); \
              return [sr.styleSheets.length, sr.styleSheets[0].ownerNode === ss, \
                      sr.styleSheets instanceof StyleSheetList, other.styleSheets.length].join(); })()"
        ),
        "1,true,true,0"
    );
}

/// `getAnimations`/`elementFromPoint` exist and answer with no layout/animations.
#[test]
fn shadow_root_animations_and_hit_test_without_layout() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    assert_eq!(
        s(
            &rt,
            "[Array.isArray(sr.getAnimations()), sr.getAnimations().length, \
              sr.elementFromPoint(1, 1), sr.elementsFromPoint(1, 1).length].join()"
        ),
        "true,0,,0"
    );
}

/// `isEqualNode`/`normalize`/`compareDocumentPosition` reach the shadow root's `nid`.
#[test]
fn node_methods_run_on_a_shadow_root() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    assert_eq!(
        s(
            &rt,
            "(function(){ sr.appendChild(document.createTextNode('a')); \
              sr.appendChild(document.createTextNode('b')); sr.normalize(); \
              var p = document.createElement('p'); sr.appendChild(p); \
              return [sr.childNodes.length, sr.firstChild.data, sr.isEqualNode(sr), \
                      sr.isEqualNode(document.createElement('div')), \
                      sr.compareDocumentPosition(p) & 16, typeof sr.moveBefore].join(); })()"
        ),
        "2,ab,true,false,16,function"
    );
}

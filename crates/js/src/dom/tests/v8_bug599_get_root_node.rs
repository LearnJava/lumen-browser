//! BUG-599 — `Node.getRootNode()` on the live `document` singleton.
//!
//! The node wrappers got the method long ago
//! (`_LUMEN_WRAPPER_MEMBERS.getRootNode`, walks parents to the tree root), and
//! `DocumentFragment` carries its own copy, but the handwritten `document`
//! literal does not inherit `Node.prototype` and had no own copy — exactly the
//! shape of BUG-327 (`hasChildNodes`) and BUG-732 (`compareDocumentPosition`)
//! before those were fixed the same way.
//!
//! Two call sites make this a hard blocker rather than a rounding error:
//! the vendored `tools/wptrunner/wptrunner/testdriver-extra.js` selector
//! builder (`current = current.getRootNode().host`, on the path of nearly
//! every `test_driver_internal.*` action), and react-dom, which calls
//! `getRootNode()` on the app container — under a Next.js App Router that
//! container *is* `document`, so hydration died with
//! `Minified React error #446`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// DOM §4.4: the root of the document's own tree is the document itself.
/// This is the react-dom call that used to throw `getRootNode is not a
/// function` and take hydration down with it.
#[test]
fn document_get_root_node_is_the_document() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof document.getRootNode === 'function'"));
    assert!(is_true(&rt, "document.getRootNode() === document"));
    // `composed: true` cannot change the answer for a document — there is no
    // shadow root above it — but the argument must not upset the call either.
    assert!(is_true(&rt, "document.getRootNode({ composed: true }) === document"));
    assert!(is_true(&rt, "document.getRootNode(undefined) === document"));
}

/// An attached node and the document must agree on the root, and the answer
/// must be the very same `document` object identity that `ownerDocument`
/// hands out — `testdriver-extra.js` compares the two with `==`.
#[test]
fn attached_node_root_matches_document() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var _main = document.getElementById('main');").unwrap();
    assert!(is_true(&rt, "_main.getRootNode() === document"));
    assert!(is_true(&rt, "_main.getRootNode() === _main.ownerDocument"));
    assert!(is_true(
        &rt,
        "document.documentElement.getRootNode() === document.getRootNode()"
    ));
}

/// A still-detached subtree roots at its own topmost node, not at the
/// document — the case that distinguishes a real tree walk from a constant.
#[test]
fn detached_subtree_roots_at_its_own_top() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var _top = document.createElement('div'); \
         var _kid = document.createElement('span'); \
         _top.appendChild(_kid);",
    )
    .unwrap();
    assert!(is_true(&rt, "_kid.getRootNode() !== document"));
    assert!(is_true(&rt, "_kid.getRootNode().isSameNode(_top)"));
    assert!(is_true(&rt, "_top.getRootNode().isSameNode(_top)"));
}


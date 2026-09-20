//! BUG-1057 / GAP-DOCALLDDA — `document.all` and its `[[IsHTMLDDA]]` "unusual
//! behaviors" (HTML LS §obsolete). Split off from BUG-606 because no JS-only
//! fix can produce them: the slot comes from V8's `MarkAsUndetectable`, bound
//! locally in `cpp/undetectable.cc` (see `v8_runtime::html_all`).
//!
//! The assertions mirror WPT
//! `html/obsolete/requirements-for-implementations/other-elements-attributes-and-apis/document-all.html`
//! line for line, plus the collection surface that test does not cover.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Every assertion of the WPT test's first subtest, in its order.
#[test]
fn document_all_has_the_unusual_behaviors() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "Boolean(document.all) === false"));
    assert!(is_true(&rt, "document.all == undefined"));
    assert!(is_true(&rt, "document.all == null"));
    assert!(is_true(&rt, "(document.all != undefined) === false"));
    assert!(is_true(&rt, "(document.all != null) === false"));
    assert!(is_true(&rt, "document.all !== undefined"));
    assert!(is_true(&rt, "document.all !== null"));
    assert!(is_true(&rt, "(document.all === undefined) === false"));
    assert!(is_true(&rt, "(document.all === null) === false"));
    assert!(is_true(&rt, "typeof document.all === 'undefined'"));
    assert!(is_true(
        &rt,
        "(function() { if (document.all) { return false; } \
          if (!document.all) { return true; } return false; })()"
    ));
}

/// The WPT test's second subtest: the same object read into a local first, so
/// the behaviours have to belong to the value rather than to the property
/// access that produced it.
#[test]
fn the_unusual_behaviors_survive_assignment() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("var all = document.all;").unwrap();
    assert!(is_true(&rt, "Boolean(all) === false"));
    assert!(is_true(&rt, "all == undefined && all == null"));
    assert!(is_true(&rt, "all !== undefined && all !== null"));
    assert!(is_true(&rt, "typeof all === 'undefined'"));
}

/// The DDA slot must not leak to anything else — `document.applets` is the
/// nearest neighbour (another legacy collection built the same way) and must
/// stay an ordinary, truthy object.
#[test]
fn other_collections_keep_ordinary_semantics() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "typeof document.applets === 'object'"));
    assert!(is_true(&rt, "Boolean(document.applets) === true"));
    assert!(is_true(&rt, "document.applets != null"));
}

/// One and the same object on every read (`document.all === document.all`),
/// the way every engine answers it.
#[test]
fn document_all_is_the_same_object_every_time() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "document.all === document.all"));
}

/// Undetectable or not, it is still a collection: reads forward to the live
/// element list behind it.
#[test]
fn document_all_forwards_reads_to_a_live_collection() {
    let rt = v8_runtime_with_dom(make_doc());
    let before = rt.eval("document.all.length").unwrap();
    rt.eval(
        "var d = document.createElement('div'); d.id = 'probe'; \
         document.body.appendChild(d);",
    )
    .unwrap();
    let after = rt.eval("document.all.length").unwrap();
    assert_ne!(
        before, after,
        "document.all must be live — a newly appended element has to show up"
    );
    assert!(is_true(&rt, "document.all.probe.id === 'probe'"));
    assert!(is_true(&rt, "document.all.namedItem('probe').id === 'probe'"));
    assert!(is_true(
        &rt,
        "document.all[document.all.length - 1].id === 'probe'"
    ));
    assert!(is_true(&rt, "document.all.item(0).tagName === 'HTML'"));
    assert!(is_true(&rt, "'length' in document.all"));
}

/// HTML LS §obsolete gives `document.all` a `[[Call]]`: no argument yields
/// `undefined`, one argument behaves like `namedItem`. (V8 also *requires* the
/// call handler to exist — it CHECK-fails while instantiating an undetectable
/// template without one — so this test guards the handler's presence as much
/// as its behaviour.)
#[test]
fn document_all_is_callable_like_named_item() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(
        "var d = document.createElement('div'); d.id = 'probe'; \
         document.body.appendChild(d);",
    )
    .unwrap();
    assert!(is_true(&rt, "document.all() === undefined"));
    assert!(is_true(&rt, "document.all('probe').id === 'probe'"));
    assert!(is_true(&rt, "document.all('absent') === null"));
}


//! BUG-377 — `Node.baseURI`. The property was absent from every node kind
//! (not merely wrong): `'baseURI' in document` answered `false`, so a
//! helper opening with `document.baseURI.substring(...)` threw and took its
//! whole file with it. These tests pin the three things that made the fix
//! non-trivial: it is a `Node` attribute (so *every* node kind must answer,
//! including the four wrappers that are prototype-less literals), it is
//! `<base href>`-aware rather than a second name for `document.URL`, and it
//! is readonly (getter, no setter — the BUG-375 trap).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// Runtime on a real page URL — unlike the `v8_runtime_with_dom` twins
/// elsewhere in this file, which install with an empty URL and would
/// make every `baseURI` assertion trivially `''`.
fn runtime_at(doc: Arc<Mutex<Document>>, url: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, url, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

/// `html > head > [base?] > body > p("Hello")`. `base` is inserted only
/// when `base_href` is `Some`, so the same builder covers both the
/// "document URL wins" and the "`<base>` overrides it" cases.
fn doc_with_base(base_href: Option<&str>) -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let head = doc.create_element(QualName::html("head"));
    let body = doc.create_element(QualName::html("body"));
    let p = doc.create_element(QualName::html("p"));
    let text = doc.create_text("Hello");
    doc.append_child(doc.root(), html);
    doc.append_child(html, head);
    if let Some(href) = base_href {
        let base = doc.create_element(QualName::html("base"));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(base).data {
            attrs.push(lumen_dom::Attribute {
                name: QualName::html("href"),
                value: href.into(),
            });
        }
        doc.append_child(head, base);
    }
    doc.append_child(html, body);
    doc.append_child(body, p);
    doc.append_child(p, text);
    Arc::new(Mutex::new(doc))
}

fn string_of(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// The exact symptom line from the bug report: the property must exist,
/// not just read as `undefined`. `in` distinguishes "absent everywhere,
/// own and inherited" from "present but broken".
#[test]
fn base_uri_is_present_on_document() {
    let rt = runtime_at(doc_with_base(None), "https://example.com/a/page.html");
    assert!(is_true(&rt, "'baseURI' in document"));
    assert_eq!(string_of(&rt, "document.baseURI"), "https://example.com/a/page.html");
}

/// `Node`, not `Document`: elements, text and comment nodes answer too,
/// and all of them agree with the document.
#[test]
fn every_node_kind_reports_the_same_base_uri() {
    let rt = runtime_at(doc_with_base(None), "https://example.com/a/page.html");
    let expected = "https://example.com/a/page.html";
    assert_eq!(string_of(&rt, "document.body.baseURI"), expected);
    assert_eq!(string_of(&rt, "document.documentElement.baseURI"), expected);
    assert_eq!(string_of(&rt, "document.querySelector('p').firstChild.baseURI"), expected);
    assert_eq!(string_of(&rt, "document.createTextNode('x').baseURI"), expected);
    assert_eq!(string_of(&rt, "document.createComment('c').baseURI"), expected);
    assert_eq!(string_of(&rt, "document.createElement('div').baseURI"), expected);
    assert_eq!(string_of(&rt, "new Text('t').baseURI"), expected);
    assert_eq!(string_of(&rt, "new Comment('c').baseURI"), expected);
}

/// The two node wrappers that are plain literals with no [[Prototype]],
/// so the shared `Node.prototype` accessor never reaches them and they
/// need own copies — the easiest half of this fix to leave out.
#[test]
fn prototype_less_wrappers_report_base_uri_too() {
    let rt = runtime_at(doc_with_base(None), "https://example.com/a/page.html");
    let expected = "https://example.com/a/page.html";
    assert_eq!(string_of(&rt, "document.createDocumentFragment().baseURI"), expected);
    rt.eval("var _host = document.createElement('div'); \
                     var _sr = _host.attachShadow({ mode: 'open' });")
        .unwrap();
    assert_eq!(string_of(&rt, "_sr.baseURI"), expected);
}

/// HTML LS §4.2.3 — `<base href>` overrides the document URL, resolved
/// against it. This is what separates `baseURI` from `document.URL`;
/// returning the document URL unconditionally would pass every test
/// above and still be wrong.
#[test]
fn base_element_overrides_document_url() {
    let rt = runtime_at(
        doc_with_base(Some("/other/")),
        "https://example.com/a/page.html",
    );
    assert_eq!(string_of(&rt, "document.baseURI"), "https://example.com/other/");
    // Same answer through the Node.prototype accessor, not just the
    // document's own copy.
    assert_eq!(string_of(&rt, "document.body.baseURI"), "https://example.com/other/");
    // `document.URL` keeps reporting the document's own URL.
    assert_eq!(string_of(&rt, "document.URL"), "https://example.com/a/page.html");
}

/// A relative `<base href>` resolves against the document URL rather
/// than being handed back verbatim.
#[test]
fn relative_base_href_is_resolved() {
    let rt = runtime_at(
        doc_with_base(Some("sub/dir/")),
        "https://example.com/a/page.html",
    );
    assert_eq!(string_of(&rt, "document.baseURI"), "https://example.com/a/sub/dir/");
}

/// A document with no browsing context is `about:blank` all the way
/// through — it must not inherit the live page's base URL off
/// `Node.prototype`.
#[test]
fn detached_document_base_uri_is_about_blank() {
    let rt = runtime_at(doc_with_base(None), "https://example.com/a/page.html");
    rt.eval("var _d = document.implementation.createHTMLDocument('t');").unwrap();
    assert_eq!(string_of(&rt, "_d.baseURI"), "about:blank");
    assert_eq!(string_of(&rt, "_d.URL"), "about:blank");
}

/// Readonly per WebIDL. The failure mode to avoid is BUG-375's: an
/// empty setter accepts the write, reports success in strict mode and
/// loses the value without a trace. A getter-only accessor ignores the
/// assignment in sloppy mode and throws in strict mode — either way the
/// value is unchanged, which is what this asserts.
#[test]
fn base_uri_is_readonly() {
    let rt = runtime_at(doc_with_base(None), "https://example.com/a/page.html");
    assert!(is_true(
        &rt,
        "Object.getOwnPropertyDescriptor(document, 'baseURI').set === undefined"
    ));
    assert!(is_true(
        &rt,
        "Object.getOwnPropertyDescriptor(Node.prototype, 'baseURI').set === undefined"
    ));
    rt.eval("try { document.baseURI = 'https://evil.test/'; } catch (e) {}").unwrap();
    rt.eval("try { document.body.baseURI = 'https://evil.test/'; } catch (e) {}").unwrap();
    assert_eq!(string_of(&rt, "document.baseURI"), "https://example.com/a/page.html");
    assert_eq!(string_of(&rt, "document.body.baseURI"), "https://example.com/a/page.html");
}

/// The line the bug was found on — `fledge-util.sub.js` line 3, which
/// every one of that category's 36 files loads.
#[test]
fn fledge_util_base_url_line_works() {
    let rt = runtime_at(doc_with_base(None), "https://example.com/a/page.html");
    let code = "document.baseURI.substring(0, document.baseURI.lastIndexOf('/') + 1)";
    assert_eq!(string_of(&rt, code), "https://example.com/a/");
}

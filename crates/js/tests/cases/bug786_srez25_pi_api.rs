//! GAP-XMLDOC срез 25 (BUG-786): `document.createProcessingInstruction`'s
//! detached, JS-only PI object was missing `cloneNode`/`getRootNode` entirely
//! (unlike `Comment`/`Text`, which get them for free from the arena-backed
//! wrapper `document.createComment`/`createTextNode` build on), and
//! `createProcessingInstruction` itself did not exist at all on any detached
//! document (`DOMImplementation.createDocument`/`createHTMLDocument`,
//! `DOMParser().parseFromString`) — only the live `document` had it.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

#[test]
fn pi_clone_node_makes_an_independent_copy() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var pi = document.createProcessingInstruction('target', 'data');
var copy = pi.cloneNode();
copy !== pi
  && copy instanceof ProcessingInstruction
  && copy.target === 'target'
  && copy.data === 'data'
  && (copy.data = 'other', pi.data === 'data' && copy.data === 'other')
"#
    ));
}

#[test]
fn pi_get_root_node_without_a_parent_returns_itself() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var pi = document.createProcessingInstruction('target', 'data');
pi.getRootNode() === pi
"#
    ));
}

#[test]
fn create_document_document_has_create_processing_instruction() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var doc = document.implementation.createDocument(null, '', null);
var pi = doc.createProcessingInstruction('xml-stylesheet', 'href="a.css"');
pi instanceof ProcessingInstruction
  && pi.target === 'xml-stylesheet'
  && pi.data === 'href="a.css"'
"#
    ));
}

#[test]
fn create_html_document_has_create_processing_instruction() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var doc = document.implementation.createHTMLDocument('t');
var pi = doc.createProcessingInstruction('foo', 'bar');
pi instanceof ProcessingInstruction && pi.target === 'foo' && pi.data === 'bar'
"#
    ));
}

// `DOMParser().parseFromString(...)` builds its own, independent virtual-node
// document family (`VDocument`/`VElement`/`VComment`/`VText`, `dom_parser.rs`)
// that never participates in `instanceof` against the global interfaces
// (Phase 0 limitation, predates this срез) and has no `createProcessingInstruction`
// at all — out of scope here; `_lumen_build_detached_document` (this срез's
// `DOMImplementation.createDocument`/`createHTMLDocument` fix) is a completely
// separate code path.

#[test]
fn create_processing_instruction_still_validates_target_and_data() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var doc = document.implementation.createDocument(null, '', null);
var threwOnBadTarget = false;
try { doc.createProcessingInstruction('1bad', 'x'); } catch (e) { threwOnBadTarget = e instanceof DOMException && e.name === 'InvalidCharacterError'; }
var threwOnBadData = false;
try { doc.createProcessingInstruction('ok', 'a?>b'); } catch (e) { threwOnBadData = e instanceof DOMException && e.name === 'InvalidCharacterError'; }
threwOnBadTarget && threwOnBadData
"#
    ));
}

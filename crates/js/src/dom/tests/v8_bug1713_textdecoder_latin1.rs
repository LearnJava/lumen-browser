//! BUG-1713: `TextDecoder` принимает метки семейства windows-1252
//! (`ascii`/`latin1`/`iso-8859-1`/…) — WHATWG Encoding §4.2.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn eval_string(src: &str) -> String {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "https://example.com/", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    match rt.eval(src).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn legacy_latin1_labels_resolve_to_windows_1252() {
    let out = eval_string(
        "['ascii','us-ascii','latin1','ISO-8859-1',' l1 ','cp1252','windows-1252','iso_8859-1:1987']
            .map(function(l) { return new TextDecoder(l).encoding; }).join(',')",
    );
    assert_eq!(out, ["windows-1252"; 8].join(","));
}

#[test]
fn windows_1252_decodes_c1_punctuation_and_latin1() {
    let out = eval_string("new TextDecoder('latin1').decode(new Uint8Array([0x80, 0xE9, 0x41, 0x81]))");
    assert_eq!(out, "\u{20AC}\u{E9}A\u{81}");
}

#[test]
fn windows_1252_is_not_fatal_on_high_bytes() {
    let out = eval_string(
        "new TextDecoder('ascii', { fatal: true }).decode(new Uint8Array([0xFF, 0x9F]))",
    );
    assert_eq!(out, "\u{FF}\u{178}");
}

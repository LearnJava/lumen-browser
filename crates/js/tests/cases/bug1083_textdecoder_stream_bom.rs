//! BUG-1083 — `TextDecoder` streaming: a BOM split across chunks must still be
//! stripped (the stream start is consumed by the first decoded bytes, not by
//! the first call).
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(
        doc,
        "https://example.com/",
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
        None,
    )
    .unwrap();
    rt
}

fn str_eval(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::String(s)) => s,
        Ok(other) => panic!("expected a string from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

#[test]
fn split_utf8_bom_is_stripped_in_streaming_decode() {
    let rt = make_rt();
    assert_eq!(
        str_eval(
            &rt,
            "var d = new TextDecoder(); d.decode(new Uint8Array([0xEF,0xBB]), {stream:true}) + '|' + d.decode(new Uint8Array([0xBF,0x40]))"
        ),
        "|@"
    );
    assert_eq!(
        str_eval(
            &rt,
            "var d = new TextDecoder(); d.decode(new Uint8Array([0xEF]), {stream:true}) + '|' + d.decode(new Uint8Array([0xBB,0xBF,0x40]))"
        ),
        "|@"
    );
}

#[test]
fn mid_stream_bom_bytes_are_kept() {
    let rt = make_rt();
    assert_eq!(
        str_eval(
            &rt,
            "var d = new TextDecoder(); d.decode(new Uint8Array([0x61]), {stream:true}); String(d.decode(new Uint8Array([0xEF,0xBB,0xBF])).length)"
        ),
        "1"
    );
}

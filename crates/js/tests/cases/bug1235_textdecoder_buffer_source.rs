//! BUG-1235 — `TextDecoder.decode` must accept ANY `ArrayBufferView` as its
//! BufferSource argument. A `Uint8ClampedArray` (what `ImageData.data` is) used
//! to fall through to an empty buffer, so decoding returned `""` and every
//! WPT `security-features` image probe failed on `JSON.parse`.
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
fn decode_accepts_uint8clampedarray_and_dataview() {
    let rt = make_rt();
    assert_eq!(
        str_eval(
            &rt,
            "new TextDecoder('ascii').decode(new Uint8ClampedArray([0x7B, 0x7D]))"
        ),
        "{}"
    );
    assert_eq!(
        str_eval(
            &rt,
            "new TextDecoder('utf-8').decode(new DataView(new Uint8Array([0x40, 0x41]).buffer))"
        ),
        "@A"
    );
    // A view with a non-zero byteOffset decodes only its own window.
    assert_eq!(
        str_eval(
            &rt,
            "var u = new Uint8ClampedArray([0x61, 0x62, 0x63, 0x64]); \
             new TextDecoder().decode(new Uint8ClampedArray(u.buffer, 1, 2))"
        ),
        "bc"
    );
}

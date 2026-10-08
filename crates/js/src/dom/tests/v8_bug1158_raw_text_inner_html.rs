//! BUG-1158 — `script.innerHTML = code` (React's `dangerouslySetInnerHTML` on
//! `<script>`) parsed `code` with a generic fragment context, so `a<e` started
//! a tag and the body was cut there (yahoo: `Unexpected token ':'`).

use super::*;
use crate::v8_runtime::V8JsRuntime;

#[test]
fn inner_html_on_raw_text_elements_keeps_lt() {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    let r = rt.eval(
        "var c='for(var a=0;a<e;a++){}'; var s=document.createElement('script'); s.innerHTML=c; \
         var st=document.createElement('style'); st.innerHTML='a<b{}'; \
         var t=document.createElement('textarea'); t.innerHTML='a<b>&amp;c'; \
         [s.textContent===c, st.textContent, t.textContent].join('|')",
    );
    assert!(matches!(r, Ok(lumen_core::JsValue::String(ref v)) if v == "true|a<b{}|a<b>&c"), "{r:?}");
}

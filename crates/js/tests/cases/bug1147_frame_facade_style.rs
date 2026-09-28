//! BUG-1147 — `style`/`classList`/`dataset` у кросс-фреймового Element-фасада
//! (`frame_bridge.rs::frameElem`) поверх настоящего V8-шима. До фикса все три
//! были `undefined`: w3schools (FastCMP) падал на
//! `iframe.contentDocument.body.style.cssText = …` с
//! `Cannot set properties of undefined (setting 'cssText')`.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

/// Страница с `<iframe>`, под-документ которого зарегистрирован в бридже;
/// `d` — `contentDocument` фрейма.
fn frame_rt(child_html: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    let nid = match rt.eval("var f = document.createElement('iframe'); f.__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("expected node id, got {other:?}"),
    };
    rt.register_frame_document(
        nid,
        Arc::new(Mutex::new(lumen_html_parser::parse(child_html))),
        "about:blank".to_owned(),
        None,
        true,
        false,
        None,
    );
    rt.eval("var d = f.contentDocument;").unwrap();
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
fn frame_facade_style_is_live_css_style_declaration() {
    let rt = frame_rt("<html><body><p id='x' style='color: red'>hi</p></body></html>");
    // Сценарий FastCMP: cssText на documentElement/body под-документа.
    assert!(bool_eval(
        &rt,
        "var h = d.documentElement, b = d.body; \
         h.style instanceof CSSStyleDeclaration && h.style === h.style && \
         (b.style.cssText = 'width: 10px; margin: 0px', true) && \
         b.getAttribute('style') === b.style.cssText && \
         b.style.width === '10px' && b.style.marginTop === '0px'"
    ));
    // Разметочный style="" читается, запись свойства идёт в атрибут ребёнка.
    assert!(bool_eval(
        &rt,
        "var p = d.getElementById('x'); \
         p.style.color === 'red' && \
         (p.style.display = 'none', p.getAttribute('style').indexOf('display: none') !== -1) && \
         p.style.getPropertyValue('display') === 'none' && \
         p.style.removeProperty('color') === 'red' && p.style.color === ''"
    ));
    // Атрибут, изменённый в обход фасада стиля, виден при следующем чтении;
    // [PutForwards=cssText] — присваивание строки самому `style`.
    assert!(bool_eval(
        &rt,
        "p.setAttribute('style', 'height: 5px'); \
         var ok = p.style.height === '5px' && p.style.display === ''; \
         p.style = 'top: 1px'; \
         ok && p.style.top === '1px' && p.getAttribute('style') === p.style.cssText"
    ));
    // Главный документ не затронут: стиль его элемента по-прежнему живёт в
    // атрибуте ЕГО узла, а не узла под-документа с тем же номером.
    assert!(bool_eval(
        &rt,
        "var m = document.createElement('div'); m.style.width = '3px'; \
         m.getAttribute('style') === m.style.cssText && m.style.width === '3px' && \
         f.style.cssText === ''"
    ));
}

#[test]
fn frame_facade_class_list_is_live_dom_token_list() {
    let rt = frame_rt("<html><body><p id='x' class='a b'>hi</p></body></html>");
    assert!(bool_eval(
        &rt,
        "var p = d.getElementById('x'), cl = p.classList; \
         cl instanceof DOMTokenList && cl === p.classList && \
         cl.length === 2 && cl.contains('a') && cl[1] === 'b' && \
         (cl.add('c'), cl.remove('a'), p.className === 'b c') && \
         cl.toggle('d') === true && p.getAttribute('class') === 'b c d' && \
         (p.className = 'z', cl.value === 'z' && Array.from(cl).join() === 'z')"
    ));
}

#[test]
fn frame_facade_dataset_is_live_dom_string_map() {
    let rt = frame_rt("<html><body><p id='x' data-foo-bar='1'>hi</p></body></html>");
    assert!(bool_eval(
        &rt,
        "var p = d.getElementById('x'), ds = p.dataset; \
         ds instanceof DOMStringMap && ds === p.dataset && \
         ds.fooBar === '1' && 'fooBar' in ds && ds.missing === undefined && \
         (ds.newKey = 'v', p.getAttribute('data-new-key') === 'v') && \
         Object.keys(ds).join() === 'fooBar,newKey' && \
         (delete ds.fooBar, !p.hasAttribute('data-foo-bar'))"
    ));
}

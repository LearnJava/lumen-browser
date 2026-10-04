//! GAP-SOFTNAV-S1 — синхронная атрибуция мягкой навигации: доверенный
//! click/keydown, внутри него `pushState` со сменой URL и вставка узла, на
//! ближайшем rAF одна запись `soft-navigation`. Без клика записей нет.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

/// Раскладки в стенде нет: боксы задаются руками по `__nid__` узла.
fn give_box(rt: &V8JsRuntime, node_expr: &str) {
    let lumen_core::JsValue::Number(nid) = rt.eval(&format!("{node_expr}.__nid__")).unwrap() else {
        panic!("no nid for {node_expr}");
    };
    rt.update_layout_rects([(nid as u32, [0.0, 0.0, 100.0, 40.0])].into_iter().collect());
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// Кнопка с обработчиком: `pushState` + `appendChild` (готовый `view` в `body`).
const SETUP: &str = "\
    var btn = document.createElement('button'); document.body.appendChild(btn);\
    var nid = btn.__nid__; var view = document.createElement('div');\
    function route() {\
        history.pushState(null, '', '/about');\
        document.body.appendChild(view);\
    }\
    function entries() { return performance.getEntriesByType('soft-navigation'); }";

#[test]
fn trusted_click_with_push_state_and_insert_delivers_one_entry_on_next_frame() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    give_box(&rt, "view");
    rt.eval("btn.addEventListener('click', route);").unwrap();
    rt.eval("_lumen_dispatch_mouse_event(nid, 'click', 1, 1, 0, 1, 0);").unwrap();
    assert!(is_true(&rt, "entries().length === 0"), "запись ждёт rAF");
    rt.eval("_lumen_run_raf_callbacks(0);").unwrap();
    assert!(is_true(
        &rt,
        "entries().length === 1 && entries()[0].name === 'https://example.test/about' \
         && entries()[0] instanceof PerformanceSoftNavigationEntry"
    ));
    rt.eval("_lumen_run_raf_callbacks(0);").unwrap();
    assert!(is_true(&rt, "entries().length === 1"), "не больше одной записи на взаимодействие");
}

#[test]
fn trusted_keydown_opens_the_interaction_too() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    give_box(&rt, "view");
    rt.eval("btn.addEventListener('keydown', route);").unwrap();
    rt.eval("_lumen_dispatch_key_event(nid, 'keydown', 'Enter', 'Enter', 13, 0, 0, false, false);")
        .unwrap();
    rt.eval("_lumen_run_raf_callbacks(0);").unwrap();
    assert!(is_true(&rt, "entries().length === 1"));
}

#[test]
fn without_a_click_nothing_is_recorded() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval("route(); _lumen_run_raf_callbacks(0);").unwrap();
    assert!(is_true(&rt, "entries().length === 0"));
}

#[test]
fn script_dispatched_click_does_not_open_the_interaction() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval(
        "btn.addEventListener('click', route);\
         btn.dispatchEvent(new Event('click', { isTrusted: true })); btn.click();\
         _lumen_run_raf_callbacks(0);",
    )
    .unwrap();
    assert!(is_true(&rt, "entries().length === 0"));
}

#[test]
fn a_click_that_only_pushes_state_or_only_inserts_records_nothing() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval("btn.addEventListener('click', function() { history.pushState(null, '', '/only-url'); });")
        .unwrap();
    rt.eval("_lumen_dispatch_mouse_event(nid, 'click', 1, 1, 0, 1, 0); _lumen_run_raf_callbacks(0);")
        .unwrap();
    assert!(is_true(&rt, "entries().length === 0"));
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval("btn.addEventListener('click', function() { document.body.appendChild(document.createElement('p')); });")
        .unwrap();
    rt.eval("_lumen_dispatch_mouse_event(nid, 'click', 1, 1, 0, 1, 0); _lumen_run_raf_callbacks(0);")
        .unwrap();
    assert!(is_true(&rt, "entries().length === 0"));
}

#[test]
fn same_url_replace_state_is_not_a_navigation() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval(
        "btn.addEventListener('click', function() {\
            history.replaceState({ a: 1 }, '', location.href);\
            document.body.appendChild(document.createElement('div')); });",
    )
    .unwrap();
    rt.eval("_lumen_dispatch_mouse_event(nid, 'click', 1, 1, 0, 1, 0); _lumen_run_raf_callbacks(0);")
        .unwrap();
    assert!(is_true(&rt, "entries().length === 0"));
}

#[test]
fn a_node_without_a_box_is_not_a_soft_navigation() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    rt.eval("btn.addEventListener('click', route);").unwrap();
    rt.eval("_lumen_dispatch_mouse_event(nid, 'click', 1, 1, 0, 1, 0); _lumen_run_raf_callbacks(0);")
        .unwrap();
    assert!(is_true(&rt, "entries().length === 0"));
}

#[test]
fn inner_html_swap_counts_as_the_insertion() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(SETUP).unwrap();
    give_box(&rt, "document.body");
    rt.eval(
        "btn.addEventListener('click', function() {\
            history.pushState(null, '', '/inner');\
            document.body.innerHTML = '<main>new view</main>'; });",
    )
    .unwrap();
    rt.eval("_lumen_dispatch_mouse_event(nid, 'click', 1, 1, 0, 1, 0); _lumen_run_raf_callbacks(0);")
        .unwrap();
    assert!(is_true(&rt, "entries().length === 1 && entries()[0].name.endsWith('/inner')"));
}

#[test]
fn soft_navigation_is_a_supported_entry_type() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, "PerformanceObserver.supportedEntryTypes.indexOf('soft-navigation') !== -1"));
}

//! BUG-1811: preload-скрипт (`script.addPreloadScript`) исполняется в каждом
//! новом документе — и перестаёт исполняться после
//! `script.removePreloadScript`.
//!
//! Регистрация процесс-глобальна (как оверрайды BUG-295), поэтому тест
//! заводит свой уникальный id и в конце его снимает: другие тесты того же
//! процесса про preload-реестр ничего не спрашивают.

use super::*;

const ID: &str = "bug1811-test";

#[test]
fn preload_script_runs_in_each_new_document_until_removed() {
    // Битый скрипт в реестре не должен ломать бутстрап следующего документа;
    // проверяем это отдельной записью, которую тут же снимаем.
    crate::v8_runtime::set_global_preload_script("bug1811-broken", "this is not js (");
    crate::v8_runtime::set_global_preload_script(
        ID,
        "() => { window.__pre = (window.__pre || 0) + 1; }",
    );

    let first = runtime_with_dom(make_doc(), "https://preload.example/index.html");
    assert!(
        matches!(first.eval("window.__pre").unwrap(), JsValue::Number(n) if n == 1.0),
        "preload не исполнился в первом документе"
    );
    crate::v8_runtime::remove_global_preload_script("bug1811-broken");

    let second = runtime_with_dom(make_doc(), "https://preload.example/other.html");
    assert!(
        matches!(second.eval("window.__pre").unwrap(), JsValue::Number(n) if n == 1.0),
        "preload не исполнился во втором документе"
    );

    assert!(crate::v8_runtime::remove_global_preload_script(ID), "id был зарегистрирован");
    let third = runtime_with_dom(make_doc(), "https://preload.example/third.html");
    assert!(
        matches!(third.eval("typeof window.__pre").unwrap(), JsValue::String(ref s) if s == "undefined"),
        "снятый preload всё ещё исполняется"
    );
}

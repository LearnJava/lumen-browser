//! Нативный стриминг WebAssembly (WASM-V8-NATIVE-S2): окно, асинхронные тесты.
//!
//! Промисы `compileStreaming`/`instantiateStreaming` разрешаются через
//! очередь задач платформы V8 (BUG-1752: её разбирает сам JS-поток), поэтому
//! паттерн здесь тот же, что в `bug1752_platform_queue.rs`: скрипт пишет
//! результат в глобальную переменную, тест спит **не отправляя команд**, и
//! только потом её читает. MIME-кейсы повторяют `contenttype.any.js`
//! (параметры в заголовке отвергаются, регистр не важен), пересылка причины —
//! `rejected-arg.any.js`.

use super::*;

/// Валидный модуль `add(i32,i32)->i32` — тот же байткод, что в
/// `dom::tests::v8_wasm_native`.
const ADD_BYTES: &str = "[0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11]";

fn rt_with_dom() -> V8JsRuntime {
    let rt = rt();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

/// `compileStreaming` с корректным `Content-Type: application/wasm`
/// разрешается `WebAssembly.Module`.
#[test]
fn wasm_stream_compile_resolves_module_for_application_wasm() {
    let rt = rt_with_dom();
    rt.eval(&format!(
        "var bytes = new Uint8Array({ADD_BYTES});\
         var state = 'pending';\
         WebAssembly.compileStreaming(new Response(bytes, {{headers: {{'Content-Type': 'application/wasm'}}}}))\
           .then(function (m) {{ state = m instanceof WebAssembly.Module ? 'module' : 'not-module'; }})\
           .catch(function (e) {{ state = 'rejected:' + e.name; }});\
         undefined;"
    ))
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        rt.eval("state").unwrap(),
        JsValue::String("module".into()),
        "compileStreaming с application/wasm должен разрешиться Module-ом"
    );
}

/// `Content-Type: application/wasm;` — заголовок с параметром обязан
/// отвергаться `TypeError` (побайтовое равенство, WPT `contenttype.any.js`).
#[test]
fn wasm_stream_compile_rejects_typeerror_for_mime_with_parameter() {
    let rt = rt_with_dom();
    rt.eval(&format!(
        "var bytes = new Uint8Array({ADD_BYTES});\
         var state = 'pending';\
         WebAssembly.compileStreaming(new Response(bytes, {{headers: {{'Content-Type': 'application/wasm;'}}}}))\
           .then(function () {{ state = 'resolved'; }})\
           .catch(function (e) {{ state = e.name; }});\
         undefined;"
    ))
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        rt.eval("state").unwrap(),
        JsValue::String("TypeError".into()),
        "MIME с параметром должен отвергаться TypeError"
    );
}

/// Отклонение исходного промиса пересылается в обещание стриминга тем же
/// значением (WPT `rejected-arg.any.js`: `promise_rejects_exactly`).
#[test]
fn wasm_stream_compile_forwards_source_rejection_value() {
    let rt = rt_with_dom();
    rt.eval(
        "var state = 'pending';\
         WebAssembly.compileStreaming(Promise.reject({code: 'boom'}))\
           .then(function () { state = 'resolved'; })\
           .catch(function (e) { state = 'code=' + e.code; });\
         undefined;",
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        rt.eval("state").unwrap(),
        JsValue::String("code=boom".into()),
        "причина отклонения source должна дойти до обещания стриминга без изменений"
    );
}

/// `instantiateStreaming` разрешается `{module, instance}` (импортов у модуля
/// нет, V8 разбирает их сам).
#[test]
fn wasm_stream_instantiate_resolves_module_and_instance() {
    let rt = rt_with_dom();
    rt.eval(&format!(
        "var bytes = new Uint8Array({ADD_BYTES});\
         var state = 'pending';\
         WebAssembly.instantiateStreaming(new Response(bytes, {{headers: {{'Content-Type': 'application/wasm'}}}}))\
           .then(function (r) {{\
             state = (r.module instanceof WebAssembly.Module && r.instance instanceof WebAssembly.Instance)\
               ? 'source' : 'shape';\
           }})\
           .catch(function (e) {{ state = 'rejected:' + e.name; }});\
         undefined;"
    ))
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        rt.eval("state").unwrap(),
        JsValue::String("source".into()),
        "instantiateStreaming должен дать {{module, instance}}"
    );
}

//! BUG-1826 срез 3: native-фасад `Function.prototype.toString` для шимовых
//! функций. Станичный код (Cloudflare challenge VM — в его строковой таблице
//! `'toString'` + `'[native code]'` и имена проверяемых API) отличает
//! «настоящий» браузер от подменённого по печати функций; у Chrome все
//! платформенные функции печатаются как `[native code]`, а шимы Lumen до этого
//! куска отдавали свой исходник (71 функция из toString-пробы среза 2).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

/// Тот же список, что вернула toString-проба среза 2 c `JS-SRC`: ровно их и
/// проверяем — это поверхность, которую читает challenge VM.
#[test]
fn shim_functions_print_as_native() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var bad = [];\
             ['fetch', 'setTimeout', 'setInterval', 'requestAnimationFrame', 'Worker',\
              'Blob', 'PerformanceObserver', 'ReadableStream'].forEach(function (g) {\
                 var s = globalThis[g].toString();\
                 if (!s.startsWith('function ' + g + '() { [native code] }')) bad.push(g + ':' + s.slice(0, 40));\
             });\
             ['createElement', 'getElementById', 'querySelector', 'addEventListener'].forEach(function (m) {\
                 var s = document[m].toString();\
                 if (!s.includes('[native code]')) bad.push('document.' + m);\
             });\
             [ReadableStream.prototype.pipeTo, ReadableStream.prototype.getReader,\
              TextEncoder.prototype.encode, TextDecoder.prototype.decode,\
              XMLHttpRequest.prototype.open, URL.createObjectURL].forEach(function (f) {\
                 if (!f.toString().includes('[native code]')) bad.push(String(f.name));\
             });\
             bad.join(',')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("".into()));
}

/// Геттеры прототипов печатаются в формате Chrome: `function get x() {…}`.
#[test]
fn prototype_getters_print_as_native_accessors() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var g = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(performance), 'timeOrigin').get;\
             g.toString()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("function get timeOrigin() { [native code] }".into())
    );
}

/// Страничный код после шима не маскируется: свои функции страницы печатаются
/// исходником (иначе детект в обратную сторону — VM проверяет и собственный код).
#[test]
fn page_functions_keep_their_source() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "function __pagefn__() { return 1; }\
             var s = __pagefn__.toString();\
             s.includes('{ return 1; }') && !s.includes('[native code]')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// V8-нативы и сам патч ведут себя по спецификации: родные функции остаются
/// нативными, `toString` на не-функции бросает `TypeError`.
#[test]
fn v8_natives_and_tostring_contract_unchanged() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var r = [];\
             r.push(Math.random.toString().includes('[native code]'));\
             r.push(Function.prototype.toString.toString().includes('[native code]'));\
             r.push(Array.prototype.map.toString().includes('[native code]'));\
             try { Function.prototype.toString.call(123); r.push('no-throw'); }\
             catch (e) { r.push(e.name); }\
             r.join(',')",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("true,true,true,TypeError".into())
    );
}

/// Фасад работает и в воркерной склейке — challenge заводит blob-воркеры
/// (срез 1), а TextEncoder/ReadableStream там тоже шимовые.
#[test]
fn worker_scope_shim_functions_print_as_native() {
    let rt = crate::v8_runtime::V8JsRuntime::new().unwrap();
    crate::worker::install_worker_scope_globals_v8(&rt, None).unwrap();
    // FINGERPRINT-JS-REGISTRY: обход реестра — отдельный шаг после install
    // (у dedicated/shared/service worker он в хвосте install_*_globals_v8).
    rt.eval(crate::dom::NATIVE_REGISTRY_SWEEP).unwrap();
    let r = rt
        .eval(
            "[TextEncoder.prototype.encode.toString(), TextDecoder.prototype.decode.toString(),\
              ReadableStream.prototype.getReader.toString()].join('|')",
        )
        .unwrap();
    let s = match r {
        lumen_core::JsValue::String(s) => s,
        other => panic!("unexpected: {other:?}"),
    };
    assert!(s.contains("[native code]"), "worker toString: {s}");
}

/// BUG-1826 срез 5: начальный about:blank-кадр (первое чтение `contentWindow`)
/// в Chrome отвечает '[object Window]'/'[object HTMLDocument]', а его `eval` —
/// нативная функция чужого realm: `function eval() { [native code] }`.
/// До среза фасад печатался '[object Object]', кловер моста — своим исходником;
/// challenge-VM на этих двух чтениях уходила в вечный перепроб.
#[test]
fn initial_frame_facade_prints_like_chrome() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var f = document.createElement('iframe');\
             document.body.appendChild(f);\
             var w = f.contentWindow;\
             var ev = w.eval;\
             var out = [];\
             out.push(Object.prototype.toString.call(w) === '[object Window]');\
             out.push(Object.prototype.toString.call(f.contentDocument) === '[object HTMLDocument]');\
             out.push(typeof ev === 'function');\
             out.push(ev.toString() === 'function eval() { [native code] }');\
             out.join(',')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("true,true,true,true".into()));
}

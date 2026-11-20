//! FINGERPRINT-JS-REGISTRY: единый реестр нативных функций реалма —
//! хуки `_lumen_native_register`/`_lumen_native_sweep`, акцессорный текст
//! `get x`/`set x`, неперечислимость и работа после BUG-378 seal.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

/// Функция, созданная ПОСЛЕ обхода и помеченная хуком, печатается как
/// нативная — путь кадровых фасадов моста и ленивых шимов.
#[test]
fn late_function_registered_through_hook_prints_native() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "function __late_probe__() { return 42; }\
             _lumen_native_register(__late_probe__, 'function');\
             __late_probe__.toString()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("function __late_probe__() { [native code] }".into())
    );
}

/// Хук принимает kind 'getter'/'setter' и печатает акцессорный формат Chrome.
#[test]
fn hook_marks_getter_and_setter_kinds() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var g = function() {}; var s = function() {};\
             _lumen_native_register(g, 'getter', 'foo');\
             _lumen_native_register(s, 'setter', 'foo');\
             g.toString() + '|' + s.toString()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "function get foo() { [native code] }|function set foo() { [native code] }".into()
        )
    );
}

/// Реальный платформенный акцессор с парой get/set печатается в формате
/// Chrome: `function get x()` / `function set x()` — ключ свойства, а не
/// `fn.name` (регрессия BUG-1826 среза 5: печатало `function get()`).
#[test]
fn platform_accessor_pair_prints_get_and_set_forms() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var found = '';\
             var roots = [globalThis, document, navigator, location, history, screen, performance];\
             outer: for (var i = 0; i < roots.length; i++) {\
               var o = roots[i]; if (!o) continue;\
               var names = Object.getOwnPropertyNames(o);\
               for (var j = 0; j < names.length; j++) {\
                 var d = Object.getOwnPropertyDescriptor(o, names[j]);\
                 if (d && typeof d.get === 'function' && typeof d.set === 'function') {\
                   found = d.get.toString() + '|' + d.set.toString();\
                   break outer;\
                 }\
               }\
             }\
             found",
        )
        .unwrap();
    let s = match r {
        lumen_core::JsValue::String(s) => s,
        other => panic!("unexpected: {other:?}"),
    };
    assert!(
        s.starts_with("function get ") && s.contains("() { [native code] }")
            && s.contains("|function set ") && s.contains("() { [native code] }"),
        "accessor pair did not print Chrome form: {s}"
    );
}

/// Хуки реестра живут на внутреннем контейнере `__lumen_C` (BUG-753): для
/// страницы их нет на globalThis вообще — ни own-property, ни в `keys`.
/// Внутренние eval разрешают их через `with (container)` (проверено тестами
/// выше), фасады моста зовут их из поздних замыканий после BUG-378 seal.
#[test]
fn registry_hooks_stay_off_the_page_global() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var leak = Object.getOwnPropertyNames(globalThis).filter(function(k) {\
               return k === '_lumen_native_register' || k === '_lumen_native_sweep';\
             }).length + Object.keys(globalThis).filter(function(k) {\
               return k === '_lumen_native_register' || k === '_lumen_native_sweep';\
             }).length;\
             typeof globalThis._lumen_native_register === 'undefined' &&\
             typeof globalThis._lumen_native_sweep === 'undefined' && leak === 0",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

/// `Function.prototype.toString` сам печатается нативным, а страничная
/// функция, созданная после обхода и НЕ помеченная, сохраняет исходник.
#[test]
fn to_string_itself_is_native_and_unmarked_page_fn_keeps_source() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt
        .eval(
            "var ts = Function.prototype.toString.toString();\
             function __plain__() { return 'src'; }\
             var ps = __plain__.toString();\
             ts.includes('[native code]') && ps.includes(\"return 'src'\") && !ps.includes('[native code]')",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

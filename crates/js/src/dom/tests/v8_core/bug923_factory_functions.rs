//! BUG-923: `Audio`/`Image`/`Option` — WebIDL «legacy factory functions»,
//! и прототип элемента как `Interface.prototype`.

use super::*;

/// Строка результата; исключение — тоже строка, чтобы `TypeError` и `null`
/// не сливались в одном ассерте.
fn s(rt: &crate::v8_runtime::V8JsRuntime, expr: &str) -> String {
    let wrapped = format!(
        "String((function(){{ try {{ return {expr}; }} \
         catch (e) {{ return 'THROW:' + e.name; }} }})())"
    );
    match rt.eval(&wrapped) {
        Ok(lumen_core::JsValue::String(v)) => v,
        other => format!("{other:?}"),
    }
}

const FACTORIES: [(&str, &str); 3] = [
    ("Audio", "HTMLAudioElement"),
    ("Image", "HTMLImageElement"),
    ("Option", "HTMLOptionElement"),
];

/// Вызов без `new` обязан бросать `TypeError`, а не молча отдавать элемент.
#[test]
fn factory_call_without_new_throws() {
    let rt = v8_runtime_with_dom(make_doc());
    for (name, _) in FACTORIES {
        assert_eq!(s(&rt, &format!("{name}()")), "THROW:TypeError", "{name}()");
        assert_eq!(s(&rt, &format!("typeof new {name}()")), "object", "new {name}()");
    }
}

/// `.name`, `.length`, `.prototype` и дескриптор глобала.
#[test]
fn factory_identity_matches_webidl() {
    let rt = v8_runtime_with_dom(make_doc());
    for (name, iface) in FACTORIES {
        assert_eq!(s(&rt, &format!("{name}.name")), name);
        assert_eq!(s(&rt, &format!("{name}.length")), "0", "{name}.length");
        assert_eq!(s(&rt, &format!("{name}.prototype === {iface}.prototype")), "true");
        assert_eq!(
            s(&rt, &format!(
                "[Object.getOwnPropertyDescriptor({name}, 'prototype')].map(function(d) \
                 {{ return d.writable + ',' + d.enumerable + ',' + d.configurable; }})[0]"
            )),
            "false,false,false",
            "{name}.prototype descriptor"
        );
        assert_eq!(
            s(&rt, &format!(
                "[Object.getOwnPropertyDescriptor(globalThis, '{name}')].map(function(g) \
                 {{ return g.writable + ',' + g.enumerable + ',' + g.configurable; }})[0]"
            )),
            "true,false,true",
            "globalThis.{name} descriptor"
        );
        assert_eq!(s(&rt, &format!("new {name}() instanceof {iface}")), "true");
        assert_eq!(
            s(&rt, &format!("Object.getPrototypeOf(new {name}()) === {iface}.prototype")),
            "true"
        );
    }
}

/// Прототип элемента — сам `Interface.prototype` для любого тега (то, что
/// BUG-1122 уже починил; фиксируем, чтобы не вернулось).
#[test]
fn element_prototype_is_interface_prototype() {
    let rt = v8_runtime_with_dom(make_doc());
    for (tag, iface) in [
        ("div", "HTMLDivElement"),
        ("img", "HTMLImageElement"),
        ("audio", "HTMLAudioElement"),
    ] {
        assert_eq!(
            s(&rt, &format!(
                "Object.getPrototypeOf(document.createElement('{tag}')) === {iface}.prototype"
            )),
            "true",
            "<{tag}>"
        );
    }
}

/// Аргументы фабрик по-прежнему применяются; `new Audio()` выставляет
/// контент-атрибут `preload="auto"` (HTML LS §4.8.11).
#[test]
fn factory_arguments_are_applied() {
    let rt = v8_runtime_with_dom(make_doc());
    assert_eq!(s(&rt, "new Audio().getAttribute('preload')"), "auto");
    assert_eq!(s(&rt, "new Audio('x.mp3').getAttribute('src')"), "x.mp3");
    assert_eq!(s(&rt, "[new Image(3, 4)].map(function(i) { return i.width + ',' + i.height; })[0]"), "3,4");
    assert_eq!(
        s(&rt, "[new Option('t', 'v', true, true)].map(function(o) \
                { return [o.text, o.value, o.defaultSelected, o.selected].join(); })[0]"),
        "t,v,true,true"
    );
}

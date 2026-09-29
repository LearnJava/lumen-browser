//! BUG-1198: фрейм с непрозрачным происхождением (`sandbox` без
//! `allow-same-origin`) в кросс-фреймовых сообщениях, `window.top` фрейма в
//! полном шиме и `navigation.reload()`.
//!
//! Топология — прод-регистрации shell'а (`frame_ancestry.rs`): у родителя
//! биндинг документа ребёнка, у ребёнка слот родителя; оба рантайма с полным
//! шимом (`install_dom`), а не минимальные изоляты — `top` в полном шиме
//! unforgeable (BUG-587), и именно там фрейм терял своего верха.

use super::*;

const PARENT_URL: &str = "https://parent.example/index.html";

/// Рантайм ребёнка на `doc`, зарегистрированный под хостом `1` родителя как
/// opaque-sandbox фрейм (недоступен, `opaque = true`).
fn opaque_child(
    parent: &V8JsRuntime,
    parent_doc: &Arc<Mutex<lumen_dom::Document>>,
    doc: &Arc<Mutex<lumen_dom::Document>>,
) -> V8JsRuntime {
    let child = runtime_with_dom(Arc::clone(doc), "about:srcdoc");
    parent.register_frame_document(1, Arc::clone(doc), "about:srcdoc".to_owned(), None, false, true, None);
    child.register_parent_document(1, Arc::clone(parent_doc), PARENT_URL.to_owned(), None, false, None);
    child
}

fn eval_true(rt: &V8JsRuntime, js: &str) -> bool {
    matches!(rt.eval(js).unwrap(), JsValue::Bool(true))
}

/// Сообщения opaque-фрейма приходят с `origin === "null"`, `Origin.from(e)`
/// непрозрачен и один на документ — в том числе после повторной регистрации
/// того же документа (shell регистрирует его до и после скриптов); новый
/// документ хоста (перезагрузка) — другое непрозрачное происхождение.
#[test]
fn opaque_frame_messages_have_null_origin_and_per_document_identity() {
    let parent_doc = make_doc();
    let parent = runtime_with_dom(Arc::clone(&parent_doc), PARENT_URL);
    parent
        .eval(
            "globalThis.__r = []; globalThis.__o = []; \
             window.addEventListener('message', function (e) { \
                 var o = Origin.from(e); \
                 __o.push(o); \
                 __r.push(e.data + ':' + e.origin + ':' + o.opaque + ':' + (e.source === window[0])); \
             });",
        )
        .unwrap();
    let child_doc = make_doc();
    let child = opaque_child(&parent, &parent_doc, &child_doc);
    assert!(eval_true(&child, "window.top !== window && window.top === window.parent"), "top фрейма — фасад родителя");
    child.eval("window.top.postMessage('a', '*'); window.top.postMessage('b', '*');").unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        eval_true(&parent, "__r.join(',') === 'a:null:true:true,b:null:true:true'"),
        "получено: {:?}",
        parent.eval("__r.join(',')").unwrap()
    );
    assert!(eval_true(&parent, "__o[0].isSameOrigin(__o[1])"), "один документ — одно происхождение");

    // Повторная регистрация того же документа (с peer, после скриптов).
    parent.register_frame_document(1, Arc::clone(&child_doc), "about:srcdoc".to_owned(), None, false, true, None);
    child.eval("window.top.postMessage('c', '*');").unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(eval_true(&parent, "__o.length === 3 && __o[0].isSameOrigin(__o[2])"));

    // Перезагрузка: новый документ того же хоста.
    let reloaded_doc = make_doc();
    let reloaded = opaque_child(&parent, &parent_doc, &reloaded_doc);
    reloaded.eval("window.top.postMessage('d', '*');").unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        eval_true(&parent, "__o.length === 4 && __o[3].opaque && !__o[0].isSameOrigin(__o[3])"),
        "перезагруженный документ — новое непрозрачное происхождение"
    );
}

/// Непрозрачное происхождение адресата не совпадает ни с каким
/// сериализованным: `targetOrigin` `'/'` и явный origin (даже родительский,
/// который `about:srcdoc` унаследовал бы без песочницы) не доставляют,
/// `'*'` — доставляет.
#[test]
fn post_message_to_opaque_frame_needs_wildcard_target_origin() {
    let parent_doc = make_doc();
    let parent = runtime_with_dom(Arc::clone(&parent_doc), PARENT_URL);
    let child_doc = make_doc();
    let child = opaque_child(&parent, &parent_doc, &child_doc);
    child
        .eval("globalThis.__got = []; window.addEventListener('message', function (e) { __got.push(e.data); });")
        .unwrap();
    parent
        .eval(
            "window[0].postMessage('slash', '/'); \
             window[0].postMessage('explicit', 'https://parent.example'); \
             window[0].postMessage('star', '*');",
        )
        .unwrap();
    child.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(
        eval_true(&child, "__got.join(',') === 'star'"),
        "получено: {:?}",
        child.eval("__got.join(',')").unwrap()
    );
}

/// `navigation.reload()` ставит в очередь Navigation API действие
/// перезагрузки — shell применяет его к странице или (BUG-1198) к фрейму,
/// чей это рантайм.
#[test]
fn navigation_reload_queues_reload_action() {
    let rt = runtime_with_dom(make_doc(), PARENT_URL);
    assert!(eval_true(&rt, "typeof navigation.reload === 'function'"));
    rt.eval("navigation.reload()").unwrap();
    let updates = rt.take_nav_updates();
    assert!(
        updates.iter().any(|(action, ..)| matches!(action, crate::dom::NavAction::Reload)),
        "в очереди нет Reload"
    );
    // Несериализуемое состояние бросает до постановки в очередь.
    assert!(eval_true(
        &rt,
        "(function () { try { navigation.reload({ state: function () {} }); return false; } \
          catch (e) { return e.name === 'DataCloneError'; } })()"
    ));
    assert!(rt.take_nav_updates().is_empty());
}

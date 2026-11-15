//! BUG-1812: перенос `MessagePort` третьим аргументом `postMessage` через
//! границу кадров (`facade.postMessage(message, origin, transfer)`).
//!
//! Топология — та же, что у reCAPTCHA (и любой другой капчи на `MessageChannel`):
//! `api.js` верхней страницы отдаёт `port2` якорю в кросс-оригинном `<iframe>`,
//! якорь отвечает по нему. Оба рантайма с полным шимом (`install_dom`) — без
//! него нет ни `MessageChannel`, ни `_lumen_deliver_frame_message`.

use super::*;

const PARENT_URL: &str = "https://parent.example/index.html";
const CHILD_URL: &str = "https://child.example/frame.html";

fn eval_true(rt: &V8JsRuntime, js: &str) -> bool {
    matches!(rt.eval(js).unwrap(), JsValue::Bool(true))
}

/// Кросс-оригинная пара «страница ↔ кадр».
fn cross_origin_pair() -> (
    V8JsRuntime,
    V8JsRuntime,
    Arc<Mutex<lumen_dom::Document>>,
    Arc<Mutex<lumen_dom::Document>>,
) {
    let parent_doc = make_doc();
    let parent = runtime_with_dom(Arc::clone(&parent_doc), PARENT_URL);
    let child_doc = make_doc();
    let child = runtime_with_dom(Arc::clone(&child_doc), CHILD_URL);
    parent.register_frame_document(1, Arc::clone(&child_doc), CHILD_URL.to_owned(), None, false, false, None);
    child.register_parent_document(1, Arc::clone(&parent_doc), PARENT_URL.to_owned(), None, false, None);
    (parent, child, parent_doc, child_doc)
}

/// `event.ports` собран из списка передачи, sentinel внутри `data` разрешается
/// в тот же объект, а ответ по перенесённому порту доезжает до `port1`
/// отправителя — ровно handshake, без которого клиент капчи уходит в таймаут.
#[test]
fn transferred_message_port_echoes_across_the_frame_boundary() {
    let (parent, child, _parent_doc, _child_doc) = cross_origin_pair();
    parent
        .eval("globalThis.__got = null; window.onmessage = function (e) { globalThis.__got = e; };")
        .unwrap();
    child
        .eval(
            "globalThis.__ch = new MessageChannel(); \
             globalThis.__fromPort = null; \
             __ch.port1.onmessage = function (e) { globalThis.__fromPort = e.data; }; \
             window.parent.postMessage({ tag: 'with-port', port: __ch.port2 }, '*', [__ch.port2]);",
        )
        .unwrap();
    // Отправитель свой конец канала отсоединил (HTML LS §9.2.9).
    assert!(eval_true(&child, "__ch.port2._neutered === true"));
    // До пумпы получателя доставки нет.
    assert!(eval_true(&parent, "__got === null"));
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(eval_true(
        &parent,
        "__got !== null && __got.data.tag === 'with-port'"
    ));
    assert!(eval_true(&parent, "__got.ports.length === 1"));
    assert!(eval_true(&parent, "__got.data.port === __got.ports[0]"));
    parent.eval("__got.ports[0].postMessage('pong')").unwrap();
    child.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(eval_true(&child, "__fromPort === 'pong'"));
}

/// Сообщение без списка передачи: `event.ports` — пустой массив, не undefined.
#[test]
fn message_without_transfer_list_has_no_ports() {
    let (parent, child, _parent_doc, _child_doc) = cross_origin_pair();
    parent
        .eval("globalThis.__got = null; window.onmessage = function (e) { globalThis.__got = e; };")
        .unwrap();
    child.eval("window.parent.postMessage('plain', '*')").unwrap();
    parent.eval("_lumen_frame_pump_messages()").unwrap();
    assert!(eval_true(
        &parent,
        "__got !== null && Array.isArray(__got.ports) && __got.ports.length === 0"
    ));
}

/// Повторный перенос того же порта — `DataCloneError` (HTML LS §9.2.9).
#[test]
fn transferring_the_same_port_twice_throws_data_clone_error() {
    let (_parent, child, _parent_doc, _child_doc) = cross_origin_pair();
    assert!(eval_true(
        &child,
        "(function () { \
             var ch = new MessageChannel(); \
             window.parent.postMessage('a', '*', [ch.port2]); \
             try { window.parent.postMessage('b', '*', [ch.port2]); return false; } \
             catch (e) { return e.name === 'DataCloneError'; } \
         })()"
    ));
}

/// `window.postMessage(msg, origin, [port])` в СВОЁ окно: другой конец канала
/// остаётся в этом же изоляте, переданный порт отсоединяется, а событие несёт
/// рабочий новый объект, сцепленный с прежним партнёром.
#[test]
fn self_post_message_transfer_delivers_a_working_port() {
    let rt = runtime_with_dom(make_doc(), PARENT_URL);
    rt.eval(
        "globalThis.__got = null; \
         window.onmessage = function (e) { globalThis.__got = e; }; \
         globalThis.__ch = new MessageChannel(); \
         globalThis.__echo = null; \
         __ch.port1.onmessage = function (e) { globalThis.__echo = e.data; }; \
         window.postMessage('hi', '/', [__ch.port2]);",
    )
    .unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert!(eval_true(&rt, "__got !== null && __got.ports.length === 1"));
    assert!(eval_true(&rt, "__ch.port2._neutered === true"));
    rt.eval("__got.ports[0].postMessage('back')").unwrap();
    rt.eval("_lumen_tick_timers()").unwrap();
    assert!(eval_true(&rt, "__echo === 'back'"));
}

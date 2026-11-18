//! Тесты `v8_idb_events` — диспетчер событий IndexedDB на EventTarget
//! (IDB-EVENTS-S1; закрывает BUG-1706 и BUG-914).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

// BUG-1706: слушатель, добавленный на запрос внутри onupgradeneeded, не
// вызывается тем же событием — список слушателей клонируется перед обходом.
#[test]
fn idb_events_listener_added_during_dispatch_not_called() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var calls = 0;
                var req = indexedDB.open('ev1', 1);
                req.onupgradeneeded = function() {
                    req.addEventListener('upgradeneeded', function() { calls++; });
                };
                _lumen_idb_flush();
                calls
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(0.0));
}

// req/tx/db — EventTarget, события — настоящие Event с isTrusted = true.
#[test]
fn idb_events_eventtarget_chain() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                var req = indexedDB.open('ev2', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    out.push(req instanceof EventTarget, tx instanceof EventTarget,
                             db instanceof EventTarget, e instanceof Event,
                             e.isTrusted === true);
                };
                _lumen_idb_flush();
                out.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("true,true,true,true,true".into()));
}

// BUG-914: необработанная ошибка запроса доходит до tx.onerror и db.onerror
// ровно один раз, target === req, фазы 2 на запросе и 3 выше по цепочке.
#[test]
fn idb_events_error_bubbles_to_txn_and_db() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('ev3', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s', 'readwrite');
                    var st = tx.objectStore('s');
                    st.add({}, 1);
                    var req2 = st.add({}, 1);
                    tx.onerror = function(ev) {
                        log.push('tx:' + (ev.target === req2) + ':' + (ev.currentTarget === tx) + ':' + ev.eventPhase);
                    };
                    db.onerror = function(ev) {
                        log.push('db:' + (ev.target === req2) + ':' + (ev.currentTarget === db) + ':' + ev.eventPhase);
                    };
                    req2.onerror = function(ev) {
                        log.push('req:' + (ev.target === req2) + ':' + ev.eventPhase);
                    };
                };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("req:true:2,tx:true:true:3,db:true:true:3".into())
    );
}

// Capture-слушатель на db срабатывает раньше слушателя на самом запросе.
#[test]
fn idb_events_capture_on_db_before_request() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('ev4', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    var q = tx.objectStore('s').get(1);
                    db.addEventListener('success', function(ev) { log.push('db:' + ev.eventPhase); }, true);
                    q.addEventListener('success', function(ev) { log.push('req:' + ev.eventPhase); });
                };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("db:1,req:2".into()));
}

// stopPropagation() на запросе не пускает событие к транзакции (откат при этом
// остаётся — остановка всплытия не есть preventDefault).
#[test]
fn idb_events_stop_propagation_keeps_txn() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('ev5', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s', 'readwrite');
                    var st = tx.objectStore('s');
                    st.add({}, 1);
                    var req2 = st.add({}, 1);
                    req2.addEventListener('error', function(ev) { log.push('req'); ev.stopPropagation(); });
                    tx.addEventListener('error', function(ev) { log.push('tx'); });
                    db.addEventListener('error', function(ev) { log.push('db'); });
                    tx.onabort = function() { log.push('abort'); };
                };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("req,abort".into()));
}

// preventDefault() на error отменяет откат транзакции — приходит complete.
#[test]
fn idb_events_prevent_default_no_rollback() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('ev6', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s', 'readwrite');
                    var st = tx.objectStore('s');
                    st.add({}, 1);
                    var req2 = st.add({}, 1);
                    req2.addEventListener('error', function(ev) { ev.preventDefault(); });
                    tx.oncomplete = function() { log.push('complete'); };
                    tx.onabort = function() { log.push('abort'); };
                    db.onabort = function() { log.push('db-abort'); };
                };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("complete".into()));
}

// Без preventDefault приходит abort у транзакции и у db.onabort (BUG-914).
#[test]
fn idb_events_unhandled_error_aborts_at_db() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('ev7', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s', 'readwrite');
                    var st = tx.objectStore('s');
                    st.add({}, 1);
                    st.add({}, 1);
                    tx.onabort = function() { log.push('abort'); };
                    db.onabort = function() { log.push('db-abort'); };
                };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("abort,db-abort".into()));
}

// once срабатывает один раз; объект с handleEvent работает; dispatchEvent
// вызывает слушателя и возвращает true.
#[test]
fn idb_events_once_handle_event_dispatch() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                var req = indexedDB.open('ev8', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    var q = tx.objectStore('s').get(1);
                    q.addEventListener('success', function() { out.push('once'); }, { once: true });
                    q.addEventListener('success', { handleEvent: function() { out.push('handleEvent'); } });
                    q.addEventListener('x', function() { out.push('x'); });
                    out.push(q.dispatchEvent(new Event('x')));
                    q.addEventListener('y', function() { out.push('y'); }, { once: true });
                    q.dispatchEvent(new Event('y'));
                    q.dispatchEvent(new Event('y'));
                };
                _lumen_idb_flush();
                out.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("x,true,y,once,handleEvent".into()));
}

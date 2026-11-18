//! Тесты `v8_idb_listener_errors` — исключения в слушателях success/error/
//! upgradeneeded и close() внутри upgradeneeded (IDB-EVENTS-S3). Продакшн-путь
//! (шаговый `_idb_flush_step`) гоняется без явного `_lumen_idb_flush()`: шаги
//! продолжаются микрозадачами, которые V8 дренит после возврата из `eval`.

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

// (1) `onsuccess = () => { throw Error() }`: транзакция откатывается с
// AbortError — onabort вызван, oncomplete нет.
#[test]
fn idb_listener_error_success_handler_thrown_aborts() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('le1', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    tx.oncomplete = function() { log.push('complete'); };
                    tx.onabort = function() { log.push('abort:' + (tx.error ? tx.error.name : 'null')); };
                    var rq = tx.objectStore('s').get(0);
                    rq.onsuccess = function() { throw new Error('boom'); };
                };
            "#).unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("abort:AbortError".into())
    );
}

// (2а) То же через addEventListener('success').
#[test]
fn idb_listener_error_success_listener_thrown_aborts() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('le2', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    tx.oncomplete = function() { log.push('complete'); };
                    tx.onabort = function() { log.push('abort:' + (tx.error ? tx.error.name : 'null')); };
                    var rq = tx.objectStore('s').get(0);
                    rq.addEventListener('success', function() { throw new Error('boom'); });
                };
            "#).unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("abort:AbortError".into())
    );
}

// (2б) Исключение в слушателе error (и с preventDefault — откат всё равно);
// исключение на транзакции в фазе bubble тоже откатывает.
#[test]
fn idb_listener_error_error_listeners_abort_anyway() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('le3', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s', 'readwrite');
                    tx.oncomplete = function() { log.push('complete'); };
                    tx.onabort = function() { log.push('abort:' + (tx.error ? tx.error.name : 'null')); };
                    var st = tx.objectStore('s');
                    st.put(0, 0);
                    var rq = st.add(0, 0);
                    rq.onsuccess = function() { log.push('unreachable'); };
                    rq.onerror = function(ev) { ev.preventDefault(); throw new Error('boom'); };
                    tx.addEventListener('error', function() { log.push('tx-error-listener'); });
                };
            "#).unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("tx-error-listener,abort:AbortError".into())
    );
}

// (2в) Исключение в первом слушателе не останавливает диспетч: второй слушатель
// видит активную транзакцию, откат — после всего диспетча.
#[test]
fn idb_listener_error_dispatch_continues_then_aborts() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('le4', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    tx.onabort = function() {
                        log.push('abort:second=' + second + ':active=' + activeInSecond);
                    };
                    var rq = tx.objectStore('s').get(0);
                    var second = false, activeInSecond = false;
                    rq.addEventListener('success', function() { throw new Error('boom'); });
                    rq.addEventListener('success', function() {
                        second = true;
                        try { tx.objectStore('s').get(1); activeInSecond = true; }
                        catch (err) { activeInSecond = false; }
                    });
                };
            "#).unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("abort:second=true:active=true".into())
    );
}

// (3) Исключение в onupgradeneeded: upgrade-транзакция откатывается (abort с
// AbortError), open-запрос получает error с AbortError, success не вызван.
#[test]
fn idb_listener_error_upgradeneeded_thrown_fails_open() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('le5', 1);
                var upgradeTxn = null;
                open.onupgradeneeded = function(e) {
                    upgradeTxn = open.transaction;
                    upgradeTxn.onabort = function() {
                        log.push('upgrade-abort:' + (upgradeTxn.error ? upgradeTxn.error.name : 'null'));
                    };
                    e.target.result.createObjectStore('s');
                    throw new Error('boom');
                };
                open.onsuccess = function() { log.push('unreachable'); };
                open.onerror = function() { log.push('open-error:' + open.error.name); };
            "#).unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("upgrade-abort:AbortError,open-error:AbortError".into())
    );
}

// (4) db.close() внутри onupgradeneeded: versionchange-транзакция доходит до
// complete (стор сохраняется), но open-запрос получает error с AbortError.
#[test]
fn idb_listener_error_close_in_upgradeneeded_fails_open() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('le6', 1);
                var db = null;
                open.onupgradeneeded = function(e) {
                    db = e.target.result;
                    db.createObjectStore('s');
                    db.close();
                    open.transaction.oncomplete = function() { log.push('upgrade-complete'); };
                };
                open.onsuccess = function() { log.push('unreachable'); };
                open.onerror = function() {
                    log.push('open-error:' + open.error.name + ':stores=' + db.objectStoreNames.length +
                             ':txn=' + (function() { try { db.transaction('s'); return 'accepted'; } catch (err) { return err.name; } })());
                };
            "#).unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("upgrade-complete,open-error:AbortError:stores=1:txn=InvalidStateError".into())
    );
}

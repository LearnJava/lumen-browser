//! Тесты `v8_idb_active` — флаг активности транзакции и microtask checkpoint
//! (IDB-EVENTS-S2). Продакшн-путь (шаговый `_idb_flush_step`) гоняется без
//! явного `_lumen_idb_flush()`: шаги продолжаются микрозадачами, которые V8
//! дренит после возврата из `eval`; задачи — явным `_lumen_tick_timers()`.

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

// (1) Сразу после db.transaction('s') запрос проходит; после тика задач
// (деактивация — задача в голове _lumen_timers) новый запрос отклоняется с
// TransactionInactiveError.
#[test]
fn idb_active_accepted_at_creation_rejected_in_next_task() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('act1', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    window.st = tx.objectStore('s');
                    try { st.get(0); log.push('accepted'); } catch (err) { log.push(err.name); }
                };
            "#).unwrap();
    // Микрозадачи: шаги flush — open, upgradeneeded, success, обработчик.
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("accepted".into())
    );
    // Задачи: деактивация транзакции.
    rt.eval("_lumen_tick_timers(); 0").unwrap();
    let r = rt.eval(r#"
                var out = [];
                try { st.get(0); out.push('accepted'); } catch (err) { out.push(err.name); }
                out.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("TransactionInactiveError".into()));
}

// (2) Запрос из микрозадачи обработчика (Promise.resolve().then) принимается и
// получает своё событие success.
#[test]
fn idb_active_request_from_promise_reaction_accepted() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('act2', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    var st = tx.objectStore('s');
                    st.get(1);
                    Promise.resolve().then(function() {
                        try {
                            var q = st.get(2);
                            q.onsuccess = function() { log.push('success:' + q.result); };
                            log.push('accepted');
                        } catch (err) { log.push(err.name); }
                    });
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("accepted,success:undefined".into())
    );
}

// (3) Запрос из задачи (setTimeout 0) после тика отклоняется: деактивация,
// вставленная в голову _lumen_timers, исполняется раньше таймера обработчика.
#[test]
fn idb_active_request_from_task_rejected() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('act3', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s');
                    var st = tx.objectStore('s');
                    st.get(1);
                    setTimeout(function() {
                        try { st.get(2); log.push('accepted'); } catch (err) { log.push(err.name); }
                    }, 0);
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    rt.eval("_lumen_tick_timers(); 0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("TransactionInactiveError".into())
    );
}

// (4) async-обработчик с await между двумя запросами к одной транзакции
// доходит до complete: await — микрозадача, транзакция остаётся активной.
#[test]
fn idb_active_async_handler_two_requests_reaches_complete() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('act4', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s', 'readwrite');
                    var st = tx.objectStore('s');
                    st.put('a', 1);
                    var flag = '';
                    (async function() {
                        await Promise.resolve();
                        try { st.put('b', 2); flag += 'second-accepted'; } catch (err) { flag += err.name; }
                    })();
                    tx.oncomplete = function() { log.push(flag + ':complete'); };
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("second-accepted:complete".into())
    );
}

// Пустая транзакция, созданная в обработчике, коммитится после транзакции-хозяина
// (transaction-lifetime-empty): очередь tx1 опустела в обработчике rq1, но tx1
// остаётся в голове очереди активных транзакций до своего settlement-шага.
#[test]
fn idb_active_empty_txn_commits_after_owner() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('act5', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx1 = db.transaction('s', 'readwrite');
                    tx1.oncomplete = function() { log.push('tx1.complete'); };
                    var st = tx1.objectStore('s');
                    var rq1 = st.put('a', 1);
                    rq1.onsuccess = function() {
                        log.push('rq1.onsuccess');
                        var tx2 = db.transaction('s');
                        tx2.oncomplete = function() { log.push('tx2.complete'); };
                        var rq2 = st.put('b', 2);
                        rq2.onsuccess = function() { log.push('rq2.onsuccess'); };
                    };
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("rq1.onsuccess,rq2.onsuccess,tx1.complete,tx2.complete".into())
    );
}

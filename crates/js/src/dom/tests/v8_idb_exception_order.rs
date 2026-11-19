//! Тесты `v8_idb_exception_order` — порядок проверок исключений курсора и
//! индекса: advance(0) раньше неактивности, readonly раньше «значение
//! получено», «удалён» раньше «неактивна» у запросов индекса (IDB-EVENTS-S5).

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

// (1) advance(0) на неактивной транзакции — TypeError: проверка «ноль» идёт
// раньше «транзакция не активна» (TransactionInactiveError).
#[test]
fn idb_exc_order_advance_zero_before_inactive() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('eo-advance', 1);
                open.onupgradeneeded = function(e) {
                    var st = e.target.result.createObjectStore('s');
                    st.put('v', 'k');
                };
                open.onsuccess = function(e) {
                    var tx = e.target.result.transaction('s', 'readonly');
                    var rq = tx.objectStore('s').openKeyCursor();
                    rq.onsuccess = function() {
                        var cursor = rq.result;
                        tx.abort();
                        try { cursor.advance(0); log.push('no-throw'); }
                        catch (err) { log.push(err.name); }
                    };
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("TypeError".into())
    );
}

// (2) update на readonly-транзакции после continue() — ReadOnlyError:
// «только чтение» раньше «значение получено» (InvalidStateError).
#[test]
fn idb_exc_order_update_readonly_before_got_value() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('eo-update', 1);
                open.onupgradeneeded = function(e) {
                    var st = e.target.result.createObjectStore('s');
                    st.put('v', 'k');
                };
                open.onsuccess = function(e) {
                    var tx = e.target.result.transaction('s', 'readonly');
                    var rq = tx.objectStore('s').openCursor();
                    rq.onsuccess = function() {
                        var cursor = rq.result;
                        cursor.continue();
                        try { cursor.update('v2'); log.push('no-throw'); }
                        catch (err) { log.push(err.name); }
                    };
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("ReadOnlyError".into())
    );
}

// (3) Запрос по индексу после deleteIndex на неактивной транзакции —
// InvalidStateError («удалён»), а не TransactionInactiveError: проверка
// «удалён» идёт раньше «транзакция не активна».
#[test]
fn idb_exc_order_index_query_deleted_before_inactive() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('eo-index-del', 1);
                open.onupgradeneeded = function(e) {
                    var st = e.target.result.createObjectStore('s');
                    st.put('v', 'k');
                    st.createIndex('i', 'v');
                    var idx = st.index('i');
                    st.deleteIndex('i');
                    st.transaction.abort();
                    try { idx.get('k'); log.push('no-throw'); }
                    catch (err) { log.push(err.name); }
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("InvalidStateError".into())
    );
}

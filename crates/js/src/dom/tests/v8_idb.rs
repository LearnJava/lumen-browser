//! Тесты `v8_idb`, вынесенные из `dom.rs` (дорожка SPLIT, батч JS-1).

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

// ── IndexedDB ───────────────────────────────────────────────────────────

#[test]
fn idb_global_exists() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(
        "typeof indexedDB === 'object' && typeof indexedDB.open === 'function' \
                 && typeof IDBKeyRange === 'function' && typeof window.indexedDB === 'object'",
    ).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn idb_open_fires_upgrade_then_success() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('db1', 3);
                req.onupgradeneeded = function(e) { log.push('upg:' + e.oldVersion + '->' + e.newVersion); };
                req.onsuccess = function(e) { log.push('ok:' + e.target.result.version); };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("upg:0->3,ok:3".into()));
}

#[test]
fn idb_add_and_get_keypath() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var tx = db.transaction('s', 'readwrite');
                    var st = tx.objectStore('s');
                    st.add({ id: 1, name: 'alpha' });
                    st.add({ id: 2, name: 'beta' });
                    var g = st.get(2);
                    g.onsuccess = function() { out = g.result.name; };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("beta".into()));
}

#[test]
fn idb_autoincrement_out_of_line() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var keys = [];
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { autoIncrement: true }); };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    var a = st.add('x'); a.onsuccess = function() { keys.push(a.result); };
                    var b = st.add('y'); b.onsuccess = function() { keys.push(b.result); };
                };
                _lumen_idb_flush();
                keys.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1,2".into()));
}

#[test]
fn idb_put_overwrites() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    st.add({ id: 1, v: 'old' });
                    st.put({ id: 1, v: 'new' });
                    var g = st.get(1);
                    var c = st.count();
                    c.onsuccess = function() { out = g.result.v + ':' + c.result; };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("new:1".into()));
}

#[test]
fn idb_add_duplicate_aborts_transaction() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                req.onsuccess = function(e) {
                    var tx = e.target.result.transaction('s', 'readwrite');
                    tx.onabort = function() { log.push('abort'); };
                    var st = tx.objectStore('s');
                    st.add({ id: 1 });
                    var dup = st.add({ id: 1 });
                    dup.onerror = function(ev) { log.push('err:' + ev.target.error.name); };
                };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("err:ConstraintError,abort".into()));
}

#[test]
fn idb_empty_transaction_commits() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var db;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                var tx = db.transaction('s', 'readonly');
                tx.oncomplete = function() { log.push('complete'); };
                tx.onabort = function() { log.push('abort'); };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("complete".into()));
}

#[test]
fn idb_empty_transactions_commit_in_creation_order() {
    // transaction-lifetime-empty.any.js: two empty transactions created
    // inside a request handler commit AFTER the busy transaction that
    // preceded them, in the order they were created.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var order = [];
                var db;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                var tx1 = db.transaction('s', 'readwrite');
                tx1.oncomplete = function() { order.push('tx1'); };
                var st = tx1.objectStore('s');
                var rq1 = st.put('a', 1);
                rq1.onsuccess = function() {
                    order.push('rq1');
                    var tx2 = db.transaction('s', 'readonly');
                    tx2.oncomplete = function() { order.push('tx2'); };
                    var tx3 = db.transaction('s', 'readonly');
                    tx3.oncomplete = function() { order.push('tx3'); };
                    var rq2 = st.put('b', 2);
                    rq2.onsuccess = function() { order.push('rq2'); };
                };
                _lumen_idb_flush();
                order.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("rq1,rq2,tx1,tx2,tx3".into()));
}

#[test]
fn idb_abort_finishes_transaction_synchronously() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                function probe(name, fn) {
                    try { fn(); out.push(name + ':none'); }
                    catch (e) { out.push(name + ':' + e.name); }
                }
                var db;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                var tx = db.transaction('s', 'readwrite');
                var store = tx.objectStore('s');
                tx.abort();
                probe('objectStore', function() { tx.objectStore('s'); });
                probe('request', function() { store.get(1); });
                probe('abort-again', function() { tx.abort(); });
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "objectStore:InvalidStateError,request:TransactionInactiveError,\
                     abort-again:InvalidStateError"
                .into()
        )
    );
}

#[test]
fn idb_abort_settles_queued_requests_with_abort_error() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var db;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                var tx = db.transaction('s', 'readwrite');
                var w = tx.objectStore('s').put('x', 1);
                w.onsuccess = function() { log.push('success'); };
                w.onerror = function(ev) { log.push('error:' + ev.target.error.name); };
                tx.onabort = function() { log.push('txn-abort'); };
                tx.abort();
                _lumen_idb_flush();
                log.join(',') + '|' + w.readyState
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("error:AbortError,txn-abort|done".into())
    );
}

#[test]
fn idb_transaction_commit_refuses_further_requests() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var db;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                var tx = db.transaction('s', 'readwrite');
                var store = tx.objectStore('s');
                var w = store.put('x', 1);
                w.onsuccess = function() { log.push('written'); };
                tx.oncomplete = function() { log.push('complete'); };
                tx.commit();
                try { store.get(1); log.push('accepted'); }
                catch (e) { log.push('refused:' + e.name); }
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "refused:TransactionInactiveError,written,complete".into()
        )
    );
}

#[test]
fn idb_transaction_rejects_invalid_mode() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                function probe(name, fn) {
                    try { fn(); out.push(name + ':none'); }
                    catch (e) { out.push(name + ':' + (e.name || 'TypeError')); }
                }
                var db;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                probe('bogus', function() { db.transaction('s', 'bogus'); });
                probe('versionchange', function() { db.transaction('s', 'versionchange'); });
                // NotFoundError precedes the versionchange TypeError (§3.3.4).
                probe('missing-store', function() { db.transaction('nope', 'versionchange'); });
                probe('durability', function() { db.transaction('s', 'readonly', { durability: 'bogus' }); });
                out.push('relaxed:' + db.transaction('s', 'readonly', { durability: 'relaxed' }).durability);
                out.push('default:' + db.transaction('s', 'readonly').durability);
                _lumen_idb_flush();
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "bogus:TypeError,versionchange:TypeError,missing-store:NotFoundError,\
                     durability:TypeError,relaxed:relaxed,default:default"
                .into()
        )
    );
}

#[test]
fn idb_abort_reverts_applied_writes() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                var db;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                // seed a record the aborting transaction will overwrite and delete
                var seed = db.transaction('s', 'readwrite').objectStore('s');
                seed.put('kept', 1);
                seed.put('deleted-later', 2);
                _lumen_idb_flush();

                var tx = db.transaction('s', 'readwrite');
                var st = tx.objectStore('s');
                var w = st.put('new', 3);
                st.put('overwritten', 1);
                st.delete(2);
                // abort only once the writes have actually been applied
                w.onsuccess = function() { tx.abort(); };
                _lumen_idb_flush();

                var check = db.transaction('s', 'readonly').objectStore('s');
                var g1 = check.get(1), g2 = check.get(2), g3 = check.get(3);
                _lumen_idb_flush();
                out.push('1=' + g1.result);
                out.push('2=' + g2.result);
                out.push('3=' + g3.result);
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("1=kept,2=deleted-later,3=undefined".into())
    );
}

#[test]
fn idb_abort_reverts_key_generator() {
    // transaction-abort-generator-revert.any.js: the key generator goes
    // back to where it was, so the next add reuses the aborted key.
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var db, keys = [];
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { autoIncrement: true }); };
                req.onsuccess = function(e) { db = e.target.result; };
                _lumen_idb_flush();
                var tx1 = db.transaction('s', 'readwrite');
                var a = tx1.objectStore('s').add('x');
                a.onsuccess = function() { keys.push(a.result); tx1.abort(); };
                _lumen_idb_flush();
                var tx2 = db.transaction('s', 'readwrite');
                var b = tx2.objectStore('s').add('y');
                b.onsuccess = function() { keys.push(b.result); };
                _lumen_idb_flush();
                keys.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1,1".into()));
}

#[test]
fn idb_abort_reverts_upgrade_schema_and_version() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                var db;
                var first = indexedDB.open('d', 1);
                first.onupgradeneeded = function(e) { e.target.result.createObjectStore('keep'); };
                first.onsuccess = function(e) { db = e.target.result; db.close(); };
                _lumen_idb_flush();

                var second = indexedDB.open('d', 2);
                second.onupgradeneeded = function(e) {
                    var d = e.target.result;
                    d.createObjectStore('added');
                    d.deleteObjectStore('keep');
                    e.target.transaction.abort();
                };
                second.onerror = function() { out.push('open-error'); };
                _lumen_idb_flush();

                var third = indexedDB.open('d');
                third.onsuccess = function(e) {
                    var d = e.target.result;
                    out.push('version=' + d.version);
                    out.push('stores=' + d.objectStoreNames.join('/'));
                };
                _lumen_idb_flush();
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("open-error,version=1,stores=keep".into())
    );
}

#[test]
fn idb_getall_sorted_by_key() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    st.add('c', 3); st.add('a', 1); st.add('b', 2);
                    var g = st.getAll(); g.onsuccess = function() { out = g.result.join(''); };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("abc".into()));
}

#[test]
fn idb_getall_with_key_range() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    for (var i = 1; i <= 5; i++) st.add('v' + i, i);
                    var g = st.getAll(IDBKeyRange.bound(2, 4, false, true));
                    g.onsuccess = function() { out = g.result.join(','); };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("v2,v3".into()));
}

#[test]
fn idb_delete_and_clear() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    st.add('a', 1); st.add('b', 2); st.add('c', 3);
                    st.delete(2);
                    var c1 = st.count(); c1.onsuccess = function() {
                        st.clear();
                        var c2 = st.count(); c2.onsuccess = function() { out = c1.result + ':' + c2.result; };
                    };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("2:0".into()));
}

#[test]
fn idb_index_get_and_getall() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) {
                    var st = e.target.result.createObjectStore('s', { keyPath: 'id' });
                    st.createIndex('by_cat', 'cat');
                };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    st.add({ id: 1, cat: 'x', n: 'one' });
                    st.add({ id: 2, cat: 'y', n: 'two' });
                    st.add({ id: 3, cat: 'x', n: 'three' });
                    var idx = st.index('by_cat');
                    var g = idx.get('y');
                    var ga = idx.getAll('x');
                    ga.onsuccess = function() {
                        out = g.result.n + '|' + ga.result.map(function(r){return r.n;}).join(',');
                    };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("two|one,three".into()));
}

#[test]
fn idb_unique_index_violation() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) {
                    var st = e.target.result.createObjectStore('s', { keyPath: 'id' });
                    st.createIndex('email', 'email', { unique: true });
                };
                req.onsuccess = function(e) {
                    var tx = e.target.result.transaction('s', 'readwrite');
                    tx.onabort = function() { log.push('abort'); };
                    var st = tx.objectStore('s');
                    st.add({ id: 1, email: 'a@b.c' });
                    var dup = st.add({ id: 2, email: 'a@b.c' });
                    dup.onerror = function(ev) { log.push(ev.target.error.name); };
                };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("ConstraintError,abort".into()));
}

#[test]
fn idb_cursor_iterates_in_order() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var keys = [];
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    st.add('a', 3); st.add('b', 1); st.add('c', 2);
                    var cur = st.openCursor();
                    cur.onsuccess = function(ev) {
                        var c = ev.target.result;
                        if (c) { keys.push(c.key + '=' + c.value); c.continue(); }
                    };
                };
                _lumen_idb_flush();
                keys.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1=b,2=c,3=a".into()));
}

#[test]
fn idb_cursor_reverse_direction() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var keys = [];
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                req.onsuccess = function(e) {
                    var st = e.target.result.transaction('s', 'readwrite').objectStore('s');
                    for (var i = 1; i <= 3; i++) st.add('v', i);
                    var cur = st.openKeyCursor(null, 'prev');
                    cur.onsuccess = function(ev) {
                        var c = ev.target.result;
                        if (c) { keys.push(c.key); c.continue(); }
                    };
                };
                _lumen_idb_flush();
                keys.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("3,2,1".into()));
}

#[test]
fn idb_cursor_update_and_delete() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                req.onsuccess = function(e) {
                    var db = e.target.result;
                    var st = db.transaction('s', 'readwrite').objectStore('s');
                    st.add({ id: 1, v: 10 }); st.add({ id: 2, v: 20 }); st.add({ id: 3, v: 30 });
                    var cur = st.openCursor();
                    cur.onsuccess = function(ev) {
                        var c = ev.target.result;
                        if (!c) return;
                        if (c.primaryKey === 1) c.update({ id: 1, v: 99 });
                        else if (c.primaryKey === 2) c.delete();
                        c.continue();
                    };
                    var tx2 = db.transaction('s');
                    var g = tx2.objectStore('s').getAll();
                    g.onsuccess = function() {
                        out = g.result.map(function(r){return r.id + ':' + r.v;}).join(',');
                    };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("1:99,3:30".into()));
}

/// BUG-842: WPT's `keep_alive` idiom re-arms one request from its own
/// `onsuccess`. The drain used to run that inside a single microtask, so
/// the loop never returned and no timer, paint or later script of the
/// page ever ran again. A flush must now spend its budget, hand the rest
/// to the event loop, and leave the transaction alive meanwhile. The
/// `spins > 5000` brake exists only so that a regression fails on the
/// assertions (`alive` goes false, the transaction having run to
/// completion inside the single flush) instead of hanging the harness.
#[test]
fn idb_self_rearming_request_yields_between_turns() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var spins = 0, ticks = 0, done = '', stop = false;
                var open = indexedDB.open('d', 1);
                open.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                open.onsuccess = function(e) {
                    var tx = e.target.result.transaction('s', 'readwrite');
                    var st = tx.objectStore('s');
                    st.add('v', 1);
                    tx.oncomplete = function() { done = 'complete'; };
                    (function spin() {
                        if (stop || spins > 5000) return;
                        spins++;
                        st.get(1).onsuccess = spin;
                    })();
                    setTimeout(function() { ticks++; stop = true; }, 0);
                };
                _lumen_idb_flush();
                var spun = spins > 0, alive = done === '';
                _lumen_tick_timers();
                [spun, alive, ticks, done].join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("true,true,1,complete".into()));
}

#[test]
fn idb_keyrange_includes_and_cmp() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var kr = IDBKeyRange.bound(1, 5, true, false);
                var a = kr.includes(1) === false && kr.includes(5) === true && kr.includes(3) === true;
                var b = indexedDB.cmp(1, 2) === -1 && indexedDB.cmp('b', 'a') === 1 && indexedDB.cmp(7, 7) === 0;
                var c = indexedDB.cmp(5, 'x') === -1 && indexedDB.cmp([1,2], [1,3]) === -1;
                a && b && c
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Bool(true));
}

#[test]
fn idb_upgrade_sends_versionchange_and_waits_for_close() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [], db1 = null;
                var r1 = indexedDB.open('d', 1);
                r1.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                r1.onsuccess = function(e) { db1 = e.target.result; };
                _lumen_idb_flush();

                db1.addEventListener('versionchange', function(e) {
                    log.push('versionchange ' + e.oldVersion + '->' + e.newVersion);
                });
                var r2 = indexedDB.open('d', 2);
                r2.addEventListener('blocked', function(e) {
                    log.push('blocked ' + e.oldVersion + '->' + e.newVersion);
                });
                r2.onupgradeneeded = function() { log.push('upgradeneeded'); };
                r2.onsuccess = function() { log.push('success'); };
                _lumen_idb_flush();
                log.push('|parked, version still ' + _idb_databases['d'].version);

                db1.close();
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "versionchange 1->2,blocked 1->2,|parked, version still 1,upgradeneeded,success"
                .into()
        )
    );
}

#[test]
fn idb_upgrade_is_not_blocked_by_a_closed_connection() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [], db1 = null;
                var r1 = indexedDB.open('d', 1);
                r1.onsuccess = function(e) { db1 = e.target.result; };
                _lumen_idb_flush();
                db1.onversionchange = function() { log.push('versionchange'); };
                db1.close();
                var r2 = indexedDB.open('d', 2);
                r2.onblocked = function() { log.push('blocked'); };
                r2.onsuccess = function() { log.push('success v' + r2.result.version); };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("success v2".into()));
}

#[test]
fn idb_versionchange_handler_may_close_inline() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [], db1 = null;
                var r1 = indexedDB.open('d', 1);
                r1.onsuccess = function(e) { db1 = e.target.result; };
                _lumen_idb_flush();
                db1.onversionchange = function() { log.push('versionchange'); db1.close(); };
                var r2 = indexedDB.open('d', 2);
                r2.onblocked = function() { log.push('blocked'); };
                r2.onsuccess = function() { log.push('success'); };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    // Closing inside the handler unblocks the request in the same turn,
    // so `blocked` must not be fired at all (§3.3.1 re-checks after the
    // versionchange broadcast).
    assert_eq!(r, lumen_core::JsValue::String("versionchange,success".into()));
}

#[test]
fn idb_delete_database_blocks_on_an_open_connection() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [], db1 = null;
                var r1 = indexedDB.open('d', 3);
                r1.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                r1.onsuccess = function(e) { db1 = e.target.result; };
                _lumen_idb_flush();

                db1.onversionchange = function(e) { log.push('versionchange ' + e.oldVersion + '->' + e.newVersion); };
                var rd = indexedDB.deleteDatabase('d');
                rd.onblocked = function(e) { log.push('blocked ' + e.oldVersion + '->' + e.newVersion); };
                rd.onsuccess = function(e) { log.push('deleted ' + e.oldVersion + '->' + e.newVersion); };
                _lumen_idb_flush();
                log.push('|still there: ' + (_idb_databases['d'] ? 'yes' : 'no'));

                db1.close();
                _lumen_idb_flush();
                log.push('|still there: ' + (_idb_databases['d'] ? 'yes' : 'no'));
                log.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "versionchange 3->null,blocked 3->null,|still there: yes,deleted 3->null,|still there: no"
                .into()
        )
    );
}

#[test]
fn idb_open_behind_a_delete_sees_the_deleted_database() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var r1 = indexedDB.open('d', 5);
                r1.onsuccess = function() { r1.result.close(); };
                _lumen_idb_flush();
                // Queued in one turn: the open must resolve its version against
                // what the delete ahead of it leaves behind, not against what the
                // database looked like when open() was called.
                indexedDB.deleteDatabase('d');
                var r2 = indexedDB.open('d');
                r2.onupgradeneeded = function(e) { log.push('upgrade ' + e.oldVersion + '->' + e.newVersion); };
                r2.onsuccess = function() { log.push('v' + r2.result.version); };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("upgrade 0->1,v1".into()));
}

#[test]
fn idb_open_and_delete_requests_form_a_fifo_queue() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                // The scenario of WPT IndexedDB/open-request-queue.any.js, with
                // the deferred close driven explicitly instead of by setTimeout.
                var log = [], held = [], db = null;
                var r0 = indexedDB.open('q', 1);
                r0.onsuccess = function() { db = r0.result; };
                _lumen_idb_flush();

                function open(token, version) {
                    var r = indexedDB.open('q', version);
                    r.onsuccess = function() {
                        log.push(token + ' success');
                        var d = r.result;
                        d.onversionchange = function() { log.push(token + ' versionchange'); held.push(d); };
                    };
                    r.onblocked = function() { log.push(token + ' blocked'); };
                }
                function del(token) {
                    var r = indexedDB.deleteDatabase('q');
                    r.onsuccess = function() { log.push(token + ' success'); };
                    r.onblocked = function() { log.push(token + ' blocked'); };
                }
                open('open1', 2);
                del('delete1');
                open('open2', 3);
                del('delete2');
                db.close();

                for (var i = 0; i < 8; i++) {
                    _lumen_idb_flush();
                    while (held.length > 0) held.shift().close();
                }
                log.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "open1 success,open1 versionchange,delete1 blocked,delete1 success,\
                     open2 success,open2 versionchange,delete2 blocked,delete2 success"
                .into()
        )
    );
}

#[test]
fn idb_close_waits_for_a_running_transaction() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [], db1 = null;
                var r1 = indexedDB.open('d', 1);
                r1.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                r1.onsuccess = function(e) { db1 = e.target.result; };
                _lumen_idb_flush();

                var tx = db1.transaction('s', 'readwrite');
                tx.objectStore('s').put(1, 'k');
                tx.oncomplete = function() { log.push('txn complete'); };
                // close() is close-pending: the connection keeps blocking the
                // upgrade until its transaction has finished (§3.3.9).
                db1.close();
                var r2 = indexedDB.open('d', 2);
                r2.onupgradeneeded = function() { log.push('upgradeneeded'); };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("txn complete,upgradeneeded".into()));
}

#[test]
fn idb_open_version_downgrade_errors() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var r1 = indexedDB.open('d', 5);
                r1.onsuccess = function(e) { e.target.result.close(); log.push('v5'); };
                _lumen_idb_flush();
                var r2 = indexedDB.open('d', 2);
                r2.onerror = function(e) { log.push('err:' + e.target.error.name); };
                r2.onsuccess = function() { log.push('unexpected'); };
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("v5,err:VersionError".into()));
}

#[test]
fn idb_delete_database() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var log = [];
                var r1 = indexedDB.open('d', 1);
                r1.onsuccess = function(e) { e.target.result.close(); };
                _lumen_idb_flush();
                var del = indexedDB.deleteDatabase('d');
                del.onsuccess = function() { log.push('deleted'); };
                _lumen_idb_flush();
                indexedDB.databases().then(function(list) { log.push('count:' + list.length); });
                _lumen_idb_flush();
                log.join(',')
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("deleted".into()));
}

#[test]
fn idb_second_connection_sees_persisted_data() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out;
                var r1 = indexedDB.open('d', 1);
                r1.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                r1.onsuccess = function(e) {
                    var db = e.target.result;
                    db.transaction('s', 'readwrite').objectStore('s').add({ id: 1, v: 'kept' });
                    db.close();
                };
                _lumen_idb_flush();
                var r2 = indexedDB.open('d');
                r2.onsuccess = function(e) {
                    var g = e.target.result.transaction('s').objectStore('s').get(1);
                    g.onsuccess = function() { out = g.result.v; };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("kept".into()));
}

// ── IndexedDB persistence (Rust-backed snapshot survives reload) ──────────

/// In-memory `IdbBackend` capturing the snapshot the shim persists, shared
/// across runtimes via `Arc` to simulate the same origin across reloads.
struct MockIdb(Arc<Mutex<Option<String>>>);
impl IdbBackend for MockIdb {
    fn load(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn save(&self, snapshot: &str) {
        *self.0.lock().unwrap() = Some(snapshot.to_owned());
    }
}

fn v8_runtime_with_idb(backend: Arc<dyn IdbBackend>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.install_dom(make_doc(), "https://example.com/", None, None, None, None, Some(backend), None, None, None, false)
        .unwrap();
    rt
}

#[test]
fn idb_persists_across_runtime_reload() {
    let cell = Arc::new(Mutex::new(None));
    // First "page load": create a store and write a record.
    {
        let rt = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        rt.eval(r#"
                    var req = indexedDB.open('d', 1);
                    req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                    req.onsuccess = function(e) {
                        e.target.result.transaction('s', 'readwrite').objectStore('s').add({ id: 1, v: 'kept' });
                    };
                    _lumen_idb_flush();
                "#).unwrap();
    }
    // Backend captured a snapshot from the mutating transaction.
    assert!(cell.lock().unwrap().is_some(), "snapshot must be persisted");

    // Second "page load": a fresh runtime restores the database without re-running
    // the upgrade — the store and its record are already present.
    let rt2 = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
    let r = rt2.eval(r#"
                var out;
                var req = indexedDB.open('d');
                req.onupgradeneeded = function() { out = 'UNEXPECTED_UPGRADE'; };
                req.onsuccess = function(e) {
                    var g = e.target.result.transaction('s').objectStore('s').get(1);
                    g.onsuccess = function() { out = g.result ? g.result.v : 'MISSING'; };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("kept".into()));
}

#[test]
fn idb_persisted_version_is_restored() {
    let cell = Arc::new(Mutex::new(None));
    {
        let rt = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        rt.eval(r#"
                    var req = indexedDB.open('d', 4);
                    req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                    req.onsuccess = function() {};
                    _lumen_idb_flush();
                "#).unwrap();
    }
    let rt2 = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
    let r = rt2.eval(r#"
                var out;
                var req = indexedDB.open('d');
                req.onsuccess = function(e) { out = e.target.result.version; };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(4.0));
}

#[test]
fn idb_persisted_date_value_roundtrips() {
    let cell = Arc::new(Mutex::new(None));
    {
        let rt = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        rt.eval(r#"
                    var req = indexedDB.open('d', 1);
                    req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                    req.onsuccess = function(e) {
                        e.target.result.transaction('s', 'readwrite').objectStore('s')
                            .add({ id: 1, when: new Date(1700000000000) });
                    };
                    _lumen_idb_flush();
                "#).unwrap();
    }
    let rt2 = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
    let r = rt2.eval(r#"
                var out;
                var req = indexedDB.open('d');
                req.onsuccess = function(e) {
                    var g = e.target.result.transaction('s').objectStore('s').get(1);
                    g.onsuccess = function() {
                        out = (g.result.when instanceof Date) + ':' + g.result.when.getTime();
                    };
                };
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("true:1700000000000".into()));
}

#[test]
fn idb_persisted_delete_database_is_restored() {
    let cell = Arc::new(Mutex::new(None));
    {
        let rt = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
        rt.eval(r#"
                    var req = indexedDB.open('d', 1);
                    req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s'); };
                    // Closed before the delete: an open connection blocks it (§3.3.1).
                    req.onsuccess = function(e) { e.target.result.close(); };
                    _lumen_idb_flush();
                    indexedDB.deleteDatabase('d');
                    _lumen_idb_flush();
                "#).unwrap();
    }
    // After deletion the restored snapshot must not contain the database:
    // opening it fresh re-triggers upgradeneeded and the store is gone.
    let rt2 = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
    let r = rt2.eval(r#"
                var out = 'no-upgrade';
                var req = indexedDB.open('d');
                req.onupgradeneeded = function(e) {
                    out = 'upgrade:' + e.target.result.objectStoreNames.length;
                };
                req.onsuccess = function() {};
                _lumen_idb_flush();
                out
            "#).unwrap();
    assert_eq!(r, lumen_core::JsValue::String("upgrade:0".into()));
}

#[test]
fn idb_read_only_transaction_does_not_persist() {
    let cell = Arc::new(Mutex::new(None));
    let rt = v8_runtime_with_idb(Arc::new(MockIdb(Arc::clone(&cell))));
    // Create + populate (this persists once).
    rt.eval(r#"
                var req = indexedDB.open('d', 1);
                req.onupgradeneeded = function(e) { e.target.result.createObjectStore('s', { keyPath: 'id' }); };
                req.onsuccess = function(e) { e.target.result.transaction('s', 'readwrite').objectStore('s').add({ id: 1 }); };
                _lumen_idb_flush();
            "#).unwrap();
    // Overwrite the captured snapshot with a sentinel, then run a read-only txn.
    *cell.lock().unwrap() = Some("SENTINEL".into());
    rt.eval(r#"
                var req = indexedDB.open('d');
                req.onsuccess = function(e) { e.target.result.transaction('s').objectStore('s').get(1); };
                _lumen_idb_flush();
            "#).unwrap();
    // A read-only flush must not have re-persisted (sentinel intact).
    assert_eq!(cell.lock().unwrap().as_deref(), Some("SENTINEL"));
}

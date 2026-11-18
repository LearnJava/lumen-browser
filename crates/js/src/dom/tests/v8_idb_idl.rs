//! Тесты `v8_idb_idl` — форма IDL: интерфейсные объекты, IDBFactory,
//! IDBVersionChangeEvent, отдельный IDBCursorWithValue, атрибуты — аксессоры
//! прототипа (IDB-EVENTS-S4).

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

// (1) indexedDB — единственный экземпляр IDBFactory; конструктор интерфейса
// бросает TypeError.
#[test]
fn idb_idl_factory_instance_and_ctor() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                out.push('inst:' + (indexedDB instanceof IDBFactory));
                out.push('proto:' + (Object.getPrototypeOf(indexedDB) === IDBFactory.prototype));
                try { new IDBFactory(); out.push('ctor:no-throw'); }
                catch (err) { out.push('ctor:' + err.constructor.name); }
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("inst:true,proto:true,ctor:TypeError".into())
    );
}

// (2) upgradeneeded — настоящий IDBVersionChangeEvent с oldVersion = 0 и
// newVersion = 1.
#[test]
fn idb_idl_upgradeneeded_is_version_change_event() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('idl-ue', 1);
                open.onupgradeneeded = function(e) {
                    log.push((e instanceof IDBVersionChangeEvent) + ':' +
                             (e instanceof Event) + ':' + e.oldVersion + ':' + e.newVersion);
                    e.target.result.createObjectStore('s');
                };
                open.onsuccess = function() { log.push('ok'); };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("true:true:0:1,ok".into())
    );
}

// (3) Атрибуты IDBRequest — аксессоры прототипа над скрытыми полями: get —
// функция, собственных свойств `result` у реального запроса нет. Конструктор
// IDBRequest по WebIDL не подлежит `new` — для проверки own-полей берётся
// настоящий запрос из indexedDB.open.
#[test]
fn idb_idl_request_attributes_are_prototype_accessors() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                var d = Object.getOwnPropertyDescriptor(IDBRequest.prototype, 'result');
                out.push('get:' + typeof (d && d.get));
                var req = indexedDB.open('idl-req');
                out.push('own:' + (Object.keys(req).indexOf('result') < 0));
                out.push('val:' + (req.result === undefined));
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("get:function,own:true,val:true".into())
    );
}
// (4) openCursor отдаёт IDBCursorWithValue (он и IDBCursor), openKeyCursor —
// только IDBCursor; value есть лишь у первого.
#[test]
fn idb_idl_cursor_value_split() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
                var log = [];
                var open = indexedDB.open('idl-cur', 1);
                open.onupgradeneeded = function(e) {
                    var st = e.target.result.createObjectStore('s');
                    st.add('a', 1);
                    st.add('b', 2);
                };
                open.onsuccess = function(e) {
                    var db = e.target.result;
                    var vq = db.transaction('s').objectStore('s').openCursor();
                    var seen = 0;
                    vq.onsuccess = function() {
                        var cur = vq.result;
                        if (!cur) {
                            var kq = db.transaction('s').objectStore('s').openKeyCursor();
                            var pushed = 0;
                            kq.onsuccess = function() {
                                var kcur = kq.result;
                                if (kcur && pushed === 0) {
                                    pushed = 1;
                                    log.push('k:' + (kcur instanceof IDBCursor) + ':' +
                                             !(kcur instanceof IDBCursorWithValue) + ':' +
                                             (kcur.value === undefined));
                                    kcur.continue();
                                }
                            };
                            return;
                        }
                        if (seen === 0) {
                            log.push('v:' + (cur instanceof IDBCursorWithValue) + ':' +
                                     (cur instanceof IDBCursor) + ':' + cur.value);
                        }
                        seen++;
                        cur.continue();
                    };
                };
            "#).unwrap();
    rt.eval("0").unwrap();
    assert_eq!(
        rt.eval("log.join(',')").unwrap(),
        lumen_core::JsValue::String("v:true:true:a,k:true:true:true".into())
    );
}

// (5) Интерфейсные объекты не перечислимы на globalThis; toStringTag у
// прототипов — своё имя интерфейса.
#[test]
fn idb_idl_globals_not_enumerable() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                out.push('keys:' + (Object.keys(globalThis).indexOf('IDBRequest') < 0));
                out.push('own:' + (Object.getOwnPropertyNames(globalThis).indexOf('IDBRequest') >= 0));
                out.push('desc:' + Object.getOwnPropertyDescriptor(globalThis, 'IDBRequest').enumerable);
                out.push('tag:' + Object.prototype.toString.call(indexedDB));
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "keys:true,own:true,desc:false,tag:[object IDBFactory]".into()
        )
    );
}

// (6) Методы прототипов — length по WebIDL; операции с wrong this бросают
// TypeError.
#[test]
fn idb_idl_method_lengths_and_brand_checks() {
    let rt = v8_runtime_with_dom(make_doc());
    let r = rt.eval(r#"
                var out = [];
                out.push('put:' + IDBObjectStore.prototype.put.length);
                out.push('get:' + IDBObjectStore.prototype.get.length);
                out.push('openCursor:' + IDBObjectStore.prototype.openCursor.length);
                out.push('cmp:' + IDBFactory.prototype.cmp.length);
                out.push('evt:' + IDBVersionChangeEvent.length);
                out.push('name:' + IDBObjectStore.prototype.put.name);
                try { IDBObjectStore.prototype.put.call(null, 1); out.push('brand:no-throw'); }
                catch (err) { out.push('brand:' + err.constructor.name); }
                try { Object.getOwnPropertyDescriptor(IDBRequest.prototype, 'result').get.call({}); out.push('getter:no-throw'); }
                catch (err) { out.push('getter:' + err.constructor.name); }
                out.join(',')
            "#).unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String(
            "put:1,get:1,openCursor:0,cmp:2,evt:1,name:put,brand:TypeError,getter:TypeError".into()
        )
    );
}

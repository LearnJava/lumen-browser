//! IndexedDB JS-shim ↔ Rust-backend integration tests (Indexed Database API 3.0).
//!
//! Exercises the structured-backend wiring added in Phase 3 (`p1-ph3-indexeddb`):
//! the opaque snapshot blob persists + restores records losslessly across a runtime
//! rebuild ("reload"), while the schema (db version / object stores / indexes) is
//! additionally mirrored into the structured backend via `_lumen_idb_schema_op`.
//!
//! The backend here is a self-contained in-memory mock implementing
//! [`IdbBackend`]; it captures the snapshot and every schema op so the test can
//! assert both the blob path and the structured-mirror path without pulling in
//! `lumen-storage`.
//!
//! V8-only (S12b-F2 removed `QuickJsRuntime`, the only other `JsRuntime` impl
//! in this crate); default (no-`v8-backend`) builds compile this file to nothing.
#![cfg(feature = "v8-backend")]

use lumen_core::JsRuntime as _;
use lumen_core::ext::{IdbBackend, IdbSchemaOp};
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;
use std::sync::{Arc, Mutex};

/// In-memory [`IdbBackend`] capturing the snapshot blob and all schema ops.
#[derive(Default)]
struct MockIdb {
    /// Last snapshot written via `save` (the authoritative restore blob).
    snapshot: Mutex<Option<String>>,
    /// Every schema op applied via `apply_schema`, in order.
    schema_ops: Mutex<Vec<IdbSchemaOp>>,
}

impl IdbBackend for MockIdb {
    fn load(&self) -> Option<String> {
        self.snapshot.lock().unwrap().clone()
    }

    fn save(&self, snapshot: &str) {
        *self.snapshot.lock().unwrap() = Some(snapshot.to_owned());
    }

    fn apply_schema(&self, op: &IdbSchemaOp) -> lumen_core::Result<()> {
        self.schema_ops.lock().unwrap().push(op.clone());
        Ok(())
    }

    fn list_databases(&self) -> Vec<(String, u32)> {
        let mut out: Vec<(String, u32)> = Vec::new();
        for op in self.schema_ops.lock().unwrap().iter() {
            if let IdbSchemaOp::SetVersion { db_name, version } = op {
                if let Some(slot) = out.iter_mut().find(|(n, _)| n == db_name) {
                    slot.1 = *version;
                } else {
                    out.push((db_name.clone(), *version));
                }
            }
        }
        out
    }

    fn db_version(&self, db_name: &str) -> u32 {
        self.list_databases()
            .into_iter()
            .find(|(n, _)| n == db_name)
            .map(|(_, v)| v)
            .unwrap_or(0)
    }
}

fn make_rt(backend: Arc<dyn IdbBackend>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(
        doc,
        "https://example.test/",
        None,
        None,
        None,
        None,
        Some(backend),
        None,
        None,
        None,
        false,
    )
    .unwrap();
    rt
}

fn eval_str(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::String(s)) => s,
        Ok(other) => panic!("expected string from `{script}`, got {other:?}"),
        Err(e) => panic!("eval error in `{script}`: {e}"),
    }
}

/// Open a DB (v1) creating an object store + index, write two records, flush.
/// Then rebuild the runtime against the same backend ("reload") and read the
/// records back — values must survive via the snapshot blob.
#[test]
fn idb_records_survive_reload_via_snapshot() {
    let backend = Arc::new(MockIdb::default());

    // --- session 1: create schema + write records ---------------------------
    {
        let rt = make_rt(backend.clone() as Arc<dyn IdbBackend>);
        rt.eval(
            r#"
            var req = indexedDB.open('shop', 1);
            req.onupgradeneeded = function(ev) {
                var db = ev.target.result;
                var store = db.createObjectStore('books', { keyPath: 'id' });
                store.createIndex('byTitle', 'title', { unique: false });
            };
            req.onsuccess = function(ev) { window._db = ev.target.result; };
            'ok'
            "#,
        )
        .unwrap();
        // Drive the open + upgradeneeded + onsuccess.
        rt.eval("_lumen_idb_flush()").unwrap();
        // Write two records in a readwrite transaction, then flush (persists +
        // mirrors schema).
        rt.eval(
            r#"
            var tx = window._db.transaction('books', 'readwrite');
            var s = tx.objectStore('books');
            s.put({ id: 1, title: 'Dune' });
            s.put({ id: 2, title: 'Hyperion' });
            'ok'
            "#,
        )
        .unwrap();
        rt.eval("_lumen_idb_flush()").unwrap();
    }

    // The snapshot blob must have been written, and the schema mirrored.
    assert!(
        backend.snapshot.lock().unwrap().is_some(),
        "snapshot blob should be persisted after a mutating flush"
    );
    let has_store = backend
        .schema_ops
        .lock()
        .unwrap()
        .iter()
        .any(|op| matches!(op, IdbSchemaOp::CreateStore { store_name, .. } if store_name == "books"));
    assert!(has_store, "CreateStore('books') should be mirrored to the structured backend");
    let has_index = backend
        .schema_ops
        .lock()
        .unwrap()
        .iter()
        .any(|op| matches!(op, IdbSchemaOp::CreateIndex { index_name, .. } if index_name == "byTitle"));
    assert!(has_index, "CreateIndex('byTitle') should be mirrored");
    assert_eq!(backend.db_version("shop"), 1, "structured db_version should reflect SetVersion");

    // --- session 2: fresh runtime, same backend ("reload") ------------------
    let rt2 = make_rt(backend.clone() as Arc<dyn IdbBackend>);
    rt2.eval(
        r#"
        var req = indexedDB.open('shop', 1);
        req.onsuccess = function(ev) {
            var db = ev.target.result;
            var tx = db.transaction('books', 'readonly');
            var g = tx.objectStore('books').get(2);
            g.onsuccess = function() { window.__title = g.result ? g.result.title : null; };
        };
        'ok'
        "#,
    )
    .unwrap();
    rt2.eval("_lumen_idb_flush()").unwrap();
    let title = eval_str(&rt2, "String(window.__title)");
    assert_eq!(title, "Hyperion", "record written in session 1 must be readable after reload");
}

/// BUG-917: a dirty flush must mirror only the database that actually changed,
/// not re-walk every origin database's stores/indexes on every flush. Two
/// databases are created, then only one is mutated again (a second store);
/// the schema-op count contributed by the untouched database must not grow.
#[test]
fn idb_persist_schema_only_remirrors_dirty_databases() {
    let backend = Arc::new(MockIdb::default());
    let rt = make_rt(backend.clone() as Arc<dyn IdbBackend>);
    rt.eval(
        r#"
        var reqA = indexedDB.open('shopA', 1);
        reqA.onupgradeneeded = function(ev) { ev.target.result.createObjectStore('books'); };
        reqA.onsuccess = function(ev) { window._dbA = ev.target.result; };
        var reqB = indexedDB.open('shopB', 1);
        reqB.onupgradeneeded = function(ev) { ev.target.result.createObjectStore('toys'); };
        reqB.onsuccess = function(ev) { window._dbB = ev.target.result; };
        'ok'
        "#,
    )
    .unwrap();
    rt.eval("_lumen_idb_flush()").unwrap();

    let ops_after_create = backend.schema_ops.lock().unwrap().len();
    let shop_b_ops_after_create = backend
        .schema_ops
        .lock()
        .unwrap()
        .iter()
        .filter(|op| matches!(op,
            IdbSchemaOp::SetVersion { db_name, .. } if db_name == "shopB")
            || matches!(op, IdbSchemaOp::CreateStore { db_name, .. } if db_name == "shopB"))
        .count();
    assert!(ops_after_create > 0, "creating two databases should mirror at least their schema once");

    // Mutate only shopA (a second version upgrade adding a store); shopB is
    // never touched again.
    rt.eval(
        r#"
        window._dbA.close();
        var req2 = indexedDB.open('shopA', 2);
        req2.onupgradeneeded = function(ev) { ev.target.transaction.db.createObjectStore('authors'); };
        req2.onsuccess = function() {};
        'ok'
        "#,
    )
    .unwrap();
    rt.eval("_lumen_idb_flush()").unwrap();

    let shop_b_ops_after_mutation = backend
        .schema_ops
        .lock()
        .unwrap()
        .iter()
        .filter(|op| matches!(op,
            IdbSchemaOp::SetVersion { db_name, .. } if db_name == "shopB")
            || matches!(op, IdbSchemaOp::CreateStore { db_name, .. } if db_name == "shopB"))
        .count();
    assert_eq!(
        shop_b_ops_after_mutation, shop_b_ops_after_create,
        "an untouched database must not be re-mirrored by a flush that only dirtied another database"
    );
    assert!(
        backend.schema_ops.lock().unwrap().len() > ops_after_create,
        "shopA's new store must still be mirrored"
    );
}

/// BUG-916: `createIndex`/`deleteIndex` must apply at their own position in the
/// transaction's request queue, not synchronously — otherwise a `deleteIndex`
/// written after a data request retroactively erases the constraint that
/// request should have been checked against, and any request queued before an
/// index is created is checked against an index it should never have seen.
#[test]
fn idb_create_index_and_delete_index_are_ordered_with_data_requests() {
    let backend = Arc::new(MockIdb::default());
    let rt = make_rt(backend as Arc<dyn IdbBackend>);
    rt.eval(
        r#"
        var req = indexedDB.open('shop', 1);
        req.onupgradeneeded = function(ev) {
            ev.target.result.createObjectStore('animals');
        };
        req.onsuccess = function(ev) { window._db = ev.target.result; };
        'ok'
        "#,
    )
    .unwrap();
    // createIndex/deleteIndex are only allowed on a versionchange transaction
    // (Indexed DB §3.2.9/§3.2.10), so the ordering scenario itself must run
    // inside a second upgrade, interleaved with data requests on that same
    // transaction — exactly the shape BUG-916 was found in.
    rt.eval(
        r#"
        window._log = [];
        window._db.close();
        var req2 = indexedDB.open('shop', 2);
        req2.onupgradeneeded = function(ev) {
            var s = ev.target.transaction.objectStore('animals');
            s.add({ animal: 'Unicorn' }, 1).onsuccess = function() { window._log.push('add1: success'); };
            s.createIndex('byAnimal', 'animal', { unique: true });
            var rq2 = s.add({ animal: 'Unicorn' }, 2);
            rq2.onsuccess = function() { window._log.push('add2: success'); };
            rq2.onerror = function(e) { e.preventDefault(); window._log.push('add2: ' + this.error.name); };
            s.deleteIndex('byAnimal');
            var rq3 = s.add({ animal: 'Unicorn' }, 3);
            rq3.onsuccess = function() { window._log.push('add3: success'); };
            rq3.onerror = function(e) { e.preventDefault(); window._log.push('add3: ' + this.error.name); };
        };
        'ok'
        "#,
    )
    .unwrap();
    rt.eval("_lumen_idb_flush()").unwrap();
    let log = eval_str(&rt, "window._log.join('|')");
    assert_eq!(
        log,
        "add1: success|add2: ConstraintError|add3: success",
        "add2 must be rejected by the unique index created before it and add3 must succeed \
         once deleteIndex (queued before it) has removed that index"
    );
}

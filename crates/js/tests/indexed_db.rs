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

use lumen_core::JsRuntime as _;
use lumen_core::ext::{IdbBackend, IdbSchemaOp};
use lumen_dom::Document;
use lumen_js::QuickJsRuntime;
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

fn make_rt(backend: Arc<dyn IdbBackend>) -> QuickJsRuntime {
    let rt = QuickJsRuntime::new().unwrap();
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

fn eval_str(rt: &QuickJsRuntime, script: &str) -> String {
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

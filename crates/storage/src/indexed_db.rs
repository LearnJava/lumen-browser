//! Persistence для IndexedDB JS-шима поверх [`StorageBackend`].
//!
//! [`IdbStore`] реализует [`IdbBackend`](lumen_core::ext::IdbBackend): JS-рантайм
//! сериализует все базы IndexedDB текущего origin в один JSON-снимок, а этот
//! store кладёт его в произвольный `StorageBackend` (in-memory на время процесса
//! или SQLite на диске) под ключом `__indexeddb__`, партиционированным по origin.
//!
//! Снимок — непрозрачный для store блок: контракт лишь в том, чтобы вернуть его
//! байт-в-байт при следующем `load` для того же origin. Это позволяет базам
//! переживать reload страницы (новый JS-рантайм на каждую страницу теряет heap,
//! но `load` восстанавливает состояние).

use std::sync::{Arc, Mutex};

use lumen_core::ext::{IdbBackend, StorageBackend};

/// Ключ внутри partition origin, под которым хранится JSON-снимок всех баз
/// IndexedDB этого origin. Один снимок на origin — шим сериализует целиком.
const IDB_SNAPSHOT_KEY: &str = "__indexeddb__";

/// Per-origin persistence для IndexedDB поверх общего [`StorageBackend`].
///
/// Несколько `IdbStore` (по одному на origin) разделяют один backend через
/// `Arc<Mutex<…>>`; origin-партиционирование делает сам backend, так что данные
/// разных источников изолированы.
pub struct IdbStore {
    /// Разделяемый backend (in-memory или SQLite). `Mutex`, потому что
    /// [`StorageBackend::put`] требует `&mut self`.
    backend: Arc<Mutex<dyn StorageBackend>>,
    /// Origin (scheme+host+port), под которым партиционируется снимок.
    origin: String,
}

impl IdbStore {
    /// Создать store для конкретного `origin` поверх разделяемого `backend`.
    pub fn new(backend: Arc<Mutex<dyn StorageBackend>>, origin: impl Into<String>) -> Self {
        Self {
            backend,
            origin: origin.into(),
        }
    }
}

impl IdbBackend for IdbStore {
    fn load(&self) -> Option<String> {
        let backend = self.backend.lock().ok()?;
        let bytes = backend
            .get(Some(&self.origin), None, IDB_SNAPSHOT_KEY)
            .ok()??;
        String::from_utf8(bytes).ok()
    }

    fn save(&self, snapshot: &str) {
        // Best-effort: ошибка persistence не должна срывать JS-транзакцию.
        if let Ok(mut backend) = self.backend.lock() {
            let _ = backend.put(
                Some(&self.origin),
                None,
                IDB_SNAPSHOT_KEY,
                snapshot.as_bytes(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryStorage;
    use crate::sqlite_store::SqliteStorage;

    fn in_memory() -> Arc<Mutex<dyn StorageBackend>> {
        Arc::new(Mutex::new(InMemoryStorage::new()))
    }

    #[test]
    fn load_missing_returns_none() {
        let store = IdbStore::new(in_memory(), "https://example.com");
        assert_eq!(store.load(), None);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let store = IdbStore::new(in_memory(), "https://example.com");
        store.save(r#"{"db1":{"name":"db1","version":1,"stores":{}}}"#);
        assert_eq!(
            store.load().as_deref(),
            Some(r#"{"db1":{"name":"db1","version":1,"stores":{}}}"#)
        );
    }

    #[test]
    fn save_overwrites_previous_snapshot() {
        let store = IdbStore::new(in_memory(), "https://example.com");
        store.save("first");
        store.save("second");
        assert_eq!(store.load().as_deref(), Some("second"));
    }

    #[test]
    fn origins_are_isolated() {
        let backend = in_memory();
        let a = IdbStore::new(Arc::clone(&backend), "https://a.com");
        let b = IdbStore::new(Arc::clone(&backend), "https://b.com");
        a.save("alpha");
        b.save("beta");
        assert_eq!(a.load().as_deref(), Some("alpha"));
        assert_eq!(b.load().as_deref(), Some("beta"));
    }

    #[test]
    fn shared_backend_survives_store_recreation() {
        // Тот же backend + тот же origin → новый `IdbStore` (как новый
        // JS-рантайм после reload) видит ранее сохранённый снимок.
        let backend = in_memory();
        IdbStore::new(Arc::clone(&backend), "https://example.com").save("persisted");
        let reopened = IdbStore::new(Arc::clone(&backend), "https://example.com");
        assert_eq!(reopened.load().as_deref(), Some("persisted"));
    }

    #[test]
    fn sqlite_backend_persists_across_store_recreation() {
        let backend: Arc<Mutex<dyn StorageBackend>> =
            Arc::new(Mutex::new(SqliteStorage::open_in_memory().unwrap()));
        IdbStore::new(Arc::clone(&backend), "https://example.com")
            .save(r#"{"db":{"version":3}}"#);
        let reopened = IdbStore::new(Arc::clone(&backend), "https://example.com");
        assert_eq!(reopened.load().as_deref(), Some(r#"{"db":{"version":3}}"#));
    }

    #[test]
    fn unicode_snapshot_roundtrip() {
        let store = IdbStore::new(in_memory(), "https://кириллица.рф");
        let snapshot = r#"{"база":{"records":[{"key":"ключ","value":"значение"}]}}"#;
        store.save(snapshot);
        assert_eq!(store.load().as_deref(), Some(snapshot));
    }
}

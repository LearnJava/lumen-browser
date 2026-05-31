//! JS-shim Service Worker registration persistence поверх [`StorageBackend`].
//!
//! [`SwStore`] реализует [`SwBackend`](lumen_core::ext::SwBackend): JS-рантайм
//! сериализует все SW-регистрации текущего origin в один JSON-снимок, а этот
//! store кладёт его в произвольный `StorageBackend` (in-memory на время
//! процесса или SQLite на диске) под ключом `__sw_registrations__`,
//! партиционированным по origin.
//!
//! Снимок — непрозрачный для store блок: контракт лишь в том, чтобы вернуть
//! его байт-в-байт при следующем `load` для того же origin. Это позволяет
//! JS-регистрациям переживать reload страницы (новый JS-рантайм на каждую
//! страницу теряет heap, но `load` восстанавливает состояние).

use std::sync::{Arc, Mutex};

use lumen_core::ext::{StorageBackend, SwBackend};

/// Ключ под которым хранится JSON-снимок SW-регистраций для одного origin.
const SW_SNAPSHOT_KEY: &str = "__sw_registrations__";

/// Per-origin persistence SW-регистраций поверх общего [`StorageBackend`].
///
/// Несколько `SwStore` (по одному на origin) разделяют один backend через
/// `Arc<Mutex<…>>`; origin-партиционирование делает сам backend.
pub struct SwStore {
    /// Разделяемый backend (in-memory или SQLite). `Mutex`, потому что
    /// [`StorageBackend::put`] требует `&mut self`.
    backend: Arc<Mutex<dyn StorageBackend>>,
    /// Origin (scheme+host+port), под которым партиционируется снимок.
    origin: String,
}

impl SwStore {
    /// Создать store для конкретного `origin` поверх разделяемого `backend`.
    pub fn new(backend: Arc<Mutex<dyn StorageBackend>>, origin: impl Into<String>) -> Self {
        Self {
            backend,
            origin: origin.into(),
        }
    }
}

impl SwBackend for SwStore {
    fn load(&self) -> Option<String> {
        let backend = self.backend.lock().ok()?;
        let bytes = backend
            .get(Some(&self.origin), None, SW_SNAPSHOT_KEY)
            .ok()??;
        String::from_utf8(bytes).ok()
    }

    fn save(&self, snapshot: &str) {
        // Best-effort: ошибка persistence не должна срывать JS-операцию.
        if let Ok(mut backend) = self.backend.lock() {
            let _ = backend.put(
                Some(&self.origin),
                None,
                SW_SNAPSHOT_KEY,
                snapshot.as_bytes(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_store::SqliteStorage;
    use crate::store::InMemoryStorage;

    fn in_memory() -> Arc<Mutex<dyn StorageBackend>> {
        Arc::new(Mutex::new(InMemoryStorage::new()))
    }

    #[test]
    fn load_missing_returns_none() {
        let store = SwStore::new(in_memory(), "https://example.com");
        assert_eq!(store.load(), None);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let store = SwStore::new(in_memory(), "https://example.com");
        let snap = r#"[{"scope":"/","scriptURL":"/sw.js","state":"activated"}]"#;
        store.save(snap);
        assert_eq!(store.load().as_deref(), Some(snap));
    }

    #[test]
    fn save_overwrites_previous_snapshot() {
        let store = SwStore::new(in_memory(), "https://example.com");
        store.save("first");
        store.save("second");
        assert_eq!(store.load().as_deref(), Some("second"));
    }

    #[test]
    fn origins_are_isolated() {
        let backend = in_memory();
        let a = SwStore::new(Arc::clone(&backend), "https://a.com");
        let b = SwStore::new(Arc::clone(&backend), "https://b.com");
        a.save(r#"[{"scope":"/","scriptURL":"/sw-a.js"}]"#);
        b.save(r#"[{"scope":"/","scriptURL":"/sw-b.js"}]"#);
        assert!(a.load().unwrap().contains("sw-a"));
        assert!(b.load().unwrap().contains("sw-b"));
    }

    #[test]
    fn shared_backend_survives_store_recreation() {
        let backend = in_memory();
        SwStore::new(Arc::clone(&backend), "https://example.com")
            .save(r#"[{"scope":"/app/"}]"#);
        let reopened = SwStore::new(Arc::clone(&backend), "https://example.com");
        assert_eq!(reopened.load().as_deref(), Some(r#"[{"scope":"/app/"}]"#));
    }

    #[test]
    fn sqlite_backend_persists_across_store_recreation() {
        let backend: Arc<Mutex<dyn StorageBackend>> =
            Arc::new(Mutex::new(SqliteStorage::open_in_memory().unwrap()));
        SwStore::new(Arc::clone(&backend), "https://example.com")
            .save(r#"[{"scope":"/","state":"activated"}]"#);
        let reopened = SwStore::new(Arc::clone(&backend), "https://example.com");
        assert_eq!(
            reopened.load().as_deref(),
            Some(r#"[{"scope":"/","state":"activated"}]"#)
        );
    }
}

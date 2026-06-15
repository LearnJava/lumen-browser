//! Persistence для IndexedDB JS-шима поверх [`StorageBackend`].
//!
//! Два режима:
//! - **Shared backend** ([`IdbStore::new`]): несколько origin-ов разделяют один
//!   `StorageBackend` (in-memory или SQLite), партиционированный по origin-ключу.
//!   Подходит для тестов и ephemeral сессий.
//! - **Per-origin SQLite** ([`IdbStore::open_or_create`] / [`IdbStore::for_origin`]):
//!   каждый origin получает отдельный файл `~/.config/lumen/idb/{key}.db`, где
//!   `key = sha256_hex(eTLD+1)[:16]`. Это изолирует данные и позволяет параллельно
//!   открывать одну и ту же базу из разных вкладок (SQLite WAL обеспечивает
//!   корректность конкурентного доступа).
//!
//! Снимок — непрозрачный блок: контракт — вернуть байт-в-байт при следующем `load`
//! для того же origin. Базы переживают reload страницы (JS-heap теряется, но `load`
//! восстанавливает состояние из файла).

use std::path::Path;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use lumen_core::ext::{IdbBackend, StorageBackend};

use crate::sqlite_store::SqliteStorage;

/// Ключ внутри partition origin, под которым хранится JSON-снимок всех баз
/// IndexedDB этого origin. Один снимок на origin — шим сериализует целиком.
const IDB_SNAPSHOT_KEY: &str = "__indexeddb__";

/// Вычислить безопасный файловый ключ для origin.
///
/// Возвращает первые 16 hex-символов SHA-256 от `etld_plus_one` (eTLD+1
/// страницы, например `example.com`). Результат используется как имя файла
/// `{key}.db` в директории IDB-хранилища.
///
/// Длина 16 символов = 64 бита энтропии — достаточно для изоляции тысяч
/// origin-ов без коллизий на практике, при этом имя файла компактно.
pub fn origin_key(etld_plus_one: &str) -> String {
    let hash = Sha256::digest(etld_plus_one.as_bytes());
    hash.iter()
        .flat_map(|b| {
            let hi = b >> 4;
            let lo = b & 0xf;
            [
                char::from_digit(u32::from(hi), 16).unwrap() as u8,
                char::from_digit(u32::from(lo), 16).unwrap() as u8,
            ]
        })
        .take(16)
        .map(char::from)
        .collect()
}

/// Per-origin persistence для IndexedDB поверх [`StorageBackend`].
///
/// Два режима конструирования:
/// - [`IdbStore::new`] — разделяемый backend, origin используется как ключ
///   партиционирования внутри backend.
/// - [`IdbStore::open_or_create`] — собственный SQLite-файл; `origin` пустой,
///   т.к. изоляция достигается самим файлом, а не ключом.
pub struct IdbStore {
    /// Backend (in-memory или SQLite). `Mutex`, потому что
    /// [`StorageBackend::put`] требует `&mut self`.
    backend: Arc<Mutex<dyn StorageBackend>>,
    /// Origin-ключ для партиционирования в shared-backend режиме.
    /// Пустая строка в режиме per-origin SQLite (изоляция — сам файл).
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

    /// Открыть или создать выделенный SQLite-файл для IndexedDB.
    ///
    /// Каждый origin получает отдельный файл — изоляция данных гарантируется
    /// самим файлом, поэтому `origin` внутри хранится как пустая строка.
    /// SQLite WAL допускает конкурентный доступ из нескольких вкладок к
    /// одному и тому же файлу.
    pub fn open_or_create(path: &Path) -> lumen_core::Result<Self> {
        let sqlite = SqliteStorage::open(path)?;
        Ok(Self {
            backend: Arc::new(Mutex::new(sqlite)),
            origin: String::new(),
        })
    }

    /// Открыть или создать IDB-хранилище для `etld_plus_one` в директории `idb_dir`.
    ///
    /// Путь файла: `idb_dir/{origin_key(etld_plus_one)}.db`. Директория `idb_dir`
    /// должна существовать — её создаёт вызывающий код (shell при старте).
    pub fn for_origin(
        etld_plus_one: &str,
        idb_dir: &Path,
    ) -> lumen_core::Result<Arc<dyn IdbBackend>> {
        let key = origin_key(etld_plus_one);
        let path = idb_dir.join(format!("{key}.db"));
        let store = Self::open_or_create(&path)?;
        Ok(Arc::new(store))
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

    /// Создаёт временную директорию для тестов с файловым SQLite.
    fn tmp_idb_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen_idb_test_{suffix}"));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        dir
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

    // ── origin_key() ──

    #[test]
    fn origin_key_length_is_16() {
        assert_eq!(origin_key("example.com").len(), 16);
        assert_eq!(origin_key("co.uk").len(), 16);
        assert_eq!(origin_key("localhost").len(), 16);
    }

    #[test]
    fn origin_key_is_hex() {
        let key = origin_key("example.com");
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn origin_key_deterministic() {
        assert_eq!(origin_key("example.com"), origin_key("example.com"));
    }

    #[test]
    fn origin_key_different_domains_differ() {
        assert_ne!(origin_key("example.com"), origin_key("other.com"));
        assert_ne!(origin_key("foo.example.com"), origin_key("example.com"));
    }

    #[test]
    fn origin_key_known_value() {
        // SHA-256("example.com") = a379a6f6eeafb9a55e378c118034e2751e682fab9f2d30ab13d2125586ce1947
        // first 16 chars = "a379a6f6eeafb9a5"
        assert_eq!(origin_key("example.com"), "a379a6f6eeafb9a5");
    }

    // ── IdbStore::open_or_create() ──

    #[test]
    fn open_or_create_roundtrip() {
        let dir = tmp_idb_dir("open_or_create");
        let path = dir.join("test.db");
        {
            let store = IdbStore::open_or_create(&path).unwrap();
            store.save(r#"{"db":{"version":2}}"#);
        }
        // Открываем заново — данные должны сохраниться.
        let reopened = IdbStore::open_or_create(&path).unwrap();
        assert_eq!(reopened.load().as_deref(), Some(r#"{"db":{"version":2}}"#));
        let _ = std::fs::remove_file(&path);
    }

    // ── IdbStore::for_origin() ──

    #[test]
    fn for_origin_creates_file_with_key_name() {
        let dir = tmp_idb_dir("for_origin_creates");
        let key = origin_key("example.com");
        let expected_path = dir.join(format!("{key}.db"));

        IdbStore::for_origin("example.com", &dir).unwrap();
        assert!(expected_path.exists(), "expected {expected_path:?} to exist");
        let _ = std::fs::remove_file(&expected_path);
    }

    #[test]
    fn for_origin_two_domains_get_different_files() {
        let dir = tmp_idb_dir("for_origin_two");
        let key_a = origin_key("example.com");
        let key_b = origin_key("other.org");
        assert_ne!(key_a, key_b);

        let store_a = IdbStore::for_origin("example.com", &dir).unwrap();
        let store_b = IdbStore::for_origin("other.org", &dir).unwrap();
        store_a.save("data-a");
        store_b.save("data-b");

        // Перезапускаем — данные изолированы.
        let ra = IdbStore::for_origin("example.com", &dir).unwrap();
        let rb = IdbStore::for_origin("other.org", &dir).unwrap();
        assert_eq!(ra.load().as_deref(), Some("data-a"));
        assert_eq!(rb.load().as_deref(), Some("data-b"));

        let _ = std::fs::remove_file(dir.join(format!("{key_a}.db")));
        let _ = std::fs::remove_file(dir.join(format!("{key_b}.db")));
    }
}

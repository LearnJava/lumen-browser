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
use lumen_core::ext::{IdbOpResult, IdbRecordOp, IdbSchemaOp};
use lumen_core::{Error, Result};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

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

/// Structured per-origin SQLite backend for IndexedDB (Phase 3).
///
/// One `.db` file per origin (same naming as [`IdbStore::for_origin`]), but instead
/// of one opaque JSON blob the data lives in a relational schema:
/// `idb_meta` (database versions), `idb_stores` (object-store definitions),
/// `idb_indexes` (index definitions) and `idb_records` (one row per stored value,
/// keyed by `(db_name, store_name, key_json)`). `idb_snapshot` keeps a single-row
/// opaque blob so the legacy [`IdbBackend::load`]/[`IdbBackend::save`] path keeps
/// working for the JS shim's snapshot fallback.
///
/// The `rusqlite::Connection` is not `Sync`, so it is wrapped in a `Mutex` to make
/// `NativeIdbStore` `Send + Sync`. SQLite WAL lets several tabs on the same origin
/// share the file (one writer + N readers).
pub struct NativeIdbStore {
    /// Open connection to the per-origin SQLite file (or `:memory:` in tests).
    conn: Mutex<Connection>,
}

impl NativeIdbStore {
    /// Run the schema-init batch on a fresh connection and wrap it.
    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS idb_meta(
                 db_name TEXT PRIMARY KEY,
                 version INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS idb_stores(
                 db_name TEXT NOT NULL,
                 store_name TEXT NOT NULL,
                 key_path TEXT,
                 auto_inc INTEGER NOT NULL DEFAULT 0,
                 key_gen INTEGER NOT NULL DEFAULT 1,
                 PRIMARY KEY(db_name,store_name)
             );
             CREATE TABLE IF NOT EXISTS idb_indexes(
                 db_name TEXT NOT NULL,
                 store_name TEXT NOT NULL,
                 index_name TEXT NOT NULL,
                 key_path TEXT NOT NULL,
                 is_unique INTEGER NOT NULL DEFAULT 0,
                 multi_entry INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY(db_name,store_name,index_name)
             );
             CREATE TABLE IF NOT EXISTS idb_records(
                 db_name TEXT NOT NULL,
                 store_name TEXT NOT NULL,
                 key_json TEXT NOT NULL,
                 value_json TEXT NOT NULL,
                 PRIMARY KEY(db_name,store_name,key_json)
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS idb_snapshot(
                 id INTEGER PRIMARY KEY CHECK(id=0),
                 snapshot TEXT NOT NULL
             );",
        )
        .map_err(|e| Error::Storage(format!("idb sqlite init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open or create the structured IDB store at `path` (file is created if absent).
    pub fn open_or_create(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("idb sqlite open: {e}")))?;
        Self::init(conn)
    }

    /// Open an in-memory structured IDB store (tests / ephemeral sessions).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("idb sqlite open: {e}")))?;
        Self::init(conn)
    }

    /// Open/create the structured store for `etld_plus_one` under `idb_dir`.
    ///
    /// File path: `idb_dir/{origin_key(etld_plus_one)}.db`. `idb_dir` must exist.
    pub fn for_origin(etld_plus_one: &str, idb_dir: &Path) -> Result<Arc<dyn IdbBackend>> {
        let path = idb_dir.join(format!("{}.db", origin_key(etld_plus_one)));
        Ok(Arc::new(Self::open_or_create(&path)?))
    }
}

/// Build a key-range SQL fragment plus its bound parameter values.
///
/// `lower`/`upper` are inclusive `key_json` bounds (`None` = unbounded). Returns the
/// fragment (e.g. `" AND key_json>=? AND key_json<=?"`) and the values in order.
fn range_where(lower: &Option<String>, upper: &Option<String>) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut vals: Vec<Value> = Vec::new();
    if let Some(l) = lower {
        sql.push_str(" AND key_json>=?");
        vals.push(Value::Text(l.clone()));
    }
    if let Some(u) = upper {
        sql.push_str(" AND key_json<=?");
        vals.push(Value::Text(u.clone()));
    }
    (sql, vals)
}

/// Execute one record op against an open connection (so `commit_txn` can reuse it
/// inside a `Transaction`, which derefs to `&Connection`). Read ops return data;
/// write ops return [`IdbOpResult::None`].
fn exec_op_conn(conn: &Connection, op: &IdbRecordOp) -> Result<IdbOpResult> {
    match op {
        IdbRecordOp::Put {
            db_name,
            store_name,
            key_json,
            value_json,
        } => {
            conn.execute(
                "INSERT OR REPLACE INTO idb_records(db_name,store_name,key_json,value_json) VALUES(?,?,?,?)",
                params![db_name, store_name, key_json, value_json],
            )
            .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            Ok(IdbOpResult::None)
        }
        IdbRecordOp::Delete {
            db_name,
            store_name,
            key_json,
        } => {
            conn.execute(
                "DELETE FROM idb_records WHERE db_name=? AND store_name=? AND key_json=?",
                params![db_name, store_name, key_json],
            )
            .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            Ok(IdbOpResult::None)
        }
        IdbRecordOp::DeleteRange {
            db_name,
            store_name,
            lower,
            upper,
        } => {
            let (frag, vals) = range_where(lower, upper);
            let sql = format!("DELETE FROM idb_records WHERE db_name=? AND store_name=?{frag}");
            let mut all: Vec<Value> =
                vec![Value::Text(db_name.clone()), Value::Text(store_name.clone())];
            all.extend(vals);
            conn.execute(&sql, params_from_iter(&all))
                .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            Ok(IdbOpResult::None)
        }
        IdbRecordOp::Clear {
            db_name,
            store_name,
        } => {
            conn.execute(
                "DELETE FROM idb_records WHERE db_name=? AND store_name=?",
                params![db_name, store_name],
            )
            .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            Ok(IdbOpResult::None)
        }
        IdbRecordOp::Get {
            db_name,
            store_name,
            key_json,
        } => {
            let value: Option<String> = conn
                .query_row(
                    "SELECT value_json FROM idb_records WHERE db_name=? AND store_name=? AND key_json=?",
                    params![db_name, store_name, key_json],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            Ok(IdbOpResult::Value(value))
        }
        IdbRecordOp::GetAll {
            db_name,
            store_name,
            lower,
            upper,
            count,
        } => {
            let (frag, vals) = range_where(lower, upper);
            let mut sql = format!(
                "SELECT key_json,value_json FROM idb_records WHERE db_name=? AND store_name=?{frag} ORDER BY key_json ASC"
            );
            if *count > 0 {
                sql.push_str(&format!(" LIMIT {count}"));
            }
            let mut all: Vec<Value> =
                vec![Value::Text(db_name.clone()), Value::Text(store_name.clone())];
            all.extend(vals);
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            let rows = stmt
                .query_map(params_from_iter(&all), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?
                .collect::<rusqlite::Result<Vec<(String, String)>>>()
                .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            Ok(IdbOpResult::Records(rows))
        }
        IdbRecordOp::Count {
            db_name,
            store_name,
            lower,
            upper,
        } => {
            let (frag, vals) = range_where(lower, upper);
            let sql =
                format!("SELECT COUNT(*) FROM idb_records WHERE db_name=? AND store_name=?{frag}");
            let mut all: Vec<Value> =
                vec![Value::Text(db_name.clone()), Value::Text(store_name.clone())];
            all.extend(vals);
            let n: i64 = conn
                .query_row(&sql, params_from_iter(&all), |r| r.get(0))
                .map_err(|e| Error::Storage(format!("idb exec_op: {e}")))?;
            Ok(IdbOpResult::Count(n as u64))
        }
    }
}

impl IdbBackend for NativeIdbStore {
    fn load(&self) -> Option<String> {
        let Ok(conn) = self.conn.lock() else {
            return None;
        };
        conn.query_row("SELECT snapshot FROM idb_snapshot WHERE id=0", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()
        .ok()
        .flatten()
    }

    fn save(&self, snapshot: &str) {
        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO idb_snapshot(id,snapshot) VALUES(0,?1) ON CONFLICT(id) DO UPDATE SET snapshot=excluded.snapshot",
                params![snapshot],
            );
        }
    }

    fn db_version(&self, db_name: &str) -> u32 {
        let Ok(conn) = self.conn.lock() else {
            return 0;
        };
        conn.query_row(
            "SELECT version FROM idb_meta WHERE db_name=?1",
            params![db_name],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or(0) as u32
    }

    fn list_databases(&self) -> Vec<(String, u32)> {
        let Ok(conn) = self.conn.lock() else {
            return Vec::new();
        };
        let mut stmt = match conn.prepare("SELECT db_name,version FROM idb_meta ORDER BY db_name") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        }) {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(std::result::Result::ok)
            .map(|(name, ver)| (name, ver as u32))
            .collect()
    }

    fn apply_schema(&self, op: &IdbSchemaOp) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("idb conn poisoned".into()))?;
        match op {
            IdbSchemaOp::SetVersion { db_name, version } => {
                conn.execute(
                    "INSERT INTO idb_meta(db_name,version) VALUES(?1,?2) ON CONFLICT(db_name) DO UPDATE SET version=excluded.version",
                    params![db_name, *version as i64],
                )
                .map_err(|e| Error::Storage(format!("idb apply_schema: {e}")))?;
            }
            IdbSchemaOp::CreateStore {
                db_name,
                store_name,
                key_path,
                auto_increment,
            } => {
                conn.execute(
                    "INSERT OR REPLACE INTO idb_stores(db_name,store_name,key_path,auto_inc,key_gen) VALUES(?1,?2,?3,?4,1)",
                    params![db_name, store_name, key_path, *auto_increment as i64],
                )
                .map_err(|e| Error::Storage(format!("idb apply_schema: {e}")))?;
            }
            IdbSchemaOp::DeleteStore {
                db_name,
                store_name,
            } => {
                conn.execute(
                    "DELETE FROM idb_records WHERE db_name=?1 AND store_name=?2",
                    params![db_name, store_name],
                )
                .map_err(|e| Error::Storage(format!("idb apply_schema: {e}")))?;
                conn.execute(
                    "DELETE FROM idb_indexes WHERE db_name=?1 AND store_name=?2",
                    params![db_name, store_name],
                )
                .map_err(|e| Error::Storage(format!("idb apply_schema: {e}")))?;
                conn.execute(
                    "DELETE FROM idb_stores WHERE db_name=?1 AND store_name=?2",
                    params![db_name, store_name],
                )
                .map_err(|e| Error::Storage(format!("idb apply_schema: {e}")))?;
            }
            IdbSchemaOp::CreateIndex {
                db_name,
                store_name,
                index_name,
                key_path,
                unique,
                multi_entry,
            } => {
                conn.execute(
                    "INSERT OR REPLACE INTO idb_indexes(db_name,store_name,index_name,key_path,is_unique,multi_entry) VALUES(?1,?2,?3,?4,?5,?6)",
                    params![db_name, store_name, index_name, key_path, *unique as i64, *multi_entry as i64],
                )
                .map_err(|e| Error::Storage(format!("idb apply_schema: {e}")))?;
            }
            IdbSchemaOp::DeleteIndex {
                db_name,
                store_name,
                index_name,
            } => {
                conn.execute(
                    "DELETE FROM idb_indexes WHERE db_name=?1 AND store_name=?2 AND index_name=?3",
                    params![db_name, store_name, index_name],
                )
                .map_err(|e| Error::Storage(format!("idb apply_schema: {e}")))?;
            }
        }
        Ok(())
    }

    fn exec_op(&self, op: &IdbRecordOp) -> Result<IdbOpResult> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("idb conn poisoned".into()))?;
        exec_op_conn(&conn, op)
    }

    fn commit_txn(&self, ops: &[IdbRecordOp]) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("idb conn poisoned".into()))?;
        let tx = conn
            .transaction()
            .map_err(|e| Error::Storage(format!("idb txn begin: {e}")))?;
        for op in ops {
            exec_op_conn(&tx, op)?;
        }
        tx.commit()
            .map_err(|e| Error::Storage(format!("idb txn commit: {e}")))?;
        Ok(())
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

    // ── NativeIdbStore (structured SQLite backend) ──

    fn put(db: &str, store: &str, k: &str, v: &str) -> IdbRecordOp {
        IdbRecordOp::Put {
            db_name: db.into(),
            store_name: store.into(),
            key_json: k.into(),
            value_json: v.into(),
        }
    }

    #[test]
    fn native_set_version_and_db_version() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        assert_eq!(s.db_version("app"), 0);
        s.apply_schema(&IdbSchemaOp::SetVersion {
            db_name: "app".into(),
            version: 3,
        })
        .unwrap();
        assert_eq!(s.db_version("app"), 3);
        // Re-applying a higher version overwrites.
        s.apply_schema(&IdbSchemaOp::SetVersion {
            db_name: "app".into(),
            version: 5,
        })
        .unwrap();
        assert_eq!(s.db_version("app"), 5);
    }

    #[test]
    fn native_list_databases_sorted() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        for (name, ver) in [("zeta", 1u32), ("alpha", 2), ("mid", 7)] {
            s.apply_schema(&IdbSchemaOp::SetVersion {
                db_name: name.into(),
                version: ver,
            })
            .unwrap();
        }
        assert_eq!(
            s.list_databases(),
            vec![
                ("alpha".to_owned(), 2),
                ("mid".to_owned(), 7),
                ("zeta".to_owned(), 1),
            ]
        );
    }

    #[test]
    fn native_put_get_roundtrip() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        s.exec_op(&put("db", "books", "\"k1\"", "{\"t\":\"a\"}")).unwrap();
        let got = s
            .exec_op(&IdbRecordOp::Get {
                db_name: "db".into(),
                store_name: "books".into(),
                key_json: "\"k1\"".into(),
            })
            .unwrap();
        assert_eq!(got, IdbOpResult::Value(Some("{\"t\":\"a\"}".to_owned())));
        // Missing key → Value(None).
        let miss = s
            .exec_op(&IdbRecordOp::Get {
                db_name: "db".into(),
                store_name: "books".into(),
                key_json: "\"nope\"".into(),
            })
            .unwrap();
        assert_eq!(miss, IdbOpResult::Value(None));
    }

    #[test]
    fn native_put_overwrites_same_key() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        s.exec_op(&put("db", "s", "\"k\"", "v1")).unwrap();
        s.exec_op(&put("db", "s", "\"k\"", "v2")).unwrap();
        let got = s
            .exec_op(&IdbRecordOp::Get {
                db_name: "db".into(),
                store_name: "s".into(),
                key_json: "\"k\"".into(),
            })
            .unwrap();
        assert_eq!(got, IdbOpResult::Value(Some("v2".to_owned())));
    }

    #[test]
    fn native_getall_range_order_and_limit() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        // Keys chosen so lexicographic order is a<b<c<d.
        for k in ["\"a\"", "\"b\"", "\"c\"", "\"d\""] {
            s.exec_op(&put("db", "s", k, "v")).unwrap();
        }
        // Unbounded, ascending.
        let all = s
            .exec_op(&IdbRecordOp::GetAll {
                db_name: "db".into(),
                store_name: "s".into(),
                lower: None,
                upper: None,
                count: 0,
            })
            .unwrap();
        let IdbOpResult::Records(rows) = all else {
            panic!("expected Records");
        };
        let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["\"a\"", "\"b\"", "\"c\"", "\"d\""]);

        // Inclusive range [b, c].
        let ranged = s
            .exec_op(&IdbRecordOp::GetAll {
                db_name: "db".into(),
                store_name: "s".into(),
                lower: Some("\"b\"".into()),
                upper: Some("\"c\"".into()),
                count: 0,
            })
            .unwrap();
        let IdbOpResult::Records(rows) = ranged else {
            panic!("expected Records");
        };
        assert_eq!(rows.len(), 2);

        // count limit.
        let limited = s
            .exec_op(&IdbRecordOp::GetAll {
                db_name: "db".into(),
                store_name: "s".into(),
                lower: None,
                upper: None,
                count: 1,
            })
            .unwrap();
        let IdbOpResult::Records(rows) = limited else {
            panic!("expected Records");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "\"a\"");
    }

    #[test]
    fn native_count_delete_deleterange_clear() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        for k in ["\"a\"", "\"b\"", "\"c\""] {
            s.exec_op(&put("db", "s", k, "v")).unwrap();
        }
        let count = |st: &NativeIdbStore| {
            match st
                .exec_op(&IdbRecordOp::Count {
                    db_name: "db".into(),
                    store_name: "s".into(),
                    lower: None,
                    upper: None,
                })
                .unwrap()
            {
                IdbOpResult::Count(n) => n,
                other => panic!("expected Count, got {other:?}"),
            }
        };
        assert_eq!(count(&s), 3);

        s.exec_op(&IdbRecordOp::Delete {
            db_name: "db".into(),
            store_name: "s".into(),
            key_json: "\"a\"".into(),
        })
        .unwrap();
        assert_eq!(count(&s), 2);

        s.exec_op(&IdbRecordOp::DeleteRange {
            db_name: "db".into(),
            store_name: "s".into(),
            lower: Some("\"b\"".into()),
            upper: Some("\"b\"".into()),
        })
        .unwrap();
        assert_eq!(count(&s), 1);

        s.exec_op(&IdbRecordOp::Clear {
            db_name: "db".into(),
            store_name: "s".into(),
        })
        .unwrap();
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn native_commit_txn_atomic_batch() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        s.commit_txn(&[
            put("db", "s", "\"k1\"", "v1"),
            put("db", "s", "\"k2\"", "v2"),
            put("db", "s", "\"k3\"", "v3"),
        ])
        .unwrap();
        let n = match s
            .exec_op(&IdbRecordOp::Count {
                db_name: "db".into(),
                store_name: "s".into(),
                lower: None,
                upper: None,
            })
            .unwrap()
        {
            IdbOpResult::Count(n) => n,
            _ => panic!("expected Count"),
        };
        assert_eq!(n, 3);
    }

    #[test]
    fn native_delete_store_drops_records_and_indexes() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        s.apply_schema(&IdbSchemaOp::CreateStore {
            db_name: "db".into(),
            store_name: "s".into(),
            key_path: Some("id".into()),
            auto_increment: true,
        })
        .unwrap();
        s.apply_schema(&IdbSchemaOp::CreateIndex {
            db_name: "db".into(),
            store_name: "s".into(),
            index_name: "by_name".into(),
            key_path: "name".into(),
            unique: false,
            multi_entry: false,
        })
        .unwrap();
        s.exec_op(&put("db", "s", "\"k\"", "v")).unwrap();

        s.apply_schema(&IdbSchemaOp::DeleteStore {
            db_name: "db".into(),
            store_name: "s".into(),
        })
        .unwrap();

        let n = match s
            .exec_op(&IdbRecordOp::Count {
                db_name: "db".into(),
                store_name: "s".into(),
                lower: None,
                upper: None,
            })
            .unwrap()
        {
            IdbOpResult::Count(n) => n,
            _ => panic!("expected Count"),
        };
        assert_eq!(n, 0);
    }

    #[test]
    fn native_records_isolated_by_db_and_store() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        s.exec_op(&put("db1", "s", "\"k\"", "a")).unwrap();
        s.exec_op(&put("db2", "s", "\"k\"", "b")).unwrap();
        s.exec_op(&put("db1", "other", "\"k\"", "c")).unwrap();
        let read = |db: &str, store: &str| {
            match s
                .exec_op(&IdbRecordOp::Get {
                    db_name: db.into(),
                    store_name: store.into(),
                    key_json: "\"k\"".into(),
                })
                .unwrap()
            {
                IdbOpResult::Value(v) => v,
                _ => panic!("expected Value"),
            }
        };
        assert_eq!(read("db1", "s").as_deref(), Some("a"));
        assert_eq!(read("db2", "s").as_deref(), Some("b"));
        assert_eq!(read("db1", "other").as_deref(), Some("c"));
    }

    #[test]
    fn native_snapshot_load_save_fallback() {
        let s = NativeIdbStore::open_in_memory().unwrap();
        assert_eq!(s.load(), None);
        s.save(r#"{"db":{"version":1}}"#);
        assert_eq!(s.load().as_deref(), Some(r#"{"db":{"version":1}}"#));
        s.save("second");
        assert_eq!(s.load().as_deref(), Some("second"));
    }

    #[test]
    fn native_for_origin_persists_across_reopen() {
        let dir = tmp_idb_dir("native_for_origin");
        let key = origin_key("example.com");
        let path = dir.join(format!("{key}.db"));
        let _ = std::fs::remove_file(&path);
        {
            let store = NativeIdbStore::for_origin("example.com", &dir).unwrap();
            store
                .exec_op(&put("db", "s", "\"k\"", "persisted"))
                .unwrap();
        }
        // Reopen the same file → record survives.
        let reopened = NativeIdbStore::open_or_create(&path).unwrap();
        let got = reopened
            .exec_op(&IdbRecordOp::Get {
                db_name: "db".into(),
                store_name: "s".into(),
                key_json: "\"k\"".into(),
            })
            .unwrap();
        assert_eq!(got, IdbOpResult::Value(Some("persisted".to_owned())));
        let _ = std::fs::remove_file(&path);
    }
}

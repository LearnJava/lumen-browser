//! HSTS (HTTP Strict-Transport-Security) parser + per-host store.
//!
//! Spec: <https://datatracker.ietf.org/doc/html/rfc6797>. Сервер сообщает
//! `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload`,
//! и клиент в течение `max-age` секунд обязан обращаться к этому хосту
//! только по HTTPS (HTTP-запросы переадресуются на HTTPS).
//!
//! Слои: этот модуль — persistence (`HstsStore`, SQLite) + парсер заголовка;
//! `lumen-network::hsts` — клиентская логика (pre-request upgrade http→https,
//! разбор `Strict-Transport-Security` из ответа). Связывает их trait
//! [`HstsEnforcement`] (`lumen-core::ext`), который `HstsStore` реализует, а
//! `HttpClient::with_hsts` принимает.
//!
//! Точка подключения к реальному браузеру — [`shared_store`]: один store на
//! процесс, который шелл (`config::apply_http`) и драйвер (`build_http_client`)
//! ставят в каждый продакшн-`HttpClient`. До [BUG-402] такой точки не было и
//! весь модуль исполнялся только в тестах.
//!
//! [BUG-402]: https://github.com/LearnJava/lumen-browser/blob/main/bugs/BUG-402-FIXED.md

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::ext::HstsEnforcement;
use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HstsEntry {
    pub host: String,
    pub max_age_seconds: u64,
    pub include_subdomains: bool,
    pub preload: bool,
    /// Unix timestamp когда entry истечёт (`registered_at + max_age`).
    pub expires_at: i64,
}

/// Парсит Strict-Transport-Security header.
/// Возвращает (max_age, include_subdomains, preload). Невалидный header
/// (`max-age` отсутствует или не число) → None.
pub fn parse_sts_header(text: &str) -> Option<(u64, bool, bool)> {
    let mut max_age: Option<u64> = None;
    let mut include_subdomains = false;
    let mut preload = false;
    for piece in text.split(';') {
        let p = piece.trim();
        if p.is_empty() {
            continue;
        }
        if let Some(rest) = p.strip_prefix("max-age") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let v = rest.trim();
                // RFC 6797 §6.1.1: max-age может быть в кавычках. Снять их.
                let v = v.trim_matches('"');
                if let Ok(n) = v.parse::<u64>() {
                    max_age = Some(n);
                }
            }
        } else if p.eq_ignore_ascii_case("includeSubDomains") {
            include_subdomains = true;
        } else if p.eq_ignore_ascii_case("preload") {
            preload = true;
        }
    }
    max_age.map(|m| (m, include_subdomains, preload))
}

pub struct HstsStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for HstsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HstsStore").finish()
    }
}

impl HstsStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("hsts open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("hsts open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS hsts_hosts (
                host                TEXT PRIMARY KEY,
                max_age_seconds     INTEGER NOT NULL,
                include_subdomains  INTEGER NOT NULL DEFAULT 0,
                preload             INTEGER NOT NULL DEFAULT 0,
                expires_at          INTEGER NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS hsts_expires_idx ON hsts_hosts(expires_at);
            "#,
        )
        .map_err(|e| Error::Storage(format!("hsts init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Записать HSTS entry. `host` — lowercase ASCII hostname (без порта).
    /// `now_unix` — текущее время для вычисления `expires_at`.
    /// `max_age = 0` означает «снять HSTS» — удаляет entry.
    pub fn upsert(
        &self,
        host: &str,
        max_age: u64,
        include_subdomains: bool,
        preload: bool,
        now_unix: i64,
    ) -> Result<()> {
        if max_age == 0 {
            return self.delete(host);
        }
        let expires_at = now_unix.saturating_add(max_age as i64);
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO hsts_hosts (host, max_age_seconds, include_subdomains, preload, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (host) DO UPDATE SET
                 max_age_seconds = excluded.max_age_seconds,
                 include_subdomains = excluded.include_subdomains,
                 preload = excluded.preload,
                 expires_at = excluded.expires_at",
            params![
                host,
                max_age as i64,
                include_subdomains as i32,
                preload as i32,
                expires_at
            ],
        )
        .map_err(|e| Error::Storage(format!("hsts upsert: {e}")))?;
        Ok(())
    }

    /// Проверить, должен ли host обрабатываться как HTTPS-only.
    /// Учитывает `includeSubDomains` (если родительский домен помечен и
    /// `include_subdomains=true`, то и subdomain тоже HTTPS-only).
    /// `now_unix` нужен для отбрасывания просроченных entries.
    pub fn is_https_only(&self, host: &str, now_unix: i64) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        // Сначала точное совпадение.
        let exact = conn
            .query_row(
                "SELECT 1 FROM hsts_hosts WHERE host = ?1 AND expires_at > ?2",
                params![host, now_unix],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| Error::Storage(format!("hsts is_https_only-exact: {e}")))?
            .is_some();
        if exact {
            return Ok(true);
        }
        // Проверка subdomain: ищем родителей с include_subdomains=1.
        // Простой подход — итерируем по `host` отрезая ведущие labels.
        let mut h = host;
        while let Some(idx) = h.find('.') {
            h = &h[idx + 1..];
            if h.is_empty() {
                break;
            }
            let sub = conn
                .query_row(
                    "SELECT 1 FROM hsts_hosts
                     WHERE host = ?1 AND include_subdomains = 1 AND expires_at > ?2",
                    params![h, now_unix],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|e| Error::Storage(format!("hsts is_https_only-sub: {e}")))?
                .is_some();
            if sub {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn get(&self, host: &str) -> Result<Option<HstsEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        conn.query_row(
            "SELECT host, max_age_seconds, include_subdomains, preload, expires_at
             FROM hsts_hosts WHERE host = ?1",
            params![host],
            |r| {
                Ok(HstsEntry {
                    host: r.get(0)?,
                    max_age_seconds: r.get::<_, i64>(1)? as u64,
                    include_subdomains: r.get::<_, i32>(2)? != 0,
                    preload: r.get::<_, i32>(3)? != 0,
                    expires_at: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| Error::Storage(format!("hsts get: {e}")))
    }

    pub fn delete(&self, host: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        conn.execute("DELETE FROM hsts_hosts WHERE host = ?1", params![host])
            .map_err(|e| Error::Storage(format!("hsts delete: {e}")))?;
        Ok(())
    }

    /// Удалить все просроченные entries (для GC).
    pub fn purge_expired(&self, now_unix: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        let n = conn
            .execute(
                "DELETE FROM hsts_hosts WHERE expires_at <= ?1",
                params![now_unix],
            )
            .map_err(|e| Error::Storage(format!("hsts purge_expired: {e}")))?;
        Ok(n)
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("hsts mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM hsts_hosts", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("hsts count: {e}")))?;
        Ok(n)
    }
}

/// Адаптер `HstsStore` к `lumen-core::ext::HstsEnforcement` — позволяет
/// `lumen-network::HttpClient` принимать `Arc<dyn HstsEnforcement>` без
/// прямой зависимости на lumen-storage.
///
/// Fail-open: ошибки persistence (диск умер, mutex отравлен) логируются в
/// stderr и трактуются как «нет HSTS» (`is_https_only → false`) или
/// silent drop (`record_sts`). Принципы — в doc-комментарии trait-а.
impl HstsEnforcement for HstsStore {
    fn is_https_only(&self, host: &str, now_unix: i64) -> bool {
        match HstsStore::is_https_only(self, host, now_unix) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("HstsStore::is_https_only error: {e}; treating as not-HSTS");
                false
            }
        }
    }

    fn record_sts(
        &self,
        host: &str,
        max_age: u64,
        include_subdomains: bool,
        preload: bool,
        now_unix: i64,
    ) {
        if let Err(e) =
            self.upsert(host, max_age, include_subdomains, preload, now_unix)
        {
            eprintln!("HstsStore::record_sts error: {e}; ignored");
        }
    }
}

// ── Общий store процесса (точка подключения к браузеру, BUG-402) ─────────────

/// Портативный корень пользовательских данных браузера — `<exe_dir>/data`.
///
/// Дублирует правило `shell::adblock::browser_data_dir` (решение пользователя
/// 2026-06-16: все данные лежат в папке браузера, не в `%APPDATA%`/XDG), потому
/// что `lumen-storage` лежит ниже шелла в графе зависимостей и позвать его
/// хелпер не может. Тот же приём уже применён в
/// `lumen-paint::backend_probe` и `lumen-js::filesystem_access`.
fn browser_data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("data")))
        .unwrap_or_else(|| PathBuf::from("data"))
}

/// Путь к persistent-базе HSTS: `<exe_dir>/data/hsts/hsts.db`.
#[must_use]
pub fn default_db_path() -> PathBuf {
    browser_data_dir().join("hsts").join("hsts.db")
}

/// Текущее время в Unix-секундах (0, если системные часы до эпохи).
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Процесс-глобальный HSTS-store, общий для всех `HttpClient` процесса.
///
/// Инициализируется лениво при первом обращении; режим (`private`) фиксируется
/// первым вызовом на всё время жизни процесса — приватность выбирается на
/// старте и не меняется.
static SHARED: OnceLock<Option<Arc<HstsStore>>> = OnceLock::new();

/// Общий на процесс HSTS-store для подключения в `HttpClient::with_hsts`.
///
/// `private = true` (Tor / `no_persistent_state`) даёт in-memory store: HSTS,
/// выученный за сессию, действует до выхода и не пишется на диск. Preload-лист
/// (`lumen_network::get_preload_list`) работает в обоих режимах — он
/// консультируется внутри `maybe_upgrade_url_to_https`, а та вызывается только
/// когда store подключён, поэтому «нет store» = «нет и preload-защиты».
///
/// `None` означает, что HSTS отключён (не удалось открыть даже in-memory базу);
/// запросы тогда ведут себя как до [BUG-402] — без апгрейда http→https.
///
/// [BUG-402]: https://github.com/LearnJava/lumen-browser/blob/main/bugs/BUG-402-FIXED.md
#[must_use]
pub fn shared_store(private: bool) -> Option<Arc<dyn HstsEnforcement>> {
    SHARED
        .get_or_init(|| open_shared_store(private, &default_db_path()))
        .clone()
        .map(|s| s as Arc<dyn HstsEnforcement>)
}

/// Открыть store для запрошенного режима приватности (без глобального
/// состояния). Отделено от [`shared_store`], чтобы поведение можно было
/// проверить юнит-тестом, не замораживая процесс-глобальный `OnceLock`.
///
/// Деградация при ошибке диска — in-memory, а не «выключить»: preload-лист и
/// внутрисессионный HSTS ценнее, чем persistence, а тихо остаться без защиты
/// от downgrade-атаки — ровно тот дефект, который закрывает BUG-402.
/// Просроченные записи вычищаются здесь же (`purge_expired`) — единственная
/// точка, где store открывается, значит и естественная точка GC.
fn open_shared_store(private: bool, path: &Path) -> Option<Arc<HstsStore>> {
    let store = if private {
        HstsStore::open_in_memory().ok()
    } else {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match HstsStore::open(path) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!(
                    "hsts: cannot open {} ({e}); falling back to in-memory HSTS state",
                    path.display()
                );
                HstsStore::open_in_memory().ok()
            }
        }
    };
    let store = match store {
        Some(s) => s,
        None => {
            eprintln!("hsts: no store available; HTTPS upgrade enforcement disabled");
            return None;
        }
    };
    if let Err(e) = store.purge_expired(now_unix()) {
        eprintln!("hsts: purge_expired failed: {e}; continuing");
    }
    Some(Arc::new(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sts_basic() {
        let r = parse_sts_header("max-age=31536000; includeSubDomains; preload").unwrap();
        assert_eq!(r.0, 31_536_000);
        assert!(r.1);
        assert!(r.2);
    }

    #[test]
    fn parse_sts_without_optionals() {
        let r = parse_sts_header("max-age=3600").unwrap();
        assert_eq!(r, (3600, false, false));
    }

    #[test]
    fn parse_sts_quoted_max_age() {
        let r = parse_sts_header(r#"max-age="3600""#).unwrap();
        assert_eq!(r.0, 3600);
    }

    #[test]
    fn parse_sts_case_insensitive_directives() {
        let r = parse_sts_header("max-age=600; INCLUDESUBDOMAINS; Preload").unwrap();
        assert!(r.1);
        assert!(r.2);
    }

    #[test]
    fn parse_sts_no_max_age_returns_none() {
        assert!(parse_sts_header("includeSubDomains").is_none());
        assert!(parse_sts_header("").is_none());
    }

    #[test]
    fn upsert_and_get() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, true, false, 100).unwrap();
        let e = s.get("example.com").unwrap().unwrap();
        assert_eq!(e.max_age_seconds, 3600);
        assert!(e.include_subdomains);
        assert!(!e.preload);
        assert_eq!(e.expires_at, 3700);
    }

    #[test]
    fn upsert_with_zero_max_age_deletes() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        s.upsert("example.com", 0, false, false, 200).unwrap();
        assert!(s.get("example.com").unwrap().is_none());
    }

    #[test]
    fn is_https_only_exact_match() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        assert!(s.is_https_only("example.com", 200).unwrap());
        assert!(!s.is_https_only("other.com", 200).unwrap());
    }

    #[test]
    fn is_https_only_subdomain_match_when_include_set() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, true, false, 100).unwrap();
        // sub.example.com — родитель example.com помечен includeSubDomains.
        assert!(s.is_https_only("sub.example.com", 200).unwrap());
        assert!(s.is_https_only("deep.sub.example.com", 200).unwrap());
    }

    #[test]
    fn is_https_only_no_subdomain_match_when_include_not_set() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        assert!(!s.is_https_only("sub.example.com", 200).unwrap());
    }

    #[test]
    fn expired_entry_not_https_only() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("example.com", 100, false, false, 100).unwrap();
        // expires_at = 200; now=300 → expired.
        assert!(!s.is_https_only("example.com", 300).unwrap());
    }

    #[test]
    fn purge_expired_removes() {
        let s = HstsStore::open_in_memory().unwrap();
        s.upsert("a.com", 100, false, false, 100).unwrap(); // expires_at=200
        s.upsert("b.com", 1000, false, false, 100).unwrap(); // expires_at=1100
        let n = s.purge_expired(500).unwrap();
        assert_eq!(n, 1);
        assert!(s.get("a.com").unwrap().is_none());
        assert!(s.get("b.com").unwrap().is_some());
    }

    #[test]
    fn count_works() {
        let s = HstsStore::open_in_memory().unwrap();
        assert_eq!(s.count().unwrap(), 0);
        s.upsert("a.com", 3600, false, false, 100).unwrap();
        s.upsert("b.com", 3600, false, false, 100).unwrap();
        assert_eq!(s.count().unwrap(), 2);
    }

    // ── shared_store / open_shared_store (BUG-402) ───────────────────────────

    #[test]
    fn open_shared_store_private_is_in_memory() {
        // Приватный режим не должен трогать диск: путь заведомо несуществующий,
        // а store всё равно открывается.
        let path = PathBuf::from("/definitely/not/a/real/dir/hsts.db");
        let s = open_shared_store(true, &path).expect("in-memory store opens");
        assert!(!path.exists(), "private mode must not create the DB file");
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        assert!(s.is_https_only("example.com", 200).unwrap());
    }

    #[test]
    fn open_shared_store_persists_and_purges() {
        let dir = std::env::temp_dir().join(format!(
            "lumen_test_hsts_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = dir.join("hsts.db");
        let _ = std::fs::remove_dir_all(&dir);

        // Первое открытие создаёт директорию + файл и пишет две записи:
        // просроченную к «сейчас» и живую до 2100 года.
        {
            let s = open_shared_store(false, &path).expect("disk store opens");
            assert!(path.exists(), "disk mode must create the DB file");
            s.upsert("stale.example", 1, false, false, 100).unwrap();
            s.upsert("live.example", 4_000_000_000, false, false, 100)
                .unwrap();
            assert_eq!(s.count().unwrap(), 2);
        }

        // Второе открытие видит записи (persistence) и вычищает просроченную
        // (purge_expired на старте — BUG-402 п.3: у метода не было вызывающей
        // стороны вовсе).
        let s = open_shared_store(false, &path).expect("disk store reopens");
        assert!(s.get("stale.example").unwrap().is_none(), "expired entry purged on open");
        assert!(s.get("live.example").unwrap().is_some(), "live entry survives");

        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_shared_store_falls_back_to_memory_on_disk_error() {
        // Путь, который заведомо нельзя открыть как файл БД (родитель — файл,
        // а не директория). Store всё равно должен получиться: preload-лист и
        // внутрисессионный HSTS важнее persistence.
        let blocker = std::env::temp_dir().join(format!(
            "lumen_test_hsts_blocker_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&blocker, b"not a directory").unwrap();
        let path = blocker.join("hsts.db");
        let s = open_shared_store(false, &path);
        let _ = std::fs::remove_file(&blocker);
        let s = s.expect("falls back to in-memory instead of disabling HSTS");
        s.upsert("example.com", 3600, false, false, 100).unwrap();
        assert!(s.is_https_only("example.com", 200).unwrap());
    }

    #[test]
    fn default_db_path_lives_in_browser_data_dir() {
        let p = default_db_path();
        assert!(p.ends_with(PathBuf::from("data").join("hsts").join("hsts.db")), "{p:?}");
    }
}

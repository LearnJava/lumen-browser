//! Журнал локальных сигналов «сайт не работает» на диске (COMPAT-SIG-01).
//!
//! Нужен, чтобы окно «повторное выключение защиты за 7 суток» переживало
//! перезапуск, а потребители (чип помощи, COMPAT-SIG-04) видели прошлые
//! сигналы. Хранится только сайт (eTLD+1), код сигнала, момент и код причины
//! передачи — ни путей URL, ни текста пользователя. Хранилище не знает видов
//! сигналов: код приходит строкой от вызывающего. Ничего не отправляется.
//!
//! Две таблицы: `signals` — ограниченный журнал сильных сигналов,
//! `protection_off` — последний момент выключения защиты по сайту.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

use crate::migrations::{run_migrations, set_common_pragmas, Migration};

/// Сколько последних сигналов остаётся в журнале.
pub const SIGNAL_CAP: usize = 100;
/// Сколько живёт запись журнала, секунды (30 суток).
pub const SIGNAL_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
    CREATE TABLE IF NOT EXISTS signals (
        id     INTEGER PRIMARY KEY AUTOINCREMENT,
        kind   TEXT NOT NULL,
        site   TEXT NOT NULL,
        at     INTEGER NOT NULL,
        detail TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS protection_off (
        site TEXT PRIMARY KEY,
        at   INTEGER NOT NULL
    ) WITHOUT ROWID;
    "#,
}];

/// Запись журнала: код сигнала, сайт, момент (unix-секунды), уточнение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSignal {
    /// Стабильный код вида сигнала.
    pub kind: String,
    /// Сайт (eTLD+1).
    pub site: String,
    /// Unix-секунды.
    pub at: i64,
    /// Код причины передачи, иначе пусто.
    pub detail: String,
}

/// SQLite-хранилище журнала сигналов.
pub struct CompatSignalStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for CompatSignalStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompatSignalStore").finish()
    }
}

impl CompatSignalStore {
    /// Открыть (создать) базу по пути `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("compat_signals open: {e}")))?;
        Self::init(conn)
    }

    /// База в памяти (тесты, запасной вариант при ошибке диска).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("compat_signals open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        set_common_pragmas(&conn)
            .map_err(|e| Error::Storage(format!("compat_signals pragmas: {e}")))?;
        run_migrations(&mut conn, MIGRATIONS)
            .map_err(|e| Error::Storage(format!("compat_signals init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| Error::Storage("compat_signals mutex poisoned".into()))
    }

    /// Дописать сигнал; сверх [`SIGNAL_CAP`] старейшие удаляются.
    pub fn append(&self, signal: &StoredSignal) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO signals (kind, site, at, detail) VALUES (?1, ?2, ?3, ?4)",
            params![signal.kind, signal.site, signal.at, signal.detail],
        )
        .map_err(|e| Error::Storage(format!("compat_signals append: {e}")))?;
        conn.execute(
            "DELETE FROM signals WHERE id <= (SELECT MAX(id) FROM signals) - ?1",
            params![SIGNAL_CAP as i64],
        )
        .map_err(|e| Error::Storage(format!("compat_signals trim: {e}")))?;
        Ok(())
    }

    /// Журнал от старых записей к новым.
    pub fn load_signals(&self) -> Result<Vec<StoredSignal>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT kind, site, at, detail FROM signals ORDER BY id")
            .map_err(|e| Error::Storage(format!("compat_signals load prepare: {e}")))?;
        stmt.query_map([], |r| {
            Ok(StoredSignal { kind: r.get(0)?, site: r.get(1)?, at: r.get(2)?, detail: r.get(3)? })
        })
        .and_then(Iterator::collect)
        .map_err(|e| Error::Storage(format!("compat_signals load: {e}")))
    }

    /// Запомнить момент выключения защиты на сайте `site`.
    pub fn set_protection_off(&self, site: &str, at: i64) -> Result<()> {
        self.lock()?
            .execute(
                "INSERT OR REPLACE INTO protection_off (site, at) VALUES (?1, ?2)",
                params![site, at],
            )
            .map_err(|e| Error::Storage(format!("compat_signals set_protection_off: {e}")))?;
        Ok(())
    }

    /// Сайт → последний момент выключения защиты.
    pub fn load_protection_off(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT site, at FROM protection_off ORDER BY site")
            .map_err(|e| Error::Storage(format!("compat_signals protection prepare: {e}")))?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .and_then(Iterator::collect)
            .map_err(|e| Error::Storage(format!("compat_signals protection: {e}")))
    }

    /// Удалить устаревшее на момент `now_unix`: журнал — старше
    /// [`SIGNAL_RETENTION_SECS`], моменты выключения защиты — старше
    /// `protection_window_secs` (окно повтора, дальше они ничего не решают).
    pub fn purge(&self, now_unix: i64, protection_window_secs: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM signals WHERE at < ?1",
            params![now_unix.saturating_sub(SIGNAL_RETENTION_SECS)],
        )
        .map_err(|e| Error::Storage(format!("compat_signals purge signals: {e}")))?;
        conn.execute(
            "DELETE FROM protection_off WHERE at < ?1",
            params![now_unix.saturating_sub(protection_window_secs)],
        )
        .map_err(|e| Error::Storage(format!("compat_signals purge protection: {e}")))?;
        Ok(())
    }

    /// Стереть всё, что известно о сайте `site` (eTLD+1): записи журнала и
    /// момент выключения защиты («очистить данные сайта»). Возвращает число
    /// удалённых записей журнала.
    pub fn clear_site(&self, site: &str) -> Result<usize> {
        let conn = self.lock()?;
        let removed = conn
            .execute("DELETE FROM signals WHERE site = ?1", params![site])
            .map_err(|e| Error::Storage(format!("compat_signals clear_site signals: {e}")))?;
        conn.execute("DELETE FROM protection_off WHERE site = ?1", params![site])
            .map_err(|e| Error::Storage(format!("compat_signals clear_site protection: {e}")))?;
        Ok(removed)
    }

    /// Стереть всё («очистить данные просмотра»).
    pub fn clear(&self) -> Result<()> {
        self.lock()?
            .execute_batch("DELETE FROM signals; DELETE FROM protection_off;")
            .map_err(|e| Error::Storage(format!("compat_signals clear: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn sig(kind: &str, site: &str, at: i64) -> StoredSignal {
        StoredSignal { kind: kind.into(), site: site.into(), at, detail: String::new() }
    }

    #[test]
    fn signals_round_trip_in_order() {
        let s = CompatSignalStore::open_in_memory().unwrap();
        s.append(&sig("reload3", "a.com", T0)).unwrap();
        s.append(&StoredSignal { detail: "drm".into(), ..sig("open_in_other", "b.com", T0 + 1) })
            .unwrap();
        let all = s.load_signals().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!((all[0].kind.as_str(), all[0].site.as_str()), ("reload3", "a.com"));
        assert_eq!(all[1].detail, "drm");
    }

    #[test]
    fn clear_site_drops_only_that_site() {
        let s = CompatSignalStore::open_in_memory().unwrap();
        s.append(&sig("reload3", "a.com", T0)).unwrap();
        s.append(&sig("reload3", "b.com", T0 + 1)).unwrap();
        s.set_protection_off("a.com", T0).unwrap();
        s.set_protection_off("b.com", T0).unwrap();
        assert_eq!(s.clear_site("a.com").unwrap(), 1);
        let all = s.load_signals().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].site, "b.com");
        assert_eq!(s.load_protection_off().unwrap(), vec![("b.com".to_owned(), T0)]);
    }

    #[test]
    fn clear_drops_journal_and_protection_moments() {
        let s = CompatSignalStore::open_in_memory().unwrap();
        s.append(&sig("reload3", "a.com", T0)).unwrap();
        s.set_protection_off("a.com", T0).unwrap();
        s.clear().unwrap();
        assert!(s.load_signals().unwrap().is_empty());
        assert!(s.load_protection_off().unwrap().is_empty());
    }

    #[test]
    fn journal_keeps_only_the_last_cap_entries() {
        let s = CompatSignalStore::open_in_memory().unwrap();
        for i in 0..(SIGNAL_CAP as i64 + 7) {
            s.append(&sig("reload3", "a.com", T0 + i)).unwrap();
        }
        let all = s.load_signals().unwrap();
        assert_eq!(all.len(), SIGNAL_CAP);
        assert_eq!(all[0].at, T0 + 7);
    }

    #[test]
    fn protection_off_is_replaced_per_site() {
        let s = CompatSignalStore::open_in_memory().unwrap();
        s.set_protection_off("a.com", T0).unwrap();
        s.set_protection_off("a.com", T0 + DAY).unwrap();
        s.set_protection_off("b.com", T0).unwrap();
        assert_eq!(
            s.load_protection_off().unwrap(),
            [("a.com".to_owned(), T0 + DAY), ("b.com".to_owned(), T0)]
        );
    }

    #[test]
    fn purge_drops_stale_rows_and_clear_drops_all() {
        let s = CompatSignalStore::open_in_memory().unwrap();
        s.append(&sig("reload3", "old.com", T0)).unwrap();
        s.append(&sig("reload3", "new.com", T0 + 29 * DAY)).unwrap();
        s.set_protection_off("old.com", T0).unwrap();
        s.set_protection_off("new.com", T0 + 29 * DAY).unwrap();
        s.purge(T0 + 31 * DAY, 7 * DAY).unwrap();
        let left: Vec<_> = s.load_signals().unwrap().into_iter().map(|x| x.site).collect();
        assert_eq!(left, ["new.com"]);
        assert_eq!(s.load_protection_off().unwrap().len(), 1);
        s.clear().unwrap();
        assert!(s.load_signals().unwrap().is_empty());
        assert!(s.load_protection_off().unwrap().is_empty());
    }

    #[test]
    fn data_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("lumen-compat-signals-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("compat_signals.db");
        {
            let s = CompatSignalStore::open(&path).unwrap();
            s.append(&sig("reload3", "a.com", T0)).unwrap();
            s.set_protection_off("a.com", T0).unwrap();
        }
        let s = CompatSignalStore::open(&path).unwrap();
        assert_eq!(s.load_signals().unwrap().len(), 1);
        assert_eq!(s.load_protection_off().unwrap().len(), 1);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

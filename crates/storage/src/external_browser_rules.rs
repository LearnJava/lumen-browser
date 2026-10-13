//! Правила «открывать этот сайт в другом браузере» (COMPAT-HO-04, COMPAT-HO-05).
//!
//! Правило привязано к сайту (eTLD+1), живёт ровно [`RULE_TTL_SECS`] (30 дней),
//! срабатывает только по клику пользователя на ссылку — это решает вызывающий
//! код, хранилище лишь отвечает «есть ли живое правило». Бессрочных правил нет:
//! [`ExternalBrowserRules::grant`] всегда ставит срок, [`ExternalBrowserRules::renew`]
//! продлевает на те же 30 дней от «сейчас» (кнопка «Проверить снова в Lumen?»
//! после обновления — это [`ExternalBrowserRule::needs_recheck`], а не молчаливое
//! продление).
//!
//! В правиле нет ни cookie, ни пути URL: только сайт, идентификатор браузера
//! (имя ключа реестра `StartMenuInternet`), код причины и версия Lumen на момент
//! создания. Эти же строки — список «Сайты, открытые в другом браузере».

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::migrations::{run_migrations, set_common_pragmas, Migration};

/// Срок жизни правила: 30 дней (COMPAT-HO-04).
pub const RULE_TTL_SECS: i64 = 30 * 24 * 60 * 60;

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
    CREATE TABLE IF NOT EXISTS external_browser_rules (
        site          TEXT PRIMARY KEY,
        browser_id    TEXT NOT NULL,
        reason        TEXT NOT NULL,
        lumen_version TEXT NOT NULL,
        created_at    INTEGER NOT NULL,
        expires_at    INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS ebr_expires_idx ON external_browser_rules(expires_at);
    "#,
}];

/// Живое (или только что истёкшее) правило передачи сайта во внешний браузер.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalBrowserRule {
    /// Сайт (eTLD+1, для IP и `localhost` — сам хост), нижний регистр.
    pub site: String,
    /// Идентификатор целевого браузера (ключ `StartMenuInternet`).
    pub browser_id: String,
    /// Код причины (`HandoffReason::code`), без текста пользователя.
    pub reason: String,
    /// Версия Lumen (`CARGO_PKG_VERSION`), при которой правило создано.
    pub lumen_version: String,
    /// Unix-секунды создания (или последнего продления).
    pub created_at: i64,
    /// Unix-секунды окончания действия.
    pub expires_at: i64,
}

impl ExternalBrowserRule {
    /// `true`, если срок правила вышел к моменту `now_unix`.
    #[must_use]
    pub fn is_expired(&self, now_unix: i64) -> bool {
        self.expires_at <= now_unix
    }

    /// `true`, если Lumen обновился с момента создания правила: пора спросить
    /// «Проверить снова в Lumen?» (COMPAT-HO-04).
    #[must_use]
    pub fn needs_recheck(&self, current_version: &str) -> bool {
        self.lumen_version != current_version
    }
}

/// `true`, если два URL/хоста принадлежат одному сайту (eTLD+1) — тот же ключ,
/// по которому хранятся правила. Ссылка внутри сайта не перехватывается.
#[must_use]
pub fn same_site(a: &str, b: &str) -> bool {
    crate::partition::site_of(a) == crate::partition::site_of(b)
}

/// SQLite-хранилище правил внешнего браузера.
pub struct ExternalBrowserRules {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for ExternalBrowserRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalBrowserRules").finish()
    }
}

fn row_to_rule(r: &rusqlite::Row<'_>) -> rusqlite::Result<ExternalBrowserRule> {
    Ok(ExternalBrowserRule {
        site: r.get(0)?,
        browser_id: r.get(1)?,
        reason: r.get(2)?,
        lumen_version: r.get(3)?,
        created_at: r.get(4)?,
        expires_at: r.get(5)?,
    })
}

const SELECT: &str =
    "SELECT site, browser_id, reason, lumen_version, created_at, expires_at FROM external_browser_rules";

impl ExternalBrowserRules {
    /// Открыть (создать) базу по пути `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("external_browser_rules open: {e}")))?;
        Self::init(conn)
    }

    /// База в памяти (тесты, приватные окна).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("external_browser_rules open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        set_common_pragmas(&conn)
            .map_err(|e| Error::Storage(format!("external_browser_rules pragmas: {e}")))?;
        run_migrations(&mut conn, MIGRATIONS)
            .map_err(|e| Error::Storage(format!("external_browser_rules init: {e}")))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| Error::Storage("external_browser_rules mutex poisoned".into()))
    }

    /// Создать правило для сайта хоста/URL `host_or_url` на 30 дней от `now_unix`;
    /// существующее правило этого сайта заменяется.
    pub fn grant(
        &self,
        host_or_url: &str,
        browser_id: &str,
        reason: &str,
        lumen_version: &str,
        now_unix: i64,
    ) -> Result<ExternalBrowserRule> {
        let rule = ExternalBrowserRule {
            site: crate::partition::site_of(host_or_url),
            browser_id: browser_id.to_owned(),
            reason: reason.to_owned(),
            lumen_version: lumen_version.to_owned(),
            created_at: now_unix,
            expires_at: now_unix.saturating_add(RULE_TTL_SECS),
        };
        self.lock()?
            .execute(
                "INSERT OR REPLACE INTO external_browser_rules
                 (site, browser_id, reason, lumen_version, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    rule.site,
                    rule.browser_id,
                    rule.reason,
                    rule.lumen_version,
                    rule.created_at,
                    rule.expires_at
                ],
            )
            .map_err(|e| Error::Storage(format!("external_browser_rules grant: {e}")))?;
        Ok(rule)
    }

    /// Живое правило сайта хоста/URL `host_or_url`; истёкшее не возвращается.
    pub fn active_for(
        &self,
        host_or_url: &str,
        now_unix: i64,
    ) -> Result<Option<ExternalBrowserRule>> {
        let site = crate::partition::site_of(host_or_url);
        self.lock()?
            .query_row(
                &format!("{SELECT} WHERE site = ?1 AND expires_at > ?2"),
                params![site, now_unix],
                row_to_rule,
            )
            .optional()
            .map_err(|e| Error::Storage(format!("external_browser_rules active_for: {e}")))
    }

    /// Все живые правила — список «Сайты, открытые в другом браузере»,
    /// сначала ближайшие к истечению.
    pub fn list_active(&self, now_unix: i64) -> Result<Vec<ExternalBrowserRule>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(&format!("{SELECT} WHERE expires_at > ?1 ORDER BY expires_at, site"))
            .map_err(|e| Error::Storage(format!("external_browser_rules list prepare: {e}")))?;
        stmt.query_map(params![now_unix], row_to_rule)
            .and_then(Iterator::collect)
            .map_err(|e| Error::Storage(format!("external_browser_rules list: {e}")))
    }

    /// Продлить правило на 30 дней от `now_unix` и перезаписать версию Lumen
    /// (ответ «Нет, оставить там» на перепроверку). `false` — правила нет.
    pub fn renew(&self, host_or_url: &str, lumen_version: &str, now_unix: i64) -> Result<bool> {
        let site = crate::partition::site_of(host_or_url);
        let n = self
            .lock()?
            .execute(
                "UPDATE external_browser_rules
                 SET created_at = ?2, expires_at = ?3, lumen_version = ?4 WHERE site = ?1",
                params![site, now_unix, now_unix.saturating_add(RULE_TTL_SECS), lumen_version],
            )
            .map_err(|e| Error::Storage(format!("external_browser_rules renew: {e}")))?;
        Ok(n > 0)
    }

    /// Снять правило сайта (кнопка «Проверить снова в Lumen» / «Убрать»).
    pub fn remove(&self, host_or_url: &str) -> Result<bool> {
        let site = crate::partition::site_of(host_or_url);
        let n = self
            .lock()?
            .execute("DELETE FROM external_browser_rules WHERE site = ?1", params![site])
            .map_err(|e| Error::Storage(format!("external_browser_rules remove: {e}")))?;
        Ok(n > 0)
    }

    /// Удалить истёкшие правила; возвращает их число.
    pub fn purge_expired(&self, now_unix: i64) -> Result<usize> {
        self.lock()?
            .execute("DELETE FROM external_browser_rules WHERE expires_at <= ?1", params![now_unix])
            .map_err(|e| Error::Storage(format!("external_browser_rules purge: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn store() -> ExternalBrowserRules {
        ExternalBrowserRules::open_in_memory().unwrap()
    }

    #[test]
    fn same_site_compares_registrable_domains() {
        assert!(same_site("https://www.example.com/a", "https://login.example.com/b"));
        assert!(!same_site("https://example.com/", "https://example.org/"));
    }

    #[test]
    fn rule_is_per_site_and_matches_subdomains() {
        let s = store();
        s.grant("https://www.example.com/a?x=1", "Yandex", "gost", "0.1.0", T0).unwrap();
        let r = s.active_for("login.example.com", T0 + 1).unwrap().unwrap();
        assert_eq!(r.site, "example.com");
        assert_eq!(r.browser_id, "Yandex");
        assert!(s.active_for("example.org", T0 + 1).unwrap().is_none());
    }

    #[test]
    fn rule_expires_on_day_31_not_day_29() {
        let s = store();
        s.grant("example.com", "Chrome", "drm", "0.1.0", T0).unwrap();
        assert!(s.active_for("example.com", T0 + 29 * DAY).unwrap().is_some());
        assert!(s.active_for("example.com", T0 + 30 * DAY).unwrap().is_none());
        assert!(s.active_for("example.com", T0 + 31 * DAY).unwrap().is_none());
        assert!(s.list_active(T0 + 31 * DAY).unwrap().is_empty());
        assert_eq!(s.purge_expired(T0 + 31 * DAY).unwrap(), 1);
    }

    #[test]
    fn regrant_replaces_and_renew_extends_from_now() {
        let s = store();
        s.grant("example.com", "Chrome", "other", "0.1.0", T0).unwrap();
        s.grant("example.com", "Edge", "drm", "0.1.0", T0 + DAY).unwrap();
        let list = s.list_active(T0 + DAY).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].browser_id, "Edge");
        assert!(s.renew("example.com", "0.2.0", T0 + 20 * DAY).unwrap());
        let r = s.active_for("example.com", T0 + 49 * DAY).unwrap().unwrap();
        assert_eq!(r.expires_at, T0 + 50 * DAY);
        assert!(!r.needs_recheck("0.2.0"));
        assert!(!s.renew("other.com", "0.2.0", T0).unwrap());
    }

    #[test]
    fn upgrade_asks_for_recheck_and_remove_clears() {
        let s = store();
        let r = s.grant("example.com", "Chrome", "calls", "0.1.0", T0).unwrap();
        assert!(r.needs_recheck("0.1.1"));
        assert!(!r.needs_recheck("0.1.0"));
        assert!(s.remove("example.com").unwrap());
        assert!(!s.remove("example.com").unwrap());
        assert!(s.active_for("example.com", T0).unwrap().is_none());
    }

    #[test]
    fn list_orders_by_expiry() {
        let s = store();
        s.grant("b.com", "Chrome", "other", "v", T0 + DAY).unwrap();
        s.grant("a.com", "Chrome", "other", "v", T0).unwrap();
        let sites: Vec<_> = s.list_active(T0).unwrap().into_iter().map(|r| r.site).collect();
        assert_eq!(sites, ["a.com", "b.com"]);
    }
}

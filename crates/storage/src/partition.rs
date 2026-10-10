//! Ключ разделения хранилищ по сайту (ADR-012) и единая очистка данных сайта.
//!
//! [`PartitionKey`] — top-level site (eTLD+1 по PSL, либо сам хост для IP,
//! `localhost` и голых public suffix-ов). Хранилища держат сайт в разных видах:
//! origin (`https://a.example.com:8443`), URL, голый хост, cookie-domain
//! (`.example.com`). [`PartitionKey::matches`] принимает любой из них.
//!
//! [`clear_site_data`] обходит переданные хранилища и удаляет всё, что
//! относится к одному сайту. У каждого хранилища свой `clear_site`: либо выбирает
//! ключи и удаляет совпавшие, либо (cache_storage, service_workers,
//! notifications, site_engagement) удаляет по колонке `site`.

use std::path::Path;
use std::sync::Mutex;

use lumen_core::ext::PublicSuffixList;
use lumen_core::{Error, Result};
use rusqlite::{params, Connection};

use crate::psl::PslProvider;

/// Top-level site, по которому разделяются и чистятся данные.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartitionKey {
    site: String,
}

impl PartitionKey {
    /// Ключ по origin, URL, `host:port` или голому хосту. `None`, если хоста нет.
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let host = host_of(input)?;
        Some(Self { site: site_of_host(&host) })
    }

    /// Строка сайта (`example.co.uk`), в том же виде, что у `origin_key` IDB.
    #[must_use]
    pub fn site(&self) -> &str {
        &self.site
    }

    /// Относится ли origin / URL / хост / cookie-domain к этому сайту.
    #[must_use]
    pub fn matches(&self, input: &str) -> bool {
        host_of(input).is_some_and(|h| site_of_host(&h) == self.site)
    }
}

/// Хост из origin / URL / `host:port` / cookie-domain, в нижнем регистре,
/// без userinfo, порта и ведущей точки.
fn host_of(input: &str) -> Option<String> {
    let rest = input.split_once("://").map_or(input, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host = host.trim_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn site_of_host(host: &str) -> String {
    let is_ip = host.contains(':') || host.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    if is_ip {
        return host.to_owned();
    }
    PslProvider::new()
        .registrable_domain(host)
        .unwrap_or(host)
        .to_owned()
}

/// Значение колонки `site` для origin / URL / хоста: top-level site, а при
/// отсутствии хоста — сама строка в нижнем регистре (чтобы ключ был непустым).
pub(crate) fn site_of(input: &str) -> String {
    PartitionKey::parse(input).map_or_else(|| input.to_ascii_lowercase(), |k| k.site)
}

/// Заполнить колонку `site` у строк, созданных до миграции (`site = ''`).
pub(crate) fn backfill_site(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("SELECT DISTINCT {column} FROM {table} WHERE site = ''"))?;
    let keys = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for key in keys {
        conn.execute(
            &format!("UPDATE {table} SET site = ?1 WHERE {column} = ?2 AND site = ''"),
            params![site_of(&key), key],
        )?;
    }
    Ok(())
}

/// Удалить из `table` строки с `site = site.site()` одним запросом по индексу.
pub(crate) fn clear_by_site(
    conn: &Mutex<Connection>,
    store: &str,
    table: &str,
    site: &PartitionKey,
) -> Result<usize> {
    let conn = conn
        .lock()
        .map_err(|_| Error::Storage(format!("{store} mutex poisoned")))?;
    conn.execute(&format!("DELETE FROM {table} WHERE site = ?1"), params![site.site()])
        .map_err(|e| Error::Storage(format!("{store} clear_site delete: {e}")))
}

/// Удалить из `table` строки, у которых `column` относится к `site`.
/// Возвращает число удалённых строк.
pub(crate) fn clear_column(
    conn: &Mutex<Connection>,
    store: &str,
    table: &str,
    column: &str,
    site: &PartitionKey,
) -> Result<usize> {
    let err = |what: &str, e: rusqlite::Error| {
        Error::Storage(format!("{store} clear_site {what}: {e}"))
    };
    let conn = conn
        .lock()
        .map_err(|_| Error::Storage(format!("{store} mutex poisoned")))?;
    let mut stmt = conn
        .prepare(&format!("SELECT DISTINCT {column} FROM {table}"))
        .map_err(|e| err("select", e))?;
    let keys: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| err("select", e))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| err("select", e))?;
    drop(stmt);
    let mut removed = 0;
    for key in keys.iter().filter(|k| site.matches(k)) {
        removed += conn
            .execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), params![key])
            .map_err(|e| err("delete", e))?;
    }
    Ok(removed)
}

/// Хранилища, из которых чистятся данные сайта. Не заданные (`None`) пропускаются.
#[derive(Default)]
pub struct SiteDataTargets<'a> {
    /// Cookies (по domain и по top-level site раздела).
    pub cookies: Option<&'a crate::CookieJar>,
    /// HTTP-кэш (по top-level site раздела и по хосту URL).
    pub http_cache: Option<&'a crate::HttpCache>,
    /// Разрешения.
    pub permissions: Option<&'a crate::Permissions>,
    /// HSTS-записи.
    pub hsts: Option<&'a crate::HstsStore>,
    /// Вовлечённость.
    pub site_engagement: Option<&'a crate::SiteEngagementStore>,
    /// Cache Storage API.
    pub cache_storage: Option<&'a crate::CacheStorage>,
    /// Регистрации service worker-ов.
    pub service_workers: Option<&'a crate::ServiceWorkers>,
    /// Push-подписки.
    pub push_subscriptions: Option<&'a crate::PushSubscriptions>,
    /// Уведомления.
    pub notifications: Option<&'a crate::Notifications>,
    /// Автозаполнение форм (не пароли).
    pub autofill: Option<&'a crate::Autofill>,
    /// CSP-политики.
    pub csp_policies: Option<&'a crate::CspPolicies>,
    /// Referrer-Policy.
    pub referrer_policies: Option<&'a crate::ReferrerPolicies>,
    /// Permissions-Policy.
    pub permissions_policies: Option<&'a crate::PermissionsPolicies>,
    /// Web App Manifest.
    pub web_manifests: Option<&'a crate::WebManifests>,
    /// DNS-кэш (по хосту).
    pub dns_cache: Option<&'a crate::DnsCache>,
    /// Регистрации BroadcastChannel.
    pub broadcast_channels: Option<&'a crate::BroadcastChannels>,
    /// Каталог файлов IndexedDB (`<data>/idb`).
    pub idb_dir: Option<&'a Path>,
}

/// Итог очистки: сколько записей удалено из каждого хранилища и какие упали.
#[derive(Debug, Default)]
pub struct SiteDataReport {
    /// `(хранилище, число удалённых записей)`.
    pub removed: Vec<(&'static str, usize)>,
    /// `(хранилище, текст ошибки)`; остальные хранилища очищены.
    pub failed: Vec<(&'static str, String)>,
}

impl SiteDataReport {
    /// Всего удалённых записей.
    #[must_use]
    pub fn total(&self) -> usize {
        self.removed.iter().map(|(_, n)| n).sum()
    }
}

/// Удалить все данные `site` из переданных хранилищ. Ошибка одного хранилища
/// не прерывает остальные — она попадает в [`SiteDataReport::failed`].
pub fn clear_site_data(site: &PartitionKey, t: &SiteDataTargets<'_>) -> SiteDataReport {
    let mut report = SiteDataReport::default();
    let mut put = |name: &'static str, r: Result<usize>| match r {
        Ok(n) => report.removed.push((name, n)),
        Err(e) => report.failed.push((name, e.to_string())),
    };
    if let Some(s) = t.cookies {
        put("cookies", s.clear_site(site));
    }
    if let Some(s) = t.http_cache {
        put("http_cache", s.clear_site(site));
    }
    if let Some(s) = t.permissions {
        put("permissions", s.clear_site(site));
    }
    if let Some(s) = t.hsts {
        put("hsts", s.clear_site(site));
    }
    if let Some(s) = t.site_engagement {
        put("site_engagement", s.clear_site(site));
    }
    if let Some(s) = t.cache_storage {
        put("cache_storage", s.clear_site(site));
    }
    if let Some(s) = t.service_workers {
        put("service_workers", s.clear_site(site));
    }
    if let Some(s) = t.push_subscriptions {
        put("push_subscriptions", s.clear_site(site));
    }
    if let Some(s) = t.notifications {
        put("notifications", s.clear_site(site));
    }
    if let Some(s) = t.autofill {
        put("autofill", s.clear_site(site));
    }
    if let Some(s) = t.csp_policies {
        put("csp_policies", s.clear_site(site));
    }
    if let Some(s) = t.referrer_policies {
        put("referrer_policy", s.clear_site(site));
    }
    if let Some(s) = t.permissions_policies {
        put("permissions_policy", s.clear_site(site));
    }
    if let Some(s) = t.web_manifests {
        put("web_manifest", s.clear_site(site));
    }
    if let Some(s) = t.dns_cache {
        put("dns_cache", s.clear_site(site));
    }
    if let Some(s) = t.broadcast_channels {
        put("broadcast_channels", s.clear_site(site));
    }
    if let Some(dir) = t.idb_dir {
        put("indexed_db", clear_idb_files(dir, site));
    }
    report
}

/// Удалить файл IndexedDB сайта (`<origin_key>.db` и WAL/SHM-спутники).
///
/// Если файл держит открытая страница (на Windows SQLite не даёт его удалить),
/// файл остаётся, а все таблицы `idb_*` опустошаются через второе соединение.
fn clear_idb_files(dir: &Path, site: &PartitionKey) -> Result<usize> {
    let base = format!("{}.db", crate::indexed_db::origin_key(site.site()));
    let db_path = dir.join(&base);
    match std::fs::remove_file(&db_path) {
        Ok(()) => {
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(dir.join(format!("{base}{suffix}")));
            }
            Ok(1)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(_) => wipe_idb_rows(&db_path).map(|()| 1),
    }
}

/// Опустошить таблицы IndexedDB в файле, открытом другим соединением.
fn wipe_idb_rows(path: &Path) -> Result<()> {
    let conn = Connection::open(path)
        .map_err(|e| Error::Storage(format!("idb wipe open {}: {e}", path.display())))?;
    conn.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| Error::Storage(format!("idb wipe busy: {e}")))?;
    conn.execute_batch(
        "DELETE FROM idb_records; DELETE FROM idb_indexes; DELETE FROM idb_stores;
         DELETE FROM idb_meta; DELETE FROM idb_snapshot;",
    )
    .map_err(|e| Error::Storage(format!("idb wipe {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn key(s: &str) -> PartitionKey {
        PartitionKey::parse(s).expect("host")
    }

    #[test]
    fn site_is_registrable_domain() {
        assert_eq!(key("https://a.b.example.com:8443/x?y#z").site(), "example.com");
        assert_eq!(key("https://shop.example.co.uk").site(), "example.co.uk");
        assert_eq!(key(".Example.COM").site(), "example.com");
        assert_eq!(key("user:pw@mail.example.com:25").site(), "example.com");
    }

    #[test]
    fn ip_localhost_and_suffix_keep_host() {
        assert_eq!(key("http://127.0.0.1:3000").site(), "127.0.0.1");
        assert_eq!(key("http://[::1]:80/").site(), "::1");
        assert_eq!(key("http://localhost:9000").site(), "localhost");
        assert_eq!(key("co.uk").site(), "co.uk");
    }

    #[test]
    fn matches_any_representation() {
        let k = key("example.com");
        assert!(k.matches("https://www.example.com"));
        assert!(k.matches("https://example.com/a/b?c"));
        assert!(k.matches(".cdn.example.com"));
        assert!(k.matches("api.example.com"));
        assert!(!k.matches("https://evil-example.com"));
        assert!(!k.matches("https://example.com.evil.org"));
        assert!(!k.matches(""));
    }

    #[test]
    fn empty_input_has_no_key() {
        assert!(PartitionKey::parse("").is_none());
        assert!(PartitionKey::parse("https:///path").is_none());
    }

    #[test]
    fn v1_database_gets_site_column_backfilled() {
        let path = std::env::temp_dir()
            .join(format!("lumen-partition-mig-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE site_engagement (
                    origin TEXT PRIMARY KEY, visit_count INTEGER NOT NULL DEFAULT 0,
                    total_time_seconds INTEGER NOT NULL DEFAULT 0,
                    last_visit INTEGER NOT NULL, first_visit INTEGER NOT NULL
                 ) WITHOUT ROWID;
                 INSERT INTO site_engagement (origin, visit_count, last_visit, first_visit) VALUES
                    ('https://a.example.com', 1, 1, 1), ('https://example.com', 1, 1, 1),
                    ('https://other.org', 1, 1, 1);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let store = crate::SiteEngagementStore::open(&path).unwrap();
        assert_eq!(store.clear_site(&key("example.com")).unwrap(), 2);
        assert_eq!(store.count().unwrap(), 1);
        store.record_visit("https://x.example.com", 5).unwrap();
        assert_eq!(store.clear_site(&key("example.com")).unwrap(), 1);
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn clear_site_data_removes_only_that_site() {
        use crate::{CookieJar, Cookie, HstsStore, PermissionKind, PermissionState, Permissions, SameSite};
        let jar = CookieJar::open_in_memory().unwrap();
        let mk = |domain: &str| Cookie {
            domain: domain.into(),
            path: "/".into(),
            name: "a".into(),
            value: "1".into(),
            expires_at: None,
            secure: false,
            http_only: false,
            same_site: SameSite::Lax,
        };
        jar.set(mk("www.example.com"), None).unwrap();
        jar.set(mk("evil-example.com"), None).unwrap();
        // чужой cookie, partitioned под example.com
        jar.set(mk("tracker.net"), Some("example.com")).unwrap();
        jar.set(mk("tracker.net"), Some("other.org")).unwrap();
        let perms = Permissions::open_in_memory().unwrap();
        for o in ["https://example.com", "https://a.example.com:8443", "https://other.org"] {
            perms.set(o, &PermissionKind::Geolocation, PermissionState::Granted, None).unwrap();
        }
        let hsts = HstsStore::open_in_memory().unwrap();
        hsts.upsert("sub.example.com", 100, false, false, 0).unwrap();
        hsts.upsert("other.org", 100, false, false, 0).unwrap();

        let dir = std::env::temp_dir().join(format!("lumen-partition-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let k = key("example.com");
        let idb = dir.join(format!("{}.db", crate::indexed_db::origin_key(k.site())));
        let other_idb = dir.join(format!("{}.db", crate::indexed_db::origin_key("other.org")));
        std::fs::write(&idb, b"x").unwrap();
        std::fs::write(&other_idb, b"x").unwrap();

        let targets = SiteDataTargets {
            cookies: Some(&jar),
            permissions: Some(&perms),
            hsts: Some(&hsts),
            idb_dir: Some(&dir),
            ..SiteDataTargets::default()
        };
        let report = clear_site_data(&k, &targets);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        let n = |name: &str| report.removed.iter().find(|(s, _)| *s == name).map(|(_, n)| *n);
        assert_eq!(n("cookies"), Some(2));
        assert_eq!(n("permissions"), Some(2));
        assert_eq!(n("hsts"), Some(1));
        assert_eq!(n("indexed_db"), Some(1));
        assert!(!idb.exists());
        assert!(other_idb.exists());
        assert_eq!(hsts.count().unwrap(), 1);
        let left = jar.get_for_request("evil-example.com", "/", false, 0, None).unwrap();
        assert_eq!(left.len(), 1);
        let left = jar.get_for_request("tracker.net", "/", false, 0, Some("other.org")).unwrap();
        assert_eq!(left.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clear_site_data_wipes_idb_held_open_by_page() {
        use lumen_core::ext::{IdbRecordOp, IdbSchemaOp};
        let dir = std::env::temp_dir().join(format!("lumen-partition-idb-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let k = key("example.com");
        let page = crate::indexed_db::NativeIdbStore::for_origin(k.site(), &dir).unwrap();
        page.apply_schema(&IdbSchemaOp::CreateStore {
            db_name: "d".into(),
            store_name: "s".into(),
            key_path: None,
            auto_increment: false,
        })
        .unwrap();
        page.exec_op(&IdbRecordOp::Put {
            db_name: "d".into(),
            store_name: "s".into(),
            key_json: "1".into(),
            value_json: "2".into(),
        })
        .unwrap();
        let report = clear_site_data(&k, &SiteDataTargets { idb_dir: Some(&dir), ..SiteDataTargets::default() });
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        let got = page
            .exec_op(&IdbRecordOp::Get { db_name: "d".into(), store_name: "s".into(), key_json: "1".into() })
            .unwrap();
        assert!(matches!(got, lumen_core::ext::IdbOpResult::Value(None)), "{got:?}");
        drop(page);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clear_site_covers_dns_broadcast_bfcache_push_messages() {
        use crate::{BfCache, BfCacheEntry, BfCachePayload, BroadcastChannels, DnsCache};
        let k = key("example.com");

        let dns = DnsCache::open_in_memory().unwrap();
        dns.put("www.example.com", &["1.1.1.1".into()], 0, 60).unwrap();
        dns.put("other.org", &["2.2.2.2".into()], 0, 60).unwrap();
        let bc = BroadcastChannels::open_in_memory().unwrap();
        bc.register("https://a.example.com", "ch", "c1", 0).unwrap();
        bc.register("https://other.org", "ch", "c1", 0).unwrap();
        let report = clear_site_data(
            &k,
            &SiteDataTargets {
                dns_cache: Some(&dns),
                broadcast_channels: Some(&bc),
                ..SiteDataTargets::default()
            },
        );
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(report.total(), 2);
        assert_eq!(dns.count().unwrap(), 1);
        assert_eq!(bc.count().unwrap(), 1);

        let mut bf = BfCache::new(4);
        for url in ["https://www.example.com/a", "https://other.org/b"] {
            bf.store(BfCacheEntry {
                url: url.into(),
                payload: BfCachePayload::HtmlSnapshot(String::new()),
                scroll_x: 0.0,
                scroll_y: 0.0,
                title: None,
            });
        }
        assert_eq!(bf.clear_site(&k), 1);
        assert_eq!(bf.len(), 1);
        assert!(bf.retrieve("https://other.org/b").is_some());

        let msgs = crate::push_messages::PushMessages::open_in_memory().unwrap();
        msgs.enqueue(1, "https://example.com", b"x", 0).unwrap();
        msgs.enqueue(2, "https://other.org", b"y", 0).unwrap();
        assert_eq!(msgs.clear_site(&k).unwrap(), 1);
        assert_eq!(msgs.count_pending(1).unwrap(), 0);
        assert_eq!(msgs.count_pending(2).unwrap(), 1);
    }
}

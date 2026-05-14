//! Site engagement — per-origin метрики использования для omnibox
//! ранжирования и UI-сортировки. Аналог Chromium `site engagement`
//! signals.
//!
//! Каждый origin имеет:
//! - visit_count: число посещений;
//! - total_time_seconds: суммарное время на сайте (foreground time);
//! - last_visit: Unix timestamp последнего визита;
//! - first_visit: Unix timestamp первого визита.
//!
//! `score(now_unix, half_life_days)` возвращает decay-нормированный
//! score (recent visits весят больше старых) — для ранжирования
//! результатов в omnibox / new-tab «top sites».

use std::path::Path;
use std::sync::Mutex;

use lumen_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteEngagement {
    pub origin: String,
    pub visit_count: i64,
    pub total_time_seconds: i64,
    pub last_visit: i64,
    pub first_visit: i64,
}

impl SiteEngagement {
    /// Engagement score с exponential decay по last_visit. Чем дальше
    /// last_visit от `now_unix`, тем меньше score. `half_life_days`
    /// — за сколько дней score падает вдвое (типично 30-90 дней).
    /// Базовая формула: `(visit_count + total_time_sec/300) * 0.5^(age/half_life)`.
    /// 300 секунд = 5 минут на сайте ≈ 1 визиту в весе.
    pub fn score(&self, now_unix: i64, half_life_days: f64) -> f64 {
        let age_seconds = (now_unix - self.last_visit).max(0) as f64;
        let age_days = age_seconds / 86_400.0;
        let decay = 0.5f64.powf(age_days / half_life_days.max(0.001));
        let base = self.visit_count as f64 + (self.total_time_seconds as f64) / 300.0;
        base * decay
    }
}

pub struct SiteEngagementStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SiteEngagementStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SiteEngagementStore").finish()
    }
}

impl SiteEngagementStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("site_engagement open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("site_engagement open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS site_engagement (
                origin             TEXT PRIMARY KEY,
                visit_count        INTEGER NOT NULL DEFAULT 0,
                total_time_seconds INTEGER NOT NULL DEFAULT 0,
                last_visit         INTEGER NOT NULL,
                first_visit        INTEGER NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS se_last_visit_idx ON site_engagement(last_visit DESC);
            "#,
        )
        .map_err(|e| Error::Storage(format!("site_engagement init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Зафиксировать визит. Инкрементирует visit_count, обновляет last_visit
    /// (MAX). Для нового origin создаёт строку с visit_count=1.
    pub fn record_visit(&self, origin: &str, now_unix: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("site_engagement mutex poisoned".into()))?;
        conn.execute(
            "INSERT INTO site_engagement (origin, visit_count, last_visit, first_visit)
             VALUES (?1, 1, ?2, ?2)
             ON CONFLICT (origin) DO UPDATE SET
                 visit_count = visit_count + 1,
                 last_visit = MAX(last_visit, excluded.last_visit)",
            params![origin, now_unix],
        )
        .map_err(|e| Error::Storage(format!("site_engagement record_visit: {e}")))?;
        Ok(())
    }

    /// Добавить time на сайте (foreground seconds).
    pub fn add_time(&self, origin: &str, seconds: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("site_engagement mutex poisoned".into()))?;
        conn.execute(
            "UPDATE site_engagement SET total_time_seconds = total_time_seconds + ?1
             WHERE origin = ?2",
            params![seconds.max(0), origin],
        )
        .map_err(|e| Error::Storage(format!("site_engagement add_time: {e}")))?;
        Ok(())
    }

    pub fn get(&self, origin: &str) -> Result<Option<SiteEngagement>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("site_engagement mutex poisoned".into()))?;
        conn.query_row(
            "SELECT origin, visit_count, total_time_seconds, last_visit, first_visit
             FROM site_engagement WHERE origin = ?1",
            params![origin],
            row_to_engagement,
        )
        .optional()
        .map_err(|e| Error::Storage(format!("site_engagement get: {e}")))
    }

    /// Топ-N origin-ов по score (decay-нормированному). Алгоритм:
    /// читаем все записи, вычисляем score в Rust (т.к. SQLite не имеет
    /// pow), сортируем DESC, обрезаем по limit. Для < 10 000 sites
    /// (типичный профиль) — мгновенно.
    pub fn top_by_score(
        &self,
        now_unix: i64,
        half_life_days: f64,
        limit: usize,
    ) -> Result<Vec<(SiteEngagement, f64)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("site_engagement mutex poisoned".into()))?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT origin, visit_count, total_time_seconds, last_visit, first_visit
                 FROM site_engagement",
            )
            .map_err(|e| Error::Storage(format!("site_engagement top prepare: {e}")))?;
        let rows = stmt
            .query_map([], row_to_engagement)
            .map_err(|e| Error::Storage(format!("site_engagement top query: {e}")))?;
        let mut all: Vec<(SiteEngagement, f64)> = Vec::new();
        for r in rows {
            let e = r.map_err(|e| Error::Storage(format!("site_engagement row: {e}")))?;
            let score = e.score(now_unix, half_life_days);
            all.push((e, score));
        }
        all.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        all.truncate(limit);
        Ok(all)
    }

    pub fn delete(&self, origin: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("site_engagement mutex poisoned".into()))?;
        conn.execute(
            "DELETE FROM site_engagement WHERE origin = ?1",
            params![origin],
        )
        .map_err(|e| Error::Storage(format!("site_engagement delete: {e}")))?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| Error::Storage("site_engagement mutex poisoned".into()))?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM site_engagement", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("site_engagement count: {e}")))?;
        Ok(n)
    }
}

fn row_to_engagement(row: &rusqlite::Row<'_>) -> rusqlite::Result<SiteEngagement> {
    Ok(SiteEngagement {
        origin: row.get(0)?,
        visit_count: row.get(1)?,
        total_time_seconds: row.get(2)?,
        last_visit: row.get(3)?,
        first_visit: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> SiteEngagementStore {
        SiteEngagementStore::open_in_memory().unwrap()
    }

    #[test]
    fn record_visit_inserts_new_origin() {
        let s = make();
        s.record_visit("https://example.com", 100).unwrap();
        let e = s.get("https://example.com").unwrap().unwrap();
        assert_eq!(e.visit_count, 1);
        assert_eq!(e.first_visit, 100);
        assert_eq!(e.last_visit, 100);
        assert_eq!(e.total_time_seconds, 0);
    }

    #[test]
    fn record_visit_increments() {
        let s = make();
        s.record_visit("https://x/", 100).unwrap();
        s.record_visit("https://x/", 200).unwrap();
        s.record_visit("https://x/", 300).unwrap();
        let e = s.get("https://x/").unwrap().unwrap();
        assert_eq!(e.visit_count, 3);
        assert_eq!(e.first_visit, 100);  // preserved
        assert_eq!(e.last_visit, 300);   // updated
    }

    #[test]
    fn add_time_accumulates() {
        let s = make();
        s.record_visit("https://x/", 100).unwrap();
        s.add_time("https://x/", 60).unwrap();
        s.add_time("https://x/", 120).unwrap();
        assert_eq!(s.get("https://x/").unwrap().unwrap().total_time_seconds, 180);
    }

    #[test]
    fn score_decreases_with_age() {
        let e = SiteEngagement {
            origin: "x".into(),
            visit_count: 10,
            total_time_seconds: 0,
            last_visit: 0,
            first_visit: 0,
        };
        // half-life = 1 day. Через 1 день score = 5; через 2 = 2.5.
        let now_in_day = 86_400;
        let now_in_2_days = 172_800;
        let s1 = e.score(now_in_day, 1.0);
        let s2 = e.score(now_in_2_days, 1.0);
        assert!((s1 - 5.0).abs() < 0.01);
        assert!((s2 - 2.5).abs() < 0.01);
    }

    #[test]
    fn score_includes_time_on_site() {
        let e = SiteEngagement {
            origin: "x".into(),
            visit_count: 0,
            total_time_seconds: 1500,  // 5 минут × 5 = 5 "виртуальных визитов"
            last_visit: 100,
            first_visit: 100,
        };
        // visit_count=0 + 1500/300 = 5. No decay (last_visit == now).
        assert!((e.score(100, 30.0) - 5.0).abs() < 0.01);
    }

    #[test]
    fn top_by_score_sorted_correctly() {
        let s = make();
        s.record_visit("https://recent/", 1000).unwrap();
        for _ in 0..10 {
            s.record_visit("https://old-frequent/", 100).unwrap();
        }
        s.record_visit("https://new-rare/", 1100).unwrap();
        // now = 1200, half-life = 7 days. recent (1 visit, age ~200s) очень
        // близок к visit_count=1; old-frequent (10 visits, age 1100s) старее,
        // но больше. new-rare (1 visit, age 100s) — почти не decay-нут.
        let top = s.top_by_score(1200, 7.0, 10).unwrap();
        // old-frequent должен быть #1 (10 visits ~ 9 после decay).
        assert_eq!(top[0].0.origin, "https://old-frequent/");
        // new-rare выше recent (меньше age).
        assert!(top.iter().position(|x| x.0.origin == "https://new-rare/").unwrap()
            < top.iter().position(|x| x.0.origin == "https://recent/").unwrap());
    }

    #[test]
    fn top_by_score_respects_limit() {
        let s = make();
        for i in 0..5 {
            s.record_visit(&format!("https://e{i}/"), 100).unwrap();
        }
        let top = s.top_by_score(200, 30.0, 3).unwrap();
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn delete_removes_origin() {
        let s = make();
        s.record_visit("https://x/", 100).unwrap();
        s.delete("https://x/").unwrap();
        assert!(s.get("https://x/").unwrap().is_none());
    }

    #[test]
    fn get_missing_returns_none() {
        let s = make();
        assert!(s.get("https://nope/").unwrap().is_none());
    }

    #[test]
    fn count_works() {
        let s = make();
        assert_eq!(s.count().unwrap(), 0);
        s.record_visit("https://a/", 100).unwrap();
        s.record_visit("https://b/", 200).unwrap();
        s.record_visit("https://a/", 300).unwrap();  // existing
        assert_eq!(s.count().unwrap(), 2);
    }

    #[test]
    fn cyrillic_origin() {
        let s = make();
        s.record_visit("https://пример.рф/", 100).unwrap();
        s.add_time("https://пример.рф/", 300).unwrap();
        let e = s.get("https://пример.рф/").unwrap().unwrap();
        assert_eq!(e.origin, "https://пример.рф/");
        assert_eq!(e.total_time_seconds, 300);
    }

    #[test]
    fn add_time_negative_is_clamped_to_zero() {
        let s = make();
        s.record_visit("https://x/", 100).unwrap();
        s.add_time("https://x/", -50).unwrap();
        assert_eq!(s.get("https://x/").unwrap().unwrap().total_time_seconds, 0);
    }
}

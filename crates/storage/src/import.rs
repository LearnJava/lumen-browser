//! UX-IMPORT: чтение закладок и истории из профилей Chrome/Edge/Firefox.
//!
//! Модуль только читает чужие файлы и отдаёт нейтральные структуры
//! ([`ImportedBookmark`], [`ImportedVisit`]); запись в наши хранилища —
//! [`import_bookmarks`] / [`import_history`]. Базы SQLite копируются во
//! временный файл перед открытием: браузер-источник может держать их открытыми.
//! Пароли здесь не трогаются (DPAPI/NSS — отдельный срез).

use std::path::{Path, PathBuf};

use lumen_core::{Error, Result};
use rusqlite::{Connection, OpenFlags};

use crate::bookmarks::Bookmarks;
use crate::history::History;

/// Секунд между 1601-01-01 (эпоха WebKit/Chrome) и 1970-01-01.
const WEBKIT_EPOCH_OFFSET_SECS: i64 = 11_644_473_600;

/// Браузер-источник.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBrowser {
    /// Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
    /// Mozilla Firefox.
    Firefox,
}

/// Найденный профиль чужого браузера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProfile {
    /// Какой браузер.
    pub browser: SourceBrowser,
    /// Каталог профиля.
    pub dir: PathBuf,
}

/// Закладка из чужого браузера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedBookmark {
    /// Адрес (только http/https).
    pub url: String,
    /// Название.
    pub title: String,
    /// Путь папки через `/` (`Панель закладок/Работа`); пусто — корень.
    pub folder: String,
    /// Unix-секунды; 0 — неизвестно.
    pub created_at: i64,
}

/// Запись истории из чужого браузера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedVisit {
    /// Адрес (только http/https).
    pub url: String,
    /// Заголовок страницы.
    pub title: String,
    /// Unix-секунды последнего визита.
    pub visit_date: i64,
    /// Число визитов, не меньше 1.
    pub visit_count: i64,
}

/// Итог записи в хранилище.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Записано.
    pub imported: usize,
    /// Пропущено как уже существующее.
    pub skipped: usize,
}

fn importable(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn webkit_micros_to_unix(us: i64) -> i64 {
    if us <= 0 {
        return 0;
    }
    (us / 1_000_000 - WEBKIT_EPOCH_OFFSET_SECS).max(0)
}

/// Профили, найденные в стандартных местах текущего пользователя ОС.
pub fn detect_profiles() -> Vec<SourceProfile> {
    let mut out = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        for (browser, rel) in [
            (SourceBrowser::Chrome, "Google/Chrome/User Data"),
            (SourceBrowser::Edge, "Microsoft/Edge/User Data"),
        ] {
            let Ok(rd) = std::fs::read_dir(local.join(rel)) else { continue };
            let mut dirs: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    (n == "Default" || n.starts_with("Profile "))
                        && (p.join("Bookmarks").is_file() || p.join("History").is_file())
                })
                .collect();
            dirs.sort();
            out.extend(dirs.into_iter().map(|dir| SourceProfile { browser, dir }));
        }
    }
    if let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from)
        && let Ok(rd) = std::fs::read_dir(roaming.join("Mozilla/Firefox/Profiles"))
    {
        let mut dirs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("places.sqlite").is_file())
            .collect();
        dirs.sort();
        out.extend(
            dirs.into_iter()
                .map(|dir| SourceProfile { browser: SourceBrowser::Firefox, dir }),
        );
    }
    out
}

/// Закладки профиля: Chromium-JSON либо `places.sqlite`.
pub fn read_bookmarks(profile: &SourceProfile) -> Result<Vec<ImportedBookmark>> {
    match profile.browser {
        SourceBrowser::Chrome | SourceBrowser::Edge => {
            let text = std::fs::read_to_string(profile.dir.join("Bookmarks"))
                .map_err(|e| Error::Storage(format!("import: Bookmarks: {e}")))?;
            parse_chromium_bookmarks(&text)
        }
        SourceBrowser::Firefox => read_firefox_bookmarks(&profile.dir.join("places.sqlite")),
    }
}

/// История профиля.
pub fn read_history(profile: &SourceProfile) -> Result<Vec<ImportedVisit>> {
    match profile.browser {
        SourceBrowser::Chrome | SourceBrowser::Edge => {
            read_chromium_history(&profile.dir.join("History"))
        }
        SourceBrowser::Firefox => read_firefox_history(&profile.dir.join("places.sqlite")),
    }
}

/// Разбор файла `Bookmarks` Chrome/Edge.
pub fn parse_chromium_bookmarks(json: &str) -> Result<Vec<ImportedBookmark>> {
    let root: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| Error::Storage(format!("import: Bookmarks json: {e}")))?;
    let mut out = Vec::new();
    if let Some(roots) = root.get("roots").and_then(|r| r.as_object()) {
        for node in roots.values().filter(|n| n.is_object()) {
            let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
            walk_chromium(node, name, &mut out);
        }
    }
    Ok(out)
}

fn walk_chromium(node: &serde_json::Value, folder: &str, out: &mut Vec<ImportedBookmark>) {
    let Some(children) = node.get("children").and_then(|c| c.as_array()) else { return };
    for child in children {
        let str_of = |k: &str| child.get(k).and_then(|v| v.as_str()).unwrap_or("");
        match str_of("type") {
            "url" if importable(str_of("url")) => out.push(ImportedBookmark {
                url: str_of("url").to_string(),
                title: str_of("name").to_string(),
                folder: folder.to_string(),
                // date_added — строка с микросекундами WebKit.
                created_at: webkit_micros_to_unix(str_of("date_added").parse().unwrap_or(0)),
            }),
            "folder" => {
                let sub = if folder.is_empty() {
                    str_of("name").to_string()
                } else {
                    format!("{folder}/{}", str_of("name"))
                };
                walk_chromium(child, &sub, out);
            }
            _ => {}
        }
    }
}

/// Временный каталог, удаляемый при drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lumen-import-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Storage(format!("import: tmpdir: {e}")))?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Открыть копию чужой базы (вместе с WAL, если он есть).
fn open_copy(src: &Path) -> Result<(Connection, TempDir)> {
    let guard = TempDir::new()?;
    let dst = guard.0.join("db.sqlite");
    std::fs::copy(src, &dst)
        .map_err(|e| Error::Storage(format!("import: copy {}: {e}", src.display())))?;
    let with_suffix = |p: &Path| {
        let mut s = p.as_os_str().to_owned();
        s.push("-wal");
        PathBuf::from(s)
    };
    // WAL Firefox хранит свежие записи; его отсутствие — не ошибка.
    let _ = std::fs::copy(with_suffix(src), with_suffix(&dst));
    let conn = Connection::open_with_flags(&dst, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| Error::Storage(format!("import: open {}: {e}", src.display())))?;
    Ok((conn, guard))
}

fn sql_err(what: &str, e: rusqlite::Error) -> Error {
    Error::Storage(format!("import: {what}: {e}"))
}

/// `History` Chrome/Edge: таблица `urls`.
pub fn read_chromium_history(path: &Path) -> Result<Vec<ImportedVisit>> {
    let (conn, _g) = open_copy(path)?;
    query_visits(
        &conn,
        "SELECT url, COALESCE(title, ''), last_visit_time, visit_count FROM urls
         WHERE hidden = 0 ORDER BY last_visit_time DESC",
        webkit_micros_to_unix,
    )
}

/// `places.sqlite` Firefox: история из `moz_places`.
pub fn read_firefox_history(path: &Path) -> Result<Vec<ImportedVisit>> {
    let (conn, _g) = open_copy(path)?;
    query_visits(
        &conn,
        "SELECT url, COALESCE(title, ''), COALESCE(last_visit_date, 0), visit_count
         FROM moz_places WHERE visit_count > 0 AND hidden = 0
         ORDER BY last_visit_date DESC",
        |us| (us / 1_000_000).max(0),
    )
}

fn query_visits(
    conn: &Connection,
    sql: &str,
    to_unix: impl Fn(i64) -> i64,
) -> Result<Vec<ImportedVisit>> {
    let mut stmt = conn.prepare(sql).map_err(|e| sql_err("history prepare", e))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ImportedVisit {
                url: r.get(0)?,
                title: r.get(1)?,
                visit_date: to_unix(r.get::<_, i64>(2)?),
                visit_count: r.get::<_, i64>(3)?.max(1),
            })
        })
        .map_err(|e| sql_err("history query", e))?;
    let mut out = Vec::new();
    for row in rows {
        let v = row.map_err(|e| sql_err("history row", e))?;
        if importable(&v.url) {
            out.push(v);
        }
    }
    Ok(out)
}

/// Закладки Firefox: `moz_bookmarks` (type 1 — закладка, 2 — папка).
pub fn read_firefox_bookmarks(path: &Path) -> Result<Vec<ImportedBookmark>> {
    let (conn, _g) = open_copy(path)?;
    // id → (parent, title) для сборки пути папки.
    let mut folders = std::collections::HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, parent, COALESCE(title, '') FROM moz_bookmarks WHERE type = 2")
            .map_err(|e| sql_err("folders prepare", e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, (r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))
            })
            .map_err(|e| sql_err("folders query", e))?;
        for row in rows {
            let (id, v) = row.map_err(|e| sql_err("folders row", e))?;
            folders.insert(id, v);
        }
    }
    let folder_path = |mut id: i64| {
        let mut parts = Vec::new();
        // Предел глубины защищает от циклов в битой базе.
        for _ in 0..64 {
            let Some((parent, title)) = folders.get(&id) else { break };
            // Корень (parent = 0) служебный и без названия.
            if *parent != 0 && !title.is_empty() {
                parts.push(title.clone());
            }
            id = *parent;
        }
        parts.reverse();
        parts.join("/")
    };
    let mut stmt = conn
        .prepare(
            "SELECT p.url, COALESCE(b.title, p.title, ''), b.dateAdded, b.parent
             FROM moz_bookmarks b JOIN moz_places p ON p.id = b.fk
             WHERE b.type = 1 ORDER BY b.id",
        )
        .map_err(|e| sql_err("bookmarks prepare", e))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| sql_err("bookmarks query", e))?;
    let mut out = Vec::new();
    for row in rows {
        let (url, title, added, parent) = row.map_err(|e| sql_err("bookmarks row", e))?;
        if importable(&url) {
            out.push(ImportedBookmark {
                url,
                title,
                folder: folder_path(parent),
                created_at: (added / 1_000_000).max(0),
            });
        }
    }
    Ok(out)
}

/// Записать закладки; уже существующий URL не перезаписывается.
pub fn import_bookmarks(store: &Bookmarks, items: &[ImportedBookmark]) -> Result<ImportReport> {
    let mut rep = ImportReport::default();
    for b in items {
        if store.get(&b.url)?.is_some() {
            rep.skipped += 1;
            continue;
        }
        store.add(&b.url, &b.title, &b.folder, &[], "", b.created_at)?;
        rep.imported += 1;
    }
    Ok(rep)
}

/// Записать историю: счётчик и дата объединяются с имеющимися.
pub fn import_history(store: &History, items: &[ImportedVisit]) -> Result<ImportReport> {
    let mut rep = ImportReport::default();
    for v in items {
        store.import_visit(&v.url, &v.title, v.visit_date, v.visit_count)?;
        rep.imported += 1;
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME_JSON: &str = r#"{"roots":{
      "bookmark_bar":{"name":"Панель","type":"folder","children":[
        {"type":"url","name":"A","url":"https://a.test/","date_added":"13300000000000000"},
        {"type":"folder","name":"Работа","children":[
          {"type":"url","name":"B","url":"https://b.test/","date_added":"0"},
          {"type":"url","name":"JS","url":"javascript:void(0)"}]}]},
      "other":{"name":"Другие","type":"folder","children":[
        {"type":"url","name":"C","url":"http://c.test/"}]}}}"#;

    #[test]
    fn chromium_bookmarks_tree() {
        let v = parse_chromium_bookmarks(CHROME_JSON).unwrap();
        assert_eq!(v.len(), 3);
        let a = v.iter().find(|b| b.title == "A").unwrap();
        assert_eq!(a.folder, "Панель");
        assert_eq!(a.created_at, 13_300_000_000 - WEBKIT_EPOCH_OFFSET_SECS);
        let b = v.iter().find(|b| b.title == "B").unwrap();
        assert_eq!(b.folder, "Панель/Работа");
        assert_eq!(b.created_at, 0);
        assert_eq!(v.iter().find(|b| b.title == "C").unwrap().folder, "Другие");
    }

    #[test]
    fn chromium_garbage_is_error() {
        assert!(parse_chromium_bookmarks("not json").is_err());
        assert!(parse_chromium_bookmarks("{}").unwrap().is_empty());
    }

    fn temp_db(name: &str, sql: &str) -> (TempDir, PathBuf) {
        let g = TempDir::new().unwrap();
        let p = g.0.join(name);
        Connection::open(&p).unwrap().execute_batch(sql).unwrap();
        (g, p)
    }

    #[test]
    fn firefox_places_roundtrip() {
        let (_g, p) = temp_db(
            "places.sqlite",
            "CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url TEXT, title TEXT,
               visit_count INTEGER, hidden INTEGER DEFAULT 0, last_visit_date INTEGER);
             CREATE TABLE moz_bookmarks(id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER,
               parent INTEGER, title TEXT, dateAdded INTEGER);
             INSERT INTO moz_places VALUES(1,'https://a.test/','Page A',3,0,1700000000000000);
             INSERT INTO moz_places VALUES(2,'place:type=6','Smart',0,0,NULL);
             INSERT INTO moz_places VALUES(3,'https://h.test/','Hidden',2,1,1700000000000000);
             INSERT INTO moz_bookmarks VALUES(1,2,NULL,0,'',0);
             INSERT INTO moz_bookmarks VALUES(2,2,NULL,1,'menu',0);
             INSERT INTO moz_bookmarks VALUES(3,2,NULL,2,'Работа',0);
             INSERT INTO moz_bookmarks VALUES(4,1,1,3,'Мой A',1600000000000000);
             INSERT INTO moz_bookmarks VALUES(5,1,2,3,'Smart',0);",
        );
        let bm = read_firefox_bookmarks(&p).unwrap();
        assert_eq!(bm.len(), 1);
        assert_eq!(bm[0].folder, "menu/Работа");
        assert_eq!(bm[0].title, "Мой A");
        assert_eq!(bm[0].created_at, 1_600_000_000);
        let h = read_firefox_history(&p).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!((h[0].visit_count, h[0].visit_date), (3, 1_700_000_000));
    }

    #[test]
    fn chromium_history_and_store_merge() {
        let (_g, p) = temp_db(
            "History",
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT,
               visit_count INTEGER, last_visit_time INTEGER, hidden INTEGER DEFAULT 0);
             INSERT INTO urls VALUES(1,'https://a.test/','A',4,13300000000000000,0);
             INSERT INTO urls VALUES(2,'chrome://settings','S',1,13300000000000000,0);",
        );
        let v = read_chromium_history(&p).unwrap();
        assert_eq!(v.len(), 1);
        let store = History::open_in_memory().unwrap();
        store.record_visit("https://a.test/", "Local", 100).unwrap();
        let rep = import_history(&store, &v).unwrap();
        assert_eq!(rep.imported, 1);
        let e = store.get("https://a.test/").unwrap().unwrap();
        assert_eq!(e.visit_count, 5);
        assert_eq!(e.visit_date, v[0].visit_date);
        assert_eq!(e.title, "A");
    }

    #[test]
    fn bookmarks_import_skips_existing() {
        let store = Bookmarks::open_in_memory().unwrap();
        store.add("https://a.test/", "Mine", "", &[], "", 1).unwrap();
        let v = parse_chromium_bookmarks(CHROME_JSON).unwrap();
        let rep = import_bookmarks(&store, &v).unwrap();
        assert_eq!((rep.imported, rep.skipped), (2, 1));
        assert_eq!(store.get("https://a.test/").unwrap().unwrap().title, "Mine");
    }
}

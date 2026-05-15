//! Safe Browsing — локальный hash-prefix фильтр malware / phishing URL.
//!
//! Аналог Google Safe Browsing v4 в варианте «локальный список без облака»:
//! списки распространяются как полные `SHA-256(canonical_expression)` (32 байта
//! на запись), без обновлений по сети и без full-hash callback-ов. Это
//! сознательное отклонение от спецификации Google ради принципа №1
//! «приватность по умолчанию» — мы не отправляем хешированные префиксы URL
//! на чужой сервер за подтверждением.
//!
//! API делится на две части:
//! - [`SafeBrowsingList`] — SQLite таблица hash → threat_type, CRUD + lookup;
//! - [`SafeBrowsingFilter`] — реализация `lumen_core::ext::RequestFilter`,
//!   подключаемая в `lumen-network::HttpClient::with_filter`. На каждый URL
//!   генерирует до 20 канонических вариантов (Safe Browsing v4 §4.4), хэширует
//!   каждый и ищет в таблице.
//!
//! Канонизация URL (упрощённая по Safe Browsing v4 §4.4):
//! - lowercase host (с переводом IDN в Punycode через `lumen_core::idn`);
//! - удалить fragment;
//! - схлопнуть последовательные `/` в одиночные;
//! - resolve `.`/`..` сегментов;
//! - пустой path → `/`;
//! - **результат hashing-а** — строка `host[/path[?query]]` без scheme и без
//!   user-info; для каждого host_level × path_level одна запись.
//!
//! Phase 0 ограничения:
//! - **Полные хэши**, не 4-byte prefix как в спецификации. Места меньше
//!   ценим, чем простоту (для FP-free lookup всё равно нужны full-hash-и).
//! - **Без percent-encoding нормализации**: повторный unescape согласно spec
//!   (Safe Browsing v4 §4.4.1) не делается — для адресных URL Phase 0 это
//!   приемлемо, для adversarial content (URL с `%2E%2E` для bypass) — todo.
//! - **Без public-suffix list**: при генерации host suffixes мы не отсекаем
//!   на eTLD+1 (что в Google v4 защищает от ложных совпадений по `co.uk`).
//!   Минимум — обрезаем до 2-х компонент. Подключение PSL — отдельный exception
//!   или собственная упрощённая таблица — отдельная задача.

use std::path::Path;
use std::sync::{Arc, Mutex};

use lumen_core::ext::{PublicSuffixList, RequestFilter};
use lumen_core::hash::sha256;
use lumen_core::idn::domain_to_ascii;
use lumen_core::url::Url;
use lumen_core::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};

// ── ThreatType ──────────────────────────────────────────────────────────────

/// Категория угрозы для записи в Safe Browsing list. Имена совпадают с
/// Google Safe Browsing API `ThreatType` для совместимости импорта чужих
/// списков; внутри Lumen эти значения попадают только в `reason`-строку
/// для network log / UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreatType {
    /// Заражённый сайт, распространяющий вредоносное ПО.
    Malware,
    /// Phishing / спуфинг легитимных сайтов.
    SocialEngineering,
    /// Adware / browser hijacker.
    UnwantedSoftware,
    /// Сайты с подозрительным контентом, ниже порога Malware.
    PotentiallyHarmful,
    /// Forward-compat: чужие списки могут содержать неизвестные категории.
    Other(String),
}

impl ThreatType {
    /// Сериализация в стабильный кодовый идентификатор для БД (lowercase
    /// snake_case, тот же что у Google v4 API в lowercase).
    #[must_use]
    pub fn as_code(&self) -> String {
        match self {
            Self::Malware => "malware".to_string(),
            Self::SocialEngineering => "social_engineering".to_string(),
            Self::UnwantedSoftware => "unwanted_software".to_string(),
            Self::PotentiallyHarmful => "potentially_harmful".to_string(),
            Self::Other(s) => s.clone(),
        }
    }

    /// Обратный парсинг из кодового id. Неизвестные строки → `Other(s)`,
    /// чтобы forward-compat импорт чужих списков не валился.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code {
            "malware" => Self::Malware,
            "social_engineering" => Self::SocialEngineering,
            "unwanted_software" => Self::UnwantedSoftware,
            "potentially_harmful" => Self::PotentiallyHarmful,
            other => Self::Other(other.to_string()),
        }
    }
}

// ── Canonical URL + hash variants ───────────────────────────────────────────

/// Сгенерировать список всех 5×4=20 канонических вариантов `host/path?query`
/// для проверки URL против Safe Browsing list-а. См. [`canonical_expression_variants_with_psl`]
/// — basic версия без PSL, host-suffix enumeration урезается просто до 2-х компонент.
///
/// Алгоритм по Safe Browsing v4 §4.4.2:
/// - host: оригинальный + 4 потомка через срез leading-component-ов
///   (`a.b.c.d` → `a.b.c.d`, `b.c.d`, `c.d`); не урезаем ниже 2 компонент,
///   т.е. для `a.b.c.d` получаем 3 варианта, для `a.b.c.d.e` — 4.
/// - path: оригинальный + variants через срез trailing-сегментов
///   (`/1/2.html?p=1` → `/1/2.html?p=1`, `/1/2.html`, `/1/`, `/`);
///   query сохраняется в одном самом длинном варианте.
///
/// Итог — `Vec<String>` (не deduped — caller хэширует каждый и вставит в
/// `HashSet<[u8;32]>` если duplicates критичны).
#[must_use]
pub fn canonical_expression_variants(url: &Url) -> Vec<String> {
    canonical_expression_variants_with_psl(url, None)
}

/// Версия [`canonical_expression_variants`] с опциональной обрезкой
/// host-suffix enumeration через `PublicSuffixList` до eTLD+1 включительно.
///
/// Без PSL мы останавливаемся «когда осталась 1 компонента» — это правильно
/// для `.com` (не идём до `com`), но неправильно для `.co.uk`: цепочка
/// `a.b.example.co.uk` → `example.co.uk` → `co.uk` → STOP оставит `co.uk`
/// в списке вариантов; запись «evil.co.uk» в Safe Browsing спокойно
/// сматчит **любой** сайт `*.co.uk` через `co.uk` shadow-entry. PSL
/// решает это: при `is_public_suffix(tail)` → break **без** добавления
/// tail-а. Тогда для `a.b.example.co.uk` цепочка — `a.b.example.co.uk`
/// → `b.example.co.uk` → `example.co.uk` → STOP (`co.uk` — public suffix).
///
/// Если PSL = `None`, поведение точно совпадает с
/// [`canonical_expression_variants`] (фолбэк на 2-component rule).
#[must_use]
pub fn canonical_expression_variants_with_psl(
    url: &Url,
    psl: Option<&dyn PublicSuffixList>,
) -> Vec<String> {
    let host = match url.host_ascii() {
        Ok(h) if !h.is_empty() => h.to_ascii_lowercase(),
        _ => return Vec::new(),
    };

    // Host suffixes: full + срезы по leading-точкам.
    let mut hosts: Vec<String> = Vec::new();
    // Если сам host — public suffix, ничего не enumerate-им (защита от
    // input-а вида http://co.uk/ блокировать всё `.co.uk`).
    if let Some(p) = psl
        && p.is_public_suffix(&host)
    {
        // По-прежнему добавляем сам host (точечная блокировка по host —
        // допустима, если caller сознательно добавил такую запись).
        hosts.push(host.clone());
    } else {
        hosts.push(host.clone());
        let mut i = 0usize;
        while let Some(pos) = host[i..].find('.') {
            let start = i + pos + 1;
            let tail = &host[start..];
            // PSL стоп-правило: tail — known public suffix → не идём ниже.
            if let Some(p) = psl
                && p.is_public_suffix(tail)
            {
                break;
            }
            // Без PSL: fallback на 2-component rule.
            if psl.is_none() && !tail.contains('.') {
                break;
            }
            hosts.push(tail.to_string());
            i = start;
            if hosts.len() >= 5 {
                break;
            }
        }
    }

    // Path variants.
    let normalized = normalize_path(url.path());
    let query = url.query();

    let mut paths: Vec<String> = Vec::new();
    // Самый длинный — полный path с query.
    if let Some(q) = query {
        paths.push(format!("{normalized}?{q}"));
    }
    // Полный path без query.
    paths.push(normalized.clone());
    // Trailing-cut: убираем сегменты с конца до тех пор, пока path != `/`.
    let mut p = normalized.clone();
    while paths.len() < 4 {
        // Найти последнюю `/`. Если path == "/" — выйти.
        if p == "/" || p.is_empty() {
            break;
        }
        // Если path заканчивается на `/`, обрезаем `/` плюс предыдущий сегмент.
        let trimmed = p.trim_end_matches('/');
        match trimmed.rfind('/') {
            Some(pos) => {
                p = trimmed[..=pos].to_string(); // включая `/`
            }
            None => {
                p = "/".to_string();
            }
        }
        if paths.last().is_some_and(|last| last == &p) {
            break;
        }
        paths.push(p.clone());
    }
    // Гарантируем наличие `/` (root) в списке.
    if !paths.iter().any(|s| s == "/") {
        paths.push("/".to_string());
    }

    let mut out: Vec<String> = Vec::with_capacity(hosts.len() * paths.len());
    for h in &hosts {
        for p in &paths {
            // Канонический вид: `host/path` (path начинается с `/`).
            // Для root: `host/`.
            let entry = if p == "/" {
                format!("{h}/")
            } else {
                format!("{h}{p}")
            };
            out.push(entry);
        }
    }
    out
}

/// Нормализация path по упрощённым правилам Safe Browsing v4 §4.4.1:
/// - удалить дублирующиеся `/` (`//x` → `/x`);
/// - resolve `.`/`..` сегментов;
/// - пустой path → `/`.
fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let mut stack: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    for s in &segments {
        match *s {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    let trailing_slash = path.ends_with('/');
    let joined = stack.join("/");
    if joined.is_empty() {
        "/".to_string()
    } else if trailing_slash {
        format!("/{joined}/")
    } else {
        format!("/{joined}")
    }
}

/// Хэш канонического expression-а — SHA-256 32 байта. Удобный helper для
/// caller-ов: «дайте мне hash для URL, я положу его в список».
#[must_use]
pub fn hash_expression(expr: &str) -> [u8; 32] {
    sha256(expr.as_bytes())
}

// ── Storage ─────────────────────────────────────────────────────────────────

/// SQLite-backed список Safe Browsing записей.
///
/// Таблица `safe_browsing(list_name, full_hash, threat_type, added_at)` с
/// composite PK `(list_name, full_hash)`. Одна и та же запись может фигурировать
/// в нескольких списках одновременно (например, `google-malware-v4` и
/// `local-block-list`) — при `lookup` возвращается первое найденное (любой
/// список = match).
///
/// `full_hash` — BLOB(32) ровно. Foreign-import чужих списков проверяет
/// длину — invalid blob тихо отбрасывается; см. [`Self::add_hash`].
pub struct SafeBrowsingList {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for SafeBrowsingList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeBrowsingList").finish()
    }
}

impl SafeBrowsingList {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("safe_browsing open: {e}")))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| Error::Storage(format!("safe_browsing open_in_memory: {e}")))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS safe_browsing (
                list_name    TEXT NOT NULL,
                full_hash    BLOB NOT NULL,
                threat_type  TEXT NOT NULL,
                added_at     INTEGER NOT NULL,
                PRIMARY KEY (list_name, full_hash)
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS safe_browsing_hash_idx ON safe_browsing(full_hash);
            "#,
        )
        .map_err(|e| Error::Storage(format!("safe_browsing init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Добавить запись по уже-хэшированному значению. `full_hash` обязан
    /// быть 32 байта (SHA-256). Дубликат `(list_name, full_hash)` обновляет
    /// `threat_type` и `added_at` (`INSERT OR REPLACE`).
    pub fn add_hash(
        &self,
        list_name: &str,
        full_hash: &[u8],
        threat: &ThreatType,
        added_at: i64,
    ) -> Result<()> {
        if full_hash.len() != 32 {
            return Err(Error::Storage(format!(
                "safe_browsing add_hash: expected 32-byte hash, got {}",
                full_hash.len()
            )));
        }
        let code = threat.as_code();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO safe_browsing(list_name, full_hash, threat_type, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![list_name, full_hash, code, added_at],
        )
        .map_err(|e| Error::Storage(format!("safe_browsing add_hash: {e}")))?;
        Ok(())
    }

    /// Удобный wrapper: канонизировать URL → SHA-256 → `add_hash`.
    /// Записывает **один** хэш (полный canonical expression), а не все
    /// 20 variants — это даёт «точечную» блокировку. Если нужно
    /// блокировать целый домен, caller должен явно вызвать `add_url`
    /// для каждого host-suffix отдельно.
    pub fn add_url(
        &self,
        list_name: &str,
        url: &Url,
        threat: &ThreatType,
        added_at: i64,
    ) -> Result<()> {
        let host = url
            .host_ascii()
            .map_err(|e| Error::Storage(format!("safe_browsing add_url host: {e}")))?
            .to_ascii_lowercase();
        let host = if host.is_empty() {
            return Err(Error::Storage("safe_browsing add_url: empty host".into()));
        } else {
            // domain_to_ascii уже выполнена в host_ascii — но lowercase ещё нужен.
            // Дополнительно нормализуем через idn если на входе Unicode-host
            // (защита для caller-ов, передающих cyrillic URL без host_ascii).
            domain_to_ascii(&host).unwrap_or(host)
        };
        let path = normalize_path(url.path());
        let expr = match url.query() {
            Some(q) => format!("{host}{}?{q}", if path == "/" { "/" } else { &path }),
            None => format!("{host}{}", if path == "/" { "/" } else { &path }),
        };
        let hash = sha256(expr.as_bytes());
        self.add_hash(list_name, &hash, threat, added_at)
    }

    /// Прямой lookup по полному хэшу (32 байта). Возвращает первое
    /// найденное `(list_name, threat_type)` среди всех списков. `None`
    /// если ни в одном списке нет.
    pub fn lookup_hash(&self, full_hash: &[u8]) -> Result<Option<(String, ThreatType)>> {
        if full_hash.len() != 32 {
            return Ok(None);
        }
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT list_name, threat_type FROM safe_browsing
                 WHERE full_hash = ?1 LIMIT 1",
                params![full_hash],
                |r| {
                    let list_name: String = r.get(0)?;
                    let threat_code: String = r.get(1)?;
                    Ok((list_name, threat_code))
                },
            )
            .optional()
            .map_err(|e| Error::Storage(format!("safe_browsing lookup_hash: {e}")))?;
        Ok(row.map(|(ln, tc)| (ln, ThreatType::from_code(&tc))))
    }

    /// Главный entry-point фильтрации: проверить URL против всех списков,
    /// генерируя 20 канонических вариантов (host suffixes × path prefixes).
    /// На первый match возвращает `(list_name, threat_type)`; пустой host
    /// → None (нечего проверять). Без PSL обрезка host-suffix-ов идёт до
    /// 2 компонент (см. [`canonical_expression_variants`]).
    pub fn lookup_url(&self, url: &Url) -> Result<Option<(String, ThreatType)>> {
        self.lookup_url_with_psl(url, None)
    }

    /// Версия [`Self::lookup_url`] с опциональной PSL-обрезкой host-suffix
    /// enumeration. С PSL host-suffix цепочка останавливается на eTLD+1,
    /// и `co.uk` / `xn--p1ai` сами не попадают в lookup-варианты — это
    /// защищает от ложно-широких блокировок «whole-TLD».
    pub fn lookup_url_with_psl(
        &self,
        url: &Url,
        psl: Option<&dyn PublicSuffixList>,
    ) -> Result<Option<(String, ThreatType)>> {
        let variants = canonical_expression_variants_with_psl(url, psl);
        if variants.is_empty() {
            return Ok(None);
        }
        for expr in &variants {
            let hash = sha256(expr.as_bytes());
            if let Some(hit) = self.lookup_hash(&hash)? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }

    /// Удалить все записи указанного списка. `clear_list("google-v4")` —
    /// типичная операция перед re-import свежей dump-копии.
    pub fn clear_list(&self, list_name: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute(
                "DELETE FROM safe_browsing WHERE list_name = ?1",
                params![list_name],
            )
            .map_err(|e| Error::Storage(format!("safe_browsing clear_list: {e}")))?;
        Ok(n)
    }

    /// Удалить все записи во всех списках. Используется при logout/profile
    /// reset.
    pub fn clear_all(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM safe_browsing", [])
            .map_err(|e| Error::Storage(format!("safe_browsing clear_all: {e}")))?;
        Ok(n)
    }

    /// Сколько записей в конкретном списке.
    pub fn count_in(&self, list_name: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM safe_browsing WHERE list_name = ?1",
                params![list_name],
                |r| r.get(0),
            )
            .map_err(|e| Error::Storage(format!("safe_browsing count_in: {e}")))?;
        Ok(n as usize)
    }

    /// Сколько всего записей во всех списках.
    pub fn count_total(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM safe_browsing", [], |r| r.get(0))
            .map_err(|e| Error::Storage(format!("safe_browsing count_total: {e}")))?;
        Ok(n as usize)
    }
}

// ── RequestFilter wrapper ───────────────────────────────────────────────────

/// Тонкая обёртка над [`SafeBrowsingList`] для подключения в
/// `lumen-network::HttpClient::with_filter(...)`. Каждый исходящий запрос
/// проверяется до TCP/TLS-соединения; на match эмитится
/// `Event::RequestBlocked { reason: "{threat} ({list_name})" }` и fetch
/// возвращает Err — те же правила, что у любого `RequestFilter`.
///
/// Ошибки lookup-а (DB locked, BLOB corrupted) трактуются как «не блокировать»
/// — fail-open, симметрично HSTS-политике. Логируем через `eprintln!` для
/// диагностики; адекватный сетевой log приёмник появится отдельной задачей.
pub struct SafeBrowsingFilter {
    list: Arc<SafeBrowsingList>,
    psl: Option<Arc<dyn PublicSuffixList>>,
}

impl SafeBrowsingFilter {
    #[must_use]
    pub fn new(list: Arc<SafeBrowsingList>) -> Self {
        Self { list, psl: None }
    }

    /// Builder-конструктор с подключённым `PublicSuffixList`. С PSL
    /// host-suffix enumeration обрезается до eTLD+1 (см.
    /// [`SafeBrowsingList::lookup_url_with_psl`]).
    #[must_use]
    pub fn with_psl(list: Arc<SafeBrowsingList>, psl: Arc<dyn PublicSuffixList>) -> Self {
        Self {
            list,
            psl: Some(psl),
        }
    }
}

impl std::fmt::Debug for SafeBrowsingFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeBrowsingFilter").finish()
    }
}

impl RequestFilter for SafeBrowsingFilter {
    fn should_block(&self, url: &Url) -> Option<String> {
        let psl = self.psl.as_deref();
        match self.list.lookup_url_with_psl(url, psl) {
            Ok(Some((list_name, threat))) => {
                Some(format!("{} ({list_name})", threat.as_code()))
            }
            Ok(None) => None,
            Err(e) => {
                eprintln!("safe_browsing lookup failed: {e}; fail-open");
                None
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ThreatType ──────────────────────────────────────────────────────────

    #[test]
    fn threat_type_round_trip() {
        for t in [
            ThreatType::Malware,
            ThreatType::SocialEngineering,
            ThreatType::UnwantedSoftware,
            ThreatType::PotentiallyHarmful,
        ] {
            assert_eq!(ThreatType::from_code(&t.as_code()), t);
        }
    }

    #[test]
    fn threat_type_unknown_code_falls_to_other() {
        let t = ThreatType::from_code("some_future_category");
        assert_eq!(t, ThreatType::Other("some_future_category".to_string()));
        assert_eq!(t.as_code(), "some_future_category");
    }

    // ── normalize_path ──────────────────────────────────────────────────────

    #[test]
    fn normalize_path_empty_becomes_root() {
        assert_eq!(normalize_path(""), "/");
    }

    #[test]
    fn normalize_path_collapses_duplicate_slashes() {
        assert_eq!(normalize_path("/a//b///c"), "/a/b/c");
    }

    #[test]
    fn normalize_path_resolves_dot_dotdot() {
        assert_eq!(normalize_path("/a/./b/../c"), "/a/c");
        assert_eq!(normalize_path("/a/b/../../c"), "/c");
        assert_eq!(normalize_path("/../../etc"), "/etc");
    }

    #[test]
    fn normalize_path_preserves_trailing_slash() {
        assert_eq!(normalize_path("/a/b/"), "/a/b/");
        assert_eq!(normalize_path("/a/b"), "/a/b");
    }

    // ── canonical_expression_variants ───────────────────────────────────────

    #[test]
    fn variants_empty_for_missing_host() {
        let url = Url::parse("file:///etc/passwd").unwrap();
        // file:// scheme не даёт host (или даёт пустой) → variants пустой.
        let v = canonical_expression_variants(&url);
        assert!(v.is_empty() || v.iter().all(|e| !e.starts_with('/')));
    }

    #[test]
    fn variants_for_simple_url() {
        let url = Url::parse("http://example.com/path/to/page").unwrap();
        let v = canonical_expression_variants(&url);
        // hosts: example.com (нельзя срезать дальше — будет single component `com`)
        // paths: /path/to/page, /path/to/, /path/, /
        // 1 host × 4 paths = 4 variants.
        assert_eq!(v.len(), 4);
        assert!(v.contains(&"example.com/path/to/page".to_string()));
        assert!(v.contains(&"example.com/path/to/".to_string()));
        assert!(v.contains(&"example.com/path/".to_string()));
        assert!(v.contains(&"example.com/".to_string()));
    }

    #[test]
    fn variants_for_subdomain_url() {
        let url = Url::parse("http://a.b.example.com/").unwrap();
        let v = canonical_expression_variants(&url);
        // hosts: a.b.example.com, b.example.com, example.com (3 hosts).
        // paths: / (только root).
        // 3 × 1 = 3.
        assert_eq!(v.len(), 3);
        assert!(v.contains(&"a.b.example.com/".to_string()));
        assert!(v.contains(&"b.example.com/".to_string()));
        assert!(v.contains(&"example.com/".to_string()));
    }

    #[test]
    fn variants_for_query_string() {
        let url = Url::parse("http://example.com/page?id=1").unwrap();
        let v = canonical_expression_variants(&url);
        // path-варианты: /page?id=1, /page, /
        // 1 host × 3 paths = 3.
        assert_eq!(v.len(), 3);
        assert!(v.contains(&"example.com/page?id=1".to_string()));
        assert!(v.contains(&"example.com/page".to_string()));
        assert!(v.contains(&"example.com/".to_string()));
    }

    #[test]
    fn variants_lowercase_host() {
        let url = Url::parse("http://EXAMPLE.COM/Page").unwrap();
        let v = canonical_expression_variants(&url);
        assert!(v.iter().any(|e| e.starts_with("example.com")));
        // Path case-sensitivity — оригинал сохраняется.
        assert!(v.contains(&"example.com/Page".to_string()));
    }

    #[test]
    fn variants_idn_host_is_punycode() {
        let url = Url::parse("http://пример.рф/").unwrap();
        let v = canonical_expression_variants(&url);
        assert!(v.iter().any(|e| e.starts_with("xn--e1afmkfd.xn--p1ai")));
    }

    #[test]
    fn variants_normalized_path() {
        let url = Url::parse("http://example.com/a/./b/../c").unwrap();
        let v = canonical_expression_variants(&url);
        // После normalize_path: /a/c
        assert!(v.contains(&"example.com/a/c".to_string()));
    }

    #[test]
    fn variants_under_20_for_deeply_nested() {
        let url = Url::parse("http://a.b.c.d.e/f/g/h/i/j?q=1").unwrap();
        let v = canonical_expression_variants(&url);
        // Не должно быть больше 5 × 4 = 20.
        assert!(v.len() <= 20, "got {} variants", v.len());
    }

    // ── Storage CRUD ────────────────────────────────────────────────────────

    fn open() -> SafeBrowsingList {
        SafeBrowsingList::open_in_memory().unwrap()
    }

    #[test]
    fn empty_store_lookup_returns_none() {
        let store = open();
        let url = Url::parse("http://example.com/").unwrap();
        assert!(store.lookup_url(&url).unwrap().is_none());
        assert_eq!(store.count_total().unwrap(), 0);
    }

    #[test]
    fn add_hash_then_lookup_succeeds() {
        let store = open();
        let h = hash_expression("evil.com/malware.exe");
        store
            .add_hash("test-list", &h, &ThreatType::Malware, 100)
            .unwrap();
        let hit = store.lookup_hash(&h).unwrap();
        assert_eq!(
            hit,
            Some(("test-list".to_string(), ThreatType::Malware))
        );
    }

    #[test]
    fn add_hash_rejects_wrong_length() {
        let store = open();
        let short = [0u8; 16];
        let err = store
            .add_hash("test", &short, &ThreatType::Malware, 0)
            .unwrap_err();
        assert!(format!("{err:?}").contains("32-byte"), "got {err:?}");
    }

    #[test]
    fn add_url_indexes_canonical_form() {
        let store = open();
        let url = Url::parse("http://EVIL.COM/Bad?Q=1").unwrap();
        store
            .add_url("test", &url, &ThreatType::Malware, 0)
            .unwrap();
        // lookup точно по тому же URL должен дать hit.
        let hit = store.lookup_url(&url).unwrap();
        assert_eq!(hit, Some(("test".to_string(), ThreatType::Malware)));
    }

    #[test]
    fn lookup_matches_host_suffix() {
        let store = open();
        // Заносим запись по `evil.com/`.
        let h = hash_expression("evil.com/");
        store
            .add_hash("test", &h, &ThreatType::SocialEngineering, 0)
            .unwrap();

        // Запрос к субдомену `a.evil.com/anything` должен сматчить через
        // host-suffix вариант.
        let url = Url::parse("http://a.evil.com/anything").unwrap();
        let hit = store.lookup_url(&url).unwrap();
        assert_eq!(
            hit,
            Some(("test".to_string(), ThreatType::SocialEngineering))
        );
    }

    #[test]
    fn lookup_matches_path_prefix() {
        let store = open();
        // Заносим запись по `example.com/a/`.
        let h = hash_expression("example.com/a/");
        store
            .add_hash("test", &h, &ThreatType::Malware, 0)
            .unwrap();

        // Запрос к более глубокому пути матчит через path-trim.
        let url = Url::parse("http://example.com/a/b/c").unwrap();
        let hit = store.lookup_url(&url).unwrap();
        assert_eq!(hit, Some(("test".to_string(), ThreatType::Malware)));
    }

    #[test]
    fn lookup_unrelated_url_returns_none() {
        let store = open();
        let h = hash_expression("evil.com/");
        store
            .add_hash("test", &h, &ThreatType::Malware, 0)
            .unwrap();
        // good.com не должен сматчить.
        let url = Url::parse("http://good.com/").unwrap();
        assert!(store.lookup_url(&url).unwrap().is_none());
    }

    #[test]
    fn multiple_lists_first_match_wins() {
        let store = open();
        let h = hash_expression("evil.com/");
        store
            .add_hash("list-a", &h, &ThreatType::Malware, 0)
            .unwrap();
        store
            .add_hash("list-b", &h, &ThreatType::SocialEngineering, 0)
            .unwrap();
        let url = Url::parse("http://evil.com/").unwrap();
        // Любой из двух — допустимый результат, но `Some(_)` точно.
        assert!(store.lookup_url(&url).unwrap().is_some());
    }

    #[test]
    fn clear_list_removes_only_target_list() {
        let store = open();
        let h1 = hash_expression("evil-1.com/");
        let h2 = hash_expression("evil-2.com/");
        store
            .add_hash("list-a", &h1, &ThreatType::Malware, 0)
            .unwrap();
        store
            .add_hash("list-b", &h2, &ThreatType::Malware, 0)
            .unwrap();
        assert_eq!(store.count_total().unwrap(), 2);
        let removed = store.clear_list("list-a").unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.count_in("list-a").unwrap(), 0);
        assert_eq!(store.count_in("list-b").unwrap(), 1);
        assert_eq!(store.count_total().unwrap(), 1);
    }

    #[test]
    fn clear_all_drops_everything() {
        let store = open();
        for i in 0..5 {
            let h = hash_expression(&format!("evil-{i}.com/"));
            store
                .add_hash("list", &h, &ThreatType::Malware, 0)
                .unwrap();
        }
        assert_eq!(store.count_total().unwrap(), 5);
        let removed = store.clear_all().unwrap();
        assert_eq!(removed, 5);
        assert_eq!(store.count_total().unwrap(), 0);
    }

    #[test]
    fn add_hash_replaces_duplicate_in_same_list() {
        let store = open();
        let h = hash_expression("e.com/");
        store
            .add_hash("list", &h, &ThreatType::Malware, 100)
            .unwrap();
        // Re-add с другим threat_type — должен REPLACE.
        store
            .add_hash("list", &h, &ThreatType::SocialEngineering, 200)
            .unwrap();
        assert_eq!(store.count_in("list").unwrap(), 1);
        let hit = store.lookup_hash(&h).unwrap();
        assert_eq!(
            hit,
            Some(("list".to_string(), ThreatType::SocialEngineering))
        );
    }

    #[test]
    fn lookup_hash_rejects_wrong_length() {
        let store = open();
        let short = [0u8; 16];
        // Не Err, а None — invalid hash просто «не найден».
        assert!(store.lookup_hash(&short).unwrap().is_none());
    }

    // ── SafeBrowsingFilter ──────────────────────────────────────────────────

    #[test]
    fn filter_blocks_listed_url_and_reports_reason() {
        let store = Arc::new(open());
        let h = hash_expression("evil.com/");
        store
            .add_hash("local-block", &h, &ThreatType::Malware, 0)
            .unwrap();

        let filter = SafeBrowsingFilter::new(store);
        let url = Url::parse("http://evil.com/page").unwrap();
        let reason = filter.should_block(&url);
        assert_eq!(reason, Some("malware (local-block)".to_string()));
    }

    #[test]
    fn filter_passes_clean_url() {
        let store = Arc::new(open());
        let h = hash_expression("evil.com/");
        store
            .add_hash("list", &h, &ThreatType::Malware, 0)
            .unwrap();

        let filter = SafeBrowsingFilter::new(store);
        let url = Url::parse("http://good.com/").unwrap();
        assert!(filter.should_block(&url).is_none());
    }

    #[test]
    fn filter_matches_subdomain_through_host_suffix() {
        let store = Arc::new(open());
        let h = hash_expression("evil.com/");
        store
            .add_hash("list", &h, &ThreatType::Malware, 0)
            .unwrap();

        let filter = SafeBrowsingFilter::new(store);
        // Субдомен НЕ в списке, но host-suffix evil.com — есть → match.
        let url = Url::parse("http://tracker.evil.com/").unwrap();
        let reason = filter.should_block(&url);
        assert_eq!(reason, Some("malware (list)".to_string()));
    }

    #[test]
    fn filter_is_send_sync_and_object_safe() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SafeBrowsingFilter>();
        fn check_dyn(_f: &dyn RequestFilter) {}
        let store = Arc::new(open());
        let filter = SafeBrowsingFilter::new(store);
        check_dyn(&filter);
    }

    // ── PSL integration ────────────────────────────────────────────────

    use crate::psl::PslProvider;

    #[test]
    fn variants_with_psl_stops_at_etld_plus_one() {
        // Без PSL: a.b.example.co.uk → [a.b.example.co.uk, b.example.co.uk,
        //                              example.co.uk, co.uk] (4 хоста).
        // С PSL: остановка на example.co.uk, `co.uk` исключён.
        let url = Url::parse("http://a.b.example.co.uk/").unwrap();
        let psl = PslProvider::new();
        let v = canonical_expression_variants_with_psl(&url, Some(&psl));
        // 3 host-suffix варианта × 1 path (root) = 3.
        assert_eq!(v.len(), 3, "got {v:?}");
        assert!(v.contains(&"a.b.example.co.uk/".to_string()));
        assert!(v.contains(&"b.example.co.uk/".to_string()));
        assert!(v.contains(&"example.co.uk/".to_string()));
        assert!(!v.iter().any(|e| e == "co.uk/"), "co.uk leaked: {v:?}");
    }

    #[test]
    fn variants_without_psl_includes_couk_unfortunately() {
        // Без PSL `co.uk` остаётся в списке (документируем regression-у
        // относительно `with_psl` — caller, который хочет точности, обязан
        // подключить PSL).
        let url = Url::parse("http://a.b.example.co.uk/").unwrap();
        let v = canonical_expression_variants_with_psl(&url, None);
        assert!(v.iter().any(|e| e == "co.uk/"), "expected co.uk in {v:?}");
    }

    #[test]
    fn variants_with_psl_does_not_enumerate_bare_public_suffix() {
        // Если сам host — public suffix (например, прямой `co.uk`),
        // host-suffix enumeration не должна порождать `uk` или `com`-варианты.
        let url = Url::parse("http://co.uk/path").unwrap();
        let psl = PslProvider::new();
        let v = canonical_expression_variants_with_psl(&url, Some(&psl));
        // Только `co.uk` сам и его path-варианты — 1 host × 3 paths = 3.
        assert!(v.iter().all(|e| e.starts_with("co.uk")), "leak: {v:?}");
    }

    #[test]
    fn filter_with_psl_blocks_subdomain_via_etld_plus_one() {
        // Записан hit по `example.co.uk/`. Запрос к sub-домену
        // `tracker.example.co.uk/` должен сматчить через host-suffix.
        let store = Arc::new(open());
        let h = hash_expression("example.co.uk/");
        store
            .add_hash("list", &h, &ThreatType::Malware, 0)
            .unwrap();
        let filter = SafeBrowsingFilter::with_psl(store, Arc::new(PslProvider::new()));
        let url = Url::parse("http://tracker.example.co.uk/").unwrap();
        let reason = filter.should_block(&url);
        assert_eq!(reason, Some("malware (list)".to_string()));
    }

    #[test]
    fn filter_with_psl_does_not_match_through_public_suffix() {
        // Адверсарь записал hit по `co.uk/` (в `list-a`). Без PSL это
        // блокировало бы все `*.co.uk` сайты. С PSL — `co.uk` исключён
        // из host-suffix вариантов, поэтому невинный `good.co.uk` не
        // попадает под блокировку.
        let store = Arc::new(open());
        let evil_h = hash_expression("co.uk/");
        store
            .add_hash("bogus", &evil_h, &ThreatType::Malware, 0)
            .unwrap();
        let filter = SafeBrowsingFilter::with_psl(store, Arc::new(PslProvider::new()));
        let url = Url::parse("http://good.co.uk/").unwrap();
        assert!(
            filter.should_block(&url).is_none(),
            "co.uk shadow-entry blocked good.co.uk"
        );
    }
}

//! HSTS-интеграция для HttpClient: pre-request upgrade HTTP→HTTPS и
//! post-response парсинг `Strict-Transport-Security` header.
//!
//! Spec: <https://datatracker.ietf.org/doc/html/rfc6797>. Реализация policy
//! и storage — `lumen-storage::hsts::HstsStore` (через trait
//! `lumen-core::ext::HstsEnforcement`); этот модуль — только клиентская
//! интеграция в fetch-pipeline.
//!
//! Pure-функции принимают `&dyn HstsEnforcement` и `now_unix` — без скрытого
//! доступа к системному времени или БД, что делает их тестируемыми без
//! SQLite и без `Clock`-trait-а.

use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::ext::HstsEnforcement;
use lumen_core::url::Url;

use crate::hsts_preload::get_preload_list;

/// Парсер `Strict-Transport-Security` header (RFC 6797 §6.1.1). Возвращает
/// `(max_age, include_subdomains, preload)` или `None` если header невалиден.
///
/// Грамматика — `directive [;directive]*`; распознаются: `max-age=<n>`
/// (может быть в кавычках по §6.1.1), `includeSubDomains` (без значения,
/// case-insensitive), `preload` (без значения, case-insensitive по §RFC 8740).
/// Прочие директивы игнорируются (forward-compat). Без `max-age` → None.
fn parse_sts_header(text: &str) -> Option<(u64, bool, bool)> {
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

/// Текущее unix-время в секундах. Не тестируется напрямую — это thin wrapper
/// над `SystemTime::now()`. При невозможности получить время
/// (system clock before epoch — нереалистично) возвращает 0, что аналогично
/// «no HSTS entry is in the future» — все check-и провалятся, как при
/// fail-open отсутствующем store.
pub(crate) fn current_unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Если у `url` scheme=http и в HSTS-store есть запись для host (либо
/// родительского домена с includeSubDomains) — вернуть новый Url со схемой
/// https. Иначе None.
///
/// RFC 6797 §8.3 «URI Loading and Port Mapping»:
/// — если port == 80 (http default) — port убирается (станет default 443
///   для https);
/// — иначе custom-port сохраняется без изменений.
///
/// Path, query и fragment сохраняются как есть.
///
/// `Result` нужен только для проброса ошибки `Url::parse` финального URL —
/// `HstsEnforcement::is_https_only` сам Result не возвращает (fail-open).
pub(crate) fn maybe_upgrade_url_to_https(
    hsts: &dyn HstsEnforcement,
    url: &Url,
    now_unix: i64,
) -> lumen_core::Result<Option<Url>> {
    if url.scheme() != "http" {
        return Ok(None);
    }
    let host_ascii = url
        .host_ascii()
        .map_err(|e| lumen_core::Error::Network(e.to_string()))?;
    if host_ascii.is_empty() {
        return Ok(None);
    }

    // Проверяем HSTS Preload List первым (не требует редиректа, работает
    // без сохранённого policy). Затем проверяем HstsStore для динамически
    // полученных HSTS headers.
    let preload_list = get_preload_list();
    let should_upgrade = preload_list.is_preloaded(&host_ascii)
        || hsts.is_https_only(&host_ascii, now_unix);

    if !should_upgrade {
        return Ok(None);
    }
    let port_str = match url.port() {
        Some(80) => String::new(),
        Some(p) => format!(":{p}"),
        None => String::new(),
    };
    let pq = url.path_and_query();
    let fragment_str = match url.fragment() {
        Some(f) => format!("#{f}"),
        None => String::new(),
    };
    let new_url_str = format!(
        "https://{host}{port_str}{pq}{fragment_str}",
        host = url.host()
    );
    let new_url = Url::parse(&new_url_str)
        .map_err(|e| lumen_core::Error::Network(format!("HSTS upgrade parse: {e}")))?;
    Ok(Some(new_url))
}

/// Сохранить HSTS policy из ответа, если выполнено всё:
/// — ответ получен по HTTPS (RFC 6797 §8.1 — STS на HTTP игнорируется,
///   active attacker мог бы подделать header);
/// — присутствует заголовок `Strict-Transport-Security`;
/// — header валиден (есть `max-age=<число>`).
///
/// `max-age = 0` означает «снять HSTS» — обработка делегируется реализации
/// trait-а (для `HstsStore` это удаление entry, см. `HstsStore::upsert`).
///
/// `host` — ASCII / Punycode hostname (через `Url::host_ascii`).
/// Best-effort: ничего не возвращает — невалидные header-ы тихо
/// игнорируются (forward-compat для будущих директив).
pub(crate) fn process_sts_response(
    hsts: &dyn HstsEnforcement,
    scheme: &str,
    host: &str,
    headers: &[(String, String)],
    now_unix: i64,
) {
    if scheme != "https" {
        return;
    }
    let Some(raw) = find_header(headers, "strict-transport-security") else {
        return;
    };
    let Some((max_age, include_sub, preload)) = parse_sts_header(raw) else {
        return;
    };
    hsts.record_sts(host, max_age, include_sub, preload, now_unix);
}

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let name_lc = name.to_ascii_lowercase();
    headers
        .iter()
        .find(|(k, _)| k.to_ascii_lowercase() == name_lc)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    // ── parse_sts_header ─────────────────────────────────────────────────────

    #[test]
    fn parse_sts_basic() {
        let r = parse_sts_header("max-age=31536000; includeSubDomains; preload").unwrap();
        assert_eq!(r, (31_536_000, true, true));
    }

    #[test]
    fn parse_sts_without_optionals() {
        assert_eq!(parse_sts_header("max-age=3600").unwrap(), (3600, false, false));
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
    fn parse_sts_max_age_zero() {
        // max-age=0 — валидно (RFC 6797 §6.1.1, означает «снять HSTS»).
        assert_eq!(parse_sts_header("max-age=0").unwrap(), (0, false, false));
    }

    #[test]
    fn parse_sts_no_max_age_returns_none() {
        assert!(parse_sts_header("includeSubDomains").is_none());
        assert!(parse_sts_header("").is_none());
        assert!(parse_sts_header("preload; includeSubDomains").is_none());
    }

    #[test]
    fn parse_sts_unknown_directives_ignored() {
        // forward-compat: будущие директивы должны быть прозрачно проигнорированы.
        let r = parse_sts_header("max-age=100; future-directive=value; preload").unwrap();
        assert_eq!(r, (100, false, true));
    }

    // ── Mock HstsEnforcement для unit-тестов ─────────────────────────────────

    #[derive(Default)]
    struct MockHsts {
        /// Hosts, которые is_https_only вернёт true. Запись `("example.com", false)`
        /// — exact-match only; `("example.com", true)` — includeSubDomains.
        upgrade_hosts: Mutex<Vec<(String, bool)>>,
        records: Mutex<Vec<RecordedSts>>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct RecordedSts {
        host: String,
        max_age: u64,
        include_subdomains: bool,
        preload: bool,
        now_unix: i64,
    }

    impl MockHsts {
        fn with_host(host: &str, include_subdomains: bool) -> Self {
            let m = Self::default();
            m.upgrade_hosts
                .lock()
                .unwrap()
                .push((host.to_owned(), include_subdomains));
            m
        }

        fn records(&self) -> Vec<RecordedSts> {
            self.records.lock().unwrap().clone()
        }
    }

    impl HstsEnforcement for MockHsts {
        fn is_https_only(&self, host: &str, _now_unix: i64) -> bool {
            let list = self.upgrade_hosts.lock().unwrap();
            for (h, includes_sub) in list.iter() {
                if h == host {
                    return true;
                }
                if *includes_sub && host.ends_with(&format!(".{h}")) {
                    return true;
                }
            }
            false
        }

        fn record_sts(
            &self,
            host: &str,
            max_age: u64,
            include_subdomains: bool,
            preload: bool,
            now_unix: i64,
        ) {
            self.records.lock().unwrap().push(RecordedSts {
                host: host.to_owned(),
                max_age,
                include_subdomains,
                preload,
                now_unix,
            });
        }
    }

    // ── maybe_upgrade_url_to_https ────────────────────────────────────────────

    #[test]
    fn upgrade_http_to_https_when_host_in_store() {
        let hsts = MockHsts::with_host("example.com", false);
        let url = Url::parse("http://example.com/path").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap().unwrap();
        assert_eq!(upgraded.scheme(), "https");
        assert_eq!(upgraded.host(), "example.com");
        assert_eq!(upgraded.path(), "/path");
        assert_eq!(upgraded.port(), None);
    }

    #[test]
    fn no_upgrade_when_host_not_in_store() {
        let hsts = MockHsts::with_host("other.com", false);
        let url = Url::parse("http://example.com/").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap();
        assert!(upgraded.is_none());
    }

    #[test]
    fn no_upgrade_for_already_https_url() {
        let hsts = MockHsts::with_host("example.com", false);
        let url = Url::parse("https://example.com/").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap();
        assert!(upgraded.is_none());
    }

    #[test]
    fn upgrade_includes_subdomain_when_parent_has_include_subdomains() {
        // example.com помечен includeSubDomains → api.example.com тоже
        // обязан upgrade-иться (политика наследуется по longest-suffix-match,
        // это вычисляется в HstsStore::is_https_only / mock).
        let hsts = MockHsts::with_host("example.com", true);
        let url = Url::parse("http://api.example.com/v1").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap().unwrap();
        assert_eq!(upgraded.scheme(), "https");
        assert_eq!(upgraded.host(), "api.example.com");
    }

    #[test]
    fn upgrade_strips_explicit_port_80() {
        // RFC 6797 §8.3: явный :80 убирается при upgrade (станет default 443).
        let hsts = MockHsts::with_host("example.com", false);
        let url = Url::parse("http://example.com:80/").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap().unwrap();
        assert_eq!(upgraded.port(), None);
        assert_eq!(upgraded.as_str(), "https://example.com/");
    }

    #[test]
    fn upgrade_preserves_custom_port() {
        // Custom port не равный 80 — сохраняется (как https://host:8080/).
        // Поведение по §8.3.
        let hsts = MockHsts::with_host("example.com", false);
        let url = Url::parse("http://example.com:8080/").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap().unwrap();
        assert_eq!(upgraded.port(), Some(8080));
        assert_eq!(upgraded.scheme(), "https");
    }

    #[test]
    fn upgrade_preserves_query_and_fragment() {
        let hsts = MockHsts::with_host("example.com", false);
        let url = Url::parse("http://example.com/page?q=1&r=2#sec").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap().unwrap();
        assert_eq!(upgraded.scheme(), "https");
        assert_eq!(upgraded.path(), "/page");
        assert_eq!(upgraded.query(), Some("q=1&r=2"));
        assert_eq!(upgraded.fragment(), Some("sec"));
    }

    #[test]
    fn upgrade_uses_punycode_for_idn_lookup() {
        // IDN host передаётся в store как Punycode (host_ascii). Mock matches
        // только по ASCII-форме — тест проверяет, что upgrade всё равно
        // отрабатывает на Unicode-URL.
        let hsts = MockHsts::with_host("xn--d1abbgf6aiiy.xn--p1ai", false);
        let url = Url::parse("http://президент.рф/page").unwrap();
        let upgraded = maybe_upgrade_url_to_https(&hsts, &url, 0).unwrap().unwrap();
        assert_eq!(upgraded.scheme(), "https");
        // serialized Url хранит Unicode-форму для адресной строки.
        assert_eq!(upgraded.host(), "президент.рф");
    }

    // ── process_sts_response ──────────────────────────────────────────────────

    #[test]
    fn save_sts_from_https_response() {
        let hsts = MockHsts::default();
        let headers = vec![(
            "strict-transport-security".to_owned(),
            "max-age=31536000; includeSubDomains".to_owned(),
        )];
        process_sts_response(&hsts, "https", "example.com", &headers, 1000);
        let recs = hsts.records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].host, "example.com");
        assert_eq!(recs[0].max_age, 31_536_000);
        assert!(recs[0].include_subdomains);
        assert!(!recs[0].preload);
        assert_eq!(recs[0].now_unix, 1000);
    }

    #[test]
    fn ignore_sts_from_http_response() {
        // RFC 6797 §8.1: STS на HTTP-ответе должен быть проигнорирован.
        let hsts = MockHsts::default();
        let headers = vec![(
            "Strict-Transport-Security".to_owned(),
            "max-age=3600".to_owned(),
        )];
        process_sts_response(&hsts, "http", "example.com", &headers, 0);
        assert!(hsts.records().is_empty());
    }

    #[test]
    fn no_sts_header_is_noop() {
        let hsts = MockHsts::default();
        let headers = vec![("Content-Type".to_owned(), "text/html".to_owned())];
        process_sts_response(&hsts, "https", "example.com", &headers, 0);
        assert!(hsts.records().is_empty());
    }

    #[test]
    fn invalid_sts_header_is_noop() {
        // Без max-age — невалидно, не сохраняем.
        let hsts = MockHsts::default();
        let headers = vec![(
            "strict-transport-security".to_owned(),
            "includeSubDomains".to_owned(),
        )];
        process_sts_response(&hsts, "https", "example.com", &headers, 0);
        assert!(hsts.records().is_empty());
    }

    #[test]
    fn sts_max_age_zero_passed_through() {
        // max-age=0 valid, означает «снять HSTS». Мы передаём как есть —
        // обработка снятия лежит на HstsStore.upsert (она удалит entry).
        let hsts = MockHsts::default();
        let headers = vec![(
            "strict-transport-security".to_owned(),
            "max-age=0".to_owned(),
        )];
        process_sts_response(&hsts, "https", "example.com", &headers, 5000);
        let recs = hsts.records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].max_age, 0);
    }

    #[test]
    fn sts_header_lookup_case_insensitive() {
        // HTTP header names case-insensitive (RFC 7230 §3.2). У нас сервер
        // может прислать "Strict-Transport-Security" или "STRICT-...".
        let hsts = MockHsts::default();
        let headers = vec![(
            "STRICT-TRANSPORT-SECURITY".to_owned(),
            "max-age=42".to_owned(),
        )];
        process_sts_response(&hsts, "https", "example.com", &headers, 0);
        assert_eq!(hsts.records().len(), 1);
    }
}

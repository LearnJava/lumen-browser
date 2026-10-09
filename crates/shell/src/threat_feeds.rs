//! Загрузка открытых списков угроз в `safe_browsing.db`
//! (UX-SECURITY-UI, срез 3).
//!
//! Списки скачиваются целиком и проверяются локально ([`crate::threat_store`]):
//! адрес посещаемой страницы никуда не уходит. Источники — открытые текстовые
//! фиды «один адрес на строку»: URLhaus (вредоносное ПО), OpenPhish и
//! Phishing.Database (фишинг). Обновление — раз в [`REFRESH_INTERVAL_SECS`],
//! момент последней успешной загрузки хранится файлом `<data>/safe_browsing/<имя>.stamp`.
//! Сетевая ошибка не трогает прежние записи.

use std::time::{SystemTime, UNIX_EPOCH};

use lumen_core::url::Url;
use lumen_network::HttpClient;
use lumen_storage::{SafeBrowsingList, ThreatType};

/// Период обновления фидов.
const REFRESH_INTERVAL_SECS: i64 = 6 * 3600;

struct Feed {
    name: &'static str,
    url: &'static str,
    threat: ThreatType,
}

fn feeds() -> [Feed; 3] {
    [
        Feed {
            name: "urlhaus",
            url: "https://urlhaus.abuse.ch/downloads/text/",
            threat: ThreatType::Malware,
        },
        Feed {
            name: "openphish",
            url: "https://openphish.com/feed.txt",
            threat: ThreatType::SocialEngineering,
        },
        Feed {
            name: "phishing-database",
            url: "https://raw.githubusercontent.com/mitchellkrogza/Phishing.Database/master/phishing-links-ACTIVE-NOW.txt",
            threat: ThreatType::SocialEngineering,
        },
    ]
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn stamp_path(name: &str) -> std::path::PathBuf {
    crate::adblock::browser_data_dir().join("safe_browsing").join(format!("{name}.stamp"))
}

fn is_due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|t| now.saturating_sub(t) >= REFRESH_INTERVAL_SECS)
}

fn read_stamp(name: &str) -> Option<i64> {
    std::fs::read_to_string(stamp_path(name)).ok()?.trim().parse().ok()
}

/// Хэши http(s)-адресов из текста фида; комментарии (`#`), пустые строки и
/// не-web адреса пропускаются. Адрес без схемы (в Phishing.Database бывают
/// голые домены) читается как `http://`.
fn parse_feed(text: &str) -> Vec<[u8; 32]> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parsed = if line.contains("://") {
            Url::parse(line)
        } else {
            Url::parse(&format!("http://{line}"))
        };
        let Ok(url) = parsed else { continue };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        if let Ok(h) = SafeBrowsingList::url_hash(&url) {
            out.push(h);
        }
    }
    out
}

fn refresh_feed(list: &SafeBrowsingList, client: &HttpClient, feed: &Feed) {
    let now = now_unix();
    if !is_due(read_stamp(feed.name), now) {
        return;
    }
    let Ok(url) = Url::parse(feed.url) else { return };
    let body = match client.fetch_subresource_document(&url, false) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("safe-browsing: фид {} не загрузился: {e}", feed.name);
            return;
        }
    };
    let hashes = parse_feed(&String::from_utf8_lossy(&body));
    if hashes.is_empty() {
        // Пустой или нераспознанный ответ не должен стереть рабочий список.
        eprintln!("safe-browsing: фид {} пуст, прежний список сохранён", feed.name);
        return;
    }
    match list.replace_list(feed.name, &hashes, &feed.threat, now) {
        Ok(n) => {
            let stamp = stamp_path(feed.name);
            if let Some(dir) = stamp.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(stamp, now.to_string());
            eprintln!("safe-browsing: фид {} обновлён ({n} записей)", feed.name);
        }
        Err(e) => eprintln!("safe-browsing: фид {} не записан: {e}", feed.name),
    }
}

/// Обновить просроченные фиды. Блокирует поток — звать из фонового.
pub fn refresh(client: &HttpClient) {
    let Some(list) = crate::threat_store::global() else { return };
    for feed in &feeds() {
        refresh_feed(list, client, feed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_comments_and_non_web() {
        let h = parse_feed("# header\n\nhttp://a.example/x.exe\nftp://b.example/\nc.example/login\n");
        assert_eq!(h.len(), 2);
        let c = Url::parse("http://c.example/login").unwrap();
        assert!(h.contains(&SafeBrowsingList::url_hash(&c).unwrap()));
    }

    #[test]
    fn parsed_feed_is_found_by_lookup() {
        let l = SafeBrowsingList::open_in_memory().unwrap();
        let hashes = parse_feed("https://evil-feed.example/pay?id=1\n");
        l.replace_list("openphish", &hashes, &ThreatType::SocialEngineering, 0).unwrap();
        let hit = l.lookup_url(&Url::parse("https://evil-feed.example/pay?id=1").unwrap()).unwrap();
        assert_eq!(hit.map(|h| h.1), Some(ThreatType::SocialEngineering));
    }

    #[test]
    fn due_when_never_fetched_or_stale() {
        assert!(is_due(None, 100));
        assert!(!is_due(Some(100), 100 + REFRESH_INTERVAL_SECS - 1));
        assert!(is_due(Some(100), 100 + REFRESH_INTERVAL_SECS));
    }
}

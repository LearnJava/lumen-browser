//! Проверка адреса навигации по локальной базе Safe Browsing
//! (UX-SECURITY-UI, срез 2).
//!
//! База — `<data>/safe_browsing.db` ([`lumen_storage::SafeBrowsingList`]). Пока
//! пользователь не добавил записи (или база не открылась), проверка ничего не
//! находит — fail-open, как у `SafeBrowsingFilter`. «Всё равно перейти»
//! действует до конца сеанса и только для хоста.

use std::collections::HashSet;
use std::sync::Mutex;

use lumen_core::url::Url;
use lumen_storage::{SafeBrowsingList, ThreatType};

/// Общая база процесса; `None` в приватных режимах и если файл не открылся.
pub fn global() -> Option<&'static SafeBrowsingList> {
    static LIST: std::sync::OnceLock<Option<SafeBrowsingList>> = std::sync::OnceLock::new();
    LIST.get_or_init(open_default).as_ref()
}

fn open_default() -> Option<SafeBrowsingList> {
    let cfg = crate::config::global();
    if cfg.no_persistent_state || cfg.http_profile == lumen_network::HttpProfile::TorBrowser {
        return None;
    }
    let dir = crate::adblock::browser_data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    match SafeBrowsingList::open(dir.join("safe_browsing.db")) {
        Ok(l) => Some(l),
        Err(e) => {
            eprintln!("safe-browsing: база не открылась: {e}");
            None
        }
    }
}

fn allowed() -> &'static Mutex<HashSet<String>> {
    static SET: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    SET.get_or_init(Mutex::default)
}

/// Разрешить хост до конца сеанса («Всё равно перейти»).
pub fn allow_host(host: &str) {
    if let Ok(mut s) = allowed().lock() {
        s.insert(host.to_ascii_lowercase());
    }
}

/// Угроза для `url`: `(имя списка, тип)`; `None` — чисто, хост разрешён или
/// база недоступна.
pub fn check(url: &str) -> Option<(String, ThreatType)> {
    check_in(global()?, url)
}

fn check_in(list: &SafeBrowsingList, url: &str) -> Option<(String, ThreatType)> {
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    if allowed().lock().is_ok_and(|s| s.contains(&parsed.host().to_ascii_lowercase())) {
        return None;
    }
    match list.lookup_url(&parsed) {
        Ok(hit) => hit,
        Err(e) => {
            eprintln!("safe-browsing: проверка не удалась: {e}; пропускаем");
            None
        }
    }
}

/// Заголовок и пояснение экрана для типа угрозы.
pub fn describe(threat: &ThreatType, host: &str) -> (String, String) {
    match threat {
        ThreatType::SocialEngineering => (
            "Подозрение на фишинг".to_owned(),
            format!("Сайт {host} выдаёт себя за другой, чтобы выманить пароли и платёжные данные."),
        ),
        ThreatType::Malware => (
            "Сайт распространяет вредоносное ПО".to_owned(),
            format!("На сайте {host} замечено вредоносное ПО, оно может повредить устройство."),
        ),
        ThreatType::UnwantedSoftware => (
            "Сайт предлагает нежелательное ПО".to_owned(),
            format!("Сайт {host} навязывает программы, меняющие настройки браузера."),
        ),
        ThreatType::PotentiallyHarmful | ThreatType::Other(_) => (
            "Сайт может быть опасен".to_owned(),
            format!("Сайт {host} числится в списке подозрительных."),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_with(url: &str) -> SafeBrowsingList {
        let l = SafeBrowsingList::open_in_memory().unwrap();
        l.add_url("test", &Url::parse(url).unwrap(), &ThreatType::SocialEngineering, 0).unwrap();
        l
    }

    #[test]
    fn listed_url_is_reported() {
        let l = list_with("https://phish-a.example/login");
        let hit = check_in(&l, "https://phish-a.example/login").expect("hit");
        assert_eq!(hit.1, ThreatType::SocialEngineering);
        assert!(check_in(&l, "https://clean-a.example/").is_none());
    }

    #[test]
    fn allowed_host_and_non_web_schemes_pass() {
        let l = list_with("https://phish-b.example/");
        allow_host("PHISH-B.example");
        assert!(check_in(&l, "https://phish-b.example/").is_none());
        let l2 = list_with("https://phish-c.example/");
        assert!(check_in(&l2, "file:///phish-c.example/").is_none());
    }

    #[test]
    fn phishing_text_names_the_host() {
        let (title, msg) = describe(&ThreatType::SocialEngineering, "x.example");
        assert!(title.contains("фишинг"));
        assert!(msg.contains("x.example"));
    }
}

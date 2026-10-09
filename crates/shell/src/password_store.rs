//! Хранилище паролей браузера и решение «предлагать ли сохранить»
//! (UX-PASSWORDS, срез 2).
//!
//! Файлы лежат в `<data>/`: `logins.db` — шифрованные записи
//! ([`lumen_storage::SavedLogins`]), `logins.key` — 32 байта ключа AES-256-GCM.
//! Ключ лежит рядом с базой, поэтому защита — от случайного просмотра файла
//! базы и от переноса одной базы без ключа, но не от владельца каталога данных;
//! ключ из пароля профиля (`ProfileRegistry::unlock`) подключит срез 3.
//!
//! В приватных режимах (`--no-persistent-state`, Tor) хранилища нет вовсе, в
//! анонимном профиле предложение не показывается (проверяет вызывающий).

use lumen_storage::SavedLogins;

use crate::login_form::Credentials;

/// Предложение сохранить или обновить пароль, показанное в инфобаре.
#[derive(Clone, PartialEq, Eq)]
pub struct LoginOffer {
    pub origin: String,
    pub username: String,
    pub password: String,
    /// Для этого логина на сайте уже есть другой пароль.
    pub update: bool,
}

impl std::fmt::Debug for LoginOffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginOffer")
            .field("origin", &self.origin)
            .field("username", &self.username)
            .field("update", &self.update)
            .finish_non_exhaustive()
    }
}

/// `scheme://host[:port]` для `http(s)`-адреса; другие схемы пароли не хранят.
pub fn origin_of(url: &str) -> Option<String> {
    let parsed = lumen_core::url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_empty() {
        return None;
    }
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some(format!("{}://{}{}", parsed.scheme(), parsed.host(), port))
}

/// Нужно ли предлагать сохранение для введённых данных.
///
/// `None` — сайт в списке «никогда», такой пароль уже сохранён или хранилище
/// не отвечает (тогда лучше промолчать, чем показать кнопку, которая не сработает).
pub fn plan_offer(store: &SavedLogins, origin: &str, creds: &Credentials) -> Option<LoginOffer> {
    if store.is_never_save(origin).ok()? {
        return None;
    }
    let update = match store.get(origin, &creds.username).ok()? {
        Some(saved) if saved.password == creds.password => return None,
        Some(_) => true,
        None => false,
    };
    Some(LoginOffer {
        origin: origin.to_owned(),
        username: creds.username.clone(),
        password: creds.password.clone(),
        update,
    })
}

/// Общее хранилище процесса; `None` в приватных режимах и если файлы не открылись.
pub fn global() -> Option<&'static SavedLogins> {
    static STORE: std::sync::OnceLock<Option<SavedLogins>> = std::sync::OnceLock::new();
    STORE.get_or_init(open_default).as_ref()
}

fn open_default() -> Option<SavedLogins> {
    let cfg = crate::config::global();
    if cfg.no_persistent_state || cfg.http_profile == lumen_network::HttpProfile::TorBrowser {
        return None;
    }
    let dir = crate::adblock::browser_data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let key = load_or_create_key(&dir.join("logins.key"))?;
    match SavedLogins::open(dir.join("logins.db"), key) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("passwords: хранилище не открылось: {e}");
            None
        }
    }
}

fn load_or_create_key(path: &std::path::Path) -> Option<[u8; lumen_storage::profile_vault::KEY_LEN]> {
    if let Ok(bytes) = std::fs::read(path) {
        return bytes.try_into().ok();
    }
    let key = lumen_storage::profile_vault::generate_storage_key().ok()?;
    std::fs::write(path, key).ok()?;
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login_form::LoginKind;

    const SITE: &str = "https://example.com";

    fn creds(user: &str, pw: &str) -> Credentials {
        Credentials { username: user.into(), password: pw.into(), kind: LoginKind::SignIn }
    }

    fn store() -> SavedLogins {
        SavedLogins::open_in_memory([1; 32]).unwrap()
    }

    #[test]
    fn origin_keeps_port_and_rejects_other_schemes() {
        assert_eq!(origin_of("https://a.test/x?y").as_deref(), Some("https://a.test"));
        assert_eq!(origin_of("http://a.test:8080/").as_deref(), Some("http://a.test:8080"));
        assert_eq!(origin_of("file:///c:/x.html"), None);
        assert_eq!(origin_of("about:blank"), None);
    }

    #[test]
    fn new_login_is_offered_as_new() {
        let o = plan_offer(&store(), SITE, &creds("anna", "pw")).unwrap();
        assert!(!o.update);
        assert_eq!((o.origin.as_str(), o.username.as_str()), (SITE, "anna"));
    }

    #[test]
    fn same_password_is_not_offered() {
        let s = store();
        s.save(SITE, "anna", "pw", 1).unwrap();
        assert!(plan_offer(&s, SITE, &creds("anna", "pw")).is_none());
    }

    #[test]
    fn changed_password_is_offered_as_update() {
        let s = store();
        s.save(SITE, "anna", "old", 1).unwrap();
        assert!(plan_offer(&s, SITE, &creds("anna", "new")).unwrap().update);
    }

    #[test]
    fn never_save_site_is_silent() {
        let s = store();
        s.never_save(SITE).unwrap();
        assert!(plan_offer(&s, SITE, &creds("anna", "pw")).is_none());
    }

    #[test]
    fn offer_debug_hides_password() {
        let o = plan_offer(&store(), SITE, &creds("anna", "topsecret")).unwrap();
        assert!(!format!("{o:?}").contains("topsecret"));
    }
}

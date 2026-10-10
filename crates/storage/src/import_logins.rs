//! UX-IMPORT: пароли из Chrome/Edge (`Login Data` + ключ из `Local State`).
//!
//! Схема Chromium на Windows: `Local State` → `os_crypt.encrypted_key` (base64,
//! префикс `DPAPI`, остальное — ключ AES-256, обёрнутый DPAPI текущего
//! пользователя); пароль в `logins.password_value` — `v10`/`v11` + nonce(12) +
//! шифртекст + тег(16), AES-256-GCM. Записи `v20` (app-bound encryption)
//! расшифровать вне процесса Chrome нельзя — они пропускаются и считаются.
//! Firefox (NSS `key4.db`) — отдельный срез.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use lumen_core::{Error, Result};
use rusqlite::{Connection, OpenFlags};

use crate::saved_logins::SavedLogins;

const NONCE_LEN: usize = 12;
const DPAPI_PREFIX: &[u8] = b"DPAPI";

/// Пароль из чужого браузера, уже расшифрованный.
#[derive(Clone, PartialEq, Eq)]
pub struct ImportedLogin {
    /// Origin формы (`https://host`), как хранит `SavedLogins`.
    pub origin: String,
    /// Имя пользователя.
    pub username: String,
    /// Пароль в открытом виде; живёт только до записи в хранилище.
    pub password: String,
}

impl std::fmt::Debug for ImportedLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportedLogin")
            .field("origin", &self.origin)
            .field("username", &self.username)
            .field("password", &"<скрыт>")
            .finish()
    }
}

/// Результат чтения: расшифрованное и число пропущенных записей.
#[derive(Debug, Default)]
pub struct LoginsRead {
    /// Успешно расшифрованные записи.
    pub logins: Vec<ImportedLogin>,
    /// Не расшифровано (`v20`, повреждено, пустой пароль/имя).
    pub skipped: usize,
}

/// Итог записи в хранилище.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoginsReport {
    /// Новых записей.
    pub added: usize,
    /// Уже было с таким же или другим паролем — не тронуто.
    pub kept: usize,
}

fn err(what: impl std::fmt::Display) -> Error {
    Error::Storage(format!("import logins: {what}"))
}

/// Стандартный base64 (RFC 4648) без зависимостей; пробелы игнорируются.
pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace()) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Расшифровать `password_value` ключом AES-256 (`v10`/`v11`).
/// `None` — другой формат (`v20`, старый DPAPI-блоб) или неверный ключ.
pub fn decrypt_chromium_password(key: &[u8; 32], blob: &[u8]) -> Option<String> {
    let body = blob.strip_prefix(b"v10").or_else(|| blob.strip_prefix(b"v11"))?;
    if body.len() < NONCE_LEN + 16 {
        return None;
    }
    let (nonce, ct) = body.split_at(NONCE_LEN);
    let plain = Aes256Gcm::new(key.into()).decrypt(Nonce::from_slice(nonce), ct).ok()?;
    String::from_utf8(plain).ok()
}

/// Получить ключ AES из содержимого `Local State`.
pub fn chromium_key_from_local_state(local_state_json: &str) -> Result<[u8; 32]> {
    let v: serde_json::Value = serde_json::from_str(local_state_json).map_err(err)?;
    let b64 = v
        .pointer("/os_crypt/encrypted_key")
        .and_then(|k| k.as_str())
        .ok_or_else(|| err("в Local State нет os_crypt.encrypted_key"))?;
    let raw = base64_decode(b64).ok_or_else(|| err("encrypted_key не base64"))?;
    let wrapped = raw
        .strip_prefix(DPAPI_PREFIX)
        .ok_or_else(|| err("encrypted_key без префикса DPAPI"))?;
    let key = dpapi_unprotect(wrapped)?;
    key.try_into().map_err(|k: Vec<u8>| err(format!("ключ {} байт вместо 32", k.len())))
}

#[cfg(windows)]
fn dpapi_unprotect(data: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: `input` указывает на живой срез на время вызова (API его не
    // изменяет, `*mut` — особенность сигнатуры); `out` заполняется системой, а
    // выделенный ею буфер читается ровно `cbData` байт и освобождается LocalFree.
    unsafe {
        let ok = CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut out,
        );
        if ok == 0 || out.pbData.is_null() {
            return Err(err("CryptUnprotectData отказал (другой пользователь Windows?)"));
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Ok(v)
    }
}

#[cfg(not(windows))]
fn dpapi_unprotect(_data: &[u8]) -> Result<Vec<u8>> {
    Err(err("DPAPI доступен только в Windows"))
}

/// Прочитать пароли профиля Chrome/Edge: `profile_dir` — каталог `Default` /
/// `Profile N`; `Local State` лежит на уровень выше.
pub fn read_chromium_logins(profile_dir: &Path) -> Result<LoginsRead> {
    let state_path = profile_dir
        .parent()
        .map(|p| p.join("Local State"))
        .ok_or_else(|| err("у профиля нет родительского каталога"))?;
    let state = std::fs::read_to_string(&state_path)
        .map_err(|e| err(format!("{}: {e}", state_path.display())))?;
    let key = chromium_key_from_local_state(&state)?;
    read_chromium_logins_with_key(&profile_dir.join("Login Data"), &key)
}

/// Читает `Login Data` готовым ключом (отделено от DPAPI ради тестов).
pub fn read_chromium_logins_with_key(db: &Path, key: &[u8; 32]) -> Result<LoginsRead> {
    let dir = std::env::temp_dir().join(format!("lumen-import-logins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(err)?;
    let copy = dir.join("Login Data");
    let result = (|| {
        std::fs::copy(db, &copy).map_err(|e| err(format!("{}: {e}", db.display())))?;
        let conn = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
        let mut stmt = conn
            .prepare(
                "SELECT origin_url, username_value, password_value FROM logins
                 WHERE blacklisted_by_user = 0",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?))
            })
            .map_err(err)?;
        let mut read = LoginsRead::default();
        for row in rows {
            let (url, username, blob) = row.map_err(err)?;
            let origin = origin_of(&url);
            match (origin, decrypt_chromium_password(key, &blob)) {
                (Some(origin), Some(password)) if !password.is_empty() && !username.is_empty() => {
                    read.logins.push(ImportedLogin { origin, username, password });
                }
                _ => read.skipped += 1,
            }
        }
        Ok(read)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// `scheme://host[:port]` из URL; `None` для не-http(s).
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

/// Записать пароли; существующая пара `(origin, username)` не перезаписывается.
pub fn import_logins(
    store: &SavedLogins,
    items: &[ImportedLogin],
    now_unix: i64,
) -> Result<LoginsReport> {
    let mut rep = LoginsReport::default();
    for l in items {
        if store.get(&l.origin, &l.username)?.is_some() {
            rep.kept += 1;
            continue;
        }
        store.save(&l.origin, &l.username, &l.password, now_unix)?;
        rep.added += 1;
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    fn encrypt(key: &[u8; 32], prefix: &[u8], plain: &str) -> Vec<u8> {
        let nonce = [3u8; NONCE_LEN];
        let ct = Aes256Gcm::new(key.into())
            .encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
            .unwrap();
        [prefix, &nonce, &ct].concat()
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(base64_decode("Zm9v\nYmFy").unwrap(), b"foobar");
        assert!(base64_decode("a*b").is_none());
    }

    #[test]
    fn decrypt_v10_and_rejects_others() {
        let blob = encrypt(&KEY, b"v10", "пароль-1");
        assert_eq!(decrypt_chromium_password(&KEY, &blob).as_deref(), Some("пароль-1"));
        assert_eq!(decrypt_chromium_password(&[8; 32], &blob), None, "чужой ключ");
        assert_eq!(decrypt_chromium_password(&KEY, &encrypt(&KEY, b"v20", "x")), None);
        assert_eq!(decrypt_chromium_password(&KEY, b"v10short"), None);
    }

    #[test]
    fn origin_extraction() {
        assert_eq!(origin_of("https://a.test/login?x=1").as_deref(), Some("https://a.test"));
        assert_eq!(origin_of("http://a.test:8080/").as_deref(), Some("http://a.test:8080"));
        assert_eq!(origin_of("android://hash@com.app/"), None);
    }

    #[test]
    fn reads_login_data_and_imports() {
        let dir = std::env::temp_dir().join(format!("lumen-logins-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("Login Data");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE logins(origin_url TEXT, username_value TEXT, password_value BLOB,
               blacklisted_by_user INTEGER DEFAULT 0);",
        )
        .unwrap();
        let ins = "INSERT INTO logins(origin_url, username_value, password_value, blacklisted_by_user)
                   VALUES (?1, ?2, ?3, ?4)";
        conn.execute(ins, rusqlite::params!["https://a.test/login", "ann", encrypt(&KEY, b"v10", "pw-a"), 0]).unwrap();
        conn.execute(ins, rusqlite::params!["https://b.test/", "bob", encrypt(&KEY, b"v20", "x"), 0]).unwrap();
        conn.execute(ins, rusqlite::params!["https://c.test/", "", encrypt(&KEY, b"v10", "y"), 0]).unwrap();
        conn.execute(ins, rusqlite::params!["https://d.test/", "dan", Vec::<u8>::new(), 1]).unwrap();
        drop(conn);

        let read = read_chromium_logins_with_key(&db, &KEY).unwrap();
        assert_eq!(read.logins.len(), 1);
        assert_eq!(read.logins[0].origin, "https://a.test");
        assert_eq!(read.skipped, 2, "v20 и пустое имя; blacklisted не считается");

        let store = SavedLogins::open_in_memory([9u8; 32]).unwrap();
        store.save("https://a.test", "zed", "other", 1).unwrap();
        let rep = import_logins(&store, &read.logins, 5).unwrap();
        assert_eq!((rep.added, rep.kept), (1, 0));
        let rep = import_logins(&store, &read.logins, 6).unwrap();
        assert_eq!((rep.added, rep.kept), (0, 1));
        assert_eq!(store.get("https://a.test", "ann").unwrap().unwrap().password, "pw-a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip_via_local_state() {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
        let input = CRYPT_INTEGER_BLOB { cbData: 32, pbData: KEY.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: тест; входной срез жив, выходной буфер копируется и освобождается.
        let wrapped = unsafe {
            let ok = CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                &mut out,
            );
            assert_ne!(ok, 0);
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData as _);
            v
        };
        let raw = [DPAPI_PREFIX, &wrapped].concat();
        let json = format!(r#"{{"os_crypt":{{"encrypted_key":"{}"}}}}"#, base64_encode(&raw));
        assert_eq!(chromium_key_from_local_state(&json).unwrap(), KEY);
        assert!(chromium_key_from_local_state("{}").is_err());
    }

    #[cfg(windows)]
    fn base64_encode(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for ch in data.chunks(3) {
            let n = ch.iter().enumerate().fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
            for i in 0..4 {
                if i <= ch.len() {
                    s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }
}

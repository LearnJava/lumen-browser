//! BUG-1803: системный HTTP-прокси Windows (WinINET) как дефолт.
//!
//! Chrome и Firefox берут прокси из `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`
//! (`ProxyEnable` / `ProxyServer` / `ProxyOverride`). Lumen до этого среза не читал
//! системные настройки вовсе: единственным механизмом был `proxy = "…"` в
//! `data/fingerprint.toml` (BUG-1210), поэтому у пользователя с включённым
//! системным прокси браузер ходил напрямую и сайт, доступный только через
//! прокси, открыть было нельзя.
//!
//! Приоритет задаёт `cli_args`: `--proxy` / `proxy =` из `fingerprint.toml`
//! сильнее системного значения — система здесь только дефолт. Tor-режим
//! (`--tor`) системный прокси не берёт.
//!
//! PAC (`AutoConfigURL`) не исполняется — это отдельный срез (BUG-1803
//! «Затем — PAC/автоопределение»); при одном лишь PAC системного прокси нет.
//!
//! Отказной выключатель: `LUMEN_NO_SYSTEM_PROXY=1` в окружении — автопрогоны
//! (WPT, graphic tests) и отладка получают прежнее прямое соединение.
//!
//! Доработка (2026-11-18): список обхода применяется, только когда системный
//! прокси реально выбран маршрутом ([`note_applied`] из `cli_args`) — иначе
//! `ProxyOverride` под `--tor`/`--proxy`/fingerprint-прокси пробивал бы
//! прямые соединения мимо socks5-туннеля. `socks=` без http(s)-записи
//! становится `socks5_proxy`; `<local>` и `prefix*` из `ProxyOverride`
//! доезжают до матчера `no_proxy_for_host` (он их понимает с этого же среза).

use std::sync::atomic::{AtomicBool, Ordering};

/// Системные настройки прокси в виде полей [`crate::config::FingerprintProfile`].
#[derive(Debug, Default)]
pub struct SystemProxy {
    /// `http(s)://…` — значение `FingerprintProfile::proxy`.
    pub proxy: Option<String>,
    /// `socks5://…` — значение `FingerprintProfile::socks5_proxy`; заполняется
    /// только когда http(s)-прокси в `ProxyServer` нет вообще — иначе SOCKS5
    /// перехватил бы весь трафик, чего WinINET не делает.
    pub socks5_proxy: Option<String>,
}

static APPLIED: AtomicBool = AtomicBool::new(false);

/// Отметить, что системный прокси выбран маршрутом. Зовётся из `cli_args`
/// при применении — до этого `ProxyOverride` не должен ни на что влиять.
pub fn note_applied() {
    APPLIED.store(true, Ordering::Relaxed);
}

/// Прокси из системных настроек.
#[cfg(target_os = "windows")]
pub fn system_proxy() -> Option<SystemProxy> {
    let settings = read_internet_settings();
    if !settings.enabled {
        return None;
    }
    parse_proxy_endpoints(settings.server.as_deref()?)
}

#[cfg(not(target_os = "windows"))]
pub fn system_proxy() -> Option<SystemProxy> {
    None
}

/// Список «ходи напрямую» из `ProxyOverride`, уже нормализованный под
/// сопоставление [`lumen_network::no_proxy_for_host`]: точный хост/домен с
/// поддоменами, `<local>` (intranet-имя без точки) и `prefix*` (IP-диапазоны
/// вида `172.16.*`). Применяется, только когда системный прокси реально
/// маршрутизирует трафик ([`note_applied`]).
pub fn bypass_patterns() -> &'static [String] {
    static PATTERNS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
        if !APPLIED.load(Ordering::Relaxed) {
            return Vec::new();
        }
        #[cfg(target_os = "windows")]
        {
            parse_proxy_override(read_internet_settings().overrides.as_deref().unwrap_or_default())
        }
        #[cfg(not(target_os = "windows"))]
        {
            Vec::new()
        }
    })
}

/// URL прокси без userinfo — для stderr-лога (креды в лог не пишем).
#[must_use]
pub fn redact_for_log(url: &str) -> String {
    match url.find("://") {
        Some(scheme_end) => {
            let rest = &url[scheme_end + 3..];
            match rest.find('@') {
                Some(at) => format!("{}://***@{}", &url[..scheme_end], &rest[at + 1..]),
                None => url.to_string(),
            }
        }
        None => url.to_string(),
    }
}

/// `host:port` либо `http=host:port;https=host:port;socks=…` → поля
/// [`FingerprintProfile`](crate::config::FingerprintProfile).
///
/// `https=` приоритетнее `http=` (страницы в Lumen почти всегда https), при
/// отсутствии `=` вся строка считается адресом одного прокси для всех схем.
/// `socks=` берётся в `socks5_proxy` только когда http(s)-записи нет —
/// WinINET шлёт через socks и http-трафик, но смешивать два системных
/// прокси в одном профиле мы не будем.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))] // вызывающий код — cfg(windows)
pub(crate) fn parse_proxy_endpoints(server: &str) -> Option<SystemProxy> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    let mut http: Option<&str> = None;
    let mut socks: Option<&str> = None;
    if server.contains('=') {
        for entry in server.split(';') {
            let Some((scheme, value)) = entry.split_once('=') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match scheme.trim().to_ascii_lowercase().as_str() {
                // `https=` сильнее `http=` независимо от порядка записи.
                "https" => http = Some(value),
                "http" if http.is_none() => http = Some(value),
                "socks" | "socks5" => socks = socks.or(Some(value)),
                _ => {}
            }
        }
    } else {
        http = Some(server);
    }
    let proxy = http.and_then(to_http_proxy_url);
    if proxy.is_some() {
        return Some(SystemProxy {
            proxy,
            socks5_proxy: None,
        });
    }
    let socks5_proxy = socks.and_then(to_socks5_url);
    if socks5_proxy.is_some() {
        return Some(SystemProxy {
            proxy: None,
            socks5_proxy,
        });
    }
    None
}

/// `host:port`/`scheme://…` → `http(s)://…`; прочие схемы отбрасываем, чтобы
/// не получить молча «без прокси».
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn to_http_proxy_url(address: &str) -> Option<String> {
    let has_scheme = address.contains("://");
    let url = if has_scheme {
        address.to_string()
    } else {
        format!("http://{address}")
    };
    // `parse_http_proxy` в `config` принимает только `http(s)://`.
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url)
    } else {
        None
    }
}

/// `host:port`/`scheme://…` → `socks5://…`; `socks4`/`ftp` и прочее — мимо.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn to_socks5_url(address: &str) -> Option<String> {
    let has_scheme = address.contains("://");
    let url = if has_scheme {
        address.to_string()
    } else {
        format!("socks5://{address}")
    };
    if url.starts_with("socks5://") || url.starts_with("socks5h://") {
        Some(url.replacen("socks5h://", "socks5://", 1))
    } else {
        None
    }
}

/// `ProxyOverride` (`localhost;127.*;*.example.ru;<local>`) → шаблоны для
/// [`lumen_network::install_no_proxy_patterns`].
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn parse_proxy_override(overrides: &str) -> Vec<String> {
    let mut out = Vec::new();
    for entry in overrides.split(';') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        // `<local>` — intranet-имена без точки; матчится в `no_proxy_for_host`.
        if entry.eq_ignore_ascii_case("<local>") {
            if !out.iter().any(|p| p == "<local>") {
                out.push("<local>".to_string());
            }
            continue;
        }
        // Семантика реестра допускает `*.` перед доменом — это ровно «домен и
        // поддомены», то же, что у сопоставителя. Хвостовой `*` (`172.16.*`)
        // — префикс-диапазон; прочие `*`/`/`/`:` не выражаются.
        let pattern = entry.strip_prefix("*.").unwrap_or(entry);
        if pattern.contains('/') || pattern.contains(':') {
            continue;
        }
        if pattern == "*"
            || pattern.matches('*').count() > 1
            || (pattern.contains('*') && !pattern.ends_with('*'))
        {
            continue;
        }
        let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
        if !pattern.is_empty() && !out.iter().any(|p| p == &pattern) {
            out.push(pattern);
        }
    }
    out
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct InternetSettings {
    enabled: bool,
    server: Option<String>,
    overrides: Option<String>,
    auto_config_url: Option<String>,
}

#[cfg(target_os = "windows")]
fn read_internet_settings() -> InternetSettings {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_READ, REG_DWORD, REG_EXPAND_SZ, REG_SZ, RegCloseKey,
        RegOpenKeyExW, RegQueryValueExW,
    };

    const PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(windows_sys::Win32::System::Registry::HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: ключ открыт в `Key::open` и ещё не закрыт.
            let _ = unsafe { RegCloseKey(self.0) };
        }
    }

    impl Key {
        fn open(path: &str) -> Option<Self> {
            let mut h = 0;
            let p = wide(path);
            // SAFETY: `p` — NUL-терминированная UTF-16 строка, `h` — валидный out-указатель.
            let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, p.as_ptr(), 0, KEY_READ, &mut h) };
            (rc == 0).then_some(Self(h))
        }

        fn dword(&self, name: &str) -> Option<u32> {
            let n = wide(name);
            let mut ty = 0u32;
            let mut data = 0u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            // SAFETY: `n` — NUL-терминированное имя, `data` вмещает `size` байт.
            let rc = unsafe {
                RegQueryValueExW(
                    self.0,
                    n.as_ptr(),
                    std::ptr::null(),
                    &mut ty,
                    (&raw mut data).cast::<u8>(),
                    &mut size,
                )
            };
            (rc == 0 && ty == REG_DWORD).then_some(data)
        }

        fn string(&self, name: &str) -> Option<String> {
            let n = wide(name);
            let mut ty = 0u32;
            let mut size = 0u32;
            // SAFETY: запрос размера (lpdata = null); указатели валидны на время вызова.
            let rc = unsafe {
                RegQueryValueExW(self.0, n.as_ptr(), std::ptr::null(), &mut ty, std::ptr::null_mut(), &mut size)
            };
            if rc != 0 || (ty != REG_SZ && ty != REG_EXPAND_SZ) || size == 0 || size > 65_536 {
                return None;
            }
            let mut buf = vec![0u16; (size as usize).div_ceil(2)];
            let mut size2 = (buf.len() * 2) as u32;
            // SAFETY: `buf` вмещает `size2` байт, ядро записывает не больше.
            let rc = unsafe {
                RegQueryValueExW(
                    self.0,
                    n.as_ptr(),
                    std::ptr::null(),
                    &mut ty,
                    buf.as_mut_ptr().cast::<u8>(),
                    &mut size2,
                )
            };
            if rc != 0 {
                return None;
            }
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let s = String::from_utf16_lossy(&buf[..len]);
            Some(if ty == REG_EXPAND_SZ { expand_env(&s) } else { s })
        }
    }

    fn expand_env(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            let tail = &rest[i + 1..];
            match tail.find('%') {
                Some(j) => {
                    match std::env::var(&tail[..j]) {
                        Ok(v) => out.push_str(&v),
                        Err(_) => out.push_str(&rest[i..i + j + 2]),
                    }
                    rest = &tail[j + 1..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    let Some(key) = Key::open(PATH) else {
        return InternetSettings::default();
    };
    InternetSettings {
        enabled: key.dword("ProxyEnable").unwrap_or(0) != 0,
        server: key.string("ProxyServer"),
        overrides: key.string("ProxyOverride"),
        auto_config_url: key.string("AutoConfigURL"),
    }
}

/// Диагностика маршрута: `Some` — системный прокси выбран, PAC-only — `None`
/// с пояснением в stderr (иначе «включён прокси, а Lumen молчит» неотличим
/// от «прокси выключен»).
#[cfg(target_os = "windows")]
pub fn describe_disabled() -> Option<String> {
    let settings = read_internet_settings();
    if !settings.enabled
        && settings
            .auto_config_url
            .as_deref()
            .is_some_and(|u| !u.trim().is_empty())
    {
        return Some(format!(
            "системный прокси задан только PAC ({}), исполнение PAC не поддерживается — соединение прямое",
            settings.auto_config_url.unwrap_or_default()
        ));
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn describe_disabled() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoints(server: &str) -> SystemProxy {
        parse_proxy_endpoints(server).unwrap_or_default()
    }

    #[test]
    fn plain_server_gets_http_scheme() {
        assert_eq!(
            endpoints("127.0.0.1:12334").proxy.as_deref(),
            Some("http://127.0.0.1:12334")
        );
    }

    #[test]
    fn server_with_scheme_is_kept() {
        assert_eq!(
            endpoints("http://user:pass@proxy.local:8080").proxy.as_deref(),
            Some("http://user:pass@proxy.local:8080")
        );
    }

    #[test]
    fn per_scheme_list_prefers_https() {
        let p = endpoints("http=proxy:3128;https=tls-proxy:3129");
        assert_eq!(p.proxy.as_deref(), Some("http://tls-proxy:3129"));
        assert!(p.socks5_proxy.is_none());
        assert_eq!(
            endpoints("http=proxy:3128").proxy.as_deref(),
            Some("http://proxy:3128")
        );
    }

    #[test]
    fn socks_only_becomes_socks5_proxy() {
        let p = endpoints("socks=proxy:1080");
        assert!(p.proxy.is_none());
        assert_eq!(p.socks5_proxy.as_deref(), Some("socks5://proxy:1080"));
    }

    #[test]
    fn http_proxy_wins_over_socks_entry() {
        // WinINET шлёт через http-прокси; socks= при наличии http(s) не трогаем.
        let p = endpoints("socks=proxy:1080;https=tls-proxy:3129");
        assert_eq!(p.proxy.as_deref(), Some("http://tls-proxy:3129"));
        assert!(p.socks5_proxy.is_none());
    }

    #[test]
    fn socks5h_scheme_is_normalized() {
        let p = endpoints("socks=socks5h://proxy:1080");
        assert_eq!(p.socks5_proxy.as_deref(), Some("socks5://proxy:1080"));
    }

    #[test]
    fn empty_and_unsupported_values_are_rejected() {
        assert!(parse_proxy_endpoints("").is_none());
        assert!(parse_proxy_endpoints("ftp=proxy:21").is_none());
        assert!(parse_proxy_endpoints("socks4=proxy:1080").is_none());
        assert!(endpoints("ftp=proxy:21").proxy.is_none());
    }

    #[test]
    fn overrides_keep_local_and_ip_prefixes() {
        assert_eq!(
            parse_proxy_override("localhost;127.*;*.Profi.ru;<local>;intranet"),
            vec![
                "localhost".to_string(),
                "127.*".to_string(),
                "profi.ru".to_string(),
                "<local>".to_string(),
                "intranet".to_string()
            ]
        );
    }

    #[test]
    fn overrides_drop_mid_wildcards_and_ports() {
        assert_eq!(
            parse_proxy_override("foo*bar;host:8080;*"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn redact_hides_userinfo() {
        assert_eq!(
            redact_for_log("http://user:pass@proxy.local:8080"),
            "http://***@proxy.local:8080"
        );
        assert_eq!(
            redact_for_log("http://proxy.local:8080"),
            "http://proxy.local:8080"
        );
    }
}

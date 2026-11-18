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

/// Прокси из системных настроек: URL для [`crate::config::FingerprintProfile::proxy`].
#[cfg(target_os = "windows")]
pub fn system_proxy() -> Option<String> {
    let settings = read_internet_settings();
    if !settings.enabled {
        return None;
    }
    settings.server.as_deref().and_then(parse_proxy_server)
}

/// Список «ходи напрямую» из `ProxyOverride`, уже нормализованный под
/// сопоставление [`lumen_network::no_proxy_for_host`] (точный хост или
/// поддомены). Wildcard-записи, которые эта семантика не выражает
/// (`127.*`, `192.168.*`), отбрасываются: loopback и так минует прокси
/// (см. `is_loopback_host` в `lumen-network`).
pub fn bypass_patterns() -> &'static [String] {
    static PATTERNS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    PATTERNS.get_or_init(|| {
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

#[cfg(not(target_os = "windows"))]
pub fn system_proxy() -> Option<String> {
    None
}

/// `host:port` либо `http=host:port;https=host:port;socks=…` → `http://host:port`.
///
/// `https=` приоритетнее `http=` (страницы в Lumen почти всегда https), при
/// отсутствии `=` вся строка считается адресом одного прокси для всех схем.
pub(crate) fn parse_proxy_server(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    let address = if server.contains('=') {
        let mut http = None;
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
                // Запись `socks=` сюда не годится: `FingerprintProfile::proxy`
                // разбирает только `http(s)://`, и значение молча не применилось бы
                // (SOCKS5 у Lumen задаётся отдельным `socks5_proxy`/`--tor`).
                _ => {}
            }
        }
        http?
    } else {
        server
    };
    let has_scheme = address.contains("://");
    let url = if has_scheme {
        address.to_string()
    } else {
        format!("http://{address}")
    };
    // `parse_http_proxy`/`parse_socks5_proxy` в `config` принимают только
    // `http(s)://` и `socks5://`; прочие схемы (например `ftp=` в
    // ProxyServer) отбрасываем, чтобы не получить молча «без прокси».
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("socks5://") {
        Some(url)
    } else {
        None
    }
}

/// `ProxyOverride` (`localhost;127.*;*.example.ru;<local>`) → шаблоны для
/// [`lumen_network::install_no_proxy_patterns`].
pub(crate) fn parse_proxy_override(overrides: &str) -> Vec<String> {
    let mut out = Vec::new();
    for entry in overrides.split(';') {
        let entry = entry.trim();
        if entry.is_empty() || entry.eq_ignore_ascii_case("<local>") {
            continue;
        }
        // Семантика реестра допускает `*.` перед доменом — это ровно «домен и
        // поддомены», то же, что у сопоставителя. Прочие `*` не выражаются.
        let pattern = entry.strip_prefix("*.").unwrap_or(entry);
        if pattern.contains('*') || pattern.contains('/') || pattern.contains(':') {
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

    #[test]
    fn plain_server_gets_http_scheme() {
        assert_eq!(
            parse_proxy_server("127.0.0.1:12334").as_deref(),
            Some("http://127.0.0.1:12334")
        );
    }

    #[test]
    fn server_with_scheme_is_kept() {
        assert_eq!(
            parse_proxy_server("http://user:pass@proxy.local:8080").as_deref(),
            Some("http://user:pass@proxy.local:8080")
        );
    }

    #[test]
    fn per_scheme_list_prefers_https() {
        assert_eq!(
            parse_proxy_server("http=proxy:3128;https=tls-proxy:3129").as_deref(),
            Some("http://tls-proxy:3129")
        );
        assert_eq!(
            parse_proxy_server("http=proxy:3128").as_deref(),
            Some("http://proxy:3128")
        );
    }

    #[test]
    fn empty_and_unsupported_values_are_rejected() {
        assert!(parse_proxy_server("").is_none());
        assert!(parse_proxy_server("ftp=proxy:21").is_none());
        // Только `socks=` — не повод отдавать `proxy`, который не разберётся.
        assert!(parse_proxy_server("socks=proxy:1080").is_none());
        assert_eq!(
            parse_proxy_server("socks=proxy:1080;https=tls-proxy:3129").as_deref(),
            Some("http://tls-proxy:3129")
        );
    }

    #[test]
    fn overrides_drop_local_and_wildcard_middle() {
        assert_eq!(
            parse_proxy_override("localhost;127.*;*.Profi.ru;<local>;intranet"),
            vec!["localhost".to_string(), "profi.ru".to_string(), "intranet".to_string()]
        );
    }
}

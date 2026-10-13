//! Передача страницы во внешний браузер (COMPAT-HANDOFF, COMPAT-HO-01…03).
//!
//! Браузеры перечисляются по `SOFTWARE\Clients\StartMenuInternet` (HKLM и HKCU)
//! и запускаются исполняемым файлом с URL аргументом: `ShellExecuteW` с URL
//! отдал бы ссылку браузеру по умолчанию, то есть самому Lumen. Передаётся
//! только URL — ни cookie, ни данные форм.
//!
//! Модуль чистый: перечисление реестра ([`enumerate`]) отделено от разбора
//! командной строки, классификации и выбора по причине ([`pick`]) — их
//! проверяют тесты без реестра. Правила на 30 дней хранит
//! `lumen_storage::ExternalBrowserRules`.

use std::path::{Path, PathBuf};

/// Предупреждение перед первой передачей (COMPAT-HO-03): показывается один раз.
pub const FIRST_HANDOFF_WARNING: &str =
    "Вход и введённые данные не перенесутся, защиты Lumen там не действуют.";

/// Семейство браузера — по нему выбирается цель для причины.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserKind {
    /// Microsoft Edge.
    Edge,
    /// Google Chrome.
    Chrome,
    /// Яндекс Браузер.
    Yandex,
    /// Chromium-Gost (ГОСТ-TLS на CryptoPro).
    ChromiumGost,
    /// Mozilla Firefox.
    Firefox,
    /// Любой другой зарегистрированный браузер.
    Other,
}

/// Браузер, найденный в реестре.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalBrowser {
    /// Имя ключа `StartMenuInternet` — стабильный идентификатор правила.
    pub id: String,
    /// Отображаемое имя («Google Chrome»).
    pub name: String,
    /// Исполняемый файл.
    pub exe: PathBuf,
    /// Семейство.
    pub kind: BrowserKind,
}

/// Почему страницу передают — определяет, какой браузер подходит.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffReason {
    /// НУЦ, ГОСТ-TLS, электронная подпись: нужен Яндекс Браузер или Chromium-Gost.
    Crypto,
    /// DRM-видео (Widevine).
    Drm,
    /// Видеозвонки (WebRTC).
    Calls,
    /// Всё остальное («сайт не работает»).
    Other,
}

impl HandoffReason {
    /// Код причины для правила и локального сигнала (без текста пользователя).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Crypto => "crypto",
            Self::Drm => "drm",
            Self::Calls => "calls",
            Self::Other => "other",
        }
    }

    /// Порядок предпочтения семейств; пустой хвост значит «других не берём».
    fn preference(self) -> &'static [BrowserKind] {
        use BrowserKind::{Chrome, ChromiumGost, Edge, Firefox, Other, Yandex};
        match self {
            // Chrome и Edge ГОСТ-TLS не откроют — подставлять их нельзя.
            Self::Crypto => &[Yandex, ChromiumGost],
            Self::Drm | Self::Calls => &[Edge, Chrome, Yandex, Firefox],
            Self::Other => &[Edge, Chrome, Yandex, Firefox, ChromiumGost, Other],
        }
    }
}

/// Результат выбора браузера под причину.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick<'a> {
    /// Подходящий браузер найден.
    Browser(&'a ExternalBrowser),
    /// Подходящего нет; текст для пользователя — [`no_suitable_message`].
    NoSuitable,
}

/// Семейство по ключу реестра, имени и имени исполняемого файла.
#[must_use]
pub fn classify(id: &str, name: &str, exe: &Path) -> BrowserKind {
    let hay = format!(
        "{} {} {}",
        id.to_lowercase(),
        name.to_lowercase(),
        exe.file_name().and_then(|f| f.to_str()).unwrap_or("").to_lowercase()
    );
    if hay.contains("gost") {
        BrowserKind::ChromiumGost
    } else if hay.contains("yandex") || hay.contains("яндекс") {
        BrowserKind::Yandex
    } else if hay.contains("msedge") || hay.contains("microsoft edge") {
        BrowserKind::Edge
    } else if hay.contains("firefox") {
        BrowserKind::Firefox
    } else if hay.contains("chrome.exe") || hay.contains("google chrome") {
        BrowserKind::Chrome
    } else {
        BrowserKind::Other
    }
}

/// Путь к исполняемому файлу из значения `shell\open\command`:
/// `"C:\...\chrome.exe" --single-argument %1` → `C:\...\chrome.exe`.
#[must_use]
pub fn parse_command(cmd: &str) -> Option<PathBuf> {
    let cmd = cmd.trim();
    let exe = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        // Без кавычек путь с пробелами неоднозначен: берём до «.exe», иначе до пробела.
        let lower = cmd.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(i) => &cmd[..i + 4],
            None => cmd.split_whitespace().next()?,
        }
    };
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

/// Выбрать браузер под причину из найденных. `own_exe` (сам Lumen) исключается.
#[must_use]
pub fn pick<'a>(
    reason: HandoffReason,
    browsers: &'a [ExternalBrowser],
    own_exe: Option<&Path>,
) -> Pick<'a> {
    let usable = |b: &&ExternalBrowser| own_exe.is_none_or(|own| !same_file(&b.exe, own));
    for kind in reason.preference() {
        if let Some(b) = browsers.iter().filter(usable).find(|b| b.kind == *kind) {
            return Pick::Browser(b);
        }
    }
    Pick::NoSuitable
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

/// Текст для случая [`Pick::NoSuitable`]: браузер говорит прямо, а не молчит
/// (COMPAT-HO-02).
#[must_use]
pub fn no_suitable_message(reason: HandoffReason) -> &'static str {
    match reason {
        HandoffReason::Crypto => {
            "Chrome и Edge этот сайт тоже не откроют: нужен Яндекс Браузер или \
             Chromium-Gost, но на этом компьютере их нет."
        }
        HandoffReason::Drm | HandoffReason::Calls => {
            "Другого подходящего браузера на этом компьютере не найдено: установите \
             Edge или Chrome."
        }
        HandoffReason::Other => "Других браузеров на этом компьютере не найдено.",
    }
}

/// Передавать можно только http(s): `file:`, `javascript:` и свои схемы во
/// внешний процесс не уходят.
#[must_use]
pub fn is_handoff_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        && !url.chars().any(|c| c.is_control())
}

/// Перехватывать ли клик по ссылке `target` на странице `current` при живом
/// правиле: цель — http(s) и другой сайт. Ссылки внутри сайта открываются в
/// Lumen: страницу там пользователь уже открыл сам.
#[must_use]
pub fn is_cross_site_handoff(current: &str, target: &str) -> bool {
    is_handoff_url(target) && !lumen_storage::same_site(current, target)
}

/// Браузер из списка по идентификатору правила.
#[must_use]
pub fn find_by_id<'a>(browsers: &'a [ExternalBrowser], id: &str) -> Option<&'a ExternalBrowser> {
    browsers.iter().find(|b| b.id == id)
}

/// Текст уведомления после передачи; предупреждение — только при первой.
#[must_use]
pub fn handoff_notice(browser_name: &str, first_time: bool) -> String {
    if first_time {
        format!("Открыто в {browser_name}. {FIRST_HANDOFF_WARNING}")
    } else {
        format!("Открыто в {browser_name}.")
    }
}

/// Запустить `browser` с `url`. Только URL — без cookie и без данных форм.
pub fn launch(browser: &ExternalBrowser, url: &str) -> std::io::Result<()> {
    if !is_handoff_url(url) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "во внешний браузер передаются только http(s)-адреса",
        ));
    }
    std::process::Command::new(&browser.exe).arg(url.trim()).spawn().map(drop)
}

/// Браузеры, зарегистрированные в системе, без самого Lumen. Вне Windows — пусто.
#[must_use]
pub fn enumerate() -> Vec<ExternalBrowser> {
    #[cfg(target_os = "windows")]
    {
        let own = std::env::current_exe().ok();
        let mut out: Vec<ExternalBrowser> = Vec::new();
        for (id, name, command) in registry::start_menu_internet() {
            let Some(exe) = parse_command(&command) else { continue };
            if !exe.is_file() || own.as_deref().is_some_and(|o| same_file(&exe, o)) {
                continue;
            }
            if out.iter().any(|b| same_file(&b.exe, &exe)) {
                continue;
            }
            let kind = classify(&id, &name, &exe);
            out.push(ExternalBrowser { id, name, exe, kind });
        }
        out
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "windows")]
mod registry {
    //! Чтение `SOFTWARE\Clients\StartMenuInternet` через windows-sys.

    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };

    const BASE: &str = r"SOFTWARE\Clients\StartMenuInternet";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Раскрыть `%VAR%` в значении `REG_EXPAND_SZ`.
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

    /// Открытый ключ реестра; закрывается в `Drop`.
    struct Key(HKEY);

    impl Key {
        fn open(root: HKEY, path: &str) -> Option<Self> {
            let mut h: HKEY = 0;
            let p = wide(path);
            // SAFETY: `p` — NUL-терминированная UTF-16 строка, `h` — валидный out-указатель.
            let rc = unsafe { RegOpenKeyExW(root, p.as_ptr(), 0, KEY_READ, &mut h) };
            (rc == 0).then_some(Self(h))
        }

        /// Значение по умолчанию (`""`) подключа `sub` либо самого ключа.
        fn default_string(&self, sub: &str) -> Option<String> {
            let key = if sub.is_empty() { None } else { Some(Key::open(self.0, sub)?) };
            let h = key.as_ref().map_or(self.0, |k| k.0);
            let name = wide("");
            let (mut ty, mut size) = (0u32, 0u32);
            // SAFETY: запрос размера (lpdata = null); указатели валидны на время вызова.
            let rc = unsafe {
                RegQueryValueExW(h, name.as_ptr(), std::ptr::null(), &mut ty, std::ptr::null_mut(), &mut size)
            };
            if rc != 0 || (ty != REG_SZ && ty != REG_EXPAND_SZ) || size == 0 || size > 65_536 {
                return None;
            }
            let mut buf = vec![0u16; (size as usize).div_ceil(2)];
            let mut size2 = (buf.len() * 2) as u32;
            // SAFETY: `buf` вмещает `size2` байт, ядро записывает не больше.
            let rc = unsafe {
                RegQueryValueExW(
                    h,
                    name.as_ptr(),
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

        fn subkeys(&self) -> Vec<String> {
            let mut out = Vec::new();
            for index in 0u32.. {
                let mut buf = [0u16; 256];
                let mut len = buf.len() as u32;
                // SAFETY: `buf` вмещает `len` символов; прочие указатели — null.
                let rc = unsafe {
                    RegEnumKeyExW(
                        self.0,
                        index,
                        buf.as_mut_ptr(),
                        &mut len,
                        std::ptr::null(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if rc != 0 {
                    break;
                }
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
            out
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: `self.0` открыт в `Key::open` и ещё не закрыт.
            unsafe { RegCloseKey(self.0) };
        }
    }

    /// `(ключ, имя, команда)` браузеров из HKLM, затем HKCU.
    pub(super) fn start_menu_internet() -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let Some(base) = Key::open(root, BASE) else { continue };
            for id in base.subkeys() {
                let Some(k) = Key::open(base.0, &id) else { continue };
                let Some(command) = k.default_string(r"shell\open\command") else { continue };
                let name = k.default_string("").filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
                out.push((id, name, command));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: &str, name: &str, exe: &str) -> ExternalBrowser {
        let exe = PathBuf::from(exe);
        ExternalBrowser { id: id.into(), name: name.into(), kind: classify(id, name, &exe), exe }
    }

    fn installed() -> Vec<ExternalBrowser> {
        vec![
            b("Firefox-308046B0AF4A39CB", "Mozilla Firefox", r"C:\Program Files\Mozilla Firefox\firefox.exe"),
            b("Google Chrome", "Google Chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            b("Microsoft Edge", "Microsoft Edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
            b("YandexBrowser.ABC", "Яндекс Браузер", r"C:\Users\u\AppData\Local\Yandex\YandexBrowser\Application\browser.exe"),
            b("Lumen", "Lumen", r"C:\lumen\lumen.exe"),
        ]
    }

    #[test]
    fn classify_known_families() {
        let kinds: Vec<_> = installed().iter().map(|x| x.kind).collect();
        use BrowserKind::*;
        assert_eq!(kinds, [Firefox, Chrome, Edge, Yandex, Other]);
        let gost = classify("Chromium-Gost", "Chromium-Gost", Path::new(r"C:\g\chromium-gost.exe"));
        assert_eq!(gost, ChromiumGost);
    }

    #[test]
    fn parse_command_handles_quotes_and_args() {
        assert_eq!(
            parse_command(r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --single-argument %1"#),
            Some(PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"))
        );
        assert_eq!(
            parse_command(r"C:\Program Files\Mozilla Firefox\firefox.exe -osint -url %1"),
            Some(PathBuf::from(r"C:\Program Files\Mozilla Firefox\firefox.exe"))
        );
        assert_eq!(parse_command(r"C:\x\app -a"), Some(PathBuf::from(r"C:\x\app")));
        assert_eq!(parse_command("  "), None);
        assert_eq!(parse_command(r#""""#), None);
    }

    #[test]
    fn crypto_prefers_yandex_and_never_falls_back_to_chrome() {
        let all = installed();
        match pick(HandoffReason::Crypto, &all, None) {
            Pick::Browser(x) => assert_eq!(x.kind, BrowserKind::Yandex),
            Pick::NoSuitable => panic!("Яндекс установлен"),
        }
        let no_yandex: Vec<_> = all.into_iter().filter(|x| x.kind != BrowserKind::Yandex).collect();
        assert_eq!(pick(HandoffReason::Crypto, &no_yandex, None), Pick::NoSuitable);
        assert!(no_suitable_message(HandoffReason::Crypto).starts_with("Chrome"));
    }

    #[test]
    fn drm_and_calls_prefer_edge_then_chrome() {
        let all = installed();
        for reason in [HandoffReason::Drm, HandoffReason::Calls] {
            match pick(reason, &all, None) {
                Pick::Browser(x) => assert_eq!(x.kind, BrowserKind::Edge),
                Pick::NoSuitable => panic!(),
            }
        }
        let no_edge: Vec<_> = all.into_iter().filter(|x| x.kind != BrowserKind::Edge).collect();
        match pick(HandoffReason::Drm, &no_edge, None) {
            Pick::Browser(x) => assert_eq!(x.kind, BrowserKind::Chrome),
            Pick::NoSuitable => panic!(),
        }
    }

    #[test]
    fn own_exe_is_never_picked() {
        let all = installed();
        let only_lumen = vec![all[4].clone()];
        assert_eq!(
            pick(HandoffReason::Other, &only_lumen, Some(Path::new(r"C:\LUMEN\lumen.exe"))),
            Pick::NoSuitable
        );
    }

    #[test]
    fn reason_codes_are_distinct() {
        let codes: Vec<_> = [HandoffReason::Crypto, HandoffReason::Drm, HandoffReason::Calls, HandoffReason::Other]
            .iter()
            .map(|r| r.code())
            .collect();
        assert_eq!(codes, ["crypto", "drm", "calls", "other"]);
    }

    #[test]
    fn only_http_urls_are_handed_off() {
        assert!(is_handoff_url("https://example.com/a?b=1"));
        assert!(is_handoff_url(" HTTP://example.com "));
        assert!(!is_handoff_url("file:///C:/x"));
        assert!(!is_handoff_url("javascript:alert(1)"));
        assert!(!is_handoff_url("lumen://settings"));
        assert!(!is_handoff_url("https://a.com/\r\n--new-window"));
        let err = launch(&installed()[0], "file:///C:/x").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn link_is_intercepted_only_across_sites() {
        let cur = "https://news.example.com/a";
        assert!(is_cross_site_handoff(cur, "https://bank.ru/login"));
        assert!(!is_cross_site_handoff(cur, "https://www.example.com/b"));
        assert!(!is_cross_site_handoff(cur, "mailto:a@bank.ru"));
        assert!(!is_cross_site_handoff(cur, "javascript:void(0)"));
    }

    #[test]
    fn warning_only_on_first_handoff() {
        assert!(handoff_notice("Edge", true).contains(FIRST_HANDOFF_WARNING));
        assert_eq!(handoff_notice("Edge", false), "Открыто в Edge.");
    }

    #[test]
    fn rule_browser_is_found_by_id() {
        let list = installed();
        let id = list[0].id.clone();
        assert_eq!(find_by_id(&list, &id), Some(&list[0]));
        assert!(find_by_id(&list, "no-such-browser").is_none());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn enumerate_does_not_panic_and_excludes_missing_files() {
        for br in enumerate() {
            assert!(br.exe.is_file(), "{:?}", br.exe);
        }
    }
}

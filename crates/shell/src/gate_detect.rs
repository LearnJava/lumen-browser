//! COMPAT-GATE-DETECT: распознавание антибот-стен и баннеров «браузер не
//! поддерживается» (`docs/research/site-compat/report.md` §8.4,
//! COMPAT-ERR-10…12).
//!
//! Чистая часть без часов и ввода-вывода. [`classify`] смотрит на статус,
//! заголовки, адрес и начало тела ответа главного документа и называет, кто
//! закрыл дверь: Cloudflare, DDoS-Guard, Qrator, SmartCaptcha, Akamai, DataDome,
//! Wildberries (498), Avito (429/439), Ozon (403 и `/abt-challenge/`) либо
//! «защита сайта» без имени. [`unsupported_banner`] ищет в видимом тексте
//! страницы баннер «браузер не поддерживается» (RU и EN).
//!
//! Один ответ-признак ещё не приговор: Cloudflare решает JS-проверку сам и
//! перезагружает страницу. Панель показывается, только когда тот же сайт отдал
//! стену трижды за минуту ([`GateLoop`]), — это и есть «петля антибота».

use std::collections::HashMap;

/// Окно петли, секунды.
pub const LOOP_WINDOW_SECS: i64 = 60;
/// Сколько стен подряд в окне считаются петлёй.
pub const LOOP_COUNT: usize = 3;
/// Сколько байт тела смотрит классификатор: стена — это маленькая страница.
const SCAN_BYTES: usize = 64 * 1024;
/// Видимого текста меньше этого — «страница-стена», а не обычная страница с
/// виджетом капчи в форме.
const WALL_TEXT_CHARS: usize = 1500;
/// Баннер в начале страницы: фраза в первых символах видимого текста.
const BANNER_HEAD_CHARS: usize = 800;

/// Кто поставил стену.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Vendor {
    Cloudflare,
    DdosGuard,
    Qrator,
    SmartCaptcha,
    Akamai,
    DataDome,
    Wildberries,
    Avito,
    Ozon,
    /// Стена есть, а чья — по признакам не определить.
    Unknown,
}

impl Vendor {
    /// Название для заголовка панели.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Cloudflare => "Cloudflare",
            Self::DdosGuard => "DDoS-Guard",
            Self::Qrator => "Qrator",
            Self::SmartCaptcha => "Yandex SmartCaptcha",
            Self::Akamai => "Akamai",
            Self::DataDome => "DataDome",
            Self::Wildberries => "Wildberries",
            Self::Avito => "Авито",
            Self::Ozon => "Ozon",
            Self::Unknown => "антибот",
        }
    }

    /// Стабильный код для журнала сигналов.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Cloudflare => "cloudflare",
            Self::DdosGuard => "ddos_guard",
            Self::Qrator => "qrator",
            Self::SmartCaptcha => "smartcaptcha",
            Self::Akamai => "akamai",
            Self::DataDome => "datadome",
            Self::Wildberries => "wildberries",
            Self::Avito => "avito",
            Self::Ozon => "ozon",
            Self::Unknown => "unknown",
        }
    }
}

/// Заголовок ответа: (имя, значение).
pub type Header = (String, String);

fn header<'a>(headers: &'a [Header], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Хост и путь адреса в нижнем регистре.
fn host_and_path(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..end].rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    (host.to_ascii_lowercase(), rest[end..].to_ascii_lowercase())
}

fn host_is(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|p| p.ends_with('.'))
}

/// Начало тела как текст в нижнем регистре.
fn head_lower(body: &[u8]) -> String {
    let cut = &body[..body.len().min(SCAN_BYTES)];
    String::from_utf8_lossy(cut).to_lowercase()
}

/// Видимый текст HTML: без комментариев, `<script>`/`<style>`/`<noscript>`/
/// `<template>`, без поддеревьев с `hidden` и `display:none`; пробелы сжаты,
/// ASCII-буквы в нижнем регистре.
/// Это не парсер, а грубый сканер для классификации: ему достаточно не
/// принимать скрытый служебный текст за видимый.
pub(crate) fn visible_text(html: &str) -> String {
    const VOID: [&str; 14] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
        "track", "wbr",
    ];
    // ASCII-регистр не меняет длину в байтах, поэтому индексы остаются общими.
    let html = html.to_ascii_lowercase();
    let html = html.as_str();
    let b = html.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    // Открытые скрытые поддеревья: (имя тега, глубина одноимённых вложений).
    let mut hidden: Option<(String, usize)> = None;
    while i < b.len() {
        if b[i] != b'<' {
            let end = html[i..].find('<').map_or(b.len(), |p| i + p);
            if hidden.is_none() {
                out.push_str(&html[i..end]);
            }
            i = end;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i + 4..].find("-->").map_or(b.len(), |p| i + 4 + p + 3);
            continue;
        }
        let Some(close) = html[i..].find('>') else { break };
        let tag = &html[i + 1..i + close];
        let next = i + close + 1;
        let closing = tag.starts_with('/');
        let name_part = tag.trim_start_matches('/');
        let name: String = name_part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() {
            i = next;
            continue;
        }
        if let Some((hn, depth)) = hidden.as_mut() {
            if *hn == name {
                if closing {
                    if *depth == 0 {
                        hidden = None;
                    } else {
                        *depth -= 1;
                    }
                } else if !tag.ends_with('/') {
                    *depth += 1;
                }
            }
            i = next;
            continue;
        }
        if closing || VOID.contains(&name.as_str()) || tag.ends_with('/') {
            if !closing && name == "br" {
                out.push(' ');
            }
            i = next;
            continue;
        }
        if matches!(name.as_str(), "script" | "style" | "noscript" | "template") {
            let needle = format!("</{name}");
            i = html[next..].find(&needle).map_or(b.len(), |p| next + p);
            // Пропустили тело; закрывающий тег разберёт следующая итерация.
            continue;
        }
        let low = tag.to_ascii_lowercase();
        let squashed: String = low.chars().filter(|c| !c.is_whitespace()).collect();
        let is_hidden = squashed.contains("display:none")
            || squashed.contains("visibility:hidden")
            || low.split_whitespace().any(|w| w == "hidden" || w.starts_with("hidden="));
        if is_hidden {
            hidden = Some((name, 0));
        } else {
            out.push(' ');
        }
        i = next;
    }
    let decoded = out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&#39;", "'").replace("&quot;", "\"");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Страница-стена: мало видимого текста. Страницы с виджетом капчи внутри
/// формы входа длиннее.
fn is_wall(body_lower: &str) -> bool {
    visible_text(body_lower).chars().count() < WALL_TEXT_CHARS
}

fn any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

/// Признаки антибот-стены в ответе главного документа.
///
/// `status` — HTTP-статус финального ответа, `url` — его адрес. `None` — обычная
/// страница. Порядок проверок — от самых однозначных признаков к общим.
#[must_use]
pub fn classify(status: u16, headers: &[Header], body: &[u8], url: &str) -> Option<Vendor> {
    let (host, path) = host_and_path(url);
    let text = head_lower(body);
    let blocked = status >= 400;
    let wall = is_wall(&text);

    // Cloudflare: заголовок самого Cloudflare, пока он держит проверку.
    if header(headers, "cf-mitigated").is_some_and(|v| v.to_ascii_lowercase().contains("challenge")) {
        return Some(Vendor::Cloudflare);
    }
    if (blocked || wall)
        && (text.contains("cdn-cgi/challenge-platform")
            || (blocked && text.contains("challenges.cloudflare.com"))
            || (text.contains("<title>just a moment") && text.contains("cloudflare")))
    {
        return Some(Vendor::Cloudflare);
    }
    if status == 403 && text.contains("cloudflare") && (text.contains("ray id") || text.contains("attention required")) {
        return Some(Vendor::Cloudflare);
    }

    // DDoS-Guard.
    let server = header(headers, "server").unwrap_or("").to_ascii_lowercase();
    if (text.contains("/.well-known/ddos-guard/") && (blocked || wall))
        || (server.contains("ddos-guard") && matches!(status, 403 | 503) && wall)
        || (blocked && text.contains("ddos-guard"))
    {
        return Some(Vendor::DdosGuard);
    }

    // Qrator: вызов `/__qrator/` или отказ с его идентификатором запроса.
    if text.contains("/__qrator/")
        || (header(headers, "x-qrator-requestid").is_some() && matches!(status, 401 | 403 | 429 | 503))
        || (blocked && text.contains("qrator"))
    {
        return Some(Vendor::Qrator);
    }

    // Yandex SmartCaptcha: страница `/showcaptcha` или капча вместо страницы.
    if (host.contains("yandex") && path.starts_with("/showcaptcha"))
        || (text.contains("smartcaptcha.yandexcloud.net") && (blocked || wall))
    {
        return Some(Vendor::SmartCaptcha);
    }

    // DataDome.
    if (text.contains("captcha-delivery.com") && (blocked || wall))
        || ((header(headers, "x-datadome").is_some() || server.contains("datadome")) && matches!(status, 403 | 429))
    {
        return Some(Vendor::DataDome);
    }

    // Akamai: «Access Denied … Reference #18.…».
    if status == 403
        && text.contains("reference #")
        && any(&text, &["errors.edgesuite.net", "access denied", "you don't have permission to access"])
    {
        return Some(Vendor::Akamai);
    }

    // Российские площадки: статус и хост либо фирменный адрес вызова.
    if status == 498 || ((blocked || wall) && (text.contains("/__wbaas/") || path.starts_with("/__wbaas/"))) {
        return Some(Vendor::Wildberries);
    }
    if (blocked || wall) && (text.contains("/abt-challenge") || text.contains("antibot challenge page")) {
        return Some(Vendor::Ozon);
    }
    if host_is(&host, "ozon.ru") && status == 403 {
        return Some(Vendor::Ozon);
    }
    if host_is(&host, "avito.ru") && matches!(status, 403 | 429 | 439) {
        return Some(Vendor::Avito);
    }

    // Общая стена: отказ со словами про проверку на человека.
    if matches!(status, 403 | 429 | 503)
        && wall
        && any(
            &text,
            &[
                "доступ ограничен",
                "verify you are human",
                "are you a robot",
                "unusual traffic",
                "необычный трафик",
                "подозрительная активность",
                "подозрительный трафик",
            ],
        )
    {
        return Some(Vendor::Unknown);
    }
    None
}

/// Фразы баннеров «браузер не поддерживается».
const BANNER_PHRASES: [&str; 17] = [
    "браузер не поддерживается",
    "браузер больше не поддерживается",
    "ваш браузер устарел",
    "браузер устарел",
    "неподдерживаемый браузер",
    "не поддерживает ваш браузер",
    "ваш браузер не поддерживает",
    "обновите ваш браузер",
    "обновите браузер",
    "your browser is not supported",
    "browser is not supported",
    "browser isn't supported",
    "browser is no longer supported",
    "unsupported browser",
    "your browser is out of date",
    "please update your browser",
    "please upgrade your browser",
];

/// Есть ли в видимом тексте HTML баннер «браузер не поддерживается».
///
/// Фраза должна стоять в начале видимого текста (баннер над страницей) либо
/// страница должна быть короткой (страница-заглушка). Скрытые элементы,
/// комментарии (`<!--[if lt IE 9]>`), скрипты и `<noscript>` не считаются.
#[must_use]
pub fn unsupported_banner(html: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&html[..html.len().min(SCAN_BYTES)]).into_owned();
    let visible = visible_text(&text).to_lowercase();
    let total = visible.chars().count();
    BANNER_PHRASES.iter().any(|p| {
        visible.find(p).is_some_and(|byte_pos| {
            let chars_before = visible[..byte_pos].chars().count();
            total < WALL_TEXT_CHARS || chars_before < BANNER_HEAD_CHARS
        })
    })
}

/// Что опознано в ответе главного документа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Антибот-стена этого поставщика.
    Antibot(Vendor),
    /// Страница показывает баннер «браузер не поддерживается».
    UnsupportedBanner,
}

/// Опознать стену или баннер в ответе. `content_type` — заголовок как есть;
/// тело, не похожее на HTML, на баннер не проверяется. Стена важнее баннера.
#[must_use]
pub fn inspect(status: u16, headers: &[Header], body: &[u8], url: &str, content_type: Option<&str>) -> Option<Verdict> {
    if let Some(vendor) = classify(status, headers, body, url) {
        return Some(Verdict::Antibot(vendor));
    }
    let html = content_type.is_none_or(|ct| {
        let ct = ct.to_ascii_lowercase();
        ct.contains("html") || ct.contains("xml")
    });
    (html && unsupported_banner(body)).then_some(Verdict::UnsupportedBanner)
}

/// Счётчик петли: сколько раз один сайт отдал стену за последнюю минуту.
/// Живёт в памяти окна и ничего не пишет на диск, поэтому работает и в
/// анонимном профиле.
#[derive(Debug, Default)]
pub struct GateLoop {
    hits: HashMap<String, Vec<i64>>,
}

impl GateLoop {
    /// Стена на `site` в момент `now` (unix-секунды). Возвращает, сколько стен
    /// за окно, включая эту.
    pub fn record(&mut self, site: &str, now: i64) -> usize {
        self.hits.retain(|_, ts| {
            ts.retain(|&t| now - t <= LOOP_WINDOW_SECS);
            !ts.is_empty()
        });
        let ts = self.hits.entry(site.to_owned()).or_default();
        ts.push(now);
        ts.len()
    }

    /// Петля ли это: [`LOOP_COUNT`] стен и больше.
    #[must_use]
    pub fn is_loop(count: usize) -> bool {
        count >= LOOP_COUNT
    }

    /// Забыть сайт: пользователь прошёл проверку или очистил данные.
    pub fn forget(&mut self, site: &str) {
        self.hits.remove(site);
    }

    /// Забыть всё.
    pub fn clear(&mut self) {
        self.hits.clear();
    }
}

/// Короткий идентификатор проверки для обращения в поддержку: 8 шестнадцатеричных
/// знаков от сайта, поставщика стены и минуты. IP и прочие данные сети в него не
/// входят.
#[must_use]
pub fn check_id(site: &str, vendor: Vendor, now: i64) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in site.bytes().chain(vendor.code().bytes()).chain((now / 60).to_le_bytes()) {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{:04X}-{:04X}", h >> 16, h & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(pairs: &[(&str, &str)]) -> Vec<Header> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    fn gate(status: u16, headers: &[(&str, &str)], body: &str, url: &str) -> Option<Vendor> {
        classify(status, &h(headers), body.as_bytes(), url)
    }

    /// Стенд: ответы антибот-систем, как их отдают сайты, и ожидаемый тип.
    #[test]
    fn stand_responses_get_the_right_vendor() {
        use Vendor::*;
        type Case = (&'static str, u16, Vec<(&'static str, &'static str)>, &'static str, &'static str, Vendor);
        let cases: Vec<Case> = vec![
            ("cf header", 403, vec![("cf-mitigated", "challenge")], "<html></html>", "https://a.example/", Cloudflare),
            (
                "cf just a moment",
                403,
                vec![("server", "cloudflare")],
                "<!DOCTYPE html><html><head><title>Just a moment...</title></head><body>\
                 <script src=\"/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1\"></script></body></html>",
                "https://a.example/",
                Cloudflare,
            ),
            (
                "cf turnstile wall 200",
                200,
                vec![],
                "<html><body><p>Verify</p><iframe src=\"https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/g/turnstile\"></iframe></body></html>",
                "https://a.example/",
                Cloudflare,
            ),
            (
                "cf 1020",
                403,
                vec![],
                "<title>Attention Required! | Cloudflare</title><p>Cloudflare Ray ID: 8abc</p>",
                "https://a.example/",
                Cloudflare,
            ),
            (
                "ddos-guard js",
                403,
                vec![("server", "ddos-guard")],
                "<title>DDoS-Guard</title><script src=\"/.well-known/ddos-guard/js-challenge/index.js\"></script>",
                "https://a.example/",
                DdosGuard,
            ),
            ("ddos-guard header", 503, vec![("Server", "ddos-guard")], "<p>Checking your browser</p>", "https://a.example/", DdosGuard),
            (
                "qrator challenge",
                200,
                vec![],
                "<html><script src=\"/__qrator/qauth_utm_v2.js\"></script></html>",
                "https://a.example/",
                Qrator,
            ),
            ("qrator 403", 403, vec![("x-qrator-requestid", "abc")], "<h1>403</h1>", "https://a.example/", Qrator),
            ("smartcaptcha showcaptcha", 200, vec![], "<title>Вы не робот?</title>", "https://yandex.ru/showcaptcha?cc=1", SmartCaptcha),
            (
                "smartcaptcha embed wall",
                200,
                vec![],
                "<body><div id=\"c\"></div><script src=\"https://smartcaptcha.yandexcloud.net/captcha.js\"></script></body>",
                "https://a.example/",
                SmartCaptcha,
            ),
            (
                "datadome iframe",
                403,
                vec![],
                "<iframe src=\"https://geo.captcha-delivery.com/captcha/?initialCid=x\"></iframe>",
                "https://a.example/",
                DataDome,
            ),
            (
                "akamai",
                403,
                vec![],
                "<h1>Access Denied</h1>You don't have permission to access \"http://a.example/\" on this server.<p>Reference #18.4a1b.1700000000.1234",
                "https://a.example/",
                Akamai,
            ),
            ("wb 498", 498, vec![], "", "https://www.wildberries.ru/catalog", Wildberries),
            (
                "wb wbaas",
                200,
                vec![],
                "<html><script src=\"/__wbaas/challenge.js\"></script></html>",
                "https://www.wildberries.ru/",
                Wildberries,
            ),
            ("avito 429", 429, vec![], "<h1>Доступ ограничен: проблема с IP</h1>", "https://www.avito.ru/moskva", Avito),
            ("avito 439", 439, vec![], "<html></html>", "https://www.avito.ru/", Avito),
            ("ozon 403", 403, vec![], "<html></html>", "https://www.ozon.ru/", Ozon),
            (
                "ozon abt",
                200,
                vec![],
                "<title>Antibot Challenge Page</title><script src=\"/abt-challenge/x.js\"></script>",
                "https://other.example/",
                Ozon,
            ),
            ("generic human check", 403, vec![], "<h1>Please verify you are human</h1>", "https://a.example/", Unknown),
        ];
        assert!(cases.len() >= 15);
        for (name, status, headers, body, url, want) in cases {
            assert_eq!(gate(status, &headers, body, url), Some(want), "{name}");
        }
    }

    #[test]
    fn ordinary_responses_are_not_gates() {
        // Виджет Turnstile и SmartCaptcha внутри длинной страницы формы входа.
        let filler = "Текст страницы. ".repeat(200);
        let page = format!(
            "<html><body><p>{filler}</p><iframe src=\"https://challenges.cloudflare.com/turnstile\"></iframe>\
             <script src=\"https://smartcaptcha.yandexcloud.net/captcha.js\"></script></body></html>"
        );
        assert_eq!(gate(200, &[], &page, "https://a.example/login"), None);
        // Обычный 404 и 403 без признаков антибота.
        assert_eq!(gate(404, &[], "<h1>Not found</h1>", "https://a.example/x"), None);
        assert_eq!(gate(403, &[], "<h1>Forbidden</h1>", "https://a.example/dir/"), None);
        // Страница через Cloudflare и DDoS-Guard без проверки.
        assert_eq!(gate(200, &[("server", "cloudflare")], "<html><p>Hello</p></html>", "https://a.example/"), None);
        assert_eq!(gate(200, &[("server", "ddos-guard")], "<html><p>Hello</p></html>", "https://a.example/"), None);
        // Авито и Ozon, отдавшие страницу.
        assert_eq!(gate(200, &[], "<p>Объявления</p>", "https://www.avito.ru/"), None);
        assert_eq!(gate(200, &[], "<p>Каталог</p>", "https://www.ozon.ru/"), None);
        // Слово «captcha» в тексте статьи.
        assert_eq!(gate(200, &[], "<p>How a captcha works</p>", "https://a.example/blog"), None);
    }

    #[test]
    fn banners_in_ru_and_en_are_found() {
        let cases = [
            "<html><body><h1>Ваш браузер не поддерживается</h1><p>Установите Chrome.</p></body></html>",
            "<body><div class=\"bar\">Ваш браузер устарел. Обновите браузер.</div><main>Каталог</main></body>",
            "<body><h2>Your browser is not supported</h2></body>",
            "<body><div>Please update your browser to continue.</div></body>",
            "<body><p>Unsupported browser</p></body>",
            "<body><p>Данный браузер не поддерживается нашим сервисом</p></body>",
        ];
        for (i, html) in cases.iter().enumerate() {
            assert!(unsupported_banner(html.as_bytes()), "case {i}: {html}");
        }
    }

    #[test]
    fn hidden_and_comment_banners_are_ignored() {
        let cases = [
            "<body><!--[if lt IE 9]><p>Your browser is not supported</p><![endif]--><p>Hello</p></body>",
            "<body><div style=\"display: none\"><p>Your browser is not supported</p></div><p>Hello</p></body>",
            "<body><div hidden><div>x</div><p>Ваш браузер устарел</p></div><p>Hello</p></body>",
            "<body><noscript>Your browser is not supported</noscript><p>Hello</p></body>",
            "<body><script>var m = 'Your browser is not supported';</script><p>Hello</p></body>",
        ];
        for (i, html) in cases.iter().enumerate() {
            assert!(!unsupported_banner(html.as_bytes()), "case {i}: {html}");
        }
        // Фраза глубоко в длинной странице — статья про браузеры, а не баннер.
        let long = format!("<body><p>{}</p><p>unsupported browser</p></body>", "слово ".repeat(600));
        assert!(!unsupported_banner(long.as_bytes()));
    }

    #[test]
    fn visible_text_skips_service_content() {
        let t = visible_text(
            "<html><head><title>T</title><style>p{}</style></head><body><p>a &amp; b</p><br>c<script>x</script></body></html>",
        );
        assert_eq!(t, "t a & b c");
    }

    #[test]
    fn loop_needs_three_walls_within_a_minute() {
        let mut l = GateLoop::default();
        assert!(!GateLoop::is_loop(l.record("a.example", 100)));
        assert!(!GateLoop::is_loop(l.record("a.example", 110)));
        assert!(GateLoop::is_loop(l.record("a.example", 120)));
        // Другой сайт считается отдельно.
        assert_eq!(l.record("b.example", 121), 1);
        // Окно ушло вперёд: счёт идёт заново.
        assert_eq!(l.record("a.example", 400), 1);
        l.forget("a.example");
        assert_eq!(l.record("a.example", 401), 1);
    }

    #[test]
    fn check_id_is_stable_and_has_no_network_data() {
        let a = check_id("a.example", Vendor::Cloudflare, 1_700_000_000);
        assert_eq!(a, check_id("a.example", Vendor::Cloudflare, 1_700_000_010));
        assert_ne!(a, check_id("b.example", Vendor::Cloudflare, 1_700_000_000));
        assert_eq!(a.len(), 9);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    /// Локальные HTML-страницы репозитория (не больше `cap`), обход в глубину.
    fn corpus_pages(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>, cap: usize) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            if out.len() >= cap {
                return;
            }
            let path = e.path();
            if path.is_dir() {
                corpus_pages(&path, out, cap);
            } else if path.extension().and_then(|x| x.to_str()) == Some("html") {
                out.push(path);
            }
        }
    }

    /// 0 ложных срабатываний на локальном корпусе страниц (ответы 200).
    #[test]
    fn no_false_positives_on_local_corpus() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut pages = Vec::new();
        for dir in ["graphic_tests", "samples", "docs"] {
            corpus_pages(&root.join(dir), &mut pages, 1500);
        }
        for path in &pages {
            let Ok(body) = std::fs::read(path) else { continue };
            let url = "https://corpus.example/page.html";
            assert_eq!(classify(200, &[], &body, url), None, "{}", path.display());
            assert!(!unsupported_banner(&body), "{}", path.display());
        }
        assert!(pages.len() >= 200, "в корпусе только {} страниц", pages.len());
    }
}

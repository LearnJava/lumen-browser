//! COMPAT-ERRORS: страница сетевой ошибки главного документа.
//!
//! Раньше `LoadEvent::LoadError` ставил `load_failed` и ничего не рисовал — на
//! экране оставалась выгруженная прежняя страница. Теперь вкладка показывает
//! статическую страницу по правилам COMPAT-ERR-00
//! (`docs/research/site-compat/report.md` §8.4): заголовок ≤80 символов, первая
//! фраза — что случилось словами пользователя, 1–3 действия, внизу мелко — код
//! как в Chromium. Здесь — чистая часть (тексты и HTML); показ — `lumen/net_error.rs`.

use lumen_core::net_error::NetErrorCode;

use crate::crash_page::escape_html;
use crate::external_browser::{is_handoff_url, HandoffReason};
use crate::handoff_page::HANDOFF_URL;
use crate::PageSource;

/// Тексты страницы для кода: (заголовок, первая фраза, действия).
///
/// Вердикт «кто виноват» не выносится: проб нет, поэтому уровень — «не знаем»
/// или «вероятно», и слов вроде «сбой на стороне сайта» тут нет.
fn texts(code: NetErrorCode, host: &str) -> (String, String, &'static [&'static str]) {
    use NetErrorCode as C;
    match code {
        C::NameNotResolved => (
            format!("Не удаётся найти сайт {host}"),
            "Имя сайта не удалось превратить в адрес. Либо в адресе опечатка, либо нет связи с DNS-сервером.".into(),
            &[
                "Проверьте, нет ли опечатки в адресе.",
                "Проверьте подключение к интернету: другие сайты открываются?",
                "Если сайт открывается в другом браузере на этом компьютере, откройте его там.",
            ],
        ),
        C::ConnectionRefused => (
            format!("Сайт {host} отклонил подключение"),
            "Компьютер, на котором работает сайт, ответил отказом на попытку подключиться.".into(),
            &[
                "Повторите через несколько минут: сервис мог перезапускаться.",
                "Если вы используете VPN или прокси, попробуйте открыть сайт без них.",
            ],
        ),
        C::ConnectionTimedOut => (
            format!("Сайт {host} слишком долго не отвечает"),
            "Подключиться не удалось: ответ не пришёл за отведённое время.".into(),
            &[
                "Проверьте подключение к интернету.",
                "Повторите позже: сайт или сеть могут быть перегружены.",
                "Если вы используете VPN или прокси, попробуйте открыть сайт без них.",
            ],
        ),
        C::ConnectionReset | C::ConnectionClosed => (
            format!("Соединение с сайтом {host} прервано"),
            "Соединение было установлено, но оборвалось раньше, чем пришла страница целиком.".into(),
            &[
                "Повторите загрузку.",
                "Если ошибка повторяется на разных сайтах, проверьте антивирус и сетевые настройки.",
            ],
        ),
        C::AddressUnreachable => (
            format!("Не удаётся добраться до сайта {host}"),
            "До компьютера сайта нет маршрута: сеть или хост недоступны.".into(),
            &[
                "Проверьте подключение к интернету.",
                "Если вы используете VPN или прокси, проверьте их.",
            ],
        ),
        C::TooManyRedirects => (
            format!("Сайт {host} зациклил перенаправления"),
            "Сайт бесконечно отправляет браузер с одного адреса на другой.".into(),
            &[
                "Удалите cookie этого сайта и повторите: чаще всего петлю замыкает устаревший cookie.",
                "Если не помогло, проблема на стороне сайта.",
            ],
        ),
        C::InvalidResponse => (
            format!("Сайт {host} прислал непонятный ответ"),
            "Ответ сервера не удалось разобрать как HTTP.".into(),
            &["Повторите загрузку.", "Если ошибка постоянная, откройте сайт в другом браузере."],
        ),
        C::SslProtocolError => (
            format!("Не удалось установить защищённое соединение с {host}"),
            "Сайт и Lumen не договорились о параметрах шифрования.".into(),
            &[
                "Повторите загрузку.",
                "Если вы используете антивирус с проверкой HTTPS, попробуйте открыть сайт при выключенной проверке.",
            ],
        ),
        C::FileNotFound => (
            "Файл не найден".to_owned(),
            "По этому адресу на компьютере нет файла или папки.".into(),
            &["Проверьте путь: файл могли переместить или удалить."],
        ),
        C::InvalidUrl => (
            "Адрес указан неверно".to_owned(),
            "Адрес не удалось разобрать.".into(),
            &["Проверьте адрес на опечатки."],
        ),
        C::UnknownUrlScheme => (
            "Lumen не открывает адреса такого вида".to_owned(),
            "Схема адреса (часть до двоеточия) не поддерживается.".into(),
            &["Если адрес должен открывать другое приложение, откройте его вручную."],
        ),
        C::BlockedByClient => (
            "Загрузка заблокирована".to_owned(),
            "Lumen не стал загружать эту страницу: её заблокировала политика безопасности или защита от трекеров.".into(),
            &["Если страница нужна, отключите защиту для этого сайта на значке щита."],
        ),
        C::Http2ProtocolError => (
            format!("Соединение с сайтом {host} оборвано по ошибке протокола"),
            "Сервер (или промежуточный узел) закрыл соединение HTTP/2 с кодом ошибки, а не штатно.".into(),
            &[
                "Повторите загрузку: сбой часто разовый.",
                "Если вы используете антивирус, VPN или прокси, попробуйте открыть сайт без них.",
                "Если ошибка постоянная, откройте сайт в другом браузере.",
            ],
        ),
        C::TunnelConnectionFailed => (
            format!("Прокси не открыл соединение с сайтом {host}"),
            "Прокси-сервер отказался передавать защищённое соединение к сайту.".into(),
            &[
                "Проверьте настройки прокси.",
                "Попробуйте открыть сайт без прокси или через другой.",
            ],
        ),
        C::ContentDecodingFailed => (
            format!("Страница сайта {host} пришла повреждённой"),
            "Ответ сервера заявлен как сжатый, но распаковать его не удалось.".into(),
            &[
                "Повторите загрузку: возможно, ответ повредился в пути.",
                "Если ошибка постоянная, откройте сайт в другом браузере.",
            ],
        ),
        C::HttpStatus(status) => (
            format!("Сайт {host} не отдал страницу (HTTP {status})"),
            format!("Сервер сайта ответил кодом {status} ({}) и не прислал ни одной страницы.", status_meaning(status)),
            &[
                "Подождите: такие сбои обычно проходят за несколько минут.",
                "Если ошибка постоянная, откройте сайт в другом браузере.",
            ],
        ),
        C::Failed => (
            "Не удалось загрузить страницу".to_owned(),
            "Причину определить не удалось.".into(),
            &["Повторите загрузку.", "Если ошибка постоянная, откройте страницу в другом браузере."],
        ),
    }
}

/// Главный документ с кодом 5xx и пустым (или пробельным) телом — браузер
/// показывает вместо него страницу ошибки, как Chromium. С телом сервер
/// сам объяснил сбой, и его страницу не трогаем.
pub(crate) fn is_empty_server_error(status: u16, body: &[u8]) -> bool {
    (500..=599).contains(&status) && body.iter().all(u8::is_ascii_whitespace)
}

/// Что означает код 5xx — одним оборотом, без вердикта «кто виноват».
fn status_meaning(status: u16) -> &'static str {
    match status {
        502 => "сервер-посредник получил от основного сервера неверный ответ",
        503 => "сервер временно не принимает запросы",
        504 => "сервер-посредник не дождался основного сервера",
        _ => "сервер не смог обработать запрос",
    }
}

/// Паузы между автоповторами 5xx, секунды; после последней повтор останавливается.
const AUTO_RETRY_DELAYS: [u32; 3] = [5, 15, 45];

/// Автоповтор страницы ошибки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoRetry {
    /// Для этой ошибки автоповтора нет.
    Off,
    /// Страница сама повторит загрузку через столько секунд.
    In(u32),
    /// Попытки закончились — дальше только кнопка «Повторить».
    Exhausted,
}

impl AutoRetry {
    /// Автоповтор для `attempt`-го показа (с нуля) страницы ошибки с кодом
    /// `code`: только 502/503/504 — признак временного сбоя; прочие 5xx и
    /// сетевые сбои повторять вслепую нельзя (POST, перегруженный сервер).
    pub(crate) fn for_attempt(code: NetErrorCode, attempt: usize) -> Self {
        match code {
            NetErrorCode::HttpStatus(502..=504) => {
                AUTO_RETRY_DELAYS.get(attempt).map_or(Self::Exhausted, |s| Self::In(*s))
            }
            _ => Self::Off,
        }
    }
}

/// Причина передачи другому браузеру для кнопки «Открыть в другом браузере»
/// (COMPAT-HANDOFF); `None` — кнопки нет. Сбой рукопожатия — вероятно ГОСТ-TLS
/// (нужен Яндекс/Chromium-Gost), прочие сетевые сбои — «сайт не работает».
/// Блокировка политикой/защитой и ошибки адреса не обходятся: другой браузер
/// дал бы ту же ошибку или снял бы защиту, которую пользователь включил.
fn handoff_reason(code: NetErrorCode) -> Option<HandoffReason> {
    use NetErrorCode as C;
    match code {
        C::SslProtocolError => Some(HandoffReason::Crypto),
        C::FileNotFound | C::InvalidUrl | C::UnknownUrlScheme | C::BlockedByClient => None,
        _ => Some(HandoffReason::Other),
    }
}

/// Выбор другой причины под основной ссылкой: от причины зависит, какой
/// браузер подойдёт (ГОСТ-TLS — не Chrome/Edge, DRM и звонки — не ГОСТ-браузер).
fn other_reasons(chosen: HandoffReason) -> String {
    let links: Vec<String> = [HandoffReason::Crypto, HandoffReason::Drm, HandoffReason::Calls, HandoffReason::Other]
        .into_iter()
        .filter(|r| *r != chosen)
        .map(|r| format!("<a href=\"{}\">{}</a>", handoff_link(r), r.label()))
        .collect();
    format!("<p class=\"alt\">Причина другая: {}</p>", links.join(" · "))
}

/// Ссылка кнопки; принимается только пока на экране сама страница ошибки
/// (`Lumen::apply_net_error_link`).
fn handoff_link(reason: HandoffReason) -> String {
    format!("{HANDOFF_URL}?open={}", reason.code())
}

/// Причина из резолвнутой ссылки кнопки; `None` — не ссылка этой страницы.
#[must_use]
pub(crate) fn parse_handoff_link(url: &str) -> Option<HandoffReason> {
    let code = url.strip_prefix(HANDOFF_URL)?.strip_prefix("?open=")?;
    HandoffReason::from_code(code)
}

/// Хост адреса для заголовка; без хоста (файл, пустой адрес) — сам адрес.
fn host_of(url: &str) -> String {
    lumen_core::url::Url::parse(url)
        .ok()
        .map(|u| u.host().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| url.to_owned())
}

/// HTML страницы ошибки. `url` — адрес несостоявшейся навигации (кнопка
/// «Повторить» ведёт на него), `detail` — исходный текст ошибки: попадает в
/// мелкий блок внизу, чтобы сообщение об ошибке было с чем сверять.
pub(crate) fn build_net_error_html(url: &str, code: NetErrorCode, detail: &str, auto_retry: AutoRetry) -> String {
    let (title, lead, actions) = texts(code, &escape_html(&host_of(url)));
    let items: String = actions.iter().map(|a| format!("<li>{}</li>", escape_html(a))).collect();
    let retry = if url.is_empty() {
        String::new()
    } else {
        format!("<p><a class=\"btn\" href=\"{}\">Повторить</a></p>", escape_html(url))
    };
    // Автоповтор — `<meta refresh>` на сам адрес: страница ошибки показывается
    // под адресом несостоявшейся навигации, и таймер заново загружает его.
    let (refresh, auto_note) = match auto_retry {
        AutoRetry::In(secs) if !url.is_empty() => (
            format!("<meta http-equiv=\"refresh\" content=\"{secs};url={}\">", escape_html(url)),
            format!("<p class=\"alt\">Страница сама повторит загрузку через {secs} с.</p>"),
        ),
        AutoRetry::Exhausted => (
            String::new(),
            "<p class=\"alt\">Автоматические повторы закончились: нажмите «Повторить», когда будет удобно.</p>".to_owned(),
        ),
        _ => (String::new(), String::new()),
    };
    let other = match handoff_reason(code) {
        Some(reason) if is_handoff_url(url) => {
            format!(
                "<p><a href=\"{}\">Открыть в другом браузере</a></p>{}",
                handoff_link(reason),
                other_reasons(reason)
            )
        }
        _ => String::new(),
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">{refresh}<title>{title}</title>\
<style>body{{font-family:sans-serif;margin:0;padding:80px 40px;background:#f4f4f6;color:#222}}\
h1{{font-size:26px;max-width:640px}}p,li{{font-size:16px;max-width:640px}}li{{margin:4px 0}}\
a.btn{{display:inline-block;padding:10px 22px;background:#2b6cdf;color:#fff;border-radius:6px;\
text-decoration:none}}p.alt{{font-size:13px;color:#555}}.code{{margin-top:48px;font-size:12px;color:#777}}\
pre{{font-size:12px;color:#777;max-width:640px;white-space:pre-wrap}}</style></head>\
<body><h1>{title}</h1><p>{lead}</p><ul>{items}</ul>{retry}{auto_note}{other}\
<div class=\"code\">{code_name}</div><pre>{detail}</pre></body></html>",
        lead = escape_html(&lead),
        code_name = code.name(),
        detail = escape_html(detail),
    )
}

/// Источник страницы ошибки для навигации на `url`.
pub(crate) fn net_error_source(url: &str, code: NetErrorCode, detail: &str, auto_retry: AutoRetry) -> PageSource {
    PageSource::Static { html: build_net_error_html(url, code, detail, auto_retry), url: url.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_names_host_and_chromium_code() {
        let html = build_net_error_html("https://nope.example/a?b=1", NetErrorCode::NameNotResolved, "resolve nope.example: x", AutoRetry::Off);
        assert!(html.contains("Не удаётся найти сайт nope.example"));
        assert!(html.contains("ERR_NAME_NOT_RESOLVED"));
        assert!(html.contains("href=\"https://nope.example/a?b=1\""));
        assert!(html.contains("resolve nope.example: x"));
    }

    #[test]
    fn page_escapes_url_and_detail() {
        let html = build_net_error_html("http://a/?q=\"><script>", NetErrorCode::Failed, "<b>&", AutoRetry::Off);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;b&gt;&amp;"));
    }

    #[test]
    fn titles_fit_the_80_char_rule_and_no_blame_words() {
        let host = "очень-длинное-имя-хоста.example";
        for code in [
            NetErrorCode::NameNotResolved,
            NetErrorCode::ConnectionRefused,
            NetErrorCode::ConnectionTimedOut,
            NetErrorCode::ConnectionReset,
            NetErrorCode::AddressUnreachable,
            NetErrorCode::TooManyRedirects,
            NetErrorCode::InvalidResponse,
            NetErrorCode::SslProtocolError,
            NetErrorCode::Http2ProtocolError,
            NetErrorCode::TunnelConnectionFailed,
            NetErrorCode::ContentDecodingFailed,
            NetErrorCode::FileNotFound,
            NetErrorCode::InvalidUrl,
            NetErrorCode::UnknownUrlScheme,
            NetErrorCode::BlockedByClient,
            NetErrorCode::HttpStatus(502),
            NetErrorCode::HttpStatus(503),
            NetErrorCode::HttpStatus(504),
            NetErrorCode::HttpStatus(500),
            NetErrorCode::Failed,
        ] {
            let (title, lead, actions) = texts(code, host);
            assert!(title.chars().count() <= 80, "{code:?}: {title}");
            assert!((1..=3).contains(&actions.len()), "{code:?}");
            for t in [title.as_str(), lead.as_str()] {
                assert!(!t.contains("атак") && !t.contains("угроз"), "{code:?}: {t}");
            }
        }
    }

    #[test]
    fn other_browser_link_only_for_http_and_not_for_blocks() {
        let html = build_net_error_html("https://a.example/", NetErrorCode::ConnectionTimedOut, "", AutoRetry::Off);
        assert!(html.contains("href=\"about:handoff?open=other\">Открыть в другом браузере"));
        let html = build_net_error_html("https://a.example/", NetErrorCode::SslProtocolError, "", AutoRetry::Off);
        assert!(html.contains("about:handoff?open=crypto"));
        for code in [NetErrorCode::BlockedByClient, NetErrorCode::InvalidUrl, NetErrorCode::FileNotFound] {
            let html = build_net_error_html("https://a.example/", code, "", AutoRetry::Off);
            assert!(!html.contains("about:handoff"), "{code:?}");
        }
        let html = build_net_error_html("file:///x", NetErrorCode::Failed, "", AutoRetry::Off);
        assert!(!html.contains("about:handoff"));
    }

    #[test]
    fn other_reasons_are_offered_except_the_chosen_one() {
        let html = build_net_error_html("https://a.example/", NetErrorCode::ConnectionTimedOut, "", AutoRetry::Off);
        for code in ["crypto", "drm", "calls"] {
            assert!(html.contains(&format!("href=\"about:handoff?open={code}\"")), "{code}");
        }
        assert_eq!(html.matches("about:handoff?open=other").count(), 1);
        let html = build_net_error_html("https://a.example/", NetErrorCode::SslProtocolError, "", AutoRetry::Off);
        assert_eq!(html.matches("about:handoff?open=crypto").count(), 1);
        assert!(html.contains("about:handoff?open=other\">сайт не работает"));
    }

    #[test]
    fn handoff_link_roundtrip() {
        for r in [HandoffReason::Crypto, HandoffReason::Other] {
            assert_eq!(parse_handoff_link(&handoff_link(r)), Some(r));
        }
        assert_eq!(parse_handoff_link("about:handoff?open=evil"), None);
        assert_eq!(parse_handoff_link("about:handoff?remove=a.example"), None);
    }

    #[test]
    fn file_source_has_no_retry_link() {
        let html = build_net_error_html("", NetErrorCode::FileNotFound, "", AutoRetry::Off);
        assert!(!html.contains("class=\"btn\""));
    }

    #[test]
    fn empty_5xx_page_names_status_and_refreshes_itself() {
        let url = "https://a.example/p?x=1&y=2";
        let html = build_net_error_html(url, NetErrorCode::HttpStatus(503), "http status 503", AutoRetry::In(15));
        assert!(html.contains("Сайт a.example не отдал страницу (HTTP 503)"));
        assert!(html.contains("ERR_HTTP_RESPONSE_CODE_FAILURE"));
        assert!(html.contains("<meta http-equiv=\"refresh\" content=\"15;url=https://a.example/p?x=1&amp;y=2\">"));
        assert!(html.contains("повторит загрузку через 15 с"));
        assert!(html.contains("about:handoff?open=other"));
    }

    #[test]
    fn auto_retry_backs_off_then_stops_and_only_for_gateway_codes() {
        let c = NetErrorCode::HttpStatus(502);
        assert_eq!(AutoRetry::for_attempt(c, 0), AutoRetry::In(5));
        assert_eq!(AutoRetry::for_attempt(c, 1), AutoRetry::In(15));
        assert_eq!(AutoRetry::for_attempt(c, 2), AutoRetry::In(45));
        assert_eq!(AutoRetry::for_attempt(c, 3), AutoRetry::Exhausted);
        assert_eq!(AutoRetry::for_attempt(NetErrorCode::HttpStatus(500), 0), AutoRetry::Off);
        assert_eq!(AutoRetry::for_attempt(NetErrorCode::ConnectionTimedOut, 0), AutoRetry::Off);
        let html = build_net_error_html("https://a.example/", c, "", AutoRetry::Exhausted);
        assert!(!html.contains("http-equiv"));
        assert!(html.contains("Автоматические повторы закончились"));
    }

    #[test]
    fn only_5xx_without_a_body_is_replaced() {
        assert!(is_empty_server_error(503, b""));
        assert!(is_empty_server_error(500, b" \t\n"));
        assert!(!is_empty_server_error(503, b"<h1>maintenance</h1>"));
        assert!(!is_empty_server_error(404, b""));
        assert!(!is_empty_server_error(0, b""));
        assert!(!is_empty_server_error(200, b""));
    }
}

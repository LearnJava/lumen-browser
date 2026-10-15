//! COMPAT-GATE-DETECT: страница «защита сайта не пропускает Lumen»
//! (COMPAT-ERR-10, `docs/research/site-compat/report.md` §8.4).
//!
//! Показывается, когда один сайт трижды за минуту отдал антибот-стену
//! ([`crate::gate_detect::GateLoop`]). Уровень уверенности — «установлено»:
//! петля видна по самим ответам, поэтому текст прямой, а вина названа. Нет
//! кнопки «Повторить», смены User-Agent и очистки всех cookie — повторы в петле
//! не помогают, а подмена браузера с этой страницы вернула бы ту же стену.
//! Внизу — идентификатор проверки без IP и без адреса страницы. Чистая часть;
//! показ — `lumen/gate.rs`.

use crate::crash_page::escape_html;
use crate::external_browser::{is_handoff_url, HandoffReason};
use crate::gate_detect::Vendor;
use crate::net_error_page::handoff_link;
use crate::PageSource;

/// Хост адреса для заголовка; без хоста — сам адрес.
fn host_of(url: &str) -> String {
    lumen_core::url::Url::parse(url)
        .ok()
        .map(|u| u.host().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| url.to_owned())
}

/// Заголовок панели (≤80 символов).
#[must_use]
pub(crate) fn title(vendor: Vendor) -> String {
    match vendor {
        Vendor::Unknown => "Защита сайта не пропускает Lumen".to_owned(),
        v => format!("Защита сайта ({}) не пропускает Lumen", v.name()),
    }
}

/// HTML страницы. `url` — адрес, на котором стена повторилась (по нему кнопка
/// «Открыть в другом браузере» передаёт страницу), `check_id` — идентификатор
/// проверки из [`crate::gate_detect::check_id`].
#[must_use]
pub(crate) fn build_gate_html(url: &str, vendor: Vendor, check_id: &str) -> String {
    let title = escape_html(&title(vendor));
    let host = escape_html(&host_of(url));
    let open = if is_handoff_url(url) {
        format!("<p><a class=\"btn\" href=\"{}\">Открыть в другом браузере</a></p>", handoff_link(HandoffReason::Other))
    } else {
        String::new()
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
<style>body{{font-family:sans-serif;margin:0;padding:80px 40px;background:#f4f4f6;color:#222}}\
h1{{font-size:26px;max-width:640px}}p,li{{font-size:16px;max-width:640px}}li{{margin:4px 0}}\
a.btn{{display:inline-block;padding:10px 22px;background:#2b6cdf;color:#fff;border-radius:6px;\
text-decoration:none}}.code{{margin-top:48px;font-size:12px;color:#777;max-width:640px}}</style></head>\
<body><h1>{title}</h1>\
<p>Сайт {host} несколько раз подряд показал проверку, которую Lumen не проходит. \
Дело не в вашей сети, повторы не помогут.</p>\
<ul><li>Откройте страницу в другом браузере: там проверка может пройти.</li>\
<li>Если сайт нужен вам постоянно, сообщите о проблеме и приложите ID проверки.</li></ul>\
{open}\
<div class=\"code\">ID проверки: {check_id}<br>\
В нём нет IP-адреса и адреса страницы: только сайт, название защиты и время с точностью до минуты.</div>\
</body></html>",
        check_id = escape_html(check_id),
    )
}

/// Источник страницы для навигации на `url`.
#[must_use]
pub(crate) fn gate_source(url: &str, vendor: Vendor, check_id: &str) -> PageSource {
    PageSource::Static { html: build_gate_html(url, vendor, check_id), url: url.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net_error_page::parse_handoff_link;

    #[test]
    fn titles_fit_and_name_the_vendor() {
        for v in [
            Vendor::Cloudflare,
            Vendor::DdosGuard,
            Vendor::Qrator,
            Vendor::SmartCaptcha,
            Vendor::Akamai,
            Vendor::DataDome,
            Vendor::Wildberries,
            Vendor::Avito,
            Vendor::Ozon,
            Vendor::Unknown,
        ] {
            let t = title(v);
            assert!(t.chars().count() <= 80, "{t}");
            assert!(t.contains("Lumen"), "{t}");
        }
        assert!(title(Vendor::Cloudflare).contains("Cloudflare"));
    }

    #[test]
    fn page_offers_handoff_and_check_id_but_no_retry_and_no_ip() {
        let html = build_gate_html("https://a.example/x?q=1", Vendor::Cloudflare, "AB12-CD34");
        assert!(html.contains("Открыть в другом браузере"));
        assert!(html.contains("ID проверки: AB12-CD34"));
        assert!(html.contains("Дело не в вашей сети"));
        assert!(!html.contains("Повторить"));
        assert!(!html.to_lowercase().contains("user-agent"));
        assert!(!html.contains("cookie"));
        // Адрес страницы в самом тексте не печатается — только хост.
        assert!(!html.contains("q=1"));
        let link = html.split("href=\"").nth(1).and_then(|r| r.split('"').next()).unwrap();
        assert_eq!(parse_handoff_link(link), Some(HandoffReason::Other));
    }

    #[test]
    fn page_without_http_address_has_no_handoff_button() {
        let html = build_gate_html("file:///x", Vendor::Unknown, "0000-0000");
        assert!(!html.contains("Открыть в другом браузере"));
    }

    #[test]
    fn host_is_escaped() {
        let html = build_gate_html("https://a.example/", Vendor::Qrator, "<b>");
        assert!(!html.contains("<b>"));
    }
}

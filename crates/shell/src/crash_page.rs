//! UX-CRASH: экран «Страница упала» и перехват паник конвейера вкладки.
//!
//! Процесса рендера нет (UX-PROCESS), поэтому паника в загрузке/JS/layout/paint
//! раньше валила весь браузер. Промежуточный шаг: события окна и движковые
//! задания исполняются под `catch_unwind`, а вместо упавшей страницы вкладка
//! показывает статический экран с кнопкой перезагрузки — остальные вкладки
//! живы. Здесь — чистая часть: HTML экрана и разбор текста паники.

use crate::PageSource;

/// Текст паники из payload `catch_unwind`.
pub(crate) fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "паника без текста".to_owned()
    }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// HTML экрана падения. `url` — адрес упавшей страницы (кнопка ведёт на него),
/// `detail` — сообщение паники.
pub(crate) fn build_crash_html(url: &str, detail: &str) -> String {
    let url = escape_html(url);
    let detail = escape_html(detail);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Страница упала</title>\
<style>body{{font-family:sans-serif;margin:0;padding:80px 40px;background:#f4f4f6;color:#222}}\
h1{{font-size:28px}}p{{font-size:16px;max-width:640px}}\
a.btn{{display:inline-block;padding:10px 22px;background:#2b6cdf;color:#fff;border-radius:6px;\
text-decoration:none}}pre{{background:#e6e6ea;padding:12px;max-width:640px;white-space:pre-wrap}}\
</style></head><body><h1>Страница упала</h1>\
<p>Во время обработки страницы произошла внутренняя ошибка. Остальные вкладки не затронуты.</p>\
<p><a class=\"btn\" href=\"{url}\">Перезагрузить страницу</a></p>\
<p>{url}</p><pre>{detail}</pre></body></html>"
    )
}

/// Источник экрана падения для страницы `url`.
pub(crate) fn crash_source(url: &str, detail: &str) -> PageSource {
    PageSource::Static { html: build_crash_html(url, detail), url: url.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_html_escapes_url_and_detail() {
        let html = build_crash_html("http://a/?q=\"><script>", "bad <b> & co");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;b&gt; &amp; co"));
        assert!(html.contains("href=\"http://a/?q=&quot;&gt;&lt;script&gt;\""));
    }

    #[test]
    fn panic_text_reads_str_and_string_payloads() {
        assert_eq!(panic_text(&"раз"), "раз");
        assert_eq!(panic_text(&String::from("два")), "два");
        assert_eq!(panic_text(&5_u32), "паника без текста");
    }
}

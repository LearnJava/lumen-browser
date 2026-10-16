//! Страницы отчёта о поломке (COMPAT-REPORT): `about:report` (форма),
//! `about:report-preview` (предпросмотр и экспорт) и `about:report-done`
//! («что дальше»). Страницы без JS: форма — обычная GET-форма, действия
//! предпросмотра — спец-ссылки, которые перехватывает шелл (та же схема, что
//! у `about:handoff` и `about:compat`). Чистая часть — HTML и разбор ссылок;
//! состояние и исполнение — `crate::lumen::report`.

use lumen_core::form::decode_form_value;

use crate::compat_report::{Draft, Reason};
use crate::newtab::escape_html;

/// Форма отчёта.
pub const REPORT_URL: &str = "about:report";
/// Предпросмотр отчёта.
pub const PREVIEW_URL: &str = "about:report-preview";
/// Экран после экспорта.
pub const DONE_URL: &str = "about:report-done";

/// `true` для любой из трёх страниц отчёта.
#[must_use]
pub fn is_report_url(url: &str) -> bool {
    [REPORT_URL, PREVIEW_URL, DONE_URL].iter().any(|page| {
        url.strip_prefix(page).is_some_and(|rest| rest.is_empty() || rest.starts_with('?'))
    })
}

/// Действие на странице предпросмотра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewAction {
    /// Сохранить отчёт файлом в папку загрузок.
    Save,
    /// Положить отчёт в буфер обмена.
    Copy,
    /// Вернуться к форме.
    Edit,
}

/// Действие из резолвнутого URL ссылки предпросмотра; `None` — не ссылка этой страницы.
#[must_use]
pub fn parse_preview_action(url: &str) -> Option<PreviewAction> {
    match url.strip_prefix(PREVIEW_URL)?.strip_prefix("?do=")? {
        "save" => Some(PreviewAction::Save),
        "copy" => Some(PreviewAction::Copy),
        "edit" => Some(PreviewAction::Edit),
        _ => None,
    }
}

/// Ввод формы из URL отправки GET-формы `about:report?...`; `None` — это не отправка формы.
#[must_use]
pub fn parse_form_submit(url: &str) -> Option<Draft> {
    let query = url.strip_prefix(REPORT_URL)?.strip_prefix('?')?;
    let mut draft = Draft::default();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode_form_value(value);
        match key {
            "reason" => draft.reason = Reason::from_code(&value),
            "note" => draft.note = value,
            "full_url" => draft.full_url = !value.is_empty(),
            "blocked" => draft.blocked_list = !value.is_empty(),
            _ => {}
        }
    }
    Some(draft)
}

/// Что показывает форма.
#[derive(Debug, Clone)]
pub struct FormView<'a> {
    /// Сайт, о котором сообщают (хост).
    pub host: &'a str,
    /// Адрес, который уйдёт по умолчанию (`scheme://host/path`).
    pub short_url: &'a str,
    /// Причины в порядке показа.
    pub order: &'a [Reason],
    /// Прежний выбор (возврат из предпросмотра).
    pub draft: &'a Draft,
    /// «Мы знаем об этой проблеме…», если сайт есть в реестре известных проблем.
    pub known: Option<&'a str>,
    /// Сколько запросов защиты заблокировали на странице.
    pub blocked_count: u32,
    /// Эта страница не сайт (нечего сообщать).
    pub no_site: bool,
}

const STYLE: &str = "\
body{margin:0;padding:48px 40px;background:#1b1b1f;color:#e8e8ea;font-family:'Inter',sans-serif}\
h1{font-size:26px;font-weight:600;margin:0 0 8px 0}\
h2{font-size:16px;font-weight:600;margin:24px 0 8px 0}\
.lead{color:#9a9aa2;font-size:14px;max-width:720px;margin:0 0 20px 0}\
.box{max-width:720px;padding:14px 18px;margin-bottom:14px;background:#27272c;border-radius:8px;font-size:14px}\
.known{border-left:4px solid #e8c46a}\
.warn{border-left:4px solid #d9534f}\
.opt{display:block;padding:5px 0;font-size:14px}\
.fine{color:#9a9aa2;font-size:13px;margin-top:4px}\
textarea{width:700px;height:90px;background:#1b1b1f;color:#e8e8ea;border:1px solid #4a4a52;border-radius:6px;padding:8px;font-size:14px}\
pre{max-width:720px;padding:14px 18px;background:#101013;border-radius:8px;font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere}\
.btn,button{display:inline-block;padding:8px 16px;margin:8px 8px 0 0;border-radius:6px;background:#3a3a40;\
color:#e8e8ea;text-decoration:none;font-size:14px;border:0}\
.primary{background:#2b6cdf;color:#fff}";

fn page(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"utf-8\"><title>{}</title>\
<style>{STYLE}</style></head><body>{body}</body></html>",
        escape_html(title)
    )
}

/// Страница-форма `about:report`.
#[must_use]
pub fn build_form_html(view: &FormView<'_>) -> String {
    if view.no_site {
        return page(
            "Сообщить о проблеме с сайтом",
            "<h1>Сообщить о проблеме с сайтом</h1>\
<p class=\"lead\">Откройте страницу сайта, который работает не так, и снова выберите «Сообщить о проблеме» \
в окне сайта (щит в адресной строке) или в палитре команд.</p>",
        );
    }
    let mut body = format!(
        "<h1>Сообщить о проблеме с сайтом</h1>\
<p class=\"lead\">Сайт: <b>{}</b>. Отчёт — разовое действие: ничего не отправляется само, \
сначала вы увидите всё, что в него войдёт, и сами решите, что с ним делать. \
Если сайт работает в Chrome, но не в Lumen, — это наша задача.</p>",
        escape_html(view.host)
    );
    if let Some(known) = view.known {
        body.push_str(&format!("<div class=\"box known\">{}</div>", escape_html(known)));
    }
    body.push_str("<form action=\"about:report\" method=\"get\"><h2>Что не так?</h2><div class=\"box\">");
    for reason in view.order {
        let checked = if view.draft.reason == Some(*reason) { " checked" } else { "" };
        body.push_str(&format!(
            "<label class=\"opt\"><input type=\"radio\" name=\"reason\" value=\"{}\"{checked}> {}</label>",
            reason.code(),
            escape_html(reason.label())
        ));
    }
    body.push_str(&format!(
        "</div><h2>Подробности (необязательно)</h2>\
<textarea name=\"note\" maxlength=\"2000\">{}</textarea>\
<div class=\"fine\">Адреса e-mail, телефоны, номера карт и токены вычищаются до предпросмотра.</div>\
<h2>Что приложить сверх обычного</h2><div class=\"box\">\
<label class=\"opt\"><input type=\"checkbox\" name=\"full_url\" value=\"1\"{}> Полный адрес страницы \
<span class=\"fine\">(по умолчанию — только {})</span></label>\
<label class=\"opt\"><input type=\"checkbox\" name=\"blocked\" value=\"1\"{}> Список заблокированного \
<span class=\"fine\">(хосты запросов, отсечённых защитой: {})</span></label>\
</div><button class=\"primary\" type=\"submit\">Показать, что уйдёт</button></form>",
        escape_html(&view.draft.note),
        if view.draft.full_url { " checked" } else { "" },
        escape_html(view.short_url),
        if view.draft.blocked_list { " checked" } else { "" },
        view.blocked_count,
    ));
    page("Сообщить о проблеме с сайтом", &body)
}

/// Что показывает предпросмотр.
#[derive(Debug, Clone)]
pub struct PreviewView<'a> {
    /// Точный текст отчёта — то, что уйдёт в файл или буфер.
    pub text: &'a str,
    /// По этому сайту сегодня отчёт уже был: экспорт недоступен.
    pub duplicate_today: bool,
}

/// Страница-предпросмотр `about:report-preview`.
#[must_use]
pub fn build_preview_html(view: &PreviewView<'_>) -> String {
    let mut body = String::from(
        "<h1>Что войдёт в отчёт</h1>\
<p class=\"lead\">Это весь отчёт целиком. В нём нет cookie, IP-адреса и идентификатора вашего браузера.</p>",
    );
    body.push_str(&format!("<pre>{}</pre>", escape_html(view.text)));
    if view.duplicate_today {
        body.push_str(
            "<div class=\"box warn\">По этому сайту сегодня отчёт уже был, повторный не нужен. \
Если стало лучше или хуже — сообщите завтра.</div>\
<a class=\"btn\" href=\"about:report-preview?do=edit\">Изменить</a>",
        );
    } else {
        body.push_str(
            "<a class=\"btn primary\" href=\"about:report-preview?do=save\">Сохранить файлом</a>\
<a class=\"btn\" href=\"about:report-preview?do=copy\">Скопировать в буфер</a>\
<a class=\"btn\" href=\"about:report-preview?do=edit\">Изменить</a>",
        );
    }
    page("Предпросмотр отчёта", &body)
}

/// Куда ушёл отчёт.
#[derive(Debug, Clone)]
pub enum Delivered {
    /// Файл по этому пути.
    File(String),
    /// В буфер обмена.
    Clipboard,
}

/// Страница «что дальше» `about:report-done`.
#[must_use]
pub fn build_done_html(delivered: &Delivered, remembered: bool) -> String {
    let where_to = match delivered {
        Delivered::File(path) => format!("Отчёт сохранён: <b>{}</b>.", escape_html(path)),
        Delivered::Clipboard => "Отчёт скопирован в буфер обмена.".to_owned(),
    };
    let next = if remembered {
        "Мы запомнили сайт на этом компьютере: когда пакет совместимости обновится и для сайта появится \
правка, Lumen скажет, что он теперь работает."
    } else {
        "Анонимный профиль ничего не запоминает, поэтому сообщения «сайт теперь работает» не будет."
    };
    page(
        "Отчёт готов",
        &format!(
            "<h1>Отчёт готов</h1><div class=\"box\">{where_to}</div>\
<h2>Что дальше</h2>\
<p class=\"lead\">Пока у отчётов нет приёмника, Lumen ничего не отправляет сам: файлом или текстом из буфера \
вы можете поделиться на странице проекта. {next}</p>\
<p class=\"lead\">Если сайт работает в Chrome, но не в Lumen, — это наша задача, а не ваша.</p>"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat_report::shuffled_reasons;

    fn view<'a>(order: &'a [Reason], draft: &'a Draft) -> FormView<'a> {
        FormView {
            host: "bank.ru",
            short_url: "https://bank.ru/login",
            order,
            draft,
            known: None,
            blocked_count: 3,
            no_site: false,
        }
    }

    #[test]
    fn form_has_every_reason_radio_with_other_last() {
        let order = shuffled_reasons(5);
        let html = build_form_html(&view(&order, &Draft::default()));
        for r in Reason::ALL {
            assert!(html.contains(&format!("value=\"{}\"", r.code())), "нет причины {}", r.code());
            assert!(html.contains(r.label()), "нет подписи {}", r.label());
        }
        let last = html.rfind("name=\"reason\"").expect("радиокнопки есть");
        assert!(html[last..].starts_with("name=\"reason\" value=\"other\""), "«Другое» не последняя");
        assert!(html.contains("<textarea name=\"note\""));
    }

    #[test]
    fn form_restores_the_previous_choice() {
        let order = shuffled_reasons(5);
        let draft = Draft { reason: Some(Reason::Vpn), note: "a <b>".into(), full_url: true, blocked_list: false };
        let html = build_form_html(&view(&order, &draft));
        assert!(html.contains("value=\"vpn\" checked"));
        assert!(html.contains("a &lt;b&gt;"));
        assert!(html.contains("name=\"full_url\" value=\"1\" checked"));
        assert!(!html.contains("name=\"blocked\" value=\"1\" checked"));
    }

    #[test]
    fn known_issue_is_shown_above_the_form() {
        let order = shuffled_reasons(1);
        let draft = Draft::default();
        let mut v = view(&order, &draft);
        v.known = Some("Мы знаем об этой проблеме: капча зацикливается (BUG-1)");
        let html = build_form_html(&v);
        let known = html.find("Мы знаем об этой проблеме").expect("сообщение есть");
        assert!(known < html.find("<form").expect("форма есть"));
    }

    #[test]
    fn non_site_page_has_no_form() {
        let order = shuffled_reasons(1);
        let draft = Draft::default();
        let mut v = view(&order, &draft);
        v.no_site = true;
        assert!(!build_form_html(&v).contains("<form"));
    }

    #[test]
    fn submit_url_round_trips_without_text() {
        let draft = parse_form_submit("about:report?reason=captcha&full_url=1").expect("отправка формы");
        assert_eq!(draft.reason, Some(Reason::Captcha));
        assert!(draft.full_url && !draft.blocked_list);
        assert!(draft.note.is_empty(), "отправка без текста возможна");
        let draft = parse_form_submit("about:report?reason=&note=%D0%BF%D1%80%D0%B8%D0%B2%D0%B5%D1%82+%D0%BC%D0%B8%D1%80")
            .expect("отправка формы");
        assert_eq!(draft.reason, None);
        assert_eq!(draft.note, "привет мир");
        assert_eq!(parse_form_submit("about:report"), None);
        assert_eq!(parse_form_submit("https://x.ru/about:report?reason=vpn"), None);
    }

    #[test]
    fn preview_actions_parse_only_from_the_preview_page() {
        assert_eq!(parse_preview_action("about:report-preview?do=save"), Some(PreviewAction::Save));
        assert_eq!(parse_preview_action("about:report-preview?do=copy"), Some(PreviewAction::Copy));
        assert_eq!(parse_preview_action("about:report-preview?do=edit"), Some(PreviewAction::Edit));
        assert_eq!(parse_preview_action("about:report-preview?do=send"), None);
        assert_eq!(parse_preview_action("about:report?do=save"), None);
        assert_eq!(parse_preview_action("about:compat?do=save"), None);
    }

    #[test]
    fn report_urls_are_recognised() {
        assert!(is_report_url("about:report"));
        assert!(is_report_url("about:report-preview"));
        assert!(is_report_url("about:report-done"));
        assert!(is_report_url("about:report?reason=vpn"));
        assert!(!is_report_url("about:reporter"));
        assert!(!is_report_url("https://x.ru/about:report"));
    }

    #[test]
    fn preview_escapes_the_report_and_hides_export_for_duplicates() {
        let html = build_preview_html(&PreviewView { text: "{\"note\": \"<script>\"}", duplicate_today: false });
        assert!(html.contains("&lt;script&gt;") && !html.contains("<script>"));
        assert!(html.contains("do=save") && html.contains("do=copy"));
        let dup = build_preview_html(&PreviewView { text: "{}", duplicate_today: true });
        assert!(!dup.contains("do=save") && !dup.contains("do=copy"));
        assert!(dup.contains("уже был") && dup.contains("do=edit"));
    }

    #[test]
    fn done_page_states_the_chrome_position_and_anonymous_caveat() {
        let html = build_done_html(&Delivered::File("C:\\Загрузки\\r.json".into()), true);
        assert!(html.contains("Если сайт работает в Chrome, но не в Lumen, — это наша задача"));
        assert!(html.contains("теперь работает"));
        let anon = build_done_html(&Delivered::Clipboard, false);
        assert!(anon.contains("Анонимный профиль ничего не запоминает"));
    }
}

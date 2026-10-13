//! `about:handoff` — страница «Сайты, открытые в другом браузере»
//! (COMPAT-HANDOFF, COMPAT-HO-04).
//!
//! Показывает живые правила [`lumen_storage::ExternalBrowserRules`]: сайт, браузер,
//! причину, сколько дней осталось. Действия — спец-ссылки `about:handoff?...`
//! ([`HandoffAction`] / [`parse_action`]), та же схема, что у `about:newtab`:
//! страница без JS, клик перехватывает шелл. Чистая часть — HTML и разбор ссылок;
//! чтение правил и исполнение действий — `crate::lumen::handoff`.

use lumen_core::form::{decode_form_value, encode_form_urlencoded, FormEntry};

use crate::newtab::escape_html;

/// Канонический URL страницы.
pub const HANDOFF_URL: &str = "about:handoff";

/// Секунд в сутках.
const DAY_SECS: i64 = 24 * 60 * 60;

/// Действие, закодированное в спец-ссылке `about:handoff?...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffAction {
    /// «Убрать»: снять правило, ссылки на сайт снова открываются в Lumen.
    Remove { site: String },
    /// «Проверить снова в Lumen»: снять правило и открыть сайт в Lumen.
    Recheck { site: String },
    /// «Оставить там»: продлить правило на 30 дней с текущей версией Lumen.
    Keep { site: String },
}

/// Разобрать резолвнутый URL кликнутой ссылки; `None` — не ссылка этой страницы.
#[must_use]
pub fn parse_action(url: &str) -> Option<HandoffAction> {
    let query = url.strip_prefix(HANDOFF_URL)?.strip_prefix('?')?;
    let (key, value) = query.split_once('=')?;
    let site = decode_form_value(value);
    if site.is_empty() {
        return None;
    }
    match key {
        "remove" => Some(HandoffAction::Remove { site }),
        "recheck" => Some(HandoffAction::Recheck { site }),
        "keep" => Some(HandoffAction::Keep { site }),
        _ => None,
    }
}

fn action_link(key: &str, site: &str) -> String {
    format!("{HANDOFF_URL}?{}", encode_form_urlencoded(&[FormEntry::text(key, site)]))
}

/// Строка таблицы: одно живое правило, уже сведённое с реестром браузеров.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffRow {
    /// Сайт правила (eTLD+1).
    pub site: String,
    /// Имя браузера; `None`, если его больше нет в реестре.
    pub browser_name: Option<String>,
    /// Код причины (`HandoffReason::code`).
    pub reason: String,
    /// Unix-секунды окончания.
    pub expires_at: i64,
    /// Lumen обновился после создания правила — спросить «Проверить снова?».
    pub needs_recheck: bool,
}

/// Человекочитаемая причина по коду правила.
#[must_use]
pub fn reason_label(code: &str) -> &'static str {
    match code {
        "crypto" => "ГОСТ / электронная подпись",
        "drm" => "DRM-видео",
        "calls" => "видеозвонки",
        _ => "сайт не работает",
    }
}

/// Сколько суток (с округлением вверх) осталось правилу.
#[must_use]
pub fn days_left(expires_at: i64, now_unix: i64) -> i64 {
    let left = expires_at.saturating_sub(now_unix).max(0);
    (left + DAY_SECS - 1) / DAY_SECS
}

/// HTML страницы. Пустой список — пояснение, как правило появляется.
#[must_use]
pub fn build_handoff_html(rows: &[HandoffRow], now_unix: i64) -> String {
    let mut body = String::new();
    if rows.is_empty() {
        body.push_str(
            "<p class=\"empty\">Правил нет. Правило появляется, когда в палитре команд \
выбрано «Always Open This Site in Another Browser (30 days)»: ссылки на этот сайт \
30 дней открываются в выбранном браузере.</p>",
        );
    }
    for row in rows {
        let site = escape_html(&row.site);
        let browser = match &row.browser_name {
            Some(name) => escape_html(name),
            None => "браузер не найден — ссылки открываются в Lumen".to_owned(),
        };
        let reason = reason_label(&row.reason);
        let days = days_left(row.expires_at, now_unix);
        let mut buttons = String::new();
        if row.needs_recheck {
            buttons.push_str(&format!(
                "<a class=\"btn primary\" href=\"{}\">Проверить снова в Lumen</a>\
<a class=\"btn\" href=\"{}\">Оставить там</a>",
                escape_html(&action_link("recheck", &row.site)),
                escape_html(&action_link("keep", &row.site)),
            ));
        }
        buttons.push_str(&format!(
            "<a class=\"btn\" href=\"{}\">Убрать</a>",
            escape_html(&action_link("remove", &row.site)),
        ));
        let hint = if row.needs_recheck {
            "<div class=\"hint\">Lumen обновился с момента создания правила. \
Проверить снова в Lumen?</div>"
        } else {
            ""
        };
        body.push_str(&format!(
            "<div class=\"row\"><div class=\"info\"><div class=\"site\">{site}</div>\
<div class=\"meta\">{browser} · {reason} · осталось дней: {days}</div>{hint}</div>\
<div class=\"actions\">{buttons}</div></div>"
        ));
    }
    format!(
        "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"utf-8\">\
<title>Сайты, открытые в другом браузере</title><style>\
body{{margin:0;padding:48px 40px;background:#1b1b1f;color:#e8e8ea;font-family:'Inter',sans-serif}}\
h1{{font-size:26px;font-weight:600;margin:0 0 8px 0}}\
.lead{{color:#9a9aa2;font-size:14px;max-width:720px;margin:0 0 28px 0}}\
.row{{display:flex;align-items:center;justify-content:space-between;max-width:760px;\
padding:14px 18px;margin-bottom:10px;background:#27272c;border-radius:8px}}\
.site{{font-size:17px;font-weight:600}}\
.meta{{color:#9a9aa2;font-size:13px;margin-top:4px}}\
.hint{{color:#e8c46a;font-size:13px;margin-top:6px}}\
.actions{{display:flex;gap:8px}}\
.btn{{display:inline-block;padding:7px 14px;border-radius:6px;background:#3a3a40;\
color:#e8e8ea;text-decoration:none;font-size:13px}}\
.btn.primary{{background:#2b6cdf;color:#fff}}\
.empty{{color:#9a9aa2;font-size:15px;max-width:620px}}\
</style></head><body><h1>Сайты, открытые в другом браузере</h1>\
<p class=\"lead\">Ссылки на эти сайты открываются в другом установленном браузере. \
Правило действует 30 дней и создаётся только вашим кликом; на другие сайты оно не влияет.</p>\
{body}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(site: &str, recheck: bool) -> HandoffRow {
        HandoffRow {
            site: site.to_owned(),
            browser_name: Some("Google Chrome".to_owned()),
            reason: "drm".to_owned(),
            expires_at: 10 * DAY_SECS,
            needs_recheck: recheck,
        }
    }

    #[test]
    fn links_round_trip() {
        for (key, site) in [("remove", "example.com"), ("recheck", "a&b=c.ru"), ("keep", "пример.рф")] {
            let parsed = parse_action(&action_link(key, site)).expect("ссылка разбирается");
            let expected = match key {
                "remove" => HandoffAction::Remove { site: site.to_owned() },
                "recheck" => HandoffAction::Recheck { site: site.to_owned() },
                _ => HandoffAction::Keep { site: site.to_owned() },
            };
            assert_eq!(parsed, expected);
        }
    }

    #[test]
    fn foreign_urls_are_not_actions() {
        assert_eq!(parse_action("about:handoff"), None);
        assert_eq!(parse_action("about:handoff?remove="), None);
        assert_eq!(parse_action("about:handoff?drop=x.com"), None);
        assert_eq!(parse_action("about:newtab?remove=x.com"), None);
        assert_eq!(parse_action("https://x.com/about:handoff?remove=x.com"), None);
    }

    #[test]
    fn days_round_up_and_clamp() {
        assert_eq!(days_left(DAY_SECS + 1, 0), 2);
        assert_eq!(days_left(DAY_SECS, 0), 1);
        assert_eq!(days_left(1, 0), 1);
        assert_eq!(days_left(0, 100), 0);
    }

    #[test]
    fn empty_page_explains_rules() {
        let html = build_handoff_html(&[], 0);
        assert!(html.contains("Правил нет"));
        assert!(!html.contains("class=\"row\""));
    }

    #[test]
    fn rows_have_remove_and_recheck_only_when_needed() {
        let html = build_handoff_html(&[row("fresh.com", false), row("stale.com", true)], 0);
        assert_eq!(html.matches("class=\"row\"").count(), 2);
        assert_eq!(html.matches("Убрать").count(), 2);
        assert_eq!(html.matches("Проверить снова в Lumen</a>").count(), 1);
        assert!(html.contains("about:handoff?recheck=stale.com"));
        assert!(!html.contains("about:handoff?recheck=fresh.com"));
        assert!(html.contains("осталось дней: 10"));
    }

    #[test]
    fn missing_browser_and_markup_are_escaped() {
        let mut r = row("<b>x</b>.com", false);
        r.browser_name = None;
        let html = build_handoff_html(&[r], 0);
        assert!(!html.contains("<b>x</b>"));
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;.com"));
        assert!(html.contains("браузер не найден"));
    }
}

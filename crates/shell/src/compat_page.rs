//! `about:compat` — страница пакета совместимости (COMPAT-PACK-PAGE, ADR-034 п.4).
//!
//! Показывает каждую запись пакета (`lumen_update_manifest::compat_pack`): причину,
//! действие, сайты, ссылку на BUG/KB, `review_by` и выключатель; сверху — версия пакета,
//! общий выключатель «не использовать список Lumen» и журнал изменений. Действия — спец-ссылки
//! `about:compat?...` ([`CompatAction`] / [`parse_action`]), та же схема, что у `about:handoff`:
//! страница без JS, клик перехватывает шелл. Чистая часть — HTML, разбор ссылок и дата;
//! чтение пакета, настроек и исполнение действий — `crate::lumen::compat`.

use lumen_core::form::{decode_form_value, encode_form_urlencoded, FormEntry};
use lumen_update_manifest::compat_pack::{CompatEntry, CompatPack, CompatPolicy};

use crate::newtab::escape_html;

/// Канонический URL страницы.
pub const COMPAT_URL: &str = "about:compat";

/// Действие, закодированное в спец-ссылке `about:compat?...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatAction {
    /// Выключить запись `id`.
    EntryOff { id: String },
    /// Включить запись `id` обратно.
    EntryOn { id: String },
    /// «Не использовать список Lumen»: `true` — выключить весь пакет.
    PackOff(bool),
}

/// Разобрать резолвнутый URL кликнутой ссылки; `None` — не ссылка этой страницы.
#[must_use]
pub fn parse_action(url: &str) -> Option<CompatAction> {
    let query = url.strip_prefix(COMPAT_URL)?.strip_prefix('?')?;
    let (key, value) = query.split_once('=')?;
    let value = decode_form_value(value);
    match (key, value.as_str()) {
        (_, "") => None,
        ("off", id) => Some(CompatAction::EntryOff { id: id.to_owned() }),
        ("on", id) => Some(CompatAction::EntryOn { id: id.to_owned() }),
        ("pack", "off") => Some(CompatAction::PackOff(true)),
        ("pack", "on") => Some(CompatAction::PackOff(false)),
        _ => None,
    }
}

fn action_link(key: &str, value: &str) -> String {
    format!("{COMPAT_URL}?{}", encode_form_urlencoded(&[FormEntry::text(key, value)]))
}

/// Откуда взят показанный пакет.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackSource {
    /// Базовая копия, вшитая в сборку.
    Base,
}

/// Всё, что нужно странице: пакет и выбор пользователя.
#[derive(Debug, Clone)]
pub struct CompatView<'a> {
    /// Показываемый пакет.
    pub pack: &'a CompatPack,
    /// Откуда он.
    pub source: PackSource,
    /// Выбор пользователя (общий и по записям).
    pub policy: &'a CompatPolicy,
    /// Отладочная сборка: пакет выключен независимо от выбора.
    pub debug_build: bool,
    /// Журнал изменений, старые сверху: `(unix-секунды, текст)`.
    pub journal: &'a [(i64, String)],
    /// Сейчас, unix-секунды (для `review_by`).
    pub now_unix: i64,
}

/// Дата `YYYY-MM-DD` по unix-секундам (UTC), алгоритм civil-from-days.
#[must_use]
pub fn ymd(unix: i64) -> String {
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn applies(view: &CompatView<'_>, id: &str) -> bool {
    !view.debug_build && view.policy.is_entry_on(id)
}

fn entry_row(e: &CompatEntry, view: &CompatView<'_>, today: &str) -> String {
    let on = applies(view, &e.id);
    let hosts = e.matches.iter().map(|h| escape_html(h)).collect::<Vec<_>>().join(", ");
    let review = if e.review_by.as_str() < today {
        format!("<span class=\"warn\">пересмотр до {} — просрочен</span>", escape_html(&e.review_by))
    } else {
        format!("пересмотр до {}", escape_html(&e.review_by))
    };
    let button = if view.policy.is_entry_off(&e.id) {
        format!("<a class=\"btn primary\" href=\"{}\">Включить</a>", escape_html(&action_link("on", &e.id)))
    } else {
        format!("<a class=\"btn\" href=\"{}\">Выключить</a>", escape_html(&action_link("off", &e.id)))
    };
    let state = if on { "применяется" } else { "не применяется" };
    let class = if on { "row" } else { "row off" };
    format!(
        "<div class=\"{class}\"><div class=\"info\"><div class=\"site\">{hosts}</div>\
<div class=\"meta\">{kind} · {summary}</div>\
<div class=\"meta\">причина: {cause} · {bug} · {review} · {state}</div></div>\
<div class=\"actions\">{button}</div></div>",
        kind = escape_html(e.action.kind()),
        summary = escape_html(&e.action.summary()),
        cause = escape_html(&e.root_cause),
        bug = escape_html(&e.bug),
    )
}

fn journal_html(journal: &[(i64, String)]) -> String {
    if journal.is_empty() {
        return "<p class=\"empty\">Изменений пока нет.</p>".to_owned();
    }
    let mut out = String::from("<ul class=\"journal\">");
    for (at, text) in journal.iter().rev() {
        out.push_str(&format!("<li>{} — {}</li>", ymd(*at), escape_html(text)));
    }
    out.push_str("</ul>");
    out
}

/// HTML страницы. Пустой пакет — пояснение, зачем страница нужна.
#[must_use]
pub fn build_compat_html(view: &CompatView<'_>) -> String {
    let source = match view.source {
        PackSource::Base => "базовая копия сборки",
    };
    let signed = if view.pack.key_id.is_empty() {
        String::new()
    } else {
        format!(", ключ подписи {}", escape_html(&view.pack.key_id))
    };
    let pack_button = if view.policy.pack_off {
        format!(
            "<a class=\"btn primary\" href=\"{}\">Использовать список Lumen</a>",
            escape_html(&action_link("pack", "on"))
        )
    } else {
        format!(
            "<a class=\"btn\" href=\"{}\">Не использовать список Lumen</a>",
            escape_html(&action_link("pack", "off"))
        )
    };
    let status = if view.debug_build {
        "<div class=\"hint\">Отладочная сборка: пакет выключен, записи не применяются.</div>"
    } else if view.policy.pack_off {
        "<div class=\"hint\">Список Lumen не используется: ни одна запись не применяется ни на одном сайте.</div>"
    } else {
        ""
    };
    let today = ymd(view.now_unix);
    let mut rows = String::new();
    if view.pack.entries.is_empty() {
        rows.push_str(
            "<p class=\"empty\">Записей нет. Здесь появляется каждое исправление совместимости, \
которое Lumen применяет к сайту: причина, ссылка на ошибку и срок пересмотра. \
Каждое можно выключить.</p>",
        );
    }
    for e in &view.pack.entries {
        rows.push_str(&entry_row(e, view, &today));
    }
    let active = view.pack.entries.iter().filter(|e| applies(view, &e.id)).count();
    format!(
        "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"utf-8\">\
<title>Пакет совместимости</title><style>\
body{{margin:0;padding:48px 40px;background:#1b1b1f;color:#e8e8ea;font-family:'Inter',sans-serif}}\
h1{{font-size:26px;font-weight:600;margin:0 0 8px 0}}\
h2{{font-size:18px;font-weight:600;margin:32px 0 12px 0}}\
.lead{{color:#9a9aa2;font-size:14px;max-width:720px;margin:0 0 20px 0}}\
.pack{{display:flex;align-items:center;justify-content:space-between;max-width:760px;\
padding:14px 18px;margin-bottom:20px;background:#27272c;border-radius:8px}}\
.row{{display:flex;align-items:center;justify-content:space-between;max-width:760px;\
padding:14px 18px;margin-bottom:10px;background:#27272c;border-radius:8px}}\
.row.off{{opacity:.6}}\
.site{{font-size:17px;font-weight:600}}\
.meta{{color:#9a9aa2;font-size:13px;margin-top:4px}}\
.hint{{color:#e8c46a;font-size:13px;margin:0 0 16px 0}}\
.warn{{color:#e8c46a}}\
.actions{{display:flex;gap:8px}}\
.btn{{display:inline-block;padding:7px 14px;border-radius:6px;background:#3a3a40;\
color:#e8e8ea;text-decoration:none;font-size:13px}}\
.btn.primary{{background:#2b6cdf;color:#fff}}\
.empty{{color:#9a9aa2;font-size:15px;max-width:620px}}\
.journal{{color:#9a9aa2;font-size:13px;max-width:760px;padding-left:18px}}\
</style></head><body><h1>Пакет совместимости</h1>\
<p class=\"lead\">Подписанный список исправлений для отдельных сайтов. Это данные, а не код: \
записи берутся из закрытого набора действий и не шире домена сайта. Каждая запись видна здесь \
и выключается отдельно.</p>\
{status}\
<div class=\"pack\"><div class=\"info\"><div class=\"site\">Версия пакета {revision}</div>\
<div class=\"meta\">{source}, формат {format}{signed} · записей: {total}, применяется: {active}</div></div>\
<div class=\"actions\">{pack_button}</div></div>\
{rows}<h2>Журнал</h2>{journal}</body></html>",
        revision = view.pack.revision,
        format = view.pack.format,
        total = view.pack.entries.len(),
        journal = journal_html(view.journal),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_update_manifest::compat_pack::{Action, SiteClass, PACK_FORMAT};

    fn entry(id: &str, review_by: &str) -> CompatEntry {
        CompatEntry {
            id: id.to_owned(),
            root_cause: "engine-css".to_owned(),
            matches: vec!["example.com".to_owned()],
            site_class: SiteClass::General,
            action: Action::KnownIssue { message: "нет <звука>".to_owned() },
            bug: "BUG-7".to_owned(),
            review_by: review_by.to_owned(),
            tests: Vec::new(),
            test_exemption: Some("только плашка".to_owned()),
            engine_max: Some("9.9.9".to_owned()),
        }
    }

    fn pack(entries: Vec<CompatEntry>) -> CompatPack {
        CompatPack { format: PACK_FORMAT, revision: 3, entries, key_id: String::new(), signature: String::new() }
    }

    fn view<'a>(pack: &'a CompatPack, policy: &'a CompatPolicy, journal: &'a [(i64, String)]) -> CompatView<'a> {
        CompatView { pack, source: PackSource::Base, policy, debug_build: false, journal, now_unix: 1_790_000_000 }
    }

    #[test]
    fn links_round_trip() {
        for id in ["a-css", "b_flag"] {
            assert_eq!(parse_action(&action_link("off", id)), Some(CompatAction::EntryOff { id: id.to_owned() }));
            assert_eq!(parse_action(&action_link("on", id)), Some(CompatAction::EntryOn { id: id.to_owned() }));
        }
        assert_eq!(parse_action(&action_link("pack", "off")), Some(CompatAction::PackOff(true)));
        assert_eq!(parse_action(&action_link("pack", "on")), Some(CompatAction::PackOff(false)));
    }

    #[test]
    fn foreign_or_broken_links_are_not_actions() {
        assert_eq!(parse_action("about:compat"), None);
        assert_eq!(parse_action("about:compat?off="), None);
        assert_eq!(parse_action("about:compat?pack=maybe"), None);
        assert_eq!(parse_action("about:compat?drop=x"), None);
        assert_eq!(parse_action("about:handoff?off=x"), None);
    }

    #[test]
    fn ymd_matches_known_dates() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(1_790_000_000), "2026-09-21");
        assert_eq!(ymd(951_782_400), "2000-02-29");
        assert_eq!(ymd(-86_400), "1969-12-31");
    }

    #[test]
    fn rows_show_cause_bug_review_and_switch() {
        let p = pack(vec![entry("a-note", "2027-01-31"), entry("b-old", "2026-01-01")]);
        let policy = CompatPolicy { pack_off: false, disabled: vec!["b-old".to_owned()] };
        let html = build_compat_html(&view(&p, &policy, &[]));
        assert!(html.contains("Версия пакета 3"));
        assert!(html.contains("BUG-7") && html.contains("engine-css") && html.contains("example.com"));
        assert!(html.contains("нет &lt;звука&gt;"), "текст записи экранируется");
        assert!(html.contains("href=\"about:compat?off=a-note\""));
        assert!(html.contains("href=\"about:compat?on=b-old\""));
        assert!(html.contains("пересмотр до 2026-01-01 — просрочен"));
        assert!(html.contains("записей: 2, применяется: 1"));
    }

    #[test]
    fn pack_off_and_debug_build_say_nothing_applies() {
        let p = pack(vec![entry("a-note", "2027-01-31")]);
        let off = CompatPolicy { pack_off: true, disabled: Vec::new() };
        let html = build_compat_html(&view(&p, &off, &[]));
        assert!(html.contains("href=\"about:compat?pack=on\"") && html.contains("применяется: 0"));
        let on = CompatPolicy::default();
        let mut v = view(&p, &on, &[]);
        v.debug_build = true;
        let html = build_compat_html(&v);
        assert!(html.contains("Отладочная сборка") && html.contains("применяется: 0"));
    }

    #[test]
    fn empty_pack_and_journal_explain_themselves() {
        let p = pack(Vec::new());
        let policy = CompatPolicy::default();
        let journal = vec![(0, "запись a<b выключена".to_owned()), (86_400, "второе".to_owned())];
        let html = build_compat_html(&view(&p, &policy, &[]));
        assert!(html.contains("Записей нет") && html.contains("Изменений пока нет"));
        let html = build_compat_html(&view(&p, &policy, &journal));
        assert!(html.contains("1970-01-02 — второе"));
        assert!(html.contains("a&lt;b"));
        assert!(html.find("второе") < html.find("a&lt;b"), "новые сверху");
    }
}

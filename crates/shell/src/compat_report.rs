//! Отчёт о поломке сайта (COMPAT-REPORT, `docs/research/site-compat/report.md` §8.6,
//! COMPAT-REP-01…07): причины, очистка текста и адреса, сбор отчёта и локальный
//! учёт «по этому домену сегодня отчёт уже был».
//!
//! Здесь только чистые правила — ни окна, ни сети, ни диска. Страницы отчёта —
//! `crate::report_page`, сбор данных окна и экспорт — `crate::lumen::report`.
//! Отчёт — разовое действие пользователя: он не читает настроек телеметрии, не
//! несёт стабильного идентификатора клиента и не попадает в контекст страницы.

use std::fmt::Write as _;
use std::sync::OnceLock;

use lumen_update_manifest::compat_pack::{Action, CompatEntry};
use regex::{Captures, Regex};
use serde_json::{json, Value};

/// Версия формата отчёта (поле `format`).
pub const REPORT_FORMAT: u32 = 1;

/// Сколько символов свободного текста сохраняется.
pub const NOTE_MAX_CHARS: usize = 2000;

/// Сколько записей каждого счётчика (ошибки JS, коды HTTP, …) уходит в отчёт.
pub const COUNTED_MAX: usize = 20;

// ── Причины (REP-03) ─────────────────────────────────────────────────────────

/// Причина, по которой пользователь сообщает о поломке.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Страница не загружается или остаётся пустой.
    Blank,
    /// Элементы расположены неправильно.
    Layout,
    /// Не хватает картинок или содержимого.
    Missing,
    /// Кнопки, меню или формы не работают.
    Controls,
    /// Видео или звук не воспроизводятся.
    Media,
    /// Не получается войти в аккаунт.
    Login,
    /// Страница очень медленная или зависает.
    Slow,
    /// Текст или шрифты отображаются неправильно.
    Text,
    /// Не работают загрузка, скачивание или печать.
    Transfer,
    /// Сайт пишет, что браузер не поддерживается.
    Unsupported,
    /// Просит отключить VPN.
    Vpn,
    /// Требует сертификат Минцифры, ГОСТ или плагин подписи.
    Gost,
    /// Просит установить другой браузер.
    OtherBrowser,
    /// Бесконечная проверка или капча.
    Captcha,
    /// Не проходит оплата, 3-D Secure, СБП.
    Payment,
    /// Другое — всегда последняя.
    Other,
}

impl Reason {
    /// Все причины, `Other` последняя.
    pub const ALL: [Reason; 16] = [
        Self::Blank,
        Self::Layout,
        Self::Missing,
        Self::Controls,
        Self::Media,
        Self::Login,
        Self::Slow,
        Self::Text,
        Self::Transfer,
        Self::Unsupported,
        Self::Vpn,
        Self::Gost,
        Self::OtherBrowser,
        Self::Captcha,
        Self::Payment,
        Self::Other,
    ];

    /// Стабильный код (значение радиокнопки и поле `reason` отчёта).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Blank => "blank",
            Self::Layout => "layout",
            Self::Missing => "missing",
            Self::Controls => "controls",
            Self::Media => "media",
            Self::Login => "login",
            Self::Slow => "slow",
            Self::Text => "text",
            Self::Transfer => "transfer",
            Self::Unsupported => "unsupported",
            Self::Vpn => "vpn",
            Self::Gost => "gost",
            Self::OtherBrowser => "other-browser",
            Self::Captcha => "captcha",
            Self::Payment => "payment",
            Self::Other => "other",
        }
    }

    /// Причина по коду из [`Reason::code`].
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }

    /// Подпись в форме.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Blank => "Страница не загружается или остаётся пустой",
            Self::Layout => "Элементы страницы расположены неправильно",
            Self::Missing => "Не хватает картинок или содержимого",
            Self::Controls => "Кнопки, меню или формы не работают",
            Self::Media => "Видео или звук не воспроизводятся",
            Self::Login => "Не получается войти в аккаунт",
            Self::Slow => "Страница очень медленная или зависает",
            Self::Text => "Текст или шрифты отображаются неправильно",
            Self::Transfer => "Не работает загрузка, скачивание или печать",
            Self::Unsupported => "Сайт пишет, что браузер не поддерживается",
            Self::Vpn => "Просит отключить VPN",
            Self::Gost => "Требует сертификат Минцифры, ГОСТ или плагин подписи",
            Self::OtherBrowser => "Просит установить другой браузер",
            Self::Captcha => "Бесконечная проверка или капча",
            Self::Payment => "Не проходит оплата, 3-D Secure, СБП",
            Self::Other => "Другое",
        }
    }
}

/// Причины в порядке для формы: перемешаны по `seed`, «Другое» остаётся последней.
#[must_use]
pub fn shuffled_reasons(seed: u64) -> Vec<Reason> {
    let mut reasons: Vec<Reason> = Reason::ALL.into_iter().filter(|r| *r != Reason::Other).collect();
    // xorshift64*: достаточно для порядка в форме, зависимостей не нужно.
    let mut state = seed | 1;
    for i in (1..reasons.len()).rev() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let j = (r >> 33) as usize % (i + 1);
        reasons.swap(i, j);
    }
    reasons.push(Reason::Other);
    reasons
}

// ── Очистка (REP-04) ─────────────────────────────────────────────────────────

fn cached<'a>(cell: &'a OnceLock<Regex>, pattern: &str) -> &'a Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("шаблон очистки компилируется"))
}

fn digit_count(s: &str) -> usize {
    s.chars().filter(char::is_ascii_digit).count()
}

/// Вычистить из `text` адреса e-mail, значения паролей и токенов, номера карт и
/// телефоны. Лишнее лучше вычистить, чем оставить: отчёт читает человек.
#[must_use]
pub fn scrub(text: &str) -> String {
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    static SECRET: OnceLock<Regex> = OnceLock::new();
    static BEARER: OnceLock<Regex> = OnceLock::new();
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    static CARD: OnceLock<Regex> = OnceLock::new();
    static PHONE: OnceLock<Regex> = OnceLock::new();

    let s = cached(&EMAIL, r"[\w.+\-]+@[\w\-]+(?:\.[\w\-]+)+").replace_all(text, "[e-mail]");
    let s = cached(
        &SECRET,
        r"(?i)\b(token|secret|password|passwd|pwd|api[_-]?key|access[_-]?token|auth|session|sid|пароль|токен|код)\s*[:=]\s*\S+",
    )
    .replace_all(&s, "$1=[скрыто]");
    let s = cached(&BEARER, r"(?i)\bbearer\s+\S+").replace_all(&s, "bearer [скрыто]");
    let s = cached(&TOKEN, r"[A-Za-z0-9_\-+=]{24,}").replace_all(&s, |c: &Captures<'_>| {
        let word = &c[0];
        let has_digit = word.bytes().any(|b| b.is_ascii_digit());
        let has_letter = word.bytes().any(|b| b.is_ascii_alphabetic());
        if (has_digit && has_letter) || word.len() >= 40 {
            "[токен]".to_owned()
        } else {
            word.to_owned()
        }
    });
    let s = cached(&CARD, r"(?:\d[ \-]?){12,18}\d").replace_all(&s, |c: &Captures<'_>| {
        if (13..=19).contains(&digit_count(&c[0])) {
            "[карта]".to_owned()
        } else {
            c[0].to_owned()
        }
    });
    let s = cached(&PHONE, r"\+?\d[\d\s().\-]{8,}\d").replace_all(&s, |c: &Captures<'_>| {
        if digit_count(&c[0]) >= 10 {
            "[телефон]".to_owned()
        } else {
            c[0].to_owned()
        }
    });
    s.into_owned()
}

/// Свободный текст пользователя: очищен и обрезан до [`NOTE_MAX_CHARS`].
#[must_use]
pub fn clean_note(note: &str) -> String {
    let trimmed: String = note.trim().chars().take(NOTE_MAX_CHARS).collect();
    scrub(&trimmed)
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["token", "secret", "pass", "pwd", "key", "auth", "sid", "session", "code", "sig", "mail", "phone", "card"]
        .iter()
        .any(|needle| key.contains(needle))
}

/// Адрес для отчёта. По умолчанию `scheme://host[:port]/path` без `userinfo`,
/// query и fragment; с `full` — и они, но значения секретных ключей скрыты, а
/// остальное прошло [`scrub`]. Не http(s)-адрес в отчёт не попадает.
#[must_use]
pub fn report_url(url: &str, full: bool) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host = authority.rsplit('@').next().unwrap_or(authority).to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let (before_fragment, fragment) = match tail.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (tail, None),
    };
    let (path, query) = match before_fragment.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (before_fragment, None),
    };
    let path = if path.is_empty() { "/" } else { path };
    let mut out = format!("{scheme}://{host}{}", scrub(path));
    if full {
        if let Some(query) = query {
            let pairs: Vec<String> = query
                .split('&')
                .map(|pair| match pair.split_once('=') {
                    Some((k, v)) if is_secret_key(k) || scrub(v) != v => format!("{k}=[скрыто]"),
                    Some((k, v)) => format!("{k}={v}"),
                    None => scrub(pair),
                })
                .collect();
            let _ = write!(out, "?{}", pairs.join("&"));
        }
        if let Some(fragment) = fragment {
            let _ = write!(out, "#{}", scrub(fragment));
        }
    }
    Some(out)
}

// ── Реестр известных проблем (REP-02) ────────────────────────────────────────

/// Текст «Мы знаем об этой проблеме…» по записям пакета для сайта; `None` — сайта
/// в реестре нет. Пакет здесь — справка, поэтому выбор пользователя («не использовать
/// список Lumen», выключенные записи) на него не влияет.
#[must_use]
pub fn known_issue_text(entries: &[&CompatEntry]) -> Option<String> {
    let parts: Vec<String> = entries
        .iter()
        .map(|e| match &e.action {
            Action::KnownIssue { message } => format!("Мы знаем об этой проблеме: {message} ({}).", e.bug),
            Action::HandoffHint { reason } => {
                format!("Мы знаем об этой проблеме. Обход: открыть сайт в другом браузере — {reason} ({}).", e.bug)
            }
            other => format!(
                "Мы знаем об этой проблеме: для сайта уже есть правка в пакете совместимости — {} ({}). Если она не помогла, опишите, что не так.",
                other.summary(),
                e.bug
            ),
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

// ── Автоматические поля (REP-05) ─────────────────────────────────────────────

/// Имя со счётчиком: тип JS-ошибки, имя API, код ответа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    /// Имя или код.
    pub name: String,
    /// Сколько раз встретилось.
    pub count: u32,
}

/// Правка пакета, применённая к странице.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedInfo {
    /// Id записи пакета.
    pub id: String,
    /// Класс первопричины.
    pub root_cause: String,
    /// `BUG-NNN` или `KB-NNN`.
    pub bug: String,
}

/// Что окно знает о странице без участия пользователя. Здесь нет IP, cookie,
/// содержимого страницы и идентификатора клиента.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoFields {
    /// Версия Lumen.
    pub lumen_version: String,
    /// Ревизия пакета совместимости.
    pub pack_revision: u32,
    /// Профиль идентичности (`system`, `en-US+UTC`, …).
    pub identity: String,
    /// Тип профиля: `personal`, `work`, `anonymous`, `guest`, …
    pub profile_kind: String,
    /// Защиты (блокировщик) включены на этом сайте.
    pub protection_on: bool,
    /// Сколько запросов защиты заблокировали на странице.
    pub blocked_count: u32,
    /// Применённые правки пакета.
    pub applied: Vec<AppliedInfo>,
    /// Шаги панели, после которых пользователь ответил «Заработало» (`fixed_by`).
    pub fixed_by: Vec<String>,
    /// Имена нереализованных API со счётчиками; пока источника нет — пусто.
    pub unimplemented_apis: Vec<Counted>,
    /// Типы исключений JS со счётчиками.
    pub js_errors: Vec<Counted>,
    /// Коды ответов и сетевые отказы упавших запросов со счётчиками.
    pub http_errors: Vec<Counted>,
    /// Издатель TLS-цепочки; `None` — не известен.
    pub tls_issuer: Option<String>,
    /// Продукт-перехватчик TLS; `None` — не обнаружен или не известен.
    pub tls_interceptor: Option<String>,
    /// Признак прокси или VPN; `None` — не известно.
    pub proxy_or_vpn: Option<bool>,
    /// Код поставщика антибот-стены, если она срабатывала (без IP).
    pub antibot_vendor: Option<String>,
    /// Категория сайта: `bank`, `gov`, `payment`, `call`; `None` — обычный.
    pub category: Option<String>,
}

/// Категория сайта по адресу хоста — для группировки отчётов у приёмника.
#[must_use]
pub fn site_category(host: &str) -> Option<&'static str> {
    let host = host.to_ascii_lowercase();
    let has = |names: &[&str]| names.iter().any(|n| host == *n || host.ends_with(&format!(".{n}")));
    if has(&["gosuslugi.ru", "nalog.ru", "mos.ru", "pfr.gov.ru", "gov.ru"]) {
        Some("gov")
    } else if has(&["sberbank.ru", "sber.ru", "tinkoff.ru", "tbank.ru", "vtb.ru", "alfabank.ru", "gazprombank.ru", "raiffeisen.ru"]) {
        Some("bank")
    } else if has(&["yoomoney.ru", "cloudpayments.ru", "nspk.ru", "qiwi.com"]) {
        Some("payment")
    } else if has(&["telemost.yandex.ru", "meet.google.com", "zoom.us", "whereby.com"]) {
        Some("call")
    } else {
        None
    }
}

/// Сложить повторяющиеся имена в счётчики, самые частые первыми.
#[must_use]
pub fn count_names<I: IntoIterator<Item = String>>(names: I) -> Vec<Counted> {
    let mut out: Vec<Counted> = Vec::new();
    for name in names {
        match out.iter_mut().find(|c| c.name == name) {
            Some(c) => c.count += 1,
            None => out.push(Counted { name, count: 1 }),
        }
    }
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    out.truncate(COUNTED_MAX);
    out
}

/// Тип исключения JS из текста консоли (`Uncaught TypeError: …` → `TypeError`).
#[must_use]
pub fn js_error_type(message: &str) -> Option<String> {
    static TYPE: OnceLock<Regex> = OnceLock::new();
    let re = cached(&TYPE, r"\b([A-Z][A-Za-z]*(?:Error|Exception))\b");
    re.captures(message).map(|c| c[1].to_owned())
}

fn counted_json(items: &[Counted]) -> Value {
    Value::Array(items.iter().map(|c| json!({ "name": c.name, "count": c.count })).collect())
}

// ── Сборка отчёта ────────────────────────────────────────────────────────────

/// Выбор пользователя в форме.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    /// Причина; `None` — не выбрана.
    pub reason: Option<Reason>,
    /// Свободный текст (до очистки).
    pub note: String,
    /// Тумблер «полный адрес».
    pub full_url: bool,
    /// Тумблер «список заблокированного».
    pub blocked_list: bool,
}

/// Собрать отчёт. `blocked_hosts` попадает в него только с тумблером; `day` —
/// дата без времени (`YYYY-MM-DD`). Ни одно поле не стабильно между отчётами
/// клиента: идентификатора нет.
#[must_use]
pub fn build_report(url: &str, draft: &Draft, auto: &AutoFields, blocked_hosts: &[String], day: &str) -> Value {
    let reason = draft.reason.unwrap_or(Reason::Other);
    let blocked: Vec<String> = if draft.blocked_list {
        blocked_hosts.iter().map(|h| scrub(h)).take(COUNTED_MAX * 5).collect()
    } else {
        Vec::new()
    };
    json!({
        "format": REPORT_FORMAT,
        "date": day,
        "url": report_url(url, draft.full_url),
        "reason": { "code": reason.code(), "label": reason.label() },
        "note": clean_note(&draft.note),
        "lumen": {
            "version": auto.lumen_version,
            "pack_revision": auto.pack_revision,
            "identity": auto.identity,
            "profile": auto.profile_kind,
        },
        "protection": { "on": auto.protection_on, "blocked_count": auto.blocked_count },
        "blocked_hosts": blocked,
        "applied_fixes": auto.applied.iter()
            .map(|a| json!({ "id": a.id, "root_cause": a.root_cause, "bug": a.bug }))
            .collect::<Vec<_>>(),
        "fixed_by": auto.fixed_by,
        "unimplemented_apis": counted_json(&auto.unimplemented_apis),
        "js_errors": counted_json(&auto.js_errors),
        "http_errors": counted_json(&auto.http_errors),
        "tls": { "issuer": auto.tls_issuer, "interceptor": auto.tls_interceptor },
        "proxy_or_vpn": auto.proxy_or_vpn,
        "antibot_vendor": auto.antibot_vendor,
        "category": auto.category,
    })
}

/// Текст отчёта, как он показан в предпросмотре и уходит в файл или буфер.
#[must_use]
pub fn report_text(report: &Value) -> String {
    serde_json::to_string_pretty(report).unwrap_or_default()
}

/// Имя файла экспорта: `lumen-report-<host>-<YYYYMMDD>.json`.
#[must_use]
pub fn export_file_name(site: &str, day: &str) -> String {
    let site: String = site
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    format!("lumen-report-{site}-{}.json", day.replace('-', ""))
}

// ── Учёт отправленного (REP-06, REP-07) ──────────────────────────────────────

/// Один отправленный отчёт: домен, день, ревизия пакета на тот момент.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportedSite {
    /// Сайт (eTLD+1).
    pub site: String,
    /// День: unix-секунды / 86400.
    pub day: i64,
    /// Ревизия пакета, когда сообщили.
    pub pack_revision: u32,
    /// Пользователю уже сказали, что сайт заработал.
    pub notified: bool,
}

/// Сайты, о которых сообщал пользователь. Лежит локально; без идентификатора.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportedSites {
    items: Vec<ReportedSite>,
}

/// Сколько записей о сообщённых сайтах хранится.
const REPORTED_CAP: usize = 200;

impl ReportedSites {
    /// Разобрать сохранённый текст (по строке `site\tday\trevision\tnotified`).
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let items = text
            .lines()
            .filter_map(|line| {
                let mut f = line.split('\t');
                Some(ReportedSite {
                    site: f.next().filter(|s| !s.is_empty())?.to_owned(),
                    day: f.next()?.parse().ok()?,
                    pack_revision: f.next()?.parse().ok()?,
                    notified: f.next()? == "1",
                })
            })
            .collect();
        Self { items }
    }

    /// Текст для сохранения.
    #[must_use]
    pub fn to_text(&self) -> String {
        self.items
            .iter()
            .map(|r| format!("{}\t{}\t{}\t{}", r.site, r.day, r.pack_revision, u8::from(r.notified)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Сегодня (`day`) по этому сайту отчёт уже был.
    #[must_use]
    pub fn reported_on(&self, site: &str, day: i64) -> bool {
        self.items.iter().any(|r| r.site == site && r.day == day)
    }

    /// Запомнить отправку: запись по сайту заменяется (последняя побеждает).
    pub fn record(&mut self, site: &str, day: i64, pack_revision: u32) {
        self.items.retain(|r| r.site != site);
        self.items.push(ReportedSite { site: site.to_owned(), day, pack_revision, notified: false });
        let skip = self.items.len().saturating_sub(REPORTED_CAP);
        self.items.drain(..skip);
    }

    /// Сайты, о которых сообщали на более старой ревизии пакета и для которых
    /// теперь есть правка (`has_fix`). Каждый возвращается один раз.
    pub fn take_newly_fixed(&mut self, current_revision: u32, has_fix: impl Fn(&str) -> bool) -> Vec<String> {
        let mut fixed = Vec::new();
        for r in &mut self.items {
            if !r.notified && r.pack_revision < current_revision && has_fix(&r.site) {
                r.notified = true;
                fixed.push(r.site.clone());
            }
        }
        fixed
    }

    /// Сколько сайтов запомнено.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Ничего не запомнено.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_lists_all_required_reasons_with_other_last() {
        let order = shuffled_reasons(7);
        assert_eq!(order.len(), Reason::ALL.len());
        assert_eq!(order.last(), Some(&Reason::Other));
        for needed in [
            "Просит отключить VPN",
            "Требует сертификат Минцифры, ГОСТ или плагин подписи",
            "Просит установить другой браузер",
            "Бесконечная проверка или капча",
            "Не проходит оплата, 3-D Secure, СБП",
        ] {
            assert!(order.iter().any(|r| r.label() == needed), "нет причины «{needed}»");
        }
        // Десять причин Firefox плюс пять российских плюс «Другое».
        assert_eq!(Reason::ALL.len(), 10 + 5 + 1);
    }

    #[test]
    fn first_reason_changes_over_twenty_openings() {
        let firsts: std::collections::HashSet<_> =
            (1..=20_u64).map(|seed| shuffled_reasons(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15))[0].code()).collect();
        assert!(firsts.len() > 1, "первая причина не меняется: {firsts:?}");
        for seed in 0..20 {
            assert_eq!(shuffled_reasons(seed).last(), Some(&Reason::Other), "seed {seed}");
        }
    }

    #[test]
    fn reason_codes_round_trip() {
        for r in Reason::ALL {
            assert_eq!(Reason::from_code(r.code()), Some(r));
        }
        assert_eq!(Reason::from_code("nope"), None);
    }

    #[test]
    fn default_url_drops_userinfo_query_and_fragment() {
        let url = "https://user:pw@Example.com:8443/reset/a?token=abc&x=1#frag";
        assert_eq!(report_url(url, false).as_deref(), Some("https://example.com:8443/reset/a"));
        assert_eq!(report_url("https://example.com", false).as_deref(), Some("https://example.com/"));
        assert_eq!(report_url("about:blank", false), None);
        assert_eq!(report_url("file:///C:/x", false), None);
    }

    #[test]
    fn url_with_token_and_email_is_cleaned_even_when_full() {
        let url = "https://shop.ru/confirm/ivan@mail.ru/aGVsbG8gd29ybGQgdG9rZW4xMjM0NTY3?token=abc&email=a@b.ru&page=2#top";
        let short = report_url(url, false).expect("http(s)");
        assert_eq!(short, "https://shop.ru/confirm/[e-mail]/[токен]");
        let full = report_url(url, true).expect("http(s)");
        assert!(!full.contains("ivan@mail.ru") && !full.contains("a@b.ru"), "{full}");
        assert!(!full.contains("abc&") && full.contains("token=[скрыто]"), "{full}");
        assert!(full.contains("page=2") && full.ends_with("#top"), "{full}");
    }

    #[test]
    fn scrub_removes_emails_phones_cards_and_tokens() {
        let text = "пишу на ivan.petrov+x@mail.ru, тел +7 (916) 123-45-67 или 89161234567, \
карта 4111 1111 1111 1111, пароль: hunter2, Bearer abc.def, \
ключ eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 и всё";
        let out = scrub(text);
        for leaked in ["ivan.petrov", "123-45-67", "89161234567", "4111", "hunter2", "abc.def", "eyJhbGci"] {
            assert!(!out.contains(leaked), "«{leaked}» остался: {out}");
        }
        for marker in ["[e-mail]", "[телефон]", "[карта]", "[токен]"] {
            assert!(out.contains(marker), "нет {marker}: {out}");
        }
        assert!(out.contains("пишу на"));
    }

    #[test]
    fn scrub_keeps_ordinary_text_and_short_numbers() {
        let text = "кнопка «Оплатить» не нажимается, ошибка 403 на странице 12 в 2026 году";
        assert_eq!(scrub(text), text);
    }

    #[test]
    fn note_is_trimmed_and_capped() {
        let long = "а".repeat(NOTE_MAX_CHARS + 500);
        assert_eq!(clean_note(&long).chars().count(), NOTE_MAX_CHARS);
        assert_eq!(clean_note("  тест  "), "тест");
    }

    fn sample_auto() -> AutoFields {
        AutoFields {
            lumen_version: "1.2.3".into(),
            pack_revision: 4,
            identity: "system".into(),
            profile_kind: "personal".into(),
            protection_on: true,
            blocked_count: 7,
            applied: vec![AppliedInfo { id: "fix-1".into(), root_cause: "engine-css".into(), bug: "BUG-1".into() }],
            fixed_by: vec!["no_cache".into()],
            unimplemented_apis: vec![Counted { name: "navigator.hid".into(), count: 2 }],
            js_errors: vec![Counted { name: "TypeError".into(), count: 3 }],
            http_errors: vec![Counted { name: "403".into(), count: 1 }],
            tls_issuer: Some("Russian Trusted Root CA".into()),
            tls_interceptor: None,
            proxy_or_vpn: Some(false),
            antibot_vendor: Some("ddos-guard".into()),
            category: Some("bank".into()),
        }
    }

    #[test]
    fn report_carries_every_auto_field_and_no_identifiers() {
        let draft = Draft { reason: Some(Reason::Vpn), note: "не пускает".into(), full_url: false, blocked_list: false };
        let report = build_report("https://bank.ru/login?sid=1", &draft, &sample_auto(), &[], "2026-10-16");
        for key in [
            "format", "date", "url", "reason", "note", "lumen", "protection", "blocked_hosts", "applied_fixes",
            "fixed_by", "unimplemented_apis", "js_errors", "http_errors", "tls", "proxy_or_vpn", "antibot_vendor",
            "category",
        ] {
            assert!(report.get(key).is_some(), "в отчёте нет поля {key}");
        }
        assert_eq!(report["url"], "https://bank.ru/login");
        assert_eq!(report["lumen"]["pack_revision"], 4);
        assert_eq!(report["applied_fixes"][0]["id"], "fix-1");
        assert_eq!(report["tls"]["issuer"], "Russian Trusted Root CA");
        let text = report_text(&report).to_ascii_lowercase();
        for forbidden in ["cookie", "client_id", "device_id", "ip_address", "\"ip\""] {
            assert!(!text.contains(forbidden), "в отчёте есть {forbidden}");
        }
    }

    #[test]
    fn blocked_list_is_sent_only_with_the_toggle() {
        let hosts = vec!["ads.example.net".to_owned()];
        let mut draft = Draft::default();
        let off = build_report("https://a.ru/", &draft, &AutoFields::default(), &hosts, "2026-10-16");
        assert_eq!(off["blocked_hosts"].as_array().map(Vec::len), Some(0));
        draft.blocked_list = true;
        let on = build_report("https://a.ru/", &draft, &AutoFields::default(), &hosts, "2026-10-16");
        assert_eq!(on["blocked_hosts"][0], "ads.example.net");
    }

    #[test]
    fn report_without_reason_is_other_and_note_optional() {
        let report = build_report("https://a.ru/x", &Draft::default(), &AutoFields::default(), &[], "2026-10-16");
        assert_eq!(report["reason"]["code"], "other");
        assert_eq!(report["note"], "");
    }

    #[test]
    fn counters_group_and_sort() {
        let counted = count_names(["B", "A", "B", "C", "B", "A"].map(str::to_owned));
        let pairs: Vec<_> = counted.iter().map(|c| (c.name.as_str(), c.count)).collect();
        assert_eq!(pairs, [("B", 3), ("A", 2), ("C", 1)]);
        assert_eq!(js_error_type("Uncaught TypeError: x is not a function").as_deref(), Some("TypeError"));
        assert_eq!(js_error_type("Uncaught (in promise) DOMException: nope").as_deref(), Some("DOMException"));
        assert_eq!(js_error_type("просто текст"), None);
    }

    fn entry(action: Action) -> CompatEntry {
        CompatEntry {
            id: "e".into(),
            root_cause: "gate-loop".into(),
            matches: vec!["bank.ru".into()],
            site_class: Default::default(),
            action,
            bug: "BUG-9".into(),
            review_by: "2027-01-01".into(),
            tests: Vec::new(),
            test_exemption: Some("тест".into()),
            engine_max: None,
        }
    }

    #[test]
    fn registry_domain_gets_the_known_issue_message() {
        assert_eq!(known_issue_text(&[]), None);
        let known = entry(Action::KnownIssue { message: "капча зацикливается".into() });
        let text = known_issue_text(&[&known]).expect("сайт в реестре");
        assert!(text.contains("Мы знаем об этой проблеме") && text.contains("капча зацикливается"));
        assert!(text.contains("BUG-9"));
        let hint = entry(Action::HandoffHint { reason: "drm".into() });
        let text = known_issue_text(&[&known, &hint]).expect("сайт в реестре");
        assert!(text.contains("Обход") && text.contains("капча"));
    }

    #[test]
    fn categories_cover_the_named_kinds() {
        assert_eq!(site_category("www.gosuslugi.ru"), Some("gov"));
        assert_eq!(site_category("online.sberbank.ru"), Some("bank"));
        assert_eq!(site_category("yoomoney.ru"), Some("payment"));
        assert_eq!(site_category("zoom.us"), Some("call"));
        assert_eq!(site_category("example.com"), None);
        assert_eq!(site_category("notsber.ru"), None);
    }

    #[test]
    fn same_site_same_day_is_a_duplicate_next_day_is_not() {
        let mut log = ReportedSites::default();
        assert!(!log.reported_on("bank.ru", 100));
        log.record("bank.ru", 100, 4);
        assert!(log.reported_on("bank.ru", 100));
        assert!(!log.reported_on("bank.ru", 101), "на следующий день можно снова");
        assert!(!log.reported_on("other.ru", 100));
        // Повторная запись по сайту не плодит строк.
        log.record("bank.ru", 101, 4);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn reported_sites_survive_a_text_round_trip() {
        let mut log = ReportedSites::default();
        log.record("a.ru", 10, 1);
        log.record("b.ru", 11, 2);
        let back = ReportedSites::parse(&log.to_text());
        assert_eq!(back, log);
        assert!(ReportedSites::parse("мусор\n\t\n").is_empty());
    }

    #[test]
    fn newly_fixed_site_is_announced_once() {
        let mut log = ReportedSites::default();
        log.record("fixed.ru", 10, 1);
        log.record("still.ru", 10, 1);
        log.record("same-rev.ru", 10, 2);
        let has_fix = |site: &str| site == "fixed.ru" || site == "same-rev.ru";
        assert_eq!(log.take_newly_fixed(2, has_fix), ["fixed.ru"]);
        assert!(log.take_newly_fixed(2, has_fix).is_empty(), "второй раз не сообщаем");
        assert_eq!(log.take_newly_fixed(3, has_fix), ["same-rev.ru"]);
    }

    #[test]
    fn export_name_is_filesystem_safe() {
        assert_eq!(export_file_name("exa mple.ru", "2026-10-16"), "lumen-report-exa_mple.ru-20261016.json");
    }
}

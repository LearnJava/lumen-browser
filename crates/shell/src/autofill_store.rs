//! Хранилище автозаполнения браузера и решение «предлагать ли сохранить
//! введённое в форму» (UX-AUTOFILL, срез 2).
//!
//! База — `<data>/autofill.db` ([`lumen_storage::Autofill`]), значения лежат
//! открытым текстом: пароли и карты сюда не попадают. Платёжные поля срез не
//! сохраняет вовсе — для них нужен отдельный явный запрос (срез 4); код
//! проверки карты не сохраняется никогда. В приватных режимах хранилища нет.

use lumen_dom::Document;
use lumen_storage::Autofill;

use crate::autofill_form::{find_autofill_forms, FieldKind};
use crate::*;

/// Не больше стольких значений на поле просматриваем при поиске дубликатов.
const KNOWN_LIMIT: i64 = 50;

/// Значения формы, которые браузер предлагает запомнить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutofillOffer {
    pub origin: String,
    /// Пары `(ключ поля, значение)`; все непустые, без карт и без дубликатов.
    pub entries: Vec<(&'static str, String)>,
    /// Сколько из них в хранилище ещё не было: предложение показывается, только если > 0.
    pub new_count: usize,
}

/// Значения полей формы `form`, пригодные для сохранения.
pub fn collect_values(doc: &Document, form: NodeId) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for f in find_autofill_forms(doc).into_iter().filter(|f| f.form == Some(form)) {
        for field in f.fields {
            let kind: FieldKind = field.kind;
            if kind.is_card() || kind.is_never_stored() {
                continue;
            }
            let value = doc.control_value(field.node).trim().to_owned();
            let entry = (kind.key(), value);
            if !entry.1.is_empty() && !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
    out
}

/// Предложение для введённых значений; `None`, если нечего запоминать
/// (всё уже сохранено) или хранилище не отвечает.
pub fn plan_offer(store: &Autofill, origin: &str, values: Vec<(&'static str, String)>) -> Option<AutofillOffer> {
    let mut new_count = 0;
    for (key, value) in &values {
        let known = store.suggestions(origin, key, KNOWN_LIMIT).ok()?;
        if !known.iter().any(|e| &e.value == value) {
            new_count += 1;
        }
    }
    (new_count > 0).then(|| AutofillOffer { origin: origin.to_owned(), entries: values, new_count })
}

/// Сколько значений показывает выпадающая подсказка у поля.
pub const MENU_LIMIT: usize = 5;

/// Значения для подсказки у поля `key`: сохранённые для сайта, начинающиеся с
/// уже введённого (без учёта регистра), кроме точного совпадения. Порядок —
/// как в хранилище (частота, затем давность).
pub fn field_suggestions(store: &Autofill, origin: &str, key: &str, typed: &str) -> Vec<String> {
    let typed = typed.trim().to_lowercase();
    let Ok(known) = store.suggestions(origin, key, KNOWN_LIMIT) else { return Vec::new() };
    known
        .into_iter()
        .map(|e| e.value)
        .filter(|v| {
            let lv = v.to_lowercase();
            lv != typed && lv.starts_with(&typed)
        })
        .take(MENU_LIMIT)
        .collect()
}

/// Общее хранилище процесса; `None` в приватных режимах и если файл не открылся.
pub fn global() -> Option<&'static Autofill> {
    static STORE: std::sync::OnceLock<Option<Autofill>> = std::sync::OnceLock::new();
    STORE.get_or_init(open_default).as_ref()
}

fn open_default() -> Option<Autofill> {
    let cfg = crate::config::global();
    if cfg.no_persistent_state || cfg.http_profile == lumen_network::HttpProfile::TorBrowser {
        return None;
    }
    let dir = crate::adblock::browser_data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    match Autofill::open(dir.join("autofill.db")) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("autofill: хранилище не открылось: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "https://example.com";

    fn form_values(html: &str) -> Vec<(&'static str, String)> {
        let doc = lumen_html_parser::parse(html);
        let form = doc.find_by_id("f").expect("form");
        collect_values(&doc, form)
    }

    #[test]
    fn collects_trimmed_non_card_values_only() {
        let got = form_values(
            r#"<form id="f"><input type="email" value=" a@b.c ">
            <input name="city" value="Омск"><input name="phone" type="tel" value="">
            <input autocomplete="cc-number" value="4111"><input autocomplete="cc-csc" value="123"></form>"#,
        );
        assert_eq!(got, [("email", "a@b.c".to_owned()), ("address-level2", "Омск".to_owned())]);
    }

    #[test]
    fn field_suggestions_filter_by_typed_prefix() {
        let store = Autofill::open_in_memory().unwrap();
        for v in ["Омск", "Орёл", "Томск"] {
            store.record(SITE, "address-level2", v, 1).unwrap();
        }
        store.record(SITE, "email", "a@b.c", 1).unwrap();
        let all = field_suggestions(&store, SITE, "address-level2", "");
        assert_eq!(all.len(), 3);
        let mut o = field_suggestions(&store, SITE, "address-level2", " о");
        o.sort();
        assert_eq!(o, ["Омск", "Орёл"]);
        assert!(field_suggestions(&store, SITE, "address-level2", "омск").is_empty());
        assert!(field_suggestions(&store, "https://other.org", "address-level2", "").is_empty());
    }

    #[test]
    fn offers_only_when_something_is_new() {
        let store = Autofill::open_in_memory().unwrap();
        let values = vec![("email", "a@b.c".to_owned())];
        let offer = plan_offer(&store, SITE, values.clone()).expect("new value");
        assert_eq!(offer.new_count, 1);
        store.record(SITE, "email", "a@b.c", 1).unwrap();
        assert!(plan_offer(&store, SITE, values.clone()).is_none());
        let mixed = vec![("email", "a@b.c".to_owned()), ("tel", "123".to_owned())];
        let offer = plan_offer(&store, SITE, mixed).expect("tel is new");
        assert_eq!((offer.entries.len(), offer.new_count), (2, 1));
        assert!(plan_offer(&store, "https://other.org", values).is_some());
    }
}

//! Платёжные карты в автозаполнении (UX-AUTOFILL, срез 4).
//!
//! Карта сохраняется только по явной кнопке «Сохранить карту» и лежит не в
//! открытом `autofill.db`, а в зашифрованном `logins.db`
//! ([`lumen_storage::SavedLogins`], AES-256-GCM) под служебным origin
//! [`ORIGIN`]: имя записи — последние четыре цифры, «пароль» — номер, имя
//! владельца и срок. Код проверки (CVC) не сохраняется никогда. Подставляется
//! карта только выбором строки в списке у поля номера.

use lumen_dom::{Document, NodeId};
use lumen_storage::SavedLogins;

use crate::autofill_form::{find_autofill_forms, FieldKind};

/// Служебный origin записей карт; настоящим сайтом быть не может (нет `://`).
pub const ORIGIN: &str = "lumen-card:";

/// Разделитель полей в зашифрованной записи (в значениях полей не встречается).
const SEP: char = '\u{1f}';

#[derive(Clone, PartialEq, Eq)]
pub struct Card {
    pub number: String,
    pub name: String,
    pub expiry: String,
}

impl std::fmt::Debug for Card {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Card").field("last4", &self.last4()).finish_non_exhaustive()
    }
}

impl Card {
    pub fn last4(&self) -> String {
        let n = self.number.len();
        self.number[n.saturating_sub(4)..].to_owned()
    }

    /// Подпись строки списка и инфобара — без номера целиком.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("•••• {}", self.last4())
        } else {
            format!("•••• {} · {}", self.last4(), self.name)
        }
    }

    fn encode(&self) -> String {
        format!("{}{SEP}{}{SEP}{}", self.number, self.name, self.expiry)
    }

    fn decode(s: &str) -> Option<Self> {
        let mut it = s.split(SEP);
        let card = Self {
            number: it.next()?.to_owned(),
            name: it.next()?.to_owned(),
            expiry: it.next()?.to_owned(),
        };
        valid_number(&card.number).then_some(card)
    }
}

/// Номер без пробелов и дефисов; `None`, если это не 12–19 цифр с верной суммой Луна.
pub fn normalize_number(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    valid_number(&digits).then_some(digits)
}

fn valid_number(d: &str) -> bool {
    if !(12..=19).contains(&d.len()) || !d.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let sum: u32 = d
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let v = u32::from(b - b'0');
            if i % 2 == 1 {
                let v = v * 2;
                if v > 9 { v - 9 } else { v }
            } else {
                v
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Карта, введённая в форму `form`; `None`, если номера нет или он неверен.
pub fn collect_card(doc: &Document, form: NodeId) -> Option<Card> {
    let mut number = None;
    let mut name = String::new();
    let mut expiry = String::new();
    for f in find_autofill_forms(doc).into_iter().filter(|f| f.form == Some(form)) {
        for field in f.fields {
            let value = doc.control_value(field.node).trim().to_owned();
            match field.kind {
                FieldKind::CardNumber if number.is_none() => number = normalize_number(&value),
                FieldKind::CardName if name.is_empty() => name = value,
                FieldKind::CardExpiry if expiry.is_empty() => expiry = value,
                _ => {}
            }
        }
    }
    Some(Card { number: number?, name, expiry })
}

/// Предложение сохранить: `None`, если такая карта уже есть или хранилище молчит.
pub fn plan_offer(store: &SavedLogins, card: Card) -> Option<Card> {
    match store.get(ORIGIN, &card.last4()).ok()? {
        Some(saved) if saved.password == card.encode() => None,
        _ => Some(card),
    }
}

pub fn save(store: &SavedLogins, card: &Card, now: i64) -> lumen_core::Result<()> {
    store.save(ORIGIN, &card.last4(), &card.encode(), now).map(|_| ())
}

/// Сохранённые карты, свежие первыми.
pub fn saved_cards(store: &SavedLogins) -> Vec<Card> {
    let Ok(last4s) = store.usernames_for(ORIGIN) else { return Vec::new() };
    last4s
        .iter()
        .filter_map(|l| store.get(ORIGIN, l).ok().flatten())
        .filter_map(|s| Card::decode(&s.password))
        .collect()
}

/// Поля формы, в которые подставляется выбранная карта.
pub fn fill_values(doc: &Document, field: NodeId, card: &Card) -> Vec<(NodeId, String)> {
    let Some(form) = find_autofill_forms(doc)
        .into_iter()
        .find(|f| f.fields.iter().any(|x| x.node == field))
    else {
        return Vec::new();
    };
    let nodes_of = |k: FieldKind| form.fields.iter().filter(move |f| f.kind == k).map(|f| f.node);
    let mut out = Vec::new();
    out.extend(nodes_of(FieldKind::CardNumber).take(1).map(|n| (n, card.number.clone())));
    if !card.name.is_empty() {
        out.extend(nodes_of(FieldKind::CardName).take(1).map(|n| (n, card.name.clone())));
    }
    // Месяц и год в двух полях разделить без разметки нельзя — только одно поле срока.
    let mut exp = nodes_of(FieldKind::CardExpiry);
    if let (Some(n), None) = (exp.next(), exp.next())
        && !card.expiry.is_empty()
    {
        out.push((n, card.expiry.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const VISA: &str = "4111 1111 1111 1111";

    fn doc(html: &str) -> (Document, NodeId) {
        let d = lumen_html_parser::parse(html);
        let f = d.find_by_id("f").expect("form");
        (d, f)
    }

    #[test]
    fn luhn_and_normalization() {
        assert_eq!(normalize_number(VISA).as_deref(), Some("4111111111111111"));
        assert!(normalize_number("4111 1111 1111 1112").is_none());
        assert!(normalize_number("1234").is_none());
        assert!(normalize_number("4111a11111111111").is_none());
    }

    #[test]
    fn collects_card_without_cvc() {
        let (d, f) = doc(&format!(
            r#"<form id="f"><input autocomplete="cc-number" value="{VISA}">
            <input autocomplete="cc-name" value="IVAN IVANOV"><input autocomplete="cc-exp" value="12/30">
            <input autocomplete="cc-csc" value="123"></form>"#
        ));
        let c = collect_card(&d, f).expect("card");
        assert_eq!((c.number.as_str(), c.name.as_str(), c.expiry.as_str()), ("4111111111111111", "IVAN IVANOV", "12/30"));
        assert!(!c.encode().contains("123"));
        assert_eq!(c.label(), "•••• 1111 · IVAN IVANOV");
        assert!(!format!("{c:?}").contains("4111"));
        let (d, f) = doc(r#"<form id="f"><input autocomplete="cc-number" value="1234"></form>"#);
        assert!(collect_card(&d, f).is_none());
    }

    #[test]
    fn stored_encrypted_and_offered_once() {
        let store = SavedLogins::open_in_memory([7u8; 32]).unwrap();
        let card = Card { number: "4111111111111111".into(), name: "A B".into(), expiry: "01/31".into() };
        assert_eq!(plan_offer(&store, card.clone()), Some(card.clone()));
        save(&store, &card, 1).unwrap();
        assert!(plan_offer(&store, card.clone()).is_none());
        assert_eq!(saved_cards(&store), [card]);
    }

    #[test]
    fn fill_targets_one_form_only() {
        let (d, _) = doc(
            r#"<form id="f"><input id="n" autocomplete="cc-number"><input autocomplete="cc-name">
            <input autocomplete="cc-exp-month"><input autocomplete="cc-exp-year"></form>"#,
        );
        let n = d.find_by_id("n").unwrap();
        let card = Card { number: "4111111111111111".into(), name: "A B".into(), expiry: "01/31".into() };
        let got = fill_values(&d, n, &card);
        assert_eq!(got.len(), 2, "два поля срока не заполняются");
        assert_eq!(got[0], (n, card.number.clone()));
    }
}

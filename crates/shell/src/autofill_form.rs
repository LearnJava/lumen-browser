//! Распознавание полей автозаполнения: имя, почта, телефон, адрес, карта
//! (UX-AUTOFILL, срез 1).
//!
//! Чистая функция над DOM: ничего не читает из хранилища и не показывает UI.
//! Следующие срезы (подсказка у поля, сохранение после отправки) опираются на
//! [`find_autofill_forms`], чтобы знать, какое поле какого рода. Формы с полем
//! пароля сюда не входят — ими занимается [`crate::login_form`].

use lumen_dom::{Document, InputType, NodeId};

/// Род поля.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    Name,
    GivenName,
    FamilyName,
    Email,
    Tel,
    Organization,
    StreetAddress,
    City,
    Region,
    PostalCode,
    Country,
    CardNumber,
    CardName,
    CardExpiry,
    CardCvc,
}

impl FieldKind {
    /// Платёжные данные: сохранять и подставлять только с явного подтверждения.
    pub fn is_card(self) -> bool {
        matches!(self, Self::CardNumber | Self::CardName | Self::CardExpiry | Self::CardCvc)
    }

    /// Код, который никогда не сохраняется (даже с подтверждением).
    pub fn is_never_stored(self) -> bool {
        self == Self::CardCvc
    }

    /// Ключ `field_name` в хранилище `lumen_storage::Autofill`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::GivenName => "given-name",
            Self::FamilyName => "family-name",
            Self::Email => "email",
            Self::Tel => "tel",
            Self::Organization => "organization",
            Self::StreetAddress => "street-address",
            Self::City => "address-level2",
            Self::Region => "address-level1",
            Self::PostalCode => "postal-code",
            Self::Country => "country",
            Self::CardNumber => "cc-number",
            Self::CardName => "cc-name",
            Self::CardExpiry => "cc-exp",
            Self::CardCvc => "cc-csc",
        }
    }
}

/// Распознанное поле.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutofillField {
    pub node: NodeId,
    pub kind: FieldKind,
}

/// Форма (или набор полей вне `<form>`) с полями автозаполнения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutofillForm {
    pub form: Option<NodeId>,
    pub fields: Vec<AutofillField>,
}

impl AutofillForm {
    pub fn has_card(&self) -> bool {
        self.fields.iter().any(|f| f.kind.is_card())
    }
}

/// Найти формы с распознанными полями, в порядке появления. Формы входа
/// (с полем пароля) пропускаются целиком.
pub fn find_autofill_forms(doc: &Document) -> Vec<AutofillForm> {
    let mut inputs = Vec::new();
    collect_inputs(doc, doc.root(), &mut inputs);

    let mut groups: Vec<(Option<NodeId>, Vec<NodeId>)> = Vec::new();
    for id in inputs {
        let owner = lumen_dom::find_ancestor_form(doc, id);
        match groups.iter_mut().find(|(o, _)| *o == owner) {
            Some((_, v)) => v.push(id),
            None => groups.push((owner, vec![id])),
        }
    }

    groups
        .into_iter()
        .filter(|(_, members)| {
            !members
                .iter()
                .any(|&id| doc.get(id).input_type() == Some(InputType::Password))
        })
        .filter_map(|(form, members)| {
            let fields: Vec<AutofillField> = members
                .iter()
                .filter_map(|&node| classify(doc, node).map(|kind| AutofillField { node, kind }))
                .collect();
            (!fields.is_empty()).then_some(AutofillForm { form, fields })
        })
        .collect()
}

/// Род одного поля: сначала токены `autocomplete`, потом тип `<input>` и
/// эвристика по `name`/`id`/`placeholder`. `autocomplete=off` отключает поле.
pub fn classify(doc: &Document, id: NodeId) -> Option<FieldKind> {
    let node = doc.get(id);
    let ty = node.input_type()?;
    if !matches!(ty, InputType::Text | InputType::Email | InputType::Tel) {
        return None;
    }
    if let Some(ac) = node.get_attr("autocomplete") {
        let mut off = false;
        for token in ac.split_whitespace() {
            match from_token(&token.to_ascii_lowercase()) {
                Some(k) => return Some(k),
                None => off |= token.eq_ignore_ascii_case("off"),
            }
        }
        if off {
            return None;
        }
    }
    match ty {
        InputType::Email => return Some(FieldKind::Email),
        InputType::Tel => return Some(FieldKind::Tel),
        _ => {}
    }
    let hint = [node.get_attr("name"), node.get_attr("id"), node.get_attr("placeholder")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    from_hint(&hint)
}

fn from_token(t: &str) -> Option<FieldKind> {
    Some(match t {
        "name" => FieldKind::Name,
        "given-name" | "additional-name" => FieldKind::GivenName,
        "family-name" => FieldKind::FamilyName,
        "email" => FieldKind::Email,
        "tel" | "tel-national" | "tel-local" => FieldKind::Tel,
        "organization" => FieldKind::Organization,
        "street-address" | "address-line1" | "address-line2" | "address-line3" => FieldKind::StreetAddress,
        "address-level2" => FieldKind::City,
        "address-level1" => FieldKind::Region,
        "postal-code" => FieldKind::PostalCode,
        "country" | "country-name" => FieldKind::Country,
        "cc-number" => FieldKind::CardNumber,
        "cc-name" | "cc-given-name" | "cc-family-name" => FieldKind::CardName,
        "cc-exp" | "cc-exp-month" | "cc-exp-year" => FieldKind::CardExpiry,
        "cc-csc" => FieldKind::CardCvc,
        _ => return None,
    })
}

/// Эвристика без `autocomplete`. Порядок проверок важен: карточные — раньше
/// общих («card name» не должно стать именем).
fn from_hint(h: &str) -> Option<FieldKind> {
    let has = |subs: &[&str]| subs.iter().any(|s| h.contains(s));
    if has(&["cvv", "cvc", "csc", "security code"]) {
        Some(FieldKind::CardCvc)
    } else if has(&["card number", "cardnumber", "card-number", "card_number", "ccnum", "cc-number"]) {
        Some(FieldKind::CardNumber)
    } else if has(&["card name", "cardholder", "card-name", "card_name", "name on card"]) {
        Some(FieldKind::CardName)
    } else if has(&["expiry", "expiration", "exp-date", "exp_date", "expdate"]) {
        Some(FieldKind::CardExpiry)
    } else if has(&["email", "e-mail"]) {
        Some(FieldKind::Email)
    } else if has(&["phone", "mobile", "telephone"]) {
        Some(FieldKind::Tel)
    } else if has(&["zip", "postal", "postcode"]) {
        Some(FieldKind::PostalCode)
    } else if has(&["first name", "first-name", "first_name", "firstname", "given"]) {
        Some(FieldKind::GivenName)
    } else if has(&["last name", "last-name", "last_name", "lastname", "surname", "family"]) {
        Some(FieldKind::FamilyName)
    } else if has(&["full name", "full-name", "full_name", "fullname", "your name"]) || h.trim() == "name" {
        Some(FieldKind::Name)
    } else if has(&["company", "organization", "organisation"]) {
        Some(FieldKind::Organization)
    } else if has(&["address", "street"]) {
        Some(FieldKind::StreetAddress)
    } else if has(&["city", "town"]) {
        Some(FieldKind::City)
    } else if has(&["state", "province", "region"]) {
        Some(FieldKind::Region)
    } else if has(&["country"]) {
        Some(FieldKind::Country)
    } else {
        None
    }
}

/// Видимые, не отключённые и не только читаемые `<input>` в порядке документа.
fn collect_inputs(doc: &Document, id: NodeId, out: &mut Vec<NodeId>) {
    let node = doc.get(id);
    if node.input_type().is_some()
        && node.get_attr("disabled").is_none()
        && node.get_attr("hidden").is_none()
        && node.get_attr("readonly").is_none()
    {
        out.push(id);
    }
    for &child in &node.children.clone() {
        collect_inputs(doc, child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(html: &str) -> Vec<FieldKind> {
        let doc = lumen_html_parser::parse(html);
        find_autofill_forms(&doc)
            .into_iter()
            .flat_map(|f| f.fields.into_iter().map(|x| x.kind))
            .collect()
    }

    #[test]
    fn autocomplete_tokens_win_over_hints() {
        assert_eq!(
            kinds(r#"<form><input name="phone" autocomplete="email"><input autocomplete="section-a shipping postal-code"></form>"#),
            [FieldKind::Email, FieldKind::PostalCode]
        );
    }

    #[test]
    fn input_type_and_name_hints() {
        assert_eq!(
            kinds(r#"<form><input type="email"><input type="tel"><input name="firstName"><input name="last_name"><input id="zip"><input name="city"></form>"#),
            [
                FieldKind::Email,
                FieldKind::Tel,
                FieldKind::GivenName,
                FieldKind::FamilyName,
                FieldKind::PostalCode,
                FieldKind::City
            ]
        );
    }

    #[test]
    fn card_hints_do_not_become_name() {
        let k = kinds(r#"<form><input name="card_name"><input name="cardnumber"><input placeholder="CVV"><input name="expiry"></form>"#);
        assert_eq!(k, [FieldKind::CardName, FieldKind::CardNumber, FieldKind::CardCvc, FieldKind::CardExpiry]);
        assert!(k.iter().all(|k| k.is_card()));
        assert!(FieldKind::CardCvc.is_never_stored());
        assert!(!FieldKind::CardNumber.is_never_stored());
    }

    #[test]
    fn off_disabled_hidden_readonly_and_search_are_skipped() {
        assert!(kinds(
            r#"<form><input type="email" autocomplete="off"><input type="email" disabled>
               <input type="email" hidden><input type="email" readonly><input type="search" name="email"></form>"#
        )
        .is_empty());
    }

    #[test]
    fn unknown_autocomplete_token_falls_back_to_hint() {
        assert_eq!(kinds(r#"<form><input type="email" autocomplete="nope"></form>"#), [FieldKind::Email]);
    }

    #[test]
    fn login_forms_are_left_to_the_password_manager() {
        assert!(kinds(r#"<form><input type="email"><input type="password"></form>"#).is_empty());
    }

    #[test]
    fn forms_are_separate_and_formless_group() {
        let doc = lumen_html_parser::parse(
            r#"<form><input type="email"></form><form><input type="tel"></form><div><input name="city"></div>"#,
        );
        let f = find_autofill_forms(&doc);
        assert_eq!(f.len(), 3);
        assert!(f[0].form.is_some() && f[2].form.is_none());
        assert!(!f[0].has_card());
    }

    #[test]
    fn storage_keys_are_distinct() {
        use std::collections::HashSet;
        let all = [
            FieldKind::Name,
            FieldKind::GivenName,
            FieldKind::FamilyName,
            FieldKind::Email,
            FieldKind::Tel,
            FieldKind::Organization,
            FieldKind::StreetAddress,
            FieldKind::City,
            FieldKind::Region,
            FieldKind::PostalCode,
            FieldKind::Country,
            FieldKind::CardNumber,
            FieldKind::CardName,
            FieldKind::CardExpiry,
            FieldKind::CardCvc,
        ];
        assert_eq!(all.iter().map(|k| k.key()).collect::<HashSet<_>>().len(), all.len());
    }
}

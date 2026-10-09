//! `#loginBar` — инфобар «сохранить пароль?» (UX-PASSWORDS, срез 2).
//!
//! Дочерний модуль `model`: пишет через те же отслеживаемые примитивы, что и
//! остальная привязка.

use lumen_dom::Document;

use super::{set_class_token, set_text};

/// Состояние `#loginBar`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeLoginOfferModel {
    /// `true` показывает панель (`.open`).
    pub open: bool,
    /// `#loginTitle`: «Сохранить пароль?» или «Обновить пароль?».
    pub title: String,
    /// `#loginMeta`: сайт и логин. Пароль сюда не попадает.
    pub meta: String,
    /// Текст на кнопке `#loginSaveBtn`.
    pub save_label: String,
}

pub(super) fn bind_login_offer(doc: &mut Document, offer: &ChromeLoginOfferModel) {
    if let Some(bar) = doc.find_by_id(crate::ids::LOGIN_BAR) {
        set_class_token(doc, bar, "open", offer.open);
    }
    if let Some(n) = doc.find_by_id(crate::ids::LOGIN_TITLE) {
        set_text(doc, n, &offer.title);
    }
    if let Some(n) = doc.find_by_id(crate::ids::LOGIN_META) {
        set_text(doc, n, &offer.meta);
    }
    if let Some(n) = doc.find_by_id(crate::ids::LOGIN_SAVE_BTN) {
        set_text(doc, n, &offer.save_label);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{bind_model, has_class, ChromeModel};
    use super::*;

    #[test]
    fn login_bar_follows_the_model() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path).expect("chrome.html");
        let mut doc = lumen_html_parser::parse(&html);
        let bar = doc.find_by_id(crate::ids::LOGIN_BAR).expect("asset has #loginBar");
        bind_model(&mut doc, &ChromeModel::default());
        assert!(!has_class(&doc, bar, "open"));
        let model = ChromeModel {
            login_offer: ChromeLoginOfferModel {
                open: true,
                title: "Сохранить пароль?".into(),
                meta: "example.com · anna".into(),
                save_label: "Сохранить".into(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(has_class(&doc, bar, "open"));
        let meta = doc.find_by_id(crate::ids::LOGIN_META).expect("#loginMeta");
        let text = match &doc.get(doc.get(meta).children[0]).data {
            lumen_dom::NodeData::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert_eq!(text, "example.com · anna");
    }
}

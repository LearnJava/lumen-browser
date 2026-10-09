//! `#securityBox` — блокирующий экран безопасности: ошибка сертификата и
//! предупреждение о вредоносном сайте (UX-SECURITY-UI).
//!
//! Дочерний модуль `model`: пишет через те же отслеживаемые примитивы, что и
//! остальная привязка.

use lumen_dom::Document;

use super::{set_class_token, set_text};

/// Состояние `#securityBox`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeSecurityModel {
    /// `true` показывает экран (`.open`).
    pub open: bool,
    /// Заголовок: «Ваше подключение не защищено».
    pub title: String,
    /// Что произошло, для какого узла.
    pub message: String,
    /// Техническая причина (текст ошибки сертификата, тип угрозы).
    pub detail: String,
    /// Подпись опасного действия «Всё равно перейти».
    pub proceed_label: String,
}

pub(super) fn bind_security(doc: &mut Document, sec: &ChromeSecurityModel) {
    if let Some(bx) = doc.find_by_id(crate::ids::SECURITY_BOX) {
        set_class_token(doc, bx, "open", sec.open);
    }
    if let Some(n) = doc.find_by_id(crate::ids::SECURITY_TITLE) {
        set_text(doc, n, &sec.title);
    }
    if let Some(n) = doc.find_by_id(crate::ids::SECURITY_MESSAGE) {
        set_text(doc, n, &sec.message);
    }
    if let Some(n) = doc.find_by_id(crate::ids::SECURITY_DETAIL) {
        set_text(doc, n, &sec.detail);
    }
    if let Some(n) = doc.find_by_id(crate::ids::SECURITY_PROCEED_BTN) {
        let label = if sec.proceed_label.is_empty() { "Всё равно перейти" } else { &sec.proceed_label };
        set_text(doc, n, label);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{bind_model, has_class, ChromeModel};
    use super::*;

    #[test]
    fn security_box_follows_the_model() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path).expect("chrome.html");
        let mut doc = lumen_html_parser::parse(&html);
        let bx = doc.find_by_id(crate::ids::SECURITY_BOX).expect("asset has #securityBox");
        bind_model(&mut doc, &ChromeModel::default());
        assert!(!has_class(&doc, bx, "open"));
        let model = ChromeModel {
            security: ChromeSecurityModel {
                open: true,
                title: "Ваше подключение не защищено".into(),
                message: "bad.example".into(),
                detail: "certificate expired".into(),
                proceed_label: String::new(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(has_class(&doc, bx, "open"));
        let btn = doc.find_by_id(crate::ids::SECURITY_PROCEED_BTN).expect("proceed button");
        match doc.get(doc.get(btn).children[0]).data {
            lumen_dom::NodeData::Text(ref t) => assert_eq!(t, "Всё равно перейти"),
            _ => panic!("text expected"),
        }
    }
}

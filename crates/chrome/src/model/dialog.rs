//! `#dialogBox` — модальный диалог страницы `alert`/`confirm`/`prompt` и
//! подтверждение ухода (UX-DIALOGS).
//!
//! Дочерний модуль `model`: пишет через те же отслеживаемые примитивы, что и
//! остальная привязка.

use lumen_dom::Document;

use super::{set_class_token, set_text};

/// Состояние `#dialogBox`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeDialogModel {
    /// `true` показывает диалог (`.open`).
    pub open: bool,
    /// Заголовок: «example.com сообщает».
    pub title: String,
    /// Текст страницы.
    pub message: String,
    /// `Some` — это `prompt`: показать поле с текстом (каретка дописана вызывающим).
    pub input: Option<String>,
    /// `false` скрывает «Отмена» (`alert`).
    pub show_cancel: bool,
    /// Подпись основной кнопки.
    pub ok_label: String,
    /// Подпись «Отмена»; пусто — «Отмена».
    pub cancel_label: String,
}

pub(super) fn bind_dialog(doc: &mut Document, dialog: &ChromeDialogModel) {
    if let Some(bx) = doc.find_by_id(crate::ids::DIALOG_BOX) {
        set_class_token(doc, bx, "open", dialog.open);
        set_class_token(doc, bx, "has-input", dialog.input.is_some());
        set_class_token(doc, bx, "no-cancel", !dialog.show_cancel);
    }
    if let Some(n) = doc.find_by_id(crate::ids::DIALOG_TITLE) {
        set_text(doc, n, &dialog.title);
    }
    if let Some(n) = doc.find_by_id(crate::ids::DIALOG_MESSAGE) {
        set_text(doc, n, &dialog.message);
    }
    if let Some(n) = doc.find_by_id(crate::ids::DIALOG_INPUT) {
        set_text(doc, n, dialog.input.as_deref().unwrap_or(""));
    }
    if let Some(n) = doc.find_by_id(crate::ids::DIALOG_OK_BTN) {
        let label = if dialog.ok_label.is_empty() { "OK" } else { &dialog.ok_label };
        set_text(doc, n, label);
    }
    if let Some(n) = doc.find_by_id(crate::ids::DIALOG_CANCEL_BTN) {
        let label = if dialog.cancel_label.is_empty() { "Отмена" } else { &dialog.cancel_label };
        set_text(doc, n, label);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{bind_model, has_class, ChromeModel};
    use super::*;

    fn text_of(doc: &Document, id: &str) -> String {
        let n = doc.find_by_id(id).expect("node by id");
        match doc.get(n).children.first().map(|&c| &doc.get(c).data) {
            Some(lumen_dom::NodeData::Text(t)) => t.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn dialog_box_follows_the_model() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path).expect("chrome.html");
        let mut doc = lumen_html_parser::parse(&html);
        let bx = doc.find_by_id(crate::ids::DIALOG_BOX).expect("asset has #dialogBox");
        bind_model(&mut doc, &ChromeModel::default());
        assert!(!has_class(&doc, bx, "open"));
        let model = ChromeModel {
            dialog: ChromeDialogModel {
                open: true,
                title: "example.com сообщает".into(),
                message: "Как вас зовут?".into(),
                input: Some("Анна".into()),
                show_cancel: true,
                ok_label: String::new(),
                cancel_label: String::new(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(has_class(&doc, bx, "open"));
        assert!(has_class(&doc, bx, "has-input"));
        assert!(!has_class(&doc, bx, "no-cancel"));
        assert_eq!(text_of(&doc, crate::ids::DIALOG_MESSAGE), "Как вас зовут?");
        assert_eq!(text_of(&doc, crate::ids::DIALOG_INPUT), "Анна");
        assert_eq!(text_of(&doc, crate::ids::DIALOG_OK_BTN), "OK");
    }
}

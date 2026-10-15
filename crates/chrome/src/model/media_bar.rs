//! `#mediaBar` — инфобар «для видео нужен компонент ОС» (COMPAT-MEDIA-OSDEC-WIN-BAR).
//!
//! Дочерний модуль `model`: пишет через те же отслеживаемые примитивы, что и
//! остальная привязка.

use lumen_dom::Document;

use super::{set_class_token, set_text};

/// Состояние `#mediaBar`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeMediaBarModel {
    /// `true` показывает панель (`.open`).
    pub open: bool,
    /// `#mediaTitle`: что нужно поставить.
    pub title: String,
    /// `#mediaMeta`: пояснение.
    pub meta: String,
    /// Текст на кнопке `#mediaInstallBtn`.
    pub install_label: String,
}

pub(super) fn bind_media_bar(doc: &mut Document, bar: &ChromeMediaBarModel) {
    if let Some(n) = doc.find_by_id(crate::ids::MEDIA_BAR) {
        set_class_token(doc, n, "open", bar.open);
    }
    if let Some(n) = doc.find_by_id(crate::ids::MEDIA_TITLE) {
        set_text(doc, n, &bar.title);
    }
    if let Some(n) = doc.find_by_id(crate::ids::MEDIA_META) {
        set_text(doc, n, &bar.meta);
    }
    if let Some(n) = doc.find_by_id(crate::ids::MEDIA_INSTALL_BTN) {
        set_text(doc, n, &bar.install_label);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{bind_model, has_class, ChromeModel};
    use super::*;

    #[test]
    fn media_bar_follows_the_model() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path).expect("chrome.html");
        let mut doc = lumen_html_parser::parse(&html);
        let bar = doc.find_by_id(crate::ids::MEDIA_BAR).expect("asset has #mediaBar");
        bind_model(&mut doc, &ChromeModel::default());
        assert!(!has_class(&doc, bar, "open"));
        let model = ChromeModel {
            media_bar: ChromeMediaBarModel {
                open: true,
                title: "Нужен пакет мультимедиа Windows".into(),
                meta: "Видео не открылось".into(),
                install_label: "Открыть параметры".into(),
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(has_class(&doc, bar, "open"));
        let btn = doc.find_by_id(crate::ids::MEDIA_INSTALL_BTN).expect("#mediaInstallBtn");
        let text = match &doc.get(doc.get(btn).children[0]).data {
            lumen_dom::NodeData::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert_eq!(text, "Открыть параметры");
    }
}

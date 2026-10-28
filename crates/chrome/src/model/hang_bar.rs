//! `#hangBar` — плашка «Страница не отвечает» (COMPAT-HANG).
//!
//! Дочерний модуль `model`: пишет через те же отслеживаемые примитивы, что и
//! остальная привязка.

use lumen_dom::Document;

use super::{set_class_token, set_text};

/// Состояние `#hangBar`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChromeHangBarModel {
    /// `true` показывает панель (`.open`).
    pub open: bool,
    /// `#hangMeta`: сколько секунд скрипт не возвращает управление.
    pub meta: String,
}

pub(super) fn bind_hang_bar(doc: &mut Document, bar: &ChromeHangBarModel) {
    if let Some(n) = doc.find_by_id(crate::ids::HANG_BAR) {
        set_class_token(doc, n, "open", bar.open);
    }
    if let Some(n) = doc.find_by_id(crate::ids::HANG_META) {
        set_text(doc, n, &bar.meta);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{bind_model, has_class, ChromeModel};
    use super::*;

    #[test]
    fn hang_bar_follows_the_model() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path).expect("chrome.html");
        let mut doc = lumen_html_parser::parse(&html);
        let bar = doc.find_by_id(crate::ids::HANG_BAR).expect("asset has #hangBar");
        bind_model(&mut doc, &ChromeModel::default());
        assert!(!has_class(&doc, bar, "open"));
        let model = ChromeModel {
            hang_bar: ChromeHangBarModel { open: true, meta: "Скрипт не отвечает 7 с.".into() },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(has_class(&doc, bar, "open"));
        let meta = doc.find_by_id(crate::ids::HANG_META).expect("#hangMeta");
        let text = match &doc.get(doc.get(meta).children[0]).data {
            lumen_dom::NodeData::Text(t) => t.clone(),
            _ => String::new(),
        };
        assert_eq!(text, "Скрипт не отвечает 7 с.");
    }
}

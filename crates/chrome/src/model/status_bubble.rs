//! `#statusBubble` — адрес ссылки под курсором в нижнем углу страницы (UX-STATUS-BUBBLE).
//!
//! Дочерний модуль `model`: пишет через те же отслеживаемые примитивы, что и
//! остальная привязка. Таймлайн показа, усечение адреса и уход от курсора —
//! в оболочке (`crates/shell/src/status_bubble.rs`); здесь только запись
//! готового состояния в документ.

use lumen_dom::Document;

use super::{set_attr, set_class_token, set_text};

/// Состояние `#statusBubble`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChromeStatusBubbleModel {
    /// Что написано в пузыре; пустая строка — пузыря на экране нет.
    pub text: String,
    /// Прозрачность `0.0..=1.0` (fade-in/out считает оболочка).
    pub opacity: f32,
    /// `true` — правый нижний угол вместо левого (курсор подошёл к левому).
    pub right: bool,
    /// Отступ от нижнего края области страницы, px: высота горизонтальной
    /// полосы прокрутки минус съезд вниз от курсора (может быть отрицательным).
    pub bottom: f32,
}

pub(super) fn bind_status_bubble(doc: &mut Document, bubble: &ChromeStatusBubbleModel) {
    let Some(n) = doc.find_by_id(crate::ids::STATUS_BUBBLE) else { return };
    let shown = !bubble.text.is_empty() && bubble.opacity > 0.0;
    set_class_token(doc, n, "open", shown);
    if !shown {
        return;
    }
    set_class_token(doc, n, "right", bubble.right);
    set_text(doc, n, &bubble.text);
    set_attr(
        doc,
        n,
        "style",
        &format!("opacity:{:.3};bottom:{}px", bubble.opacity.clamp(0.0, 1.0), bubble.bottom),
    );
}

#[cfg(test)]
mod tests {
    use super::super::{bind_model, has_class, ChromeModel};
    use super::*;

    fn asset() -> Document {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
        let html = std::fs::read_to_string(&path).expect("chrome.html");
        lumen_html_parser::parse(&html)
    }

    fn bubble_text(doc: &Document, n: lumen_dom::NodeId) -> String {
        doc.get(n)
            .children
            .iter()
            .filter_map(|&c| match &doc.get(c).data {
                lumen_dom::NodeData::Text(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hidden_by_default_and_follows_the_model() {
        let mut doc = asset();
        let n = doc.find_by_id(crate::ids::STATUS_BUBBLE).expect("asset has #statusBubble");
        bind_model(&mut doc, &ChromeModel::default());
        assert!(!has_class(&doc, n, "open"));

        let model = ChromeModel {
            status_bubble: ChromeStatusBubbleModel {
                text: "https://example.com/путь".into(),
                opacity: 0.5,
                right: false,
                bottom: 8.0,
            },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(has_class(&doc, n, "open"));
        assert!(!has_class(&doc, n, "right"));
        assert_eq!(bubble_text(&doc, n), "https://example.com/путь");
        let style = doc.get(n).get_attr("style").unwrap_or_default().to_owned();
        assert!(style.contains("opacity:0.500") && style.contains("bottom:8px"), "{style}");
    }

    #[test]
    fn opposite_corner_and_slide_are_written() {
        let mut doc = asset();
        let n = doc.find_by_id(crate::ids::STATUS_BUBBLE).expect("#statusBubble");
        let model = ChromeModel {
            status_bubble: ChromeStatusBubbleModel { text: "u".into(), opacity: 1.0, right: true, bottom: -12.0 },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(has_class(&doc, n, "right"));
        assert!(doc.get(n).get_attr("style").unwrap_or_default().contains("bottom:-12px"));
    }

    #[test]
    fn faded_out_bubble_is_not_drawn() {
        let mut doc = asset();
        let n = doc.find_by_id(crate::ids::STATUS_BUBBLE).expect("#statusBubble");
        let model = ChromeModel {
            status_bubble: ChromeStatusBubbleModel { text: "u".into(), opacity: 0.0, right: false, bottom: 0.0 },
            ..ChromeModel::default()
        };
        bind_model(&mut doc, &model);
        assert!(!has_class(&doc, n, "open"));
    }
}

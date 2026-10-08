//! BUG-1312: `order` у детей flex/grid-контейнера меняет порядок отрисовки (CSS Grid L1 §6.3,
//! CSS Flexbox L1 §5.4 — «order-modified document order»), а не только раскладку.

use super::ordered_build_scroll::build_ordered;
use super::text_and_images::build;
use super::*;

/// Тексты `DrawText` в порядке следования в display list.
fn texts(dl: &DisplayList) -> Vec<String> {
    dl.iter()
        .filter_map(|c| match c {
            DisplayCommand::DrawText { text, .. } => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

const GRID_OVERLAP: &str = r#"<div style="display:grid">
    <div style="order:1;grid-area:1/1">G</div><div style="grid-area:1/1">R</div></div>"#;
const FLEX_ROW: &str = r#"<div style="display:flex">
    <div style="order:2">C</div><div style="order:-1">A</div><div>B</div></div>"#;

#[test]
fn grid_item_with_higher_order_paints_later() {
    assert_eq!(texts(&build_ordered(GRID_OVERLAP, "")), ["R", "G"]);
}

#[test]
fn grid_item_with_higher_order_paints_later_in_walk_path() {
    assert_eq!(texts(&build(GRID_OVERLAP, "")), ["R", "G"]);
}

#[test]
fn flex_items_paint_in_order_modified_document_order() {
    assert_eq!(texts(&build_ordered(FLEX_ROW, "")), ["A", "B", "C"]);
    assert_eq!(texts(&build(FLEX_ROW, "")), ["A", "B", "C"]);
}

#[test]
fn equal_order_keeps_document_order() {
    let html = r#"<div style="display:grid"><div style="order:3;grid-area:1/1">X</div>
        <div style="order:3;grid-area:1/1">Y</div></div>"#;
    assert_eq!(texts(&build_ordered(html, "")), ["X", "Y"]);
}

#[test]
fn order_is_ignored_outside_flex_and_grid() {
    let html = r#"<div><div style="order:1">P</div><div>Q</div></div>"#;
    assert_eq!(texts(&build_ordered(html, "")), ["P", "Q"]);
}

/// Позиционированный z-index:auto ребёнок — свой слой; порядок слоёв с равным z тоже по `order`.
#[test]
fn positioned_grid_items_with_equal_z_follow_order() {
    let html = r#"<div style="display:grid">
        <div style="order:1;grid-area:1/1;position:relative">G</div>
        <div style="grid-area:1/1;position:relative">R</div></div>"#;
    assert_eq!(texts(&build_ordered(html, "")), ["R", "G"]);
}

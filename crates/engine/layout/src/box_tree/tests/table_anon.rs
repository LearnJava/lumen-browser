use lumen_core::geom::{Rect, Size};

// BUG-1362: CSS 2.1 §17.2.1 — anonymous table objects.

fn lay(html: &str) -> (lumen_dom::Document, super::super::LayoutBox) {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse("body{margin:0}");
    let root = super::super::layout(&doc, &sheet, Size::new(500.0, 600.0));
    (doc, root)
}

fn rect_of(html: &str, id: &str) -> Rect {
    let (doc, root) = lay(html);
    super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect
}

const CELL: &str = "display:table-cell;width:40px;height:20px";

#[test]
fn orphan_cells_share_an_anonymous_table_row() {
    let html = format!(
        r#"<div><span id="a" style="{CELL}"></span><span id="b" style="{CELL}"></span></div>"#
    );
    let (a, b) = (rect_of(&html, "a"), rect_of(&html, "b"));
    assert_eq!((a.x, a.y, a.width, a.height), (0.0, 0.0, 40.0, 20.0), "{a:?}");
    assert_eq!((b.x, b.y), (40.0, 0.0), "{b:?}");
}

#[test]
fn orphan_rows_stack_in_one_anonymous_table() {
    let html = format!(
        r#"<div><span style="display:table-row"><span id="a" style="{CELL}"></span></span><span style="display:table-row"><span id="b" style="{CELL}"></span></span></div>"#
    );
    let (a, b) = (rect_of(&html, "a"), rect_of(&html, "b"));
    assert_eq!((a.x, a.y), (0.0, 0.0), "{a:?}");
    assert_eq!((b.x, b.y), (0.0, 20.0), "{b:?}");
}

#[test]
fn text_directly_in_a_table_gets_an_anonymous_row_and_cell() {
    let html = r#"<div id="t" style="display:table;font:20px/20px monospace">TXT</div>"#;
    let r = rect_of(html, "t");
    assert!(r.height >= 20.0, "таблица с одним текстом не пуста: {r:?}");
    let (_, root) = lay(html);
    let mut texts = 0;
    count_text(&root, &mut texts);
    assert!(texts > 0, "текст должен остаться в дереве");
}

fn count_text(b: &super::super::LayoutBox, n: &mut usize) {
    if let super::super::BoxKind::InlineRun { segments, .. } = &b.kind
        && segments.iter().any(|s| s.text.contains("TXT"))
    {
        *n += 1;
    }
    for c in &b.children {
        count_text(c, n);
    }
}

#[test]
fn text_in_a_row_becomes_an_anonymous_cell_beside_real_cells() {
    let html = format!(
        r#"<div style="display:table"><div id="r" style="display:table-row">TXT<span id="b" style="{CELL}"></span></div></div>"#
    );
    let (doc, root) = lay(&html);
    let b = super::find_by_id_all(&root, &doc, "b").expect("нет #b");
    assert_eq!(b.rect.width, 40.0, "{:?}", b.rect);
    // Строка — родитель #b; перед настоящей ячейкой стоит анонимная с текстом.
    let row = super::find_by_id_all(&root, &doc, "r").expect("нет #r");
    assert_eq!(row.children.len(), 2);
    assert_eq!(row.children[0].origin.role, super::super::BoxRole::AnonymousBlock);
    assert_eq!(row.children[0].style.display, lumen_layout_display_cell());
    assert!(!row.children[0].children.is_empty(), "текст не потерян");
    assert_eq!(row.children[1].origin.role, super::super::BoxRole::Element);
}

fn lumen_layout_display_cell() -> crate::style::Display {
    crate::style::Display::TableCell
}

#[test]
fn cells_directly_in_a_table_get_an_anonymous_row() {
    let html = format!(
        r#"<div style="display:table"><span id="a" style="{CELL}"></span><span id="b" style="{CELL}"></span></div>"#
    );
    let (a, b) = (rect_of(&html, "a"), rect_of(&html, "b"));
    assert_eq!((a.width, a.height), (40.0, 20.0), "{a:?}");
    assert_eq!((b.x, b.y), (40.0, 0.0), "{b:?}");
}

#[test]
fn well_formed_table_gets_no_anonymous_boxes() {
    let html = r#"<table id="t"><tbody><tr><td>a</td><td>b</td></tr></tbody></table>"#;
    let (doc, root) = lay(html);
    let table = super::find_by_id_all(&root, &doc, "t").expect("нет #t");
    assert_eq!(table.children.len(), 1);
    let tbody = &table.children[0];
    assert_eq!(tbody.origin.role, super::super::BoxRole::Element);
    assert_eq!(tbody.children.len(), 1);
    assert_eq!(tbody.children[0].origin.role, super::super::BoxRole::Element);
    assert_eq!(tbody.children[0].children.len(), 2);
}

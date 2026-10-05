//! CSS Fragmentation L3 §3.1 — `break-before`/`break-after: column|always` open a new column
//! of a multicol container (WPT `css-flexbox/alignment/flex-align-baseline-multicol-001`: three
//! blocks with `break-before/after: column` in `columns: 3` stand side by side, not stacked).

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

fn lay(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

fn children_of_wide(b: &LayoutBox, n: usize) -> Option<&Vec<LayoutBox>> {
    if b.children.len() >= n {
        return Some(&b.children);
    }
    b.children.iter().find_map(|c| children_of_wide(c, n))
}

/// `(x, y)` of the three `.b` boxes of a 3-column container with `extra` CSS on the boxes and
/// `fill` as its `column-fill`.
fn boxes(extra: &str, fill: &str) -> Vec<(f32, f32)> {
    let root = lay(
        "<div id=\"c\"><div class=\"b\"></div><div class=\"b\"></div><div class=\"b\"></div></div>",
        &format!(
            "body{{margin:0}} #c{{columns:3;column-gap:0;width:300px;column-fill:{fill}}} \
             .b{{height:20px;background:cyan;{extra}}}"
        ),
    );
    let kids = children_of_wide(&root, 3).expect("multicol container");
    kids.iter()
        .filter(|c| matches!(c.kind, BoxKind::Block) && c.rect.height > 0.0)
        .map(|c| (c.rect.x, c.rect.y))
        .collect()
}

#[test]
fn boxes_without_forced_breaks_share_a_column_when_filling_sequentially() {
    let v = boxes("", "auto");
    assert_eq!(v.len(), 3, "{v:?}");
    assert!(v.iter().all(|b| b.0 == 0.0), "{v:?}");
}

#[test]
fn break_before_column_puts_each_box_in_its_own_column() {
    for fill in ["auto", "balance"] {
        let v = boxes("break-before:column", fill);
        assert_eq!(v.len(), 3, "{fill}: {v:?}");
        assert_eq!(v.iter().map(|b| b.0).collect::<Vec<_>>(), [0.0, 100.0, 200.0], "{fill}");
        assert!(v.iter().all(|b| b.1 == 0.0), "{fill}: {v:?}");
    }
}

#[test]
fn break_after_always_is_a_column_break_too() {
    let v = boxes("break-after:always", "auto");
    assert_eq!(v.iter().map(|b| b.0).collect::<Vec<_>>(), [0.0, 100.0, 200.0]);
}

#[test]
fn break_before_page_does_not_break_a_column() {
    let v = boxes("break-before:page", "auto");
    assert!(v.iter().all(|b| b.0 == 0.0), "{v:?}");
}

#[test]
fn a_forced_break_after_the_last_box_adds_no_column() {
    let v = boxes("break-after:column", "auto");
    assert_eq!(v.iter().map(|b| b.0).collect::<Vec<_>>(), [0.0, 100.0, 200.0]);
}

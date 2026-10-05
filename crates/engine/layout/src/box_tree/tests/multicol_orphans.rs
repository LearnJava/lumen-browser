//! CSS Fragmentation L3 §3.3 — `orphans`/`widows` between the line boxes of a multicol
//! container (WPT `css/css-gaps/multicol/multicol-gap-decorations-026`: two lines before a
//! spanner and six after it, in 50px-wide columns).

use lumen_core::geom::Size;

use crate::box_tree::{BoxKind, LayoutBox};

fn lay(html: &str, css: &str) -> LayoutBox {
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(css);
    super::super::layout(&doc, &sheet, Size::new(800.0, 600.0))
}

/// `(x, y)` of every `InlineRun` under `b`, in tree order.
fn runs(b: &LayoutBox, out: &mut Vec<(f32, f32)>) {
    if matches!(b.kind, BoxKind::InlineRun { .. }) {
        out.push((b.rect.x, b.rect.y));
    }
    for c in &b.children {
        runs(c, out);
    }
}

fn columns(css: &str, text: &str) -> Vec<f32> {
    let root = lay(
        &format!("<div id=\"c\">{text}</div>"),
        &format!("body{{margin:0}} #c{{{css}}}"),
    );
    let mut v = Vec::new();
    runs(&root, &mut v);
    let mut xs: Vec<f32> = v.iter().map(|r| r.0).collect();
    xs.dedup();
    xs
}

const SIX: &str = "a<br>b<br>c<br>d<br>e<br>f";

#[test]
fn balancing_six_lines_makes_three_columns_not_six() {
    // Default `orphans`/`widows: 2`: columns of two lines (3 columns), not one line each.
    let xs = columns(
        "columns:6;column-gap:0;width:600px;font:16px/20px sans-serif",
        SIX,
    );
    assert_eq!(xs.len(), 3, "{xs:?}");
}

#[test]
fn rules_of_one_line_spread_the_lines_over_every_column() {
    let xs = columns(
        "columns:6;column-gap:0;width:600px;font:16px/20px sans-serif;orphans:1;widows:1",
        SIX,
    );
    assert_eq!(xs.len(), 6, "{xs:?}");
}

#[test]
fn a_filled_column_does_not_end_with_a_lone_line() {
    // `column-fill: auto`, 100px = five lines per column: a sixth line alone in column 2 would
    // be a widow, so column 1 gives up its last line.
    let xs = columns(
        "columns:2;column-gap:0;width:200px;height:100px;column-fill:auto;font:16px/20px sans-serif",
        SIX,
    );
    let mut per_col = std::collections::BTreeMap::new();
    let root = lay(
        &format!("<div id=\"c\">{SIX}</div>"),
        "body{margin:0} #c{columns:2;column-gap:0;width:200px;height:100px;column-fill:auto;font:16px/20px sans-serif}",
    );
    let mut v = Vec::new();
    runs(&root, &mut v);
    for (x, _) in v {
        *per_col.entry(x as i32).or_insert(0) += 1;
    }
    assert_eq!(xs.len(), 2);
    assert_eq!(per_col.values().copied().collect::<Vec<_>>(), vec![4, 2]);
}

//! CSS Fragmentation L3 §4 / CSS Tables L3 — a table cut across the columns of a multicol
//! container between its rows (`multicol_flow`).

use crate::{BoxKind, LayoutBox, first_element_child, lay_measured};

fn dump(b: &LayoutBox, d: usize, out: &mut String) {
    let k = match &b.kind {
        BoxKind::InlineRun { .. } => "run".to_string(),
        k => format!("{:?}", std::mem::discriminant(k)),
    };
    out.push_str(&format!(
        "{}{} {:?} x={:.0} y={:.0} w={:.0} h={:.0}\n",
        "  ".repeat(d),
        k,
        b.style.display,
        b.rect.x,
        b.rect.y,
        b.rect.width,
        b.rect.height
    ));
    for c in &b.children {
        dump(c, d + 1, out);
    }
}

#[test]
#[ignore = "probe: prints the tree"]
fn probe() {
    let root = lay_measured(
        "<div id='c'><table id='t'><tbody><tr><td style='height:100px'>a</td></tr><tr><td style='height:100px'>b</td></tr></tbody></table></div>",
        "#c { width: 300px; column-count: 2; column-gap: 0px; column-fill: auto; height: 100px; } table { border-spacing: 0; }",
        800.0,
    );
    let c = first_element_child(&root);
    let mut s = String::new();
    dump(c, 0, &mut s);
    panic!("{s}");
}

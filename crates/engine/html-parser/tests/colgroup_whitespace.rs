//! HTML LS §13.2.6.4.12 «in column group» — пробелы и комментарии между `<colgroup>` и `<col>`
//! остаются внутри группы (TABLE-VWM). Раньше пробел закрывал группу, и каждый `<col>` получал
//! собственную неявную `<colgroup>`: в таблице с `<colgroup> <col> <col></colgroup>` выходило
//! три группы вместо одной.

#![allow(clippy::unwrap_used)]

use lumen_dom::{Document, NodeData, NodeId};

fn count(doc: &Document, id: NodeId, local: &str) -> usize {
    let own = matches!(&doc.get(id).data, NodeData::Element { name, .. } if name.local == local);
    usize::from(own) + doc.get(id).children.iter().map(|&c| count(doc, c, local)).sum::<usize>()
}

#[test]
fn whitespace_between_colgroup_and_col_keeps_one_group() {
    let doc = lumen_html_parser::parse(
        "<table><colgroup> <col> <col>\n</colgroup><tr><td>x</td><td>y</td></tr></table>",
    );
    let root = doc.root();
    assert_eq!(count(&doc, root, "colgroup"), 1);
    assert_eq!(count(&doc, root, "col"), 2);
}

#[test]
fn comment_inside_colgroup_keeps_one_group() {
    let doc = lumen_html_parser::parse(
        "<table><colgroup><!-- a --><col><!-- b --><col></colgroup><tr><td>x</td></tr></table>",
    );
    assert_eq!(count(&doc, doc.root(), "colgroup"), 1);
}

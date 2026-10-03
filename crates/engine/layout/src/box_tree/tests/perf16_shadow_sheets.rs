//! PERF-16 срез 5 — `build_shadow_sheets` walks the host map, not every arena slot.
//!
//! The result must stay what the per-node scan produced: one sheet per host whose shadow tree
//! holds `<style>` text (nested hosts included), no entry for a host without styles or for a
//! document without shadow roots.

#[test]
fn sheets_are_keyed_by_hosts_with_style_text() {
    let doc = lumen_html_parser::parse(
        r#"<html><body>
        <div id="a"><template shadowrootmode="open"><style>p { color: red }</style><p>x</p></template></div>
        <div id="b"><template shadowrootmode="open"><p>no styles</p></template></div>
        <div id="c"><template shadowrootmode="open"><div id="inner"><template shadowrootmode="open"><style>i { top: 1px }</style></template></div></template></div>
        </body></html>"#,
    );
    let sheets = super::super::entry::build_shadow_sheets(&doc);
    let host = |id: &str| doc.find_by_id(id).unwrap_or_else(|| panic!("fixture `{id}`"));
    assert!(sheets.contains_key(&host("a")));
    assert!(!sheets.contains_key(&host("b")), "host without <style> has no sheet");
    assert!(!sheets.contains_key(&host("c")), "outer host's own tree has no <style>");
    assert_eq!(sheets.len(), 2, "`a` plus the host nested inside `c`'s shadow tree");
}

#[test]
fn document_without_shadow_roots_has_no_sheets() {
    let doc = lumen_html_parser::parse("<html><body><style>p{}</style><p>x</p></body></html>");
    assert!(super::super::entry::build_shadow_sheets(&doc).is_empty());
    assert_eq!(doc.shadow_hosts().count(), 0);
}

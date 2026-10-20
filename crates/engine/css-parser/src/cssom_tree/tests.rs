use super::*;

fn tree(src: &str) -> CssomTree {
    CssomTree::parse(src)
}

fn texts(t: &CssomTree) -> Vec<String> {
    t.rules.iter().filter(|r| r.kind.visible()).map(serialize::css_text).collect()
}

#[test]
fn flat_style_rule_serializes_on_one_line() {
    assert_eq!(texts(&tree("a{color:red;background:blue}")), ["a { color: red; background: blue; }"]);
    assert_eq!(texts(&tree("a{}")), ["a { }"]);
}

#[test]
fn selector_text_is_canonical() {
    assert_eq!(tree("a>b,c  d{x:y}").rules[0].prelude, "a > b, c d");
}

#[test]
fn invalid_selector_drops_the_rule() {
    assert_eq!(tree("a:nosuchpseudo{x:y} b{x:y}").rules.len(), 1);
    assert_eq!(tree("%{x:y}").rules.len(), 0);
}

#[test]
fn nested_rule_is_a_child_and_keeps_ampersand() {
    let t = tree("div { color: red; & .b { x: y } }");
    let div = &t.rules[0];
    assert_eq!(div.decls.len(), 1);
    assert_eq!(div.children.len(), 1);
    assert_eq!(div.children[0].prelude, "& .b");
    assert_eq!(texts(&t), ["div {\n  color: red;\n  & .b { x: y; }\n}"]);
}

#[test]
fn implicit_nested_rule_and_relative_combinator() {
    let t = tree("div { .a { x: y } > p { x: y } }");
    let kids: Vec<&str> = t.rules[0].children.iter().map(|c| c.prelude.as_str()).collect();
    assert_eq!(kids, ["& .a", "& > p"]);
}

#[test]
fn declarations_after_a_rule_become_nested_declarations() {
    let t = tree(".a { color: red; & b { x: y } background: blue; --z: 1 }");
    let a = &t.rules[0];
    assert_eq!(a.children.len(), 2);
    assert_eq!(a.children[1].kind, RuleKind::NestedDeclarations);
    assert_eq!(serialize::css_text(&a.children[1]), "background: blue; --z: 1;");
}

#[test]
fn group_inside_style_rule_holds_declarations_as_child() {
    let t = tree("div { @media screen { color: red; background-color: green; } }");
    assert_eq!(
        texts(&t),
        ["div {\n  @media screen {\n  color: red; background-color: green;\n}\n}"]
    );
}

#[test]
fn empty_group_serializes_with_empty_body() {
    assert_eq!(texts(&tree("@media screen {}")), ["@media screen {\n}"]);
    assert_eq!(texts(&tree("@scope {}")), ["@scope {\n}"]);
}

#[test]
fn media_condition_gets_space_after_colon() {
    assert_eq!(tree("@media (min-width:100px) { a{x:y} }").rules[0].prelude, "(min-width: 100px)");
}

#[test]
fn at_rules_are_visible_in_css_rules() {
    let src = "@layer a, b;\n@import url(x.css);\n@font-face { font-family: f }\n@keyframes k { from { x: y } to { x: z } }\n@property --p { syntax: '*'; inherits: false }";
    let t = tree(src);
    let kinds: Vec<RuleKind> = t.rules.iter().map(|r| r.kind).collect();
    // @import after @layer statement is allowed; @font-face follows.
    assert_eq!(
        kinds,
        [
            RuleKind::LayerStatement,
            RuleKind::Import,
            RuleKind::FontFace,
            RuleKind::Keyframes,
            RuleKind::Property
        ]
    );
    assert_eq!(texts(&t)[0], "@layer a, b;");
    assert_eq!(texts(&t)[1], "@import url(x.css);");
    assert_eq!(texts(&t)[2], "@font-face { font-family: f; }");
    assert_eq!(texts(&t)[3], "@keyframes k {\n  from { x: y; }\n  to { x: z; }\n}");
}

#[test]
fn layer_only_sheet_has_one_rule() {
    assert_eq!(tree("@layer base;").visible_len(), 1);
    assert_eq!(tree("@layer base { a { x: y } }").visible_len(), 1);
}

#[test]
fn import_string_is_serialized_as_url() {
    assert_eq!(texts(&tree("@import 'x.css' layer(a);"))[0], "@import url(\"x.css\") layer(a);");
}

#[test]
fn import_after_a_style_rule_is_dropped() {
    let t = tree("a{x:y} @import url(x.css);");
    assert_eq!(t.rules.len(), 1);
}

#[test]
fn unclosed_blocks_are_closed_by_eof() {
    let t = tree("@media screen { a { x: y");
    assert_eq!(texts(&t), ["@media screen {\n  a { x: y; }\n}"]);
}

#[test]
fn unknown_at_rule_is_hidden_but_kept_for_the_cascade() {
    let t = tree("@foo bar { x }\na{x:y}");
    assert_eq!(t.visible_len(), 1);
    assert!(t.to_source().contains("@foo bar"));
}

#[test]
fn custom_property_block_value_is_a_declaration() {
    let t = tree("a { --x: { b: c }; d: e }");
    assert_eq!(t.rules[0].children.len(), 0);
    assert_eq!(t.rules[0].decls.len(), 2);
}

#[test]
fn untouched_tree_roundtrips_raw_source() {
    let src = "a{color:red}\n@media screen{b{x:y}}";
    let t = tree(src);
    assert_eq!(t.to_source(), "a{color:red}\n@media screen{b{x:y}}");
}

#[test]
fn insert_and_delete_at_root() {
    let mut t = tree("a{x:y} c{x:y}");
    assert_eq!(t.insert_rule(None, "b{x:y}", 1), Ok(1));
    assert_eq!(t.rules[1].prelude, "b");
    assert_eq!(t.insert_rule(None, "b{x:y}", 9), Err(CssomTreeError::IndexSize));
    assert_eq!(t.insert_rule(None, "b{x:y} d{x:y}", 0), Err(CssomTreeError::Syntax));
    assert_eq!(t.insert_rule(None, "@nosuch x;", 0), Err(CssomTreeError::Syntax));
    assert_eq!(t.delete_rule(None, 0), Ok(()));
    assert_eq!(t.delete_rule(None, 5), Err(CssomTreeError::IndexSize));
    assert_eq!(t.rules.len(), 2);
}

#[test]
fn insert_at_rules_that_used_to_throw() {
    let mut t = tree("");
    for (i, text) in ["@layer a, b;", "@font-face { font-family: f }", "@keyframes k { from { x: y } }", "@scope (.a) { b { x: y } }", "@supports (display:flex) { a { x: y } }"]
        .iter()
        .enumerate()
    {
        assert_eq!(t.insert_rule(None, text, i), Ok(i), "{text}");
    }
    assert_eq!(t.visible_len(), 5);
}

#[test]
fn import_must_precede_other_rules() {
    let mut t = tree("a{x:y}");
    assert_eq!(t.insert_rule(None, "@import url(x.css);", 1), Err(CssomTreeError::HierarchyRequest));
    assert_eq!(t.insert_rule(None, "@import url(x.css);", 0), Ok(0));
    assert_eq!(t.insert_rule(None, "b{x:y}", 0), Err(CssomTreeError::HierarchyRequest));
}

#[test]
fn insert_into_group_and_style_rule() {
    let mut t = tree("@media screen { a{x:y} } .p { x: y }");
    let media = t.rules[0].id;
    let p = t.rules[1].id;
    assert_eq!(t.insert_rule(Some(media), "b{x:y}", 0), Ok(0));
    assert_eq!(t.rules[0].children[0].prelude, "b");
    assert_eq!(t.insert_rule(Some(p), "& .q { x: y }", 0), Ok(0));
    assert_eq!(t.insert_rule(Some(p), "color: red", 1), Ok(1));
    assert_eq!(t.rules[1].children[1].kind, RuleKind::NestedDeclarations);
    assert_eq!(
        t.insert_rule(Some(p), "nothing-to-insert-because-invalid-property", 0),
        Err(CssomTreeError::Syntax)
    );
    assert_eq!(t.insert_rule(Some(9_999_999), "a{}", 0), Err(CssomTreeError::NotFound));
}

#[test]
fn mutation_regenerates_source_that_reparses() {
    let mut t = tree("@media screen { a { x: y } }\n.p { color: red }");
    let media = t.rules[0].id;
    t.insert_rule(Some(media), "b { x: z }", 1).unwrap();
    let src = t.to_source();
    assert!(src.contains(".p { color: red }"), "untouched sibling keeps raw: {src}");
    let reparsed = crate::parse(&src);
    assert_eq!(reparsed.media_rules.len(), 1);
    assert_eq!(reparsed.media_rules[0].rules.len(), 2);
}

#[test]
fn set_prelude_validates_selectors() {
    let mut t = tree("a{x:y}");
    let id = t.rules[0].id;
    assert_eq!(t.set_prelude(id, "b>c"), Ok(true));
    assert_eq!(t.rules[0].prelude, "b > c");
    assert_eq!(t.set_prelude(id, "%"), Ok(false));
    assert_eq!(t.rules[0].prelude, "b > c");
}

#[test]
fn set_decls_replaces_declarations() {
    let mut t = tree("a{x:y}");
    let id = t.rules[0].id;
    t.set_decls(id, crate::parse_inline_style("color: blue")).unwrap();
    assert_eq!(texts(&t), ["a { color: blue; }"]);
    assert!(t.to_source().contains("color: blue"));
}

#[test]
fn parent_lookup() {
    let t = tree("a { & b { x: y } }");
    let a = t.rules[0].id;
    let b = t.rules[0].children[0].id;
    assert_eq!(t.parent_of(a), Some(None));
    assert_eq!(t.parent_of(b), Some(Some(a)));
    assert_eq!(t.parent_of(u32::MAX), None);
}

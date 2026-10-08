use super::sheet_diff::longest_increasing_run;
use super::*;

fn texts(rules: &[&Rule]) -> Vec<String> {
    rules.iter().map(|r| r.selector_text()).collect()
}

#[test]
fn rules_appended_at_the_end_are_the_whole_difference() {
    let old = parse("a { color: red } b { color: blue }");
    let new = parse("a { color: red } b { color: blue } .x { margin: 1px } .y { margin: 2px }");
    let changed = old.changed_style_rules(&new).expect("only plain rules differ");
    assert_eq!(texts(&changed), [".x", ".y"]);
}

#[test]
fn a_removed_rule_is_reported() {
    let old = parse("a { color: red } .gone { color: blue } b { color: green }");
    let new = parse("a { color: red } b { color: green }");
    let changed = old.changed_style_rules(&new).expect("only plain rules differ");
    assert_eq!(texts(&changed), [".gone"]);
}

#[test]
fn identical_plain_rules_give_an_empty_difference() {
    let old = parse("a { color: red } b { color: blue }");
    let new = old.clone();
    assert_eq!(old.changed_style_rules(&new).expect("same content").len(), 0);
}

#[test]
fn a_block_moved_from_the_end_to_the_middle_names_only_the_block() {
    // The shell merged `.m` into its base sheet, which put it ahead of the rules the
    // script-side patch had appended behind the base — the same rules, a different order.
    let old = parse("a { color: red } b { color: blue } c { color: gray } .m { margin: 1px }");
    let new = parse("a { color: red } .m { margin: 1px } b { color: blue } c { color: gray }");
    let changed = old.changed_style_rules(&new).expect("only plain rules differ");
    assert_eq!(texts(&changed), [".m"]);
}

#[test]
fn a_move_with_an_addition_names_both() {
    let old = parse("a { color: red } b { color: blue } .m { margin: 1px }");
    let new = parse("a { color: red } .m { margin: 1px } .n { margin: 2px } b { color: blue }");
    let changed = old.changed_style_rules(&new).expect("only plain rules differ");
    let mut names = texts(&changed);
    names.sort();
    assert_eq!(names, [".m", ".n"]);
}

#[test]
fn a_changed_declaration_counts_as_a_removed_and_an_added_rule() {
    let old = parse("a { color: red } b { color: blue } c { color: gray }");
    let new = parse("a { color: red } b { color: green } c { color: gray }");
    let changed = old.changed_style_rules(&new).expect("only plain rules differ");
    assert_eq!(texts(&changed), ["b", "b"]);
}

#[test]
fn a_difference_outside_the_style_rules_is_not_expressible() {
    let old = parse("a { color: red }");
    for extra in [
        "@layer base { a { color: blue } }",
        "@property --p { syntax: '<length>'; inherits: false; initial-value: 0px }",
        "@scope (.s) { a { color: blue } }",
        "@container (min-width: 1px) { a { color: blue } }",
    ] {
        let new = parse(&format!("a {{ color: red }} {extra}"));
        assert!(old.changed_style_rules(&new).is_none(), "{extra} must not be expressible as a style-rule change");
    }
}

#[test]
fn media_and_supports_blocks_name_the_rules_inside() {
    let old = parse("a { color: red } @media print { b { color: blue } }");
    let new = parse(
        "a { color: red } @media print { b { color: blue } } @media (min-width: 1px) { .m { margin: 1px } .n { margin: 2px } }          @supports (display: grid) { .s { margin: 3px } }",
    );
    let changed = old.changed_style_rules(&new).expect("only style rules differ");
    assert_eq!(texts(&changed), [".m", ".n", ".s"]);
    // The unchanged block is not named; a block edited in place is named whole.
    let edited = parse("a { color: red } @media print { b { color: blue } .x { margin: 0 } }");
    let changed = old.changed_style_rules(&edited).expect("only style rules differ");
    assert_eq!(texts(&changed), ["b", "b", ".x"]);
}

#[test]
fn font_faces_and_keyframes_are_not_in_the_way() {
    let old = parse("a { color: red }");
    let new = parse(
        "a { color: red } .x { margin: 1px } @keyframes k { from { opacity: 0 } to { opacity: 1 } }          @font-face { font-family: F; src: url(f.woff2) }",
    );
    let changed = old.changed_style_rules(&new).expect("the cascade reads neither");
    assert_eq!(texts(&changed), [".x"]);
}

#[test]
fn the_longest_in_order_run_is_what_stays_put() {
    let pairs: Vec<(usize, usize)> = [1usize, 2, 0, 3, 4, 5].iter().copied().enumerate().collect();
    let run = longest_increasing_run(&pairs);
    assert_eq!(run.iter().map(|p| p.1).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    assert!(longest_increasing_run(&[]).is_empty());
}

use super::*;

#[test]
fn every_sheet_that_comes_into_existence_gets_its_own_revision() {
    let a = parse("p { color: red }");
    let b = parse("p { color: red }");
    let c = a.clone();
    let d = Stylesheet::default();
    let e = Stylesheet::default();

    let revs = [a.revision(), b.revision(), c.revision(), d.revision(), e.revision()];
    for (i, x) in revs.iter().enumerate() {
        for (j, y) in revs.iter().enumerate() {
            assert!(
                i == j || x != y,
                "revisions must be unique: sheet {i} and {j} share {x:?}"
            );
        }
    }
    // Identity is not content: same CSS, and a clone, still compare equal.
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(d, e);
}

#[test]
fn merging_rules_in_changes_the_revision_and_carries_every_field() {
    let mut sheet = parse("p { color: red }");
    let before = sheet.revision();
    sheet.merge_from(parse(
        "@media print { p { color: blue } } \
         @color-profile --p { src: url(a.icc) } \
         @function --double(--x) { result: 2 } \
         div { color: green }",
    ));

    assert_ne!(before, sheet.revision(), "a mutated sheet is a different sheet");
    assert_eq!(sheet.rules.len(), 2, "the merged top-level rule must be there");
    assert_eq!(sheet.media_rules.len(), 1);
    // The two fields the hand-rolled merge at the old call site had missed.
    assert_eq!(sheet.color_profiles.len(), 1, "@color-profile must survive a merge");
    assert_eq!(sheet.function_rules.len(), 1, "@function must survive a merge");
}

#[test]
fn rule_selector_and_style_text_serialize_for_cssom() {
    let sheet = parse("p.a , div { color: red; font-weight: bold !important }");
    let rule = &sheet.rules[0];
    assert_eq!(rule.selector_text(), "p.a, div");
    assert_eq!(rule.style_css_text(), "color: red; font-weight: bold !important;");
}

#[test]
fn cssom_rules_preserves_source_order_across_style_and_media() {
    let sheet = parse("p { color: red } @media print { div { color: blue } } a { color: green }");
    let kinds: Vec<_> = sheet
        .cssom_rules()
        .iter()
        .map(|r| match r {
            CssomRuleRef::Style(_) => "style",
            CssomRuleRef::Media(_) => "media",
        })
        .collect();
    assert_eq!(kinds, ["style", "media", "style"]);
}

#[test]
fn insert_rule_appends_at_end_by_default_index() {
    let mut sheet = parse("a {}");
    let before = sheet.revision();
    let idx = sheet.insert_rule("b {}", 1).unwrap();
    assert_eq!(idx, 1);
    assert_ne!(before, sheet.revision());
    assert_eq!(sheet.cssom_rules().len(), 2);
    assert_eq!(sheet.rules[1].selector_text(), "b");
}

#[test]
fn insert_rule_at_a_middle_index_shifts_later_rules() {
    let mut sheet = parse("a {} c {}");
    sheet.insert_rule("b {}", 1).unwrap();
    let order: Vec<_> = sheet.rules.iter().map(Rule::selector_text).collect();
    assert_eq!(order, ["a", "b", "c"]);
}

#[test]
fn insert_rule_interleaves_media_and_style_correctly() {
    let mut sheet = parse("a {} @media print { p {} }");
    sheet.insert_rule("b {}", 1).unwrap();
    assert_eq!(sheet.rules.iter().map(Rule::selector_text).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(sheet.media_rules.len(), 1);
    let kinds: Vec<_> = sheet
        .cssom_rules()
        .iter()
        .map(|r| match r {
            CssomRuleRef::Style(_) => "style",
            CssomRuleRef::Media(_) => "media",
        })
        .collect();
    assert_eq!(kinds, ["style", "style", "media"]);
}

#[test]
fn insert_rule_rejects_index_past_the_end() {
    let mut sheet = parse("a {}");
    assert_eq!(sheet.insert_rule("b {}", 2), Err(CssomRuleMutationError::IndexSize));
}

#[test]
fn insert_rule_rejects_a_bare_declaration() {
    let mut sheet = Stylesheet::default();
    assert_eq!(sheet.insert_rule("color: red;", 0), Err(CssomRuleMutationError::Syntax));
}

#[test]
fn insert_rule_rejects_more_than_one_rule() {
    let mut sheet = Stylesheet::default();
    assert_eq!(sheet.insert_rule("a {} b {}", 0), Err(CssomRuleMutationError::Syntax));
}

#[test]
fn insert_rule_rejects_a_kind_cssom_rules_cannot_represent() {
    let mut sheet = Stylesheet::default();
    assert_eq!(
        sheet.insert_rule("@font-face { font-family: X; src: url(a.woff); }", 0),
        Err(CssomRuleMutationError::Syntax)
    );
}

#[test]
fn delete_rule_removes_the_rule_at_index() {
    let mut sheet = parse("a {} b {} c {}");
    let before = sheet.revision();
    sheet.delete_rule(1).unwrap();
    assert_ne!(before, sheet.revision());
    assert_eq!(sheet.rules.iter().map(Rule::selector_text).collect::<Vec<_>>(), ["a", "c"]);
}

#[test]
fn delete_rule_removes_a_media_block_without_disturbing_style_rules() {
    let mut sheet = parse("a {} @media print { p {} } b {}");
    sheet.delete_rule(1).unwrap();
    assert!(sheet.media_rules.is_empty());
    assert_eq!(sheet.rules.iter().map(Rule::selector_text).collect::<Vec<_>>(), ["a", "b"]);
}

#[test]
fn delete_rule_rejects_index_at_or_past_the_length() {
    let mut sheet = parse("a {}");
    assert_eq!(sheet.delete_rule(1), Err(CssomRuleMutationError::IndexSize));
    let mut empty = Stylesheet::default();
    assert_eq!(empty.delete_rule(0), Err(CssomRuleMutationError::IndexSize));
}

#[test]
fn mark_mutated_mints_a_new_revision() {
    let mut sheet = parse("p { color: red }");
    let before = sheet.revision();
    sheet.rules.push(Rule { selectors: Vec::new(), declarations: Vec::new() });
    sheet.mark_mutated();
    assert_ne!(before, sheet.revision());
}

/// The structural half of [`StylesheetRevision`]'s invariant: a cache keyed
/// by revision is only sound while every in-place mutation announces itself.
///
/// A promise of that shape breaks as *visibly wrong styles*, not as a slow
/// frame, and it breaks the day someone adds an innocuous `sheet.rules.
/// push(..)` three crates away — so it is guarded by scanning the sources
/// rather than by review (same reasoning, and the same shape, as
/// `lumen_chrome`'s `every_dom_mutation_in_model_rs_goes_through_a_tracked_
/// primitive`).
///
/// Only files that name `Stylesheet` are scanned: the container field names
/// are ordinary words (`rules`, `imports`, `properties`) that unrelated
/// types in the workspace also use, and a file that never mentions the type
/// cannot name a binding of it.
#[test]
fn every_stylesheet_mutation_in_the_workspace_announces_itself() {
    const FIELDS: &[&str] = &[
        "rules", "properties", "media_rules", "imports", "font_faces", "layer_order",
        "layers", "supports_rules", "keyframes", "counter_styles", "page_rules",
        "scope_rules", "starting_style_rules", "container_rules", "font_palette_values",
        "color_profiles", "function_rules", "top_level_order",
    ];
    const MUTATORS: &[&str] = &[
        "push(", "extend(", "append(", "insert(", "clear(", "remove(", "retain(",
        "truncate(", "pop(", "sort(", "sort_by(", "sort_by_key(", "dedup(",
        "swap_remove(", "drain(", "resize(", "split_off(",
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("crates");
    let root = root.canonicalize().unwrap_or(root);
    assert!(
        root.is_dir(),
        "the gate must scan real sources; {} is not a directory",
        root.display(),
    );
    // `parser.rs` is where `Stylesheet::merge_from` (the sanctioned mutator) is
    // implemented; this file's `mark_mutated_mints_a_new_revision` test below
    // deliberately mutates unguarded to prove the point, immediately followed
    // by `mark_mutated()`.
    let exempt: Vec<std::path::PathBuf> = ["src/parser.rs", "src/parser/tests/revision.rs"]
        .iter()
        .map(|rel| {
            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
            p.canonicalize().unwrap_or(p)
        })
        .collect();

    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    assert!(files.len() > 20, "only {} .rs files found — the walk is broken", files.len());

    let mut offenders = Vec::new();
    for path in &files {
        let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
        if exempt.contains(&canon) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path) else { continue };
        if !src.contains("Stylesheet") {
            continue;
        }
        for (n, line) in src.lines().enumerate() {
            for field in FIELDS {
                for mutator in MUTATORS {
                    let needle = format!(".{field}.{mutator}");
                    if line.contains(&needle) {
                        offenders.push(format!(
                            "{}:{}: {}",
                            path.display(),
                            n + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a `Stylesheet`'s rules were changed in place without minting a new \
         revision, which leaves every revision-keyed cache (the cascade's \
         `CascadeIndex`) serving the pre-change index. Use \
         `Stylesheet::merge_from`, or call `Stylesheet::mark_mutated` right \
         after:\n  {}",
        offenders.join("\n  "),
    );
}

/// Every `.rs` file under `dir`, skipping build output.
fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

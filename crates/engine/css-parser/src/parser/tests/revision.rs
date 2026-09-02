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
        "color_profiles", "function_rules",
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

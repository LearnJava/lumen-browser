use super::*;

mod panels;

fn parse_asset() -> Document {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/chrome/chrome.html");
    let html = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    crate::parse_document(&html).0
}

fn model_with_tabs(tabs: Vec<ChromeTabModel>) -> ChromeModel {
    ChromeModel { tabs, ..ChromeModel::default() }
}

/// BUG-341 S16 completeness gate — the structural half of the slice.
///
/// `ChromeMutations::content` is only sound if **every** write this module
/// makes into the document is bookkept. That cannot be established by
/// reading the file once: a future `bind_*` helper that calls
/// `doc.append_child` directly would silently produce stale boxes, and no
/// behavioural test covers a binding nobody has written yet. So the
/// invariant is enforced against this file's own source: the four raw
/// `Document` mutators may appear only on lines carrying the
/// `S16-tracked-primitive` marker, i.e. inside the wrappers that do the
/// bookkeeping.
///
/// If this fails on code you just added: don't add the marker — route the
/// call through `attach_child` / `insert_child_before` / `detach_node`, or
/// (for a new kind of in-place write) add a wrapper next to them that calls
/// `record_content`.
#[test]
fn every_dom_mutation_in_model_rs_goes_through_a_tracked_primitive() {
    const SRC: &str = include_str!("../model.rs");
    // Assembled at runtime rather than written out: a literal
    // `"doc.append_child("` in this file would match itself.
    let raw: Vec<String> = ["append_child", "insert_before", "detach", "get_mut"]
        .iter()
        .map(|m| format!("doc.{m}("))
        .collect();

    let mut offenders = Vec::new();
    for (i, line) in SRC.lines().enumerate() {
        if line.trim_start().starts_with("//") || line.contains("S16-tracked-primitive") {
            continue;
        }
        if raw.iter().any(|needle| line.contains(needle.as_str())) {
            offenders.push(format!("model.rs:{}: {}", i + 1, line.trim()));
        }
    }
    assert!(
        offenders.is_empty(),
        "these lines mutate the document without content bookkeeping, so\n\
         `ChromeMutations::content` would under-report and\n\
         `ContentDirty::Nodes` would hand `build_box` a stale subtree:\n{}",
        offenders.join("\n"),
    );
}

/// BUG-341 S16: a text-only model change (a renamed tab) must be reported
/// as content, and only as content.
///
/// Both halves matter. Reporting it as content is what stops the reused box
/// from keeping the old string — the corruption this slice risks. *Not*
/// reporting it as selector-relevant is what keeps S6's cascade root-set
/// empty, so a rename costs a box rebuild on one chain instead of a
/// document re-cascade.
///
/// Contrast with the omnibox, whose value lives in a `value` **attribute**:
/// that is a selector-relevant write (`[value]`, `:placeholder-shown`, …)
/// and correctly lands in both sets.
#[test]
fn bind_model_tracked_reports_a_renamed_tab_as_content_only() {
    let mut doc = parse_asset();
    let tab = |title: &str| {
        model_with_tabs(vec![ChromeTabModel {
            id: 1, title: title.to_owned(), active: true, sleeping: false, loading: false,
            is_child: false, tree: None, container_color: None, group: None,
        }])
    };
    bind_model_tracked(&mut doc, &tab("Alpha"));
    let touched = bind_model_tracked(&mut doc, &tab("Alphabet"));
    assert!(
        touched.selector.is_empty(),
        "a text-only change matches no selector differently — reporting it as \
         selector-relevant would re-cascade the subtree for nothing: {:?}",
        touched.selector,
    );
    assert!(
        !touched.content.is_empty(),
        "a text-only change MUST be reported as content — otherwise `ContentDirty::Nodes` \
         lets `build_box` reuse the tab row's box with the previous title in it",
    );
}

/// BUG-341 S16: rebinding a bit-identical model must report **nothing**,
/// content included.
///
/// This is the load-bearing precondition for S15's whole-document box reuse
/// on a pure hover frame: `relayout_chrome_host` calls `bind_model_tracked`
/// before *every* chrome layout, so any binding that rewrites the DOM
/// unconditionally shows up here as a content-dirty node and cancels the
/// reuse of its whole ancestor chain. Twelve cells did exactly that before
/// S16 (`set_text` detached and recreated its text node every call), which
/// only became visible once content was tracked at all — the sibling
/// `..._reports_nothing_touched_...` test above passes either way, because
/// none of those rewrites is selector-relevant.
#[test]
fn bind_model_tracked_reports_no_content_for_an_unchanged_model() {
    let mut doc = parse_asset();
    let model = ChromeModel::default();
    bind_model_tracked(&mut doc, &model);
    let second = bind_model_tracked(&mut doc, &model);
    assert!(
        second.content.is_empty(),
        "rebinding an identical model rewrote {} node(s) anyway: {:?}. Every binding must \
         compare before it writes — an unconditional rewrite here costs box reuse on every \
         single chrome frame, hover included.",
        second.content.len(),
        second.content,
    );
}

/// BUG-341 S6: two `bind_model_tracked` calls with a bit-identical model
/// must report nothing touched — this is the precondition
/// `relayout_chrome_host`'s incremental path relies on for a pure
/// interactive-state (hover/focus/active) cycle to still take the
/// `ContentDirty::Nothing` fast path exactly like before S6.
#[test]
fn bind_model_tracked_reports_nothing_touched_for_an_unchanged_model() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![ChromeTabModel {
        id: 1, title: "Alpha".to_owned(), active: true, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: None, group: None,
        }]);
    let first = bind_model_tracked(&mut doc, &model);
    assert!(!first.is_empty(), "the very first bind populates the tab list — must report touched nodes");
    let second = bind_model_tracked(&mut doc, &model);
    assert!(
        second.is_empty(),
        "rebinding the identical model must touch nothing: {second:?}",
    );
}

/// A theme flip changes `body[data-theme]` — `bind_model_tracked` must
/// report exactly `<body>`, not the whole document.
#[test]
fn bind_model_tracked_reports_the_body_on_a_theme_change() {
    let mut doc = parse_asset();
    bind_model_tracked(&mut doc, &ChromeModel { dark_theme: false, ..ChromeModel::default() });
    let touched = bind_model_tracked(&mut doc, &ChromeModel { dark_theme: true, ..ChromeModel::default() });
    let body = doc.body().expect("asset has <body>");
    assert_eq!(
        touched.selector.keys().copied().collect::<HashSet<_>>(),
        HashSet::from([body]),
        "only <body> should be reported touched",
    );
    // BUG-341 S17: the report names the attribute, not just the node.
    assert_eq!(
        touched.selector[&body].attrs.iter().map(String::as_str).collect::<Vec<_>>(),
        ["data-theme"],
        "a theme flip writes exactly `data-theme`",
    );
    assert!(!touched.selector[&body].structural, "a theme flip moves no children");
}

/// Adding a tab is a structural change on `#sbTabs`/`#hbarTabs` (the
/// `reconcile_row_list`-driven containers) — `bind_model_tracked` must
/// report the containers, not silently miss the row-count change.
#[test]
fn bind_model_tracked_reports_the_container_when_a_tab_is_added() {
    let mut doc = parse_asset();
    let one_tab = model_with_tabs(vec![ChromeTabModel {
        id: 1, title: "Alpha".to_owned(), active: true, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: None, group: None,
        }]);
    bind_model_tracked(&mut doc, &one_tab);
    let two_tabs = model_with_tabs(vec![
        ChromeTabModel {
            id: 1, title: "Alpha".to_owned(), active: true, sleeping: false, loading: false,
            is_child: false, tree: None, container_color: None, group: None,
        },
        ChromeTabModel {
            id: 2, title: "Beta".to_owned(), active: false, sleeping: false, loading: false,
            is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    let touched = bind_model_tracked(&mut doc, &two_tabs);
    let sb_tabs = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let hbar_tabs = doc.find_by_id(crate::ids::HBAR_TABS).expect("asset has #hbarTabs");
    assert!(
        touched.selector.get(&sb_tabs).is_some_and(|t| t.structural),
        "touched must report #sbTabs as structural: {touched:?}",
    );
    assert!(
        touched.selector.get(&hbar_tabs).is_some_and(|t| t.structural),
        "touched must report #hbarTabs as structural: {touched:?}",
    );
}

#[test]
fn binds_theme_layout_and_profile_onto_body() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        dark_theme: true,
        layout_vertical: false,
        profile_slug: Some("work".to_owned()),
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let body = doc.body().expect("asset has <body>");
    assert_eq!(doc.get(body).get_attr("data-theme"), Some("dark"));
    assert_eq!(doc.get(body).get_attr("data-layout"), Some("horizontal"));
    assert_eq!(doc.get(body).get_attr("data-profile"), Some("work"));
}

#[test]
fn no_profile_slug_removes_the_attribute() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel { profile_slug: None, ..ChromeModel::default() });
    let body = doc.body().expect("asset has <body>");
    assert_eq!(doc.get(body).get_attr("data-profile"), None);
}

#[test]
fn tab_list_is_rebuilt_from_the_model() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![
        ChromeTabModel {
            id: 7, title: "Alpha".to_owned(), active: true, sleeping: false, loading: false,
            is_child: false, tree: None, container_color: None, group: None,
        },
        ChromeTabModel {
            id: 9, title: "Beta".to_owned(), active: false, sleeping: true, loading: false,
            is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let rows: Vec<NodeId> = doc
        .get(container)
        .children
        .iter()
        .copied()
        .filter(|&c| has_class(&doc, c, "tab-row"))
        .collect();
    assert_eq!(rows.len(), 2, "old demo rows must be gone, only the 2 model tabs remain");
    assert!(has_class(&doc, rows[0], "active"));
    assert_eq!(doc.get(rows[0]).get_attr("data-tab-id"), Some("7"));
    assert!(has_class(&doc, rows[1], "sleeping"));
    assert_eq!(doc.get(rows[1]).get_attr("data-tab-id"), Some("9"));
}

#[test]
fn empty_tab_list_clears_all_rows() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &model_with_tabs(Vec::new()));
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let rows = doc.get(container).children.iter().filter(|&&c| has_class(&doc, c, "tab-row")).count();
    assert_eq!(rows, 0);
}

/// BUG-341/CC-14: rebinding an unchanged tab list must not tear down and
/// recreate the rows — `layout_mutation_incremental`'s `graft_geometry`
/// matches subtrees by `NodeId`, so a fresh id on every relayout (even
/// when nothing changed) defeated it. Checks identity is preserved not
/// just for the row itself but every descendant graft matches into
/// (fav/title/close), since `graft_geometry` recurses.
#[test]
fn rebinding_unchanged_tabs_preserves_row_and_descendant_node_ids() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![
        ChromeTabModel { id: 7, title: "Alpha".to_owned(), active: true, sleeping: false, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
        ChromeTabModel { id: 9, title: "Beta".to_owned(), active: false, sleeping: true, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let rows_before: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();
    let descendants_before: Vec<Vec<NodeId>> =
        rows_before.iter().map(|&r| doc.get(r).children.clone()).collect();

    bind_model(&mut doc, &model);
    let rows_after: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();
    let descendants_after: Vec<Vec<NodeId>> =
        rows_after.iter().map(|&r| doc.get(r).children.clone()).collect();

    assert_eq!(rows_before, rows_after, "unchanged rows must keep their NodeId across bind_model calls");
    assert_eq!(
        descendants_before, descendants_after,
        "unchanged rows' children (fav/title/close) must keep their NodeId too"
    );
}

/// A title change is the common case (only content differs, shape
/// doesn't) — must hit `update_tab_row`'s fast path: same row id, text
/// updated in place.
#[test]
fn rebinding_a_changed_title_keeps_the_row_id_and_updates_text_in_place() {
    let mut doc = parse_asset();
    let mut model = model_with_tabs(vec![
        ChromeTabModel { id: 7, title: "Alpha".to_owned(), active: true, sleeping: false, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row_before = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();

    model.tabs[0].title = "Alpha Renamed".to_owned();
    bind_model(&mut doc, &model);
    let row_after = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();
    assert_eq!(row_before, row_after, "a title-only change must not recreate the row");

    let title_node = doc
        .get(row_after)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-title"))
        .expect("row has a .tab-title span");
    let text: Vec<NodeId> = doc.get(title_node).children.clone();
    let NodeData::Text(s) = &doc.get(text[0]).data else { panic!(".tab-title has a text child") };
    assert_eq!(s, "Alpha Renamed");
}

/// Removing a tab must detach only the trailing surplus row and keep the
/// surviving row's id (position-based reconciliation, matching
/// `graft_geometry`'s own by-index matching).
#[test]
fn shrinking_the_tab_list_detaches_the_trailing_row_and_keeps_the_survivor_id() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![
        ChromeTabModel { id: 1, title: "One".to_owned(), active: true, sleeping: false, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
        ChromeTabModel { id: 2, title: "Two".to_owned(), active: false, sleeping: false, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let rows_before: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();

    bind_model(&mut doc, &model_with_tabs(vec![model.tabs[0].clone()]));
    let rows_after: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "tab-row")).collect();
    assert_eq!(rows_after, vec![rows_before[0]], "surviving row must keep its id; the extra row is detached");
}

/// A `sleeping` flip changes the row's trailing slot shape (badge vs
/// close button) — must fall back to rebuilding the row's children, but
/// the row itself keeps its id and the final DOM state is correct.
#[test]
fn toggling_sleeping_keeps_the_row_id_and_swaps_badge_for_close_button() {
    let mut doc = parse_asset();
    let mut model = model_with_tabs(vec![
        ChromeTabModel { id: 1, title: "One".to_owned(), active: false, sleeping: false, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row_before = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();

    model.tabs[0].sleeping = true;
    bind_model(&mut doc, &model);
    let row_after = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();
    assert_eq!(row_before, row_after, "a shape change must still keep the row's own id");
    assert!(has_class(&doc, row_after, "sleeping"));
    let children = doc.get(row_after).children.clone();
    assert!(children.iter().any(|&c| has_class(&doc, c, "tab-badge")));
    assert!(!children.iter().any(|&c| has_class(&doc, c, "tab-close")));
}

/// UX-LOADING: `loading` turns the active tab's fav slot into `.spinner`
/// and the reload button into the stop button; clearing it restores both.
#[test]
fn loading_tab_shows_spinner_and_stop_button() {
    let mut doc = parse_asset();
    let mut model = model_with_tabs(vec![
        ChromeTabModel { id: 1, title: "One".to_owned(), active: true, sleeping: false, loading: true, is_child: false, tree: None, container_color: None, group: None },
    ]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row = doc.get(container).children.iter().copied().find(|&c| has_class(&doc, c, "tab-row")).unwrap();
    let fav = doc.get(row).children.iter().copied().find(|&c| has_class(&doc, c, "spinner"));
    assert!(fav.is_some(), "loading tab must carry a .spinner");
    let btn = find_by_attr(&doc, "data-action", "reload").expect("asset has the reload button");
    assert!(has_class(&doc, btn, "loading"));

    model.tabs[0].loading = false;
    bind_model(&mut doc, &model);
    assert!(has_class(&doc, fav.unwrap(), "tab-fav"));
    assert!(!has_class(&doc, btn, "loading"));
}

/// Same identity-preservation guarantee for the workspace switcher.
#[test]
fn rebinding_unchanged_workspaces_preserves_item_and_descendant_node_ids() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        workspaces: vec![
            ChromeWorkspaceModel { id: 1, name: "Личное".to_owned(), active: true, color: "#0066FF".to_owned() },
            ChromeWorkspaceModel { id: 2, name: "Проект Х".to_owned(), active: false, color: "#8B5CF6".to_owned() },
        ],
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let container = find_by_attr(&doc, "data-testid", "workspace-switcher").expect("asset has the switcher");
    let items_before: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "ws-item")).collect();

    bind_model(&mut doc, &model);
    let items_after: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "ws-item")).collect();
    assert_eq!(items_before, items_after, "unchanged workspace items must keep their NodeId");
}

#[test]
fn workspace_switcher_is_rebuilt_and_add_button_stays_last() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        workspaces: vec![
            ChromeWorkspaceModel { id: 1, name: "Личное".to_owned(), active: true, color: "#0066FF".to_owned() },
            ChromeWorkspaceModel { id: 2, name: "Проект Х".to_owned(), active: false, color: "#8B5CF6".to_owned() },
        ],
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let container = find_by_attr(&doc, "data-testid", "workspace-switcher").expect("asset has the switcher");
    let children = doc.get(container).children.clone();
    let items: Vec<NodeId> = children.iter().copied().filter(|&c| has_class(&doc, c, "ws-item")).collect();
    assert_eq!(items.len(), 2);
    assert_eq!(doc.get(items[0]).get_attr("data-ws-id"), Some("1"));
    assert!(has_class(&doc, items[0], "active"));
    assert_eq!(doc.get(items[0]).get_attr("style"), Some("--ws-color:#0066FF"));
    assert_eq!(doc.get(items[1]).get_attr("data-ws-id"), Some("2"));
    assert!(!has_class(&doc, items[1], "active"));
    let add_btn_pos = children
        .iter()
        .position(|&c| doc.get(c).get_attr("data-action") == Some("add-workspace"))
        .expect("the '+' button must still be present");
    let last_item_pos = children.iter().position(|&c| c == items[1]).unwrap();
    assert!(add_btn_pos > last_item_pos, "the '+' button must stay after every rebuilt workspace item");
}

#[test]
fn hbar_tab_list_mirrors_the_sidebar_tab_list() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![
        ChromeTabModel {
            id: 3, title: "Gamma".to_owned(), active: true, sleeping: false, loading: false,
            is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::HBAR_TABS).expect("asset has #hbarTabs");
    let rows: Vec<NodeId> = doc
        .get(container)
        .children
        .iter()
        .copied()
        .filter(|&c| has_class(&doc, c, "hbar-tab"))
        .collect();
    assert_eq!(rows.len(), 1, "old demo hbar rows must be gone, only the 1 model tab remains");
    assert!(has_class(&doc, rows[0], "active"));
    assert_eq!(doc.get(rows[0]).get_attr("data-tab-id"), Some("3"));
}

/// UX-TAB-CLOSE-FREEZE: every `.hbar-tab` carries a `close-tab` × keyed to
/// its own tab id, and `hbar_tab_width` pins/unpins the inline width.
#[test]
fn hbar_tab_has_close_button_and_freeze_width() {
    let mut doc = parse_asset();
    let tab = |id: usize| ChromeTabModel {
        id, title: format!("T{id}"), active: id == 1, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: None, group: None,
    };
    let mut model = model_with_tabs(vec![tab(1), tab(2)]);
    model.hbar_tab_width = Some(120.0);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::HBAR_TABS).expect("asset has #hbarTabs");
    let rows: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "hbar-tab")).collect();
    assert_eq!(rows.len(), 2);
    for (row, id) in rows.iter().zip(["1", "2"]) {
        let close = doc
            .get(*row)
            .children
            .iter()
            .copied()
            .find(|&c| has_class(&doc, c, "tab-close"))
            .expect("hbar tab has a close button");
        assert_eq!(doc.get(close).get_attr("data-action"), Some("close-tab"));
        assert_eq!(doc.get(close).get_attr("data-tab-id"), Some(id));
        assert_eq!(
            doc.get(*row).get_attr("style"),
            Some("width:120px;min-width:120px;max-width:120px;flex:none")
        );
    }
    // Closing tab 1 keeps the pin on the survivor, releasing drops it.
    let mut model = model_with_tabs(vec![tab(2)]);
    model.hbar_tab_width = Some(120.0);
    bind_model(&mut doc, &model);
    model.hbar_tab_width = None;
    bind_model(&mut doc, &model);
    let rows: Vec<NodeId> =
        doc.get(container).children.iter().copied().filter(|&c| has_class(&doc, c, "hbar-tab")).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(doc.get(rows[0]).get_attr("style"), None);
}

#[test]
fn hbar_workspace_pills_mirror_the_sidebar_switcher() {
    let mut doc = parse_asset();
    let model = ChromeModel {
        workspaces: vec![
            ChromeWorkspaceModel { id: 5, name: "Чтение".to_owned(), active: true, color: "#1F9D55".to_owned() },
        ],
        ..ChromeModel::default()
    };
    bind_model(&mut doc, &model);
    let container = find_by_class(&doc, "hbar-ws").expect("asset has .hbar-ws");
    let pills: Vec<NodeId> = doc
        .get(container)
        .children
        .iter()
        .copied()
        .filter(|&c| has_class(&doc, c, "hbar-ws-pill"))
        .collect();
    assert_eq!(pills.len(), 1, "old demo pills must be gone, only the 1 model workspace remains");
    assert!(has_class(&doc, pills[0], "active"));
    assert_eq!(doc.get(pills[0]).get_attr("data-ws-id"), Some("5"));
    assert_eq!(doc.get(pills[0]).get_attr("style"), Some("--ws-color:#1F9D55"));
}

#[test]
fn sidebar_collapsed_flag_toggles_the_collapsed_class() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &ChromeModel { sidebar_collapsed: true, ..ChromeModel::default() });
    let sidebar = doc.find_by_id(crate::ids::SIDEBAR).expect("asset has #sidebar");
    assert!(has_class(&doc, sidebar, "collapsed"));

    bind_model(&mut doc, &ChromeModel { sidebar_collapsed: false, ..ChromeModel::default() });
    assert!(!has_class(&doc, sidebar, "collapsed"), "a later bind_model with the flag off must remove the class");
}

#[test]
fn child_tab_gets_the_child_class_tree_line_and_container_stripe() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![ChromeTabModel {
        id: 11, title: "Reply".to_owned(), active: false, sleeping: false, loading: false,
        is_child: true, tree: None, container_color: Some("#1F9D55".to_owned()), group: None,
        }]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-row"))
        .expect("one tab row bound");
    assert!(has_class(&doc, row, "child"));
    let has_tree_line = doc.get(row).children.iter().any(|&c| has_class(&doc, c, "tree-line"));
    assert!(has_tree_line, "child row must carry a .tree-line connector span");
    let stripe = doc
        .get(row)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "container-stripe"))
        .expect("container_color must render a .container-stripe span");
    assert_eq!(doc.get(stripe).get_attr("style"), Some("background:#1F9D55"));
}

fn tree_tab(id: usize, depth: usize, expanded: Option<bool>) -> ChromeTabModel {
    ChromeTabModel {
        id, title: format!("T{id}"), active: false, sleeping: false, loading: false,
        is_child: depth > 0, tree: Some(ChromeTabTree { depth, expanded }),
        container_color: None, group: None,
    }
}

fn first_tab_rows(doc: &Document) -> Vec<NodeId> {
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    doc.get(container).children.iter().copied().filter(|&c| has_class(doc, c, "tab-row")).collect()
}

/// UX-CHROME-PANELS 14: tree mode renders a collapse arrow only on a
/// parent row and indents deeper levels by `TREE_INDENT_PX` each.
#[test]
fn tree_rows_carry_arrow_on_parents_and_indent_by_depth() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![
        tree_tab(1, 0, Some(true)),
        tree_tab(2, 1, Some(false)),
        tree_tab(3, 2, None),
    ]);
    bind_model(&mut doc, &model);
    let rows = first_tab_rows(&doc);
    assert_eq!(rows.len(), 3);
    let arrow = |row: NodeId| doc.get(row).children.iter().copied().find(|&c| has_class(&doc, c, "tab-arrow"));
    let a1 = arrow(rows[0]).expect("parent row has an arrow");
    assert_eq!(doc.get(a1).get_attr("data-action"), Some("toggle-tab-subtree"));
    assert_eq!(doc.get(a1).get_attr("data-tab-id"), Some("1"));
    assert_eq!(doc.get(a1).get_attr("aria-expanded"), Some("true"));
    let a2 = arrow(rows[1]).expect("collapsed parent has an arrow");
    assert_eq!(doc.get(a2).get_attr("aria-expanded"), Some("false"));
    assert!(arrow(rows[2]).is_none(), "a leaf has no arrow");
    assert_eq!(doc.get(rows[0]).get_attr("style"), None);
    assert_eq!(doc.get(rows[1]).get_attr("style"), Some("margin-left:12px"));
    assert_eq!(doc.get(rows[2]).get_attr("style"), Some("margin-left:24px"));
}

/// Folding a subtree flips the arrow in place on the next bind, and
/// leaving tree mode drops the arrow and the indent again.
#[test]
fn tree_row_updates_in_place_when_folded_and_when_mode_ends() {
    let mut doc = parse_asset();
    bind_model(&mut doc, &model_with_tabs(vec![tree_tab(1, 0, Some(true)), tree_tab(2, 1, None)]));
    let row = first_tab_rows(&doc)[0];
    bind_model(&mut doc, &model_with_tabs(vec![tree_tab(1, 0, Some(false))]));
    let rows = first_tab_rows(&doc);
    assert_eq!(rows, vec![row], "the parent row keeps its node");
    let arrow = doc.get(row).children.iter().copied().find(|&c| has_class(&doc, c, "tab-arrow")).unwrap();
    assert_eq!(doc.get(arrow).get_attr("aria-expanded"), Some("false"));
    let flat = ChromeTabModel { tree: None, ..tree_tab(1, 0, None) };
    bind_model(&mut doc, &model_with_tabs(vec![flat]));
    let row = first_tab_rows(&doc)[0];
    assert!(!doc.get(row).children.iter().any(|&c| has_class(&doc, c, "tab-arrow")));
    assert_eq!(doc.get(row).get_attr("style"), None);
}

/// BUG-409: a grouped tab renders `.group-stripe` before `.container-stripe`
/// (matching `populate_tab_row_children`'s order), coloured per
/// `ChromeTabGroup::color` and titled with the group name for the sidebar's
/// hover tooltip.
#[test]
fn grouped_tab_renders_a_group_stripe_before_the_container_stripe() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![ChromeTabModel {
        id: 3, title: "Roadmap".to_owned(), active: false, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: Some("#1F9D55".to_owned()),
        group: Some(ChromeTabGroup { color: "#8B5CF6".to_owned(), name: "Проект Х".to_owned(), collapsed: false }),
    }]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-row"))
        .expect("one tab row bound");
    let children: Vec<NodeId> = doc.get(row).children.clone();
    assert!(has_class(&doc, children[0], "group-stripe"), "group stripe must render first");
    assert_eq!(doc.get(children[0]).get_attr("style"), Some("background:#8B5CF6"));
    assert_eq!(doc.get(children[0]).get_attr("title"), Some("Проект Х"));
    assert!(
        has_class(&doc, children[1], "container-stripe"),
        "container stripe must follow the group stripe"
    );
}

/// BUG-409: the leftmost (chip) row left visible for a collapsed group
/// hints at the fold in its stripe tooltip, since the row itself carries
/// no other visual difference from an expanded group's member.
#[test]
fn collapsed_group_stripe_title_hints_at_the_fold() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![ChromeTabModel {
        id: 4, title: "Chip".to_owned(), active: false, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: None,
        group: Some(ChromeTabGroup { color: "#3B82F6".to_owned(), name: "Работа".to_owned(), collapsed: true }),
    }]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-row"))
        .expect("one tab row bound");
    let stripe = doc
        .get(row)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "group-stripe"))
        .expect("group must render a .group-stripe span");
    assert_eq!(doc.get(stripe).get_attr("title"), Some("Работа (свёрнута)"));
}

#[test]
fn ungrouped_tab_has_no_group_stripe() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![ChromeTabModel {
        id: 5, title: "Solo".to_owned(), active: false, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: None, group: None,
    }]);
    bind_model(&mut doc, &model);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-row"))
        .expect("one tab row bound");
    assert!(doc.get(row).children.iter().all(|&c| !has_class(&doc, c, "group-stripe")));
}

/// BUG-409: joining a group changes the row's child shape (a
/// `.group-stripe` appears), so [`update_tab_row`] must take its
/// shape-mismatch fallback — but the row itself must still keep its
/// `NodeId` (BUG-341/CC-14), same guarantee `container_color` already has.
#[test]
fn rebinding_a_tab_that_joins_a_group_updates_row_in_place() {
    let mut doc = parse_asset();
    let ungrouped = model_with_tabs(vec![ChromeTabModel {
        id: 6, title: "Later Group".to_owned(), active: false, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: None, group: None,
    }]);
    bind_model(&mut doc, &ungrouped);
    let container = doc.find_by_id(crate::ids::SB_TABS).expect("asset has #sbTabs");
    let row_before = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-row"))
        .expect("one tab row bound");

    let grouped = model_with_tabs(vec![ChromeTabModel {
        id: 6, title: "Later Group".to_owned(), active: false, sleeping: false, loading: false,
        is_child: false, tree: None, container_color: None,
        group: Some(ChromeTabGroup { color: "#EF4444".to_owned(), name: "Red".to_owned(), collapsed: false }),
    }]);
    bind_model(&mut doc, &grouped);
    let row_after = doc
        .get(container)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-row"))
        .expect("one tab row bound");
    assert_eq!(row_before, row_after, "shape change keeps the row's own NodeId");
    let stripe = doc.get(row_after).children.iter().copied().find(|&c| has_class(&doc, c, "group-stripe"));
    assert!(stripe.is_some(), "row must gain a group stripe after the shape-change fallback rebuild");
}

/// CC-13: `rebuild_tab_list`/`rebuild_hbar_tab_list` replace the asset's
/// static `.tab-row`/`.hbar-tab` markup wholesale, so the `role="tab"`/
/// `aria-selected` `scripts/gen_chrome_assets.py` bakes into the static
/// asset never reaches a real bound row — this must be set in Rust
/// instead, mirroring `data-action`.
#[test]
fn bound_tab_rows_carry_role_tab_and_aria_selected_in_both_layouts() {
    let mut doc = parse_asset();
    let model = model_with_tabs(vec![
        ChromeTabModel { id: 1, title: "Активная".to_owned(), active: true, sleeping: false, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
        ChromeTabModel { id: 2, title: "Пример".to_owned(), active: false, sleeping: false, loading: false, is_child: false, tree: None, container_color: None, group: None,
        },
    ]);
    bind_model(&mut doc, &model);

    for container_id in [crate::ids::SB_TABS, crate::ids::HBAR_TABS] {
        let container = doc.find_by_id(container_id).expect("asset has the tab-list container");
        let rows: Vec<NodeId> = doc
            .get(container)
            .children
            .iter()
            .copied()
            .filter(|&c| has_class(&doc, c, "tab-row") || has_class(&doc, c, "hbar-tab"))
            .collect();
        assert_eq!(rows.len(), 2, "expected both bound tabs as rows under {container_id:?}");
        assert_eq!(doc.get(rows[0]).get_attr("role"), Some("tab"));
        assert_eq!(doc.get(rows[0]).get_attr("aria-selected"), Some("true"));
        assert_eq!(doc.get(rows[1]).get_attr("role"), Some("tab"));
        assert_eq!(doc.get(rows[1]).get_attr("aria-selected"), Some("false"));
    }

    let sb_container = doc.find_by_id(crate::ids::SB_TABS).unwrap();
    let first_row = doc
        .get(sb_container)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-row"))
        .unwrap();
    let close = doc
        .get(first_row)
        .children
        .iter()
        .copied()
        .find(|&c| has_class(&doc, c, "tab-close"))
        .expect("active tab row has a close button");
    assert_eq!(doc.get(close).get_attr("aria-label"), Some("Закрыть вкладку"));
}

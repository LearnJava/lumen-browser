//! Integration tests for lumen-a11y: AXTree construction from real HTML.

use lumen_a11y::{AXRole, LiveRegion, build_ax_tree};
use lumen_html_parser::parse;

// ── Implicit role mapping ────────────────────────────────────────────────────

#[test]
fn role_nav() {
    let doc = parse("<nav>Menu</nav>");
    let tree = build_ax_tree(&doc, doc.root());
    let nav = find_role_dfs(&tree.root, AXRole::Navigation);
    assert!(nav.is_some(), "expected Navigation role for <nav>");
}

#[test]
fn role_main() {
    let doc = parse("<main>Content</main>");
    let tree = build_ax_tree(&doc, doc.root());
    let m = find_role_dfs(&tree.root, AXRole::Main);
    assert!(m.is_some(), "expected Main role for <main>");
}

#[test]
fn role_headings() {
    for (tag, expected_level) in [("h1", 1u32), ("h2", 2), ("h3", 3), ("h4", 4), ("h5", 5), ("h6", 6)] {
        let doc = parse(&format!("<{tag}>Title</{tag}>"));
        let tree = build_ax_tree(&doc, doc.root());
        let h = find_role_dfs(&tree.root, AXRole::Heading);
        assert!(h.is_some(), "expected Heading role for <{tag}>");
        assert_eq!(h.unwrap().state.level, Some(expected_level), "<{tag}> level");
    }
}

#[test]
fn role_link_with_href() {
    let doc = parse(r#"<a href="/page">Click</a>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let link = find_role_dfs(&tree.root, AXRole::Link);
    assert!(link.is_some(), "expected Link role for <a href>");
}

#[test]
fn role_a_without_href_is_generic() {
    let doc = parse("<a>Not a link</a>");
    let tree = build_ax_tree(&doc, doc.root());
    let link = find_role_dfs(&tree.root, AXRole::Link);
    assert!(link.is_none(), "<a> without href should not be Link");
}

#[test]
fn role_img_with_alt() {
    let doc = parse(r#"<img src="x.png" alt="A photo">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let img = find_role_dfs(&tree.root, AXRole::Img);
    assert!(img.is_some(), "expected Img role for <img alt='...'>");
    assert_eq!(img.unwrap().name, "A photo");
}

#[test]
fn role_img_empty_alt_is_presentation() {
    let doc = parse(r#"<img src="x.png" alt="">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let pres = find_role_dfs(&tree.root, AXRole::Presentation);
    assert!(pres.is_some(), "expected Presentation role for <img alt=''>");
}

#[test]
fn role_button() {
    let doc = parse("<button>OK</button>");
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_some(), "expected Button role for <button>");
    assert_eq!(btn.unwrap().name, "OK");
}

#[test]
fn role_input_checkbox() {
    let doc = parse(r#"<input type="checkbox">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox);
    assert!(cb.is_some(), "expected Checkbox role for <input type='checkbox'>");
}

#[test]
fn role_input_radio() {
    let doc = parse(r#"<input type="radio">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let r = find_role_dfs(&tree.root, AXRole::Radio);
    assert!(r.is_some(), "expected Radio role for <input type='radio'>");
}

#[test]
fn role_input_text() {
    let doc = parse(r#"<input type="text">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let t = find_role_dfs(&tree.root, AXRole::TextBox);
    assert!(t.is_some(), "expected TextBox for <input type='text'>");
}

#[test]
fn role_input_submit() {
    let doc = parse(r#"<input type="submit" value="Send">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_some(), "expected Button for <input type='submit'>");
    assert_eq!(btn.unwrap().name, "Send");
}

#[test]
fn role_select_combobox() {
    let doc = parse("<select><option>A</option></select>");
    let tree = build_ax_tree(&doc, doc.root());
    let cb = find_role_dfs(&tree.root, AXRole::ComboBox);
    assert!(cb.is_some(), "expected ComboBox for <select>");
}

#[test]
fn role_select_multiple_listbox() {
    let doc = parse("<select multiple><option>A</option></select>");
    let tree = build_ax_tree(&doc, doc.root());
    let lb = find_role_dfs(&tree.root, AXRole::ListBox);
    assert!(lb.is_some(), "expected ListBox for <select multiple>");
}

#[test]
fn role_table_row_cell() {
    let doc = parse("<table><tr><td>Cell</td></tr></table>");
    let tree = build_ax_tree(&doc, doc.root());
    assert!(find_role_dfs(&tree.root, AXRole::Table).is_some(), "expected Table");
    assert!(find_role_dfs(&tree.root, AXRole::Row).is_some(), "expected Row");
    assert!(find_role_dfs(&tree.root, AXRole::Cell).is_some(), "expected Cell");
}

#[test]
fn role_list_and_listitem() {
    let doc = parse("<ul><li>Item</li></ul>");
    let tree = build_ax_tree(&doc, doc.root());
    assert!(find_role_dfs(&tree.root, AXRole::List).is_some(), "expected List");
    assert!(find_role_dfs(&tree.root, AXRole::ListItem).is_some(), "expected ListItem");
}

// ── Explicit role override ────────────────────────────────────────────────────

#[test]
fn explicit_role_overrides_implicit() {
    // A <div> with role="button" should become Button, not Generic.
    let doc = parse(r#"<div role="button">Click me</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_some(), "explicit role='button' should override div's Generic role");
}

#[test]
fn explicit_role_none_maps_to_none() {
    let doc = parse(r#"<img src="x.png" role="none">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let none = find_role_dfs(&tree.root, AXRole::None);
    assert!(none.is_some(), "role='none' should map to AXRole::None");
}

// ── Accessible name computation ───────────────────────────────────────────────

#[test]
fn name_from_aria_label() {
    let doc = parse(r#"<button aria-label="Close dialog">X</button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Close dialog");
}

#[test]
fn name_from_aria_labelledby() {
    let doc = parse(r#"<div id="lbl">First name</div><input aria-labelledby="lbl">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "First name");
}

#[test]
fn name_from_text_content() {
    let doc = parse("<button>Submit</button>");
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Submit");
}

#[test]
fn name_from_alt() {
    let doc = parse(r#"<img src="x.png" alt="Company logo">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let img = find_role_dfs(&tree.root, AXRole::Img).expect("img");
    assert_eq!(img.name, "Company logo");
}

#[test]
fn name_aria_label_takes_priority_over_text() {
    let doc = parse(r#"<button aria-label="Close">X</button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Close");
}

// ── State flags ───────────────────────────────────────────────────────────────

#[test]
fn state_disabled_from_html_attr() {
    let doc = parse(r#"<button disabled>Send</button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert!(btn.state.disabled, "button with disabled attr should have disabled=true");
}

#[test]
fn state_aria_disabled() {
    let doc = parse(r#"<div role="button" aria-disabled="true">Send</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert!(btn.state.disabled);
}

#[test]
fn state_required_from_html_attr() {
    let doc = parse(r#"<input type="text" required>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert!(tb.state.required);
}

#[test]
fn state_checked_checkbox_html() {
    let doc = parse(r#"<input type="checkbox" checked>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox).expect("checkbox");
    assert_eq!(cb.state.checked, Some(Some(true)));
}

#[test]
fn state_unchecked_checkbox() {
    let doc = parse(r#"<input type="checkbox">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox).expect("checkbox");
    assert_eq!(cb.state.checked, Some(Some(false)));
}

#[test]
fn state_aria_checked_mixed() {
    let doc = parse(r#"<div role="checkbox" aria-checked="mixed">Partially</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox).expect("checkbox");
    assert_eq!(cb.state.checked, Some(None));
}

#[test]
fn state_expanded_true() {
    let doc = parse(r#"<button aria-expanded="true">Menu</button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.state.expanded, Some(true));
}

#[test]
fn state_tab_index() {
    let doc = parse(r#"<div tabindex="0">Focusable</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    // There may be multiple Generic nodes (div, span, etc.); find the one with tabindex.
    let focusable = find_with_tabindex(&tree.root, 0);
    assert!(focusable.is_some(), "expected a node with tabindex=0");
}

#[test]
fn state_aria_hidden_excludes_subtree() {
    let doc = parse(r#"<div><button aria-hidden="true">Hidden</button><button>Visible</button></div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    // There should be exactly one Button in the tree (the hidden one is excluded).
    let buttons: Vec<_> = collect_role_dfs(&tree.root, AXRole::Button);
    assert_eq!(buttons.len(), 1, "aria-hidden button should be excluded from AX tree");
    assert_eq!(buttons[0].name, "Visible");
}

#[test]
fn state_live_polite() {
    let doc = parse(r#"<div aria-live="polite">Status</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let live = find_with_live(&tree.root);
    assert_eq!(live, Some(LiveRegion::Polite));
}

#[test]
fn state_level_h2() {
    let doc = parse("<h2>Chapter</h2>");
    let tree = build_ax_tree(&doc, doc.root());
    let h = find_role_dfs(&tree.root, AXRole::Heading).expect("heading");
    assert_eq!(h.state.level, Some(2));
}

#[test]
fn state_aria_level_override() {
    let doc = parse(r#"<h1 aria-level="3">Override</h1>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let h = find_role_dfs(&tree.root, AXRole::Heading).expect("heading");
    // aria-level takes priority over implicit heading level.
    assert_eq!(h.state.level, Some(3));
}

// ── Description ───────────────────────────────────────────────────────────────

#[test]
fn description_from_aria_describedby() {
    let doc = parse(r#"<div id="desc">Enter your full name</div><input aria-describedby="desc">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.description, "Enter your full name");
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Depth-first search for the first node with the given role.
fn find_role_dfs(node: &lumen_a11y::AXNode, role: AXRole) -> Option<&lumen_a11y::AXNode> {
    if node.role == role {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_role_dfs(child, role) {
            return Some(found);
        }
    }
    None
}

/// Collect all nodes with the given role (DFS).
fn collect_role_dfs(node: &lumen_a11y::AXNode, role: AXRole) -> Vec<&lumen_a11y::AXNode> {
    let mut results = Vec::new();
    collect_role_recursive(node, role, &mut results);
    results
}

fn collect_role_recursive<'a>(
    node: &'a lumen_a11y::AXNode,
    role: AXRole,
    out: &mut Vec<&'a lumen_a11y::AXNode>,
) {
    if node.role == role {
        out.push(node);
    }
    for child in &node.children {
        collect_role_recursive(child, role, out);
    }
}

fn find_with_live(node: &lumen_a11y::AXNode) -> Option<LiveRegion> {
    if node.state.live.is_some() {
        return node.state.live;
    }
    for child in &node.children {
        if let Some(lr) = find_with_live(child) {
            return Some(lr);
        }
    }
    None
}

// ── Extended HTML-AAM coverage tests ─────────────────────────────────────────────

#[test]
fn name_from_multiple_labelledby() {
    let doc = parse(
        r#"<div id="first">First</div><div id="second">Last</div><input aria-labelledby="first second">"#,
    );
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "First Last");
}

#[test]
fn description_from_title() {
    let doc = parse(r#"<button title="Close dialog">X</button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.description, "Close dialog");
}

#[test]
fn input_type_email_is_textbox() {
    let doc = parse(r#"<input type="email">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox);
    assert!(tb.is_some());
}

#[test]
fn state_readonly() {
    let doc = parse(r#"<input type="text" readonly>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert!(tb.state.readonly);
}

#[test]
fn state_invalid() {
    let doc = parse(r#"<input type="text" aria-invalid="true">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert!(tb.state.invalid);
}

#[test]
fn state_aria_busy_on_status() {
    let doc = parse(r#"<output aria-busy="true">Processing...</output>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let status = find_role_dfs(&tree.root, AXRole::Status);
    assert!(status.is_some());
    assert!(status.unwrap().state.busy);
}

// ── Extended ARIA roles (Wave 2) ─────────────────────────────────────────────

#[test]
fn role_explicit_alert() {
    let doc = parse(r#"<div role="alert">Error message</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let alert = find_role_dfs(&tree.root, AXRole::Alert);
    assert!(alert.is_some(), "expected Alert role");
}

#[test]
fn role_explicit_alertdialog() {
    let doc = parse(r#"<div role="alertdialog">Warning</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let ad = find_role_dfs(&tree.root, AXRole::AlertDialog);
    assert!(ad.is_some(), "expected AlertDialog role");
}

#[test]
fn role_explicit_feed() {
    let doc = parse(r#"<div role="feed">Social feed</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let feed = find_role_dfs(&tree.root, AXRole::Feed);
    assert!(feed.is_some(), "expected Feed role");
}

#[test]
fn role_explicit_log() {
    let doc = parse(r#"<div role="log">Log output</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let log = find_role_dfs(&tree.root, AXRole::Log);
    assert!(log.is_some(), "expected Log role");
}

#[test]
fn role_explicit_note() {
    let doc = parse(r#"<div role="note">Additional note</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let note = find_role_dfs(&tree.root, AXRole::Note);
    assert!(note.is_some(), "expected Note role");
}

#[test]
fn role_input_search() {
    let doc = parse(r#"<input type="search">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let sb = find_role_dfs(&tree.root, AXRole::Searchbox);
    assert!(sb.is_some(), "expected Searchbox role for <input type='search'>");
}

#[test]
fn role_explicit_switch() {
    let doc = parse(r#"<input type="checkbox" role="switch">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let sw = find_role_dfs(&tree.root, AXRole::Switch);
    assert!(sw.is_some(), "expected Switch role");
}

#[test]
fn role_explicit_tablist() {
    let doc = parse(r#"<div role="tablist">
        <div role="tab">Tab 1</div>
        <div role="tab">Tab 2</div>
    </div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tablist = find_role_dfs(&tree.root, AXRole::TabList);
    assert!(tablist.is_some(), "expected TabList role");
    let tabs: Vec<_> = collect_roles_dfs(&tree.root, AXRole::Tab);
    assert_eq!(tabs.len(), 2, "expected 2 Tab roles");
}

#[test]
fn role_explicit_tabpanel() {
    let doc = parse(r#"<div role="tabpanel">Content</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tp = find_role_dfs(&tree.root, AXRole::TabPanel);
    assert!(tp.is_some(), "expected TabPanel role");
}

#[test]
fn role_explicit_tree() {
    let doc = parse(r#"<div role="tree">
        <div role="treeitem">Item 1</div>
        <div role="treeitem">Item 2</div>
    </div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tree_role = find_role_dfs(&tree.root, AXRole::Tree);
    assert!(tree_role.is_some(), "expected Tree role");
    let items: Vec<_> = collect_roles_dfs(&tree.root, AXRole::TreeItem);
    assert_eq!(items.len(), 2, "expected 2 TreeItem roles");
}

#[test]
fn role_explicit_toolbar() {
    let doc = parse(r#"<div role="toolbar">Tool buttons</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::Toolbar);
    assert!(tb.is_some(), "expected Toolbar role");
}

#[test]
fn role_explicit_tooltip() {
    let doc = parse(r#"<div role="tooltip">Helpful hint</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tt = find_role_dfs(&tree.root, AXRole::Tooltip);
    assert!(tt.is_some(), "expected Tooltip role");
}

#[test]
fn role_explicit_rowheader() {
    let doc = parse(r#"<table><tr><th role="rowheader">Header</th></tr></table>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let rh = find_role_dfs(&tree.root, AXRole::RowHeader);
    assert!(rh.is_some(), "expected RowHeader role");
}

#[test]
fn role_explicit_marquee() {
    let doc = parse(r#"<div role="marquee">Scrolling text</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let mq = find_role_dfs(&tree.root, AXRole::Marquee);
    assert!(mq.is_some(), "expected Marquee role");
}

#[test]
fn role_explicit_application() {
    let doc = parse(r#"<div role="application">Web app</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let app = find_role_dfs(&tree.root, AXRole::Application);
    assert!(app.is_some(), "expected Application role");
}

#[test]
fn role_explicit_timer() {
    let doc = parse(r#"<div role="timer">1:00</div>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tm = find_role_dfs(&tree.root, AXRole::Timer);
    assert!(tm.is_some(), "expected Timer role");
}

// ── Label association tests ──────────────────────────────────────────────────

#[test]
fn label_explicit_association_via_for() {
    let doc = parse(r#"
        <label for="username">User name:</label>
        <input type="text" id="username">
    "#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "User name:", "explicit <label for> should provide name");
}

#[test]
fn label_implicit_association() {
    let doc = parse(r#"
        <label>
            Email:
            <input type="text">
        </label>
    "#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "Email:", "implicit <label> should provide name");
}

#[test]
fn label_explicit_takes_priority_over_placeholder() {
    let doc = parse(r#"
        <label for="search">Search</label>
        <input type="text" id="search" placeholder="Enter query">
    "#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "Search", "<label> should take priority over placeholder");
}

#[test]
fn label_fallback_to_placeholder_when_no_label() {
    let doc = parse(r#"<input type="text" placeholder="Enter text">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "Enter text", "placeholder should be fallback when no label");
}

// ── Description computation edge cases ──────────────────────────────────────

#[test]
fn description_title_not_duplicated_as_name() {
    let doc = parse(r#"<img src="x.png" alt="Logo" title="Company Logo">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let img = find_role_dfs(&tree.root, AXRole::Img).expect("img");
    assert_eq!(img.name, "Logo", "name from alt");
    assert_eq!(img.description, "Company Logo", "description from title");
}

#[test]
fn description_title_not_used_when_same_as_name() {
    let doc = parse(r#"<img src="x.png" alt="Logo" title="Logo">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let img = find_role_dfs(&tree.root, AXRole::Img).expect("img");
    assert_eq!(img.name, "Logo");
    assert_eq!(img.description, "", "title should not duplicate name");
}

#[test]
fn form_control_textarea_with_label() {
    let doc = parse(r#"
        <label for="msg">Message:</label>
        <textarea id="msg"></textarea>
    "#);
    let tree = build_ax_tree(&doc, doc.root());
    // textarea should map to Multiline TextBox role
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textarea as textbox");
    assert_eq!(tb.name, "Message:", "textarea should get name from label");
}

#[test]
fn form_control_select_with_label() {
    let doc = parse(r#"
        <label for="country">Country:</label>
        <select id="country">
            <option>USA</option>
        </select>
    "#);
    let tree = build_ax_tree(&doc, doc.root());
    let cb = find_role_dfs(&tree.root, AXRole::ComboBox).expect("select");
    assert_eq!(cb.name, "Country:", "select should get name from label");
}

#[test]
fn input_image_type_with_alt() {
    let doc = parse(r#"<input type="image" src="btn.png" alt="Submit form">"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("input[type=image]");
    assert_eq!(btn.name, "Submit form", "input[type=image] should use alt as name");
}

#[test]
fn button_with_icon_only() {
    let doc = parse(r#"<button><img src="close.svg" alt="Close"></button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Close", "button with only img should use img alt");
}

#[test]
fn button_with_icon_and_text() {
    let doc = parse(r#"<button><img src="save.svg" alt=""> Save</button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Save", "button with text should use text, not img");
}

// ── Serialization tests ──────────────────────────────────────────────────────

#[test]
fn tree_serialization() {
    let doc = parse(r#"
        <nav>Navigation</nav>
        <main>
            <article>Content</article>
        </main>
    "#);
    let tree = build_ax_tree(&doc, doc.root());

    // Test that the tree can be serialized with serde
    let json_str = serde_json::to_string(&tree).expect("tree serialization");
    assert!(!json_str.is_empty(), "serialization should produce non-empty JSON");

    // Test roundtrip: serialize and deserialize
    let deserialized: lumen_a11y::AXTree =
        serde_json::from_str(&json_str).expect("deserialization");
    assert_eq!(tree.root.role, deserialized.root.role, "role should match after roundtrip");
}

#[test]
fn node_name_preservation() {
    let doc = parse(r#"<button aria-label="Custom name">Default</button>"#);
    let tree = build_ax_tree(&doc, doc.root());
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Custom name", "aria-label should override button text");
}

fn find_with_tabindex(node: &lumen_a11y::AXNode, index: i32) -> Option<&lumen_a11y::AXNode> {
    if node.state.tab_index == Some(index) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_with_tabindex(child, index) {
            return Some(found);
        }
    }
    None
}

fn collect_roles_dfs(
    node: &lumen_a11y::AXNode,
    target_role: AXRole,
) -> Vec<&lumen_a11y::AXNode> {
    let mut result = Vec::new();
    if node.role == target_role {
        result.push(node);
    }
    for child in &node.children {
        result.extend(collect_roles_dfs(child, target_role));
    }
    result
}

//! Integration tests for lumen-a11y: AXTree construction from real HTML.

use lumen_a11y::{AXRole, LiveRegion, build_ax_tree};
use lumen_dom::build_flat_tree;
use lumen_html_parser::parse;

/// Helper to build AX tree with automatic FlatTree construction.
fn build_tree(html: &str) -> lumen_a11y::AXTree {
    let doc = parse(html);
    let flat_tree = build_flat_tree(&doc);
    build_ax_tree(&doc, doc.root(), &flat_tree)
}

// ── Implicit role mapping ────────────────────────────────────────────────────

#[test]
fn role_nav() {
    let tree = build_tree("<nav>Menu</nav>");
    let nav = find_role_dfs(&tree.root, AXRole::Navigation);
    assert!(nav.is_some(), "expected Navigation role for <nav>");
}

#[test]
fn role_main() {
    let tree = build_tree("<main>Content</main>");
    let m = find_role_dfs(&tree.root, AXRole::Main);
    assert!(m.is_some(), "expected Main role for <main>");
}

#[test]
fn role_headings() {
    for (tag, expected_level) in [("h1", 1u32), ("h2", 2), ("h3", 3), ("h4", 4), ("h5", 5), ("h6", 6)] {
        let tree = build_tree(&format!("<{tag}>Title</{tag}>"));
        let h = find_role_dfs(&tree.root, AXRole::Heading);
        assert!(h.is_some(), "expected Heading role for <{tag}>");
        assert_eq!(h.unwrap().state.level, Some(expected_level), "<{tag}> level");
    }
}

#[test]
fn role_link_with_href() {
    let tree = build_tree(r#"<a href="/page">Click</a>"#);
    let link = find_role_dfs(&tree.root, AXRole::Link);
    assert!(link.is_some(), "expected Link role for <a href>");
}

#[test]
fn role_a_without_href_is_generic() {
    let tree = build_tree("<a>Not a link</a>");
    let link = find_role_dfs(&tree.root, AXRole::Link);
    assert!(link.is_none(), "<a> without href should not be Link");
}

#[test]
fn role_img_with_alt() {
    let tree = build_tree(r#"<img src="x.png" alt="A photo">"#);
    let img = find_role_dfs(&tree.root, AXRole::Img);
    assert!(img.is_some(), "expected Img role for <img alt='...'>");
    assert_eq!(img.unwrap().name, "A photo");
}

#[test]
fn role_img_empty_alt_is_presentation() {
    let tree = build_tree(r#"<img src="x.png" alt="">"#);
    let pres = find_role_dfs(&tree.root, AXRole::Presentation);
    assert!(pres.is_some(), "expected Presentation role for <img alt=''>");
}

#[test]
fn role_button() {
    let tree = build_tree("<button>OK</button>");
    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_some(), "expected Button role for <button>");
    assert_eq!(btn.unwrap().name, "OK");
}

#[test]
fn role_input_checkbox() {
    let tree = build_tree(r#"<input type="checkbox">"#);
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox);
    assert!(cb.is_some(), "expected Checkbox role for <input type='checkbox'>");
}

#[test]
fn role_input_radio() {
    let tree = build_tree(r#"<input type="radio">"#);
    let r = find_role_dfs(&tree.root, AXRole::Radio);
    assert!(r.is_some(), "expected Radio role for <input type='radio'>");
}

#[test]
fn role_input_text() {
    let tree = build_tree(r#"<input type="text">"#);
    let t = find_role_dfs(&tree.root, AXRole::TextBox);
    assert!(t.is_some(), "expected TextBox for <input type='text'>");
}

#[test]
fn role_input_submit() {
    let tree = build_tree(r#"<input type="submit" value="Send">"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_some(), "expected Button for <input type='submit'>");
    assert_eq!(btn.unwrap().name, "Send");
}

#[test]
fn role_select_combobox() {
    let tree = build_tree("<select><option>A</option></select>");
    let cb = find_role_dfs(&tree.root, AXRole::ComboBox);
    assert!(cb.is_some(), "expected ComboBox for <select>");
}

#[test]
fn role_select_multiple_listbox() {
    let tree = build_tree("<select multiple><option>A</option></select>");
    let lb = find_role_dfs(&tree.root, AXRole::ListBox);
    assert!(lb.is_some(), "expected ListBox for <select multiple>");
}

#[test]
fn role_table_row_cell() {
    let tree = build_tree("<table><tr><td>Cell</td></tr></table>");
    assert!(find_role_dfs(&tree.root, AXRole::Table).is_some(), "expected Table");
    assert!(find_role_dfs(&tree.root, AXRole::Row).is_some(), "expected Row");
    assert!(find_role_dfs(&tree.root, AXRole::Cell).is_some(), "expected Cell");
}

#[test]
fn role_list_and_listitem() {
    let tree = build_tree("<ul><li>Item</li></ul>");
    assert!(find_role_dfs(&tree.root, AXRole::List).is_some(), "expected List");
    assert!(find_role_dfs(&tree.root, AXRole::ListItem).is_some(), "expected ListItem");
}

// ── Explicit role override ────────────────────────────────────────────────────

#[test]
fn explicit_role_overrides_implicit() {
    // A <div> with role="button" should become Button, not Generic.
    let tree = build_tree(r#"<div role="button">Click me</div>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_some(), "explicit role='button' should override div's Generic role");
}

#[test]
fn explicit_role_none_maps_to_none() {
    let tree = build_tree(r#"<img src="x.png" role="none">"#);
    let none = find_role_dfs(&tree.root, AXRole::None);
    assert!(none.is_some(), "role='none' should map to AXRole::None");
}

// ── Accessible name computation ───────────────────────────────────────────────

#[test]
fn name_from_aria_label() {
    let tree = build_tree(r#"<button aria-label="Close dialog">X</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Close dialog");
}

#[test]
fn name_from_aria_labelledby() {
    let tree = build_tree(r#"<div id="lbl">First name</div><input aria-labelledby="lbl">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "First name");
}

#[test]
fn name_from_text_content() {
    let tree = build_tree("<button>Submit</button>");
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Submit");
}

#[test]
fn name_from_alt() {
    let tree = build_tree(r#"<img src="x.png" alt="Company logo">"#);
    let img = find_role_dfs(&tree.root, AXRole::Img).expect("img");
    assert_eq!(img.name, "Company logo");
}

#[test]
fn name_aria_label_takes_priority_over_text() {
    let tree = build_tree(r#"<button aria-label="Close">X</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Close");
}

// ── State flags ───────────────────────────────────────────────────────────────

#[test]
fn state_disabled_from_html_attr() {
    let tree = build_tree(r#"<button disabled>Send</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert!(btn.state.disabled, "button with disabled attr should have disabled=true");
}

#[test]
fn state_aria_disabled() {
    let tree = build_tree(r#"<div role="button" aria-disabled="true">Send</div>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert!(btn.state.disabled);
}

#[test]
fn state_required_from_html_attr() {
    let tree = build_tree(r#"<input type="text" required>"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert!(tb.state.required);
}

#[test]
fn state_checked_checkbox_html() {
    let tree = build_tree(r#"<input type="checkbox" checked>"#);
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox).expect("checkbox");
    assert_eq!(cb.state.checked, Some(Some(true)));
}

#[test]
fn state_unchecked_checkbox() {
    let tree = build_tree(r#"<input type="checkbox">"#);
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox).expect("checkbox");
    assert_eq!(cb.state.checked, Some(Some(false)));
}

#[test]
fn state_aria_checked_mixed() {
    let tree = build_tree(r#"<div role="checkbox" aria-checked="mixed">Partially</div>"#);
    let cb = find_role_dfs(&tree.root, AXRole::Checkbox).expect("checkbox");
    assert_eq!(cb.state.checked, Some(None));
}

#[test]
fn state_expanded_true() {
    let tree = build_tree(r#"<button aria-expanded="true">Menu</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.state.expanded, Some(true));
}

#[test]
fn state_tab_index() {
    let tree = build_tree(r#"<div tabindex="0">Focusable</div>"#);
    // There may be multiple Generic nodes (div, span, etc.); find the one with tabindex.
    let focusable = find_with_tabindex(&tree.root, 0);
    assert!(focusable.is_some(), "expected a node with tabindex=0");
}

#[test]
fn state_aria_hidden_excludes_subtree() {
    let tree = build_tree(r#"<div><button aria-hidden="true">Hidden</button><button>Visible</button></div>"#);
    // There should be exactly one Button in the tree (the hidden one is excluded).
    let buttons: Vec<_> = collect_role_dfs(&tree.root, AXRole::Button);
    assert_eq!(buttons.len(), 1, "aria-hidden button should be excluded from AX tree");
    assert_eq!(buttons[0].name, "Visible");
}

#[test]
fn state_live_polite() {
    let tree = build_tree(r#"<div aria-live="polite">Status</div>"#);
    let live = find_with_live(&tree.root);
    assert_eq!(live, Some(LiveRegion::Polite));
}

#[test]
fn state_level_h2() {
    let tree = build_tree("<h2>Chapter</h2>");
    let h = find_role_dfs(&tree.root, AXRole::Heading).expect("heading");
    assert_eq!(h.state.level, Some(2));
}

#[test]
fn state_aria_level_override() {
    let tree = build_tree(r#"<h1 aria-level="3">Override</h1>"#);
    let h = find_role_dfs(&tree.root, AXRole::Heading).expect("heading");
    // aria-level takes priority over implicit heading level.
    assert_eq!(h.state.level, Some(3));
}

// ── Description ───────────────────────────────────────────────────────────────

#[test]
fn description_from_aria_describedby() {
    let tree = build_tree(r#"<div id="desc">Enter your full name</div><input aria-describedby="desc">"#);
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
    let tree = build_tree(
        r#"<div id="first">First</div><div id="second">Last</div><input aria-labelledby="first second">"#,
    );
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "First Last");
}

#[test]
fn description_from_title() {
    let tree = build_tree(r#"<button title="Close dialog">X</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.description, "Close dialog");
}

#[test]
fn input_type_email_is_textbox() {
    let tree = build_tree(r#"<input type="email">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox);
    assert!(tb.is_some());
}

#[test]
fn state_readonly() {
    let tree = build_tree(r#"<input type="text" readonly>"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert!(tb.state.readonly);
}

#[test]
fn state_invalid() {
    let tree = build_tree(r#"<input type="text" aria-invalid="true">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert!(tb.state.invalid);
}

#[test]
fn state_aria_busy_on_status() {
    let tree = build_tree(r#"<output aria-busy="true">Processing...</output>"#);
    let status = find_role_dfs(&tree.root, AXRole::Status);
    assert!(status.is_some());
    assert!(status.unwrap().state.busy);
}

// ── Extended ARIA roles (Wave 2) ─────────────────────────────────────────────

#[test]
fn role_explicit_alert() {
    let tree = build_tree(r#"<div role="alert">Error message</div>"#);
    let alert = find_role_dfs(&tree.root, AXRole::Alert);
    assert!(alert.is_some(), "expected Alert role");
}

#[test]
fn role_explicit_alertdialog() {
    let tree = build_tree(r#"<div role="alertdialog">Warning</div>"#);
    let ad = find_role_dfs(&tree.root, AXRole::AlertDialog);
    assert!(ad.is_some(), "expected AlertDialog role");
}

#[test]
fn role_explicit_feed() {
    let tree = build_tree(r#"<div role="feed">Social feed</div>"#);
    let feed = find_role_dfs(&tree.root, AXRole::Feed);
    assert!(feed.is_some(), "expected Feed role");
}

#[test]
fn role_explicit_log() {
    let tree = build_tree(r#"<div role="log">Log output</div>"#);
    let log = find_role_dfs(&tree.root, AXRole::Log);
    assert!(log.is_some(), "expected Log role");
}

#[test]
fn role_explicit_note() {
    let tree = build_tree(r#"<div role="note">Additional note</div>"#);
    let note = find_role_dfs(&tree.root, AXRole::Note);
    assert!(note.is_some(), "expected Note role");
}

#[test]
fn role_input_search() {
    let tree = build_tree(r#"<input type="search">"#);
    let sb = find_role_dfs(&tree.root, AXRole::Searchbox);
    assert!(sb.is_some(), "expected Searchbox role for <input type='search'>");
}

#[test]
fn role_explicit_switch() {
    let tree = build_tree(r#"<input type="checkbox" role="switch">"#);
    let sw = find_role_dfs(&tree.root, AXRole::Switch);
    assert!(sw.is_some(), "expected Switch role");
}

#[test]
fn role_explicit_tablist() {
    let tree = build_tree(r#"<div role="tablist">
        <div role="tab">Tab 1</div>
        <div role="tab">Tab 2</div>
    </div>"#);
    let tablist = find_role_dfs(&tree.root, AXRole::TabList);
    assert!(tablist.is_some(), "expected TabList role");
    let tabs: Vec<_> = collect_roles_dfs(&tree.root, AXRole::Tab);
    assert_eq!(tabs.len(), 2, "expected 2 Tab roles");
}

#[test]
fn role_explicit_tabpanel() {
    let tree = build_tree(r#"<div role="tabpanel">Content</div>"#);
    let tp = find_role_dfs(&tree.root, AXRole::TabPanel);
    assert!(tp.is_some(), "expected TabPanel role");
}

#[test]
fn role_explicit_tree() {
    let tree = build_tree(r#"<div role="tree">
        <div role="treeitem">Item 1</div>
        <div role="treeitem">Item 2</div>
    </div>"#);
    let tree_role = find_role_dfs(&tree.root, AXRole::Tree);
    assert!(tree_role.is_some(), "expected Tree role");
    let items: Vec<_> = collect_roles_dfs(&tree.root, AXRole::TreeItem);
    assert_eq!(items.len(), 2, "expected 2 TreeItem roles");
}

#[test]
fn role_explicit_toolbar() {
    let tree = build_tree(r#"<div role="toolbar">Tool buttons</div>"#);
    let tb = find_role_dfs(&tree.root, AXRole::Toolbar);
    assert!(tb.is_some(), "expected Toolbar role");
}

#[test]
fn role_explicit_tooltip() {
    let tree = build_tree(r#"<div role="tooltip">Helpful hint</div>"#);
    let tt = find_role_dfs(&tree.root, AXRole::Tooltip);
    assert!(tt.is_some(), "expected Tooltip role");
}

#[test]
fn role_explicit_rowheader() {
    let tree = build_tree(r#"<table><tr><th role="rowheader">Header</th></tr></table>"#);
    let rh = find_role_dfs(&tree.root, AXRole::RowHeader);
    assert!(rh.is_some(), "expected RowHeader role");
}

#[test]
fn role_explicit_marquee() {
    let tree = build_tree(r#"<div role="marquee">Scrolling text</div>"#);
    let mq = find_role_dfs(&tree.root, AXRole::Marquee);
    assert!(mq.is_some(), "expected Marquee role");
}

#[test]
fn role_explicit_application() {
    let tree = build_tree(r#"<div role="application">Web app</div>"#);
    let app = find_role_dfs(&tree.root, AXRole::Application);
    assert!(app.is_some(), "expected Application role");
}

#[test]
fn role_explicit_timer() {
    let tree = build_tree(r#"<div role="timer">1:00</div>"#);
    let tm = find_role_dfs(&tree.root, AXRole::Timer);
    assert!(tm.is_some(), "expected Timer role");
}

// ── Label association tests ──────────────────────────────────────────────────

#[test]
fn label_explicit_association_via_for() {
    let tree = build_tree(r#"
        <label for="username">User name:</label>
        <input type="text" id="username">
    "#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "User name:", "explicit <label for> should provide name");
}

#[test]
fn label_implicit_association() {
    let tree = build_tree(r#"
        <label>
            Email:
            <input type="text">
        </label>
    "#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "Email:", "implicit <label> should provide name");
}

#[test]
fn label_explicit_takes_priority_over_placeholder() {
    let tree = build_tree(r#"
        <label for="search">Search</label>
        <input type="text" id="search" placeholder="Enter query">
    "#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "Search", "<label> should take priority over placeholder");
}

#[test]
fn label_fallback_to_placeholder_when_no_label() {
    let tree = build_tree(r#"<input type="text" placeholder="Enter text">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    assert_eq!(tb.name, "Enter text", "placeholder should be fallback when no label");
}

// ── Description computation edge cases ──────────────────────────────────────

#[test]
fn description_title_not_duplicated_as_name() {
    let tree = build_tree(r#"<img src="x.png" alt="Logo" title="Company Logo">"#);
    let img = find_role_dfs(&tree.root, AXRole::Img).expect("img");
    assert_eq!(img.name, "Logo", "name from alt");
    assert_eq!(img.description, "Company Logo", "description from title");
}

#[test]
fn description_title_not_used_when_same_as_name() {
    let tree = build_tree(r#"<img src="x.png" alt="Logo" title="Logo">"#);
    let img = find_role_dfs(&tree.root, AXRole::Img).expect("img");
    assert_eq!(img.name, "Logo");
    assert_eq!(img.description, "", "title should not duplicate name");
}

#[test]
fn form_control_textarea_with_label() {
    let tree = build_tree(r#"
        <label for="msg">Message:</label>
        <textarea id="msg"></textarea>
    "#);
    // textarea should map to Multiline TextBox role
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textarea as textbox");
    assert_eq!(tb.name, "Message:", "textarea should get name from label");
}

#[test]
fn form_control_select_with_label() {
    let tree = build_tree(r#"
        <label for="country">Country:</label>
        <select id="country">
            <option>USA</option>
        </select>
    "#);
    let cb = find_role_dfs(&tree.root, AXRole::ComboBox).expect("select");
    assert_eq!(cb.name, "Country:", "select should get name from label");
}

#[test]
fn input_image_type_with_alt() {
    let tree = build_tree(r#"<input type="image" src="btn.png" alt="Submit form">"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("input[type=image]");
    assert_eq!(btn.name, "Submit form", "input[type=image] should use alt as name");
}

#[test]
fn button_with_icon_only() {
    let tree = build_tree(r#"<button><img src="close.svg" alt="Close"></button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Close", "button with only img should use img alt");
}

#[test]
fn button_with_icon_and_text() {
    let tree = build_tree(r#"<button><img src="save.svg" alt=""> Save</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Save", "button with text should use text, not img");
}

#[test]
fn link_text_from_content() {
    let tree = build_tree(r#"<a href="/page">Read more</a>"#);
    let link = find_role_dfs(&tree.root, AXRole::Link).expect("link");
    assert_eq!(link.name, "Read more", "link should use text content");
}

#[test]
fn link_empty_href_not_link_role() {
    let tree = build_tree("<a>Not a link</a>");
    let link = find_role_dfs(&tree.root, AXRole::Link);
    assert!(link.is_none(), "link without href should not be Link role");
}

#[test]
fn heading_text_from_content() {
    let tree = build_tree("<h1>Main Title</h1>");
    let heading = find_role_dfs(&tree.root, AXRole::Heading).expect("heading");
    assert_eq!(heading.name, "Main Title", "heading should use text content");
}

#[test]
fn summary_disclosure_widget() {
    let tree = build_tree(r#"
        <details>
            <summary>Click to expand</summary>
            <p>Hidden content</p>
        </details>
    "#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("summary as button");
    assert_eq!(btn.name, "Click to expand", "summary should use text content");
}

// ── Serialization tests ──────────────────────────────────────────────────────

#[test]
fn tree_serialization() {
    let tree = build_tree(r#"
        <nav>Navigation</nav>
        <main>
            <article>Content</article>
        </main>
    "#);

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
    let tree = build_tree(r#"<button aria-label="Custom name">Default</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.name, "Custom name", "aria-label should override button text");
}

// ── Stage 3: ARIA attribute application ──────────────────────────────────────

#[test]
fn aria_current_page() {
    let tree = build_tree(r#"<a href="/" aria-current="page">Home</a>"#);
    let link = find_role_dfs(&tree.root, AXRole::Link).expect("link");
    assert_eq!(link.state.current, Some(lumen_a11y::AriaCurrent::Page), "aria-current=page");
}

#[test]
fn aria_current_step() {
    let tree = build_tree(r#"<a href="/step2" aria-current="step">Step 2</a>"#);
    let node = find_role_dfs(&tree.root, AXRole::Link).expect("link");
    assert_eq!(node.state.current, Some(lumen_a11y::AriaCurrent::Step), "aria-current=step");
}

#[test]
fn aria_current_location() {
    let tree = build_tree("<a href=\"#section\" aria-current=\"location\">Section</a>");
    let link = find_role_dfs(&tree.root, AXRole::Link).expect("link");
    assert_eq!(
        link.state.current,
        Some(lumen_a11y::AriaCurrent::Location),
        "aria-current=location"
    );
}

#[test]
fn aria_current_date() {
    let tree = build_tree(r#"<span role="button" aria-current="date">May 27</span>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.state.current, Some(lumen_a11y::AriaCurrent::Date), "aria-current=date");
}

#[test]
fn aria_current_true_maps_to_page() {
    let tree = build_tree(r#"<a href="/" aria-current="true">Home</a>"#);
    let link = find_role_dfs(&tree.root, AXRole::Link).expect("link");
    assert_eq!(
        link.state.current,
        Some(lumen_a11y::AriaCurrent::Page),
        "aria-current=true maps to page"
    );
}

#[test]
fn aria_modal() {
    let tree = build_tree(r#"<div role="dialog" aria-modal="true">Modal</div>"#);
    let dialog = find_role_dfs(&tree.root, AXRole::Dialog).expect("dialog");
    assert!(dialog.state.modal, "aria-modal should be true");
}

#[test]
fn aria_roledescription() {
    let tree = build_tree(r#"<div role="button" aria-roledescription="play button">Play</div>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.state.role_description, "play button", "aria-roledescription");
}

#[test]
fn aria_valuenow_valuemin_valuemax() {
    let tree = build_tree(r#"<input type="range" aria-valuenow="50" aria-valuemin="0" aria-valuemax="100">"#);
    let slider = find_role_dfs(&tree.root, AXRole::Slider).expect("slider");
    assert_eq!(slider.state.value_now, "50", "aria-valuenow");
    assert_eq!(slider.state.value_min, "0", "aria-valuemin");
    assert_eq!(slider.state.value_max, "100", "aria-valuemax");
}

#[test]
fn aria_valuetext() {
    let tree = build_tree(r#"<div role="slider" aria-valuetext="50 degrees">Temperature</div>"#);
    let slider = find_role_dfs(&tree.root, AXRole::Slider).expect("slider");
    assert_eq!(slider.state.value_text, "50 degrees", "aria-valuetext");
}

// ── Stage 3: Computed role mapping ───────────────────────────────────────────

#[test]
fn role_row_requires_table_context() {
    let tree = build_tree(r#"
        <table>
            <tr><td>Cell</td></tr>
        </table>
    "#);
    let row = find_role_dfs(&tree.root, AXRole::Row).expect("row in table");
    assert_eq!(row.role, AXRole::Row, "row should be valid inside table");
}

#[test]
fn role_cell_requires_row_context() {
    let tree = build_tree(r#"
        <table>
            <tr><td>Cell</td></tr>
        </table>
    "#);
    let cell = find_role_dfs(&tree.root, AXRole::Cell).expect("cell in row");
    assert_eq!(cell.role, AXRole::Cell, "cell should be valid inside row");
}

#[test]
fn role_listitem_requires_list_context() {
    let tree = build_tree(r#"
        <ul>
            <li>Item 1</li>
        </ul>
    "#);
    let item = find_role_dfs(&tree.root, AXRole::ListItem).expect("list item");
    assert_eq!(item.role, AXRole::ListItem, "listitem should be valid inside list");
}

#[test]
fn role_tab_requires_tablist_context() {
    let tree = build_tree(r#"
        <div role="tablist">
            <button role="tab">Tab 1</button>
        </div>
    "#);
    let tab = find_role_dfs(&tree.root, AXRole::Tab).expect("tab");
    assert_eq!(tab.role, AXRole::Tab, "tab should be valid inside tablist");
}

#[test]
fn role_option_requires_listbox_context() {
    let tree = build_tree(r#"
        <div role="listbox">
            <div role="option">Option 1</div>
        </div>
    "#);
    let option = find_role_dfs(&tree.root, AXRole::Option).expect("option");
    assert_eq!(option.role, AXRole::Option, "option should be valid inside listbox");
}

#[test]
fn role_treeitem_requires_tree_context() {
    let tree = build_tree(r#"
        <div role="tree">
            <div role="treeitem">Item 1</div>
        </div>
    "#);
    let item = find_role_dfs(&tree.root, AXRole::TreeItem).expect("treeitem");
    assert_eq!(item.role, AXRole::TreeItem, "treeitem should be valid inside tree");
}

#[test]
fn invalid_role_falls_back_to_implicit() {
    // role="row" outside of table context should fall back to implicit role
    let tree = build_tree(r#"<div role="row">Not in table</div>"#);
    let node = find_role_dfs(&tree.root, AXRole::Row);
    // Should fall back to implicit role (Generic) instead of Row
    assert!(node.is_none(), "row outside table should not have Row role");
}

#[test]
fn menuitem_requires_menu_context() {
    let tree = build_tree(r#"
        <div role="menu">
            <div role="menuitem">Item 1</div>
        </div>
    "#);
    let item = find_role_dfs(&tree.root, AXRole::MenuItem).expect("menuitem");
    assert_eq!(item.role, AXRole::MenuItem, "menuitem should be valid inside menu");
}

// ── Stage 3: Relationship attributes ──────────────────────────────────────────

#[test]
fn relationship_attributes_initialized() {
    // Verify that relationship attributes (aria-controls, aria-owns, aria-flowto, aria-details)
    // are now resolved via Document::find_by_id()
    let tree = build_tree(r#"
        <button aria-controls="panel" aria-owns="owned1 owned2">Button</button>
        <div id="panel">Panel</div>
        <div id="owned1">O1</div>
        <div id="owned2">O2</div>
    "#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");

    // Relationship attributes should now be resolved
    assert!(btn.controls.is_some(), "aria-controls should resolve to panel's NodeId");
    assert_eq!(btn.owns.len(), 2, "aria-owns should contain 2 NodeIds");
    assert!(btn.flow_to.is_empty(), "aria-flowto should be empty when not specified");
    assert!(btn.details.is_none(), "aria-details should be None when not specified");
}

#[test]
fn aria_controls_attribute_present() {
    let tree = build_tree(r#"<button aria-controls="panel">Open Panel</button><div id="panel">Panel</div>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert!(btn.controls.is_some(), "aria-controls should resolve to panel's NodeId");
    // Verify the resolved NodeId matches the panel element
    let panel_id = btn.controls.expect("panel should be found");
    let panel_node_id = panel_id;
    assert_ne!(panel_node_id, btn.node_id, "panel should be a different node");
}

#[test]
fn aria_owns_attribute_present() {
    let tree = build_tree(
        r#"<div role="group" aria-owns="child1 child2">Group <div id="child1">C1</div><div id="child2">C2</div></div>"#,
    );
    let group = find_role_dfs(&tree.root, AXRole::Group).expect("group");
    assert_eq!(group.owns.len(), 2, "aria-owns should contain 2 NodeIds");
    assert_ne!(group.owns[0], group.node_id, "owned child 1 should be different from group");
    assert_ne!(group.owns[1], group.node_id, "owned child 2 should be different from group");
}

#[test]
fn aria_flowto_attribute_present() {
    let tree = build_tree(
        r#"<span aria-flowto="next">First</span><span id="next">Second</span>"#,
    );
    let first = find_role_dfs(&tree.root, AXRole::Generic).expect("first span");
    // TODO: Once Document::find_by_id() is implemented, should resolve to next span's NodeId
    assert!(first.flow_to.is_empty(), "aria-flowto resolution pending Document API");
}

#[test]
fn aria_details_attribute_present() {
    let tree = build_tree(
        r#"<input type="password" aria-details="pwd-hint"><div id="pwd-hint">Must be 8+ chars</div>"#,
    );
    let input = find_role_dfs(&tree.root, AXRole::TextBox).expect("password input");
    assert!(input.details.is_some(), "aria-details should resolve to pwd-hint's NodeId");
    assert_ne!(input.details.expect("details"), input.node_id, "details should be a different node");
}

#[test]
fn columnheader_requires_row_context() {
    let tree = build_tree(r#"
        <table>
            <tr>
                <th>Name</th>
                <th>Age</th>
            </tr>
        </table>
    "#);
    let header = find_role_dfs(&tree.root, AXRole::ColumnHeader).expect("columnheader");
    assert_eq!(header.role, AXRole::ColumnHeader, "columnheader valid in row");
}

#[test]
fn rowheader_or_columnheader_in_table() {
    let tree = build_tree(r#"
        <table>
            <tr>
                <th scope="row">Item</th>
                <td>Data</td>
            </tr>
        </table>
    "#);
    // <th> becomes either RowHeader or ColumnHeader depending on scope/position
    let headers = collect_roles_dfs(&tree.root, AXRole::RowHeader);
    let col_headers = collect_roles_dfs(&tree.root, AXRole::ColumnHeader);
    assert!(
        !headers.is_empty() || !col_headers.is_empty(),
        "table header should be rowheader or columnheader"
    );
}

#[test]
fn aria_current_on_non_link() {
    // aria-current should work on any element, not just links
    let tree = build_tree(r#"<button aria-current="page">Current Page</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.state.current, Some(lumen_a11y::AriaCurrent::Page), "aria-current on button");
}

#[test]
fn role_attributes_with_empty_string_ignored() {
    // Empty role attribute should not affect implicit role
    let tree = build_tree(r#"<button role="">Click</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert_eq!(btn.role, AXRole::Button, "button role preserved with empty role attr");
}

#[test]
fn explicit_role_none_semantics() {
    // role="none" / role="presentation" removes semantics
    let tree = build_tree(r#"<button role="none">Not a button</button>"#);
    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_none(), "role=none overrides button implicit role");
    let generic = find_role_dfs(&tree.root, AXRole::Generic);
    // Should have some role, but not Button
    assert!(generic.is_some() || find_role_dfs(&tree.root, AXRole::None).is_some());
}

#[test]
fn nested_table_row_is_valid() {
    // Row in nested table should still be valid
    let tree = build_tree(r#"
        <table>
            <tr>
                <td>
                    <table>
                        <tr><td>Nested</td></tr>
                    </table>
                </td>
            </tr>
        </table>
    "#);
    let rows = collect_roles_dfs(&tree.root, AXRole::Row);
    assert_eq!(rows.len(), 2, "both outer and nested rows should exist");
}

#[test]
fn aria_current_false_not_present() {
    // aria-current="false" should result in None, not Some(false)
    let tree = build_tree(r#"<a href="/" aria-current="false">Not Current</a>"#);
    let link = find_role_dfs(&tree.root, AXRole::Link).expect("link");
    assert!(link.state.current.is_none(), "aria-current=false should be None");
}

// ── Phase 2B: Shadow DOM + Accessibility Tree ────────────────────────────────
// These tests verify that build_ax_tree() correctly uses FlatTree to traverse
// shadow DOM boundaries and properly handle slot assignments.

#[test]
fn ax_tree_uses_flat_tree_for_traversal() {
    // Verify that build_ax_tree() accepts and uses FlatTree parameter.
    // This test ensures the signature change and FlatTree integration is working.
    let tree = build_tree("<nav>Navigation</nav>");
    let nav = find_role_dfs(&tree.root, AXRole::Navigation);
    assert!(nav.is_some(), "accessibility tree should traverse via FlatTree");
}

#[test]
fn aria_relationship_resolution_via_find_by_id() {
    // Verify that aria-* relationships are resolved using Document::find_by_id()
    let tree = build_tree(r#"
        <button id="btn" aria-controls="panel1" aria-owns="item1 item2">
            Open
        </button>
        <div id="panel1">Panel content</div>
        <div id="item1">Item 1</div>
        <div id="item2">Item 2</div>
    "#);

    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button");
    assert!(
        btn.controls.is_some(),
        "aria-controls should be resolved to panel NodeId"
    );
    assert_eq!(btn.owns.len(), 2, "aria-owns should contain both owned NodeIds");
}

#[test]
fn aria_flowto_resolution() {
    // Test aria-flowto attribute resolution
    let tree = build_tree(r#"
        <div id="first">First section</div>
        <div id="second" aria-flowto="third">Second section</div>
        <div id="third">Third section</div>
    "#);

    // Find the second div
    let mut found_second = false;
    for node in collect_roles_dfs(&tree.root, AXRole::Generic) {
        if node.name.contains("Second") {
            found_second = true;
            assert_eq!(node.flow_to.len(), 1, "aria-flowto should resolve to one NodeId");
            break;
        }
    }
    assert!(found_second, "should find second div with aria-flowto");
}

#[test]
fn aria_details_resolution() {
    // Test aria-details attribute resolution
    let tree = build_tree(r#"
        <input type="password" id="pwd" aria-details="hint">
        <div id="hint">Password must be at least 8 characters</div>
    "#);

    let input = find_role_dfs(&tree.root, AXRole::TextBox).expect("password input");
    assert!(
        input.details.is_some(),
        "aria-details should resolve to hint NodeId"
    );
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

// ── Phase 2D: Presentational Roles, Slot Forwarding, Complex Nesting ────────
// These tests verify handling of transparent roles (presentation, none, generic, group),
// nested slot-like structures, and complex shadow nesting scenarios.

#[test]
fn role_presentation_collapses_subtree() {
    let tree = build_tree(r#"
        <div role="presentation">
            <button>Button inside presentation</button>
        </div>
    "#);

    let btn = find_role_dfs(&tree.root, AXRole::Button).expect("button should exist inside presentation");
    assert_eq!(btn.name, "Button inside presentation");
}

#[test]
fn role_none_behaves_like_presentation() {
    let tree = build_tree(r#"
        <div role="none">
            <div role="menu">
                <div role="menuitem">Item</div>
            </div>
        </div>
    "#);

    let menu = find_role_dfs(&tree.root, AXRole::Menu);
    assert!(menu.is_some(), "menu should be visible despite role='none' wrapper");
}

#[test]
fn presentational_role_inheritance_in_nesting() {
    let tree = build_tree(r#"
        <div role="tablist">
            <div role="presentation">
                <div role="tab">Tab 1</div>
            </div>
        </div>
    "#);

    let tab = find_role_dfs(&tree.root, AXRole::Tab);
    assert!(tab.is_some(), "tab should be valid even with presentation ancestor");
}

#[test]
fn nested_slot_like_structure_with_group() {
    let tree = build_tree(r#"
        <div role="group">
            <div role="presentation">
                <div role="group">
                    <button>Nested button</button>
                </div>
            </div>
        </div>
    "#);

    let btn = find_role_dfs(&tree.root, AXRole::Button);
    assert!(btn.is_some(), "button should be accessible in nested groups");
}

#[test]
fn complex_shadow_like_nested_lists() {
    let tree = build_tree(r#"
        <div role="list">
            <div role="presentation">
                <div role="listitem">Item 1</div>
                <div role="presentation">
                    <div role="listitem">Item 2</div>
                </div>
            </div>
        </div>
    "#);

    let list = find_role_dfs(&tree.root, AXRole::List).expect("list");
    let items = collect_roles_dfs(list, AXRole::ListItem);
    assert_eq!(items.len(), 2, "both items should be valid despite presentation wrappers");
}

#[test]
fn role_validation_across_presentation_boundaries() {
    let tree = build_tree(r#"
        <div role="menu">
            <div role="presentation">
                <div role="menuitem">Cut</div>
                <div role="menuitem">Copy</div>
            </div>
        </div>
    "#);

    let menu = find_role_dfs(&tree.root, AXRole::Menu).expect("menu");
    let items = collect_roles_dfs(menu, AXRole::MenuItem);
    assert_eq!(items.len(), 2, "menuitems should be valid across presentation");
}

#[test]
fn multiple_slot_like_distribution() {
    let tree = build_tree(r#"
        <div role="tablist">
            <div class="tabs">
                <div role="tab">Tab 1</div>
                <div role="tab">Tab 2</div>
            </div>
            <div class="panels">
                <div role="tabpanel">Panel 1</div>
                <div role="tabpanel">Panel 2</div>
            </div>
        </div>
    "#);

    let tablist = find_role_dfs(&tree.root, AXRole::TabList).expect("tablist");
    let tabs = collect_roles_dfs(tablist, AXRole::Tab);
    let panels = collect_roles_dfs(tablist, AXRole::TabPanel);

    assert_eq!(tabs.len(), 2, "should have 2 tabs");
    assert_eq!(panels.len(), 2, "should have 2 tab panels");
}

#[test]
fn nested_shadow_boundaries_with_form_controls() {
    let tree = build_tree(r#"
        <fieldset>
            <legend>Settings</legend>
            <div role="presentation">
                <div role="group">
                    <label>Option 1<input type="checkbox"></label>
                </div>
                <div role="group">
                    <label>Option 2<input type="checkbox"></label>
                </div>
            </div>
        </fieldset>
    "#);

    let checkboxes = collect_roles_dfs(&tree.root, AXRole::Checkbox);
    assert_eq!(checkboxes.len(), 2, "both checkboxes should be accessible");
}

#[test]
fn complex_deeply_nested_shadow_structure() {
    let tree = build_tree(r#"
        <div role="menu">
            <div role="presentation">
                <div role="group">
                    <div role="presentation">
                        <div role="menuitem">Item 1</div>
                        <div role="menuitem">Item 2</div>
                    </div>
                </div>
            </div>
        </div>
    "#);

    let menu = find_role_dfs(&tree.root, AXRole::Menu).expect("menu");
    let items = collect_roles_dfs(menu, AXRole::MenuItem);
    assert_eq!(items.len(), 2, "menuitems should be valid in deeply nested structure");
}

// ── Constraint validation + accessibility ────────────────────────────────────

#[test]
fn state_invalid_required_missing_value() {
    let tree = build_tree(r#"<input type="text" required>"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // Empty required input fails constraint validation → invalid state
    assert!(tb.state.invalid, "required empty input should be marked invalid");
}

#[test]
fn state_invalid_email_malformed() {
    let tree = build_tree(r#"<input type="email" value="not-an-email">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // type=email with syntactically wrong value fails constraint validation → invalid
    assert!(tb.state.invalid, "email input with malformed value should be invalid");
}

#[test]
fn state_valid_required_with_value() {
    let tree = build_tree(r#"<input type="text" required value="hello">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // Required input with non-empty value passes validation → not invalid
    assert!(!tb.state.invalid, "required input with value should be valid");
}

#[test]
fn state_valid_email_correct() {
    let tree = build_tree(r#"<input type="email" value="user@example.com">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // type=email with correct format passes validation → not invalid
    assert!(!tb.state.invalid, "email input with valid format should not be invalid");
}

#[test]
fn state_invalid_minlength_underflow() {
    let tree = build_tree(r#"<input type="text" minlength="5" value="hi">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // Value shorter than minlength fails constraint validation → invalid
    assert!(tb.state.invalid, "input with value shorter than minlength should be invalid");
}

#[test]
fn state_invalid_maxlength_overflow() {
    let tree = build_tree(r#"<input type="text" maxlength="3" value="hello">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // Value longer than maxlength fails constraint validation → invalid
    assert!(tb.state.invalid, "input with value longer than maxlength should be invalid");
}

#[test]
fn state_aria_invalid_overrides() {
    let tree = build_tree(r#"<input type="text" value="valid-content" aria-invalid="true">"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // aria-invalid="true" is always marked invalid, regardless of actual validity
    assert!(tb.state.invalid, "aria-invalid='true' should override validity");
}

#[test]
fn state_required_and_invalid() {
    let tree = build_tree(r#"<input type="text" required>"#);
    let tb = find_role_dfs(&tree.root, AXRole::TextBox).expect("textbox");
    // Required flag should be set regardless of validity
    assert!(tb.state.required, "required attribute should set required state");
    assert!(tb.state.invalid, "required empty input should be invalid");
}

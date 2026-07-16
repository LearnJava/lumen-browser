//! DEVX-2: non-pixel golden regression layer on the driver API.
//!
//! Modeled on `graphic_tests` but asserted through `BrowserSession` instead of
//! pixel diffing (`layout_box_by_selector`, `computed_style_snapshot`,
//! `query_a11y`), so it runs without GPU/Edge via `cargo test -p lumen-driver`.
//! Fixtures are small self-contained pages in `tests/fixtures/`, each isolating
//! one concern: container geometry, cascade/specificity, and a11y roles of
//! form controls.

use lumen_driver::{AxQuery, BrowserSession, InProcessSession};
use lumen_layout::style::FontWeight;

fn navigate_fixture(session: &mut InProcessSession, file: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(file);
    session
        .navigate(&format!("file://{}", path.display()))
        .expect("navigate");
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0
}

// ── golden-containers.html: nested block + flex geometry ────────────────────

#[test]
fn golden_containers_block_and_flex_geometry() {
    let mut session = InProcessSession::new();
    navigate_fixture(&mut session, "golden-containers.html");

    let outer = session
        .layout_box_by_selector(".outer")
        .expect("query .outer")
        .expect(".outer not found");
    assert!(
        approx(outer.border_box.width, 324.0),
        "outer border-box width should be 300 content + 20 padding + 4 border = 324, got {}",
        outer.border_box.width
    );

    let inner = session
        .layout_box_by_selector(".inner-a")
        .expect("query .inner-a")
        .expect(".inner-a not found");
    assert!(
        approx(inner.border_box.width, 100.0) && approx(inner.border_box.height, 50.0),
        "inner-a should stay 100x50, got {}x{}",
        inner.border_box.width,
        inner.border_box.height
    );

    let flex_row = session
        .layout_box_by_selector(".flex-row")
        .expect("query .flex-row")
        .expect(".flex-row not found");
    assert!(
        approx(flex_row.border_box.width, 300.0),
        "flex-row should be 300 wide, got {}",
        flex_row.border_box.width
    );

    let item_a = session
        .layout_box_by_selector(".item-a")
        .expect("query .item-a")
        .expect(".item-a not found");
    let item_b = session
        .layout_box_by_selector(".item-b")
        .expect("query .item-b")
        .expect(".item-b not found");
    for (name, item) in [("item-a", &item_a), ("item-b", &item_b)] {
        assert!(
            approx(item.border_box.width, 60.0) && approx(item.border_box.height, 40.0),
            "{name} should be 60x40, got {}x{}",
            item.border_box.width,
            item.border_box.height
        );
    }
    eprintln!("DEBUG a.x={} a.w={} b.x={} b.w={}", item_a.border_box.x, item_a.border_box.width, item_b.border_box.x, item_b.border_box.width);
    let gap = item_b.border_box.x - (item_a.border_box.x + item_a.border_box.width);
    assert!(
        approx(gap, 10.0),
        "item-b should sit 10px (margin-left) after item-a, got gap {gap}"
    );
}

// ── golden-cascade.html: specificity, !important, inheritance ───────────────

#[test]
fn golden_cascade_specificity_and_inheritance() {
    let mut session = InProcessSession::new();
    navigate_fixture(&mut session, "golden-cascade.html");

    let tag_wins = session
        .computed_style_snapshot(".tag-wins")
        .expect("style")
        .expect(".tag-wins not found");
    assert_eq!(
        (tag_wins.color.r, tag_wins.color.g, tag_wins.color.b),
        (0, 0, 255),
        "tag selector `p {{ color: blue }}` should override inherited body color"
    );

    let class_wins = session
        .computed_style_snapshot(".note")
        .expect("style")
        .expect(".note not found");
    assert_eq!(
        (class_wins.color.r, class_wins.color.g, class_wins.color.b),
        (0, 128, 0),
        "class selector (0,1,0) should beat tag selector (0,0,1)"
    );

    let id_wins = session
        .computed_style_snapshot("#warn")
        .expect("style")
        .expect("#warn not found");
    assert_eq!(
        (id_wins.color.r, id_wins.color.g, id_wins.color.b),
        (255, 0, 0),
        "id selector (1,0,0) should beat class selector (0,1,0)"
    );

    let important_wins = session
        .computed_style_snapshot(".force-orange")
        .expect("style")
        .expect(".force-orange not found");
    assert_eq!(
        (important_wins.color.r, important_wins.color.g, important_wins.color.b),
        (255, 165, 0),
        "!important class rule should beat an inline style declaration"
    );

    let child = session
        .computed_style_snapshot(".child")
        .expect("style")
        .expect(".child not found");
    assert_eq!(
        child.font_weight,
        FontWeight::BOLD,
        "div with no own font-weight should inherit bold from its .bold-group ancestor"
    );
}

// ── golden-form-a11y.html: accessible roles/names of form controls ──────────

#[test]
fn golden_form_controls_accessible_roles() {
    let mut session = InProcessSession::new();
    navigate_fixture(&mut session, "golden-form-a11y.html");

    let name_field = session
        .query_a11y(&AxQuery::Role { role: "textbox".into(), name: Some("Name".into()) })
        .expect("query_a11y failed")
        .expect("expected textbox labelled 'Name' via <label for>");
    assert_eq!(name_field.placeholder, "Jane Doe");

    let agree = session
        .query_a11y(&AxQuery::Role { role: "checkbox".into(), name: Some("agree".into()) })
        .expect("query_a11y failed")
        .expect("expected checkbox labelled via implicit <label> wrapping");
    assert_eq!(agree.state.checked, Some(Some(true)));

    let radios = session
        .query_a11y_all(&AxQuery::Role { role: "radio".into(), name: None })
        .expect("query_a11y_all failed");
    assert_eq!(radios.len(), 2, "expected two radio buttons in the 'plan' group");

    let plan_a = session
        .query_a11y(&AxQuery::Role { role: "radio".into(), name: Some("Plan A".into()) })
        .expect("query_a11y failed")
        .expect("expected radio labelled 'Plan A'");
    assert_eq!(plan_a.state.checked, Some(Some(true)));

    session
        .query_a11y(&AxQuery::Role { role: "combobox".into(), name: None })
        .expect("query_a11y failed")
        .expect("expected <select> to expose combobox role (no multiple/size>1)");

    let comments = session
        .query_a11y(&AxQuery::Role { role: "textbox".into(), name: None })
        .expect("query_a11y failed");
    assert!(
        comments.is_some(),
        "expected at least one textbox (name-field or textarea)"
    );
    let textarea = session
        .query_a11y_all(&AxQuery::Role { role: "textbox".into(), name: None })
        .expect("query_a11y_all failed")
        .into_iter()
        .find(|n| n.placeholder == "Comments")
        .expect("expected the <textarea> to expose role=textbox with its placeholder");
    assert_eq!(textarea.placeholder, "Comments");

    let send = session
        .query_a11y(&AxQuery::Role { role: "button".into(), name: Some("Send".into()) })
        .expect("query_a11y failed");
    assert!(send.is_some(), "expected a submit button named 'Send'");
}

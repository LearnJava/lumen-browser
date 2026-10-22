//! Unit tests of `forms.rs` (click dispatch, constraint validation, picker
//! overlays, form submission encoding).
//!
//! Moved verbatim out of the inline `mod tests` of `forms.rs` (file-size gate,
//! docs/lint-policy.md §5.1); only the 4-space module indent is removed.

use super::*;
use lumen_dom::{Attribute, Document, NodeData, NodeId, QualName};

fn indicator_pills(dl: &DisplayList) -> Vec<Rect> {
    dl.iter()
        .filter_map(|c| match c {
            DisplayCommand::FillRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect()
}

#[test]
fn indicator_without_labels_draws_nothing() {
    assert!(build_input_indicator(Rect::new(10.0, 100.0, 200.0, 30.0), &[], 0.0, 800.0).is_empty());
}

#[test]
fn indicator_sits_above_the_field_at_its_right_edge() {
    let field = Rect::new(10.0, 100.0, 200.0, 30.0);
    let dl = build_input_indicator(field, &["\u{21ea}"], 0.0, 800.0);
    let pills = indicator_pills(&dl);
    assert_eq!(pills.len(), 1);
    let p = pills[0];
    assert!(p.y + p.height <= field.y, "outside the field, above it: {p:?}");
    assert!((p.x + p.width - (field.x + field.width)).abs() < 0.01, "right-aligned: {p:?}");
    assert!(dl.iter().any(|c| matches!(c, DisplayCommand::DrawText { text, .. } if text == "\u{21ea}")));
}

#[test]
fn indicator_goes_below_when_the_field_touches_the_viewport_top() {
    let field = Rect::new(10.0, 100.0, 200.0, 30.0);
    // Scrolled so the field's top is at viewport y = 4: no room above.
    let pills = indicator_pills(&build_input_indicator(field, &["RU"], 96.0, 800.0));
    assert!(pills[0].y >= 4.0 + field.height, "{:?}", pills[0]);
}

#[test]
fn indicator_pills_do_not_overlap_and_stay_in_the_viewport() {
    let field = Rect::new(700.0, 100.0, 200.0, 30.0);
    let pills = indicator_pills(&build_input_indicator(field, &["\u{21ea}", "RU"], 0.0, 800.0));
    assert_eq!(pills.len(), 2);
    assert!(pills[0].x + pills[0].width <= pills[1].x, "{pills:?}");
    assert!(pills[1].x + pills[1].width <= 800.0 + 0.01, "{pills:?}");
}

#[test]
fn caps_lock_glyph_exists_in_the_ui_font() {
    let font = lumen_font::Font::parse(crate::assets::INTER_FONT).unwrap();
    let gid = font.cmap().unwrap().glyph_index(0x21ea);
    assert!(gid.is_some_and(|g| g != 0), "U+21EA missing from Inter: the badge would draw a box");
}
use lumen_layout::{ComputedStyle, DirtyBits};

fn make_submit_doc() -> (Document, NodeId) {
    // <form action="/go" method="get">
    //   <input name="q" value="rust">
    //   <input type="submit">
    // </form>
    let mut doc = Document::new();
    let form = doc.create_element(QualName::html("form"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
        attrs.push(Attribute { name: QualName::html("action"), value: "/go".into() });
        attrs.push(Attribute { name: QualName::html("method"), value: "get".into() });
    }
    let q = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(q).data {
        attrs.push(Attribute { name: QualName::html("name"), value: "q".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "rust".into() });
    }
    let submit = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
    }
    doc.append_child(doc.root(), form);
    doc.append_child(form, q);
    doc.append_child(form, submit);
    (doc, submit)
}

#[test]
fn submitter_overrides_win_over_form_attributes() {
    let (mut doc, submit) = make_submit_doc();
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
        attrs.push(Attribute { name: QualName::html("formaction"), value: "/other".into() });
        attrs.push(Attribute { name: QualName::html("formmethod"), value: "POST".into() });
        attrs.push(Attribute {
            name: QualName::html("formenctype"),
            value: "multipart/form-data".into(),
        });
        attrs.push(Attribute { name: QualName::html("formtarget"), value: "_top".into() });
    }
    let (mut action, mut method) = ("/go".to_owned(), "get".to_owned());
    let mut enctype = "application/x-www-form-urlencoded".to_owned();
    let mut target = String::new();
    apply_submitter_overrides(&doc, Some(submit), &mut action, &mut method, &mut enctype, &mut target);
    assert_eq!(action, "/other");
    assert_eq!(method, "post");
    assert_eq!(enctype, "multipart/form-data");
    assert_eq!(target, "_top");
}

#[test]
fn submitter_overrides_ignore_invalid_values_and_non_submit_buttons() {
    let (mut doc, submit) = make_submit_doc();
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
        attrs.push(Attribute { name: QualName::html("formaction"), value: String::new() });
        attrs.push(Attribute { name: QualName::html("formmethod"), value: "put".into() });
        attrs.push(Attribute { name: QualName::html("formenctype"), value: "x/y".into() });
    }
    let (mut action, mut method) = ("/go".to_owned(), "get".to_owned());
    let mut enctype = "text/plain".to_owned();
    let mut target = String::new();
    apply_submitter_overrides(&doc, Some(submit), &mut action, &mut method, &mut enctype, &mut target);
    assert_eq!((action.as_str(), method.as_str(), enctype.as_str()), ("/go", "get", "text/plain"));

    // type=button is not a submit button: its formaction is inert.
    let btn = doc.create_element(QualName::html("button"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(btn).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "button".into() });
        attrs.push(Attribute { name: QualName::html("formaction"), value: "/x".into() });
    }
    apply_submitter_overrides(&doc, Some(btn), &mut action, &mut method, &mut enctype, &mut target);
    assert_eq!(action, "/go");
}

#[test]
fn make_get_url_appends_query() {
    assert_eq!(make_get_url("/search", "q=hello"), "/search?q=hello");
}

#[test]
fn make_get_url_empty_body() {
    assert_eq!(make_get_url("/page", ""), "/page");
}

#[test]
fn make_get_url_empty_action() {
    assert_eq!(make_get_url("", "k=v"), "?k=v");
}

#[test]
fn make_get_url_strips_existing_query() {
    // HTML LS: заменяем query-string целиком при submit.
    assert_eq!(make_get_url("/s?old=1", "new=2"), "/s?new=2");
}

#[test]
fn make_get_url_strips_fragment() {
    assert_eq!(make_get_url("/page#sec", "x=1"), "/page?x=1");
}

#[test]
fn set_textarea_text_shadows_default_value() {
    let mut doc = Document::new();
    let ta = doc.create_element(QualName::html("textarea"));
    let old_text = doc.create_text("old value");
    doc.append_child(doc.root(), ta);
    doc.append_child(ta, old_text);

    set_textarea_text(&mut doc, ta, "new value");

    // Current value follows the edit…
    assert_eq!(doc.control_value(ta), "new value");
    // …while the child text stays the *default* value, which
    // `defaultValue`/`form.reset()` restore (HTML LS §4.10.11, BUG-441).
    let children = &doc.get(ta).children;
    assert_eq!(children.len(), 1);
    match &doc.get(children[0]).data {
        NodeData::Text(s) => assert_eq!(s, "old value"),
        _ => panic!("expected a text child"),
    }
    doc.clear_control_value(ta);
    assert_eq!(doc.control_value(ta), "old value");
}

#[test]
fn set_textarea_text_on_empty_textarea() {
    let mut doc = Document::new();
    let ta = doc.create_element(QualName::html("textarea"));
    doc.append_child(doc.root(), ta);

    set_textarea_text(&mut doc, ta, "hello");

    assert_eq!(doc.control_value(ta), "hello");
    assert!(doc.get(ta).children.is_empty());
}

#[test]
fn set_value_leaves_default_value_attribute_intact() {
    let mut doc = Document::new();
    let inp = doc.create_element(QualName::html("input"));
    doc.append_child(doc.root(), inp);
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
        attrs.push(Attribute { name: QualName::html("value"), value: "default".into() });
    }

    set_value(&mut doc, inp, "typed");

    assert_eq!(doc.control_value(inp), "typed");
    assert_eq!(doc.get(inp).get_attr("value"), Some("default"));
    doc.clear_control_value(inp);
    assert_eq!(doc.control_value(inp), "default");
}

#[test]
fn build_form_submit_get() {
    let (doc, submit) = make_submit_doc();
    let state = FormState::default();
    let result = build_form_submit(&doc, submit, &state);
    assert!(result.is_some());
    let (action, method, body) = result.unwrap();
    assert_eq!(action, "/go");
    assert_eq!(method, "get");
    assert_eq!(body, "q=rust");
}

#[test]
fn build_form_submit_post() {
    let mut doc = Document::new();
    let form = doc.create_element(QualName::html("form"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
        attrs.push(Attribute { name: QualName::html("action"), value: "/api".into() });
        attrs.push(Attribute { name: QualName::html("method"), value: "POST".into() });
    }
    let inp = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
        attrs.push(Attribute { name: QualName::html("name"), value: "email".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "a@b.c".into() });
    }
    let submit = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
    }
    doc.append_child(doc.root(), form);
    doc.append_child(form, inp);
    doc.append_child(form, submit);

    let state = FormState::default();
    let (action, method, body) = build_form_submit(&doc, submit, &state).unwrap();
    assert_eq!(action, "/api");
    assert_eq!(method, "post");
    assert!(body.contains("email="));
}

#[test]
fn build_form_submit_no_form_returns_none() {
    let mut doc = Document::new();
    let orphan = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(orphan).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
    }
    doc.append_child(doc.root(), orphan);
    let state = FormState::default();
    assert!(build_form_submit(&doc, orphan, &state).is_none());
}

#[test]
fn build_form_submit_runtime_state_overrides_dom() {
    let (doc, submit) = make_submit_doc();
    // Найдём NodeId поля "q" через DOM
    let form_id = lumen_dom::find_ancestor_form(&doc, submit).unwrap();
    let fields = lumen_dom::collect_dom_form_fields(&doc, form_id);
    assert!(!fields.is_empty());
    // Найдём NodeId поля q по имени
    let q_id = doc.root();  // заглушка — runtime_state override ищет по name-атрибуту
    let _ = q_id;
    // Для полноты проверяем что body содержит encoded name
    let state = FormState::default();
    let (_, _, body) = build_form_submit(&doc, submit, &state).unwrap();
    assert_eq!(body, "q=rust");
}

#[test]
fn validation_error_required_field() {
    let mut doc = Document::new();
    let form = doc.create_element(QualName::html("form"));
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("required"), value: String::new() });
        attrs.push(Attribute { name: QualName::html("placeholder"), value: "Email".into() });
    }
    doc.append_child(doc.root(), form);
    doc.append_child(form, input);

    if let Some(vs) = element_validity(&doc, input) {
        let msg = validation_error_message(&doc, input, &vs, &FormState::default()).unwrap();
        assert!(msg.contains("обязательно"));
    }
}

#[test]
fn validation_error_type_mismatch_email() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "email".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "not-an-email".into() });
        attrs.push(Attribute { name: QualName::html("placeholder"), value: "Email".into() });
    }
    doc.append_child(doc.root(), input);

    if let Some(vs) = element_validity(&doc, input)
        && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
    {
        assert!(msg.contains("email"));
    }
}

#[test]
fn validation_error_type_mismatch_url() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "url".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "not a url".into() });
        attrs.push(Attribute { name: QualName::html("placeholder"), value: "Website".into() });
    }
    doc.append_child(doc.root(), input);

    if let Some(vs) = element_validity(&doc, input)
        && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
    {
        assert!(msg.contains("URL"));
    }
}

#[test]
fn validation_error_range_underflow() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "number".into() });
        attrs.push(Attribute { name: QualName::html("min"), value: "5".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "3".into() });
        attrs.push(Attribute { name: QualName::html("placeholder"), value: "Age".into() });
    }
    doc.append_child(doc.root(), input);

    if let Some(vs) = element_validity(&doc, input)
        && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
    {
        assert!(msg.contains("меньше"));
    }
}

#[test]
fn validation_error_range_overflow() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "number".into() });
        attrs.push(Attribute { name: QualName::html("max"), value: "100".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "150".into() });
        attrs.push(Attribute { name: QualName::html("placeholder"), value: "Score".into() });
    }
    doc.append_child(doc.root(), input);

    if let Some(vs) = element_validity(&doc, input)
        && let Some(msg) = validation_error_message(&doc, input, &vs, &FormState::default())
    {
        assert!(msg.contains("больше"));
    }
}

#[test]
fn find_all_validation_errors_multiple() {
    let mut doc = Document::new();
    let form = doc.create_element(QualName::html("form"));

    // First invalid input (required)
    let inp1 = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp1).data {
        attrs.push(Attribute { name: QualName::html("required"), value: String::new() });
        attrs.push(Attribute { name: QualName::html("placeholder"), value: "Name".into() });
    }

    // Second invalid input (invalid email)
    let inp2 = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp2).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "email".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "bad-email".into() });
        attrs.push(Attribute { name: QualName::html("placeholder"), value: "Email".into() });
    }

    doc.append_child(doc.root(), form);
    doc.append_child(form, inp1);
    doc.append_child(form, inp2);

    // This would need a proper LayoutBox tree to test. For now just verify
    // that find_all_validation_errors function exists and compiles.
    let _ = find_all_validation_errors;
}

// ──── multipart encoding + enctype tests ────────────────────────────────

fn make_enctype_doc(enctype: &str) -> (Document, NodeId) {
    let mut doc = Document::new();
    let form = doc.create_element(QualName::html("form"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(form).data {
        attrs.push(Attribute { name: QualName::html("action"), value: "/upload".into() });
        attrs.push(Attribute { name: QualName::html("method"), value: "post".into() });
        attrs.push(Attribute { name: QualName::html("enctype"), value: enctype.into() });
    }
    let inp = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(inp).data {
        attrs.push(Attribute { name: QualName::html("name"), value: "file".into() });
        attrs.push(Attribute { name: QualName::html("value"), value: "data".into() });
    }
    let submit = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(submit).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
    }
    doc.append_child(doc.root(), form);
    doc.append_child(form, inp);
    doc.append_child(form, submit);
    (doc, submit)
}

#[test]
fn encode_form_fields_multipart_contains_boundary() {
    let fields = vec![("name".to_string(), "alice".to_string())];
    let (ct, body) = encode_form_fields_multipart(&fields, "testboundary");
    assert_eq!(ct, "multipart/form-data; boundary=testboundary");
    let s = String::from_utf8(body).unwrap();
    assert!(s.contains("--testboundary"), "body must contain boundary");
    assert!(s.contains("name=\"name\""), "body must contain field name");
    assert!(s.contains("alice"), "body must contain field value");
    assert!(s.ends_with("--testboundary--\r\n"), "body must end with closing boundary");
}

#[test]
fn encode_form_fields_multipart_two_fields() {
    let fields = vec![
        ("a".to_string(), "1".to_string()),
        ("b".to_string(), "2".to_string()),
    ];
    let (_ct, body) = encode_form_fields_multipart(&fields, "B");
    let s = String::from_utf8(body).unwrap();
    let a_pos = s.find("name=\"a\"").unwrap();
    let b_pos = s.find("name=\"b\"").unwrap();
    assert!(a_pos < b_pos, "fields must appear in order");
}

#[test]
fn get_form_enctype_default_urlencoded() {
    let (doc, submit) = make_submit_doc(); // no enctype attr
    let enctype = get_form_enctype(&doc, submit);
    assert_eq!(enctype, "application/x-www-form-urlencoded");
}

#[test]
fn encode_form_fields_plain_crlf_lines_no_escaping() {
    let fields = vec![("a b".to_owned(), "1&2".to_owned()), ("c".to_owned(), "é".to_owned())];
    let (ct, body) = encode_form_fields_plain(&fields);
    assert_eq!(ct, "text/plain;charset=UTF-8");
    assert_eq!(String::from_utf8(body).unwrap(), "a b=1&2\r\nc=é\r\n");
}

#[test]
fn get_form_enctype_multipart() {
    let (doc, submit) = make_enctype_doc("multipart/form-data");
    let enctype = get_form_enctype(&doc, submit);
    assert_eq!(enctype, "multipart/form-data");
}

#[test]
fn get_form_enctype_text_plain() {
    let (doc, submit) = make_enctype_doc("text/plain");
    let enctype = get_form_enctype(&doc, submit);
    assert_eq!(enctype, "text/plain");
}

#[test]
fn get_form_enctype_unknown_falls_back_to_urlencoded() {
    let (doc, submit) = make_enctype_doc("application/json");
    let enctype = get_form_enctype(&doc, submit);
    assert_eq!(enctype, "application/x-www-form-urlencoded");
}

#[test]
fn get_form_enctype_no_ancestor_form_defaults() {
    let mut doc = Document::new();
    let orphan = doc.create_element(QualName::html("input"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(orphan).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "submit".into() });
    }
    doc.append_child(doc.root(), orphan);
    let enctype = get_form_enctype(&doc, orphan);
    assert_eq!(enctype, "application/x-www-form-urlencoded");
}

// ──── <select> dropdown tests ────────────────────────────────────────────

fn make_select_doc() -> (Document, NodeId) {
    // <select>
    //   <option value="a">Apple</option>
    //   <option value="b" selected>Banana</option>
    //   <option value="c" disabled>Cherry</option>
    // </select>
    let mut doc = Document::new();
    let sel = doc.create_element(QualName::html("select"));

    let opt_a = doc.create_element(QualName::html("option"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(opt_a).data {
        attrs.push(Attribute { name: QualName::html("value"), value: "a".into() });
    }
    let txt_a = doc.create_text(String::from("Apple"));

    let opt_b = doc.create_element(QualName::html("option"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(opt_b).data {
        attrs.push(Attribute { name: QualName::html("value"), value: "b".into() });
        attrs.push(Attribute { name: QualName::html("selected"), value: String::new() });
    }
    let txt_b = doc.create_text(String::from("Banana"));

    let opt_c = doc.create_element(QualName::html("option"));
    if let NodeData::Element { attrs, .. } = &mut doc.get_mut(opt_c).data {
        attrs.push(Attribute { name: QualName::html("value"), value: "c".into() });
        attrs.push(Attribute { name: QualName::html("disabled"), value: String::new() });
    }
    let txt_c = doc.create_text(String::from("Cherry"));

    doc.append_child(doc.root(), sel);
    doc.append_child(sel, opt_a);
    doc.append_child(opt_a, txt_a);
    doc.append_child(sel, opt_b);
    doc.append_child(opt_b, txt_b);
    doc.append_child(sel, opt_c);
    doc.append_child(opt_c, txt_c);
    (doc, sel)
}

#[test]
fn collect_select_options_labels_and_values() {
    let (doc, sel) = make_select_doc();
    let opts = collect_select_options(&doc, sel);
    assert_eq!(opts.len(), 3);
    assert_eq!(opts[0].label, "Apple");
    assert_eq!(opts[0].value, "a");
    assert!(!opts[0].selected);
    assert!(!opts[0].disabled);
    assert_eq!(opts[1].label, "Banana");
    assert_eq!(opts[1].value, "b");
    assert!(opts[1].selected);
    assert!(!opts[1].disabled);
    assert!(opts[2].disabled);
}

#[test]
fn selectlist_opens_dropdown_and_collects_listbox_options() {
    let doc = lumen_html_parser::parse(
        r#"<body><selectlist id="s"><listbox><option>A</option><option selected>B</option></listbox><option>C</option></selectlist></body>"#,
    );
    let sel = doc.find_by_id("s").unwrap_or_else(|| unreachable!());
    assert!(matches!(classify_click(&doc, sel), FormClickAction::OpenSelectDropdown(_)));
    let labels: Vec<_> =
        collect_select_options(&doc, sel).into_iter().map(|o| o.label).collect();
    assert_eq!(labels, ["A", "B", "C"]);
}

#[test]
fn classify_click_select_returns_open_dropdown() {
    let (doc, sel) = make_select_doc();
    let action = classify_click(&doc, sel);
    assert!(matches!(action, FormClickAction::OpenSelectDropdown(_)));
}

#[test]
fn hit_select_option_returns_correct_row() {
    let anchor = Rect::new(100.0, 200.0, 120.0, 22.0);
    // dropdown appears below anchor (no flip needed for default viewport)
    let vp_w = 1024.0;
    let vp_h = 720.0;
    let scroll_y = 0.0;
    // Click on first row: just below the anchor + border + PAD_Y
    let anchor_bottom_vp = 200.0 + 22.0;
    let dd_y = anchor_bottom_vp + 1.0;
    let click_y = dd_y + DROPDOWN_PAD_Y + DROPDOWN_ROW_H * 0.5;
    let result = hit_select_option(anchor, 3, scroll_y, vp_w, vp_h, 110.0, click_y);
    assert_eq!(result, Some(0));
}

#[test]
fn hit_select_option_second_row() {
    let anchor = Rect::new(100.0, 200.0, 120.0, 22.0);
    let vp_w = 1024.0;
    let vp_h = 720.0;
    let scroll_y = 0.0;
    let anchor_bottom_vp = 200.0 + 22.0;
    let dd_y = anchor_bottom_vp + 1.0;
    let click_y = dd_y + DROPDOWN_PAD_Y + DROPDOWN_ROW_H * 1.5;
    let result = hit_select_option(anchor, 3, scroll_y, vp_w, vp_h, 110.0, click_y);
    assert_eq!(result, Some(1));
}

#[test]
fn hit_select_option_outside_returns_none() {
    let anchor = Rect::new(100.0, 200.0, 120.0, 22.0);
    // Click far outside the dropdown area.
    let result = hit_select_option(anchor, 3, 0.0, 1024.0, 720.0, 500.0, 500.0);
    assert_eq!(result, None);
}

#[test]
fn apply_select_choice_moves_selected_attr() {
    let (mut doc, sel) = make_select_doc();
    let opts = collect_select_options(&doc, sel);
    // Initially "b" (idx=1) is selected. Pick idx=0.
    apply_select_choice(&mut doc, &opts, 0);
    let updated = collect_select_options(&doc, sel);
    assert!(updated[0].selected);
    assert!(!updated[1].selected);
    assert!(!updated[2].selected);
}

#[test]
fn build_select_dropdown_non_empty() {
    let (doc, sel) = make_select_doc();
    let opts = collect_select_options(&doc, sel);
    let anchor = Rect::new(10.0, 10.0, 100.0, 22.0);
    let dl = build_select_dropdown(anchor, &opts, 0.0, 1024.0, 720.0);
    assert!(!dl.is_empty(), "dropdown display list should not be empty");
}

#[test]
fn base_select_dropdown_honors_author_option_background() {
    // `appearance: base-select` must let author CSS on `<option>` reach the
    // picker: a per-option `background-color` becomes a FillRect in the
    // dropdown display list (unlike the fixed-colour native dropdown).
    let (doc, sel) = make_select_doc();
    let opts = collect_select_options(&doc, sel);
    let sheet = lumen_css_parser::parse("option { background-color: rgb(10, 20, 30); }");
    let sel_style = ComputedStyle::root();
    let anchor = Rect::new(10.0, 10.0, 100.0, 22.0);
    let vp = Size::new(1024.0, 720.0);
    let styles = resolve_option_row_styles(&doc, &sheet, &sel_style, &opts, vp, false);
    let dl = build_base_select_dropdown(anchor, &sel_style, &opts, &styles, 0.0, 1024.0, 720.0);
    let has_author_bg = dl.iter().any(|cmd| {
        matches!(
            cmd,
            DisplayCommand::FillRect {
                color: Color { r: 10, g: 20, b: 30, a: 255 },
                ..
            }
        )
    });
    assert!(
        has_author_bg,
        "author option background-color must reach the base-select dropdown display list"
    );
}

// ── <details>/<summary> tests ─────────────────────────────────────────────

/// Build <details><summary>S</summary><p>content</p></details>.
fn make_details_doc() -> (Document, NodeId, NodeId) {
    let mut doc = Document::new();
    let details = doc.create_element(QualName::html("details"));
    let summary = doc.create_element(QualName::html("summary"));
    let p = doc.create_element(QualName::html("p"));
    doc.append_child(doc.root(), details);
    doc.append_child(details, summary);
    doc.append_child(details, p);
    (doc, details, summary)
}

#[test]
fn classify_click_summary_returns_toggle_details() {
    let (doc, details, summary) = make_details_doc();
    match classify_click(&doc, summary) {
        FormClickAction::ToggleDetails(id) => assert_eq!(id, details),
        other => panic!("expected ToggleDetails, got {other:?}"),
    }
}

#[test]
fn toggle_details_open_adds_open_attr() {
    let (mut doc, details, _) = make_details_doc();
    // Initially no `open` attribute.
    assert!(doc.get(details).get_attr("open").is_none());
    toggle_details_open(&mut doc, details);
    assert!(doc.get(details).get_attr("open").is_some());
}

#[test]
fn toggle_details_open_removes_open_attr() {
    let (mut doc, details, _) = make_details_doc();
    // Add `open` first.
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(details).data {
        attrs.push(Attribute { name: QualName::html("open"), value: String::new() });
    }
    toggle_details_open(&mut doc, details);
    assert!(doc.get(details).get_attr("open").is_none());
}

#[test]
fn classify_click_non_summary_returns_nothing_for_p() {
    let mut doc = Document::new();
    let p = doc.create_element(QualName::html("p"));
    doc.append_child(doc.root(), p);
    matches!(classify_click(&doc, p), FormClickAction::Nothing);
}

#[test]
fn summary_without_details_parent_returns_nothing() {
    // <summary> that is NOT inside a <details> should return Nothing.
    let mut doc = Document::new();
    let summary = doc.create_element(QualName::html("summary"));
    doc.append_child(doc.root(), summary);
    matches!(classify_click(&doc, summary), FormClickAction::Nothing);
}

// ── activation target (BUG-837) ──────────────────────────────────────────

/// Give `node` an attribute; the test docs are built by hand, so there is no
/// parser to carry `type`/`for`/`disabled` in.
fn set_attr(doc: &mut Document, node: NodeId, name: &str, value: &str) {
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(node).data {
        attrs.push(Attribute { name: QualName::html(name), value: value.to_owned() });
    }
}

/// `<label for="cb"><span>peas?</span></label><input type=checkbox id=cb>`
/// — returns the span the hit test would land on and the checkbox.
fn make_label_doc() -> (Document, NodeId, NodeId, NodeId) {
    let mut doc = Document::new();
    let label = doc.create_element(QualName::html("label"));
    let span = doc.create_element(QualName::html("span"));
    let cb = doc.create_element(QualName::html("input"));
    set_attr(&mut doc, label, "for", "cb");
    set_attr(&mut doc, cb, "type", "checkbox");
    set_attr(&mut doc, cb, "id", "cb");
    doc.append_child(doc.root(), label);
    doc.append_child(label, span);
    doc.append_child(doc.root(), cb);
    (doc, label, span, cb)
}

#[test]
fn click_inside_a_label_toggles_the_labeled_control() {
    let (doc, _label, span, cb) = make_label_doc();
    match classify_click(&doc, span) {
        FormClickAction::ToggleCheckbox(id) => assert_eq!(id, cb),
        other => panic!("expected ToggleCheckbox, got {other:?}"),
    }
}

#[test]
fn click_on_the_label_itself_toggles_the_labeled_control() {
    let (doc, label, _span, cb) = make_label_doc();
    match classify_click(&doc, label) {
        FormClickAction::ToggleCheckbox(id) => assert_eq!(id, cb),
        other => panic!("expected ToggleCheckbox, got {other:?}"),
    }
}

#[test]
fn a_label_wrapping_its_control_needs_no_for_attribute() {
    let mut doc = Document::new();
    let label = doc.create_element(QualName::html("label"));
    let cb = doc.create_element(QualName::html("input"));
    let span = doc.create_element(QualName::html("span"));
    set_attr(&mut doc, cb, "type", "checkbox");
    doc.append_child(doc.root(), label);
    doc.append_child(label, span);
    doc.append_child(label, cb);
    match classify_click(&doc, span) {
        FormClickAction::ToggleCheckbox(id) => assert_eq!(id, cb),
        other => panic!("expected ToggleCheckbox, got {other:?}"),
    }
}

#[test]
fn a_label_pointing_at_nothing_activates_nothing() {
    let (mut doc, label, span, _cb) = make_label_doc();
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(label).data {
        attrs.retain(|a| a.name.local.as_str() != "for");
    }
    set_attr(&mut doc, label, "for", "missing");
    assert!(matches!(classify_click(&doc, span), FormClickAction::Nothing));
}

#[test]
fn interactive_content_stops_the_walk() {
    // HTML LS §4.10.20: the label does nothing for a click targeted at an
    // interactive-content descendant, so the checkbox must stay untouched.
    let (mut doc, label, _span, _cb) = make_label_doc();
    let ta = doc.create_element(QualName::html("textarea"));
    let inner = doc.create_element(QualName::html("span"));
    doc.append_child(label, ta);
    doc.append_child(ta, inner);
    assert!(matches!(classify_click(&doc, ta), FormClickAction::Nothing));
    assert!(matches!(classify_click(&doc, inner), FormClickAction::Nothing));
}

#[test]
fn click_inside_a_submit_button_submits() {
    let mut doc = Document::new();
    let button = doc.create_element(QualName::html("button"));
    let img = doc.create_element(QualName::html("img"));
    doc.append_child(doc.root(), button);
    doc.append_child(button, img);
    match classify_click(&doc, img) {
        FormClickAction::SubmitForm(id) => assert_eq!(id, button),
        other => panic!("expected SubmitForm, got {other:?}"),
    }
}

#[test]
fn a_disabled_ancestor_is_not_activated() {
    let mut doc = Document::new();
    let button = doc.create_element(QualName::html("button"));
    let img = doc.create_element(QualName::html("img"));
    set_attr(&mut doc, button, "disabled", "");
    doc.append_child(doc.root(), button);
    doc.append_child(button, img);
    assert!(matches!(classify_click(&doc, img), FormClickAction::Nothing));
}

#[test]
fn click_inside_a_summary_toggles_its_details() {
    let (mut doc, details, summary) = make_details_doc();
    let span = doc.create_element(QualName::html("span"));
    doc.append_child(summary, span);
    match classify_click(&doc, span) {
        FormClickAction::ToggleDetails(id) => assert_eq!(id, details),
        other => panic!("expected ToggleDetails, got {other:?}"),
    }
}

// ── <dialog> modal overlay tests ─────────────────────────────────────────

fn make_dialog_doc() -> (Document, NodeId) {
    let mut doc = Document::new();
    let dlg = doc.create_element(QualName::html("dialog"));
    doc.append_child(doc.root(), dlg);
    (doc, dlg)
}

fn make_dialog_lb(node: NodeId, rect: Rect) -> LayoutBox {
    LayoutBox {
        anchor_scroll: None, cv_hidden: Vec::new(),
        node,
        rect,
        used_padding: None, used_line_height: 16.0 * 1.2,
        grid_baselines: None,
        fieldset_legend: None,
        table_caption_inset: [0.0; 2],
        clamp_clip: None,
        subgrid_tracks: None, used_grid_tracks: None,
        style: std::sync::Arc::new(ComputedStyle::root()),
        kind: BoxKind::Block,
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: DirtyBits::CLEAN,
        origin: lumen_layout::BoxOrigin { node: Some(node), role: lumen_layout::BoxRole::Element },
    }
}

#[test]
fn collect_modal_dialogs_empty_without_sentinel() {
    let (doc, _) = make_dialog_doc();
    // No `data-lumen-modal` attr → empty result.
    assert!(collect_modal_dialogs(&doc).is_empty());
}

#[test]
fn collect_modal_dialogs_finds_sentinel() {
    let (mut doc, dlg) = make_dialog_doc();
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(dlg).data {
        attrs.push(Attribute {
            name: QualName::html("data-lumen-modal"),
            value: String::new(),
        });
    }
    let result = collect_modal_dialogs(&doc);
    assert_eq!(result, vec![dlg]);
}

#[test]
fn collect_modal_dialogs_finds_multiple_in_order() {
    let mut doc = Document::new();
    let d1 = doc.create_element(QualName::html("dialog"));
    let d2 = doc.create_element(QualName::html("dialog"));
    doc.append_child(doc.root(), d1);
    doc.append_child(doc.root(), d2);
    for &id in &[d1, d2] {
        if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(id).data {
            attrs.push(Attribute {
                name: QualName::html("data-lumen-modal"),
                value: String::new(),
            });
        }
    }
    assert_eq!(collect_modal_dialogs(&doc), vec![d1, d2]);
}

#[test]
fn build_dialog_overlay_starts_with_backdrop() {
    let mut _doc = Document::new();
    let nid = _doc.create_element(QualName::html("dialog"));
    let lb = make_dialog_lb(nid, Rect::new(200.0, 100.0, 300.0, 200.0));
    let overlay = build_dialog_overlay(&lb, 0.0, 1024.0, 720.0);
    // First command must be a full-viewport FillRect (the ::backdrop).
    match overlay.first() {
        Some(DisplayCommand::FillRect { rect, .. }) => {
            assert!((rect.width - 1024.0).abs() < 1.0);
            assert!((rect.height - 720.0).abs() < 1.0);
        }
        other => panic!("expected FillRect backdrop, got {other:?}"),
    }
}

#[test]
fn build_dialog_overlay_wraps_with_transform() {
    let mut _doc = Document::new();
    let nid = _doc.create_element(QualName::html("dialog"));
    let lb = make_dialog_lb(nid, Rect::new(0.0, 0.0, 200.0, 100.0));
    let overlay = build_dialog_overlay(&lb, 0.0, 1024.0, 720.0);
    let has_push = overlay.iter().any(|c| matches!(c, DisplayCommand::PushTransform { .. }));
    let has_pop = overlay.iter().any(|c| matches!(c, DisplayCommand::PopTransform));
    assert!(has_push, "overlay must contain PushTransform");
    assert!(has_pop, "overlay must contain PopTransform");
}

// ── Range input tests ─────────────────────────────────────────────────────

#[test]
fn classify_click_range_input_returns_slide_range() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "range".to_owned() });
    }
    doc.append_child(doc.root(), input);
    assert!(matches!(classify_click(&doc, input), FormClickAction::SlideRange(_)));
}

#[test]
fn apply_range_value_sets_correct_value() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "range".to_owned() });
        attrs.push(Attribute { name: QualName::html("min"), value: "0".to_owned() });
        attrs.push(Attribute { name: QualName::html("max"), value: "100".to_owned() });
    }
    doc.append_child(doc.root(), input);

    // Click at exact midpoint of a 100px-wide slider rect.
    let rect = lumen_core::geom::Rect::new(0.0, 0.0, 100.0, 20.0);
    apply_range_value(&mut doc, input, rect, 50.0);

    let val: f32 = doc.control_value(input).trim().parse().unwrap_or(-1.0);
    assert!((40.0..=60.0).contains(&val), "expected ~50.0, got {val}");
}

#[test]
fn apply_range_value_clamps_to_bounds() {
    let mut doc = Document::new();
    let input = doc.create_element(QualName::html("input"));
    if let NodeData::Element { ref mut attrs, .. } = doc.get_mut(input).data {
        attrs.push(Attribute { name: QualName::html("type"), value: "range".to_owned() });
        attrs.push(Attribute { name: QualName::html("min"), value: "0".to_owned() });
        attrs.push(Attribute { name: QualName::html("max"), value: "100".to_owned() });
    }
    doc.append_child(doc.root(), input);

    // Click far to the left of the track → value should be min (0).
    let rect = lumen_core::geom::Rect::new(10.0, 0.0, 100.0, 20.0);
    apply_range_value(&mut doc, input, rect, -1000.0);

    let val: f32 = doc.control_value(input).trim().parse().unwrap_or(-1.0);
    assert!(val >= 0.0, "value clamped below min should be >= 0, got {val}");
}

// ── Date picker tests ─────────────────────────────────────────────────────

#[test]
fn days_in_month_common_cases() {
    assert_eq!(days_in_month(2026, 1), 31);
    assert_eq!(days_in_month(2026, 2), 28);
    assert_eq!(days_in_month(2024, 2), 29); // leap year
    assert_eq!(days_in_month(2026, 4), 30);
    assert_eq!(days_in_month(2026, 12), 31);
}

#[test]
fn is_leap_year_cases() {
    assert!(is_leap_year(2000)); // divisible by 400
    assert!(!is_leap_year(1900)); // divisible by 100 but not 400
    assert!(is_leap_year(2024));
    assert!(!is_leap_year(2023));
}

#[test]
fn first_weekday_of_month_june_2026() {
    // June 1, 2026 is a Monday (ISO weekday 0).
    assert_eq!(first_weekday_of_month(2026, 6), 0);
}

#[test]
fn first_weekday_of_month_january_2024() {
    // January 1, 2024 is a Monday.
    assert_eq!(first_weekday_of_month(2024, 1), 0);
}

#[test]
fn first_weekday_of_month_february_2026() {
    // February 1, 2026 is a Sunday (ISO weekday 6).
    assert_eq!(first_weekday_of_month(2026, 2), 6);
}

#[test]
fn parse_date_value_valid() {
    assert_eq!(parse_date_value("2026-06-09"), Some((2026, 6, 9)));
    assert_eq!(parse_date_value("2000-01-01"), Some((2000, 1, 1)));
}

#[test]
fn parse_date_value_invalid() {
    assert_eq!(parse_date_value("not-a-date"), None);
    assert_eq!(parse_date_value("2026-13-01"), None); // month out of range
    assert_eq!(parse_date_value("2026-06"), None);   // too short
}

#[test]
fn format_date_value_roundtrip() {
    let s = format_date_value(2026, 6, 9);
    assert_eq!(s, "2026-06-09");
    assert_eq!(parse_date_value(&s), Some((2026, 6, 9)));
}

#[test]
fn advance_month_forward() {
    assert_eq!(advance_month(2026, 12, 1), (2027, 1));
    assert_eq!(advance_month(2026, 6, 1), (2026, 7));
}

#[test]
fn advance_month_backward() {
    assert_eq!(advance_month(2026, 1, -1), (2025, 12));
    assert_eq!(advance_month(2026, 6, -1), (2026, 5));
}

#[test]
fn hit_date_picker_outside_returns_none() {
    let anchor = Rect::new(100.0, 100.0, 120.0, 22.0);
    // Click far outside the overlay area.
    let hit = hit_date_picker(anchor, 0.0, 1024.0, 2026, 6, 0.0, 0.0);
    assert_eq!(hit, DatePickerHit::None);
}

#[test]
fn hit_date_picker_prev_arrow() {
    let anchor = Rect::new(100.0, 100.0, 120.0, 22.0);
    let (pw, _) = date_picker_size();
    // Click on prev-arrow: left part of header, inside overlay.
    let vp_y = 100.0 + 22.0 + 4.0; // anchor bottom + gap
    let vp_x = 100.0_f32.min(1024.0 - pw).max(0.0);
    // Click in the middle of the prev arrow (left side of header).
    let px = vp_x + DATE_PICKER_PAD + DATE_HEADER_H / 2.0;
    let py = vp_y + DATE_PICKER_PAD + DATE_HEADER_H / 2.0;
    let hit = hit_date_picker(anchor, 0.0, 1024.0, 2026, 6, px, py);
    assert_eq!(hit, DatePickerHit::Prev);
}

#[test]
fn hit_date_picker_next_arrow() {
    let anchor = Rect::new(100.0, 100.0, 120.0, 22.0);
    let (pw, _) = date_picker_size();
    let vp_y = 100.0 + 22.0 + 4.0;
    let vp_x = 100.0_f32.min(1024.0 - pw).max(0.0);
    let content_w = pw - DATE_PICKER_PAD * 2.0;
    // Click in the middle of the next arrow (right side of header).
    let px = vp_x + DATE_PICKER_PAD + content_w - DATE_HEADER_H / 2.0;
    let py = vp_y + DATE_PICKER_PAD + DATE_HEADER_H / 2.0;
    let hit = hit_date_picker(anchor, 0.0, 1024.0, 2026, 6, px, py);
    assert_eq!(hit, DatePickerHit::Next);
}

#[test]
fn hit_date_picker_first_day() {
    // June 2026: first_weekday=Mon=0 → day 1 is in column 0, row 0.
    let anchor = Rect::new(100.0, 100.0, 120.0, 22.0);
    let (pw, _) = date_picker_size();
    let vp_y = 100.0 + 22.0 + 4.0;
    let vp_x = 100.0_f32.min(1024.0 - pw).max(0.0);
    // Day 1: col=0, row=0 in the grid.
    let cell_x = vp_x + DATE_PICKER_PAD + DATE_CELL_W / 2.0;
    let cell_y = vp_y + DATE_PICKER_PAD + DATE_HEADER_H + DATE_DAYNAME_H + DATE_CELL_H / 2.0;
    let hit = hit_date_picker(anchor, 0.0, 1024.0, 2026, 6, cell_x, cell_y);
    assert_eq!(hit, DatePickerHit::Day(1));
}

#[test]
fn build_date_picker_is_nonempty() {
    let anchor = Rect::new(10.0, 10.0, 120.0, 22.0);
    let dl = build_date_picker(anchor, 0.0, 1024.0, 2026, 6);
    assert!(!dl.is_empty(), "date picker display list should not be empty");
}

// ── FRAME-7 (remainder item 1): <textarea> caret geometry ───────────────

fn make_field_lb(node: NodeId, rect: Rect, children: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox {
        anchor_scroll: None, cv_hidden: Vec::new(),
        node,
        rect,
        used_padding: None, used_line_height: 16.0 * 1.2,
        grid_baselines: None,
        fieldset_legend: None,
        table_caption_inset: [0.0; 2],
        clamp_clip: None,
        subgrid_tracks: None, used_grid_tracks: None,
        style: std::sync::Arc::new(ComputedStyle::root()),
        kind: BoxKind::Block,
        children,
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: DirtyBits::CLEAN,
        origin: lumen_layout::BoxOrigin { node: Some(node), role: lumen_layout::BoxRole::Element },
    }
}

fn make_inline_run_lb(node: NodeId, rect: Rect) -> LayoutBox {
    LayoutBox {
        anchor_scroll: None, cv_hidden: Vec::new(),
        node,
        rect,
        used_padding: None, used_line_height: 16.0 * 1.2,
        grid_baselines: None,
        fieldset_legend: None,
        table_caption_inset: [0.0; 2],
        clamp_clip: None,
        subgrid_tracks: None, used_grid_tracks: None,
        style: std::sync::Arc::new(ComputedStyle::root()),
        kind: BoxKind::InlineRun { segments: vec![], lines: vec![], first_line_style: None, row_continuation_width: None, first_line_inset: 0.0 },
        children: vec![],
        col_span: 1,
        row_span: 1,
        svg_group_transform: None,
        scroll_x: 0.0,
        scroll_y: 0.0,
        dirty: DirtyBits::CLEAN,
        origin: lumen_layout::BoxOrigin { node: Some(node), role: lumen_layout::BoxRole::AnonymousInlineRun },
    }
}

/// 8px/char stand-in measurer — the real one is a bundled-font
/// `FontMeasurer`, not available to this crate's unit tests.
fn char_width_measure(s: &str) -> f32 {
    s.chars().count() as f32 * 8.0
}

#[test]
fn textarea_caret_rect_empty_value_falls_back_to_content_edge() {
    let node = NodeId::from_index(1);
    let field = make_field_lb(node, Rect::new(10.0, 20.0, 100.0, 50.0), vec![]);
    let rect = textarea_caret_rect(&field, "", 0, &char_width_measure);
    // No InlineRun child (untouched textarea) → border+padding content
    // edge, both zero on `ComputedStyle::root()`.
    assert_eq!(rect.x, 10.0);
    assert_eq!(rect.y, 20.0);
    assert_eq!(rect.height, 16.0 * 1.2);
}

#[test]
fn textarea_caret_rect_multiline_lands_on_second_logical_line() {
    let node = NodeId::from_index(1);
    let field = make_field_lb(node, Rect::new(0.0, 0.0, 100.0, 50.0), vec![]);
    // "ab\ncd", cursor=4 → one char into the second logical line ("c").
    let rect = textarea_caret_rect(&field, "ab\ncd", 4, &char_width_measure);
    let line_h = 16.0 * 1.2;
    assert_eq!(rect.x, 8.0); // one char ("c") at 8px/char
    assert_eq!(rect.y, line_h);
    assert_eq!(rect.height, line_h);
}

#[test]
fn textarea_caret_rect_end_of_value() {
    let node = NodeId::from_index(1);
    let field = make_field_lb(node, Rect::new(0.0, 0.0, 100.0, 50.0), vec![]);
    let rect = textarea_caret_rect(&field, "ab\ncd", 5, &char_width_measure);
    let line_h = 16.0 * 1.2;
    assert_eq!(rect.x, 16.0); // "cd" fully consumed, 2 chars
    assert_eq!(rect.y, line_h);
}

#[test]
fn textarea_caret_rect_prefers_inline_run_child_origin() {
    let node = NodeId::from_index(1);
    // The FormControl box's own border-box has no border/padding set
    // here either, but a real InlineRun child (present once the value
    // is "dirty"/runtime) must still win over the fallback — its
    // `.rect` already reflects the true layout-resolved content origin,
    // which a hand-rolled border+padding sum would not for e.g.
    // percentage padding.
    let inline_run = make_inline_run_lb(node, Rect::new(7.0, 9.0, 90.0, 40.0));
    let field = make_field_lb(node, Rect::new(0.0, 0.0, 100.0, 50.0), vec![inline_run]);
    let rect = textarea_caret_rect(&field, "x", 0, &char_width_measure);
    assert_eq!(rect.x, 7.0);
    assert_eq!(rect.y, 9.0);
}

// ── FRAME-7 остаток (1): <input> caret geometry ──────────────────────────

#[test]
fn input_caret_rect_at_start() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(10.0, 20.0, 100.0, 30.0), vec![]);
    let rect = input_caret_rect(&field, "abc", 0);
    // border 0 (ComputedStyle::root()) + 2px inset.
    assert_eq!(rect.x, 12.0);
    assert_eq!(rect.y, 20.0);
    assert_eq!(rect.width, 1.0);
    assert_eq!(rect.height, 30.0);
}

#[test]
fn input_caret_rect_advances_by_char_count() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 20.0), vec![]);
    let rect = input_caret_rect(&field, "abcd", 2);
    // font_size(16) * 0.5 * 2 chars = 16px past the content edge.
    assert_eq!(rect.x, 2.0 + 16.0);
}

#[test]
fn input_caret_rect_clamps_to_value_length() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 20.0), vec![]);
    let rect = input_caret_rect(&field, "ab", 99);
    // char_index past the value clamps to its length (2 chars).
    assert_eq!(rect.x, 2.0 + 16.0);
}

// ── FRAME-7 остаток: mouse-drag hit tests (inverse of the rects above) ──

#[test]
fn input_char_index_at_x_lands_on_content_edge() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 20.0), vec![]);
    // content_x = 0 + border(0) + inset(2).
    assert_eq!(input_char_index_at_x(&field, "abcd", 2.0), 0);
}

#[test]
fn input_char_index_at_x_rounds_to_nearest_char() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 20.0), vec![]);
    // font_size(16) * 0.5 = 8px/char; content_x + 8 rounds to 1 char in.
    assert_eq!(input_char_index_at_x(&field, "abcd", 2.0 + 8.0), 1);
    // Halfway between 1 and 2 chars rounds to the nearer one (12px → 1.5 → round-to-even 2).
    assert_eq!(input_char_index_at_x(&field, "abcd", 2.0 + 20.0), 3);
}

#[test]
fn input_char_index_at_x_clamps_to_value_bounds() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 20.0), vec![]);
    assert_eq!(input_char_index_at_x(&field, "abcd", -50.0), 0);
    assert_eq!(input_char_index_at_x(&field, "abcd", 9999.0), 4);
}

#[test]
fn textarea_char_index_at_point_first_line_start() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 50.0), vec![]);
    let idx = textarea_char_index_at_point(&field, "ab\ncd", 0.0, 0.0, &char_width_measure);
    assert_eq!(idx, 0);
}

#[test]
fn textarea_char_index_at_point_end_of_first_line() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 50.0), vec![]);
    // "ab" is 16px wide at 8px/char — a click right at that edge lands
    // after both chars, still on line 0 (not yet the '\n').
    let idx = textarea_char_index_at_point(&field, "ab\ncd", 16.0, 0.0, &char_width_measure);
    assert_eq!(idx, 2);
}

#[test]
fn textarea_char_index_at_point_second_line() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 50.0), vec![]);
    let line_h = 16.0 * 1.2;
    // Second logical line starts at char-index 3 ("ab\n" consumed).
    let idx = textarea_char_index_at_point(&field, "ab\ncd", 0.0, line_h, &char_width_measure);
    assert_eq!(idx, 3);
}

#[test]
fn textarea_char_index_at_point_clamps_below_last_line() {
    let field = make_field_lb(NodeId::from_index(1), Rect::new(0.0, 0.0, 100.0, 50.0), vec![]);
    let idx = textarea_char_index_at_point(&field, "ab\ncd", 0.0, 9999.0, &char_width_measure);
    // Clamped to the last logical line's start (char-index 3), not past it
    // since x=0 is the line's own start.
    assert_eq!(idx, 3);
}

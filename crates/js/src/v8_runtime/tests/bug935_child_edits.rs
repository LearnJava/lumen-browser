//! BUG-935 срез 89 — что `dom_changes_reader` говорит о правке списка детей: плоский
//! `ChildList` или `ChildListEnds` с признаками «правка у начала/конца» и числом правок.
//! По ним слой раскладки решает, какие дети могли сменить `:first-child`/`:last-child`.

use super::*;
use lumen_layout::style::OwnedNodeChange;

/// Изменения узла `node` с последнего чтения.
fn changes_of(read: &impl Fn() -> DomChanges, node: lumen_dom::NodeId) -> Vec<OwnedNodeChange> {
    read().changes.into_iter().filter(|(n, _)| *n == node).map(|(_, c)| c).collect()
}

fn setup() -> (V8JsRuntime, lumen_dom::NodeId, lumen_dom::NodeId) {
    let doc = make_doc();
    let (body, main) = {
        let d = doc.lock().unwrap();
        let main = d.find_by_id("main").unwrap();
        (d.get(main).parent.unwrap(), main)
    };
    (runtime_with_dom(doc, ""), body, main)
}

#[test]
fn appending_an_element_after_another_touches_the_back_end_only() {
    let (rt, body, _main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval("document.body.appendChild(document.createElement('iframe'))").unwrap();
    assert_eq!(
        changes_of(&read, body),
        vec![OwnedNodeChange::ChildListEnds { front: false, back: true, edits: 1 }]
    );
}

#[test]
fn inserting_before_the_first_element_touches_the_front_end() {
    let (rt, body, main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    let _ = main;
    rt.eval("document.body.insertBefore(document.createElement('p'), document.getElementById('main'))").unwrap();
    assert_eq!(
        changes_of(&read, body),
        vec![OwnedNodeChange::ChildListEnds { front: true, back: false, edits: 1 }]
    );
}

#[test]
fn edits_between_two_reads_add_up() {
    let (rt, body, _main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval(
        "var b = document.body, m = document.getElementById('main'); \
         b.appendChild(document.createElement('i')); \
         b.insertBefore(document.createElement('u'), m.nextSibling); \
         b.insertBefore(document.createElement('s'), m);",
    )
    .unwrap();
    // 1: `i` after `main` — back; 2: `u` between `main` and `i` — neither end; 3: `s` before `main` — front.
    assert_eq!(
        changes_of(&read, body),
        vec![OwnedNodeChange::ChildListEnds { front: true, back: true, edits: 3 }]
    );
}

#[test]
fn removing_the_only_element_touches_both_ends() {
    let (rt, body, _main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval("document.body.removeChild(document.getElementById('main'))").unwrap();
    assert_eq!(
        changes_of(&read, body),
        vec![OwnedNodeChange::ChildListEnds { front: true, back: true, edits: 1 }]
    );
}

#[test]
fn a_text_node_is_no_edit_to_the_element_list() {
    let (rt, _body, main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval("document.getElementById('main').appendChild(document.createTextNode('x'))").unwrap();
    assert_eq!(
        changes_of(&read, main),
        vec![OwnedNodeChange::ChildListEnds { front: false, back: false, edits: 0 }]
    );
}

#[test]
fn a_move_inside_one_parent_counts_as_two_edits() {
    let (rt, body, _main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval(
        "document.body.appendChild(document.createElement('p')); \
         document.body.appendChild(document.createElement('p'));",
    )
    .unwrap();
    let _ = read();
    // `main` (first of three) goes to the end: it leaves the front and arrives at the back.
    rt.eval("document.body.appendChild(document.getElementById('main'))").unwrap();
    assert_eq!(
        changes_of(&read, body),
        vec![OwnedNodeChange::ChildListEnds { front: true, back: true, edits: 2 }]
    );
}

#[test]
fn a_move_between_parents_reports_both_lists() {
    let (rt, body, main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval("document.body.appendChild(document.createElement('p'))").unwrap();
    let _ = read();
    rt.eval("document.getElementById('main').appendChild(document.body.lastChild)").unwrap();
    // `p` leaves the back of `body` and arrives in `main`'s list after `span`: `span` stands
    // before it, so the back only.
    let all = read().changes;
    let of = |node| all.iter().filter(|(n, _)| *n == node).map(|(_, c)| c.clone()).collect::<Vec<_>>();
    assert_eq!(of(body), vec![OwnedNodeChange::ChildListEnds { front: false, back: true, edits: 1 }]);
    assert_eq!(of(main), vec![OwnedNodeChange::ChildListEnds { front: false, back: true, edits: 1 }]);
}

#[test]
fn inner_html_and_text_content_stay_the_plain_child_list() {
    let (rt, _body, main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval("document.getElementById('main').innerHTML = '<b>x</b>'").unwrap();
    assert_eq!(changes_of(&read, main), vec![OwnedNodeChange::ChildList]);
    rt.eval("document.getElementById('main').appendChild(document.createElement('i'))").unwrap();
    rt.eval("document.getElementById('main').textContent = 'plain'").unwrap();
    assert_eq!(changes_of(&read, main), vec![OwnedNodeChange::ChildList]);
}

#[test]
fn a_log_that_outgrows_its_cap_falls_back_to_the_plain_child_list() {
    let (rt, body, _main) = setup();
    let read = rt.dom_changes_reader();
    assert!(read().unattributed, "первое чтение не знает базиса");
    rt.eval(&format!(
        "for (var i = 0; i < {}; i++) document.body.appendChild(document.createElement('p'));",
        crate::v8_runtime::runtime::CHILD_EDIT_CAP + 1
    ))
    .unwrap();
    assert_eq!(changes_of(&read, body), vec![OwnedNodeChange::ChildList]);
}

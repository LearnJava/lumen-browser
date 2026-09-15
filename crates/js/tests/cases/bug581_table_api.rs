//! BUG-581 — `HTMLTableElement`/`HTMLTableSectionElement`/`HTMLTableRowElement`
//! were bare interface stubs generated only so `instanceof` resolves, with
//! zero table-specific members (HTML LS §4.9.11). This covers the additive
//! API surface: `rows`/`tBodies`/`caption`/`tHead`/`tFoot`,
//! `create*`/`delete*`, `insertRow`/`deleteRow`, `insertCell`/`deleteCell`,
//! `rowIndex`/`sectionRowIndex`/`cellIndex`.
#![cfg(feature = "v8-backend")]

use std::sync::{Arc, Mutex};

use lumen_core::JsRuntime;
use lumen_dom::Document;
use lumen_js::v8_runtime::V8JsRuntime;

fn make_rt() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    let doc = Arc::new(Mutex::new(Document::new()));
    rt.install_dom(doc, "https://example.com/doc", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, script: &str) -> bool {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(b)) => b,
        Ok(other) => panic!("expected bool from script, got {other:?}: {script}"),
        Err(e) => panic!("eval error: {e} for {script}"),
    }
}

#[test]
fn rows_spans_thead_middle_tfoot_in_spec_order() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var head = table.appendChild(document.createElement('thead'));
var headRow = head.appendChild(document.createElement('tr'));
var bodyRow = table.appendChild(document.createElement('tr'));
var body = table.appendChild(document.createElement('tbody'));
var bodyRow2 = body.appendChild(document.createElement('tr'));
var foot = table.appendChild(document.createElement('tfoot'));
var footRow = foot.appendChild(document.createElement('tr'));
table.rows instanceof HTMLCollection
  && table.rows.length === 4
  && table.rows[0] === headRow
  && table.rows[1] === bodyRow
  && table.rows[2] === bodyRow2
  && table.rows[3] === footRow
"#
    ));
}

#[test]
fn rows_ignores_non_tr_and_nested_table_children() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
table.appendChild(document.createElement('p'));
var tr = table.appendChild(document.createElement('tr'));
var td = tr.appendChild(document.createElement('td'));
var nested = td.appendChild(document.createElement('table'));
nested.appendChild(document.createElement('tr'));
table.rows.length === 1 && table.rows[0] === tr
"#
    ));
}

#[test]
fn tbodies_is_live_and_direct_children_only() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var b1 = table.appendChild(document.createElement('tbody'));
table.rows; // touch rows first, tBodies must still be independently live
var b2 = table.appendChild(document.createElement('tbody'));
table.tBodies.length === 2 && table.tBodies[0] === b1 && table.tBodies[1] === b2
"#
    ));
}

#[test]
fn caption_getter_setter_and_create_delete() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
table.appendChild(document.createTextNode(' '));
var c1 = table.createCaption();
var ok1 = table.firstChild === c1 && table.caption === c1;
var c2 = table.createCaption();
var ok2 = c2 === c1; // does not create a second one
table.deleteCaption();
var ok3 = table.caption === null;
var c3 = document.createElement('caption');
table.caption = c3;
var ok4 = table.caption === c3 && table.firstChild === c3;
table.caption = null;
var ok5 = table.caption === null;
ok1 && ok2 && ok3 && ok4 && ok5
"#
    ));
}

#[test]
fn thead_setter_rejects_wrong_interface_and_wrong_local_name() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var threw_type = false, threw_hierarchy = false;
try { table.tHead = document.createElement('div'); }
catch (e) { threw_type = e instanceof TypeError; }
try { table.tHead = document.createElement('tbody'); }
catch (e) { threw_hierarchy = e.name === 'HierarchyRequestError'; }
threw_type && threw_hierarchy
"#
    ));
}

#[test]
fn thead_insertion_point_skips_caption_and_colgroup() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var caption = table.appendChild(document.createElement('caption'));
var colgroup = table.appendChild(document.createElement('colgroup'));
var body = table.appendChild(document.createElement('tbody'));
var thead = table.createTHead();
thead.previousSibling === colgroup && thead.nextSibling === body
"#
    ));
}

#[test]
fn tfoot_create_appends_and_setter_replaces_in_place() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var body = table.appendChild(document.createElement('tbody'));
var tfoot = table.createTFoot();
var ok1 = tfoot.previousSibling === body && tfoot.nextSibling === null;
var replacement = document.createElement('tfoot');
table.tFoot = replacement;
var ok2 = replacement.previousSibling === body && replacement.nextSibling === null
  && table.getElementsByTagName('tfoot').length === 1;
ok1 && ok2
"#
    ));
}

#[test]
fn create_tbody_always_mints_new_and_inserts_after_last_tbody() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var head = table.appendChild(document.createElement('thead'));
var b1 = table.appendChild(document.createElement('tbody'));
var foot = table.appendChild(document.createElement('tfoot'));
var b2 = table.createTBody();
b2 !== b1 && b2.previousSibling === b1 && b2.nextSibling === foot
"#
    ));
}

#[test]
fn insert_row_creates_tbody_when_table_has_no_rows() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var tr = table.insertRow();
var body = tr.parentNode;
(body.tagName === 'TBODY') && (body.parentNode === table) && (table.rows.length === 1)
"#
    ));
}

#[test]
fn insert_row_inserts_at_index_within_existing_rows() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var body = table.appendChild(document.createElement('tbody'));
var first = body.appendChild(document.createElement('tr'));
var second = body.appendChild(document.createElement('tr'));
var mid = table.insertRow(1);
table.rows[0] === first && table.rows[1] === mid && table.rows[2] === second
"#
    ));
}

#[test]
fn insert_row_and_delete_row_bounds_check() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var threw_neg2 = false, threw_over = false, threw_delete = false;
try { table.insertRow(-2); } catch (e) { threw_neg2 = e.name === 'IndexSizeError'; }
try { table.insertRow(1); } catch (e) { threw_over = e.name === 'IndexSizeError'; }
try { table.deleteRow(0); } catch (e) { threw_delete = e.name === 'IndexSizeError'; }
threw_neg2 && threw_over && threw_delete
"#
    ));
}

#[test]
fn delete_row_minus_one_removes_last_row_and_is_noop_when_empty() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var body = table.appendChild(document.createElement('tbody'));
var first = body.appendChild(document.createElement('tr'));
var second = body.appendChild(document.createElement('tr'));
table.deleteRow(-1);
var ok1 = table.rows.length === 1 && table.rows[0] === first;
table.deleteRow(-1);
var ok2 = table.rows.length === 0;
table.deleteRow(-1); // no-op, must not throw
var ok3 = table.rows.length === 0;
ok1 && ok2 && ok3
"#
    ));
}

#[test]
fn section_insert_row_and_delete_row_operate_on_own_rows_only() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var tbody = document.createElement('tbody');
var first = tbody.appendChild(document.createElement('tr'));
var mid = tbody.insertRow(0);
var ok1 = tbody.rows.length === 2 && tbody.rows[0] === mid && tbody.rows[1] === first;
tbody.deleteRow(-1);
var ok2 = tbody.rows.length === 1 && tbody.rows[0] === mid;
ok1 && ok2
"#
    ));
}

#[test]
fn row_index_spans_the_whole_table_none_outside_one() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var head = table.appendChild(document.createElement('thead'));
var headRow = head.appendChild(document.createElement('tr'));
var bodyRow = table.appendChild(document.createElement('tr'));
var orphan = document.createElement('div').appendChild(document.createElement('tr'));
headRow.rowIndex === 0 && bodyRow.rowIndex === 1 && orphan.rowIndex === -1
"#
    ));
}

#[test]
fn section_row_index_uses_middle_bucket_for_table_direct_children() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var table = document.createElement('table');
var t1 = table.appendChild(document.createElement('tr'));
var t2 = table.appendChild(document.createElement('tr'));
var body = table.appendChild(document.createElement('tbody'));
var b1 = body.appendChild(document.createElement('tr'));
t1.sectionRowIndex === 0 && t2.sectionRowIndex === 1 && b1.sectionRowIndex === 0
"#
    ));
}

#[test]
fn insert_cell_and_delete_cell_and_cell_index() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var tr = document.createElement('tr');
var td0 = tr.insertCell();
var td1 = tr.insertCell();
var mid = tr.insertCell(1);
var ok1 = tr.cells.length === 3 && tr.cells[0] === td0 && tr.cells[1] === mid && tr.cells[2] === td1;
var ok2 = mid.cellIndex === 1 && document.createElement('td').cellIndex === -1;
tr.deleteCell(0);
var ok3 = tr.cells.length === 2 && tr.cells[0] === mid;
var threw = false;
try { tr.deleteCell(5); } catch (e) { threw = e.name === 'IndexSizeError'; }
ok1 && ok2 && ok3 && threw
"#
    ));
}

#[test]
fn cells_ignore_non_direct_and_non_td_th_children() {
    let rt = make_rt();
    assert!(bool_eval(
        &rt,
        r#"
var tr = document.createElement('tr');
var td = tr.appendChild(document.createElement('td'));
var div = tr.appendChild(document.createElement('div'));
div.appendChild(document.createElement('td'));
var th = tr.appendChild(document.createElement('th'));
tr.cells.length === 2 && tr.cells[0] === td && tr.cells[1] === th
"#
    ));
}

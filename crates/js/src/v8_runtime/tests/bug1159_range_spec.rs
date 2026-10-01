//! BUG-1159: Range сравнивает граничные точки по порядку в дереве и
//! выполняет clone/extract/insert/surround по DOM §5.5.

use super::*;

const SETUP: &str = "document.body.innerHTML = '<p id=a>0123456789</p><p id=b>abcdef</p>';";

fn eval_str(js: &str) -> JsValue {
    let rt = runtime_with_dom(make_doc(), "https://example.test/");
    rt.eval(SETUP).unwrap();
    rt.eval(js).unwrap()
}

#[test]
fn compare_boundary_points_uses_tree_order() {
    let js = "var a = document.getElementById('a').firstChild, b = document.getElementById('b').firstChild;
        var r1 = document.createRange(); r1.setStart(a, 5); r1.setEnd(a, 6);
        var r2 = document.createRange(); r2.setStart(b, 0); r2.setEnd(b, 1);
        [r1.compareBoundaryPoints(Range.START_TO_START, r2), r2.compareBoundaryPoints(Range.START_TO_START, r1),
         r1.compareBoundaryPoints(Range.END_TO_START, r2)].join()";
    assert_eq!(eval_str(js), JsValue::String("-1,1,-1".into()));
}

#[test]
fn compare_boundary_points_rejects_bad_how() {
    let js = "var r = document.createRange();
        try { r.compareBoundaryPoints(4, r); 'no throw' } catch (e) { e.name }";
    assert_eq!(eval_str(js), JsValue::String("NotSupportedError".into()));
}

#[test]
fn compare_point_and_is_point_in_range() {
    let js = "var a = document.getElementById('a').firstChild;
        var r = document.createRange(); r.setStart(a, 2); r.setEnd(a, 6);
        [r.comparePoint(a, 1), r.comparePoint(a, 4), r.comparePoint(a, 8),
         r.isPointInRange(a, 6), r.isPointInRange(a, 7)].join()";
    assert_eq!(eval_str(js), JsValue::String("-1,0,1,true,false".into()));
}

#[test]
fn extract_contents_splits_text_and_collapses() {
    let js = "var a = document.getElementById('a').firstChild;
        var r = document.createRange(); r.setStart(a, 2); r.setEnd(a, 6);
        var f = r.extractContents();
        [f.textContent, a.data, r.collapsed, r.startOffset].join()";
    assert_eq!(eval_str(js), JsValue::String("2345,016789,true,2".into()));
}

#[test]
fn insert_node_splits_text() {
    let js = "var p = document.getElementById('a'), a = p.firstChild;
        var r = document.createRange(); r.setStart(a, 4); r.collapse(true);
        var s = document.createElement('i'); r.insertNode(s);
        p.innerHTML";
    assert_eq!(eval_str(js), JsValue::String("0123<i></i>456789".into()));
}

#[test]
fn surround_contents_wraps_selection() {
    let js = "var p = document.getElementById('a'), a = p.firstChild;
        var r = document.createRange(); r.setStart(a, 2); r.setEnd(a, 4);
        r.surroundContents(document.createElement('b'));
        p.innerHTML";
    assert_eq!(eval_str(js), JsValue::String("01<b>23</b>456789".into()));
}

#[test]
fn set_start_validates_offset() {
    let js = "var a = document.getElementById('a').firstChild;
        var r = document.createRange();
        try { r.setStart(a, 99); 'no throw' } catch (e) { e.name }";
    assert_eq!(eval_str(js), JsValue::String("IndexSizeError".into()));
}

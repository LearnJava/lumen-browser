//! `Range.getClientRects()`/`getBoundingClientRect()` and the `[object DOMRectList]` tag (BUG-1558),
//! against real layout through the same-tick flush.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with(html: &str) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("body { font-family: monospace; }")));
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(&format!("document.getElementById('main').innerHTML = {html:?};")).unwrap();
    rt
}

fn check(rt: &V8JsRuntime, script: &str) {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(true)) => {}
        other => panic!("{other:?} for {script}"),
    }
}

#[test]
fn rect_lists_carry_their_interface_tag() {
    let rt = rt_with("<span id=\"s\">abc</span>");
    check(
        &rt,
        "var s = document.getElementById('s'), r = new Range(); r.selectNodeContents(s);
         var tag = Object.prototype.toString;
         tag.call(s.getClientRects()) === '[object DOMRectList]'
           && tag.call(r.getClientRects()) === '[object DOMRectList]'
           && tag.call(r.getClientRects().item(0)) === '[object DOMRect]'
           && tag.call(new DOMRectReadOnly()) === '[object DOMRectReadOnly]'",
    );
}

#[test]
fn range_over_text_reports_the_selected_slice_only() {
    let rt = rt_with("<span id=\"s\">0123456789</span>");
    check(
        &rt,
        "var s = document.getElementById('s'), t = s.firstChild, full = s.getBoundingClientRect();
         var cw = full.width / 10;
         var r = new Range(); r.setStart(t, 2); r.setEnd(t, 5);
         var rs = r.getClientRects(), b = r.getBoundingClientRect();
         rs.length === 1 && Math.abs(rs[0].x - (full.x + 2 * cw)) < 0.01 && Math.abs(rs[0].width - 3 * cw) < 0.01
           && rs[0].height === full.height && b.x === rs[0].x && b.width === rs[0].width",
    );
}

#[test]
fn range_over_two_lines_reports_a_rect_per_line() {
    let rt = rt_with("<span id=\"s\">0123456789</span><br><span id=\"u\">0123456789</span>");
    check(
        &rt,
        "var t1 = document.getElementById('s').firstChild, t2 = document.getElementById('u').firstChild;
         var r = new Range(); r.setStart(t1, 3); r.setEnd(t2, 4);
         var rs = r.getClientRects();
         var b = r.getBoundingClientRect();
         rs.length >= 2 && rs[1].y > rs[0].y && b.height >= rs[1].bottom - rs[0].top - 0.01",
    );
}

#[test]
fn collapsed_range_is_a_zero_width_rect_and_empty_range_is_empty() {
    let rt = rt_with("<span id=\"s\">0123456789</span>");
    check(
        &rt,
        "var t = document.getElementById('s').firstChild;
         var r = new Range(); r.setStart(t, 4); r.setEnd(t, 4);
         var rs = r.getClientRects();
         rs.length === 1 && rs[0].width === 0 && rs[0].height > 0",
    );
}

#[test]
fn surrogate_pair_boundaries_snap_to_the_whole_character() {
    let rt = rt_with("<div id=\"d\">\u{1F320}a\u{1F320}</div>");
    check(
        &rt,
        "var t = document.getElementById('d').firstChild;
         var full = new Range(); full.setStart(t, 0); full.setEnd(t, 5);
         var r = new Range(); r.setStart(t, 1); r.setEnd(t, 4);
         full.getClientRects().length === 1 && r.getClientRects().length === 1
           && r.getClientRects()[0].width === full.getClientRects()[0].width",
    );
}

#[test]
fn fixed_offset_parent_is_the_containing_block_ancestor_or_null() {
    let rt = rt_with(
        "<div id=\"tr\" style=\"transform:translateX(10px)\"><div><div id=\"c\" style=\"position:fixed\"></div></div></div>\
         <div id=\"m\" style=\"margin:10px\"><div id=\"b\" style=\"position:fixed\"></div></div>\
         <div id=\"nb\" style=\"display:none\"></div>",
    );
    check(
        &rt,
        "var g = function(i) { return document.getElementById(i); };
         g('c').offsetParent === g('tr') && g('b').offsetParent === null && g('nb').offsetParent === null",
    );
}

#[test]
fn every_body_element_has_a_null_offset_parent() {
    let rt = rt_with("<div id=\"p\" style=\"position:relative\"></div>");
    check(
        &rt,
        "var b2 = document.createElement('body'), b3 = document.createElement('body');
         document.getElementById('p').append(b2); b2.append(b3);
         document.body.offsetParent === null && b2.offsetParent === null && b3.offsetParent === null",
    );
}


#[test]
fn perspective_and_preserve_3d_contain_fixed_descendants_and_boxless_has_no_offset_parent() {
    let rt = rt_with(
        "<div id=\"pe\" style=\"perspective:10px\"><div><div id=\"pc\" style=\"position:fixed\"></div></div></div>\
         <div id=\"ts\" style=\"transform-style:preserve-3d\"><div><div id=\"tc\" style=\"position:fixed\"></div></div></div>\
         <div id=\"nn\" style=\"display:none\"><a id=\"na\"></a></div>",
    );
    check(
        &rt,
        "var g = function(i) { return document.getElementById(i); };
         g('pc').offsetParent === g('pe') && g('tc').offsetParent === g('ts') && g('na').offsetParent === null
           && getComputedStyle(g('pe')).getPropertyValue('perspective') === '10px'",
    );
}

#[test]
fn text_inside_a_contained_element_is_reported_besides_the_element_itself() {
    let rt = rt_with(
        "<div id=\"c\"><div id=\"a\">Hello</div><div id=\"m\" style=\"width:0;height:0\"><div style=\"position:absolute\">Fire</div></div><div id=\"z\">World</div></div>",
    );
    check(
        &rt,
        "var g = function(i) { return document.getElementById(i); };
         var r = new Range(); r.setStart(g('a').firstChild, 0); r.setEnd(g('z').firstChild, 5);
         var rs = r.getClientRects();
         rs.length === 4",
    );
}

#[test]
fn soft_hyphens_take_no_width_in_a_range_rect() {
    let rt = rt_with("<div id=\"d\" style=\"width:100px\">12\u{ad}456</div>");
    check(
        &rt,
        "var t = document.getElementById('d').firstChild;
         var r = function(a, b) { var q = new Range(); q.setStart(t, a); q.setEnd(t, b); return q.getBoundingClientRect(); };
         var full = r(0, 6);
         full.width > 0 && r(2, 3).width === 0 && Math.abs(r(0, 2).width + r(3, 6).width - full.width) < 0.01",
    );
}

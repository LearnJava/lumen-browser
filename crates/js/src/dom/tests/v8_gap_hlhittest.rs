//! GAP-HLHITTEST — `CSS.highlights.highlightsFromPoint()` hit-tests against
//! real layout geometry through the same-tick flush
//! (`lumen_layout::text_geometry` + `_lumen_text_at_point`). Transcribes the
//! assertions of the vendored
//! `css/css-highlight-api/HighlightRegistry-highlightsFromPoint.html` and
//! `-ranges.html` that do not need an iframe.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn rt_with_text() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("body { font-family: monospace; }")));
    rt.update_viewport_size(800.0, 600.0);
    rt.eval(
        "var main = document.getElementById('main');
         main.innerHTML = '<span id=\"s1\">0123456789</span><br><span id=\"s2\">0123456789</span>';
         var t1 = document.getElementById('s1').firstChild;
         var t2 = document.getElementById('s2').firstChild;
         var rect = document.getElementById('s1').getBoundingClientRect();
         var cw = rect.width / 10, ch = rect.height;
         var cy = rect.top + ch / 2;
         function mk(n1, o1, n2, o2) { var r = new Range(); r.setStart(n1, o1); r.setEnd(n2, o2); return r; }
         function hit(x, y) { return CSS.highlights.highlightsFromPoint(x, y); }",
    )
    .unwrap();
    rt
}

fn check(rt: &V8JsRuntime, script: &str) {
    match rt.eval(script) {
        Ok(lumen_core::JsValue::Bool(true)) => {}
        other => panic!("{other:?} for {script}"),
    }
}

#[test]
fn text_is_laid_out_for_the_fixture() {
    let rt = rt_with_text();
    check(&rt, "rect.width > 0 && ch > 0");
}

#[test]
fn overlapping_highlights_sort_by_priority_then_reverse_registration() {
    let rt = rt_with_text();
    check(
        &rt,
        "var r1 = mk(t1, 2, t1, 10), r2 = mk(t1, 5, t1, 10);
         var h1 = new Highlight(r1), h2 = new Highlight(r2);
         CSS.highlights.set('a', h1); CSS.highlights.set('b', h2);
         var none = hit(rect.left + cw * 0.5, cy);
         var one = hit(rect.left + 3 * cw, cy);
         var two = hit(rect.left + 7 * cw, cy);
         h1.priority = 2; h2.priority = 1;
         var swapped = hit(rect.left + 7 * cw, cy);
         hit(rect.left - 1, rect.top - 1).length === 0
           && none.length === 0
           && one.length === 1 && one[0].highlight === h1 && one[0].ranges[0] === r1
           && two.length === 2 && two[0].highlight === h2 && two[1].highlight === h1
           && swapped[0].highlight === h1 && swapped[1].highlight === h2",
    );
}

#[test]
fn multi_line_range_hits_on_both_lines_in_highlight_order() {
    let rt = rt_with_text();
    check(
        &rt,
        "var small = mk(t1, 5, t1, 10);
         var big = new StaticRange({startContainer: t1, startOffset: 2, endContainer: t2, endOffset: 8});
         var h = new Highlight(small, big);
         CSS.highlights.set('m', h);
         var both = hit(rect.left + 7 * cw, cy);
         var line2 = hit(rect.left + cw, rect.top + 1.5 * ch);
         both.length === 1 && both[0].ranges.length === 2
           && both[0].ranges[0] === small && both[0].ranges[1] === big
           && hit(rect.left + 12 * cw, cy).length === 0
           && line2.length === 1 && line2[0].ranges.length === 1 && line2[0].ranges[0] === big
           && hit(rect.left + 9 * cw, rect.top + 1.5 * ch).length === 0
           && hit(rect.left + 5 * cw, rect.top + 3 * ch).length === 0",
    );
}

#[test]
fn collapsed_invalid_and_out_of_viewport_ranges_never_hit() {
    let rt = rt_with_text();
    check(
        &rt,
        "var collapsed = mk(t1, 5, t1, 5);
         var backwards = new StaticRange({startContainer: t1, startOffset: 8, endContainer: t1, endOffset: 1});
         var overlong = new StaticRange({startContainer: t1, startOffset: 0, endContainer: t1, endOffset: 99});
         CSS.highlights.set('c', new Highlight(collapsed, backwards, overlong));
         var full = mk(t1, 0, t1, 10);
         CSS.highlights.set('f', new Highlight(full));
         var r = hit(rect.left + 5 * cw, cy);
         r.length === 1 && r[0].ranges.length === 1 && r[0].ranges[0] === full
           && hit(801, cy).length === 0 && hit(rect.left + cw, 601).length === 0",
    );
}

#[test]
fn same_tick_dom_mutation_is_seen_by_the_hit_test() {
    let rt = rt_with_text();
    check(
        &rt,
        "var extra = document.createElement('span');
         extra.textContent = 'temporary';
         document.body.appendChild(extra);
         var et = extra.firstChild;
         var er = extra.getBoundingClientRect();
         var h = new Highlight(new StaticRange({startContainer: et, startOffset: 0, endContainer: et, endOffset: 9}));
         CSS.highlights.set('t', h);
         var before = hit(er.left + er.width / 2, er.top + er.height / 2);
         document.body.removeChild(extra);
         before.length === 1 && hit(er.left + er.width / 2, er.top + er.height / 2).length === 0",
    );
}

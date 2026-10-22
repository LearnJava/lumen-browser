//! CSSOM-11 (BUG-1456) — `CSSPseudoElement` and `Element.prototype.pseudo()`: stable identity,
//! the type grammar, and the box geometry of generated pseudo-elements read from layout.

use super::v8_bug935_s55_content_journal::{page, runtime, JOURNAL_SWITCH};
use super::*;

const SHEET: &str = "body { margin: 0; }
    #container { position: absolute; left: 100px; top: 50px; width: 300px; height: 200px; }
    #target { position: absolute; left: 50px; top: 30px; width: 100px; height: 80px; }
    #target::before { content: ''; display: block; position: absolute; left: 10px; top: 5px;
                      width: 30px; height: 20px; }
    #reference { position: absolute; left: 200px; top: 100px; width: 50px; height: 50px; }
    #boxed { position: absolute; left: 100px; top: 400px; width: 200px; height: 100px;
             padding: 10px; border: 5px solid black; margin: 20px; }
    #boxed::before { content: ''; display: block; width: 50px; height: 30px; padding: 5px;
                     border: 2px solid blue; margin: 3px; }
    #boxed.wide::before { width: 100px; }
    #boxed::after { content: 'after'; display: block; width: 80px; height: 40px; }
    #hidden::before { content: 'x'; display: none; }
    #invisible::before { content: 'i'; visibility: hidden; display: block; width: 40px; height: 20px; }
    #floated::before { content: 'f'; float: left; width: 60px; height: 40px; }
    #scaled::before { content: 's'; display: block; width: 100px; height: 50px; transform: scale(2); }";

/// Evaluates `body` (statements, ends with `return …`) in a page built from [`SHEET`] and returns
/// its string result.
fn eval(body: &str) -> String {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let rt = runtime(page());
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(SHEET)));
    let script = format!(
        "(function() {{
            var c = document.createElement('div'); c.id = 'container';
            c.innerHTML = '<div id=\"target\"></div><div id=\"reference\"></div>';
            document.body.appendChild(c);
            ['boxed', 'hidden', 'invisible', 'floated', 'scaled', 'plain'].forEach(function(id) {{
                var d = document.createElement('div'); d.id = id; document.body.appendChild(d);
            }});
            var $ = function(id) {{ return document.getElementById(id); }};
            {body}
        }})()"
    );
    match rt.eval(&script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

#[test]
fn identity_type_and_parent() {
    let out = eval(
        "var t = $('target'), p = t.pseudo('::before');
         return [typeof CSSPseudoElement, p instanceof CSSPseudoElement, p.type, p.element === t,
                 p.parent === t, p.selectorText, p === t.pseudo('::before'),
                 p === t.pseudo('::after'), $('plain').pseudo('::before') === p].join();",
    );
    assert_eq!(out, "function,true,::before,true,true,::before,true,false,false");
}

#[test]
fn unsupported_and_malformed_types_answer_null() {
    let out = eval(
        "var t = $('target');
         return ['::first-line', '::first-letter', 'before', ':before', '::nope', '', '::selection',
                 '::view-transition-group(x)', '::view-transition'].map(function(s) {
                     return String(t.pseudo(s));
                 }).join();",
    );
    // `::view-transition` is the root element's pseudo-element only.
    assert_eq!(out, "null,null,null,null,null,null,null,null,null");
}

#[test]
fn view_transition_chain_hangs_from_the_root() {
    let out = eval(
        "var vt = document.documentElement.pseudo('::view-transition');
         var g = vt.pseudo('::view-transition-group(a)');
         var ip = g.pseudo('::view-transition-image-pair(a)');
         var o = ip.pseudo('::view-transition-old(a)');
         return [vt.type, vt.parent === document.documentElement, g.type, g.parent === vt,
                 g.selectorText, ip.parent === g, o.type, o.parent === ip,
                 String(vt.pseudo('::view-transition-old(a)')), g === vt.pseudo('::view-transition-group(a)')]
                .join();",
    );
    assert_eq!(
        out,
        "::view-transition,true,::view-transition-group,true,::view-transition-group(a),true,\
         ::view-transition-old,true,null,true"
    );
}

#[test]
fn box_quads_per_box_type() {
    let out = eval(
        "var p = $('boxed').pseudo('::before');
         return ['border', 'content', 'padding', 'margin'].map(function(b) {
             var q = p.getBoxQuads({ box: b });
             var r = q[0].getBounds();
             return q.length + ':' + r.width + 'x' + r.height;
         }).join();",
    );
    assert_eq!(out, "1:64x44,1:50x30,1:60x40,1:70x50");
}

#[test]
fn box_quads_default_is_the_border_box_and_instances_are_dom_quads() {
    let out = eval(
        "var q = $('boxed').pseudo('::before').getBoxQuads();
         var r = $('boxed').pseudo('::before').getBoundingClientRect();
         return [Array.isArray(q), q.length, q[0] instanceof DOMQuad, r instanceof DOMRect,
                 r.width, r.height, q[0].getBounds().x === r.x].join();",
    );
    assert_eq!(out, "true,1,true,true,64,44,true");
}

#[test]
fn a_pseudo_without_a_box_has_no_geometry() {
    let out = eval(
        "return ['hidden', 'plain'].map(function(id) {
             var p = $(id).pseudo('::before');
             return [p === null, p.getBoxQuads().length, p.getClientRects().length,
                     p.getBoundingClientRect().width].join(':');
         }).join();",
    );
    assert_eq!(out, "false:0:0:0,false:0:0:0");
}

#[test]
fn visibility_hidden_and_float_keep_their_boxes() {
    let out = eval(
        "return ['invisible', 'floated'].map(function(id) {
             var q = $(id).pseudo('::before').getBoxQuads();
             var b = q[0].getBounds();
             return q.length + ':' + b.width + 'x' + b.height;
         }).join();",
    );
    assert_eq!(out, "1:40x20,1:60x40");
}

#[test]
fn a_transformed_pseudo_reports_the_transformed_extent() {
    let out = eval(
        "var b = $('scaled').pseudo('::before').getBoxQuads()[0].getBounds();
         return b.width + 'x' + b.height;",
    );
    assert_eq!(out, "200x100");
}

#[test]
fn invalid_box_type_throws() {
    let out = eval(
        "try { $('boxed').pseudo('::before').getBoxQuads({ box: 'outer' }); return 'no'; }
         catch (e) { return e.name; }",
    );
    assert_eq!(out, "TypeError");
}

#[test]
fn convert_point_maps_between_coordinate_spaces() {
    let out = eval(
        "var p = $('target').pseudo('::before');
         var a = p.convertPointFromNode({ x: 0, y: 0 }, $('target'));
         var d = p.convertPointFromNode({ x: 0, y: 0 }, document);
         var e = p.convertPointFromNode({ x: 0, y: 0 }, $('reference'));
         var z = p.convertPointFromNode({ x: 10, y: 20, z: 30, w: 1.5 }, $('target'));
         return [a instanceof DOMPoint, a.x, a.y, d.x, d.y, e.x, e.y, z.x, z.y, z.z, z.w].join();",
    );
    // `#target` is at (150, 80) in the page, its ::before 10px/5px inside it; `#reference` at
    // (300, 150).
    assert_eq!(out, "true,-10,-5,-160,-85,140,65,0,15,30,1.5");
}

#[test]
fn convert_rect_and_quad_return_dom_quads() {
    let out = eval(
        "var p = $('target').pseudo('::before');
         var r = p.convertRectFromNode(new DOMRect(0, 0, 10, 10), $('target'));
         var q = p.convertQuadFromNode({ p1: { x: 0, y: 0 }, p2: { x: 10, y: 0 }, p3: { x: 10, y: 10 },
                                         p4: { x: 0, y: 10 } }, $('target'));
         return [r instanceof DOMQuad, r.p1.x, r.p1.y, r.p3.x, q instanceof DOMQuad, q.p4.x, q.p4.y].join();",
    );
    assert_eq!(out, "true,-10,-5,0,true,-10,5");
}

#[test]
fn convert_from_a_pseudo_without_a_box_answers_null() {
    let out = eval(
        "var p = $('target').pseudo('::before'), n = $('plain').pseudo('::before');
         return String(p.convertPointFromNode({ x: 1, y: 1 }, n)) + ',' + String(n.convertPointFromNode({ x: 1, y: 1 }, p));",
    );
    assert_eq!(out, "null,null");
}

#[test]
fn box_quads_relative_to_another_node() {
    let out = eval(
        "var p = $('target').pseudo('::before');
         var rel = p.getBoxQuads({ relativeTo: $('target') })[0].getBounds();
         var doc = p.getBoxQuads({ relativeTo: document })[0].getBounds();
         var vp = p.getBoxQuads()[0].getBounds();
         return [rel.x, rel.y, doc.x, doc.y, vp.x, vp.y].join();",
    );
    assert_eq!(out, "10,5,160,85,160,85");
}

#[test]
fn geometry_follows_a_style_change() {
    let out = eval(
        "var p = $('boxed').pseudo('::before');
         var before = p.getBoundingClientRect().width;
         $('boxed').className = 'wide';
         return before + ',' + p.getBoundingClientRect().width;",
    );
    assert_eq!(out, "64,114");
}

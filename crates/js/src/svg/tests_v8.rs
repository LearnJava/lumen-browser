//! V8-тесты SVG DOM API (`svg.rs`).

// Хелперы тестового модуля: исключение из clippy.toml покрывает
// только тело `#[test]` (docs/lint-policy.md §10).
#![allow(clippy::unwrap_used)]
use lumen_core::ext::JsRuntime as _;
use lumen_core::JsValue;

use crate::v8_runtime::V8JsRuntime;

/// Install minimal DOM stubs then SVG bindings.
fn with_svg() -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval(r#"
            var window = globalThis;
            // Minimal Element stub (SVGElement extends it)
            class Element {
                constructor() {
                    this.attributes = {};
                    this.children = [];
                    this.childNodes = [];
                }
                getAttribute(n) { return this.attributes[n] || null; }
                setAttribute(n, v) { this.attributes[n] = v; }
                removeAttribute(n) { delete this.attributes[n]; }
                hasAttribute(n) { return n in this.attributes; }
                appendChild(c) { this.children.push(c); return c; }
                addEventListener() {}
                removeEventListener() {}
                dispatchEvent() { return true; }
            }
            window.Element = Element;
            // Minimal document with createElementNS
            var document = {
                createElementNS: function(ns, tag) { return new Element(); }
            };
            globalThis.document = document;
            window.document = document;
        "#).unwrap();
    super::install_svg_bindings_v8(&rt).unwrap();
    rt
}

fn bool_eval(rt: &V8JsRuntime, expr: &str) -> bool {
    matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
}

#[test]
fn svg_element_class_exists() {
    let rt = with_svg();
    assert!(bool_eval(&rt, "typeof window.SVGElement === 'function'"));
}

#[test]
fn bug_1092_missing_svg_globals_exist() {
    let rt = with_svg();
    for n in [
        "SVGAngle", "SVGNumber", "SVGNumberList", "SVGLengthList", "SVGAnimatedAngle",
        "SVGAnimatedNumberList", "SVGAnimatedLengthList", "SVGUnitTypes", "SVGAElement",
        "SVGMPathElement", "SVGFETurbulenceElement", "SVGFEDropShadowElement",
    ] {
        assert!(bool_eval(&rt, &format!("typeof window.{n} === 'function'")), "{n}");
    }
}

#[test]
fn bug_1093_value_types_have_webidl_shape() {
    let rt = with_svg();
    assert!(bool_eval(&rt, r#"
            const throwsType = f => { try { f(); return false; } catch (e) { return e instanceof TypeError; } };
            const len = new SVGSVGElement().createSVGLength();
            const d = Object.getOwnPropertyDescriptor(SVGLength.prototype, 'value');
            len.valueAsString = '2cm';
            const list = new SVGSVGElement().createSVGTransform();
            Object.keys(globalThis).indexOf('SVGLength') < 0
              && d.enumerable && d.get.name === 'get value' && d.set.name === 'set value'
              && throwsType(() => d.get.call(SVGLength.prototype))
              && !Object.prototype.hasOwnProperty.call(len, 'value')
              && SVGLength.SVG_LENGTHTYPE_PX === 5 && SVGLength.prototype.SVG_LENGTHTYPE_PX === 5
              && Object.getOwnPropertyDescriptor(SVGLength.prototype, 'newValueSpecifiedUnits').enumerable
              && SVGLength.prototype.newValueSpecifiedUnits.length === 2
              && throwsType(() => len.newValueSpecifiedUnits())
              && throwsType(() => new SVGLength())
              && len.unitType === SVGLength.SVG_LENGTHTYPE_CM
              && Math.abs(len.value - 96 / 2.54 * 2) < 1e-9
              && Object.prototype.toString.call(len) === '[object SVGLength]'
              && Object.getOwnPropertyDescriptor(SVGTransform.prototype, 'type').set === undefined
              && list.type === SVGTransform.SVG_TRANSFORM_MATRIX
        "#));
}

#[test]
fn bug_1093_list_operations_live_on_each_interface_prototype() {
    let rt = with_svg();
    assert!(bool_eval(&rt, r#"
            const sl = new SVGSVGElement().createSVGTransform();
            const names = ['SVGNumberList', 'SVGLengthList', 'SVGStringList', 'SVGPointList', 'SVGTransformList'];
            names.every(n => ['getItem', 'appendItem', 'clear', 'numberOfItems', 'length'].every(
              m => Object.prototype.hasOwnProperty.call(window[n].prototype, m)))
              && SVGStringList.prototype.getItem.length === 1
              && SVGStringList.prototype.insertItemBefore.length === 2
              && SVGTransformList.prototype.consolidate.length === 0
        "#));
}

#[test]
fn bug_1092_factories_return_typed_instances() {
    let rt = with_svg();
    assert!(bool_eval(&rt, r#"
            const svg = new SVGSVGElement();
            const n = svg.createSVGNumber(), a = svg.createSVGAngle();
            n.value = '2.5';
            a.valueAsString = '1rad';
            n instanceof SVGNumber && n.value === 2.5 && a instanceof SVGAngle
              && a.unitType === SVGAngle.SVG_ANGLETYPE_RAD
              && Math.abs(a.value - 180 / Math.PI) < 1e-9
              && SVGUnitTypes.SVG_UNIT_TYPE_OBJECTBOUNDINGBOX === 2
        "#));
}

#[test]
fn svg_svg_element_class_exists() {
    let rt = with_svg();
    assert!(bool_eval(&rt, "typeof window.SVGSVGElement === 'function'"));
}

#[test]
fn svg_graphics_element_get_bbox_returns_rect() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const el = new SVGGraphicsElement();
            const bb = el.getBBox();
            bb instanceof SVGRect && bb.x === 0 && bb.y === 0 &&
            bb.width === 0 && bb.height === 0
        "#);
    assert!(ok);
}

#[test]
fn svg_rect_element_has_dimensions() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const r = new SVGRectElement();
            r.x instanceof SVGAnimatedLength &&
            r.y instanceof SVGAnimatedLength &&
            r.width instanceof SVGAnimatedLength &&
            r.height instanceof SVGAnimatedLength
        "#);
    assert!(ok);
}

#[test]
fn svg_circle_element_has_cx_cy_r() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const c = new SVGCircleElement();
            c.cx instanceof SVGAnimatedLength &&
            c.cy instanceof SVGAnimatedLength &&
            c.r  instanceof SVGAnimatedLength
        "#);
    assert!(ok);
}

#[test]
fn svg_path_element_has_get_total_length() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const p = new SVGPathElement();
            typeof p.getTotalLength === 'function' && p.getTotalLength() === 0
        "#);
    assert!(ok);
}

#[test]
fn svg_svg_element_create_svg_rect() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const svg = new SVGSVGElement();
            const r = svg.createSVGRect();
            r instanceof SVGRect
        "#);
    assert!(ok);
}

#[test]
fn svg_svg_element_create_svg_point() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const svg = new SVGSVGElement();
            const p = svg.createSVGPoint();
            p instanceof SVGPoint && p.x === 0 && p.y === 0
        "#);
    assert!(ok);
}

#[test]
fn svg_matrix_multiply_identity() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const a = new SVGMatrix();
            const b = new SVGMatrix();
            const c = a.multiply(b);
            c instanceof SVGMatrix && c.a === 1 && c.d === 1 &&
            c.b === 0 && c.c === 0 && c.e === 0 && c.f === 0
        "#);
    assert!(ok);
}

#[test]
fn svg_transform_set_translate() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const t = new SVGSVGElement().createSVGTransform();
            t.setTranslate(10, 20);
            t.type === 2 && t.matrix.e === 10 && t.matrix.f === 20
        "#);
    assert!(ok);
}

#[test]
fn svg_create_element_ns_returns_typed_element() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const el = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
            el instanceof SVGCircleElement
        "#);
    assert!(ok);
}

#[test]
fn svg_create_element_ns_svg_returns_svg_svg_element() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const el = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
            el instanceof SVGSVGElement
        "#);
    assert!(ok);
}

#[test]
fn svg_create_element_ns_unknown_tag_returns_svg_element() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const el = document.createElementNS('http://www.w3.org/2000/svg', 'unknown-tag');
            el instanceof SVGElement
        "#);
    assert!(ok);
}

#[test]
fn svg_point_matrix_transform() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const p = new SVGPoint(3, 4);
            const m = new SVGMatrix(2, 0, 0, 2, 1, 1); // scale(2) + translate(1,1)
            const p2 = p.matrixTransform(m);
            p2.x === 7 && p2.y === 9
        "#);
    assert!(ok);
}

#[test]
fn svg_length_unit_types() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            SVGLength.SVG_LENGTHTYPE_NUMBER === 1 &&
            SVGLength.SVG_LENGTHTYPE_PX     === 5 &&
            SVGLength.SVG_LENGTHTYPE_PERCENTAGE === 2
        "#);
    assert!(ok);
}

#[test]
fn svg_animated_transform_list_consolidate() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const atl = new SVGRectElement().transform;
            const t = new SVGSVGElement().createSVGTransform();
            t.setTranslate(5, 0);
            atl.baseVal.appendItem(t);
            const c = atl.baseVal.consolidate();
            c instanceof SVGTransform && c.matrix.e === 5
        "#);
    assert!(ok);
}

#[test]
fn svg_linear_gradient_element_x1_x2() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const lg = new SVGLinearGradientElement();
            lg.x1 instanceof SVGAnimatedLength &&
            lg.x2 instanceof SVGAnimatedLength &&
            lg.x2.baseVal.value === 100
        "#);
    assert!(ok);
}

#[test]
fn svg_filter_element_exists() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const f = new SVGFilterElement();
            typeof f.filterUnits === 'object' && f.tagName === 'filter'
        "#);
    assert!(ok);
}

#[test]
fn svg_text_element_get_number_of_chars() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            const t = new SVGTextElement();
            t.getNumberOfChars() === 0 && t.getComputedTextLength() === 0
        "#);
    assert!(ok);
}

#[test]
fn svg_classes_on_window() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            typeof window.SVGRectElement       === 'function' &&
            typeof window.SVGCircleElement     === 'function' &&
            typeof window.SVGPathElement       === 'function' &&
            typeof window.SVGLineElement       === 'function' &&
            typeof window.SVGPolygonElement    === 'function' &&
            typeof window.SVGPolylineElement   === 'function' &&
            typeof window.SVGTextElement       === 'function' &&
            typeof window.SVGGElement          === 'function' &&
            typeof window.SVGDefsElement       === 'function' &&
            typeof window.SVGUseElement        === 'function' &&
            typeof window.SVGImageElement      === 'function' &&
            typeof window.SVGClipPathElement   === 'function' &&
            typeof window.SVGMaskElement       === 'function' &&
            typeof window.SVGLinearGradientElement === 'function' &&
            typeof window.SVGRadialGradientElement === 'function' &&
            typeof window.SVGFilterElement     === 'function' &&
            typeof window.SVGMarkerElement     === 'function'
        "#);
    assert!(ok);
}

// ── GAP-SMIL ─────────────────────────────────────────────────────────

#[test]
fn svg_smil_class_hierarchy() {
    let rt = with_svg();
    let ok = bool_eval(&rt, r#"
            (new SVGAnimateElement()) instanceof SVGAnimationElement &&
            (new SVGSetElement()) instanceof SVGAnimationElement &&
            (new SVGAnimateTransformElement()) instanceof SVGAnimationElement &&
            (new SVGAnimateMotionElement()) instanceof SVGAnimationElement &&
            typeof SVGAnimationElement.prototype.beginElement === 'function' &&
            typeof SVGAnimationElement.prototype.beginElementAt === 'function' &&
            typeof SVGAnimationElement.prototype.endElement === 'function' &&
            typeof SVGAnimationElement.prototype.endElementAt === 'function'
        "#);
    assert!(ok);
}

/// Installs the SMIL-capable native stubs `_lumen_tick_smil` needs
/// (`_lumen_get_attr`/`_lumen_u2n`/`_lumen_dispatch`,
/// `document.getElementsByTagName('*')`) on top of `with_svg()`, then
/// builds one `<animate>`-shaped node with `__nid__ = 1` reachable
/// through that stub. Attributes are set via `_lumen_smil_set_attr`
/// (a test-only helper, not a real DOM method) to avoid re-implementing
/// attribute reflection in the stub.
fn with_smil_node(local: &str, attrs: &[(&str, &str)]) -> V8JsRuntime {
    let rt = with_svg();
    let attrs_js: String = attrs
        .iter()
        .map(|(k, v)| format!("{k:?}:{v:?}"))
        .collect::<Vec<_>>()
        .join(",");
    rt.eval(&format!(
        r#"
            class Event {{
                constructor(type, opts) {{
                    this.type = type;
                    this.bubbles = !!(opts && opts.bubbles);
                    this.cancelable = !!(opts && opts.cancelable);
                }}
            }}
            window.Event = Event;
            __lumen_C._lumen_smil_attrs = {{1: {{{attrs_js}}}}};
            __lumen_C._lumen_get_attr = function(nid, attr) {{
                var a = __lumen_C._lumen_smil_attrs[nid];
                return (a && Object.prototype.hasOwnProperty.call(a, attr)) ? a[attr] : undefined;
            }};
            __lumen_C._lumen_u2n = function(v) {{ return v === undefined ? null : v; }};
            __lumen_C._lumen_dispatch_log = [];
            __lumen_C._lumen_dispatch = function(nid, event) {{ __lumen_C._lumen_dispatch_log.push(event.type); return true; }};
            var node = new (_lumen_svg_ctor_for_local({local:?}))();
            node.__nid__ = 1;
            __lumen_C._lumen_smil_node = node;
            var _allEls = [node];
            document.getElementsByTagName = function(tag) {{ return _allEls; }};
            "#
    ))
    .unwrap();
    rt
}

#[test]
fn svg_smil_full_and_partial_clock_values() {
    let cases = [
        ("00:00:01.50", "1.5"), ("00:01.50", "1.5"), ("00:30:01", "1801"),
        ("101:00:01", "363601"), ("2min", "120"), ("1h", "3600"), ("500ms", "0.5"),
    ];
    for (raw, secs) in cases {
        let rt = with_smil_node("animate", &[("attributeName", "x"), ("dur", raw)]);
        assert!(
            bool_eval(&rt, &format!("_lumen_smil_node.getSimpleDuration() === {secs}")),
            "dur={raw}"
        );
    }
    for bad in ["01:99:01", "99:01", "00:59:59.", "00:59:9.9", "00:59:.9", "00:59:009", ":30:01", "01::01", "5:30"] {
        let rt = with_smil_node("animate", &[("attributeName", "x"), ("dur", bad)]);
        assert!(bool_eval(&rt, "(function(){ try { _lumen_smil_node.getSimpleDuration(); return false; } catch (e) { return true; } })()"), "dur={bad}");
    }
}

#[test]
fn svg_smil_begin_end_events_and_numeric_interpolation() {
    // `<animate attributeName="width" begin="0s" dur="2s" from="0" to="100">`
    let rt = with_smil_node(
        "animate",
        &[
            ("attributeName", "width"),
            ("begin", "0s"),
            ("dur", "2s"),
            ("from", "0"),
            ("to", "100"),
        ],
    );
    // t = 0s: begin fires, value starts at "0".
    rt.eval("_lumen_tick_smil(0.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('beginEvent') !== -1"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === '0'"));

    // t = 1s: halfway through the 2s duration, linear interpolation.
    rt.eval("_lumen_tick_smil(1.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === '50'"));

    // t = 2s: duration elapsed, endEvent fires, fill="remove" (default)
    // clears the override.
    rt.eval("_lumen_tick_smil(2.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === undefined"));
}

#[test]
fn svg_smil_fill_freeze_keeps_value_after_end() {
    let rt = with_smil_node(
        "set",
        &[("attributeName", "visibility"), ("begin", "0s"), ("to", "visible"), ("end", "1s"), ("fill", "freeze")],
    );
    rt.eval("_lumen_tick_smil(0.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|visibility'] === 'visible'"));
    rt.eval("_lumen_tick_smil(1.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|visibility'] === 'visible'"));
}

#[test]
fn svg_smil_indefinite_begin_waits_for_begin_element_call() {
    let rt = with_smil_node("set", &[("attributeName", "width"), ("begin", "indefinite"), ("to", "100")]);
    rt.eval("_lumen_tick_smil(5.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.length === 0"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === undefined"));
    rt.eval("__lumen_C._lumen_smil_node.beginElement(); _lumen_tick_smil(5.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('beginEvent') !== -1"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|width'] === '100'"));
}

#[test]
fn svg_smil_repeat_count_fires_repeat_event() {
    let rt = with_smil_node(
        "animate",
        &[("attributeName", "x"), ("begin", "0s"), ("dur", "1s"), ("repeatCount", "3"), ("to", "10")],
    );
    rt.eval("_lumen_tick_smil(0.0);").unwrap();
    rt.eval("_lumen_tick_smil(1.5);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('repeatEvent') !== -1"));
    // Still active (2nd of 3 cycles) — no endEvent yet.
    assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') === -1"));
    rt.eval("_lumen_tick_smil(3.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1"));
}

#[test]
fn svg_smil_seek_silently_skips_past_intervals_and_dispatches_transitions() {
    let rt = with_smil_node(
        "set",
        &[("attributeName", "fill"), ("begin", "5s"), ("dur", "1s"), ("repeatCount", "2"), ("to", "green"), ("fill", "freeze")],
    );
    rt.eval(
        r#"
            var svg = new (_lumen_svg_ctor_for_local("svg"))();
            svg.pauseAnimations();
            _lumen_tick_smil(100.0);
            svg.setCurrentTime(10);
            _lumen_tick_smil(100.1);
            "#,
    )
    .unwrap();
    // Interval 5..7 lies wholly before t=10: no events, freeze value kept.
    assert!(bool_eval(&rt, "_lumen_dispatch_log.length === 0"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|fill'] === 'green'"));
    rt.eval("svg.setCurrentTime(5.5); _lumen_tick_smil(100.2);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.join() === 'beginEvent'"));
    // Seeking into the second cycle: no repeatEvent, no new events.
    rt.eval("svg.setCurrentTime(6.5); _lumen_tick_smil(100.3);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.join() === 'beginEvent'"));
    rt.eval("svg.setCurrentTime(1); _lumen_tick_smil(100.4);").unwrap();
    assert!(bool_eval(&rt, "_lumen_dispatch_log.join() === 'beginEvent,endEvent'"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|fill'] === undefined"));
}

#[test]
fn svg_smil_min_max_clamp_active_duration() {
    // dur 1s, fill freeze: `max=0.5s` ends at 0.5s, `min=3s` at 3s.
    for (attr, v, end_tick, still_active) in [("max", "0.5s", 0.6, false), ("min", "3s", 2.0, true)] {
        let rt = with_smil_node(
            "animate",
            &[("attributeName", "x"), ("begin", "0s"), ("dur", "1s"), ("to", "10"), (attr, v)],
        );
        rt.eval("_lumen_tick_smil(0.0); _lumen_tick_smil(0.0);").unwrap();
        rt.eval(&format!("_lumen_tick_smil({end_tick});")).unwrap();
        let ended = bool_eval(&rt, "_lumen_dispatch_log.indexOf('endEvent') !== -1");
        assert_eq!(ended, !still_active, "{attr}={v}");
    }
}

#[test]
fn svg_smil_repeat_n_syncbase_reacts_to_that_iteration_only() {
    let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "indefinite"), ("to", "1")]);
    rt.eval(
        r#"
            __lumen_C._lumen_smil_attrs[2] = {attributeName: "y", begin: "a.repeat(2)", to: "7"};
            var b = new (_lumen_svg_ctor_for_local("set"))();
            b.__nid__ = 2;
            _allEls.push(b);
            var a = __lumen_C._lumen_smil_node;
            var listeners = {};
            a.addEventListener = function(n, f) { listeners[n] = f; };
            document.getElementById = function(id) { return id === 'a' ? a : null; };
            _lumen_tick_smil(0.0);
            listeners.repeatEvent({ detail: 1 });
            _lumen_tick_smil(1.0);
            "#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === undefined"));
    rt.eval("listeners.repeatEvent({ detail: 2 }); _lumen_tick_smil(2.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === '7'"));
}

#[test]
fn svg_smil_syncbase_end_plus_offset_and_event_begin() {
    let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "0s"), ("dur", "1s"), ("to", "10")]);
    rt.eval(
        r#"
            var a = __lumen_C._lumen_smil_node;
            __lumen_C._lumen_smil_attrs[2] = {attributeName: "y", begin: "a.end+1s", to: "7"};
            var b = new (_lumen_svg_ctor_for_local("set"))();
            b.__nid__ = 2;
            _allEls.push(b);
            __lumen_C._lumen_smil_attrs[3] = {attributeName: "z", begin: "a.fooEvent", to: "9"};
            var c = new (_lumen_svg_ctor_for_local("set"))();
            c.__nid__ = 3;
            _allEls.push(c);
            var listeners = {};
            a.addEventListener = function(n, f) { listeners[n] = f; };
            document.getElementById = function(id) { return id === 'a' ? a : null; };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(1.0);
            "#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === undefined"));
    rt.eval("_lumen_tick_smil(1.5);").unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === undefined"));
    rt.eval("_lumen_tick_smil(2.0);").unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === '7'"));
    // Event-based begin waits for the listener to fire.
    assert!(bool_eval(&rt, "_lumen_smil_overrides['3|z'] === undefined"));
    rt.eval("listeners.fooEvent(); _lumen_tick_smil(2.5);").unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['3|z'] === '9'"));
}

#[test]
fn svg_smil_restart_end_syncbase_and_cycle() {
    // `begin="0;2s"` dur 5s: the second begin instance restarts the first
    // interval at 2s (restart=always), so a begin/end pair fires at 2s.
    let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "0;2s"), ("dur", "5s"), ("from", "0"), ("to", "10")]);
    rt.eval(
        r#"
            var a = __lumen_C._lumen_smil_node;
            var log = [];
            a.addEventListener = function(n, f) {};
            _lumen_dispatch = function(nid, ev) { log.push(nid + ':' + ev.type); };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(3.0);
            "#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "log.join() === '1:beginEvent,1:endEvent,1:beginEvent'"));
    assert!(bool_eval(&rt, "_lumen_smil_overrides['1|x'] === '2'"));
    // Cyclic syncbase (`a.begin` ↔ `b.begin`) must not hang the sweep.
    let rt = with_smil_node("animate", &[("attributeName", "x"), ("begin", "b.begin; 0s"), ("dur", "1s"), ("to", "1")]);
    rt.eval(
        r#"
            var a = __lumen_C._lumen_smil_node;
            __lumen_C._lumen_smil_attrs[2] = {attributeName: "y", begin: "a.begin", dur: "1s", to: "7"};
            var b = new (_lumen_svg_ctor_for_local("animate"))();
            b.__nid__ = 2;
            _allEls.push(b);
            document.getElementById = function(id) { return id === 'a' ? a : id === 'b' ? b : null; };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(0.5);
            "#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "_lumen_smil_overrides['2|y'] === '7'"));
}

#[test]
fn svg_smil_huge_repeat_count_is_unspecified() {
    let big = format!("1{}", "0".repeat(300));
    let rt = with_smil_node("animate", &[("attributeName", "fill"), ("begin", "0s"), ("dur", "10ms"), ("from", "#007f00"), ("to", "green"), ("fill", "freeze"), ("repeatCount", big.as_str())]);
    rt.eval(
        r#"
            var log = [];
            _lumen_dispatch = function(nid, ev) { log.push(ev.type); };
            _lumen_tick_smil(0.0);
            _lumen_tick_smil(0.5);
            "#,
    )
    .unwrap();
    assert!(bool_eval(&rt, "log.join() === 'beginEvent,endEvent'"));
}

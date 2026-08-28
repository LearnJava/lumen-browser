//! BUG-387 — `element.computedStyleMap()` (CSS Typed OM L1 §6.1) used to be
//! the same class as the mutable `attributeStyleMap` and inherited its
//! reader, so it answered `undefined` for every property that came from a
//! stylesheet rule instead of from the inline `style=""` attribute. The two
//! maps now have separate readers: the computed one goes to the same
//! cascade snapshots `getComputedStyle` uses.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`].
fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn main_nid(rt: &V8JsRuntime) -> u32 {
    match rt.eval("document.getElementById('main').__nid__").unwrap() {
        lumen_core::JsValue::Number(n) => n as u32,
        other => panic!("unexpected nid: {other:?}"),
    }
}

/// Publishes a cascade snapshot for `#main` — the fixture element carries
/// no inline `style` attribute, so anything the map answers can only have
/// come from the cascade.
fn publish_cascade(rt: &V8JsRuntime, props: &[(&str, &str)]) {
    let nid = main_nid(rt);
    let inner: std::collections::HashMap<String, String> =
        props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, inner);
    rt.update_computed_styles(outer);
}

fn s(rt: &V8JsRuntime, code: &str) -> String {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::String(v) => v,
        other => panic!("{code}: expected a string, got {other:?}"),
    }
}

fn b(rt: &V8JsRuntime, code: &str) -> bool {
    match rt.eval(code).unwrap() {
        lumen_core::JsValue::Bool(v) => v,
        other => panic!("{code}: expected a bool, got {other:?}"),
    }
}

/// The reported symptom, verbatim: a colour set by a `<style>` rule read
/// back as `undefined`, so `.toString()` threw.
#[test]
fn get_reads_the_cascade_not_the_inline_attribute() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("color", "rgb(0, 128, 0)")]);
    assert_eq!(
        s(&rt, "document.getElementById('main').computedStyleMap().get('color').toString()"),
        "rgb(0, 128, 0)"
    );
}

/// The gate for the inversion itself: the two maps must not answer from
/// the same place. `#main` has no inline style, so the mutable map has
/// nothing while the computed one has the cascade.
#[test]
fn attribute_map_and_computed_map_have_separate_readers() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("color", "rgb(0, 128, 0)")]);
    assert!(b(
        &rt,
        "document.getElementById('main').attributeStyleMap.get('color') === undefined"
    ));
    assert!(b(
        &rt,
        "document.getElementById('main').computedStyleMap().get('color') !== undefined"
    ));
}

/// …and the mutable map keeps reflecting the inline attribute it owns,
/// even when the cascade says something else for the same property.
#[test]
fn attribute_map_still_reflects_inline_style() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("color", "rgb(0, 128, 0)")]);
    rt.eval("document.getElementById('main').attributeStyleMap.set('color', 'red')")
        .unwrap();
    assert_eq!(
        s(&rt, "document.getElementById('main').attributeStyleMap.get('color').toString()"),
        "red"
    );
    assert_eq!(
        s(&rt, "document.getElementById('main').getAttribute('style')"),
        "color: red"
    );
}

/// camelCase spelling is the same property, as it is for `getComputedStyle`.
#[test]
fn get_accepts_camel_case_spelling() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("font-size", "16px")]);
    assert_eq!(
        s(&rt, "document.getElementById('main').computedStyleMap().get('fontSize').toString()"),
        "16px"
    );
}

/// A property the cascade snapshot does not carry is `undefined`, not an
/// empty value object — `has()` agrees.
#[test]
fn absent_property_is_undefined() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("color", "rgb(0, 128, 0)")]);
    assert!(b(
        &rt,
        "document.getElementById('main').computedStyleMap().get('font-weight') === undefined"
    ));
    assert!(b(
        &rt,
        "document.getElementById('main').computedStyleMap().has('font-weight') === false"
    ));
    assert!(b(
        &rt,
        "document.getElementById('main').computedStyleMap().has('color') === true"
    ));
}

/// Custom properties live in their own inherited snapshot (BUG-732) and
/// the computed map has to ask for them there.
#[test]
fn custom_property_comes_from_its_own_snapshot() {
    let rt = v8_runtime_with_dom(make_doc());
    let nid = main_nid(&rt);
    let mut inner = std::collections::HashMap::new();
    inner.insert("--gap".to_string(), "8px".to_string());
    let mut outer = std::collections::HashMap::new();
    outer.insert(nid, Arc::new(inner));
    rt.update_custom_properties(outer);
    assert_eq!(
        s(&rt, "document.getElementById('main').computedStyleMap().get('--gap').toString()"),
        "8px"
    );
}

/// A custom property name is case-sensitive: folding it to kebab-case
/// (what the shared reader did) turned `--Foo` into `---foo`.
#[test]
fn custom_property_name_is_not_case_folded() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval("document.getElementById('main').attributeStyleMap.set('--Foo', '3px')")
        .unwrap();
    assert_eq!(
        s(&rt, "document.getElementById('main').getAttribute('style')"),
        "--Foo: 3px"
    );
    assert_eq!(
        s(&rt, "document.getElementById('main').attributeStyleMap.get('--Foo').toString()"),
        "3px"
    );
}

/// Iteration used to be dead — and dead in a way that threw, since the
/// shim called `.entries()` on the string the native returned.
#[test]
fn computed_map_iterates_the_whole_cascade() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("color", "rgb(0, 128, 0)"), ("display", "block")]);
    assert!(b(&rt, "document.getElementById('main').computedStyleMap().size === 2"));
    assert_eq!(
        s(
            &rt,
            "Array.from(document.getElementById('main').computedStyleMap().keys()).join(',')"
        ),
        // Property-sorted, so the order does not drift between runs.
        "color,display"
    );
    // `iterable<USVString, sequence<CSSStyleValue>>` — the value is a sequence.
    assert_eq!(
        s(
            &rt,
            r#"
                    var out = [];
                    for (var e of document.getElementById('main').computedStyleMap()) {
                        out.push(e[0] + '=' + e[1][0].toString());
                    }
                    out.join('|')
                    "#
        ),
        "color=rgb(0, 128, 0)|display=block"
    );
}

/// The mutable map iterates its own inline declarations, not the cascade.
#[test]
fn attribute_map_iterates_inline_declarations() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("color", "rgb(0, 128, 0)"), ("display", "block")]);
    rt.eval("document.getElementById('main').attributeStyleMap.set('width', '4px')")
        .unwrap();
    assert_eq!(
        s(
            &rt,
            "Array.from(document.getElementById('main').attributeStyleMap.keys()).join(',')"
        ),
        "width"
    );
}

/// `forEach` hands out `(sequence, property, map)` in that order.
#[test]
fn for_each_yields_value_key_map() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[("display", "block")]);
    assert_eq!(
        s(
            &rt,
            r#"
                    var m = document.getElementById('main').computedStyleMap(), seen = [];
                    m.forEach(function(v, k, self) {
                        seen.push(k + '=' + v[0].toString() + ':' + (self === m));
                    });
                    seen.join('|')
                    "#
        ),
        "display=block:true"
    );
}

/// The read-only map is the *base* class now, so the mutable one is an
/// instance of it and not the other way round.
#[test]
fn inheritance_runs_in_the_spec_direction() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(b(
        &rt,
        "document.getElementById('main').attributeStyleMap instanceof StylePropertyMapReadOnly"
    ));
    assert!(b(
        &rt,
        "!(document.getElementById('main').computedStyleMap() instanceof StylePropertyMap)"
    ));
    // The computed map has no write surface at all — per §6.1 the
    // read-only interface simply does not declare `set`/`delete`.
    assert!(b(
        &rt,
        "typeof document.getElementById('main').computedStyleMap().set === 'undefined'"
    ));
}

/// Value wrapping: a dimension becomes a `CSSUnitValue` with the spec's
/// unit spelling, a bare identifier a `CSSKeywordValue`, and anything
/// else — a colour function, a list — a plain `CSSStyleValue` rather
/// than a keyword it is not.
#[test]
fn values_are_wrapped_by_shape() {
    let rt = v8_runtime_with_dom(make_doc());
    publish_cascade(&rt, &[
        ("margin-left", "-2.5px"),
        ("opacity", "0.5"),
        ("width", "50%"),
        ("display", "block"),
        ("color", "rgb(0, 128, 0)"),
    ]);
    let map = "document.getElementById('main').computedStyleMap()";
    assert!(b(&rt, &format!("{map}.get('margin-left') instanceof CSSUnitValue")));
    assert!(b(&rt, &format!("{map}.get('margin-left').value === -2.5")));
    assert!(b(&rt, &format!("{map}.get('margin-left').unit === 'px'")));
    assert!(b(&rt, &format!("{map}.get('opacity').unit === 'number'")));
    assert_eq!(s(&rt, &format!("{map}.get('opacity').toString()")), "0.5");
    assert!(b(&rt, &format!("{map}.get('width').unit === 'percent'")));
    assert_eq!(s(&rt, &format!("{map}.get('width').toString()")), "50%");
    assert!(b(&rt, &format!("{map}.get('display') instanceof CSSKeywordValue")));
    assert!(b(&rt, &format!("!({map}.get('color') instanceof CSSKeywordValue)")));
    assert!(b(&rt, &format!("{map}.get('color') instanceof CSSStyleValue")));
}

/// `CSSUnitValue.to()` used to relabel the number without converting it.
#[test]
fn unit_value_to_converts_within_a_unit_group() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(b(&rt, "new CSSUnitValue(96, 'px').to('in').value === 1"));
    assert!(b(&rt, "new CSSUnitValue(1, 's').to('ms').value === 1000"));
    assert!(b(&rt, "new CSSUnitValue(1, 'turn').to('deg').value === 360"));
    assert!(b(&rt, "new CSSUnitValue(7, 'px').to('px').value === 7"));
}

/// A conversion that needs a resolution context Lumen does not have here
/// throws, rather than answering with the unconverted number.
#[test]
fn unit_value_to_throws_on_undefined_conversion() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(b(
        &rt,
        r#"
                var threw = false;
                try { new CSSUnitValue(10, 'px').to('em'); } catch (e) { threw = e instanceof TypeError; }
                threw
                "#
    ));
}

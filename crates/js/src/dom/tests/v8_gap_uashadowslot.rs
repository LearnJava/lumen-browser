//! GAP-UASHADOWSLOT — `<select>`/`<details>` UA shadow tree with a `<slot>`
//! (HTML LS §15.5.4, WPT `html/rendering/widgets/shadow-dom.html`).
//!
//! The WPT test appends an *empty* span and sets `all: inherit`; the probes
//! here append a span with text and inherit `display`/`content-visibility`
//! one by one. Neither difference is about the shadow tree: an empty inline
//! element owns no box and so publishes no computed style, and the `all`
//! shorthand is not applied by the cascade (both tracked in ROADMAP
//! GAP-UASHADOWSLOT's remainder).

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn runtime(markup: &[(&str, &[&str])]) -> V8JsRuntime {
    runtime_with_css(markup, "")
}

fn runtime_with_css(markup: &[(&str, &[&str])], css: &str) -> V8JsRuntime {
    let mut doc = Document::new();
    let root = doc.root();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    doc.append_child(root, html);
    doc.append_child(html, body);
    for (i, (tag, bool_attrs)) in markup.iter().enumerate() {
        let el = doc.create_element(QualName::html(*tag));
        if let NodeData::Element { attrs, .. } = &mut doc.get_mut(el).data {
            attrs.push(lumen_dom::Attribute { name: QualName::html("id"), value: format!("e{i}") });
            for a in *bool_attrs {
                attrs.push(lumen_dom::Attribute { name: QualName::html(*a), value: String::new() });
            }
        }
        doc.append_child(body, el);
    }
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(Arc::new(Mutex::new(doc)), "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(css)));
    rt.update_viewport_size(800.0, 600.0);
    rt
}

/// For every `body > *`: append `<span>x</span>` inheriting `display` and
/// `content-visibility`, and report
/// `id:display/content-visibility` of the span as the slot hands them down
/// (`-` when the span publishes no computed style — not rendered).
const INHERITED: &str = "(function() {
    var out = [];
    document.querySelectorAll('body > *').forEach(function(el) {
        var child = el.appendChild(document.createElement('span'));
        child.textContent = 'x';
        child.style.display = 'inherit';
        child.style.contentVisibility = 'inherit';
        var cs = getComputedStyle(child);
        out.push(el.id + ':' + (cs.length ? cs.display + '/' + cs.contentVisibility : '-'));
        child.remove();
    });
    return out.join(' ');
})()";

fn eval_str(rt: &V8JsRuntime, src: &str) -> String {
    match rt.eval(src).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

#[test]
fn slotted_child_inherits_from_ua_slot() {
    let rt = runtime(&[
        ("select", &[]),
        ("select", &["multiple"]),
        ("details", &["open"]),
        ("details", &[]),
        ("video", &[]),
    ]);
    // `<select>`: the slot is `display: contents`. Open `<details>`: the
    // content slot is a block, content visible. Closed `<details>`: the slot
    // skips its contents, so the span is not rendered. `<video>`: no slot.
    assert_eq!(
        eval_str(&rt, INHERITED),
        "e0:contents/visible e1:contents/visible e2:block/visible e3:- e4:-"
    );
}

#[test]
fn closed_details_renders_summary_only_and_open_reveals_content() {
    let rt = runtime(&[("details", &[])]);
    let src = "(function() {
        var d = document.getElementById('e0');
        var s = d.appendChild(document.createElement('summary')); s.textContent = 'S';
        var p = d.appendChild(document.createElement('p')); p.textContent = 'P';
        var closed = getComputedStyle(s).length > 0 && getComputedStyle(p).length === 0;
        d.setAttribute('open', '');
        var open = getComputedStyle(p).length > 0 && getComputedStyle(p).display === 'block';
        d.removeAttribute('open');
        var reclosed = getComputedStyle(p).length === 0;
        return closed + ',' + open + ',' + reclosed;
    })()";
    assert_eq!(eval_str(&rt, src), "true,true,true");
}

#[test]
fn details_takes_first_summary_by_position_ignoring_slot_attribute() {
    let rt = runtime(&[("details", &[])]);
    // The second `<summary>` and a child naming a slot are ordinary content:
    // hidden while the element is closed.
    let src = "(function() {
        var d = document.getElementById('e0');
        var s1 = d.appendChild(document.createElement('summary')); s1.textContent = 'A';
        var s2 = d.appendChild(document.createElement('summary')); s2.textContent = 'B';
        var n = d.appendChild(document.createElement('div')); n.textContent = 'C';
        n.setAttribute('slot', 'anything');
        return [s1, s2, n].map(function(e) { return getComputedStyle(e).length > 0; }).join(',');
    })()";
    assert_eq!(eval_str(&rt, src), "true,false,false");
}

#[test]
fn ua_shadow_root_is_not_exposed_to_script() {
    let rt = runtime(&[("select", &[]), ("details", &[])]);
    let src = "(function() {
        return ['e0', 'e1'].map(function(id) {
            var el = document.getElementById(id);
            var clone = el.cloneNode(true);
            return String(el.shadowRoot) + '/' + String(clone.shadowRoot) + '/' + el.childNodes.length;
        }).join(' ');
    })()";
    assert_eq!(eval_str(&rt, src), "null/null/0 null/null/0");
}

#[test]
fn page_style_sheet_does_not_reach_ua_slots() {
    // CSS Scoping: a document rule never matches a node of a shadow tree, so
    // a page's `slot`/`*` rules leave the UA slots on their UA styles.
    let rt = runtime_with_css(
        &[("select", &[]), ("details", &["open"])],
        "slot { display: inline-block !important; content-visibility: hidden !important }",
    );
    assert_eq!(eval_str(&rt, INHERITED), "e0:contents/visible e1:block/visible");
}

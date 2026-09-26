//! BUG-1131 — `IntersectionObserverEntry` interface object (Intersection
//! Observer §2.3). Entries used to be plain object literals, so the global was
//! missing and duolingo's feature check redirected to its "not supported" page.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "https://example.test/", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

#[test]
fn interface_is_global_with_readonly_prototype_attributes() {
    let rt = v8_runtime_with_dom(make_doc());
    // The exact inline check duolingo runs before booting its app.
    assert!(is_true(&rt, r#""IntersectionObserver" in window && "IntersectionObserverEntry" in window
        && "intersectionRatio" in window.IntersectionObserverEntry.prototype
        && "isIntersecting" in window.IntersectionObserverEntry.prototype"#));
    for name in ["time", "rootBounds", "boundingClientRect", "intersectionRect",
                 "isIntersecting", "intersectionRatio", "target"] {
        assert!(is_true(&rt, &format!(
            "(function() {{ var d = Object.getOwnPropertyDescriptor(IntersectionObserverEntry.prototype, '{name}');
             return !!d && typeof d.get === 'function' && d.set === undefined && d.get.name === 'get {name}'; }})()"
        )), "{name}");
    }
    assert!(is_true(&rt, "(function() { try { IntersectionObserverEntry.prototype.time; return false; }
                                        catch (e) { return e instanceof TypeError; } })()"));
}

#[test]
fn constructor_converts_init_dictionary() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(r#"
        var e = new IntersectionObserverEntry({
            time: 5, rootBounds: null, target: document.body,
            boundingClientRect: { x: 1, y: 2, width: 3, height: 4 },
            intersectionRect: { x: 1, y: 2, width: 3, height: 2 },
            isIntersecting: true, intersectionRatio: 0.5,
        });
    "#).unwrap();
    assert!(is_true(&rt, "e instanceof IntersectionObserverEntry && e.time === 5 && e.rootBounds === null"));
    assert!(is_true(&rt, "e.boundingClientRect instanceof DOMRectReadOnly && e.boundingClientRect.bottom === 6"));
    assert!(is_true(&rt, "e.isIntersecting === true && e.intersectionRatio === 0.5 && e.target === document.body"));
    assert!(is_true(&rt, "Object.prototype.toString.call(e) === '[object IntersectionObserverEntry]'"));
    // Required members and the Element-typed target are enforced.
    assert!(is_true(&rt, "(function() { try { new IntersectionObserverEntry({}); return false; }
                                        catch (err) { return err instanceof TypeError; } })()"));
    assert!(is_true(&rt, "(function() { try { IntersectionObserverEntry({}); return false; }
                                        catch (err) { return err instanceof TypeError; } })()"));
}

#[test]
fn delivered_entries_are_interface_instances() {
    let doc_arc = make_doc();
    let nid = {
        let doc = doc_arc.lock().unwrap();
        super::find_element_by_tag(&doc, "body").unwrap().index() as u32
    };
    let rt = v8_runtime_with_dom(doc_arc);
    rt.update_layout_rects([(nid, [0.0, 0.0, 100.0, 50.0])].into_iter().collect());
    rt.update_viewport_size(1024.0, 720.0);
    rt.eval(r#"
        var _got = null;
        new IntersectionObserver(function(entries) { _got = entries[0]; }).observe(document.body);
        _lumen_deliver_intersection_observers();
    "#).unwrap();
    assert!(is_true(&rt, "_got instanceof IntersectionObserverEntry && _got.isIntersecting === true"));
    assert!(is_true(&rt, "_got.intersectionRatio === 1 && _got.target === document.body"));
    assert!(is_true(&rt, "_got.boundingClientRect instanceof DOMRectReadOnly && _got.boundingClientRect.width === 100"));
    assert!(is_true(&rt, "_got.rootBounds.width === 1024 && Object.keys(_got).length === 0"));
}

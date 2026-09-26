//! BUG-671 — `Selection` interface object (Selection API §3). The document's
//! selection used to be a plain object literal, so `window.Selection` did not
//! exist and `getSelection() instanceof Selection` threw.

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
fn get_selection_returns_instance_of_global_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    // The sanity check repeated at the top of the WPT `selection/` category.
    assert!(is_true(&rt, r#""Selection" in window && typeof Selection === "function"
        && getSelection() instanceof Selection && document.getSelection() === getSelection()
        && getSelection().constructor === Selection
        && Object.prototype.toString.call(getSelection()) === "[object Selection]""#));
    assert!(is_true(&rt, "(function() { try { new Selection(); return false; }
                                        catch (e) { return e instanceof TypeError; } })()"));
}

#[test]
fn members_are_branded_prototype_attributes_and_operations() {
    let rt = v8_runtime_with_dom(make_doc());
    for name in ["anchorNode", "anchorOffset", "focusNode", "focusOffset",
                 "isCollapsed", "rangeCount", "type"] {
        assert!(is_true(&rt, &format!(
            "(function() {{ var d = Object.getOwnPropertyDescriptor(Selection.prototype, '{name}');
             return !!d && typeof d.get === 'function' && d.set === undefined && d.enumerable
                 && d.get.name === 'get {name}'
                 && !Object.prototype.hasOwnProperty.call(getSelection(), '{name}'); }})()"
        )), "{name}");
    }
    for (name, len) in [("getRangeAt", 1), ("addRange", 1), ("removeRange", 1),
                        ("removeAllRanges", 0), ("empty", 0), ("collapse", 1),
                        ("setPosition", 1), ("collapseToStart", 0), ("collapseToEnd", 0),
                        ("extend", 1), ("setBaseAndExtent", 4), ("selectAllChildren", 1),
                        ("deleteFromDocument", 0), ("containsNode", 1), ("toString", 0)] {
        assert!(is_true(&rt, &format!(
            "(function() {{ var f = Selection.prototype.{name};
             return typeof f === 'function' && f.name === '{name}' && f.length === {len}; }})()"
        )), "{name}");
    }
    assert!(is_true(&rt, "(function() { try { Selection.prototype.rangeCount; return false; }
                                        catch (e) { return e instanceof TypeError; } })()"));
    assert!(is_true(&rt, "(function() { try { Selection.prototype.empty.call({}); return false; }
                                        catch (e) { return e instanceof TypeError; } })()"));
}

#[test]
fn selection_still_works_through_the_interface() {
    let rt = v8_runtime_with_dom(make_doc());
    assert!(is_true(&rt, r#"(function() {
        var s = getSelection(), b = document.body;
        s.setPosition(b, 0);
        if (s.rangeCount !== 1 || s.type !== "Caret" || s.anchorNode !== b) return false;
        s.removeAllRanges();
        return s.rangeCount === 0 && s.type === "None" && String(s) === "";
    })()"#));
}

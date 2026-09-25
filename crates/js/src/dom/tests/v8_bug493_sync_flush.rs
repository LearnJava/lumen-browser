//! CSSOM-4/BUG-493 — regression coverage for [`crate::v8_runtime::style_flush`]'s
//! synchronous flush-on-read: a script that mutates a node's style and reads
//! it back via `getComputedStyle`/`offsetWidth` in the SAME turn must see
//! the post-mutation value, not a stale or absent snapshot. See
//! `bugs/BUG-493-OPEN.md` for the full pre-fix symptom catalogue.

use super::*;
use crate::v8_runtime::V8JsRuntime;

/// V8 twin of [`super::runtime_with_dom`], plus the stylesheet/viewport push
/// `maybe_flush` needs to do anything ([`FlushHandles::maybe_flush`] is a
/// no-op without both — see `style_flush.rs`'s doc comment).
fn v8_runtime_with_flush(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse("#main { color: red; }")));
    rt.update_viewport_size(800.0, 600.0);
    rt
}

/// Pre-fix this returned `""` — `getComputedStyle` answered a snapshot
/// nothing had populated yet for a freshly styled node.
#[test]
fn get_computed_style_sees_same_tick_style_mutation() {
    let rt = v8_runtime_with_flush(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.style.width = '77px';
                return getComputedStyle(el).width;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String("77px".to_string()));
}

/// Same mechanism, the `offsetWidth`/layout-geometry accessor family
/// (срез 12 of BUG-493) rather than `getComputedStyle`.
#[test]
fn offset_width_sees_same_tick_style_mutation() {
    let rt = v8_runtime_with_flush(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.style.width = '123px';
                el.style.display = 'block';
                return el.offsetWidth;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::Number(123.0));
}

/// A read with no stylesheet pushed (worker/test contexts, or the shell's
/// not-yet-covered path per `style_flush.rs`) must stay on the pre-CSSOM-4
/// behaviour — `maybe_flush` degrades to a no-op rather than panicking.
#[test]
fn get_computed_style_without_pushed_stylesheet_stays_a_plain_cache_read() {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("globalThis._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(make_doc(), "", None, None, None, None, None, None, None, None, None, false)
        .unwrap();
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.style.width = '77px';
                return getComputedStyle(el).width;
            })()",
        )
        .unwrap();
    assert_eq!(r, lumen_core::JsValue::String(String::new()));
}

/// CSSOM-9 (BUG-472): `getComputedStyle` answers the *resolved* value — the
/// used px width of an auto-width block (800px viewport − 2×8px UA body
/// margin) and its percentage padding in px — while `computedStyleMap()`
/// keeps the *computed* `auto` / `%` (CSS Typed OM L1 §5.3), both off the
/// one snapshot.
#[test]
fn get_computed_style_resolves_geometry_but_typed_om_keeps_computed() {
    let rt = v8_runtime_with_flush(make_doc());
    let r = rt
        .eval(
            "(function() {
                var el = document.getElementById('main');
                el.style.paddingLeft = '10%';
                var cs = getComputedStyle(el);
                var map = el.computedStyleMap();
                var list = [];
                for (var p of cs) list.push(p);
                return [cs.width, cs.paddingLeft, String(map.get('width')),
                        String(map.get('padding-left')),
                        list.indexOf('width') >= 0, list.some(function(p) { return p.indexOf(':') >= 0; }),
                        cs.getPropertyValue('computed:width')].join('|');
            })()",
        )
        .unwrap();
    assert_eq!(
        r,
        lumen_core::JsValue::String("705.6px|78.4px|auto|10%|true|false|".to_string())
    );
}

//! XPath over the live Lumen DOM at page scale.
//!
//! `dedupeAndSort` in `xpath.rs` used to drop duplicates by comparing every
//! pair through the native `isSameNode` and to sort with native
//! `compareDocumentPosition`, and the `descendant` axis rebuilt its result by
//! recursive `concat`: all quadratic. A `document.evaluate('//…')` from an ad
//! script on w3schools.com held the engine thread for over 24 s and froze the
//! page. Node wrappers are created afresh on every access, so these tests run
//! on the real DOM (keyed by `__nid__`), not on the plain-object mock in
//! `xpath.rs`.

use super::*;
use crate::v8_runtime::V8JsRuntime;

fn v8_runtime_with_dom(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt
}

fn is_true(rt: &V8JsRuntime, code: &str) -> bool {
    rt.eval(code).unwrap() == lumen_core::JsValue::Bool(true)
}

/// 600 sections × (section, h2, p, span, two text nodes) ≈ 3 600 nodes: big
/// enough that the old pairwise dedupe took seconds, and every multi-step
/// result goes through the preorder-index sort (> 32 nodes).
const BUILD_LARGE: &str = r#"
    var host = document.getElementById('main');
    var html = '';
    for (var i = 0; i < 600; i++) {
        html += '<section data-i="' + i + '"><h2>T' + i + '</h2><p>x<span class="s">' + i + '</span></p></section>';
    }
    host.innerHTML = html;
"#;

#[test]
fn descendant_query_on_large_dom_is_complete_and_in_tree_order() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(BUILD_LARGE).unwrap();
    assert!(is_true(
        &rt,
        r#"(function() {
            var r = document.evaluate('//section/h2 | //span', document, null,
                XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
            if (r.snapshotLength !== 1200) return false;
            // Tree order interleaves the two branches of the union: h2 of
            // section i, then its span.
            for (var i = 0; i < 600; i++) {
                var h = r.snapshotItem(2 * i), s = r.snapshotItem(2 * i + 1);
                if (h.localName !== 'h2' || h.textContent !== 'T' + i) return false;
                if (s.localName !== 'span' || s.textContent !== String(i)) return false;
            }
            return true;
        })()"#
    ));
}

#[test]
fn duplicates_from_overlapping_steps_collapse_on_real_nodes() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(BUILD_LARGE).unwrap();
    // `//section//p/..` reaches every section once per descendant <p>; the
    // union with `//section` must still yield each section exactly once.
    assert!(is_true(
        &rt,
        r#"(function() {
            var r = document.evaluate('//section//p/.. | //section', document, null,
                XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
            if (r.snapshotLength !== 600) return false;
            return r.snapshotItem(0).getAttribute('data-i') === '0'
                && r.snapshotItem(599).getAttribute('data-i') === '599';
        })()"#
    ));
}

#[test]
fn attributes_sort_right_after_their_element() {
    let rt = v8_runtime_with_dom(make_doc());
    rt.eval(BUILD_LARGE).unwrap();
    assert!(is_true(
        &rt,
        r#"(function() {
            var r = document.evaluate('//section/@data-i | //section/h2', document, null,
                XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
            if (r.snapshotLength !== 1200) return false;
            for (var i = 0; i < 600; i++) {
                var a = r.snapshotItem(2 * i), h = r.snapshotItem(2 * i + 1);
                if (a.nodeType !== 2 || a.value !== String(i)) return false;
                if (h.localName !== 'h2') return false;
            }
            return true;
        })()"#
    ));
}

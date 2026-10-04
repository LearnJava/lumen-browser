//! BUG-935 срез 77 — the scroll-container list of an incremental flush folds the subtrees the flush
//! left alone in from the extents the previous flush kept for them, instead of walking every box
//! of the document. A kept extent is only sound while its subtree is unchanged, so the scenarios
//! change things inside and around such subtrees and compare every `scrollWidth`/`scrollHeight`
//! with a runtime that walks every box.

use super::v8_bug935_s55_content_journal::{page, runtime};
use crate::v8_runtime::V8JsRuntime;
use lumen_core::JsRuntime as _;

fn eval_string(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

/// A scroll container `#box` whose extent comes from the items under it, and a loop body (`probe`)
/// that appends and removes a `span` under `body` — the dirty root is `body`, the document under it
/// is left alone.
const SETUP: &str = "var box = document.createElement('div'); box.id = 'box';
    box.style.cssText = 'overflow:auto;width:200px;height:100px';
    document.body.appendChild(box);
    for (var i = 0; i < 6; i++) {
        var d = document.createElement('div'); d.className = 'item';
        d.innerHTML = '<p>item ' + i + ' <b>x</b></p><div class=\"in\" style=\"width:' + (100 + i * 40) + 'px;height:10px\"></div>';
        box.appendChild(d);
    }
    var items = Array.prototype.slice.call(box.children);
    function extent() { return box.scrollWidth + 'x' + box.scrollHeight; }
    function probe() {
        var s = document.createElement('span'); s.style.fontFamily = 'x' + Math.random();
        document.body.appendChild(s); extent(); document.body.removeChild(s); extent();
    }";

/// One scripted session: probes between edits of the subtrees the cache stands in for. Returns the
/// extents read after every step.
fn session(rt: &V8JsRuntime) -> String {
    rt.eval(&format!("(function() {{ {SETUP} extent(); window.__probe = probe; window.__items = items; window.__box = box; window.__extent = extent; }})()"))
        .unwrap();
    eval_string(
        rt,
        "var out = [];
         function step(f) { window.__probe(); window.__probe(); if (f) f(); out.push(window.__extent()); }
         step();
         // A subtree widened: the kept extent of its parent must not be served.
         step(function() { window.__items[2].lastChild.style.width = '600px'; });
         step();
         // ... and narrowed again.
         step(function() { window.__items[2].lastChild.style.width = '90px'; });
         // A subtree removed, and one appended with a box that sticks out to the left.
         step(function() { window.__box.removeChild(window.__items[5]); });
         step(function() {
             var n = document.createElement('div'); n.style.cssText = 'margin-left:-30px;width:700px;height:5px';
             window.__box.appendChild(n);
         });
         step();
         // The container itself resized.
         step(function() { window.__box.style.width = '350px'; });
         step();
         out.join('|')",
    )
}

#[test]
fn the_cache_changes_nothing_a_page_can_read() {
    let on = runtime(page());
    let off = runtime(page());
    off.set_scroll_rollup_off(true);
    let (with_cache, without) = (session(&on), session(&off));
    assert_eq!(with_cache, without, "a kept extent was served for a subtree that had changed");
    let (served, _) = on.scroll_rollup_counts();
    assert!(served > 0, "no flush took an extent from the cache: the scenario proved nothing");
    assert_eq!(off.scroll_rollup_counts(), (0, 0), "the switched-off runtime used the cache");
    // The widened item really showed: 6 items, the widest 600 px, so the container scrolls wider.
    assert!(with_cache.split('|').any(|s| s.starts_with("600x")), "{with_cache}");
}

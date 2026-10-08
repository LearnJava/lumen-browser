//! BUG-935 срез 74 — the restyle index the incremental flush builds from the stylesheet (a scan of
//! every selector in it) is kept between flushes for as long as the sheet's revision stays the
//! same, instead of being rebuilt by every flush of a loop that appends and removes one `span`.
//!
//! Reusing it is only sound when the sheet it was built from is the sheet in play, so the
//! scenarios also change the sheet underneath a kept index and check that the next flush answers
//! from the new one.

use super::v8_bug935_s55_content_journal::{page, runtime};
use crate::v8_runtime::V8JsRuntime;
use lumen_core::JsRuntime as _;

fn eval_string(rt: &V8JsRuntime, script: &str) -> String {
    match rt.eval(script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

const SETUP: &str = "var root = document.createElement('div'); root.id = 'root'; document.body.appendChild(root);
    for (var i = 0; i < 6; i++) {
        var d = document.createElement('div'); d.className = 'c';
        d.innerHTML = '<p>item ' + i + ' <b>x</b></p>';
        root.appendChild(d);
    }
    var els = Array.prototype.slice.call(root.children);
    function read() { getComputedStyle(els[0]).top; els[0].getBoundingClientRect(); }
    function probe() {
        var s = document.createElement('span'); s.style.fontFamily = 'x' + Math.random();
        document.body.appendChild(s); read(); document.body.removeChild(s); read();
    }";

#[test]
fn a_loop_of_flushes_over_one_sheet_scans_it_once() {
    let rt = runtime(page());
    rt.eval(&format!("(function() {{ {SETUP} read(); window.__probe = probe; }})()")).unwrap();
    let (flushes0, builds0) = (rt.incremental_flush_count(), rt.node_index_build_count());
    rt.eval("for (var k = 0; k < 8; k++) window.__probe();").unwrap();
    let flushes = rt.incremental_flush_count() - flushes0;
    let builds = rt.node_index_build_count() - builds0;
    assert!(flushes >= 12, "the loop did not go through the incremental flush ({flushes} flushes)");
    assert!(builds <= 1, "{builds} scans of the sheet for {flushes} flushes over one sheet");
}

#[test]
fn a_new_sheet_gets_its_own_index() {
    let rt = runtime(page());
    rt.eval(&format!("(function() {{ {SETUP} read(); window.__probe = probe; window.__els = els; }})()")).unwrap();
    rt.eval("window.__probe(); window.__probe();").unwrap();
    let kept = rt.node_index_build_count();
    // A rule that reads a class from an ancestor position: a stale index (built before the rule
    // existed) would call the `class` write below local to `.c`, and `p` would keep its colour.
    rt.eval(
        "var st = document.createElement('style'); st.textContent = '.on p { color: rgb(1, 2, 3); }';
         document.body.appendChild(st); window.__probe();",
    )
    .unwrap();
    assert!(rt.node_index_build_count() > kept, "a changed sheet reused the index built for the old one");
    let color = eval_string(
        &rt,
        "window.__els[2].className = 'c on'; var p = window.__els[2].firstChild;
         String(getComputedStyle(p).color)",
    );
    assert_eq!(color, "rgb(1, 2, 3)", "the rule added after the index was kept did not reach the descendant");
    // And the rule going away again.
    let color = eval_string(
        &rt,
        "window.__els[2].className = 'c'; var p = window.__els[2].firstChild; String(getComputedStyle(p).color)",
    );
    assert_ne!(color, "rgb(1, 2, 3)", "the descendant kept a colour from a rule whose class was removed");
}

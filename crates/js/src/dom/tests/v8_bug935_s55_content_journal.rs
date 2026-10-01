//! BUG-935 срез 55 — the same-tick flush reuses box subtrees on the strength of
//! the document's own content journal (`Document::take_content_journal`).
//!
//! The strict proof that the journal licenses only sound reuse is at layout
//! level (`box_build_driven_by_the_document_journal_matches_full_rebuild`). This
//! covers the wiring end to end: the same script, reading geometry after every
//! mutation (a chain of incremental flushes), runs once with the journal and
//! once with it switched off (`CONTENT_JOURNAL_DISABLED`, the pre-journal
//! `Untracked` behaviour), and the two must publish identical rects.
//!
//! Not compared against a single full layout on purpose: the scoped collectors
//! of BUG-1211 refresh only the dirty-root subtrees, so a sibling that merely
//! *moved* keeps its old rect in both modes (BUG-1238) — an independent defect
//! that would drown this signal.

use super::*;
use crate::v8_runtime::CONTENT_JOURNAL_DISABLED;
use crate::v8_runtime::V8JsRuntime;
use lumen_dom::{Document, QualName};
use std::sync::atomic::Ordering;

fn page() -> Arc<Mutex<Document>> {
    let mut doc = Document::new();
    let html = doc.create_element(QualName::html("html"));
    let body = doc.create_element(QualName::html("body"));
    doc.append_child(doc.root(), html);
    doc.append_child(html, body);
    Arc::new(Mutex::new(doc))
}

fn runtime(doc: Arc<Mutex<Document>>) -> V8JsRuntime {
    let rt = V8JsRuntime::new().unwrap();
    rt.eval("__lumen_C._LUMEN_EXTENSION_ACTIVE = true").unwrap();
    rt.install_dom(doc, "", None, None, None, None, None, None, None, None, None, false, None)
        .unwrap();
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "body { margin: 0; } .c { padding: 2px; width: 200px; } .c p { margin: 0; } input { width: 80px; }",
    )));
    rt.update_viewport_size(800.0, 600.0);
    rt
}

/// Build 12 cards, read geometry once up front and after every step of `steps`
/// (a JS array literal of functions over `els`/`root`), and return every
/// element's rect as one string. Every flush after the first is incremental.
fn run(steps: &str) -> String {
    let rt = runtime(page());
    let script = format!(
        "(function() {{
            var root = document.createElement('div');
            root.id = 'root';
            document.body.appendChild(root);
            for (var i = 0; i < 12; i++) {{
                var d = document.createElement('div');
                d.className = 'c';
                d.innerHTML = '<p>item ' + i + ' <b>x</b></p><input value=\"v' + i + '\"><span>s</span>';
                root.appendChild(d);
            }}
            var els = Array.prototype.slice.call(root.children);
            els[0].getBoundingClientRect();
            var steps = {steps};
            for (var k = 0; k < steps.length; k++) {{
                steps[k]();
                els[0].getBoundingClientRect();
            }}
            var out = [];
            function rect(e) {{
                var r = e.getBoundingClientRect();
                return [r.x, r.y, r.width, r.height].map(function(v) {{ return Math.round(v * 10) / 10; }}).join(',');
            }}
            out.push('root ' + rect(root));
            var all = root.querySelectorAll('*');
            for (var j = 0; j < all.length; j++) out.push(all[j].tagName + j + ' ' + rect(all[j]));
            return out.join('\\n');
        }})()",
        steps = steps,
    );
    match rt.eval(&script).unwrap() {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

const SCENARIOS: &[(&str, &str)] = &[
    ("width", "[function() { els[3].style.width = '50px'; }]"),
    (
        "height",
        "[function() { els[4].style.height = '80px'; }, function() { els[4].style.height = '10px'; }]",
    ),
    (
        "text",
        "[function() { els[5].style.width = '60px'; },
          function() { els[5].firstChild.firstChild.data = 'a much longer text that will wrap over several lines in a narrow box'; }]",
    ),
    (
        "tree",
        "[function() { root.appendChild(document.createElement('div')); },
          function() { root.removeChild(els[7]); },
          function() { root.insertBefore(els[2], els[9]); },
          function() { els[1].appendChild(document.createElement('p')); els[1].lastChild.textContent = 'added'; }]",
    ),
    (
        "form",
        "[function() { els[8].querySelector('input').value = 'a different and rather longer value'; },
          function() { els[9].setAttribute('hidden', ''); },
          function() { els[10].querySelector('span').className = 'x'; els[10].querySelector('span').textContent = 'wide span text'; },
          function() { els[9].removeAttribute('hidden'); }]",
    ),
    (
        "batch",
        "[function() { els[0].style.height = '40px'; els[6].textContent = 'replaced'; root.removeChild(els[11]); els[3].style.width = '90px'; }]",
    ),
];

#[test]
fn journal_driven_flush_publishes_the_same_rects_as_the_untracked_one() {
    for (name, steps) in SCENARIOS {
        CONTENT_JOURNAL_DISABLED.store(true, Ordering::SeqCst);
        let untracked = run(steps);
        CONTENT_JOURNAL_DISABLED.store(false, Ordering::SeqCst);
        let journaled = run(steps);
        assert_eq!(journaled, untracked, "{name}: the content journal changed the published geometry");
    }
}

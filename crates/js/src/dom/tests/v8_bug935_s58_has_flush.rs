//! BUG-935 срез 58 — a same-tick flush over a sheet with `:has()` publishes what
//! one full layout would.
//!
//! `:has()` lets a mutation flip the style of an ancestor arbitrarily far above
//! it, so the root-set cannot name that ancestor from the mutated node alone. Each
//! scenario runs twice over the same page: reading geometry after every step (a
//! chain of incremental flushes) and with no read until the end (one full layout).

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

const SHEET: &str = "body { margin: 0; } section { width: 300px; } .card { height: 20px; }
     .card:has(.hot) { height: 70px; }
     .card:has(> .new) { height: 90px; }
     section:has(.card .hot) { padding-top: 11px; }
     .card:has(.hot) .label { height: 33px; }
     .label { height: 5px; }";

fn run(steps: &str, read_between: bool) -> String {
    let rt = runtime(page());
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(SHEET)));
    let reads = if read_between { "els[0].getBoundingClientRect();" } else { "" };
    let script = format!(
        "(function() {{
            var sec = document.createElement('section');
            document.body.appendChild(sec);
            for (var i = 0; i < 6; i++) {{
                var d = document.createElement('div');
                d.className = 'card';
                d.innerHTML = '<p class=\"label\">l' + i + '</p><i class=\"leaf\"></i>';
                sec.appendChild(d);
            }}
            var els = Array.prototype.slice.call(sec.children);
            {reads}
            var steps = {steps};
            for (var k = 0; k < steps.length; k++) {{
                steps[k]();
                {reads}
            }}
            var out = [];
            function rect(e) {{
                var r = e.getBoundingClientRect();
                var cs = getComputedStyle(e);
                return [r.x, r.y, r.width, r.height].map(function(v) {{ return Math.round(v * 10) / 10; }}).join(',')
                    + ' ' + cs.paddingTop + ' ' + cs.height;
            }}
            out.push('sec ' + rect(sec));
            var all = sec.querySelectorAll('*');
            for (var j = 0; j < all.length; j++) out.push(all[j].tagName + j + ' ' + rect(all[j]));
            return out.join('\\n');
        }})()",
        steps = steps,
        reads = reads,
    );
    match rt.eval(&script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

const SCENARIOS: &[(&str, &str)] = &[
    // A class written on a deep descendant flips `.card:has(.hot)` and
    // `section:has(.card .hot)` — two ancestors, one of them above the parent.
    ("class_on_descendant", "[function() { els[2].lastChild.className = 'leaf hot'; }]"),
    ("class_removed_again", "[function() { els[2].lastChild.className = 'leaf hot'; }, function() { els[2].lastChild.className = 'leaf'; }]"),
    // A child-list change flips `.card:has(> .new)`.
    (
        "child_appended",
        "[function() { var n = document.createElement('b'); n.className = 'new'; els[4].appendChild(n); },
          function() { els[4].removeChild(els[4].lastChild); }]",
    ),
    // Both at once, and a subtree moved from one card to another.
    (
        "moved_subtree",
        "[function() { els[1].lastChild.className = 'leaf hot'; },
          function() { els[3].appendChild(els[1].lastChild); },
          function() { els[3].lastChild.className = 'leaf'; }]",
    ),
];

#[test]
fn has_flushes_publish_the_same_geometry_as_one_full_layout() {
    for (name, steps) in SCENARIOS {
        let full = run(steps, false);
        let incremental = run(steps, true);
        let diff: Vec<String> = full
            .lines()
            .zip(incremental.lines())
            .filter(|(f, i)| f != i)
            .take(6)
            .map(|(f, i)| format!("  full:        {f}\n  incremental: {i}"))
            .collect();
        assert!(diff.is_empty(), "{name}: a `:has()` ancestor kept a stale style\n{}", diff.join("\n"));
        assert_eq!(full.lines().count(), incremental.lines().count(), "{name}: different element count");
    }
}

/// A page with a CSSOM edit (or a script-inserted `<style>`) gets a patched sheet at
/// every flush; the patched sheet used to be re-made with a fresh revision each time,
/// and the incremental flush refuses a basis taken against another revision — so on
/// such a page every forced reflow was a full cascade.
#[test]
fn a_page_with_a_cssom_edit_still_flushes_incrementally() {
    let rt = runtime(page());
    let setup = "(function() {
        var st = document.createElement('style');
        document.body.appendChild(st);
        st.sheet.insertRule('.hot { color: red; }', 0);
        var root = document.createElement('div');
        document.body.appendChild(root);
        for (var i = 0; i < 40; i++) {
            var d = document.createElement('div');
            d.innerHTML = '<p>item ' + i + '</p><span>s</span>';
            root.appendChild(d);
        }
        window.__els = Array.prototype.slice.call(root.children);
        __els[0].getBoundingClientRect();
    })()";
    rt.eval(setup).unwrap();
    let before = rt.incremental_flush_count();
    rt.eval("(function() { __els[7].style.width = '50px'; __els[0].getBoundingClientRect(); __els[7].style.width = '60px'; __els[0].getBoundingClientRect(); })()")
        .unwrap();
    assert_eq!(
        rt.incremental_flush_count() - before,
        2,
        "the flushes after a CSSOM edit must be incremental, not full relayouts",
    );
}

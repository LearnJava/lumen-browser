//! BUG-1238 — the same-tick flush's scoped collectors (BUG-1211) must publish
//! exactly what one full layout would, including for boxes that merely *moved*
//! (a sibling after a resized box, an ancestor that grew) and were not named by
//! `dirty_roots`.
//!
//! Each scenario runs twice over the same page: once reading geometry after
//! every step (a chain of incremental flushes) and once with no read until the
//! end (the first and only flush is a full layout). The published rects, client
//! rects and computed `top`/`left`/`height`/`margin-left` must be identical.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

fn run(steps: &str, read_between: bool) -> String {
    let rt = runtime(page());
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
        "body { margin: 0; } .c { padding: 2px; width: 200px; } .c p { margin: 0; } input { width: 80px; }
         .w { position: relative; }
         .ab { position: absolute; bottom: 0; right: 0; width: 10px; height: 10px; }
         .in { height: 20px; } .m { margin: 0 auto; } .fl { float: left; }",
    )));
    let reads = if read_between {
        "els[0].getBoundingClientRect(); getComputedStyle(els[0]).top;"
    } else {
        ""
    };
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
            {reads}
            var steps = {steps};
            for (var k = 0; k < steps.length; k++) {{
                steps[k]();
                {reads}
            }}
            var out = [];
            function r1(v) {{ return Math.round(v * 10) / 10; }}
            function rect(e) {{
                var r = e.getBoundingClientRect();
                var cs = getComputedStyle(e);
                var q = Array.prototype.map.call(e.getClientRects(), function(c) {{
                    return [c.x, c.y, c.width, c.height].map(r1).join(',');
                }}).join(';');
                return [r.x, r.y, r.width, r.height].map(r1).join(',') + ' [' + q + '] '
                    + cs.top + ' ' + cs.left + ' ' + cs.height + ' ' + cs.marginLeft;
            }}
            out.push('root ' + rect(root));
            var all = root.querySelectorAll('*');
            for (var j = 0; j < all.length; j++) out.push(all[j].tagName + j + ' ' + rect(all[j]));
            return out.join('\\n');
        }})()",
        steps = steps,
        reads = reads,
    );
    match rt.eval(&script).unwrap_or_else(|e| panic!("{e:?}
{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

const SCENARIOS: &[(&str, &str)] = &[
    // The repro of the bug file: a later sibling after a resized box.
    (
        "sibling_shift",
        "[function() { els[3].style.height = '100px'; }, function() { els[3].style.height = '20px'; }]",
    ),
    ("sibling_width", "[function() { els[3].style.width = '50px'; }]"),
    // The parent (`root`) grows with its child: an ancestor is not in `dirty_roots`.
    ("ancestor_grows", "[function() { els[6].style.height = '300px'; }]"),
    (
        "auto_margin",
        "[function() { els[7].className = 'c m'; }, function() { els[7].style.width = '120px'; els[2].style.height = '5px'; }]",
    ),
    // `.ab`'s containing block is the positioned `.w`, outside the unchanged `.in`.
    (
        "abspos_outside_clean_subtree",
        "[function() { els[1].className = 'c w'; els[1].innerHTML = '<div class=\"in\"><i class=\"ab\"></i></div><p>top</p>'; },
          function() { els[1].lastChild.style.height = '90px'; },
          function() { els[1].lastChild.style.height = '10px'; }]",
    ),
    (
        "float_and_text",
        "[function() { els[4].className = 'c fl'; }, function() { els[2].firstChild.firstChild.data = 'a much longer text that wraps over several lines'; }]",
    ),
    (
        "tree",
        "[function() { root.removeChild(els[1]); }, function() { root.insertBefore(els[9], els[2]); }, function() { els[2].style.height = '40px'; }]",
    ),
];

#[test]
fn incremental_flushes_publish_the_same_geometry_as_one_full_layout() {
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
        assert!(diff.is_empty(), "{name}: scoped collectors left a stale entry\n{}", diff.join("\n"));
        assert_eq!(full.lines().count(), incremental.lines().count(), "{name}: different element count");
    }
}

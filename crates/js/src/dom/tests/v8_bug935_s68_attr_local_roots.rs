//! BUG-935 срез 68 — a `class`/`id`/`style`/`data-*` write that no selector reads from an
//! ancestor position restyles the element and its direct children, not its whole subtree.
//!
//! The wiring end to end: the page's `setAttribute`/`className`/`classList` writes are logged
//! with the value they replaced (`DomTouched::value_log`), the flush hands the old value to
//! the root-set (`NodeChange::AttrFrom`), and the root-set narrows when the tokens that moved
//! are read by no ancestor-position compound. Every scenario runs one page with the narrowing
//! (a read after every step: a chain of incremental flushes) and with it off
//! (`set_shallow_roots_off`, the pre-slice behaviour); the *whole* property map and rect of
//! every element must come out the same. A difference from one full layout is only reported —
//! the scoped collectors differ from it in ways that predate the slice (BUG-1238).
//!
//! This is the wiring test, not the soundness proof: the published computed styles are
//! re-resolved over the flush's scope, so they would not show a stale cascade entry. That proof
//! is `layout::box_tree::tests::bug935_attr_local_roots`, which compares the incremental cascade
//! map and box tree with a full rebuild (and fails when the token check is bypassed).

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

const SHEET: &str = "body { margin: 0; }
     .card { padding: 2px; width: 200px; margin: 3px; } .card p { margin: 0; }
     .open p { color: red; margin-left: 5px; } .open > .body i { font-style: normal; }
     :not(.dark) b { font-weight: 600; } .dark b { font-weight: 300; }
     .wide { width: 400px; } .loaded { outline: 1px solid blue; }
     [data-state=on] u { color: green; } [data-state] { margin-top: 2px; }
     #hero p { color: purple; } .side + .card { margin-top: 9px; }
     .card:is(.theme-a p) { border: 1px solid black; }";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// No read until the end: one full layout.
    Full,
    /// A read after every step, the narrowing on.
    Local,
    /// A read after every step, the narrowing off.
    Deep,
}

fn run(steps: &str, mode: Mode) -> (String, u64) {
    let reads = if mode == Mode::Full { "" } else { "getComputedStyle(els[0]).top; els[0].getBoundingClientRect();" };
    let rt = runtime(page());
    rt.set_shallow_roots_off(mode == Mode::Deep);
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(SHEET)));
    let script = format!(
        "(function() {{
            var root = document.createElement('div');
            root.id = 'root';
            document.body.appendChild(root);
            for (var i = 0; i < 6; i++) {{
                var d = document.createElement('div');
                d.className = 'card lazy';
                d.innerHTML = '<p>item ' + i + ' <b>x</b> <i>y</i></p><div class=\"body\"><i>q</i> <u>u</u></div><span>s</span>';
                root.appendChild(d);
            }}
            var side = document.createElement('div'); side.className = 'side';
            root.insertBefore(side, root.children[2]);
            var els = Array.prototype.slice.call(root.children);
            {reads}
            var steps = {steps};
            for (var k = 0; k < steps.length; k++) {{
                steps[k]();
                {reads}
            }}
            var out = [];
            function props(name, e) {{
                var entries = JSON.parse(_lumen_get_computed_style_entries(e.__nid__, false));
                var r = e.getBoundingClientRect();
                out.push(name + ' rect: ' + [r.x, r.y, r.width, r.height].join(','));
                if (!entries.length) out.push(name + ' <no entry>');
                for (var i = 0; i < entries.length; i++) out.push(name + ' ' + entries[i][0] + ': ' + entries[i][1]);
            }}
            props('body', document.body);
            var all = document.body.querySelectorAll('*');
            for (var j = 0; j < all.length; j++) props(all[j].tagName + j, all[j]);
            return out.join('\\n');
        }})()",
        steps = steps,
        reads = reads,
    );
    let out = match rt.eval(&script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    };
    (out, rt.shallow_roots_count())
}

fn diff(a: &str, b: &str) -> Vec<String> {
    let (a, b): (Vec<_>, Vec<_>) = (a.lines().collect(), b.lines().collect());
    let mut out: Vec<String> =
        a.iter().zip(&b).filter(|(x, y)| x != y).take(5).map(|(x, y)| format!("  {x}\n  {y}")).collect();
    if a.len() != b.len() {
        out.push(format!("  {} lines against {}", a.len(), b.len()));
    }
    out
}

const SCENARIOS: &[(&str, &str)] = &[
    // Tokens no selector reads from an ancestor: the narrowing's case.
    (
        "unreferenced_class_tokens",
        "[function() { els[0].classList.add('loaded'); },
          function() { els[0].classList.remove('lazy'); els[1].className = 'card lazy loaded'; },
          function() { els[0].className = 'card'; els[1].classList.toggle('lazy'); }]",
    ),
    // Tokens a descendant, child or `:not()` combinator reads: the whole subtree follows.
    (
        "referenced_class_tokens",
        "[function() { els[0].classList.add('open'); },
          function() { els[0].classList.add('dark'); els[1].classList.add('open'); },
          function() { els[0].classList.remove('open'); els[0].classList.remove('dark'); },
          function() { els[1].className = 'card'; }]",
    ),
    // `:is(.theme-a p)` reaches up from inside the subject compound.
    (
        "ancestor_inside_a_subject_compound",
        "[function() { els[0].classList.add('theme-a'); },
          function() { els[0].firstChild.classList.add('theme-a'); },
          function() { els[0].classList.remove('theme-a'); els[0].firstChild.classList.remove('theme-a'); }]",
    ),
    // `id`: `#hero p` reads it from an ancestor.
    (
        "id_read_by_a_descendant_combinator",
        "[function() { els[0].id = 'plain'; },
          function() { els[0].id = 'hero'; },
          function() { els[0].setAttribute('id', 'other'); els[1].id = 'hero'; },
          function() { els[1].removeAttribute('id'); }]",
    ),
    // `data-*` read by an attribute selector from an ancestor (`[data-state=on] u`) and not.
    (
        "data_attributes",
        "[function() { els[0].setAttribute('data-view', 'grid'); },
          function() { els[0].setAttribute('data-state', 'on'); },
          function() { els[0].setAttribute('data-state', 'off'); els[1].setAttribute('data-view', 'list'); },
          function() { els[0].removeAttribute('data-state'); }]",
    ),
    // Inline style: a colour is inherited by the whole subtree, a width is not.
    (
        "inline_style",
        "[function() { els[0].style.width = '120px'; },
          function() { els[0].style.color = 'red'; },
          function() { els[0].style.color = ''; els[1].style.fontSize = '20px'; },
          function() { els[0].setAttribute('style', 'width: 90px; height: 40px'); }]",
    ),
    // A write to the same attribute several times before one read.
    (
        "repeated_writes_before_a_read",
        "[function() { els[0].classList.add('open'); els[0].classList.remove('open'); els[0].classList.add('wide'); },
          function() { for (var i = 0; i < 9; i++) { els[0].classList.toggle('open'); els[0].classList.toggle('loaded'); } },
          function() { els[0].className = 'card wide'; }]",
    ),
    // A sibling combinator on the element's class (`.side + .card`).
    (
        "sibling_combinator",
        "[function() { root.children[2].classList.add('x'); },
          function() { root.children[2].classList.remove('side'); },
          function() { root.children[2].classList.add('side'); root.children[3].classList.add('y'); }]",
    ),
    // Wrapper nesting: a narrowed element inside another narrowed element.
    (
        "nested_narrowed_roots",
        "[function() { els[0].classList.add('loaded'); els[0].firstChild.classList.add('note'); els[0].lastChild.classList.add('note'); },
          function() { els[0].classList.add('wide'); els[0].firstChild.classList.add('open'); },
          function() { els[0].classList.remove('wide'); els[0].firstChild.classList.remove('open'); }]",
    ),
];

#[test]
fn narrowed_attribute_roots_publish_what_the_deep_restyle_publishes() {
    let mut engaged = 0;
    for (name, steps) in SCENARIOS {
        let (full, _) = run(steps, Mode::Full);
        let (deep, none_used) = run(steps, Mode::Deep);
        let (local, used) = run(steps, Mode::Local);
        assert_eq!(none_used, 0, "{name}: the switch did not turn the narrowing off");
        let stale = diff(&deep, &local);
        assert!(stale.is_empty(), "{name}: the narrowed restyle published something else\n{}", stale.join("\n"));
        engaged += used;
        let gap = diff(&full, &local);
        if !gap.is_empty() {
            eprintln!("{name}: scoped collectors differ from one full layout (predates the slice)\n{}", gap.join("\n"));
        }
    }
    assert!(engaged > 0, "no flush narrowed a root — the slice is not wired, and this proves nothing");
}

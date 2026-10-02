//! BUG-935 срез 64 — a stylesheet that changed since the previous flush (a `<style>` the page
//! inserted, edited or removed) is absorbed by the incremental flush: the plain rules that
//! were added, removed or moved name the elements to restyle, instead of a recascade of the
//! whole document (`incr declined: stylesheet revision changed`).
//!
//! Every scenario runs one page three ways, reading `getComputedStyle` and a rect after every
//! step: with the delta, with it off (`LUMEN_NO_SHEET_DELTA=1`, the pre-slice behaviour: the
//! full path on every sheet change) and with no read until the end (one full layout). The
//! *whole* property map and rect of every element must come out the same with and without the
//! delta; a difference from the single full layout is only reported — the scoped collectors
//! differ from it in ways that predate the slice (BUG-1238).
//!
//! The rules are the ones a too-narrow root set would get wrong: a later rule overriding an
//! earlier one at equal specificity, a combinator reaching through ancestors and siblings, an
//! inherited property set on a container, a structural pseudo-class, a pseudo-element, `*`.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

const SHEET: &str = "body { margin: 0; }
     .c { padding: 2px; width: 200px; margin: 3px; } .c p { margin: 0; } input { width: 80px; }
     .box { height: 20px; } .a span { color: red; } .b span { color: blue; }
     .c:first-child { margin-left: 7px; } .c + .c .box { height: 21px; }";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// No read until the end: one full layout.
    Full,
    /// A read after every step, the stylesheet delta on.
    Delta,
    /// A read after every step, as with `LUMEN_NO_SHEET_DELTA=1`.
    Off,
}

fn run(steps: &str, mode: Mode) -> (String, u64) {
    let reads = if mode == Mode::Full {
        ""
    } else {
        "getComputedStyle(els[0]).top; els[0].getBoundingClientRect();"
    };
    let rt = runtime(page());
    rt.set_sheet_delta_off(mode == Mode::Off);
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(SHEET)));
    let script = format!(
        "(function() {{
            var root = document.createElement('div');
            root.id = 'root';
            document.body.appendChild(root);
            for (var i = 0; i < 8; i++) {{
                var d = document.createElement('div');
                d.className = 'c';
                d.innerHTML = '<p>item ' + i + ' <b>x</b> <i>y</i></p><div class=\"box\"><u>q</u></div><input value=\"v' + i + '\"><span>s</span>';
                root.appendChild(d);
            }}
            var a = document.createElement('div'); a.className = 'a';
            a.innerHTML = '<div><p>in a <span>s</span></p></div>';
            var b = document.createElement('div'); b.className = 'b';
            b.innerHTML = '<p>in b <span>t</span></p>';
            document.body.appendChild(a); document.body.appendChild(b);
            var els = Array.prototype.slice.call(root.children);
            function addStyle(text) {{
                var st = document.createElement('style');
                st.textContent = text;
                document.body.appendChild(st);
                return st;
            }}
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
    (out, rt.sheet_delta_roots_count())
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
    // A rule for a few elements, then one that overrides it at equal specificity.
    (
        "later_rule_overrides",
        "[function() { addStyle('.c b { color: green; } .c i { font-size: 20px; }'); },
          function() { addStyle('.c b { color: purple; margin-left: 4px; }'); },
          function() { addStyle('.c p { margin: 5px; } .c:first-child p { margin: 9px; }'); }]",
    ),
    // The rules go away again, one `<style>` removed and another rewritten.
    (
        "style_removed_and_rewritten",
        "[function() { window.s1 = addStyle('.box { height: 33px; } .c u { color: red; }'); },
          function() { window.s2 = addStyle('.c span { font-weight: bold; }'); },
          function() { window.s1.parentNode.removeChild(window.s1); },
          function() { window.s2.textContent = '.c span { font-style: italic; } .c input { width: 120px; }'; },
          function() { window.s2.parentNode.removeChild(window.s2); }]",
    ),
    // Selectors that reach other elements: ancestors, siblings, `:not`, `:nth-child`.
    (
        "combinators_and_structure",
        "[function() { addStyle('.a p span { color: green; } .b > p { margin-top: 6px; } .c + .c p { margin-left: 3px; }'); },
          function() { addStyle('.c:nth-child(odd) i { font-style: italic; } .c:not(:first-child) u { color: teal; }'); }]",
    ),
    // An inherited property set on a container reaches everything below it.
    (
        "inherited_from_container",
        "[function() { addStyle('#root { color: crimson; font-size: 18px; line-height: 30px; }'); },
          function() { addStyle('.a { font-family: monospace; letter-spacing: 2px; }'); }]",
    ),
    // `body`, `*` and `:root` select the whole tree.
    (
        "universal_and_root",
        "[function() { addStyle('body { background: #eee; font-size: 17px; }'); },
          function() { addStyle('* { box-sizing: border-box; }'); },
          function() { addStyle(':root { color: navy; }'); }]",
    ),
    // A pseudo-element rule can change what is under an element whose own style stays equal
    // (generated content, the first line): it goes the full way.
    (
        "pseudo_elements",
        "[function() { addStyle('.c p::before { content: \"> \"; color: red; } .box::after { content: \"!\"; }'); },
          function() { addStyle('.c p::first-line { color: blue; } ::selection { color: red; }'); }]",
    ),
    // A sheet change together with an ordinary DOM mutation in the same flush.
    (
        "sheet_and_dom_together",
        "[function() { addStyle('.n { margin: 8px; color: orange; }'); var n = els[0].cloneNode(true); n.className = 'c n'; root.appendChild(n); },
          function() { els[1].className = 'c n'; addStyle('.c.n u { color: olive; }'); },
          function() { root.removeChild(root.lastChild); addStyle('.c.n p { margin: 1px; }'); }]",
    ),
    // Something the delta cannot express goes the full way and must still be right.
    (
        "non_plain_rules_fall_back",
        "[function() { addStyle('.c b { color: green; }'); },
          function() { addStyle('@media (min-width: 100px) { .c i { color: red; } } @keyframes k { from { opacity: 0 } to { opacity: 1 } }'); },
          function() { addStyle('@layer base { .c u { color: red; } } .c u { color: blue; }'); }]",
    ),
];

#[test]
fn a_changed_stylesheet_publishes_what_the_full_cascade_publishes() {
    let mut engaged = 0;
    for (name, steps) in SCENARIOS {
        let (full, _) = run(steps, Mode::Full);
        let (off, none_used) = run(steps, Mode::Off);
        let (delta, used) = run(steps, Mode::Delta);
        assert_eq!(none_used, 0, "{name}: the switch did not turn the stylesheet delta off");
        let stale = diff(&off, &delta);
        assert!(stale.is_empty(), "{name}: the stylesheet delta published something else\n{}", stale.join("\n"));
        engaged += used;
        let gap = diff(&full, &delta);
        if !gap.is_empty() {
            eprintln!("{name}: scoped collectors differ from one full layout (predates the slice)\n{}", gap.join("\n"));
        }
    }
    assert!(engaged > 0, "no flush took a root from a changed sheet — the slice is not wired, and this proves nothing");
}

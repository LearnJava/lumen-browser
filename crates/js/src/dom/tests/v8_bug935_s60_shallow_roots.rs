//! BUG-935 срез 60 — a child-list mutation restyles the container and its direct
//! children (`NodeChange::ChildList`), not the parent's whole subtree.
//!
//! Every scenario runs one page three ways, reading `getComputedStyle` and a rect after
//! every step: with the shallow roots, with them off (`LUMEN_NO_SHALLOW_ROOTS=1`, the
//! pre-slice behaviour) and with no read until the end (one full layout). The *whole*
//! property map and rect of every element must come out the same with and without the
//! shallow roots; a difference from the full layout is only reported — the scoped
//! collectors differ from it in ways that predate the slice (BUG-1238).
//!
//! The selectors are the ones a shallow restyle could get wrong: a positional compound on
//! the way down, a sibling combinator through a child, `:empty`, `:only-child`.

use super::v8_bug935_s55_content_journal::{page, runtime};
use super::*;

const SHEET: &str = "body { margin: 0; }
     .c { padding: 2px; width: 200px; margin: 3px; } .c p { margin: 0; } input { width: 80px; }
     .c:first-child { margin-left: 7px; } .c:last-child p b { color: red; }
     .c + .c .box { height: 21px; } .c:nth-child(odd) i { font-style: normal; }
     .c:not(:first-child) u { color: green; } .e:empty { height: 9px; }
     .e:empty + .c { margin-top: 11px; } .only:only-child { font-size: 20px; color: blue; }
     .box { height: 20px; } .a span { color: red; } .b span { color: blue; }";

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// No read until the end: one full layout.
    Full,
    /// A read after every step, the shallow roots on.
    Shallow,
    /// A read after every step, as with `LUMEN_NO_SHALLOW_ROOTS=1`.
    Deep,
}

fn run(steps: &str, mode: Mode) -> (String, u64) {
    let reads = if mode == Mode::Full {
        ""
    } else {
        "getComputedStyle(els[0]).top; els[0].getBoundingClientRect();"
    };
    let rt = runtime(page());
    rt.set_shallow_roots_off(mode == Mode::Deep);
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
            var holder = document.createElement('div');
            holder.className = 'e';
            document.body.appendChild(holder);
            var tail = document.createElement('div');
            tail.className = 'c';
            tail.innerHTML = '<p>tail</p>';
            document.body.appendChild(tail);
            var a = document.createElement('div'); a.className = 'a';
            a.innerHTML = '<div><p>in a <span>s</span></p></div>';
            var b = document.createElement('div'); b.className = 'b';
            document.body.appendChild(a); document.body.appendChild(b);
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
    // The shape of the lenta.ru `fonts2` loop: a child appended to `body` and removed again.
    (
        "append_remove_on_body",
        "[function() { var s = document.createElement('span'); s.style.fontFamily = 'serif'; document.body.appendChild(s); s.offsetWidth; },
          function() { document.body.removeChild(document.body.lastChild); },
          function() { var s = document.createElement('span'); document.body.appendChild(s); s.style.fontFamily = 'monospace'; s.offsetWidth; }]",
    ),
    // Cards appended, prepended, removed: `:first-child`/`:last-child`/`:nth-child`,
    // `.c + .c .box` and `:not(:first-child) u` all move.
    (
        "cards_appended_prepended_removed",
        "[function() { var n = els[0].cloneNode(true); root.appendChild(n); },
          function() { var n = els[1].cloneNode(true); root.insertBefore(n, root.firstChild); },
          function() { root.removeChild(root.firstChild); },
          function() { root.removeChild(root.lastChild); root.removeChild(root.children[2]); }]",
    ),
    // `:empty` flips on the holder and its sibling combinator reaches the next card.
    (
        "empty_holder",
        "[function() { var h = document.querySelector('.e'); h.appendChild(document.createElement('i')); },
          function() { var h = document.querySelector('.e'); h.removeChild(h.firstChild); },
          function() { var h = document.querySelector('.e'); h.textContent = 'text'; },
          function() { var h = document.querySelector('.e'); h.textContent = ''; }]",
    ),
    // A child stops being the only one: it and everything inside inherits another size.
    (
        "only_child",
        "[function() { var o = document.createElement('div'); o.className = 'only'; o.innerHTML = '<p>only <b>x</b></p>'; var w = document.createElement('div'); w.id = 'w'; w.appendChild(o); document.body.appendChild(w); },
          function() { document.getElementById('w').appendChild(document.createElement('div')); },
          function() { var w = document.getElementById('w'); w.removeChild(w.lastChild); }]",
    ),
    // A subtree moved between parents keeps its own style and changes its ancestors.
    (
        "moved_subtree",
        "[function() { var a = document.querySelector('.a'), b = document.querySelector('.b'); b.appendChild(a.firstChild); },
          function() { var a = document.querySelector('.a'), b = document.querySelector('.b'); a.insertBefore(b.firstChild, a.firstChild); }]",
    ),
    // `innerHTML` replaced under a card; a card's text replaced.
    (
        "inner_html_and_text",
        "[function() { els[2].innerHTML = '<p>replaced <b>b</b></p><div class=\"box\"></div>'; },
          function() { els[3].firstChild.firstChild.data = 'changed '; },
          function() { els[4].textContent = 'plain'; }]",
    ),
];

#[test]
fn shallow_roots_publish_what_the_deep_restyle_publishes() {
    let mut engaged = 0;
    for (name, steps) in SCENARIOS {
        let (full, _) = run(steps, Mode::Full);
        let (deep, none_used) = run(steps, Mode::Deep);
        let (shallow, used) = run(steps, Mode::Shallow);
        assert_eq!(none_used, 0, "{name}: the switch did not turn the shallow roots off");
        let stale = diff(&deep, &shallow);
        assert!(stale.is_empty(), "{name}: the shallow restyle published something else\n{}", stale.join("\n"));
        engaged += used;
        let gap = diff(&full, &shallow);
        if !gap.is_empty() {
            eprintln!("{name}: scoped collectors differ from one full layout (predates the slice)\n{}", gap.join("\n"));
        }
    }
    assert!(engaged > 0, "no flush used a shallow root — the slice is not wired, and this proves nothing");
}

/// BUG-1242: an unrelated style write must not shift the cards whose margins the clean
/// path used to eat.
#[test]
fn a_flush_after_an_unrelated_write_keeps_the_margins() {
    let script = |reads: bool| {
        let rt = runtime(page());
        rt.update_stylesheet(Arc::new(lumen_css_parser::parse(
            "body { margin: 0; } .m { margin: 3px; padding: 2px; width: 200px; height: 20px; } .m:first-child { margin-left: 7px; } \
             .auto { margin: 0 auto; width: 100px; height: 10px; } .rel { position: relative; left: 5px; top: 2px; height: 10px; }",
        )));
        let body = format!(
            "(function() {{
                var root = document.createElement('div'); document.body.appendChild(root);
                for (var i = 0; i < 3; i++) {{ var d = document.createElement('div'); d.className = 'm'; root.appendChild(d); }}
                var au = document.createElement('div'); au.className = 'auto'; root.appendChild(au);
                var rl = document.createElement('div'); rl.className = 'rel'; root.appendChild(rl);
                var els = root.children;
                {read}
                els[1].style.color = 'red'; {read}
                els[2].style.color = 'blue'; {read}
                var out = [];
                for (var j = 0; j < els.length; j++) {{ var r = els[j].getBoundingClientRect(); out.push([r.x, r.y, r.width, r.height].join(',')); }}
                return out.join(' | ');
            }})()",
            read = if reads { "els[0].getBoundingClientRect();" } else { "" },
        );
        match rt.eval(&body).unwrap_or_else(|e| panic!("{e:?}")) {
            lumen_core::JsValue::String(s) => s,
            other => panic!("expected a string, got {other:?}"),
        }
    };
    assert_eq!(script(true), script(false), "incremental flushes moved boxes a full layout does not");
}

//! BUG-935 срез 95 — the same-tick flush refreshes the pseudo-element computed styles and the
//! resolved custom properties only for what the plan collects, instead of walking the whole
//! document and dropping the whole previous map on every flush.
//!
//! rbc.ru reads both, and that walk was 57 % of its forced-flush time. Refreshing a part is only
//! sound if it is the part a whole-document collect would have changed, so every scenario runs
//! over the same page with a read of both maps after every step — once scoped, once with
//! `LUMEN_NO_SCOPE_PRUNE` (the whole-document collectors) — and what the page can read must come
//! out the same.

use super::v8_bug935_s55_content_journal::{page, runtime, JOURNAL_SWITCH};
use super::*;

const SHEET: &str = "body { margin: 0; --scrollbar-compensation: 0px; --gap: 4px; }
     .c { padding: 2px; width: 200px; margin: 3px; --card: var(--gap); }
     .c::before { content: 'a'; color: red; } .c::after { content: 'b'; display: block; }
     .c p { margin: 0; } .c p::first-letter { color: blue; font-size: 20px; }
     .hot::before { content: 'hot'; color: green; } .hot { --card: 9px; }
     .nobefore::before { content: none; } .gap2 { --gap: 8px; } .hid { display: none; }
     .fl { float: left; width: 40px; } .box { height: 20px; }";

/// Every scenario starts from ten cards under `root` (`els`), each with a paragraph, a nested box
/// and an inline run.
const SCENARIOS: &[(&str, &str)] = &[
    (
        "class_toggles_a_pseudo_on_one_card",
        "[function() { els[3].className = 'c hot'; },
          function() { els[3].className = 'c nobefore'; },
          function() { els[3].className = 'c'; }]",
    ),
    (
        "custom_property_on_body",
        "[function() { document.body.style.setProperty('--scrollbar-compensation', '15px'); },
          function() { document.body.style.setProperty('--scrollbar-compensation', '0px'); },
          function() { document.body.style.setProperty('--gap', '12px'); }]",
    ),
    (
        "custom_property_on_a_card_reaches_its_subtree",
        "[function() { els[2].className = 'c gap2'; },
          function() { els[2].style.setProperty('--card', '33px'); },
          function() { els[2].className = 'c'; }]",
    ),
    (
        "append_and_remove_cards",
        "[function() { var n = document.createElement('div'); n.className = 'c hot'; root.appendChild(n); },
          function() { var n = document.createElement('div'); n.className = 'c'; n.innerHTML = '<p>fresh</p>'; root.insertBefore(n, els[0]); },
          function() { gone.push(root.removeChild(root.lastChild)); },
          function() { gone.push(root.removeChild(root.firstChild)); }]",
    ),
    (
        "a_removed_subtree_publishes_nothing",
        "[function() { els[4].className = 'c hot'; },
          function() { gone.push(root.removeChild(els[4])); },
          function() { gone.push(root.removeChild(els[6])); },
          function() { root.appendChild(gone[0]); },
          function() { gone.push(root.removeChild(gone.shift())); }]",
    ),
    (
        "card_hidden_and_shown",
        "[function() { els[5].className = 'c hid'; },
          function() { els[5].className = 'c hot'; },
          function() { els[5].className = 'c'; }]",
    ),
    (
        "first_letter_text_replaced",
        "[function() { els[1].firstChild.firstChild.data = 'zzz'; },
          function() { els[1].firstChild.innerHTML = '<b>q</b>rest'; },
          function() { els[1].firstChild.textContent = 'plain'; }]",
    ),
    (
        "sibling_grows_above_a_carried_over_card",
        "[function() { var n = document.createElement('div'); n.className = 'box'; n.style.height = '90px'; root.insertBefore(n, els[4]); },
          function() { root.removeChild(els[4].previousSibling); }]",
    ),
];

fn run(steps: &str, scoped: bool) -> String {
    let rt = runtime(page());
    rt.set_scope_prune_off(!scoped);
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(SHEET)));
    let script = format!(
        "(function() {{
            var root = document.createElement('div');
            root.id = 'root';
            document.body.appendChild(root);
            for (var i = 0; i < 10; i++) {{
                var d = document.createElement('div');
                d.className = 'c';
                d.innerHTML = '<p>item ' + i + ' <b>x</b></p><div class=\"box\"><u>q</u></div><span>s</span>';
                root.appendChild(d);
            }}
            var els = Array.prototype.slice.call(root.children);
            var out = [];
            var gone = [];
            function props(name, e) {{
                var kinds = ['before', 'after', 'first-line', 'first-letter'];
                for (var k = 0; k < kinds.length; k++) {{
                    var entries = JSON.parse(_lumen_get_computed_style_pseudo_entries(e.__nid__, kinds[k]));
                    entries.sort();
                    out.push(name + ' ::' + kinds[k] + ' ' + entries.length + ' ' + entries.join(';'));
                }}
                out.push(name + ' --gap=' + _lumen_get_custom_property(e.__nid__, '--gap')
                    + ' --card=' + _lumen_get_custom_property(e.__nid__, '--card')
                    + ' --sc=' + _lumen_get_custom_property(e.__nid__, '--scrollbar-compensation'));
            }}
            function snapshot(tag) {{
                out.push('== ' + tag);
                props('root', root);
                props('body', document.body);
                var all = root.querySelectorAll('*');
                for (var j = 0; j < all.length; j++) props(all[j].tagName + j, all[j]);
                // Detached nodes have no box, so neither map may still hold them.
                for (var g = 0; g < gone.length; g++) {{
                    props('gone' + g, gone[g]);
                    var inner = gone[g].querySelectorAll('*');
                    for (var h = 0; h < inner.length; h++) props('gone' + g + '.' + h, inner[h]);
                }}
            }}
            snapshot('start');
            var steps = {steps};
            for (var s = 0; s < steps.length; s++) {{
                steps[s]();
                snapshot('step ' + s);
            }}
            return out.join('\\n');
        }})()"
    );
    match rt.eval(&script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    }
}

#[test]
fn scoped_pseudo_and_custom_property_maps_match_the_whole_document_ones() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, steps) in SCENARIOS {
        let whole = run(steps, false);
        let scoped = run(steps, true);
        let (a, b): (Vec<_>, Vec<_>) = (whole.lines().collect(), scoped.lines().collect());
        let stale: Vec<String> =
            a.iter().zip(&b).filter(|(x, y)| x != y).take(5).map(|(x, y)| format!("  {x}\n  {y}")).collect();
        assert!(stale.is_empty(), "{name}: the scoped maps are stale\n{}", stale.join("\n"));
        assert_eq!(a.len(), b.len(), "{name}: different number of lines");
    }
}

#[test]
fn the_scenarios_read_something() {
    // A guard against a test that compares two empty maps: the page really has pseudo entries
    // and custom properties to publish.
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let out = run("[function() { els[3].className = 'c hot'; }]", true);
    assert!(out.contains("--gap=4px --card=4px"), "custom properties are not read:\n{out}");
    assert!(out.contains("::before") && out.contains("content"), "no pseudo entry:\n{out}");
    let last = out.rsplit("== step 0").next().unwrap_or("");
    assert!(last.contains("--card=9px"), "the hot card did not publish its own --card:\n{last}");
}

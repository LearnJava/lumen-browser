//! BUG-935 срез 100 — the same-tick flush refreshes the per-text-node fragment table
//! ([`lumen_layout::collect_text_frag_rects`]) over the planned boxes only, instead of walking the
//! whole document and dropping the previous table on every flush. The table backs
//! `Range.getClientRects()` and `CSS.highlights.highlightsFromPoint()`.
//!
//! The table is keyed by text node, which owns no box, so a published entry may only be dropped
//! when the flush rebuilt the box it came from and no run renders that text any more. Every
//! scenario runs over the same page with a read of the whole table after every step — once
//! scoped, once with `LUMEN_NO_SCOPE_PRUNE` (the whole-document collector) — and what the page
//! can read must come out the same, including the empty answer for detached text.

use super::v8_bug935_s55_content_journal::{page, runtime, JOURNAL_SWITCH};
use super::*;

const SHEET: &str = "body { margin: 0; font: 16px/20px serif; }
     .c { padding: 2px; width: 200px; margin: 3px; }
     .c p { margin: 0; }
     .big { font-size: 30px; } .narrow { width: 90px; } .hid { display: none; }
     .pre { white-space: pre; } .box { height: 20px; }";

/// Every scenario starts from ten cards under `root` (`els`), each with a paragraph mixing text
/// and inline elements, plus a block and a span.
const SCENARIOS: &[(&str, &str)] = &[
    (
        "text_data_rewritten",
        "[function() { els[1].querySelector('p').firstChild.data = 'zzz a much longer replacement text'; },
          function() { els[1].querySelector('p').firstChild.data = 'short'; },
          function() { els[4].querySelector('u').firstChild.data = 'a very long underline indeed'; }]",
    ),
    (
        "innerhtml_and_textcontent_replace_text_nodes",
        "[function() { els[2].querySelector('p').innerHTML = '<b>q</b>rest of it'; },
          function() { els[2].querySelector('p').textContent = 'plain replacement'; },
          function() { els[6].innerHTML = '<p>fresh <i>run</i></p>'; }]",
    ),
    (
        "class_toggles_rewrap_one_card",
        "[function() { els[3].className = 'c big'; },
          function() { els[3].className = 'c narrow'; },
          function() { els[3].className = 'c pre'; },
          function() { els[3].className = 'c'; }]",
    ),
    (
        "append_and_remove_cards",
        "[function() { var n = document.createElement('div'); n.className = 'c'; n.innerHTML = '<p>new card text</p>'; root.appendChild(n); },
          function() { var n = document.createElement('div'); n.className = 'c'; n.innerHTML = '<p>inserted above</p>'; root.insertBefore(n, els[0]); },
          function() { gone.push(root.removeChild(root.lastChild)); },
          function() { gone.push(root.removeChild(root.firstChild)); }]",
    ),
    (
        "a_removed_subtree_publishes_nothing",
        "[function() { els[4].querySelector('p').firstChild.data = 'changed before removal'; },
          function() { gone.push(root.removeChild(els[4])); },
          function() { gone.push(root.removeChild(els[6])); },
          function() { root.appendChild(gone[0]); },
          function() { gone.push(root.removeChild(gone.shift())); }]",
    ),
    (
        "card_hidden_and_shown",
        "[function() { els[5].className = 'c hid'; },
          function() { els[5].className = 'c'; },
          function() { els[5].querySelector('span').style.display = 'none'; }]",
    ),
    (
        "block_grows_above_a_carried_over_card",
        "[function() { var n = document.createElement('div'); n.className = 'box'; n.style.height = '90px'; root.insertBefore(n, els[4]); },
          function() { root.removeChild(els[4].previousSibling); }]",
    ),
    (
        "sibling_grows_while_a_card_below_is_rewritten",
        "[function() { var n = document.createElement('div'); n.className = 'box'; n.style.height = '70px'; root.insertBefore(n, els[2]); els[7].querySelector('p').firstChild.data = 'both at once'; },
          function() { root.removeChild(els[2].previousSibling); els[7].className = 'c big'; },
          function() { var n = document.createElement('div'); n.className = 'c'; n.innerHTML = '<p>late</p>'; root.insertBefore(n, els[5]); els[3].querySelector('p').firstChild.data = 'also moved'; }]",
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
                d.innerHTML = '<p>item ' + i + ' <b>x</b> tail</p><div class=\"box\"><u>q</u></div><span>s</span>';
                root.appendChild(d);
            }}
            var els = Array.prototype.slice.call(root.children);
            var out = [];
            var gone = [];
            function frags(t, name) {{
                out.push(name + ' ' + _lumen_text_frag_rects(t.__nid__).join(','));
            }}
            function walk(n, path) {{
                var k = 0;
                for (var c = n.firstChild; c; c = c.nextSibling) {{
                    if (c.nodeType === 3) frags(c, path + '/t' + k);
                    else if (c.nodeType === 1) walk(c, path + '/' + c.tagName + k);
                    k++;
                }}
            }}
            function snapshot(tag) {{
                out.push('== ' + tag);
                walk(root, 'root');
                // Detached nodes have no box, so the table may hold nothing for their text.
                for (var g = 0; g < gone.length; g++) walk(gone[g], 'gone' + g);
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
fn scoped_text_fragment_table_matches_the_whole_document_one() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, steps) in SCENARIOS {
        let whole = run(steps, false);
        let scoped = run(steps, true);
        let (a, b): (Vec<_>, Vec<_>) = (whole.lines().collect(), scoped.lines().collect());
        let stale: Vec<String> =
            a.iter().zip(&b).filter(|(x, y)| x != y).take(5).map(|(x, y)| format!("  {x}\n  {y}")).collect();
        assert!(stale.is_empty(), "{name}: the scoped table is stale\n{}", stale.join("\n"));
        assert_eq!(a.len(), b.len(), "{name}: different number of lines");
    }
}

#[test]
fn the_scenarios_read_something() {
    // A guard against a test that compares two empty tables: the page really publishes fragments,
    // a rewrite changes them, and a removed card's text is gone from the table.
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let out = run(
        "[function() { els[1].querySelector('p').firstChild.data = 'zzz a much longer replacement text'; },
          function() { gone.push(root.removeChild(els[4])); }]",
        true,
    );
    let start = out.split("== step 0").next().unwrap_or("");
    let step0 = out.split("== step 0").nth(1).and_then(|s| s.split("== step 1").next()).unwrap_or("");
    assert!(start.contains("root/DIV0/P0"), "no paragraph text in the table:\n{start}");
    assert!(
        start.lines().any(|l| l.contains("/t0 ") && l.split(' ').nth(1).is_some_and(|f| f.contains(','))),
        "no fragments: {start}"
    );
    assert_ne!(start, step0, "a rewritten text node kept its fragments");
    let step1 = out.split("== step 1").nth(1).unwrap_or("");
    assert!(
        step1.lines().filter(|l| l.starts_with("gone0") && l.contains("/t")).all(|l| l.ends_with(' ')),
        "a detached card still publishes fragments:\n{step1}"
    );
}

#[test]
#[ignore = "стенд профилирования, не проверка"]
fn bug935_s100_stand() {
    // 2 200 elements, the font-detector loop of срез 76 plus one `_lumen_text_frag_rects` read per
    // mutation — the table is rebuilt on every flush once the page has asked for it, which is what
    // a page reading `Range.getClientRects()` in a rAF loop does.
    // `LUMEN_PROFILE_TREE=1 cargo test -p lumen-js --profile dev-release --features v8-backend
    // --lib bug935_s100_stand -- --ignored --nocapture --test-threads=1`, once plain and once with
    // `LUMEN_NO_SCOPE_PRUNE=1` (the whole-document collector).
    let rt = runtime(page());
    let script = "(function() {
        var root = document.createElement('main');
        document.body.appendChild(root);
        for (var i = 0; i < 200; i++) {
            var c = document.createElement('article');
            c.className = 'card';
            c.innerHTML = '<h3>t</h3><p><a href=\"#\">x</a> <b>y</b></p><ul><li>1</li><li>2</li><li>3</li></ul><div><span>a</span><em>b</em></div>';
            root.appendChild(c);
        }
        document.body.getBoundingClientRect();
        var t0 = Date.now();
        for (var k = 0; k < 300; k++) {
            var s = document.createElement('span');
            s.style.fontFamily = 'monospace';
            s.textContent = 'mmmmmmmmmmlli';
            document.body.appendChild(s);
            _lumen_text_frag_rects(s.firstChild.__nid__);
            document.body.removeChild(s);
            _lumen_text_frag_rects(k % 11 === 0 ? root.firstChild.querySelector('p').firstChild.__nid__ : s.__nid__);
        }
        return String(Date.now() - t0);
    })()";
    match rt.eval(script).unwrap() {
        lumen_core::JsValue::String(s) => eprintln!("[stand] 300 flushes: {s} ms"),
        other => panic!("{other:?}"),
    }
}

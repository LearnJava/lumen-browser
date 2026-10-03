//! BUG-935 срез 59 — the same-tick flush leaves a computed-style entry published
//! when nothing the entry is made of changed, and rebuilds every other one.
//!
//! A dirty root is re-cascaded as a whole, which on a page whose forced-reflow
//! loop appends one `span` to `body` is the whole document, and the computed-style
//! collector costs ~175 µs a box. Skipping an entry is only sound if it is the entry
//! the flush would have rebuilt, so every scenario runs over the same page reading
//! `getComputedStyle` after every step — once with the skip, once without — and
//! the *whole* property map of every element must come out the same. A third run
//! with no read until the end (one full layout) is compared too, but only reported:
//! the scoped collectors differ from it in ways that predate the skip.

use super::v8_bug935_s55_content_journal::{page, runtime, JOURNAL_SWITCH};
use super::*;

pub(super) const SHEET: &str = "body { margin: 0; }
     .c { padding: 2px; width: 200px; margin: 3px; } .c p { margin: 0; } input { width: 80px; }
     .pc { width: 50%; padding: 0 10%; } .wide { width: 300px; } .pp { padding: 5%; }
     .c:first-child { margin-left: 7px; } .e:empty { height: 9px; }
     .rel { position: relative; top: 10%; } .hid { display: none; }
     .fl { float: left; width: 40px; } .ab { position: absolute; left: 4px; top: 4px; }
     .box { height: 20px; }";

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    /// No read until the end: one full layout.
    Full,
    /// A read after every step, the skip on.
    Skip,
    /// A read after every step, as with `LUMEN_NO_STYLE_SKIP=1`.
    NoSkip,
}

fn run(steps: &str, mode: Mode) -> (String, u64) {
    run_on(SHEET, steps, mode)
}

pub(super) fn run_on(sheet: &str, steps: &str, mode: Mode) -> (String, u64) {
    run_with(sheet, steps, mode, false)
}

/// [`run_on`] with the pruning of unchanged subtrees inside a dirty root (BUG-935 срез 70)
/// switched off when `prune_off`.
pub(super) fn run_with(sheet: &str, steps: &str, mode: Mode, prune_off: bool) -> (String, u64) {
    let (out, kept, _) = run_counted(sheet, steps, mode, prune_off);
    (out, kept)
}

/// [`run_with`], plus the boxes the flushes left alone inside a dirty root.
pub(super) fn run_counted(sheet: &str, steps: &str, mode: Mode, prune_off: bool) -> (String, u64, u64) {
    let reads = if mode == Mode::Full {
        ""
    } else {
        "getComputedStyle(els[0]).top; els[0].getBoundingClientRect();"
    };
    let rt = runtime(page());
    rt.set_style_skip_off(mode == Mode::NoSkip);
    // The "skip off" baseline is the behaviour before every later slice too: no pruning either.
    rt.set_scope_prune_off(prune_off || mode == Mode::NoSkip);
    rt.update_stylesheet(Arc::new(lumen_css_parser::parse(sheet)));
    let script = format!(
        "(function() {{
            var root = document.createElement('div');
            root.id = 'root';
            document.body.appendChild(root);
            for (var i = 0; i < 10; i++) {{
                var d = document.createElement('div');
                d.className = 'c';
                d.innerHTML = '<p>item ' + i + ' <b>x</b> <i>y</i></p><div class=\"pc box\"><u>q</u></div><input value=\"v' + i + '\"><span>s</span><div class=\"e\"></div>';
                root.appendChild(d);
            }}
            var els = Array.prototype.slice.call(root.children);
            {reads}
            var out = [];
            function props(name, e) {{
                var entries = JSON.parse(_lumen_get_computed_style_entries(e.__nid__, false));
                var r = e.getBoundingClientRect();
                out.push(name + ' rect: ' + [r.x, r.y, r.width, r.height].join(','));
                // Per-fragment rects and the scroll metrics are separate caches (срез 70).
                var frags = e.getClientRects();
                var fr = [];
                for (var f = 0; f < frags.length; f++) fr.push([frags[f].x, frags[f].y, frags[f].width, frags[f].height].join(','));
                out.push(name + ' client: ' + fr.join(' | ') + ' scroll: ' + [e.scrollWidth, e.scrollHeight].join(','));
                if (!entries.length) out.push(name + ' <no entry>');
                for (var i = 0; i < entries.length; i++) out.push(name + ' ' + entries[i][0] + ': ' + entries[i][1]);
            }}
            function snapshot(tag) {{
                out.push('== ' + tag);
                props('root', root);
                props('body', document.body);
                var all = root.querySelectorAll('*');
                for (var j = 0; j < all.length; j++) props(all[j].tagName + j, all[j]);
            }}
            var steps = {steps};
            for (var k = 0; k < steps.length; k++) {{
                steps[k]();
                {reads}
                // Every step is compared, not only the last: a later step can repair what an earlier one left stale.
                if ({per_step}) snapshot('step ' + k);
            }}
            snapshot('end');
            return out.join('\\n');
        }})()",
        steps = steps,
        reads = reads,
        per_step = mode != Mode::Full,
    );
    let out = match rt.eval(&script).unwrap_or_else(|e| panic!("{e:?}\n{script}")) {
        lumen_core::JsValue::String(s) => s,
        other => panic!("expected a string, got {other:?}"),
    };
    (out, rt.style_entries_kept_count(), rt.scope_pruned_count())
}

pub(super) fn diff(a: &str, b: &str) -> Vec<String> {
    let (a, b): (Vec<_>, Vec<_>) = (a.lines().collect(), b.lines().collect());
    let mut out: Vec<String> =
        a.iter().zip(&b).filter(|(x, y)| x != y).take(5).map(|(x, y)| format!("  {x}\n  {y}")).collect();
    if a.len() != b.len() {
        out.push(format!("  {} lines against {}", a.len(), b.len()));
    }
    out
}

/// `(name, steps, must_keep)`: `must_keep` — the scenario is one where entries
/// provably survive, so a collector that quietly stopped skipping fails too (the
/// answer would still be right, just slow — the S8 lesson).
pub(super) const SCENARIOS: &[(&str, &str, bool)] = &[
    // The shape of the lenta.ru `fonts2` loop: a child appended to `body` and removed
    // again, a style written on it, a layout property read in between.
    (
        "append_remove_on_body",
        "[function() { var s = document.createElement('span'); s.style.fontFamily = 'serif'; document.body.appendChild(s); s.offsetWidth; },
          function() { document.body.removeChild(document.body.lastChild); },
          function() { var s = document.createElement('span'); document.body.appendChild(s); s.style.fontFamily = 'monospace'; s.offsetWidth; }]",
        true,
    ),
    // A child appended to `root` and removed: every card is re-cascaded as a child of the root and
    // everything under a card is carried over (BUG-935 срез 70).
    (
        "append_on_root",
        "[function() { var n = document.createElement('div'); n.className = 'box'; root.appendChild(n); },
          function() { root.removeChild(root.lastChild); },
          function() { var n = document.createElement('p'); n.textContent = 'x'; root.insertBefore(n, els[4]); }]",
        true,
    ),
    // A child-list change under `root`: the card after it is shifted, not changed.
    (
        "append_in_card",
        "[function() { var n = document.createElement('div'); n.className = 'box'; els[3].appendChild(n); },
          function() { els[3].removeChild(els[3].lastChild); }]",
        true,
    ),
    // A non-inherited property on a card: its own entry changes, its descendants
    // are re-cascaded to the styles they had. An inherited one changes all of them.
    (
        "background_on_card",
        "[function() { els[4].style.backgroundColor = 'red'; }, function() { els[4].style.backgroundColor = 'blue'; }]",
        true,
    ),
    ("colour_on_card", "[function() { els[4].style.color = 'red'; }, function() { els[4].style.color = 'blue'; }]", false),
    // The containing block of percentage widths and paddings changes width.
    (
        "container_width",
        "[function() { els[2].style.width = '120px'; }, function() { els[2].className = 'c wide pp'; }]",
        false,
    ),
    // Text inside an inline run changes; so does an inline element's own style.
    (
        "inline_run_text",
        "[function() { els[1].firstChild.firstChild.nextSibling.firstChild.data = 'a much longer text that wraps over several lines'; },
          function() { els[1].firstChild.lastChild.style.fontWeight = 'bold'; }]",
        false,
    ),
    // `:first-child` and `:empty` flip on elements the child-list change did not name.
    (
        "structural_pseudo",
        "[function() { var n = document.createElement('div'); n.className = 'c'; root.insertBefore(n, els[0]); },
          function() { els[5].lastChild.appendChild(document.createElement('b')); },
          function() { root.removeChild(root.firstChild); }]",
        false,
    ),
    // A positioned box resolves its insets against a height that changes.
    (
        "relative_inset",
        "[function() { els[6].lastChild.className = 'rel e'; els[6].style.height = '90px'; }, function() { els[6].style.height = '40px'; }]",
        false,
    ),
    // A float inserted before siblings, an absolutely positioned child, `display: none`.
    (
        "float_abs_hidden",
        "[function() { var f = document.createElement('div'); f.className = 'fl box'; root.insertBefore(f, els[2]); },
          function() { els[7].firstChild.className = 'ab'; },
          function() { els[8].className = 'c hid'; }]",
        false,
    ),
    // A moved subtree: a node cascaded under a different inherited chain.
    (
        "moved_subtree",
        "[function() { els[3].style.color = 'green'; }, function() { els[5].appendChild(els[1].firstChild); }, function() { els[5].style.color = 'red'; }]",
        false,
    ),
];

#[test]
fn skipped_computed_style_entries_equal_the_rebuilt_ones() {
    // Needs the journal on throughout — see `JOURNAL_SWITCH`.
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, steps, must_keep) in SCENARIOS {
        let (full, _) = run(steps, Mode::Full);
        let (noskip, none_kept) = run(steps, Mode::NoSkip);
        let (skip, kept) = run(steps, Mode::Skip);
        assert_eq!(none_kept, 0, "{name}: the switch did not turn the skip off");
        let stale = diff(&noskip, &skip);
        assert!(stale.is_empty(), "{name}: a skipped entry is stale\n{}", stale.join("\n"));
        if *must_keep {
            assert!(kept > 0, "{name}: no entry was left published — the skip never engaged");
        }
        let gap = diff(&full, &skip);
        if !gap.is_empty() {
            eprintln!("{name}: scoped collectors differ from one full layout (predates the skip)\n{}", gap.join("\n"));
        }
    }
}

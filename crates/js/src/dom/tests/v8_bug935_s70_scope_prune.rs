//! BUG-935 срез 70 — a dirty root is no longer collected whole: the subtrees of it that
//! nothing in the change record touched, and that sit where the published maps have them,
//! keep their entries.
//!
//! The root of the `lenta.ru` font-probe loop is `body` (a child appended and removed), and
//! until this slice four collectors walked the whole document under it to rebuild one `span`.
//! Leaving a part alone is only sound if the part is what the collectors would have rebuilt,
//! so every scenario of the срез-59 set runs over the same page with a read after every step,
//! once with the pruning and once without (`LUMEN_NO_SCOPE_PRUNE=1`), and the whole property
//! map, rect, fragment rects and scroll metrics of every element must come out the same.

use super::v8_bug935_s55_content_journal::JOURNAL_SWITCH;
use super::v8_bug935_s59_style_skip::{diff, run_counted, Mode, SCENARIOS, SHEET};

/// Scenarios where whole subtrees of a dirty root provably stay put, so a plan that quietly
/// stopped pruning (the answer would still be right, just slow — the S8 lesson) fails too.
const MUST_PRUNE: &[&str] = &["append_remove_on_body", "append_on_root"];

#[test]
fn pruned_subtrees_publish_what_a_whole_dirty_root_would() {
    // Needs the journal on throughout — see `JOURNAL_SWITCH`.
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, steps, _) in SCENARIOS {
        let (whole, _, none_pruned) = run_counted(SHEET, steps, Mode::Skip, true);
        let (pruned, _, boxes) = run_counted(SHEET, steps, Mode::Skip, false);
        assert_eq!(none_pruned, 0, "{name}: the switch did not turn the pruning off");
        let stale = diff(&whole, &pruned);
        assert!(stale.is_empty(), "{name}: a pruned subtree is stale
{}", stale.join("
"));
        if MUST_PRUNE.contains(name) {
            assert!(boxes > 0, "{name}: nothing was left alone — the pruning never engaged");
        }
        eprintln!("{name}: boxes left alone inside a dirty root: {boxes}");
    }
}

const EXTRA_SHEET: &str = ".relroot { position: relative; } .abb { position: absolute; right: 3px; bottom: 3px; width: 5px; height: 5px; }
     .fl2 { float: left; width: 30px; height: 30px; } .fl2.big { width: 160px; height: 70px; }
     .center { text-align: center; } .shrink { display: inline-block; } .tall { height: 90px; }";

/// Scenarios built to break the pruning: a carried-over subtree whose entries depend on something
/// outside it — a containing block that grows, a float that changes, a sibling that shifts it.
const EXTRA: &[(&str, &str)] = &[
    (
        "abs_child_of_a_growing_containing_block",
        "[function() { root.className = 'relroot'; },
          function() { var a = document.createElement('div'); a.className = 'abb'; els[5].appendChild(a); },
          function() { var n = document.createElement('div'); n.className = 'box tall'; root.appendChild(n); },
          function() { root.removeChild(root.lastChild); }]",
    ),
    (
        "float_inserted_then_resized",
        "[function() { var f = document.createElement('div'); f.className = 'fl2'; root.insertBefore(f, els[0]); },
          function() { root.firstChild.className = 'fl2 big'; },
          function() { root.firstChild.className = 'fl2'; },
          function() { root.removeChild(root.firstChild); }]",
    ),
    (
        "float_content_grows",
        "[function() { var f = document.createElement('div'); f.className = 'fl2'; root.insertBefore(f, els[0]); },
          function() { var n = document.createElement('div'); n.className = 'box tall'; root.firstChild.appendChild(n); },
          function() { root.firstChild.removeChild(root.firstChild.lastChild); }]",
    ),
    (
        "sibling_grows_above_carried_over_subtree",
        "[function() { var n = document.createElement('div'); n.className = 'box tall'; root.insertBefore(n, els[2]); },
          function() { root.removeChild(els[2].previousSibling); },
          function() { root.className = 'center'; },
          function() { root.className = ''; }]",
    ),
    (
        "shrink_to_fit_container",
        "[function() { root.className = 'shrink'; },
          function() { var n = document.createElement('div'); n.className = 'box'; n.style.width = '400px'; root.appendChild(n); },
          function() { root.removeChild(root.lastChild); }]",
    ),
    (
        "card_hidden_and_shown",
        "[function() { els[3].style.display = 'none'; },
          function() { root.appendChild(document.createElement('hr')); },
          function() { els[3].style.display = ''; },
          function() { root.removeChild(root.lastChild); }]",
    ),
];

#[test]
fn pruning_survives_scenarios_built_to_break_it() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let sheet = format!("{SHEET}\n{EXTRA_SHEET}");
    for (name, steps) in EXTRA {
        let (whole, _, none_pruned) = run_counted(&sheet, steps, Mode::Skip, true);
        let (pruned, _, boxes) = run_counted(&sheet, steps, Mode::Skip, false);
        assert_eq!(none_pruned, 0, "{name}: the switch did not turn the pruning off");
        let stale = diff(&whole, &pruned);
        assert!(stale.is_empty(), "{name}: a pruned subtree is stale\n{}", stale.join("\n"));
        eprintln!("{name}: boxes left alone inside a dirty root: {boxes}");
    }
}

//! BUG-935 срез 61 — a positioned box keeps its published computed-style entry when
//! every ancestor is exactly where it was, and is rebuilt otherwise.
//!
//! The slice-59 skip left every `relative`/`absolute`/`fixed` box to the collector
//! (on lenta.ru ~250 of the ~330 entries a flush rebuilt): their `top`/`bottom`/…
//! are resolved against a containing block's height or position. Same method as
//! slice 59 — every scenario against the same page with the skip on and off, the
//! whole property map of every element equal.

use super::v8_bug935_s55_content_journal::JOURNAL_SWITCH;
use super::v8_bug935_s59_style_skip::{diff, run_on, Mode};

const SHEET: &str = "body { margin: 0; }
     .c { padding: 2px; width: 200px; margin: 3px; } .c p { margin: 0; } input { width: 80px; }
     .pos { position: relative; height: 80px; }
     .a1 { position: absolute; right: 5px; bottom: 5%; width: 30px; height: 10px; }
     .a2 { position: absolute; left: 50%; top: 10%; width: 20px; height: 12px; }
     .a3 { position: absolute; width: 15px; height: 15px; }
     .a4 { position: absolute; top: 4px; left: 4px; width: 10px; height: 10px; }
     .r1 { position: relative; top: 10%; left: 3px; }
     .r2 { position: relative; bottom: 4px; }
     .fx { position: fixed; top: 2px; left: 3%; width: 10px; height: 10px; }
     .st { position: sticky; top: 0; }
     .tall { height: 150px; } .box { height: 20px; }";

/// `(name, steps, must_keep)`. Cards `els[2]`/`els[4]` are set up as containing blocks in the
/// first step; the later steps change something else, or the containing block itself.
const SCENARIOS: &[(&str, &str, bool)] = &[
    // The lenta.ru loop: a span appended to `body` and removed, while positioned boxes
    // sit under unmoved ancestors.
    (
        "loop_over_positioned",
        "[function() { els[2].className = 'c pos'; els[2].innerHTML = '<div class=\"a1\"></div><div class=\"a2\"></div><div class=\"a3\"></div><p class=\"r1\">t</p><p class=\"r2\">u</p>';
                       els[4].innerHTML = '<div class=\"fx\"></div><div class=\"st\">s</div>'; },
          function() { var s = document.createElement('span'); s.style.fontFamily = 'serif'; document.body.appendChild(s); s.offsetWidth; },
          function() { document.body.removeChild(document.body.lastChild); },
          function() { els[6].style.backgroundColor = 'red'; }]",
        true,
    ),
    // The containing block grows: percentage insets must follow, and so must the `bottom`/`right`
    // an absolute box reports for an inset it left `auto` — while its own rect does not move.
    (
        "containing_block_resized",
        "[function() { els[2].className = 'c pos'; els[2].innerHTML = '<div class=\"a1\"></div><div class=\"a2\"></div><div class=\"a3\"></div><div class=\"a4\"></div><p class=\"r1\">t</p>'; },
          function() { els[2].style.height = '140px'; },
          function() { els[2].className = 'c pos tall'; els[2].style.height = ''; },
          function() { els[2].style.paddingTop = '9px'; }]",
        false,
    ),
    // A sibling above a containing block changes height: the whole block is translated.
    (
        "containing_block_shifted",
        "[function() { els[3].className = 'c pos'; els[3].innerHTML = '<div class=\"a1\"></div><div class=\"a3\"></div><p class=\"r1\">t</p>'; },
          function() { els[1].style.height = '60px'; },
          function() { els[1].style.height = '30px'; }]",
        false,
    ),
    // A positioned box nested in another and a static box in between whose size changes.
    (
        "nested_positioned",
        "[function() { els[2].className = 'c pos'; els[2].innerHTML = '<div class=\"box\"><div class=\"pos\"><div class=\"a1\"></div><div class=\"a2\"></div></div></div>'; },
          function() { els[2].firstChild.style.height = '50px'; },
          function() { els[2].firstChild.style.height = ''; els[2].firstChild.firstChild.style.width = '120px'; }]",
        false,
    ),
    // The positioned box itself, not its surroundings, is what changes.
    (
        "positioned_itself",
        "[function() { els[2].className = 'c pos'; els[2].innerHTML = '<div class=\"a1\"></div><p class=\"r1\">t</p>'; },
          function() { els[2].firstChild.style.bottom = '20%'; },
          function() { els[2].lastChild.style.top = '30%'; },
          function() { els[2].firstChild.className = 'a2'; },
          function() { els[2].removeChild(els[2].firstChild); }]",
        false,
    ),
    // A positioned ancestor's border and `position` flip: the containing block of its absolute child changes.
    (
        "containing_block_kind",
        "[function() { els[2].className = 'c pos'; els[2].innerHTML = '<div class=\"a1\"></div><div class=\"a2\"></div>'; },
          function() { els[2].style.borderTop = '7px solid black'; },
          function() { els[2].style.position = 'static'; },
          function() { els[2].style.position = 'relative'; els[2].style.border = '0'; }]",
        false,
    ),
];

#[test]
fn skipped_positioned_entries_equal_the_rebuilt_ones() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, steps, must_keep) in SCENARIOS {
        let (noskip, none_kept) = run_on(SHEET, steps, Mode::NoSkip);
        let (skip, kept) = run_on(SHEET, steps, Mode::Skip);
        assert_eq!(none_kept, 0, "{name}: the switch did not turn the skip off");
        let stale = diff(&noskip, &skip);
        assert!(stale.is_empty(), "{name}: a skipped positioned entry is stale\n{}", stale.join("\n"));
        assert!(!*must_keep || kept > 0, "{name}: no entry was left published — the skip never engaged");
    }
}

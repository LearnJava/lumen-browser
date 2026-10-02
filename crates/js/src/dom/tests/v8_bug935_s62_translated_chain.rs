//! BUG-935 срез 62 — the geometry chain survives a box whose transform is a translation.
//!
//! lenta.ru keeps an off-canvas menu at `transform: translateX(-310px)`: it publishes its
//! border box shifted, so the chain check `published == own rect` failed there every flush and
//! the ~60 entries below were rebuilt each time. A pure translation with the style untouched
//! leaves one possible border box, so the chain is vouched for; a rotation or a scale is not.
//! Same method as slices 59/61 — every scenario against the same page with the skip on and
//! off, the whole property map of every element equal after every step.

use super::v8_bug935_s55_content_journal::JOURNAL_SWITCH;
use super::v8_bug935_s59_style_skip::{diff, run_on, Mode};

const SHEET: &str = "body { margin: 0; }
     .c { padding: 2px; width: 200px; margin: 3px; } .c p { margin: 0; } input { width: 80px; }
     .tr { transform: translateX(-40px); } .tr2 { transform: translate(5px, 7px); }
     .rot { transform: rotate(7deg); } .sc { transform: scale(1.1); }
     .mid { width: 100px; margin-left: auto; margin-right: auto; height: 12px; }
     .pct { padding-left: 10%; } .pos { position: relative; height: 40px; }
     .a1 { position: absolute; right: 5px; bottom: 10%; width: 8px; height: 8px; }";

const KIDS: &str =
    "'<div class=\"mid\"></div><p class=\"pct\">t</p><div class=\"pos\"><div class=\"a1\"></div></div>'";

const SCENARIOS: &[(&str, &str)] = &[
    (
        "translated_loop",
        "[function() { els[2].className = 'c tr'; els[2].innerHTML = KIDS; },
          function() { var s = document.createElement('span'); s.style.fontFamily = 'serif'; document.body.appendChild(s); s.offsetWidth; },
          function() { document.body.removeChild(document.body.lastChild); },
          function() { els[6].style.backgroundColor = 'red'; }]",
    ),
    // The box itself changes size or place: the chain must break.
    (
        "translated_resized",
        "[function() { els[2].className = 'c tr2'; els[2].innerHTML = KIDS; },
          function() { els[2].style.width = '150px'; },
          function() { els[2].style.paddingLeft = '20px'; },
          function() { els[2].style.height = '90px'; }]",
    ),
    // The layout box moves by what the translation cancels: the published rect stays the same.
    (
        "translated_compensated",
        "[function() { els[2].className = 'c tr'; els[2].innerHTML = KIDS; },
          function() { els[2].style.marginLeft = '23px'; els[2].style.transform = 'translateX(-63px)'; },
          function() { els[2].style.marginLeft = ''; els[2].style.transform = ''; }]",
    ),
    // A sibling above changes height, so the translated box is carried down.
    (
        "translated_shifted",
        "[function() { els[3].className = 'c tr'; els[3].innerHTML = KIDS; },
          function() { els[1].style.height = '60px'; },
          function() { els[1].style.height = '30px'; }]",
    ),
    // Rotation and scale publish a bounding box: not vouched for, and still right.
    (
        "rotated_and_scaled",
        "[function() { els[2].className = 'c rot'; els[2].innerHTML = KIDS; els[4].className = 'c sc'; els[4].innerHTML = KIDS; },
          function() { var s = document.createElement('span'); document.body.appendChild(s); s.offsetWidth; },
          function() { document.body.removeChild(document.body.lastChild); els[2].style.width = '170px'; },
          function() { els[4].style.paddingLeft = '11px'; }]",
    ),
    // The transform itself is edited.
    (
        "transform_edited",
        "[function() { els[2].className = 'c tr'; els[2].innerHTML = KIDS; },
          function() { els[2].style.transform = 'translateX(-90px)'; },
          function() { els[2].style.transform = 'translateX(-90px) rotate(3deg)'; },
          function() { els[2].style.transform = 'none'; }]",
    ),
];

#[test]
fn translated_chains_equal_the_rebuilt_ones() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, steps) in SCENARIOS {
        let steps = steps.replace("KIDS", KIDS);
        let (noskip, none_kept) = run_on(SHEET, &steps, Mode::NoSkip);
        let (skip, _) = run_on(SHEET, &steps, Mode::Skip);
        assert_eq!(none_kept, 0, "{name}: the switch did not turn the skip off");
        let stale = diff(&noskip, &skip);
        assert!(stale.is_empty(), "{name}: a skipped entry under a translated box is stale\n{}", stale.join("\n"));
    }
}

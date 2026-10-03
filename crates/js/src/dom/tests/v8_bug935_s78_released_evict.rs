//! BUG-935 срез 78 — a flush evicts the entries of a dirty root by the ids the box build
//! released from the previous tree plus the boxes the plan collects, not by listing the
//! previous tree's whole dirty area and subtracting everything the plan left alone.
//!
//! The two must forget the same nodes: every scenario of the срез-59 set runs over the same
//! page with a read after every step, once each way, and the whole property map, rects and
//! scroll metrics of every element must come out the same — and so must the *sizes* of the four
//! caches at the end, which is where an entry for a removed node that was never evicted (or one
//! evicted for a node that is still there) would show.

use super::v8_bug935_s55_content_journal::JOURNAL_SWITCH;
use super::v8_bug935_s59_style_skip::{diff, run_published, Mode, SCENARIOS, SHEET};

#[test]
fn the_release_list_evicts_what_the_dirty_area_listing_did() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, steps, _) in SCENARIOS {
        let (old, _, _, old_sizes) = run_published(SHEET, steps, Mode::Skip, false, true);
        let (new, _, _, new_sizes) = run_published(SHEET, steps, Mode::Skip, false, false);
        let stale = diff(&old, &new);
        assert!(stale.is_empty(), "{name}: a published entry differs\n{}", stale.join("\n"));
        assert_eq!(old_sizes, new_sizes, "{name}: the caches hold a different number of entries");
    }
}

/// Subtrees the box build carried over and then did not place: a parent that stops generating
/// boxes leaves its untouched children out of the new tree, and their entries must go too.
const UNPLACED: &str = "[function() { els[3].className = 'c hid'; },
      function() { els[3].offsetWidth; els[4].style.display = 'none'; },
      function() { els[3].className = 'c'; els[4].style.display = ''; },
      function() { els[5].innerHTML = ''; },
      function() { els[6].removeChild(els[6].firstChild); },
      function() { els[7].parentNode.removeChild(els[7]); }]";

#[test]
fn a_subtree_taken_out_and_not_placed_is_evicted() {
    let _journal_on = JOURNAL_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let (old, _, _, old_sizes) = run_published(SHEET, UNPLACED, Mode::Skip, false, true);
    let (new, _, _, new_sizes) = run_published(SHEET, UNPLACED, Mode::Skip, false, false);
    let stale = diff(&old, &new);
    assert!(stale.is_empty(), "a published entry differs\n{}", stale.join("\n"));
    assert_eq!(old_sizes, new_sizes, "the caches hold a different number of entries");
}

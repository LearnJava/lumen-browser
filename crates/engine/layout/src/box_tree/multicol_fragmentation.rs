//! CSS Fragmentation L3 §3.3 — `orphans` / `widows` for the atomic column placement of a
//! multicol container (CSS Multicol L1 §3.4 / §7).
//!
//! A multicol container whose segment holds line boxes (`InlineRun` children, the text
//! between `<br>`s) places them one by one into columns. Without a rule a column break may
//! fall after any line, leaving one line at the bottom of a column or one at the top of the
//! next. `orphans` is the minimum number of lines of a block left at the bottom of a column
//! before a break, `widows` the minimum at the top of the next one. When a break cannot
//! satisfy them the run of lines moves to the next column as a whole (Fragmentation L3 §3.3:
//! «the fragmentainer break is moved before the block»); when it already sits at the top of a
//! column the rule is ignored. `column-fill: balance` takes the rules into account too: the
//! balanced column height is the smallest one at which every break respects them, so two
//! lines of text stay in one column instead of being spread one per column.
//!
//! Layout works on whole boxes here, so a «line» is one `InlineRun` item (its `lines.len()`
//! lines) and the zero-height `<br>` blocks between runs are glue that never forms a break
//! of its own.
//!
//! Forced breaks (Fragmentation L3 §3.1, `break-before`/`break-after`: `column`/`always`) are
//! per-item flags: [`forced_breaks`] marks the items that must open a new column, and
//! [`pack`] honours them before it looks at the height.

use super::*;
use crate::style::BreakValue;

/// What a flow child of a multicol segment is, for the purposes of `orphans`/`widows`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ItemLines {
    /// An `InlineRun` of this many line boxes — part of the run of lines it belongs to.
    Lines(u32),
    /// A zero-height empty block (`<br>`) between two line runs: it neither starts nor ends
    /// a run.
    Glue,
    /// Anything else (a box with its own content): ends the current run.
    Other,
}

/// Classifies a laid-out (measured) flow child.
pub(super) fn item_lines(b: &LayoutBox) -> ItemLines {
    match &b.kind {
        BoxKind::InlineRun { lines, .. } if !lines.is_empty() => {
            ItemLines::Lines(lines.len() as u32)
        }
        BoxKind::Block
            if b.rect.height == 0.0
                && b.children.iter().all(|c| matches!(c.kind, BoxKind::Skip)) =>
        {
            ItemLines::Glue
        }
        _ => ItemLines::Other,
    }
}

/// For every item the `[start, end)` item range of the run of lines it belongs to
/// (`None` for items outside any run). `end` is just past the run's last `Lines` item.
fn runs(kinds: &[ItemLines]) -> Vec<Option<(usize, usize)>> {
    let mut out = vec![None; kinds.len()];
    let mut i = 0;
    while i < kinds.len() {
        if !matches!(kinds[i], ItemLines::Lines(_)) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        let mut k = i + 1;
        while k < kinds.len() {
            match kinds[k] {
                ItemLines::Lines(_) => {
                    k += 1;
                    end = k;
                }
                ItemLines::Glue => k += 1,
                ItemLines::Other => break,
            }
        }
        for slot in &mut out[start..end] {
            *slot = Some((start, end));
        }
        i = end;
    }
    out
}

fn line_count(kinds: &[ItemLines]) -> u32 {
    kinds
        .iter()
        .map(|k| if let ItemLines::Lines(n) = k { *n } else { 0 })
        .sum()
}

/// CSS Fragmentation L3 §3.1 — a forced break lands between two sibling boxes when the first
/// has `break-after` or the second `break-before` of `column`/`always`. (`page`/`region` belong
/// to other fragmentation contexts; a multicol container does not force them.) `forced[j]` is
/// set when item `j` must start a new column; the first item never does.
pub(super) fn forced_breaks(items: &[&LayoutBox]) -> Vec<bool> {
    let forces = |v: BreakValue| matches!(v, BreakValue::Column | BreakValue::Always);
    (0..items.len())
        .map(|j| {
            j > 0 && (forces(items[j - 1].style.break_after) || forces(items[j].style.break_before))
        })
        .collect()
}

/// Whether `style` carries a forced column break of its own (see [`forced_breaks`]).
pub(super) fn has_forced_break(style: &ComputedStyle) -> bool {
    matches!(style.break_before, BreakValue::Column | BreakValue::Always)
        || matches!(style.break_after, BreakValue::Column | BreakValue::Always)
}

/// Greedy column assignment: an item that does not fit the column opens the next one — a
/// column always takes at least one item — a `forced` item opens one regardless of the
/// height (unless it already starts a column), and a break inside a run of lines honours
/// `orphans`/`widows`. With `strict` a break that cannot honour them (the run already
/// starts at the top of the column) makes the whole packing fail; without it the rule is
/// ignored there. Returns the column of every item.
pub(super) fn pack(
    outer_hs: &[f32],
    kinds: &[ItemLines],
    forced: &[bool],
    target_h: f32,
    orphans: u32,
    widows: u32,
    strict: bool,
) -> Option<Vec<usize>> {
    let orphans = orphans.max(1);
    let widows = widows.max(1);
    let run_of = runs(kinds);
    let mut asg = vec![0usize; outer_hs.len()];
    let mut fills = vec![0.0f32];
    let mut cur = 0usize;
    let mut col_start = 0usize;
    for (j, &oh) in outer_hs.iter().enumerate() {
        if forced[j] && j > col_start {
            cur += 1;
            fills.push(0.0);
            col_start = j;
        } else if fills[cur] > 0.0 && fills[cur] + oh > target_h && oh > 0.0 {
            let mut brk = j;
            if let Some((rs, re)) = run_of[j].filter(|(rs, _)| *rs < j) {
                let before = line_count(&kinds[rs.max(col_start)..j]);
                let rest = line_count(&kinds[j..re]);
                // Lines to hand over to the next column so that it gets `widows` of them.
                let back = widows.saturating_sub(rest);
                if before < orphans || before < back + orphans {
                    if rs > col_start {
                        brk = rs;
                    } else if strict {
                        return None;
                    }
                } else if back > 0 {
                    let mut left = back;
                    while left > 0 && brk > col_start {
                        brk -= 1;
                        if let ItemLines::Lines(n) = kinds[brk] {
                            left = left.saturating_sub(n);
                        }
                    }
                }
            }
            let moved: f32 = outer_hs[brk..j].iter().sum();
            fills[cur] -= moved;
            cur += 1;
            fills.push(moved);
            for a in &mut asg[brk..j] {
                *a = cur;
            }
            col_start = brk;
        }
        asg[j] = cur;
        fills[cur] += oh;
    }
    Some(asg)
}

/// CSS Multicol §7.1 — balanced column height for atomic boxes: the smallest height at which
/// the greedy packing fits `n_cols` columns and every break respects `orphans`/`widows`.
/// Without line runs this is the plain minimum of the greedy packing.
pub(super) fn balanced_height(
    outer_hs: &[f32],
    kinds: &[ItemLines],
    forced: &[bool],
    n_cols: usize,
    orphans: u32,
    widows: u32,
) -> f32 {
    let total: f32 = outer_hs.iter().sum();
    if n_cols <= 1 || outer_hs.is_empty() {
        return total.max(1.0);
    }
    let max_item = outer_hs.iter().cloned().fold(0.0_f32, f32::max);
    let mut lo = max_item.max(total / n_cols as f32);
    let mut hi = total.max(lo);
    let fits = |h: f32| -> bool {
        pack(outer_hs, kinds, forced, h, orphans, widows, true)
            .is_some_and(|asg| asg.iter().copied().max().unwrap_or(0) < n_cols)
    };
    for _ in 0..40 {
        if hi - lo <= 0.25 {
            break;
        }
        let mid = (lo + hi) * 0.5;
        if fits(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi.ceil().max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ItemLines::{Glue, Lines, Other};

    fn none(n: usize) -> Vec<bool> {
        vec![false; n]
    }

    /// `n` single-line runs separated by `<br>` glue, each line 20px.
    fn lines(n: usize) -> (Vec<f32>, Vec<ItemLines>) {
        let mut hs = Vec::new();
        let mut ks = Vec::new();
        for i in 0..n {
            if i > 0 {
                hs.push(0.0);
                ks.push(Glue);
            }
            hs.push(20.0);
            ks.push(Lines(1));
        }
        (hs, ks)
    }

    fn cols_of(asg: &[usize], ks: &[ItemLines]) -> Vec<usize> {
        // Column of every line item (glue skipped).
        asg.iter()
            .zip(ks)
            .filter(|(_, k)| matches!(k, Lines(_)))
            .map(|(a, _)| *a)
            .collect()
    }

    #[test]
    fn greedy_break_keeps_two_lines_at_the_top_of_a_column() {
        // Six lines, columns of 100px would take five: the sixth would be a lone widow, so
        // the fifth moves with it.
        let (hs, ks) = lines(6);
        let asg = pack(&hs, &ks, &none(hs.len()), 100.0, 2, 2, false).unwrap();
        assert_eq!(cols_of(&asg, &ks), vec![0, 0, 0, 0, 1, 1]);
    }

    #[test]
    fn a_break_that_leaves_enough_lines_is_untouched() {
        let (hs, ks) = lines(8);
        let asg = pack(&hs, &ks, &none(hs.len()), 100.0, 2, 2, false).unwrap();
        assert_eq!(cols_of(&asg, &ks), vec![0, 0, 0, 0, 0, 1, 1, 1]);
    }

    #[test]
    fn a_run_that_cannot_split_moves_to_the_next_column() {
        // A box, then three lines of which only one would stay: orphans moves the run.
        let hs = [60.0, 20.0, 0.0, 20.0, 0.0, 20.0];
        let ks = [Other, Lines(1), Glue, Lines(1), Glue, Lines(1)];
        let asg = pack(&hs, &ks, &none(hs.len()), 80.0, 2, 2, false).unwrap();
        assert_eq!(asg, vec![0, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn at_the_top_of_a_column_the_rule_is_ignored_unless_strict() {
        let (hs, ks) = lines(4);
        // Columns of one line: every break violates orphans, and no run start is in reach.
        let asg = pack(&hs, &ks, &none(hs.len()), 20.0, 2, 2, false).unwrap();
        assert_eq!(cols_of(&asg, &ks), vec![0, 1, 2, 3]);
        assert!(pack(&hs, &ks, &none(hs.len()), 20.0, 2, 2, true).is_none());
    }

    #[test]
    fn rules_of_one_line_change_nothing() {
        let (hs, ks) = lines(6);
        let asg = pack(&hs, &ks, &none(hs.len()), 100.0, 1, 1, true).unwrap();
        assert_eq!(cols_of(&asg, &ks), vec![0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn balancing_two_lines_keeps_them_in_one_column() {
        let (hs, ks) = lines(2);
        // The search stops within 0.25px of the minimum and rounds up (as before the split).
        assert_eq!(balanced_height(&hs, &ks, &none(hs.len()), 14, 2, 2), 40.0);
        assert_eq!(balanced_height(&hs, &ks, &none(hs.len()), 14, 1, 1), 21.0);
    }

    #[test]
    fn balancing_six_lines_makes_three_columns_of_two() {
        let (hs, ks) = lines(6);
        assert_eq!(balanced_height(&hs, &ks, &none(hs.len()), 14, 2, 2), 41.0);
    }

    #[test]
    fn balancing_without_runs_is_the_plain_minimum() {
        let hs = [30.0, 30.0, 30.0, 30.0, 30.0, 30.0];
        let ks = [Other; 6];
        assert_eq!(balanced_height(&hs, &ks, &none(hs.len()), 3, 2, 2), 61.0);
    }

    #[test]
    fn a_forced_break_opens_a_column_that_would_still_have_room() {
        let hs = [10.0, 10.0, 10.0];
        let ks = [Other; 3];
        let asg = pack(&hs, &ks, &[false, true, true], 100.0, 1, 1, false).unwrap();
        assert_eq!(asg, vec![0, 1, 2]);
    }

    #[test]
    fn a_forced_break_on_the_first_item_adds_no_empty_column() {
        let hs = [60.0, 10.0, 10.0];
        let ks = [Other; 3];
        let asg = pack(&hs, &ks, &[true, true, false], 50.0, 1, 1, false).unwrap();
        assert_eq!(asg, vec![0, 1, 1]);
    }

    #[test]
    fn balancing_with_forced_breaks_fits_the_forced_columns() {
        // Three items, a forced break before each: three columns of one item whatever the
        // column count would give an even split.
        let hs = [30.0, 50.0, 20.0];
        let ks = [Other; 3];
        let h = balanced_height(&hs, &ks, &[false, true, true], 3, 1, 1);
        assert_eq!(h, 51.0);
    }
}

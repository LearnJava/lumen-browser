//! CSS Flexbox L2 `flex-wrap: balance` (#algo-balance): flex-line breaking that keeps the
//! greedy line count (or `flex-line-count` if larger) but spreads the items evenly.

/// Contiguous item runs of the greedy line breaking (`sizes` are the floored outer hypothetical
/// main sizes, `gap` the main-axis gap, `avail` the container's main size). A line takes items
/// while the sum still fits; the first item of a line is taken whatever its size.
fn greedy_breaks(sizes: &[f32], gap: f32, avail: f32) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    let mut cur = 0.0_f32;
    for (k, &sz) in sizes.iter().enumerate() {
        if k > start && cur + gap + sz > avail {
            runs.push((start, k));
            start = k;
            cur = sz;
        } else {
            cur += if k > start { gap + sz } else { sz };
        }
    }
    if start < sizes.len() {
        runs.push((start, sizes.len()));
    }
    runs
}

/// Largest problem size (`lines * items²`) the O(N·n²) assignment search is run for; above it
/// the container keeps the greedy lines rather than stalling layout.
const MAX_DP_WORK: usize = 40_000_000;

/// Break `sizes` into lines the balanced way: the line count is the greedy count raised to
/// `min_lines` (capped by the item count), and among the assignments with that many lines
/// whose multi-item lines all fit `avail`, the one with the smallest sum of squared leftover
/// space wins. Ties go to the assignment with more items on the first line, then the second…
/// Returns `None` when there is nothing to balance (≤ 1 line) so the caller keeps its lines.
pub(super) fn balanced_lines(sizes: &[f32], gap: f32, avail: f32, min_lines: u32) -> Option<Vec<Vec<usize>>> {
    let n = sizes.len();
    let greedy = greedy_breaks(sizes, gap, avail);
    let lines = greedy.len().max(min_lines as usize).min(n);
    if lines <= 1 || lines.saturating_mul(n).saturating_mul(n) > MAX_DP_WORK {
        return None;
    }
    // prefix[i] = sum of sizes[..i]; a run i..j is prefix[j]-prefix[i] + gap*(j-i-1) wide.
    let mut prefix = vec![0.0_f64; n + 1];
    for (i, &s) in sizes.iter().enumerate() {
        prefix[i + 1] = prefix[i] + f64::from(s);
    }
    let (gap, avail) = (f64::from(gap), f64::from(avail));
    // Squared leftover space of the run `i..j`; `None` when more than one item overflows.
    let cost = |i: usize, j: usize| -> Option<f64> {
        let w = prefix[j] - prefix[i] + gap * (j - i - 1) as f64;
        if j - i == 1 {
            // A lone item cannot do anything about overflowing; do not let it skew the rest.
            Some((avail - w).max(0.0).powi(2))
        } else if w > avail + 1e-3 {
            None
        } else {
            Some((avail - w).powi(2))
        }
    };
    // best[l][i]: least cost of laying items i.. onto exactly l lines; `cut[l][i]` is the end
    // of the first of them.
    let inf = f64::INFINITY;
    let mut best = vec![vec![inf; n + 1]; lines + 1];
    let mut cut = vec![vec![0_usize; n + 1]; lines + 1];
    best[0][n] = 0.0;
    for l in 1..=lines {
        for i in 0..n {
            // Leave at least one item for each of the remaining l-1 lines.
            let last_end = n - (l - 1);
            let mut b = inf;
            let mut c = 0;
            for j in (i + 1..=last_end).rev() {
                let Some(first) = cost(i, j) else { continue };
                let rest = best[l - 1][j];
                if rest == inf {
                    continue;
                }
                let total = first + rest;
                // Walking `j` downwards with a strict improvement keeps the largest `j` on a tie.
                if total < b - 1e-6 {
                    b = total;
                    c = j;
                }
            }
            best[l][i] = b;
            cut[l][i] = c;
        }
    }
    if best[lines][0] == inf {
        return None;
    }
    let mut out = Vec::with_capacity(lines);
    let mut i = 0;
    for l in (1..=lines).rev() {
        let j = cut[l][i];
        out.push((i..j).collect());
        i = j;
    }
    Some(out)
}

/// Max-content main size of a balanced row container: the narrowest the widest of `lines`
/// lines can be made (`sizes` are outer max-content sizes, `gap` between neighbours).
pub(super) fn balanced_max_content(sizes: &[f32], gap: f32, lines: u32) -> f32 {
    let n = sizes.len();
    let total = sizes.iter().sum::<f32>() + gap * n.saturating_sub(1) as f32;
    let lines = (lines as usize).min(n);
    if lines <= 1 || lines.saturating_mul(n) > 4_000_000 {
        return total;
    }
    // Bisect on the line width: the least width at which greedy packing needs ≤ `lines` lines.
    let mut lo = sizes.iter().copied().fold(0.0_f32, f32::max);
    let mut hi = total;
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if greedy_breaks(sizes, gap, mid).len() <= lines {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    // Snap to the real width of the widest line packed at `hi` (drops the bisection residue).
    greedy_breaks(sizes, gap, hi)
        .iter()
        .map(|&(i, j)| sizes[i..j].iter().sum::<f32>() + gap * (j - i - 1) as f32)
        .fold(0.0_f32, f32::max)
}

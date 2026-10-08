//! CSS Shapes L1 §3 — per-line wrapping of an `InlineRun` around floats that
//! carry a `shape-outside`.
//!
//! `wrap_inline_run` wraps a paragraph at one width, so a run laid out beside a
//! circle/polygon float used to keep the float's rectangular margin box as its
//! band on every line. [`wrap_around_shapes`] re-wraps line by line instead:
//! each line box asks the [`FloatContext`] for the band over its own vertical
//! extent ([`FloatContext::line_band`]) and is wrapped, aligned and shifted
//! inside it, so the text follows the contour.

use super::*;

/// Inputs of [`wrap_around_shapes`]. All x/y are absolute (the space of the
/// [`FloatContext`]); `left`/`width` is the run's full content box.
pub(crate) struct ShapedRun<'a> {
    pub(crate) fc: &'a FloatContext,
    pub(crate) segments: &'a [InlineSegment],
    pub(crate) style: &'a ComputedStyle,
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) width: f32,
    /// Height of one line box (`line-height-step` already applied).
    pub(crate) line_h: f32,
    /// `text-indent` (+ marker inset) of the first line.
    pub(crate) text_indent: f32,
    pub(crate) viewport: Size,
}

/// Whether `run` can use [`wrap_around_shapes`]: a plain horizontal paragraph
/// that wraps and has no feature whose bookkeeping lives in the single-width
/// pass (`::first-line`, `text-wrap: balance|pretty`, `line-clamp`, ellipsis,
/// a row continuation, preserved whitespace) — those keep the old path.
pub(crate) fn eligible(
    s: &ComputedStyle,
    first_line_style: bool,
    row_continuation: bool,
    fc: Option<&FloatContext>,
    top: f32,
) -> bool {
    fc.is_some_and(|f| f.shapes_bottom() > top)
        && !first_line_style
        && !row_continuation
        && !s.white_space.is_nowrap()
        && !s.white_space.preserves_whitespace()
        && s.text_wrap_mode != TextWrapMode::Nowrap
        && matches!(s.text_wrap_style, TextWrapStyle::Auto | TextWrapStyle::Stable)
        && s.line_clamp.is_none_or(|n| n == 0)
        && s.text_overflow != TextOverflow::Ellipsis
}

/// Right extent of a wrapped line (frags ascend in x).
fn line_extent(line: &[InlineFrag]) -> f32 {
    line.last().map_or(0.0, |f| f.x + f.width)
}

/// Wraps `run.segments` one line box at a time; each line is also aligned
/// (`text-align`) inside its own band and shifted to the band's left edge, so
/// the caller must not run `align_lines` again. Hyphenation is off: a line
/// that ended mid-word would make the word-level remainder split ambiguous
/// (same trade-off as the `::first-line` first pass).
pub(crate) fn wrap_around_shapes(
    run: &ShapedRun,
    m: &dyn TextMeasurer,
    hp: &dyn HyphenationProvider,
) -> Vec<Vec<InlineFrag>> {
    let s = run.style;
    let right = run.left + run.width;
    let mut rest: Vec<InlineSegment> = run.segments.to_vec();
    let mut out: Vec<Vec<InlineFrag>> = Vec::new();
    let mut y = run.top;
    let mut indent = run.text_indent;
    // Every pass emits at least one line and moves `y` down; the cap only
    // guards against a degenerate `line_h`.
    for _ in 0..20_000 {
        if rest.is_empty() {
            break;
        }
        let band = run.fc.line_band(y, y + run.line_h, run.left, right);
        let width = (band.1 - band.0).max(0.0);
        let wrapped = wrap_inline_run(
            &rest, width, s.font_size, indent, run.viewport, m, Hyphens::None, hp,
            s.white_space, s.word_break, s.overflow_wrap, s.line_break,
        );
        if wrapped.is_empty() {
            break;
        }
        let narrowed = band.0 > run.left || band.1 < right;
        // CSS 2.1 §9.5: a line box that cannot hold even its first word beside
        // the float moves below it. The line grid is uniform, so the skipped
        // row stays in the run as a blank line.
        if narrowed && line_extent(&wrapped[0]) > width + 0.5 {
            out.push(Vec::new());
            y += run.line_h;
            continue;
        }
        let n = wrapped.len();
        let mut taken = 0usize;
        let mut exhausted = false;
        for (j, mut line) in wrapped.into_iter().enumerate() {
            if j > 0 {
                let yj = y + j as f32 * run.line_h;
                if run.fc.line_band(yj, yj + run.line_h, run.left, right) != band {
                    break;
                }
            }
            if line.is_empty() {
                // Only whitespace left: nothing more to place.
                if out.is_empty() {
                    out.push(line);
                }
                exhausted = true;
                break;
            }
            rest = split_segments_at_first_line(&rest, &line, false).1;
            align_one_line(&mut line, j + 1 == n, width, s.text_align, s.text_align_last, s.direction);
            let shift = band.0 - run.left;
            if shift != 0.0 {
                for f in &mut line {
                    f.x += shift;
                }
            }
            out.push(line);
            taken += 1;
        }
        if exhausted {
            break;
        }
        y += taken as f32 * run.line_h;
        indent = 0.0;
    }
    out
}

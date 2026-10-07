//! Line wrapping of one text segment under `white-space: pre-wrap` /
//! `break-spaces` (CSS Text L3 §4.1.3).
//!
//! `pre` keeps a segment as one unbreakable fragment (see `wrap_inline_run`).
//! The two wrapping preserved modes keep every space and tab too, but the
//! line may break after a space: [`wrap_preserved_segment`] cuts the segment
//! at those soft wrap opportunities and fills lines greedily. Consecutive
//! tokens that stay on one line are emitted as a single fragment, so the text
//! keeps its shaping and the display list does not grow a fragment per word.
//!
//! * `pre-wrap` — a run of spaces *hangs* at the end of a line: it never forces
//!   a wrap, only the word before it must fit.
//! * `break-spaces` — every space is a wrap opportunity of its own and takes
//!   room like any other character.

use super::*;

/// Everything [`wrap_preserved_segment`] needs besides the line state.
pub(crate) struct PreservedWrap<'a> {
    pub(crate) max_width: f32,
    pub(crate) viewport: Size,
    pub(crate) m: &'a dyn TextMeasurer,
    pub(crate) white_space: crate::style::WhiteSpace,
    pub(crate) word_break: WordBreak,
    pub(crate) overflow_wrap: OverflowWrap,
    pub(crate) line_break: LineBreak,
    /// Whether a line may break between the previous segment and this one
    /// (the previous text ended with a wrap opportunity).
    pub(crate) break_before: bool,
}

/// A preserved space: the only characters after which `pre-wrap` may wrap.
fn is_preserved_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Byte offsets inside `text` at which a line may break (strictly between 0
/// and `text.len()`), in ascending order.
fn break_offsets(text: &str, p: &PreservedWrap<'_>, style: &ComputedStyle) -> Vec<usize> {
    let break_spaces = p.white_space == crate::style::WhiteSpace::BreakSpaces;
    let lb_allowed = p.word_break != WordBreak::KeepAll || p.line_break == LineBreak::Anywhere;
    let ow_chars = p.word_break == WordBreak::BreakWord
        || matches!(p.overflow_wrap, OverflowWrap::BreakWord | OverflowWrap::Anywhere);
    let ls = style.letter_spacing;
    let mut out = Vec::new();

    let mut chars = text.char_indices().peekable();
    // Start of the current run of non-space characters.
    let mut word_start = 0usize;
    while let Some((off, ch)) = chars.next() {
        let next_is_space = chars.peek().is_some_and(|&(_, n)| is_preserved_space(n));
        if is_preserved_space(ch) {
            let after = off + ch.len_utf8();
            word_start = after;
            // `pre-wrap` breaks once, after the whole run of spaces.
            if after < text.len() && (break_spaces || !next_is_space) {
                out.push(after);
            }
            continue;
        }
        // End of a word: a space or the end of the text follows.
        let word_ends = chars.peek().is_none() || next_is_space;
        if !word_ends {
            continue;
        }
        let word_end = off + ch.len_utf8();
        let word = &text[word_start..word_end];
        let inner: Vec<usize> = if p.word_break == WordBreak::BreakAll {
            // CSS Text L3 §5.1: a break between any two characters.
            word.char_indices().skip(1).map(|(o, _)| o).collect()
        } else {
            let mut opps = if lb_allowed {
                crate::line_break::break_opportunities(word, p.line_break)
            } else {
                Vec::new()
            };
            if ow_chars && opps.is_empty() {
                // CSS Text L3 §8.1: a word wider than the line breaks anywhere —
                // the other opportunities are tried first, so only an unbroken
                // word that is too wide falls back to character cuts.
                let w = measure_text_w_varied(
                    word, style.font_size, ls, style.tab_size,
                    &style.font_family, &style.font_variation_settings, p.m,
                );
                if w > p.max_width {
                    opps = word.char_indices().skip(1).map(|(o, _)| o).collect();
                }
            }
            opps
        };
        out.extend(inner.into_iter().map(|o| word_start + o));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Wraps `seg` (preserved whitespace, wrapping allowed) into `result` /
/// `current_line`. `current_x` is the pen position on the current line and is
/// advanced past the segment.
pub(crate) fn wrap_preserved_segment(
    seg: &InlineSegment,
    p: &PreservedWrap<'_>,
    result: &mut Vec<Vec<InlineFrag>>,
    current_line: &mut Vec<InlineFrag>,
    current_x: &mut f32,
) {
    let style = &seg.style;
    let em = style.font_size;
    let ls = style.letter_spacing;
    let tab_size = style.tab_size;
    let text = seg.text.as_str();
    let break_spaces = p.white_space == crate::style::WhiteSpace::BreakSpaces;
    let pad_l = style.padding_left.resolve_or_zero(em, p.max_width, p.viewport);
    let pad_r = style.padding_right.resolve_or_zero(em, p.max_width, p.viewport);
    let measure = |s: &str| {
        measure_text_w_varied(
            s, em, ls, tab_size, &style.font_family, &style.font_variation_settings, p.m,
        )
    };

    // Token boundaries: `bounds[i]..bounds[i + 1]` is token `i`.
    let mut bounds = vec![0usize];
    bounds.extend(break_offsets(text, p, style));
    bounds.push(text.len());

    // Start of the piece of the segment that stays on the current line and
    // has not been emitted as a fragment yet.
    let mut piece_start = 0usize;
    // Width of that pending piece (token widths, letter-spacing between them).
    let mut pending_w = 0.0_f32;
    let mut first_piece = true;
    *current_x += seg.pre_space;

    let last = bounds.len() - 2;
    for i in 0..=last {
        let (s, e) = (bounds[i], bounds[i + 1]);
        let token = &text[s..e];
        // Spaces hang off the end of the line under `pre-wrap`; only the part
        // before them has to fit. `break-spaces` counts the spaces too.
        let fit = if break_spaces { token } else { token.trim_end_matches(is_preserved_space) };
        let mut fit_w = measure(fit);
        if i == last {
            fit_w += seg.post_space;
        }
        let full_w = measure(token) + if i == last { seg.post_space } else { 0.0 };

        let line_empty = current_line.is_empty() && piece_start == s;
        let breakable = if i == 0 { p.break_before } else { true };
        if !line_empty && breakable && *current_x + pending_w + fit_w > p.max_width {
            // Emit what fits, then continue the token on a fresh line.
            emit_piece(
                seg, &text[piece_start..s], piece_start, first_piece, false,
                pad_l, pad_r, &measure, current_line, current_x,
            );
            first_piece &= piece_start == s;
            result.push(std::mem::take(current_line));
            *current_x = 0.0;
            piece_start = s;
            pending_w = 0.0;
        }
        pending_w += full_w + if pending_w > 0.0 { ls } else { 0.0 };
    }
    emit_piece(
        seg, &text[piece_start..], piece_start, first_piece, true,
        pad_l, pad_r, &measure, current_line, current_x,
    );
    *current_x += seg.post_space;
}

/// Pushes `piece` as a fragment of the current line and advances `current_x`.
/// An empty piece (the segment broke right at its start) emits nothing.
#[allow(clippy::too_many_arguments)]
fn emit_piece(
    seg: &InlineSegment,
    piece: &str,
    piece_start: usize,
    is_first: bool,
    is_last: bool,
    pad_l: f32,
    pad_r: f32,
    measure: &dyn Fn(&str) -> f32,
    current_line: &mut Vec<InlineFrag>,
    current_x: &mut f32,
) {
    if piece.is_empty() {
        return;
    }
    let width = measure(piece);
    current_line.push(InlineFrag {
        x: *current_x,
        y_offset: 0.0,
        width,
        text: piece.to_string(),
        style: seg.style.clone(),
        padding_left: if is_first { pad_l } else { 0.0 },
        padding_right: if is_last { pad_r } else { 0.0 },
        is_element_box: seg.is_element_box,
        img_src: None,
        img_is_lazy: false,
        is_first_line: false,
        source_node: seg.source_node,
        source_char_offset: seg.source_char_offset.saturating_add(piece_start as u32),
        bidi_level: seg.bidi_level,
        merged_sources: Vec::new(),
    });
    *current_x += width;
}

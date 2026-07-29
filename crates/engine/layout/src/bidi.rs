//! UAX #9 — Unicode Bidirectional Algorithm for the inline formatting context.
//!
//! Three stages, called from `box_tree`:
//!
//! 1. [`resolve`] runs the algorithm's rules P2–I2 over the whole paragraph
//!    (all [`InlineSegment`]s of one `BoxKind::InlineRun`) and hands back a
//!    segment list split at every embedding-level boundary, each piece tagged
//!    with its [`InlineSegment::bidi_level`]. `unicode-bidi` on a segment is
//!    expressed exactly as CSS Writing Modes L4 §2.2 defines it — by wrapping
//!    the segment's text in explicit bidi control characters before the run.
//! 2. [`reorder_line`] applies rule L2 to one finished line box, turning the
//!    logical fragment order into the visual one. It reflects fragments inside
//!    the geometric span of each reordered run, so inter-word gaps survive.
//! 3. [`visual_text`] applies the character-level half of L2 plus rule L4
//!    (mirrored glyphs) to a single right-to-left fragment. Lumen's renderers
//!    lay glyphs out left-to-right with no bidi awareness of their own, so an
//!    RTL fragment is handed to paint already reversed.
//!
//! # Known limitation — flat segment list
//!
//! `collect_inline_segments` flattens the inline box tree, so a non-`normal`
//! `unicode-bidi` is wrapped **per segment run**, not per inline element.
//! `<span style="unicode-bidi:isolate">abc <b>def</b> ghi</span>` therefore
//! produces two isolates (`abc `, ` ghi`) around a bare `def` instead of one
//! isolate around all three: `unicode-bidi` does not inherit, so the `<b>`
//! segment carries `normal` and breaks the group. Single-text-node cases —
//! `<bdo>`, `<bdi>`, `<span dir=rtl>` — are exact.

use std::borrow::Cow;

use unicode_bidi::BidiInfo;

use crate::box_tree::{InlineFrag, InlineSegment};
use crate::style::{Direction, UnicodeBidi};

/// LEFT-TO-RIGHT EMBEDDING.
const LRE: char = '\u{202A}';
/// RIGHT-TO-LEFT EMBEDDING.
const RLE: char = '\u{202B}';
/// POP DIRECTIONAL FORMATTING — closes `LRE`/`RLE`/`LRO`/`RLO`.
const PDF: char = '\u{202C}';
/// LEFT-TO-RIGHT OVERRIDE.
const LRO: char = '\u{202D}';
/// RIGHT-TO-LEFT OVERRIDE.
const RLO: char = '\u{202E}';
/// LEFT-TO-RIGHT ISOLATE.
const LRI: char = '\u{2066}';
/// RIGHT-TO-LEFT ISOLATE.
const RLI: char = '\u{2067}';
/// FIRST STRONG ISOLATE — base direction taken from the content (rule P3).
const FSI: char = '\u{2068}';
/// POP DIRECTIONAL ISOLATE — closes `LRI`/`RLI`/`FSI`.
const PDI: char = '\u{2069}';
/// OBJECT REPLACEMENT CHARACTER — stands in for a replaced element (an inline
/// `<img>`) so it participates in the algorithm as a neutral, and its `alt`
/// text does not leak into the paragraph's directional resolution.
const OBJECT: char = '\u{FFFC}';

/// Explicit bidi controls that wrap a segment's text, per CSS Writing Modes
/// L4 §2.2 «Table: bidi control codes injected».
fn wrapping(bidi: UnicodeBidi, dir: Direction) -> (&'static [char], &'static [char]) {
    let rtl = dir == Direction::Rtl;
    match bidi {
        UnicodeBidi::Normal => (&[], &[]),
        UnicodeBidi::Embed => (if rtl { &[RLE] } else { &[LRE] }, &[PDF]),
        UnicodeBidi::Isolate => (if rtl { &[RLI] } else { &[LRI] }, &[PDI]),
        UnicodeBidi::BidiOverride => (if rtl { &[RLO] } else { &[LRO] }, &[PDF]),
        UnicodeBidi::IsolateOverride => (
            if rtl { &[FSI, RLO] } else { &[FSI, LRO] },
            &[PDF, PDI],
        ),
        UnicodeBidi::Plaintext => (&[FSI], &[PDI]),
    }
}

/// True when `text` can possibly carry a non-zero embedding level: it holds a
/// right-to-left character, an Arabic number, or an explicit bidi control.
///
/// The overwhelming majority of inline text is ASCII, which this rejects with a
/// single word-at-a-time scan — the whole bidi pass is skipped for it.
fn has_bidi_relevant_char(text: &str) -> bool {
    if text.is_ascii() {
        return false;
    }
    text.chars().any(|c| {
        matches!(c as u32,
            0x0590..=0x08FF   // Hebrew, Arabic, Syriac, Thaana, NKo, Samaritan, …
            | 0x200F          // RLM
            | 0x200E          // LRM
            | 0x202A..=0x202E // LRE RLE PDF LRO RLO
            | 0x2066..=0x2069 // LRI RLI FSI PDI
            | 0xFB1D..=0xFDFF // Hebrew/Arabic presentation forms A
            | 0xFE70..=0xFEFF // Arabic presentation forms B
        ) || (0x10800..=0x10FFF).contains(&(c as u32))   // Kharoshthi … Old Hungarian
            || (0x1E800..=0x1EFFF).contains(&(c as u32)) // Mende Kikakui, Adlam, Arabic Math
    })
}

/// True when the paragraph needs the full algorithm run at all.
///
/// A left-to-right block whose every segment is plain `unicode-bidi: normal`
/// left-to-right text resolves to level 0 everywhere, which is what the
/// fragments already default to — [`resolve`] would be a no-op.
pub(crate) fn needs_resolution(segments: &[InlineSegment], base: Direction) -> bool {
    base == Direction::Rtl
        || segments.iter().any(|s| {
            s.style.unicode_bidi != UnicodeBidi::Normal
                || s.style.direction == Direction::Rtl
                || has_bidi_relevant_char(&s.text)
        })
}

/// Runs UAX #9 rules P2–I2 over the paragraph formed by `segments` and returns
/// the same content split at every embedding-level boundary.
///
/// Each returned piece carries a uniform [`InlineSegment::bidi_level`]. Splits
/// happen at character boundaries, so a single word straddling a level change
/// (`"abc123"` in an RTL paragraph) becomes two pieces — which is exactly what
/// L2 needs in order to reorder its halves independently.
///
/// A `forced_break` segment ends the bidi paragraph (rule P1): the algorithm
/// restarts on the next line with the same base level.
pub(crate) fn resolve(segments: &[InlineSegment], base: Direction) -> Vec<InlineSegment> {
    // Paragraph text, plus one mark per content character recording where it
    // came from. Injected control characters get no mark.
    let mut text = String::new();
    let mut marks: Vec<Mark> = Vec::new();

    let mut i = 0usize;
    while i < segments.len() {
        // Adjacent segments sharing the same non-`normal` `unicode-bidi` are
        // wrapped once, as a group: a `<span dir=rtl>` split into several
        // segments by whitespace handling must stay a single embedding.
        let group_bidi = segments[i].style.unicode_bidi;
        let group_dir = segments[i].style.direction;
        let mut j = i + 1;
        if group_bidi != UnicodeBidi::Normal {
            while j < segments.len()
                && segments[j].style.unicode_bidi == group_bidi
                && segments[j].style.direction == group_dir
            {
                j += 1;
            }
        }
        let (open, close) = wrapping(group_bidi, group_dir);
        text.extend(open.iter());
        for (seg_idx, seg) in segments[i..j].iter().enumerate().map(|(k, s)| (i + k, s)) {
            if seg.forced_break {
                // Rule P1 — a paragraph separator; `BidiInfo` restarts here.
                text.push('\n');
            } else if seg.img_src.is_some() {
                marks.push(Mark { para_byte: text.len(), seg_idx, seg_byte: 0 });
                text.push(OBJECT);
            } else {
                for (off, ch) in seg.text.char_indices() {
                    marks.push(Mark { para_byte: text.len(), seg_idx, seg_byte: off });
                    text.push(ch);
                }
            }
        }
        text.extend(close.iter());
        i = j;
    }

    let para_level = unicode_bidi::Level::new(u8::from(base == Direction::Rtl)).ok();
    let info = BidiInfo::new(&text, para_level);

    // `BidiInfo::levels` is indexed by byte; every byte of a multi-byte
    // character carries that character's level.
    let mut per_seg: Vec<Vec<(usize, u8)>> = vec![Vec::new(); segments.len()];
    for mark in &marks {
        let level = info
            .levels
            .get(mark.para_byte)
            .map(|l| l.number())
            .unwrap_or(0);
        per_seg[mark.seg_idx].push((mark.seg_byte, level));
    }

    let mut out: Vec<InlineSegment> = Vec::with_capacity(segments.len());
    for (idx, seg) in segments.iter().enumerate() {
        let levels = &per_seg[idx];
        let Some(&(_, first_level)) = levels.first() else {
            // Forced break or empty text — nothing to level, keep as is.
            out.push(seg.clone());
            continue;
        };
        if seg.img_src.is_some() {
            let mut piece = seg.clone();
            piece.bidi_level = first_level;
            out.push(piece);
            continue;
        }
        // Byte offsets at which the level changes, plus the text end.
        let mut cuts: Vec<(usize, u8)> = vec![(0, first_level)];
        for &(off, level) in levels.iter().skip(1) {
            if level != cuts[cuts.len() - 1].1 {
                cuts.push((off, level));
            }
        }
        if cuts.len() == 1 {
            let mut piece = seg.clone();
            piece.bidi_level = first_level;
            out.push(piece);
            continue;
        }
        for (n, &(start, level)) in cuts.iter().enumerate() {
            let end = cuts.get(n + 1).map(|&(e, _)| e).unwrap_or(seg.text.len());
            let mut piece = seg.clone();
            piece.text = seg.text[start..end].to_string();
            piece.bidi_level = level;
            piece.source_char_offset = seg.source_char_offset + start as u32;
            // Inline-box edges belong to the outer pieces only: the box's own
            // margin/border/padding is not duplicated at every level boundary.
            if n > 0 {
                piece.pre_space = 0.0;
                // ::first-letter marks the very start of the block only.
                piece.pseudo_kind = crate::box_tree::PseudoKind::None;
            }
            if n + 1 < cuts.len() {
                piece.post_space = 0.0;
            }
            out.push(piece);
        }
    }
    out
}

/// One content character's provenance inside the assembled paragraph text.
struct Mark {
    /// Byte offset of the character inside the paragraph string.
    para_byte: usize,
    /// Index of the source segment in the input slice.
    seg_idx: usize,
    /// Byte offset of the character inside that segment's own `text`.
    seg_byte: usize,
}

/// UAX #9 rule L2 — puts one line box's fragments into visual order by
/// rewriting their `x`.
///
/// From the highest embedding level down to the lowest odd level, every
/// contiguous run of fragments at that level or above is **reflected inside its
/// own geometric span**: `x' = span_start + span_end - (x + width)`. Reflection
/// (rather than re-packing) keeps every inter-word gap, inline-box padding and
/// image slot exactly as the line breaker measured it, and reflecting twice is
/// an exact identity — which L2 relies on for even levels.
///
/// The slice itself keeps **logical** order; only `x` moves. That is the
/// invariant the rest of the engine already assumed of the old RTL mirror, and
/// it is what keeps `text_iter` (find-in-page, accessibility text) and
/// Selection reading a bidi paragraph in reading order rather than in painted
/// order.
///
/// A run that starts at fragment 0 is reflected against `0.0` rather than
/// against its own leftmost `x`, so `text-indent`'s leading gap ends up on the
/// line's *start* edge — the right one in a right-to-left paragraph.
///
/// `base` supplies the paragraph embedding level for fragments whose level was
/// never resolved (the no-measurer fallback path builds fragments at level 0).
pub(crate) fn reorder_line(frags: &mut [InlineFrag], base: Direction) {
    if frags.len() < 2 {
        return;
    }
    let base_level = u8::from(base == Direction::Rtl);
    let level_of = |f: &InlineFrag| f.bidi_level.max(base_level);
    let max = frags.iter().map(level_of).max().unwrap_or(0);
    if max == 0 {
        return;
    }
    // Lowest odd level present, counting each even level as the odd one above
    // it (UAX #9: «including intermediate levels not actually present»).
    let lowest_odd = frags
        .iter()
        .map(|f| level_of(f) | 1)
        .min()
        .unwrap_or(1);

    let mut level = max;
    while level >= lowest_odd {
        let mut i = 0usize;
        while i < frags.len() {
            if level_of(&frags[i]) < level {
                i += 1;
                continue;
            }
            let start = i;
            while i < frags.len() && level_of(&frags[i]) >= level {
                i += 1;
            }
            reflect(&mut frags[start..i], start == 0);
        }
        if level == 0 {
            break;
        }
        level -= 1;
    }
}

/// Mirrors each fragment's `x` inside the run's own geometric span.
///
/// The span is taken as min/max over the run rather than from its first and
/// last element: a lower-level pass sees runs whose inner, higher-level parts
/// have already been reflected, so logical position no longer implies
/// left-to-right position.
fn reflect(run: &mut [InlineFrag], at_line_start: bool) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for frag in run.iter() {
        lo = lo.min(frag.x);
        hi = hi.max(frag.x + frag.width);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return;
    }
    let span_start = if at_line_start { 0.0 } else { lo };
    for frag in run.iter_mut() {
        frag.x = span_start + hi - (frag.x + frag.width);
    }
}

/// UAX #9 rules L2 (character half) and L4 — the text of `level`'s fragment as
/// the renderer must lay it out left to right.
///
/// Even (left-to-right) levels are returned untouched. Odd levels get their
/// grapheme clusters reversed — combining marks stay attached to the base
/// character they modify — and every mirrorable character (brackets, angle
/// quotes, mathematical relations) replaced by its `Bidi_Mirroring_Glyph`.
///
/// Lumen's rasterizers advance strictly left to right and do no bidi work of
/// their own, so this is the point where right-to-left runs become visual.
pub fn visual_text(text: &str, level: u8) -> Cow<'_, str> {
    if level.is_multiple_of(2) || text.is_empty() {
        return Cow::Borrowed(text);
    }
    // Cluster starts: a base character plus any combining marks following it.
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    for (off, ch) in text.char_indices() {
        if crate::line_break::is_combining(ch) && !clusters.is_empty() {
            let last = clusters.len() - 1;
            clusters[last].1 = off + ch.len_utf8();
        } else {
            clusters.push((off, off + ch.len_utf8()));
        }
    }
    let mut out = String::with_capacity(text.len());
    for &(start, end) in clusters.iter().rev() {
        for ch in text[start..end].chars() {
            out.push(unicode_bidi_mirroring::get_mirrored(ch).unwrap_or(ch));
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ComputedStyle;

    /// Hebrew "שלום" — four strong-RTL letters.
    const HE: &str = "שלום";

    fn seg(text: &str, dir: Direction, bidi: UnicodeBidi) -> InlineSegment {
        let mut style = ComputedStyle::root();
        style.direction = dir;
        style.unicode_bidi = bidi;
        InlineSegment {
            text: text.to_string(),
            style,
            pre_space: 0.0,
            post_space: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            img_width: 0.0,
            forced_break: false,
            pseudo_kind: crate::box_tree::PseudoKind::None,
            source_node: lumen_dom::NodeId::from_index(0),
            source_char_offset: 0,
            bidi_level: 0,
        }
    }

    fn frag(x: f32, width: f32, level: u8, text: &str) -> InlineFrag {
        InlineFrag {
            x,
            width,
            y_offset: 0.0,
            text: text.to_string(),
            style: ComputedStyle::root(),
            padding_left: 0.0,
            padding_right: 0.0,
            is_element_box: false,
            img_src: None,
            img_is_lazy: false,
            is_first_line: false,
            source_node: lumen_dom::NodeId::from_index(0),
            source_char_offset: 0,
            bidi_level: level,
        }
    }

    // ── needs_resolution ─────────────────────────────────────────────────────

    #[test]
    fn plain_ltr_ascii_skips_the_algorithm() {
        let segs = [seg("hello world", Direction::Ltr, UnicodeBidi::Normal)];
        assert!(!needs_resolution(&segs, Direction::Ltr));
    }

    #[test]
    fn rtl_block_always_resolves() {
        let segs = [seg("hello", Direction::Ltr, UnicodeBidi::Normal)];
        assert!(needs_resolution(&segs, Direction::Rtl));
    }

    #[test]
    fn hebrew_text_in_ltr_block_resolves() {
        let segs = [seg(HE, Direction::Ltr, UnicodeBidi::Normal)];
        assert!(needs_resolution(&segs, Direction::Ltr));
    }

    #[test]
    fn override_on_ascii_resolves() {
        let segs = [seg("abc", Direction::Rtl, UnicodeBidi::BidiOverride)];
        assert!(needs_resolution(&segs, Direction::Ltr));
    }

    #[test]
    fn cyrillic_is_not_bidi_relevant() {
        // Non-ASCII but strictly left-to-right: must not drag in the algorithm.
        assert!(!has_bidi_relevant_char("привет"));
        assert!(has_bidi_relevant_char(HE));
    }

    // ── resolve ──────────────────────────────────────────────────────────────

    #[test]
    fn ltr_paragraph_is_all_level_zero() {
        let segs = [seg("hello", Direction::Ltr, UnicodeBidi::Normal)];
        let out = resolve(&segs, Direction::Ltr);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bidi_level, 0);
    }

    #[test]
    fn hebrew_in_ltr_paragraph_gets_level_one() {
        let segs = [seg(HE, Direction::Ltr, UnicodeBidi::Normal)];
        let out = resolve(&segs, Direction::Ltr);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bidi_level, 1, "strong RTL letters resolve to level 1");
    }

    #[test]
    fn multibyte_levels_are_indexed_by_byte_not_char() {
        // "a" + Hebrew: if the level lookup were indexed by char instead of by
        // byte, the two-byte Hebrew letters would read neighbouring levels and
        // the split would land in the wrong place.
        let text = format!("a{HE}");
        let segs = [seg(&text, Direction::Ltr, UnicodeBidi::Normal)];
        let out = resolve(&segs, Direction::Ltr);
        assert_eq!(out.len(), 2, "one split between the Latin and Hebrew runs");
        assert_eq!(out[0].text, "a");
        assert_eq!(out[0].bidi_level, 0);
        assert_eq!(out[1].text, HE);
        assert_eq!(out[1].bidi_level, 1);
        assert_eq!(out[1].source_char_offset, 1);
    }

    #[test]
    fn rtl_paragraph_puts_latin_at_level_two() {
        let segs = [seg("abc", Direction::Rtl, UnicodeBidi::Normal)];
        let out = resolve(&segs, Direction::Rtl);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bidi_level, 2, "Latin inside an RTL paragraph nests one level");
    }

    #[test]
    fn bidi_override_forces_rtl_on_latin() {
        // `unicode-bidi: bidi-override` + `direction: rtl` injects RLO, which
        // makes even plain Latin resolve to an odd level.
        let segs = [seg("abc", Direction::Rtl, UnicodeBidi::BidiOverride)];
        let out = resolve(&segs, Direction::Ltr);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bidi_level, 1);
    }

    #[test]
    fn normal_latin_stays_ltr_without_the_override() {
        let segs = [seg("abc", Direction::Rtl, UnicodeBidi::Normal)];
        let out = resolve(&segs, Direction::Ltr);
        assert_eq!(out[0].bidi_level, 0, "`direction` alone must not move the level");
    }

    #[test]
    fn isolate_keeps_the_neighbour_out_of_the_run() {
        // Without isolation the trailing digits would join the Hebrew run's
        // number context; `isolate` walls the segment off.
        let segs = [
            seg(HE, Direction::Rtl, UnicodeBidi::Isolate),
            seg("12", Direction::Ltr, UnicodeBidi::Normal),
        ];
        let out = resolve(&segs, Direction::Ltr);
        let digits = out.last().expect("digit segment survives");
        assert_eq!(digits.text, "12");
        assert_eq!(digits.bidi_level, 0, "isolated neighbour does not raise the digits");
    }

    #[test]
    fn plaintext_takes_direction_from_content() {
        // `plaintext` ignores `direction: ltr` and uses first-strong (rule P3),
        // so Hebrew content still resolves right-to-left.
        let segs = [seg(HE, Direction::Ltr, UnicodeBidi::Plaintext)];
        let out = resolve(&segs, Direction::Ltr);
        assert_eq!(out[0].bidi_level, 1);
    }

    #[test]
    fn split_moves_pre_and_post_space_to_the_outer_pieces() {
        let mut s = seg(&format!("a{HE}"), Direction::Ltr, UnicodeBidi::Normal);
        s.pre_space = 5.0;
        s.post_space = 7.0;
        let out = resolve(&[s], Direction::Ltr);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].pre_space, out[0].post_space), (5.0, 0.0));
        assert_eq!((out[1].pre_space, out[1].post_space), (0.0, 7.0));
    }

    #[test]
    fn forced_break_segment_is_passed_through() {
        let mut br = seg("", Direction::Ltr, UnicodeBidi::Normal);
        br.forced_break = true;
        let out = resolve(&[br], Direction::Rtl);
        assert_eq!(out.len(), 1);
        assert!(out[0].forced_break);
    }

    // ── reorder_line ─────────────────────────────────────────────────────────

    #[test]
    fn ltr_line_is_left_untouched() {
        let mut line = vec![frag(0.0, 30.0, 0, "one"), frag(35.0, 40.0, 0, "two")];
        reorder_line(&mut line, Direction::Ltr);
        assert_eq!(line[0].text, "one");
        assert_eq!(line[0].x, 0.0);
        assert_eq!(line[1].x, 35.0);
    }

    /// Fragment texts sorted by their painted `x` — the visual reading order.
    fn visual_order(line: &[InlineFrag]) -> Vec<&str> {
        let mut by_x: Vec<&InlineFrag> = line.iter().collect();
        by_x.sort_by(|a, b| a.x.total_cmp(&b.x));
        by_x.iter().map(|f| f.text.as_str()).collect()
    }

    #[test]
    fn rtl_line_mirrors_word_order_and_keeps_the_gap() {
        let mut line = vec![frag(0.0, 30.0, 1, "one"), frag(35.0, 40.0, 1, "two")];
        reorder_line(&mut line, Direction::Rtl);
        assert_eq!(visual_order(&line), ["two", "one"]);
        assert_eq!(line[1].x, 0.0, "last logical word is leftmost");
        assert_eq!(line[0].x, 45.0, "5px gap preserved, now left of 'one'");
    }

    #[test]
    fn logical_order_of_the_slice_is_never_disturbed() {
        // `text_iter` and Selection walk the slice in order; only `x` may move.
        let mut line = vec![frag(0.0, 30.0, 1, "one"), frag(35.0, 40.0, 1, "two")];
        reorder_line(&mut line, Direction::Rtl);
        let logical: Vec<&str> = line.iter().map(|f| f.text.as_str()).collect();
        assert_eq!(logical, ["one", "two"]);
    }

    #[test]
    fn embedded_ltr_run_keeps_its_internal_order() {
        // RTL paragraph: [rtl0] [ltr1 ltr2] [rtl3] → the Latin pair stays in
        // left-to-right order while everything else mirrors.
        let mut line = vec![
            frag(0.0, 10.0, 1, "he0"),
            frag(10.0, 10.0, 2, "en1"),
            frag(20.0, 10.0, 2, "en2"),
            frag(30.0, 10.0, 1, "he3"),
        ];
        reorder_line(&mut line, Direction::Rtl);
        assert_eq!(visual_order(&line), ["he3", "en1", "en2", "he0"]);
    }

    #[test]
    fn double_reflection_of_even_levels_is_an_identity() {
        // LTR paragraph with digits at level 2: L2 reflects them at level 2 and
        // again at level 1, which must land them exactly where they started.
        let mut line = vec![
            frag(0.0, 10.0, 0, "abc"),
            frag(12.0, 10.0, 2, "12"),
            frag(24.0, 10.0, 2, "34"),
        ];
        reorder_line(&mut line, Direction::Ltr);
        let xs: Vec<f32> = line.iter().map(|f| f.x).collect();
        assert_eq!(xs, [0.0, 12.0, 24.0]);
    }

    #[test]
    fn text_indent_gap_moves_to_the_right_edge_in_rtl() {
        // A leading `text-indent` of 20px: after mirroring the words must be
        // packed against x=0 and the gap must end up past the last word.
        let mut line = vec![frag(20.0, 30.0, 1, "one"), frag(55.0, 40.0, 1, "two")];
        reorder_line(&mut line, Direction::Rtl);
        assert_eq!(visual_order(&line), ["two", "one"]);
        assert_eq!(line[1].x, 0.0);
        assert_eq!(line[0].x, 45.0);
        assert_eq!(line[0].x + line[0].width, 75.0, "line shortened by the indent");
    }

    #[test]
    fn unresolved_levels_still_mirror_in_an_rtl_block() {
        // The no-measurer fallback builds fragments at level 0; `base` must
        // still drive the mirror.
        let mut line = vec![frag(0.0, 30.0, 0, "one"), frag(35.0, 40.0, 0, "two")];
        reorder_line(&mut line, Direction::Rtl);
        assert_eq!(visual_order(&line), ["two", "one"]);
    }

    // ── visual_text ──────────────────────────────────────────────────────────

    #[test]
    fn even_level_text_is_borrowed_unchanged() {
        assert!(matches!(visual_text("abc", 0), Cow::Borrowed("abc")));
        assert!(matches!(visual_text("abc", 2), Cow::Borrowed("abc")));
    }

    #[test]
    fn odd_level_text_is_reversed() {
        assert_eq!(visual_text("abc", 1), "cba");
    }

    #[test]
    fn odd_level_mirrors_paired_punctuation() {
        // Rule L4: the opening paren must render as a closing one once the run
        // has been laid out right to left.
        assert_eq!(visual_text("(a)", 1), "(a)");
        assert_eq!(visual_text("(ab", 1), "ba)");
    }

    #[test]
    fn combining_marks_stay_on_their_base_character() {
        // "e" + combining acute, then "b": reversing must not detach the accent
        // and re-attach it to "b".
        assert_eq!(visual_text("e\u{0301}b", 1), "be\u{0301}");
    }

    #[test]
    fn empty_text_is_borrowed() {
        assert!(matches!(visual_text("", 1), Cow::Borrowed("")));
    }
}

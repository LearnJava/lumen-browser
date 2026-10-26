//! P1/SPLIT-DL14: text emphasis/frags/inline-run — `fn emphasis_mark_str`
//! … до конца `fn emit_first_line_background` (до `fn first_line_content_rect`,
//! которая остаётся в `display_list.rs`). Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-14).

use super::*;

/// Returns the Unicode string for a CSS `text-emphasis-style` symbol.
/// Returns empty string for `None`.
fn emphasis_mark_str(style: &TextEmphasisStyle) -> &str {
    match style {
        TextEmphasisStyle::None => "",
        TextEmphasisStyle::String(s) => s.as_str(),
        TextEmphasisStyle::Symbol { filled, shape } => match (filled, shape) {
            (true,  TextEmphasisShape::Dot)          => "\u{2022}", // •
            (false, TextEmphasisShape::Dot)          => "\u{25E6}", // ◦
            (true,  TextEmphasisShape::Circle)       => "\u{25CF}", // ●
            (false, TextEmphasisShape::Circle)       => "\u{25CB}", // ○
            (true,  TextEmphasisShape::DoubleCircle) => "\u{25C9}", // ◉
            (false, TextEmphasisShape::DoubleCircle) => "\u{25CE}", // ◎
            (true,  TextEmphasisShape::Triangle)     => "\u{25B2}", // ▲
            (false, TextEmphasisShape::Triangle)     => "\u{25B3}", // △
            (true,  TextEmphasisShape::Sesame)       => "\u{FE45}", // ﹅
            (false, TextEmphasisShape::Sesame)       => "\u{FE46}", // ﹆
        },
    }
}

/// One base character of a fragment as `text-emphasis` sees it: where its advance starts along
/// the inline axis and how long it is.
struct EmphasisSlot {
    start: f32,
    len: f32,
    /// Ideograph-class character (`is_cjk`) — upright under `text-orientation: mixed`.
    cjk: bool,
}

/// CSS Text Decoration L3 §4.2 — emphasis marks go on every typographic character unit except
/// white space and control characters; a combining mark belongs to its base.
fn emphasis_slots(text: &str, total: f32, font_size: f32) -> Vec<Option<EmphasisSlot>> {
    // The paint crate has no text measurer (see `emit_frag_text`), so the advance of a base
    // character is estimated by class: an ideograph/kana/full-width form is one em, anything
    // else is about 0.55 em; the estimates are scaled to the fragment width layout measured.
    // Exact for monospace and for pure-CJK text, within a word's variance for the rest.
    const NARROW: f32 = 0.55;
    let mut clusters: Vec<(bool, f32, bool)> = Vec::new();
    for c in text.chars() {
        let combining = matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF
            | 0x200B..=0x200F | 0x20D0..=0x20FF | 0x3099 | 0x309A | 0xFE00..=0xFE0F);
        if combining && !clusters.is_empty() {
            continue;
        }
        let cjk = lumen_layout::vertical::is_cjk(c);
        let weight = if cjk { 1.0 } else { NARROW };
        clusters.push((!(c.is_whitespace() || c.is_control()), weight, cjk));
    }
    let sum: f32 = clusters.iter().map(|&(_, w, _)| w).sum();
    if sum <= 0.0 {
        return Vec::new();
    }
    // A fragment layout could not measure (no measurer: width 0) falls back to the em estimate.
    let scale = if total > 0.0 { total / sum } else { font_size };
    let mut at = 0.0;
    clusters
        .into_iter()
        .map(|(marked, w, cjk)| {
            let slot = EmphasisSlot { start: at, len: w * scale, cjk };
            at += w * scale;
            marked.then_some(slot)
        })
        .collect()
}

/// Advance of the mark glyph, estimated for the same reason: the keyword shapes are one em of
/// the mark font, a custom string about 0.6 em per character.
fn emphasis_mark_advance(style: &TextEmphasisStyle, mark: &str, mark_size: f32) -> f32 {
    match style {
        TextEmphasisStyle::String(_) => mark.chars().count() as f32 * 0.6 * mark_size,
        _ => mark_size,
    }
}

fn emphasis_mark_cmd(frag: &InlineFrag, mark: &str, rect: Rect, mark_size: f32) -> DisplayCommand {
    DisplayCommand::DrawText {
        font_stretch: frag.style.font_stretch,
        rect,
        text: mark.to_string(),
        font_size: mark_size,
        color: frag.style.text_emphasis_color.resolve(frag.style.color),
        font_family: frag.style.font_family.clone(),
        font_weight: frag.style.font_weight,
        font_style: frag.style.font_style,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    }
}

/// CSS Text Decoration L3 §4.2 — emits one emphasis mark per base character of `frag.text`,
/// centred on the character's advance, hugging the em box of the text: above it for `over`,
/// below it for `under` (layout makes room — `emphasis_extra`).
fn emit_text_emphasis_marks(
    out: &mut Vec<DisplayCommand>,
    container_x: f32,
    frag_y: f32,
    frag: &InlineFrag,
) {
    let mark = emphasis_mark_str(&frag.style.text_emphasis_style);
    if mark.is_empty() {
        return;
    }
    let mark_size = TextEmphasisStyle::mark_size(frag.style.font_size);
    let mark_y = if frag.style.text_emphasis_position.is_over() {
        frag_y - mark_size
    } else {
        frag_y + frag.style.font_size
    };
    let mark_w = emphasis_mark_advance(&frag.style.text_emphasis_style, mark, mark_size);
    let visual = lumen_layout::bidi::visual_text(&frag.text, frag.bidi_level);
    let frag_x = container_x + frag.x;
    for slot in emphasis_slots(&visual, frag.width, frag.style.font_size).into_iter().flatten() {
        let x = frag_x + slot.start + (slot.len - mark_w) / 2.0;
        out.push(emphasis_mark_cmd(frag, mark, Rect::new(x, mark_y, mark_w, mark_size * 1.5), mark_size));
    }
}

/// Vertical counterpart of [`emit_text_emphasis_marks`] (CSS Text Decoration L3 §4.2: in a
/// vertical writing mode `right`/`left` pick the side, `over`/`under` do not apply). The marks
/// run down the column beside the glyph box, centred on each character's advance.
///
/// `column_x`/`col_width` are the column the fragment was painted in, `y0` the top of the
/// fragment along the inline axis. The glyph box is where the rasterizer puts the glyph, which
/// is not the centre of the column: an upright character starts at the column's left edge, a
/// rotated one (`mixed` Latin, `sideways`) ends at its right edge (`rasterize_text_mixed`).
fn emit_vertical_emphasis_marks(
    out: &mut Vec<DisplayCommand>,
    column_x: f32,
    col_width: f32,
    y0: f32,
    frag: &InlineFrag,
) {
    let mark = emphasis_mark_str(&frag.style.text_emphasis_style);
    if mark.is_empty() {
        return;
    }
    let fs = frag.style.font_size;
    let mark_size = TextEmphasisStyle::mark_size(fs);
    let mark_w = emphasis_mark_advance(&frag.style.text_emphasis_style, mark, mark_size);
    let right = frag.style.text_emphasis_position.is_right();
    for slot in emphasis_slots(&frag.text, frag.width, fs).into_iter().flatten() {
        let upright = match frag.style.text_orientation {
            TextOrientation::Upright => true,
            TextOrientation::Sideways => false,
            _ => slot.cjk,
        };
        let box_left = if upright { column_x } else { column_x + col_width - fs };
        let x = if right { box_left + fs } else { box_left - mark_size };
        let y = y0 + slot.start + (slot.len - mark_size) / 2.0;
        out.push(emphasis_mark_cmd(frag, mark, Rect::new(x, y, mark_w, mark_size * 1.5), mark_size));
    }
}

/// Emits shadow + DrawText + decorations for every visible frag in `line`.
///
/// A fragment a highlight (`::selection`, …) touches goes through
/// [`emit_highlighted_frag`] instead — see `lumen_layout::Highlights`.
fn emit_text_frags(
    line: &[InlineFrag],
    container_x: f32,
    container_width: f32,
    line_y: f32,
    line_h: f32,
    out: &mut Vec<DisplayCommand>,
) {
    let highlights = super::highlight_paint::active_highlights();
    for frag in line {
        if !matches!(frag.style.visibility, Visibility::Visible) {
            continue;
        }
        // Empty piece of an inline box cut by a block: box edge only, no glyphs.
        if frag.text.is_empty() && frag.img_src.is_none() {
            continue;
        }
        let frag_y = line_y + frag.y_offset;
        // Inline-replaced image: emit LazyImageSlot or DrawImage, skip text rendering.
        if let Some(src) = &frag.img_src {
            let img_rect = Rect::new(container_x + frag.x, frag_y, frag.width, line_h);
            if frag.img_is_lazy {
                // node_id unavailable in InlineFrag (no box reference); use 0 as sentinel.
                // The shell's proximity check uses the display list rects, not node_id here.
                out.push(DisplayCommand::LazyImageSlot {
                    rect: img_rect,
                    node_id: 0,
                    src: src.clone(),
                    object_fit: frag.style.object_fit,
                    object_position: frag.style.object_position,
                });
            } else {
                out.push(DisplayCommand::DrawImage {
                    rect: img_rect,
                    src: src.clone(),
                    alt: frag.text.clone(),
                    object_fit: frag.style.object_fit,
                    object_position: frag.style.object_position,
                    image_rendering: frag.style.image_rendering,
                });
            }
            continue;
        }

        let geo = FragGeometry { container_x, container_width, line_h, frag_y };
        match highlights.as_deref().and_then(|h| h.pieces(frag)) {
            Some(pieces) => super::highlight_paint::emit_highlighted_frag(out, frag, pieces, &geo),
            None => emit_frag_text(out, frag, &geo, TextParts::ALL),
        }
    }
}

/// Where one fragment sits on its line.
pub(super) struct FragGeometry {
    pub(super) container_x: f32,
    pub(super) container_width: f32,
    pub(super) line_h: f32,
    pub(super) frag_y: f32,
}

/// Which layers of a fragment's text [`emit_frag_text`] draws. A highlighted fragment
/// draws its shadows first, for all of its pieces, and the glyphs afterwards.
#[derive(Clone, Copy)]
pub(super) struct TextParts {
    pub(super) shadows: bool,
    pub(super) glyphs: bool,
}

impl TextParts {
    pub(super) const ALL: Self = Self { shadows: true, glyphs: true };
    pub(super) const SHADOWS: Self = Self { shadows: true, glyphs: false };
    pub(super) const GLYPHS: Self = Self { shadows: false, glyphs: true };
}

/// Shadows, glyphs, stroke, decorations and emphasis marks of one text fragment.
pub(super) fn emit_frag_text(out: &mut Vec<DisplayCommand>, frag: &InlineFrag, geo: &FragGeometry, parts: TextParts) {
    let FragGeometry { container_x, container_width, line_h, frag_y, .. } = *geo;
    let text_color = frag.style.used_text_fill_color();
    let base_rect = Rect::new(container_x + frag.x, frag_y, container_width, line_h);
    if parts.shadows {
        emit_text_shadows(out, base_rect, line_h, frag);
    }
    if !parts.glyphs {
        return;
    }
    // CSS Text Decoration L3 §2: underline и overline лежат под глифами, line-through — над ними.
    push_text_decoration_pass(out, container_x, frag_y, frag, DecorationPass::BehindText);
    out.push(DisplayCommand::DrawText {
        font_stretch: frag.style.font_stretch,
        rect: base_rect,
        // UAX #9 L2/L4 — a right-to-left fragment is handed to the
        // rasterizers already reversed and mirrored: they advance strictly
        // left to right and do no bidi work of their own. `frag.text` stays
        // logical, so Selection/Range offsets are unaffected.
        text: lumen_layout::bidi::visual_text(&frag.text, frag.bidi_level).into_owned(),
        font_size: frag.style.font_size,
        color: text_color,
        font_family: frag.style.font_family.clone(),
        font_weight: frag.style.font_weight,
        font_style: frag.style.font_style,
        font_features: lumen_layout::style::text_font_features(&frag.style),
        font_palette: palette_selection(&frag.style),
        font_variation_axes: lumen_layout::style::text_variation_axes(&frag.style),
        tab_size: frag.style.tab_size,
        letter_spacing: super::spacing_to_fixed(frag.style.letter_spacing),
        word_spacing: super::spacing_to_fixed(frag.style.word_spacing + frag.style.word_spacing_pct * 0.25 * frag.style.font_size),
        highlight_name: None,
        text_orientation: if frag.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
            Some(frag.style.text_orientation)
        } else {
            None
        },
    });
    emit_text_stroke(out, base_rect, frag);
    push_text_decoration_pass(out, container_x, frag_y, frag, DecorationPass::AboveText);
    emit_text_emphasis_marks(out, container_x, frag_y, frag);
}

/// WHATWG Compat §3.2 — `-webkit-text-stroke`: обводка глифов поверх заливки
/// (штрих центрирован на контуре, поэтому половина ширины перекрывает заливку).
/// Вертикальные режимы письма не поддержаны — обводка не рисуется.
fn emit_text_stroke(out: &mut Vec<DisplayCommand>, base_rect: Rect, frag: &InlineFrag) {
    let style = &frag.style;
    let width = style.text_stroke_width;
    let color = style.used_text_stroke_color();
    if width <= 0.0
        || color.a == 0
        || style.font_size <= 0.0
        || style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb
    {
        return;
    }
    let half = width / 2.0;
    out.push(DisplayCommand::DrawTextStroke {
        rect: Rect::new(base_rect.x - half, base_rect.y - half, base_rect.width + width, base_rect.height + width),
        text: lumen_layout::bidi::visual_text(&frag.text, frag.bidi_level).into_owned(),
        font_size: style.font_size,
        font_family: style.font_family.clone(),
        font_weight: style.font_weight,
        font_style: style.font_style,
        font_features: lumen_layout::style::text_font_features(style),
        tab_size: style.tab_size,
        letter_spacing: super::spacing_to_fixed(style.letter_spacing),
        word_spacing: super::spacing_to_fixed(style.word_spacing + style.word_spacing_pct * 0.25 * style.font_size),
        width,
        color,
    });
}

/// CSS Writing Modes L4 §3–4 — paints an InlineRun's lines when the box is in
/// a vertical writing mode (`vertical-rl`/`vertical-lr`/`sideways-*`).
///
/// `wrap_inline_run_vertical` (`lumen-layout`) repurposes `InlineFrag.x` as the
/// cumulative offset along the inline axis (physical Y for vertical writing
/// modes, top→bottom) and `InlineFrag.width` as the frag's own extent along
/// that axis — the same two fields the horizontal path in [`emit_inline_run`]
/// reads as a physical X offset and physical width. Before this function
/// existed, `emit_inline_run` ran unconditionally and misread those fields as
/// horizontal geometry: every frag in a line landed at the same physical Y
/// (`line_y`, correct only for a horizontal row) with X spread out by what is
/// actually the vertical cursor, and each word's DrawText got the column's
/// *width* (`b.rect.width`, one column wide) as its rect height — silently
/// breaking every real vertical `<div>` (caught by `graphic_tests/145-writing-mode.html`,
/// Ph3 writing-mode vertical Срез 5; the layout side — `vertical.rs` — was
/// already correct, only this paint conversion was never ported to the axis
/// swap).
///
/// Each outer `lines` entry is one wrapped column along the block axis; the
/// run's rect spans all of them (`lay_out_vertical_inline_run`), one
/// `used_line_height` wide each. Column 0 is the first in block direction —
/// the rightmost for `vertical-rl`/`sideways-rl`, the leftmost for
/// `vertical-lr`/`sideways-lr` — and column N steps from it by `N * col_width`
/// (leftward for `rl`, rightward for `lr`).
///
/// Not yet ported to this axis (Phase 0, same class of gap the horizontal
/// path documents elsewhere): `vertical-align` (`frag.y_offset`), inline
/// replaced content (images), `::selection` highlight, and
/// `text-overflow: ellipsis`.
fn emit_inline_run_vertical(b: &LayoutBox, lines: &[Vec<InlineFrag>], out: &mut Vec<DisplayCommand>) {
    let col_width = b.used_line_height;
    let is_rtl = matches!(
        b.style.writing_mode,
        lumen_layout::style::WritingMode::VerticalRl | lumen_layout::style::WritingMode::SidewaysRl
    );
    for (line_idx, line) in lines.iter().enumerate() {
        // The run's rect spans all its columns; column 0 is the first in block
        // direction — the rightmost for `rl`, the leftmost for `lr`.
        let column_x = if is_rtl {
            b.rect.x + b.rect.width - (line_idx + 1) as f32 * col_width
        } else {
            b.rect.x + line_idx as f32 * col_width
        };
        for frag in line {
            if !matches!(frag.style.visibility, Visibility::Visible) || frag.img_src.is_some() {
                continue;
            }
            let rect = Rect::new(column_x, b.rect.y + frag.x, col_width, frag.width);
            emit_text_shadows(out, rect, rect.height, frag);
            out.push(DisplayCommand::DrawText {
                font_stretch: frag.style.font_stretch,
                rect,
                text: frag.text.clone(),
                font_size: frag.style.font_size,
                color: frag.style.used_text_fill_color(),
                font_family: frag.style.font_family.clone(),
                font_weight: frag.style.font_weight,
                font_style: frag.style.font_style,
                font_features: lumen_layout::style::text_font_features(&frag.style),
                font_palette: palette_selection(&frag.style),
                font_variation_axes: lumen_layout::style::text_variation_axes(&frag.style),
                tab_size: frag.style.tab_size,
                letter_spacing: 0,
                word_spacing: 0,
                highlight_name: None,
                text_orientation: Some(frag.style.text_orientation),
            });
            emit_vertical_emphasis_marks(out, column_x, col_width, rect.y, frag);
        }
    }
}

/// Renders all lines of a [`BoxKind::InlineRun`].
///
/// When `text-overflow: ellipsis` (CSS UI L4 §3) is active on the box style
/// AND a line's text extends past `b.rect.width`, the line is rendered with:
/// 1. A [`DisplayCommand::PushClipRect`] narrowed by the ellipsis glyph width.
/// 2. Normal text emission inside the clip.
/// 3. [`DisplayCommand::PopClip`].
/// 4. A [`DisplayCommand::DrawText`] "…" at the clip boundary.
///
/// Requires `overflow_x != visible` on the box (CSS UI L4 §3 precondition).
/// The parent block's overflow:hidden clip ensures no pixel escapes the container.
pub(crate) fn emit_inline_run(
    b: &LayoutBox,
    lines: &[Vec<InlineFrag>],
    dpr: f32,
    out: &mut Vec<DisplayCommand>,
) {
    if b.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
        emit_inline_run_vertical(b, lines, out);
        return;
    }
    // CSS Rhythmic Sizing L1 §2 — line-height-step rounds each line box up to a
    // multiple of the step so paint stacks lines at the same rhythm layout used.
    // FONTLOAD-14 (BUG-467): `b.used_line_height` already carries the real
    // font-metrics resolution for `line-height: normal` — paint has no
    // measurer of its own, so it must read layout's resolved value rather
    // than recompute the flat `font_size * line_height` approximation.
    let raw_line_h = b.used_line_height;
    let line_h = if b.style.line_height_step > 0.0 {
        (raw_line_h / b.style.line_height_step).ceil() * b.style.line_height_step
    } else {
        raw_line_h
    };
    let wants_ellipsis = matches!(b.style.text_overflow_right, TextOverflow::Ellipsis)
        && style_overflow_clips(&b.style, b.style.overflow_x);

    emit_first_line_background(b, lines, line_h, dpr, out);

    for (line_idx, line) in lines.iter().enumerate() {
        let line_y = b.rect.y + line_idx as f32 * line_h;

        // Phase 1: inline frag backgrounds (under text).
        for frag in line.iter() {
            if !matches!(frag.style.visibility, Visibility::Visible) {
                continue;
            }
            emit_inline_frag_box(out, b.rect.x, line_y + frag.y_offset, line_h, frag);
        }

        // Detect text-overflow: find first visible frag that extends past container.
        let overflow_frag = if wants_ellipsis {
            line.iter().find(|f| {
                matches!(f.style.visibility, Visibility::Visible)
                    && f.x + f.width > b.rect.width
            })
        } else {
            None
        };

        // Phase 2: text — with or without ellipsis clip.
        if let Some(ef) = overflow_frag {
            let ew = ef.style.font_size * ELLIPSIS_EM;
            let clip_w = (b.rect.width - ew).max(0.0);
            out.push(DisplayCommand::PushClipRect {
                rect: Rect::new(b.rect.x, line_y, clip_w, line_h),
            });
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, out);
            out.push(DisplayCommand::PopClip);
            out.push(DisplayCommand::DrawText {
                font_stretch: ef.style.font_stretch,
                rect: Rect::new(b.rect.x + clip_w, line_y, ew, line_h),
                text: "\u{2026}".to_string(),
                font_size: ef.style.font_size,
                color: ef.style.used_text_fill_color(),
                font_family: ef.style.font_family.clone(),
                font_weight: ef.style.font_weight,
                font_style: ef.style.font_style,
                font_features: lumen_layout::style::text_font_features(&ef.style),
                font_palette: palette_selection(&ef.style),
                font_variation_axes: lumen_layout::style::text_variation_axes(&ef.style),
                tab_size: 0.0,
                letter_spacing: 0,
                word_spacing: 0,
                highlight_name: None,
                text_orientation: if ef.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                    Some(ef.style.text_orientation)
                } else {
                    None
                },
            });
        } else {
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, out);
        }
    }
}

/// BUG-432 — background of the `::first-line` pseudo-element.
///
/// CSS Pseudo-elements L4 §4.1 lists all background properties among those that
/// apply to `::first-line`, and `split_first_line_boxes` already puts the whole
/// pseudo-element `ComputedStyle` on the box holding the first formatted line —
/// but `emit_inline_run` only ever drew text, so the background was dropped.
///
/// §4.1 also makes the pseudo-element behave as a fictional inline tag wrapping
/// the line's content, so the painted extent is the union of the line's
/// fragments, **not** the full width of the containing block (`b.rect.width`,
/// which is what the box itself carries). Box-model properties — margin,
/// padding, border — do not apply to `::first-line` and are not painted here.
///
/// Keyed on the box role rather than on the style: every other `InlineRun` is
/// built through `anon_style`, which clears `background_color`, so this is a
/// guard against a future anonymous run that inherits one, not a filter that
/// currently discriminates.
fn emit_first_line_background(
    b: &LayoutBox,
    lines: &[Vec<InlineFrag>],
    line_h: f32,
    dpr: f32,
    out: &mut Vec<DisplayCommand>,
) {
    if !matches!(b.origin.role, BoxRole::Pseudo(PseudoKind::FirstLine)) {
        return;
    }
    if !matches!(b.style.visibility, Visibility::Visible) {
        return;
    }
    let has_color = b.style.background_color.and_then(|c| c.to_color_opt()).is_some_and(|c| c.a > 0);
    if !has_color && b.style.background_layers.is_empty() {
        return;
    }
    // The pseudo-element covers the first formatted line only; the box produced
    // by the split holds it as `lines[0]`.
    let Some(line) = lines.first() else { return };
    let Some(rect) = first_line_content_rect(b, line, line_h) else { return };

    let radii = CornerRadii::from_style_and_box(&b.style, rect.width, rect.height);
    if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
        && bg.a > 0
    {
        if radii.all_zero() {
            out.push(DisplayCommand::FillRect { rect, color: bg });
        } else {
            out.push(DisplayCommand::FillRoundedRect { rect, color: bg, radii });
        }
    }
    if !b.style.background_layers.is_empty() {
        // `emit_background_image` derives every clip/origin box from `b.rect`,
        // which here spans the whole containing block — hand it a copy narrowed
        // to the line's own extent so a gradient covers the same area the solid
        // colour does. Cheap: the copy is one line of fragments, no children.
        let mut fl = b.clone();
        fl.rect = rect;
        emit_background_image(out, &fl, dpr);
    }
}

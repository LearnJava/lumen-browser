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

/// CSS Text Decoration L4 §5 — emits per-character emphasis marks above or
/// below each grapheme cluster of `frag.text`.
///
/// Phase 0: distributes marks uniformly over the fragment width (no per-glyph
/// advance measurement). Accurate spacing requires a measurer at paint time
/// (deferred to Phase 1).
fn emit_text_emphasis_marks(
    out: &mut Vec<DisplayCommand>,
    container_x: f32,
    line_h: f32,
    frag_y: f32,
    frag: &InlineFrag,
) {
    let mark = emphasis_mark_str(&frag.style.text_emphasis_style);
    if mark.is_empty() {
        return;
    }
    let char_count = frag.text.chars().count();
    if char_count == 0 {
        return;
    }
    let mark_size = frag.style.font_size * 0.5;
    let is_over = frag.style.text_emphasis_position.is_over();
    let mark_y = if is_over {
        frag_y - mark_size * 1.2
    } else {
        frag_y + line_h
    };
    let color = frag.style.text_emphasis_color.resolve(frag.style.color);
    let char_w = frag.width / char_count as f32;
    let frag_x = container_x + frag.x;
    for i in 0..char_count {
        out.push(DisplayCommand::DrawText {
            font_stretch: frag.style.font_stretch,
            rect: Rect::new(frag_x + i as f32 * char_w, mark_y, char_w, mark_size * 1.5),
            text: mark.to_string(),
            font_size: mark_size,
            color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        });
    }
}

/// Emits shadow + DrawText + decorations for every visible frag in `line`.
///
/// When `sel` is `Some`, fragments that overlap the active selection range
/// receive a `FillRect` highlight background before the text, and optionally
/// have their text colour overridden by `sel.fg_color` (CSS Pseudo-elements
/// L4 §5.6 `::selection`).
///
/// Phase 0 limitation: selection pixel bounds are estimated proportionally
/// by byte offset, which is accurate for ASCII but approximate for non-ASCII.
/// Per-glyph accuracy requires a `TextMeasurer` which is not available here.
fn emit_text_frags(
    line: &[InlineFrag],
    container_x: f32,
    container_width: f32,
    line_y: f32,
    line_h: f32,
    sel: Option<&SelectionHighlight>,
    out: &mut Vec<DisplayCommand>,
) {
    for frag in line {
        if !matches!(frag.style.visibility, Visibility::Visible) {
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

        // ::selection highlight — emit FillRect for selected portion before text.
        let sel_fg = sel.and_then(|s| {
            let hi = frag_selection_highlight(frag, s);
            if let Some((sel_x, sel_w)) = hi {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(container_x + sel_x, line_y, sel_w, line_h),
                    color: s.bg_color,
                });
            }
            if hi.is_some() { s.fg_color } else { None }
        });

        let text_color = sel_fg.unwrap_or(frag.style.color);
        let base_rect = Rect::new(container_x + frag.x, frag_y, container_width, line_h);
        emit_text_shadows(out, base_rect, line_h, frag);
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
            font_variation_axes: {
                let mut axes: Vec<([u8; 4], f32)> = frag.style.font_variation_settings
                    .iter().map(|a| (a.tag, a.value)).collect();
                if frag.style.font_optical_sizing == FontOpticalSizing::Auto
                    && !axes.iter().any(|(t, _)| t == b"opsz")
                {
                    axes.push((*b"opsz", frag.style.font_size));
                }
                if frag.style.font_stretch != FontStretch::NORMAL
                    && !axes.iter().any(|(t, _)| t == b"wdth")
                {
                    axes.push((*b"wdth", frag.style.font_stretch.0 as f32 / 10.0));
                }
                axes
            },
            tab_size: frag.style.tab_size,
            highlight_name: None,
            text_orientation: if frag.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(frag.style.text_orientation)
            } else {
                None
            },
        });
        push_text_decoration(out, container_x, frag_y, frag);
        emit_text_emphasis_marks(out, container_x, line_h, frag_y, frag);
    }
}

/// Compute the (frag-relative x, width) pixel span that is covered by the
/// active selection for a single inline fragment.
///
/// Returns `None` when the fragment is outside the selection range.
///
/// Uses byte-proportional estimation for sub-fragment boundaries.  Accurate
/// for ASCII text; approximate for variable-width or multi-byte characters.
fn frag_selection_highlight(frag: &InlineFrag, sel: &SelectionHighlight) -> Option<(f32, f32)> {
    let range = &sel.range;
    if range.is_collapsed() {
        return None;
    }
    let frag_end = frag.source_char_offset + frag.text.len() as u32;
    let same_start = range.start.container == frag.source_node;
    let same_end = range.end.container == frag.source_node;

    // byte offsets within the frag's text
    let (byte_start, byte_end): (u32, u32) = if same_start && same_end {
        let s = range.start.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        let e = range.end.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        if e <= s { return None; }
        (s, e)
    } else if same_start {
        let s = range.start.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        (s, frag.text.len() as u32)
    } else if same_end {
        let e = range.end.offset.max(frag.source_char_offset).min(frag_end)
            - frag.source_char_offset;
        if e == 0 { return None; }
        (0, e)
    } else {
        // Frag node is between range endpoints: fully selected, but multi-node
        // selection depth is not tracked in Phase 0 without tree traversal.
        return None;
    };

    let total = frag.text.len() as f32;
    if total <= 0.0 {
        return None;
    }
    let x_start = frag.x + frag.width * (byte_start as f32 / total);
    let x_end   = frag.x + frag.width * (byte_end   as f32 / total);
    Some((x_start, (x_end - x_start).max(0.0)))
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
/// Each outer `lines` entry is one wrapped column along the block axis;
/// column 0 sits at `b.rect.x` (already correctly placed by
/// `lay_out_vertical_inline_run`/the vertical block-stacking cursor in
/// `vertical.rs`), so wrapped column N shifts by `N * col_width` — leftward
/// for `vertical-rl`/`sideways-rl` (later columns sit further from the
/// right-edge start), rightward for `vertical-lr`/`sideways-lr`.
///
/// Not yet ported to this axis (Phase 0, same class of gap the horizontal
/// path documents elsewhere): `vertical-align` (`frag.y_offset`), inline
/// replaced content (images), `::selection` highlight, and
/// `text-overflow: ellipsis`.
fn emit_inline_run_vertical(b: &LayoutBox, lines: &[Vec<InlineFrag>], out: &mut Vec<DisplayCommand>) {
    let col_width = b.rect.width;
    let is_rtl = matches!(
        b.style.writing_mode,
        lumen_layout::style::WritingMode::VerticalRl | lumen_layout::style::WritingMode::SidewaysRl
    );
    for (line_idx, line) in lines.iter().enumerate() {
        let column_x = if is_rtl {
            b.rect.x - line_idx as f32 * col_width
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
                color: frag.style.color,
                font_family: frag.style.font_family.clone(),
                font_weight: frag.style.font_weight,
                font_style: frag.style.font_style,
                font_features: lumen_layout::style::text_font_features(&frag.style),
                font_palette: palette_selection(&frag.style),
                font_variation_axes: {
                    let mut axes: Vec<([u8; 4], f32)> = frag.style.font_variation_settings
                        .iter().map(|a| (a.tag, a.value)).collect();
                    if frag.style.font_optical_sizing == FontOpticalSizing::Auto
                        && !axes.iter().any(|(t, _)| t == b"opsz")
                    {
                        axes.push((*b"opsz", frag.style.font_size));
                    }
                    if frag.style.font_stretch != FontStretch::NORMAL
                        && !axes.iter().any(|(t, _)| t == b"wdth")
                    {
                        axes.push((*b"wdth", frag.style.font_stretch.0 as f32 / 10.0));
                    }
                    axes
                },
                tab_size: frag.style.tab_size,
                highlight_name: None,
                text_orientation: Some(frag.style.text_orientation),
            });
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
    sel: Option<&SelectionHighlight>,
    dpr: f32,
    out: &mut Vec<DisplayCommand>,
) {
    if b.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
        emit_inline_run_vertical(b, lines, out);
        return;
    }
    // CSS Rhythmic Sizing L1 §2 — line-height-step rounds each line box up to a
    // multiple of the step so paint stacks lines at the same rhythm layout used.
    let raw_line_h = b.style.font_size * b.style.line_height;
    let line_h = if b.style.line_height_step > 0.0 {
        (raw_line_h / b.style.line_height_step).ceil() * b.style.line_height_step
    } else {
        raw_line_h
    };
    let wants_ellipsis = matches!(b.style.text_overflow, TextOverflow::Ellipsis)
        && overflow_clips(b.style.overflow_x);

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
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, sel, out);
            out.push(DisplayCommand::PopClip);
            out.push(DisplayCommand::DrawText {
                font_stretch: ef.style.font_stretch,
                rect: Rect::new(b.rect.x + clip_w, line_y, ew, line_h),
                text: "\u{2026}".to_string(),
                font_size: ef.style.font_size,
                color: ef.style.color,
                font_family: ef.style.font_family.clone(),
                font_weight: ef.style.font_weight,
                font_style: ef.style.font_style,
                font_features: lumen_layout::style::text_font_features(&ef.style),
                font_palette: palette_selection(&ef.style),
                font_variation_axes: {
                    let mut axes: Vec<([u8; 4], f32)> = ef.style.font_variation_settings
                        .iter().map(|a| (a.tag, a.value)).collect();
                    if ef.style.font_optical_sizing == FontOpticalSizing::Auto
                        && !axes.iter().any(|(t, _)| t == b"opsz")
                    {
                        axes.push((*b"opsz", ef.style.font_size));
                    }
                    if ef.style.font_stretch != FontStretch::NORMAL
                        && !axes.iter().any(|(t, _)| t == b"wdth")
                    {
                        axes.push((*b"wdth", ef.style.font_stretch.0 as f32 / 10.0));
                    }
                    axes
                },
                tab_size: 0.0,
                highlight_name: None,
                text_orientation: if ef.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                    Some(ef.style.text_orientation)
                } else {
                    None
                },
            });
        } else {
            emit_text_frags(line, b.rect.x, b.rect.width, line_y, line_h, sel, out);
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

use super::*;

/// CSS Pseudo-elements L4 §5.1 — split the `PseudoKind::FirstLetter` segment in
/// `row_items` into `[first_grapheme | rest]` and apply `fl_style` to the first part.
///
/// The segment was already marked by `collect_inline_segments`; this function
/// overrides its style and (when the text is longer than one char) splits it so
/// `wrap_inline_run` applies the correct font metrics to each part independently.
pub(crate) fn apply_first_letter_style(
    row_items: &mut [LayoutBox],
    fl_style: ComputedStyle,
    inherited: &ComputedStyle,
) {
    for item in row_items.iter_mut() {
        let BoxKind::InlineRun { segments, .. } = &mut item.kind else {
            continue;
        };
        for i in 0..segments.len() {
            if segments[i].pseudo_kind != PseudoKind::FirstLetter {
                continue;
            }
            let text = segments[i].text.clone();
            // CSS Pseudo-elements L4 §5.1: leading punctuation + first letter.
            let boundary = first_letter_text_len(&text);
            if boundary < text.len() {
                // Multi-char segment: split into first-letter + rest.
                let rest_text = text[boundary..].to_string();
                let first_text = text[..boundary].to_string();
                let source_node = segments[i].source_node;
                let forced_break = segments[i].forced_break;
                let is_element_box = segments[i].is_element_box;
                let img_src = segments[i].img_src.clone();
                let img_width = segments[i].img_width;
                // The tail keeps the segment's own style — it may sit inside an
                // inline (`<em>Bravo</em>`) whose declarations outlive the split.
                let own_style = segments[i].style.clone();
                segments[i].text = first_text;
                segments[i].style =
                    crate::style::merge_pseudo_inherited(&own_style, inherited, &fl_style);
                let rest = InlineSegment {
                    text: rest_text,
                    style: own_style,
                    pre_space: 0.0,
                    post_space: segments[i].post_space,
                    is_element_box,
                    img_src,
                    img_is_lazy: false,
                    img_width,
                    forced_break,
                    pseudo_kind: PseudoKind::None,
                    source_node,
                    source_char_offset: segments[i].source_char_offset + boundary as u32,
                    bidi_level: 0,
                };
                // Transfer post_space from first-letter to rest.
                segments[i].post_space = 0.0;
                segments.insert(i + 1, rest);
            } else {
                // Single-char or empty segment: just layer the pseudo style on.
                segments[i].style = crate::style::merge_pseudo_inherited(
                    &segments[i].style, inherited, &fl_style,
                );
            }
            return;
        }
    }
}

/// CSS Pseudo-elements L4 §5.1 — byte length of the `::first-letter` text unit
/// at the start of `text`: leading whitespace (raw segment text keeps source
/// newlines/indent until wrap-time collapsing) plus leading punctuation plus
/// the first letter itself.
///
/// Phase 0 approximation: char-level (no grapheme clustering), leading
/// punctuation only (the spec also includes punctuation immediately following
/// the letter); `white-space: pre` significance of the swallowed leading
/// whitespace is ignored. Returns `text.len()` when no letter is found.
pub(crate) fn first_letter_text_len(text: &str) -> usize {
    for (i, c) in text.char_indices() {
        if c.is_whitespace() || is_first_letter_punctuation(c) {
            continue;
        }
        return i + c.len_utf8();
    }
    text.len()
}

/// True for punctuation that joins the `::first-letter` text unit
/// (CSS Pseudo-elements L4 §5.1: Unicode Ps/Pe/Pi/Pf/Po classes; approximated
/// as ASCII punctuation + common typographic quotes — no Unicode tables yet).
fn is_first_letter_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(c, '«' | '»' | '“' | '”' | '‘' | '’' | '„' | '‚' | '‹' | '›')
}

/// CSS Pseudo-elements L4 §5.2 — `::first-letter` layout split, float variant
/// (drop cap, BB-2).
///
/// When the `::first-letter` rule contains `float: left|right`, the first-letter
/// segment (already split out and styled by `apply_first_letter_pseudo`) is
/// removed from its `InlineRun` and promoted to a block-level float `LayoutBox`;
/// the parent block's float machinery then places it and narrows the remaining
/// text lines around it. Returns `None` when no `FirstLetter` segment exists.
///
/// Box structure: the outer `Block` carries the full ::first-letter style
/// (float, margins, padding, border, background); the inner anonymous
/// `InlineRun` holds the single letter segment and supplies the line metrics
/// (::first-letter `font-size` × `line-height`). An `InlineRun` emptied by the
/// extraction is dropped from `row_items`.
// CSS: ::first-letter — P4 wires further drop-cap properties on top of this
// split (initial-letter, initial-letter-align).
pub(crate) fn extract_first_letter_float(
    row_items: &mut Vec<LayoutBox>,
    fl_style: &ComputedStyle,
) -> Option<LayoutBox> {
    for ri in 0..row_items.len() {
        let BoxKind::InlineRun { segments, .. } = &mut row_items[ri].kind else {
            continue;
        };
        let Some(pos) = segments.iter().position(|s| s.pseudo_kind == PseudoKind::FirstLetter)
        else {
            continue;
        };
        let mut seg = segments.remove(pos);
        seg.pre_space = 0.0;
        seg.post_space = 0.0;
        // Strip leading source whitespace (raw newlines/indent from pretty-printed
        // HTML): it would inflate the drop cap's max-content shrink-to-fit width.
        let ws_len = seg.text.len() - seg.text.trim_start().len();
        if ws_len > 0 {
            seg.text.drain(..ws_len);
            seg.source_char_offset += ws_len as u32;
        }
        let node = seg.source_node;
        if segments.is_empty() {
            row_items.remove(ri);
        }
        // Inner anonymous run: ::first-letter font metrics for the line box,
        // but it must not itself float, clear, or indent inside the drop cap.
        let mut inner_style = anon_style(fl_style);
        inner_style.float_side = FloatSide::None;
        inner_style.clear = ClearSide::None;
        inner_style.text_indent = Length::Px(0.0);
        let inner = LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(inner_style),
            kind: BoxKind::InlineRun { segments: vec![seg], lines: vec![], first_line_style: None },
            children: vec![],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        };
        let mut outer_style = fl_style.clone();
        outer_style.display = Display::Block;
        outer_style.text_indent = Length::Px(0.0);
        return Some(LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(outer_style),
            kind: BoxKind::Block,
            children: vec![inner],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        });
    }
    None
}

/// CSS Inline Layout L3 §5 — `initial-letter` drop cap (Phase 0).
///
/// Promotes the block's first-letter segment (already marked
/// `PseudoKind::FirstLetter` by `collect_inline_segments`) to an inline-start
/// float `Block` whose glyph spans `size` lines and which reserves `sink` text
/// lines beside it. Reuses the float wrap machinery (the surrounding text lines
/// narrow around the float automatically).
///
/// Phase 0 approximations: inline-start = left (LTR only); the precise
/// cap-height/baseline alignment of the spec is approximated by
/// `font-size = size × parent line-height`, and the glyph box is clipped to the
/// reserved `sink`-line height. `letter_style` carries the optional
/// `::first-letter` author style (color/font); falls back to an anonymous style
/// derived from `base`.
///
/// `base` — the parent block style (supplies the reference `line-height` in px).
/// `size` — cap height in lines (> 1). `sink` — in-flow lines (`0` = `floor(size)`).
/// Returns `None` when no first-letter segment is present (e.g. the block opens
/// with an image or is empty).
pub(crate) fn extract_initial_letter(
    row_items: &mut Vec<LayoutBox>,
    base: &ComputedStyle,
    letter_style: Option<&ComputedStyle>,
    size: f32,
    sink: u32,
) -> Option<LayoutBox> {
    for ri in 0..row_items.len() {
        let BoxKind::InlineRun { segments, .. } = &mut row_items[ri].kind else {
            continue;
        };
        let Some(pos) = segments.iter().position(|s| s.pseudo_kind == PseudoKind::FirstLetter)
        else {
            continue;
        };
        // Split off the first-letter unit. With a `::first-letter` rule present,
        // `apply_first_letter_pseudo` has already isolated the letter (rest is a
        // sibling segment); without one (initial-letter set on the element), the
        // whole opening text segment is still marked FirstLetter and must be
        // split here.
        let boundary = first_letter_text_len(&segments[pos].text);
        let mut seg = segments[pos].clone();
        let rest_text = seg.text.split_off(boundary);
        // Strip leading source whitespace (pretty-print newlines/indent): it
        // would inflate the cap's shrink-to-fit width.
        let ws_len = seg.text.len() - seg.text.trim_start().len();
        if ws_len > 0 {
            seg.text.drain(..ws_len);
            seg.source_char_offset += ws_len as u32;
        }
        if seg.text.is_empty() {
            // No actual letter (all whitespace/punctuation) — leave content as-is.
            return None;
        }
        seg.pre_space = 0.0;
        seg.post_space = 0.0;
        let node = seg.source_node;
        // Put the remainder back into the run (or drop the now-empty run).
        if rest_text.is_empty() {
            segments.remove(pos);
            if segments.is_empty() {
                row_items.remove(ri);
            }
        } else {
            let rest = &mut segments[pos];
            rest.source_char_offset += boundary as u32;
            rest.text = rest_text;
            rest.pseudo_kind = PseudoKind::None;
            rest.pre_space = 0.0;
        }

        // Used line-height in px: the engine stores `line_height` as a multiplier
        // of `font_size` (relative) or px/font_size (absolute), so the product is
        // the px line box height in both cases (mirrors `font_size * line_height`
        // used throughout layout/paint).
        let ref_line = (base.font_size * base.line_height).max(1.0);
        let cap_font = (size * ref_line).max(1.0);
        let sink_lines = if sink == 0 { size.floor().max(1.0) as u32 } else { sink };
        let sink_px = sink_lines as f32 * ref_line;

        // Inner anonymous run: enlarged glyph metrics, never floats/indents itself.
        let mut inner_style = letter_style.cloned().unwrap_or_else(|| anon_style(base));
        inner_style.font_size = cap_font;
        // Tight line box equal to the cap font size (ratio 1.0): `line_height` is a
        // multiplier of `font_size`, so 1.0 → line box height == cap_font.
        inner_style.line_height = 1.0;
        inner_style.line_height_is_relative = true;
        inner_style.float_side = FloatSide::None;
        inner_style.clear = ClearSide::None;
        inner_style.text_indent = Length::Px(0.0);
        inner_style.initial_letter_size = 1.0;
        inner_style.initial_letter_sink = 0;
        seg.style = inner_style.clone();

        let inner = LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(inner_style),
            kind: BoxKind::InlineRun { segments: vec![seg], lines: vec![], first_line_style: None },
            children: vec![],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        };

        // Outer block: inline-start float reserving exactly `sink` text lines.
        let mut outer_style = letter_style.cloned().unwrap_or_else(|| anon_style(base));
        outer_style.display = Display::Block;
        outer_style.float_side = FloatSide::Left;
        outer_style.clear = ClearSide::None;
        outer_style.text_indent = Length::Px(0.0);
        outer_style.initial_letter_size = 1.0;
        outer_style.initial_letter_sink = 0;
        outer_style.height = Some(Length::Px(sink_px));
        outer_style.overflow_x = crate::style::Overflow::Hidden;
        outer_style.overflow_y = crate::style::Overflow::Hidden;
        return Some(LayoutBox {
            node,
            rect: Rect::ZERO,
            style: Arc::new(outer_style),
            kind: BoxKind::Block,
            children: vec![inner],
            col_span: 1,
            row_span: 1, svg_group_transform: None, scroll_x: 0.0, scroll_y: 0.0, dirty: Default::default(),
            origin: BoxOrigin { node: Some(node), role: BoxRole::Pseudo(PseudoKind::FirstLetter) },
        });
    }
    None
}

/// True for the synthesized drop-cap box produced by
/// [`extract_first_letter_float`]: a float `Block` whose only child is an
/// `InlineRun` with a single `PseudoKind::FirstLetter` segment. Used to keep
/// `::first-line` overrides off the drop cap (CSS Pseudo-elements L4 §5.2:
/// ::first-letter wins where the two pseudo-elements conflict).
pub(crate) fn is_first_letter_box(b: &LayoutBox) -> bool {
    b.style.float_side != FloatSide::None
        && b.children.len() == 1
        && matches!(
            &b.children[0].kind,
            BoxKind::InlineRun { segments, .. }
                if segments.len() == 1 && segments[0].pseudo_kind == PseudoKind::FirstLetter
        )
}

/// CSS Pseudo-elements L4 §3.1 — apply `::first-line` style overrides after layout.
///
/// Must be called after `lay_out` has populated `InlineRun.lines` with `InlineFrag`s.
/// Walks the box tree; for each block-level box that has a `::first-line` rule on
/// its DOM node, overrides the style of every frag on the first formatted line
/// (`is_first_line == true`).
///
/// BUG-341 S23: the walk is skipped outright when the sheet has no
/// `::first-line` rule. It probed every block box in the document — 123 probes
/// per interaction cycle on `chrome.html`, none of which could ever hit,
/// because that sheet has no such rule. The predicate is over the same `sheet`
/// this function would consult, so skipping is exactly behaviour-preserving.
pub(crate) fn apply_first_line_pseudo_styles(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) {
    if !crate::style::sheet_targets_pseudo(sheet, viewport, dark_mode, "first-line") {
        return;
    }
    apply_first_line_pseudo_styles_inner(b, doc, sheet, viewport, dark_mode);
}

/// The recursive body of [`apply_first_line_pseudo_styles`], split out so the
/// sheet-level predicate is evaluated once per pass instead of once per box.
fn apply_first_line_pseudo_styles_inner(
    b: &mut LayoutBox,
    doc: &Document,
    sheet: &Stylesheet,
    viewport: Size,
    dark_mode: bool,
) {
    // CSS Pseudo-elements L4 §5.2 (BB-2): never apply ::first-line inside the
    // synthesized drop-cap box — ::first-letter wins where the two conflict.
    if is_first_letter_box(b) {
        return;
    }
    for child in &mut b.children {
        apply_first_line_pseudo_styles_inner(child, doc, sheet, viewport, dark_mode);
    }
    if !matches!(b.kind, BoxKind::Block | BoxKind::FlowRoot) {
        return;
    }
    let Some(fl_style) = compute_pseudo_element_style(doc, b.node, "first-line", sheet, &b.style, viewport, dark_mode) else {
        return;
    };
    // Find the first InlineRun child (or inside InlineBlockRow) and apply.
    // §3.4: layer the pseudo style over what each frag inherited from `b` —
    // an inner `<b>`/`<em>`/`style="…"` keeps its own declarations.
    let base = b.style.clone();
    let restyle = |lines: &mut Vec<Vec<InlineFrag>>| {
        if let Some(first_line) = lines.first_mut() {
            for frag in first_line.iter_mut() {
                if frag.is_first_line {
                    frag.style =
                        crate::style::merge_pseudo_inherited(&frag.style, &base, &fl_style);
                }
            }
        }
    };
    let mut applied = false;
    'find: for child in &mut b.children {
        match &mut child.kind {
            BoxKind::InlineRun { lines, .. } => {
                restyle(lines);
                applied = true;
                break 'find;
            }
            BoxKind::InlineBlockRow => {
                for row_child in &mut child.children {
                    if let BoxKind::InlineRun { lines, .. } = &mut row_child.kind {
                        restyle(lines);
                        applied = true;
                        break 'find;
                    }
                }
            }
            _ => {}
        }
    }
    let _ = applied;
}

/// Byte offsets of each whitespace-separated word start in `text`
/// (same word boundaries as `str::split_whitespace`).
fn word_start_offsets(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut in_word = false;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            in_word = false;
        } else if !in_word {
            starts.push(i);
            in_word = true;
        }
    }
    starts
}

/// CSS Pseudo-elements L4 §3.1 — partition `segments` into
/// `(consumed by the first formatted line, remainder)`.
///
/// `line0` is the first line produced by the ::first-line wrap pass; its frags
/// appear in segment order and never span segments, so consumption is counted
/// word-by-word with the same boundaries as `str::split_whitespace` (matching
/// `wrap_inline_run`). A partially consumed segment is split at the word
/// boundary: the head keeps the segment's `pre_space` (its inline box opened on
/// line 0, `post_space` → 0), the tail keeps `post_space` (`pre_space` → 0,
/// `source_char_offset` advanced by the cut byte offset).
///
/// `preserves_ws` (white-space: pre / pre-wrap): each non-empty segment before
/// the first forced break produced exactly one frag — whole segments up to and
/// including that break are consumed.
pub(crate) fn split_segments_at_first_line(
    segments: &[InlineSegment],
    line0: &[InlineFrag],
    preserves_ws: bool,
) -> (Vec<InlineSegment>, Vec<InlineSegment>) {
    let mut consumed: Vec<InlineSegment> = Vec::new();
    let mut idx = 0usize;

    if preserves_ws {
        for _ in line0 {
            // Empty non-break text segments produce no frag — consume silently.
            while idx < segments.len()
                && segments[idx].text.is_empty()
                && !segments[idx].forced_break
                && segments[idx].img_src.is_none()
            {
                consumed.push(segments[idx].clone());
                idx += 1;
            }
            if idx < segments.len() {
                consumed.push(segments[idx].clone());
                idx += 1;
            }
        }
        // The forced break that terminated line 0 belongs to it.
        if idx < segments.len() && segments[idx].forced_break {
            consumed.push(segments[idx].clone());
            idx += 1;
        }
        return (consumed, segments[idx..].to_vec());
    }

    // Word-level consumption for collapsing white-space modes.
    let mut words_taken = 0usize; // words already consumed from segments[idx]
    for frag in line0 {
        if frag.img_src.is_some() {
            // Advance to the img segment, consuming exhausted text segments.
            while idx < segments.len() && segments[idx].img_src.is_none() {
                consumed.push(segments[idx].clone());
                idx += 1;
                words_taken = 0;
            }
            if idx < segments.len() {
                consumed.push(segments[idx].clone());
                idx += 1;
            }
            continue;
        }
        let mut need = frag.text.split_whitespace().count();
        while need > 0 && idx < segments.len() {
            let seg = &segments[idx];
            if seg.img_src.is_some() || seg.forced_break {
                consumed.push(seg.clone());
                idx += 1;
                words_taken = 0;
                continue;
            }
            let total = seg.text.split_whitespace().count();
            let avail = total.saturating_sub(words_taken);
            if avail <= need {
                need -= avail;
                consumed.push(seg.clone());
                idx += 1;
                words_taken = 0;
            } else {
                words_taken += need;
                need = 0;
            }
        }
    }

    let mut rest: Vec<InlineSegment> = Vec::new();
    if words_taken > 0 && idx < segments.len() {
        // Partially consumed segment: split at the word boundary.
        let seg = &segments[idx];
        let starts = word_start_offsets(&seg.text);
        if words_taken < starts.len() {
            let cut = starts[words_taken];
            let mut head = seg.clone();
            head.text = seg.text[..cut].trim_end().to_string();
            head.post_space = 0.0;
            consumed.push(head);
            let mut tail = seg.clone();
            tail.text = seg.text[cut..].to_string();
            tail.pre_space = 0.0;
            tail.source_char_offset = seg.source_char_offset + cut as u32;
            rest.push(tail);
        } else {
            consumed.push(seg.clone());
        }
        idx += 1;
    }
    rest.extend(segments[idx..].iter().cloned());
    (consumed, rest)
}

/// CSS Pseudo-elements L4 §3.1 — ::first-line layout split (BB-1).
///
/// Post-layout pass: walks the box tree and, for every `InlineRun` carrying a
/// `first_line_style`, splits the first formatted line into its own `InlineRun`
/// box styled with the ::first-line style; the remainder keeps the base style.
/// Paint computes line height as `style.font_size * style.line_height` per box,
/// so the split gives the first line its correct (possibly larger) line box
/// height with no paint-side changes. Single-line runs are restyled in place.
/// Idempotent: `first_line_style` is cleared on every produced box.
/// The box receives the full `::first-line` `ComputedStyle` and the
/// `BoxRole::Pseudo(PseudoKind::FirstLine)` role, so background,
/// text-decoration, color and font all take effect at paint time — the role is
/// what lets `emit_inline_run` tell this box from an anonymous inline run,
/// whose `anon_style` has no background of its own (BUG-432).
pub(crate) fn split_first_line_boxes(b: &mut LayoutBox) {
    for child in &mut b.children {
        split_first_line_boxes(child);
    }
    let mut i = 0;
    while i < b.children.len() {
        let child = &mut b.children[i];
        let BoxKind::InlineRun { segments, lines, first_line_style } = &mut child.kind else {
            i += 1;
            continue;
        };
        let Some(fls) = first_line_style.take() else {
            i += 1;
            continue;
        };
        if lines.len() < 2 {
            // The whole run is the first formatted line: restyle the box in place
            // so paint uses the ::first-line font metrics for its single line box.
            child.style = Arc::new(*fls);
            child.origin.role = BoxRole::Pseudo(PseudoKind::FirstLine);
            i += 1;
            continue;
        }
        let preserves = child.style.white_space.preserves_whitespace();
        let (consumed_segs, rest_segs) =
            split_segments_at_first_line(segments, &lines[0], preserves);
        let line0 = lines[0].clone();
        let rest_lines: Vec<Vec<InlineFrag>> = lines[1..].to_vec();
        let fl_h = fls.font_size * fls.line_height;
        let base_h = child.style.font_size * child.style.line_height;
        let rect = child.rect;
        let box2 = LayoutBox {
            node: child.node,
            rect: Rect::new(rect.x, rect.y + fl_h, rect.width, rest_lines.len() as f32 * base_h),
            style: child.style.clone(),
            kind: BoxKind::InlineRun {
                segments: rest_segs,
                lines: rest_lines,
                first_line_style: None,
            },
            children: Vec::new(),
            col_span: 1,
            row_span: 1,
            svg_group_transform: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            dirty: Default::default(),
            // Fragment of the same InlineRun, split after ::first-line — same
            // provenance as the box it was split from, not a new box role.
            origin: child.origin,
        };
        // Reuse the original box as the first-line box.
        child.style = Arc::new(*fls);
        child.rect.height = fl_h;
        child.kind = BoxKind::InlineRun {
            segments: consumed_segs,
            lines: vec![line0],
            first_line_style: None,
        };
        // BUG-432: tag the box so paint can tell it from an ordinary anonymous
        // inline run and draw the pseudo-element's own background. Every other
        // `InlineRun` is built through `anon_style`, which clears
        // `background_color`; this one carries the full ::first-line style.
        child.origin.role = BoxRole::Pseudo(PseudoKind::FirstLine);
        b.children.insert(i + 1, box2);
        i += 2;
    }
}

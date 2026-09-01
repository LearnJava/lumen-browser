use super::*;

const ACCENT_DEFAULT: Color = Color { r: 21, g: 90, b: 192, a: 255 };

/// FRAME-7 remainder 2: fallback text-selection highlight colour for a
/// typeable field, used since a form control's value is not part of the DOM
/// text `::selection` targets — this engine has no per-field way to author a
/// different one. Same `#308aff` the OS-accent fallback
/// [`lumen_layout::SelectionHighlight::bg_color`]'s doc comment recommends,
/// translucent so the value text drawn on top stays legible.
const SELECTION_HIGHLIGHT_DEFAULT: Color = Color { r: 0x30, g: 0x8a, b: 0xff, a: 110 };

/// Render checkbox checkmark or radio dot for checked form controls.
/// P2 note: this renders a simple filled rectangle as indicator; a full
/// vector checkmark / circle belongs to the renderer GPU primitive set.
/// HTML rendering §15.5 — default UA label for a button-type `<input>` that has
/// no `value` attribute. `submit`/`reset` have UA labels; a bare `button` has
/// none and renders empty.
fn default_button_label(input_type: &InputType) -> String {
    match input_type {
        InputType::Submit => "Submit".to_owned(),
        InputType::Reset => "Reset".to_owned(),
        _ => String::new(),
    }
}

/// HTML rendering §15.5.5 — paint the static `value` text of a form control
/// inside its content box.
///
/// `center` horizontally centers the text (button-like controls); otherwise the
/// text is left-aligned with a small inset (text fields). Password fields
/// (`input_type == Password`) mask each character with U+2022 BULLET. The text
/// is vertically centered within the content box and clipped to it so long
/// values do not overflow the border. The content box is the border box minus
/// the border widths; a fixed 2px inset approximates the native control padding.
fn emit_input_value_text(
    b: &LayoutBox,
    value: &str,
    input_type: &InputType,
    center: bool,
    out: &mut Vec<DisplayCommand>,
) {
    if value.is_empty() {
        return;
    }
    let s = &b.style;
    // Password masking: obscure each character (grapheme-approximate by char).
    let text = if *input_type == InputType::Password {
        "\u{2022}".repeat(value.chars().count())
    } else {
        value.to_owned()
    };

    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = b.rect.x + bl + inset;
    let content_y = b.rect.y + bt;
    let content_w = (b.rect.width - bl - br - inset * 2.0).max(1.0);
    let content_h = (b.rect.height - bt - bb).max(1.0);
    let font_size = s.font_size;

    // Horizontal placement. `draw_text` has no alignment, so a centered label
    // is positioned with the same per-glyph advance approximation used for SVG
    // text anchoring (a real TextMeasurer is not available in this crate).
    let text_x = if center {
        let approx_w = font_size * 0.5 * text.chars().count() as f32;
        content_x + ((content_w - approx_w) / 2.0).max(0.0)
    } else {
        content_x
    };
    // Vertical centering: `draw_text` places the glyph top at `y`, so offset by
    // half the leftover vertical space inside the content box.
    let text_y = content_y + ((content_h - font_size) / 2.0).max(0.0);

    // Clip to the content box so overflowing text stays inside the border.
    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(content_x, content_y, content_w, content_h),
    });
    out.push(DisplayCommand::DrawText {
        font_stretch: s.font_stretch,
        rect: Rect::new(text_x, text_y, content_w, font_size),
        text,
        font_size,
        color: s.color,
        font_family: s.font_family.clone(),
        font_weight: s.font_weight,
        font_style: s.font_style,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
            Some(s.text_orientation)
        } else {
            None
        },
    });
    out.push(DisplayCommand::PopClip);
}

/// FRAME-7: paint the text-entry cursor bar inside a focused `<input>`
/// (HTML LS §4.10.5.1) at the tracked char-index position `char_index`.
///
/// Geometry mirrors [`emit_input_value_text`] (same content box, same 2px
/// inset) so the bar lines up with the text it sits inside. Horizontal
/// placement uses the same per-glyph advance approximation as that
/// function's centered-label path — this crate has no real `TextMeasurer` —
/// so the bar can drift from the true glyph edge on proportional fonts;
/// acceptable for a 1px caret, not for text layout. Password masking needs
/// no special case: the masked and plain text have the same char count, so
/// the advance is identical either way. Clipped to the content box like the
/// text, so a cursor past the field's (unscrolled) visible width is hidden
/// rather than drawn outside the border — this engine has no horizontal
/// scroll for overflowing input text at all yet.
fn emit_input_caret(b: &LayoutBox, value: &str, char_index: usize, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = b.rect.x + bl + inset;
    let content_y = b.rect.y + bt;
    let content_w = (b.rect.width - bl - br - inset * 2.0).max(1.0);
    let content_h = (b.rect.height - bt - bb).max(1.0);
    let font_size = s.font_size;

    let chars_before = char_index.min(value.chars().count());
    let caret_x = content_x + font_size * 0.5 * chars_before as f32;
    // CSS UI L4 §6.3 `caret-color: auto` (`None`) follows the text color —
    // already true for `forced-colors`/`color-scheme` overrides applied
    // earlier in the cascade (see `style/adjust.rs`), so this is the same
    // resolution rule applied one more time at paint.
    let color = s.caret_color.unwrap_or(s.color);

    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(content_x, content_y, content_w, content_h),
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(caret_x, content_y, 1.0, content_h),
        color,
    });
    out.push(DisplayCommand::PopClip);
}

/// FRAME-7 remainder 2: paint the active text-selection highlight inside a
/// focused `<input>` as a background rect behind char range `[start, end)`
/// (HTML LS §4.10.5.1, CSS Pseudo-Elements L4 §5.6 — the closest analogue,
/// though a form control's value is not a `::selection` target). Same
/// geometry approximation as [`emit_input_caret`] (per-glyph advance, no real
/// `TextMeasurer`) and same content-box clip. Drawn by the caller BEFORE
/// [`emit_input_value_text`] so the value's glyphs paint on top of the
/// highlight rather than under it.
fn emit_input_selection(b: &LayoutBox, value: &str, start: usize, end: usize, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = b.rect.x + bl + inset;
    let content_y = b.rect.y + bt;
    let content_w = (b.rect.width - bl - br - inset * 2.0).max(1.0);
    let content_h = (b.rect.height - bt - bb).max(1.0);
    let font_size = s.font_size;

    let len = value.chars().count();
    let start = start.min(len);
    let end = end.min(len);
    if start >= end {
        return;
    }
    let sel_x = content_x + font_size * 0.5 * start as f32;
    let sel_w = font_size * 0.5 * (end - start) as f32;

    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(content_x, content_y, content_w, content_h),
    });
    out.push(DisplayCommand::FillRect {
        rect: Rect::new(sel_x, content_y, sel_w, content_h),
        color: SELECTION_HIGHLIGHT_DEFAULT,
    });
    out.push(DisplayCommand::PopClip);
}

/// Paint an empty text input's `placeholder` attribute as a grey hint
/// (HTML rendering §15.5.5). Left-aligned, vertically centered and clipped to
/// the content box, mirroring `emit_input_value_text` but with a fixed grey
/// colour (`#757575`, the UA default) and no password masking.
///
/// `placeholder_style` is the computed `input::placeholder` override (CSS
/// Pseudo-Elements L4 §4.10), when an author rule matched. Only `color`,
/// `opacity` (folded into the drawn color's alpha) and `font-*` are honoured —
/// the same restricted-subset approach as `::selection`.
fn emit_input_placeholder_text(
    b: &LayoutBox,
    placeholder: &str,
    placeholder_style: Option<&lumen_layout::style::ComputedStyle>,
    out: &mut Vec<DisplayCommand>,
) {
    if placeholder.is_empty() {
        return;
    }
    let s = &b.style;
    let bl = s.border_left_width;
    let bt = s.border_top_width;
    let br = s.border_right_width;
    let bb = s.border_bottom_width;
    let inset = 2.0_f32;
    let content_x = b.rect.x + bl + inset;
    let content_y = b.rect.y + bt;
    let content_w = (b.rect.width - bl - br - inset * 2.0).max(1.0);
    let content_h = (b.rect.height - bt - bb).max(1.0);
    let font_size = placeholder_style.map_or(s.font_size, |ps| ps.font_size);
    let text_y = content_y + ((content_h - font_size) / 2.0).max(0.0);

    let default_color = Color { r: 0x75, g: 0x75, b: 0x75, a: 255 };
    let color = match placeholder_style {
        Some(ps) => Color { a: (ps.color.a as f32 * ps.opacity).round() as u8, ..ps.color },
        None => default_color,
    };
    let (font_family, font_weight, font_style) = match placeholder_style {
        Some(ps) => (ps.font_family.clone(), ps.font_weight, ps.font_style),
        None => (s.font_family.clone(), s.font_weight, s.font_style),
    };

    out.push(DisplayCommand::PushClipRect {
        rect: Rect::new(content_x, content_y, content_w, content_h),
    });
    out.push(DisplayCommand::DrawText {
        font_stretch: s.font_stretch,
        rect: Rect::new(content_x, text_y, content_w, font_size),
        text: placeholder.to_owned(),
        font_size,
        color,
        font_family,
        font_weight,
        font_style,
        font_variation_axes: vec![],
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        highlight_name: None,
        text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
            Some(s.text_orientation)
        } else {
            None
        },
    });
    out.push(DisplayCommand::PopClip);
}

/// Build the white checkmark glyph for a checked checkbox as a triangle soup
/// (for [`DisplayCommand::DrawSvgPath`]). The tick is a two-segment thick
/// polyline (short stroke down to a vertex, long stroke up to the top-right),
/// positioned and scaled inside `fill` (the accent-filled control box).
fn checkmark_triangles(fill: Rect) -> Vec<[f32; 2]> {
    let sz = fill.width.min(fill.height);
    // Normalised tick anchor points (origin top-left, y downwards).
    let pt = |nx: f32, ny: f32| [fill.x + nx * fill.width, fill.y + ny * fill.height];
    let p0 = pt(0.22, 0.52);
    let p1 = pt(0.42, 0.72);
    let p2 = pt(0.78, 0.30);
    let half = (sz * 0.09).max(1.0);

    let mut v = Vec::with_capacity(12);
    push_thick_segment(&mut v, p0, p1, half);
    push_thick_segment(&mut v, p1, p2, half);
    v
}

/// Append the two triangles of a thick line segment from `a` to `b` with
/// half-width `half` to `out` (6 vertices). Used to draw the checkmark strokes.
pub(crate) fn push_thick_segment(out: &mut Vec<[f32; 2]>, a: [f32; 2], b: [f32; 2], half: f32) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
    // Perpendicular unit vector scaled by half-width.
    let nx = -dy / len * half;
    let ny = dx / len * half;
    let a1 = [a[0] + nx, a[1] + ny];
    let a2 = [a[0] - nx, a[1] - ny];
    let b1 = [b[0] + nx, b[1] + ny];
    let b2 = [b[0] - nx, b[1] - ny];
    out.extend_from_slice(&[a1, a2, b1, a2, b2, b1]);
}

pub(crate) fn emit_form_control_indicator(
    b: &LayoutBox,
    kind: &FormControlKind,
    ov: Option<&CompositorOverride>,
    out: &mut Vec<DisplayCommand>,
) {
    // CSS Basic UI L4 §4.2 — `appearance: none` (and the legacy `-webkit-`/
    // `-moz-` aliases, normalised to `Appearance::None` at parse time) removes
    // the native "primitive appearance" of a form control: the checkbox tick,
    // radio dot, range slider, progress bar, meter bar and select arrow. The box
    // (border/padding/background) is already stripped in
    // `strip_ua_appearance_box_styling` (before the author cascade); here we
    // suppress the painted indicator so authors can fully restyle it.
    // BUG-225: the suppression is scoped to the native primitives only (color
    // swatch, checkbox tick, radio dot, range slider, progress/meter bar, select
    // arrow). Text-input `value`/`placeholder` and button labels are author
    // content, not a UA primitive, so they keep rendering under `appearance:none`.
    let suppress_primitive = b.style.appearance == Appearance::None;
    // CSS UI L4 §6.1 — accent-color tints the "accent" of checkbox, radio,
    // range and progress controls. `auto` (None) keeps the UA default blue.
    // <meter> is intentionally excluded: its bar keeps the semantic
    // green/yellow/red coloring from HTML §4.10.14, not the accent color.
    let accent = b.style.accent_color.unwrap_or(ACCENT_DEFAULT);
    match kind {
        FormControlKind::Input { input_type, checked, value_text, placeholder, placeholder_style } => {
            // HTML §4.10.5.1.15 — a color input renders its value as a swatch
            // filling the content area, independent of any author `background`
            // (the native color widget ignores author bg). Default value is
            // `#000000`. Drawn before the `checked` gate since color is not
            // a checkable type.
            if *input_type == InputType::Color {
                // The swatch is the native primitive — suppressed under
                // `appearance:none` (the control has no text value to fall back
                // to, so nothing else is painted here).
                if !suppress_primitive {
                    let swatch = lumen_layout::style::parse_color(value_text)
                        .unwrap_or(Color { r: 0, g: 0, b: 0, a: 255 });
                    let bl = b.style.border_left_width;
                    let bt = b.style.border_top_width;
                    let br = b.style.border_right_width;
                    let bb = b.style.border_bottom_width;
                    let pad = 2.0;
                    out.push(DisplayCommand::FillRect {
                        rect: Rect::new(
                            b.rect.x + bl + pad,
                            b.rect.y + bt + pad,
                            (b.rect.width  - bl - br - pad * 2.0).max(1.0),
                            (b.rect.height - bt - bb - pad * 2.0).max(1.0),
                        ),
                        color: swatch,
                    });
                }
                return;
            }
            // HTML rendering §15.5.5 — text-like inputs paint their `value` as
            // static content (left-aligned, vertically centered, clipped to the
            // content box); button-like inputs (submit/reset/button) paint the
            // `value` as a centered label. Checkable types (checkbox/radio) fall
            // through to the dot/tick indicator below; `range`/`color`/`file`/
            // `hidden`/`image` never render a text value here.
            match input_type {
                InputType::Text | InputType::Email | InputType::Password
                | InputType::Tel | InputType::Url | InputType::Number
                | InputType::Search | InputType::Date | InputType::DateTimeLocal
                | InputType::Time | InputType::Month | InputType::Week => {
                    // FRAME-7 remainder 2: the selection highlight paints
                    // BEHIND the value text (same "background rect before
                    // glyphs" order `frag_selection_highlight` uses for
                    // ordinary DOM text), so it must run before either the
                    // value or placeholder path below.
                    let selection = ov.and_then(|o| o.selection);
                    if let Some((start, end)) = selection {
                        emit_input_selection(b, value_text, start, end, out);
                    }
                    if value_text.is_empty() && !placeholder.is_empty() {
                        // HTML rendering §15.5.5 — an empty text input paints its
                        // `placeholder` as a grey hint (never masked, even for
                        // password). Drawn left-aligned, vertically centered and
                        // clipped to the content box, like the value text.
                        emit_input_placeholder_text(b, placeholder, placeholder_style.as_deref(), out);
                    } else {
                        emit_input_value_text(b, value_text, input_type, false, out);
                    }
                    // FRAME-7: the shell only sets `ov.caret` for the currently
                    // focused typeable field, so no further gating by input_type
                    // is needed here — a Date/Time/etc. input (which paints
                    // through this same arm but has no cursor tracking) never
                    // gets a caret override in the first place. FRAME-7
                    // remainder 2: an active selection suppresses the caret bar
                    // — the OS-wide convention every text editor follows.
                    if selection.is_none()
                        && let Some(idx) = ov.and_then(|o| o.caret)
                    {
                        emit_input_caret(b, value_text, idx, out);
                    }
                    return;
                }
                InputType::Submit | InputType::Reset | InputType::Button => {
                    let label = if value_text.is_empty() {
                        default_button_label(input_type)
                    } else {
                        value_text.clone()
                    };
                    emit_input_value_text(b, &label, input_type, true, out);
                    return;
                }
                _ => {}
            }
            // The checked checkbox tick / radio dot is a native primitive —
            // suppressed under `appearance:none`.
            if suppress_primitive { return; }
            if !checked { return; }
            if *input_type != InputType::Checkbox && *input_type != InputType::Radio {
                return;
            }
            // Native checked checkbox/radio (Chromium/Edge default appearance):
            // the whole control fills with accent colour — overriding any author
            // `background` — and a white glyph is drawn on top: a tick for the
            // checkbox, a centre dot for the radio.
            let bl = b.style.border_left_width;
            let bt = b.style.border_top_width;
            let br = b.style.border_right_width;
            let bb = b.style.border_bottom_width;
            let fill = Rect::new(
                b.rect.x + bl,
                b.rect.y + bt,
                (b.rect.width  - bl - br).max(1.0),
                (b.rect.height - bt - bb).max(1.0),
            );
            let white = Color { r: 255, g: 255, b: 255, a: 255 };
            match input_type {
                InputType::Radio => {
                    // Solid accent disc filling the control, then a small white
                    // centre dot (radius ≈ 0.22 of the box) — the native look.
                    let r = fill.width.min(fill.height) / 2.0;
                    out.push(DisplayCommand::FillRoundedRect {
                        rect: fill,
                        radii: crate::CornerRadii { tl: r, tr: r, br: r, bl: r, ..Default::default() },
                        color: accent,
                    });
                    let dot_d = (fill.width.min(fill.height) * 0.44).max(2.0);
                    let dot = Rect::new(
                        fill.x + (fill.width  - dot_d) / 2.0,
                        fill.y + (fill.height - dot_d) / 2.0,
                        dot_d,
                        dot_d,
                    );
                    let dr = dot_d / 2.0;
                    out.push(DisplayCommand::FillRoundedRect {
                        rect: dot,
                        radii: crate::CornerRadii { tl: dr, tr: dr, br: dr, bl: dr, ..Default::default() },
                        color: white,
                    });
                }
                _ => {
                    out.push(DisplayCommand::FillRect { rect: fill, color: accent });
                    out.push(DisplayCommand::DrawSvgPath {
                        vertices: checkmark_triangles(fill),
                        color: white,
                    });
                }
            }
        }
        FormControlKind::Select { selected_text } => {
            // The select arrow is the native primitive; the selected option text
            // is author-visible content and keeps rendering. `emit_select_indicator`
            // draws both, so pass the suppression flag down rather than gating here.
            emit_select_indicator(b, selected_text, suppress_primitive, out);
        }
        FormControlKind::Button | FormControlKind::Textarea { .. } => {}
        FormControlKind::Range { value, min, max } => {
            if !suppress_primitive {
                emit_range_slider(b, *value, *min, *max, accent, out);
            }
        }
        FormControlKind::Progress { value, max } => {
            if !suppress_primitive {
                emit_progress_bar(b, *value, *max, accent, out);
            }
        }
        FormControlKind::Meter { value, min, max, low, high, optimum } => {
            if !suppress_primitive {
                emit_meter_bar(b, *value, *min, *max, *low, *high, *optimum, out);
            }
        }
    }
}

/// Draw a range slider: gray track, accent-colored filled portion, circular thumb.
///
/// `accent` is the resolved `accent-color` (UA default blue when `auto`); it
/// tints both the filled track portion and the thumb per CSS UI L4 §6.1.
fn emit_range_slider(b: &LayoutBox, value: f32, min: f32, max: f32, accent: Color, out: &mut Vec<DisplayCommand>) {
    let range = (max - min).max(f32::EPSILON);
    let fraction = ((value - min) / range).clamp(0.0, 1.0);

    let track_h = 4.0_f32;
    let thumb_r = 8.0_f32; // thumb diameter
    let track_y = b.rect.y + (b.rect.height - track_h) / 2.0;
    let track_x = b.rect.x + thumb_r / 2.0;
    let track_w = (b.rect.width - thumb_r).max(1.0);

    let gray = Color { r: 200, g: 200, b: 200, a: 255 };
    let blue = accent;
    let track_radius = crate::CornerRadii { tl: 2.0, tr: 2.0, br: 2.0, bl: 2.0, ..Default::default() };

    // Gray background track.
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(track_x, track_y, track_w, track_h),
        radii: track_radius,
        color: gray,
    });

    // Blue filled portion (left of thumb).
    let fill_w = (track_w * fraction).max(0.0);
    if fill_w > 0.0 {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(track_x, track_y, fill_w, track_h),
            radii: track_radius,
            color: blue,
        });
    }

    // Circular thumb.
    let thumb_cx = track_x + track_w * fraction;
    let thumb_y = b.rect.y + (b.rect.height - thumb_r) / 2.0;
    let hr = thumb_r / 2.0;
    let thumb_radii = crate::CornerRadii { tl: hr, tr: hr, br: hr, bl: hr, ..Default::default() };
    out.push(DisplayCommand::FillRoundedRect {
        rect: Rect::new(thumb_cx - thumb_r / 2.0, thumb_y, thumb_r, thumb_r),
        radii: thumb_radii,
        color: blue,
    });
}

/// Draw a `<progress>` bar inside the border box.
///
/// Determinate: `accent`-colored fill proportional to `value / max`.
/// Indeterminate (`value` is `None`): static 30% fill to indicate pending state.
/// `accent` is the resolved `accent-color` (UA default blue when `auto`).
fn emit_progress_bar(b: &LayoutBox, value: Option<f32>, max: f32, accent: Color, out: &mut Vec<DisplayCommand>) {
    let pad = 2.0_f32;
    let bar_x = b.rect.x + pad;
    let bar_y = b.rect.y + pad;
    let bar_max_w = (b.rect.width - pad * 2.0).max(0.0);
    let bar_h = (b.rect.height - pad * 2.0).max(1.0);
    let blue = accent;
    let radii = crate::CornerRadii { tl: 2.0, tr: 2.0, br: 2.0, bl: 2.0, ..Default::default() };

    let fraction = match value {
        None => 0.3,
        Some(v) => (v / max.max(f32::EPSILON)).clamp(0.0, 1.0),
    };

    let fill_w = (bar_max_w * fraction).max(0.0);
    if fill_w > 0.0 {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(bar_x, bar_y, fill_w, bar_h),
            radii,
            color: blue,
        });
    }
}

/// Draw a `<meter>` gauge bar inside the border box (HTML5 §4.10.14).
///
/// Fill color: green = optimal zone, yellow = sub-optimal, red = bad.
#[allow(clippy::too_many_arguments)]
fn emit_meter_bar(
    b: &LayoutBox,
    value: f32,
    min: f32,
    max: f32,
    low: f32,
    high: f32,
    optimum: f32,
    out: &mut Vec<DisplayCommand>,
) {
    let range = (max - min).max(f32::EPSILON);
    let fraction = ((value - min) / range).clamp(0.0, 1.0);

    let pad = 2.0_f32;
    let bar_x = b.rect.x + pad;
    let bar_y = b.rect.y + pad;
    let bar_max_w = (b.rect.width - pad * 2.0).max(0.0);
    let bar_h = (b.rect.height - pad * 2.0).max(1.0);
    let radii = crate::CornerRadii { tl: 2.0, tr: 2.0, br: 2.0, bl: 2.0, ..Default::default() };

    let fill_color = meter_gauge_color(value, min, max, low, high, optimum);
    let fill_w = (bar_max_w * fraction).max(0.0);
    if fill_w > 0.0 {
        out.push(DisplayCommand::FillRoundedRect {
            rect: Rect::new(bar_x, bar_y, fill_w, bar_h),
            radii,
            color: fill_color,
        });
    }
}

/// HTML5 §4.10.14 — determine meter gauge fill color from value and thresholds.
///
/// Optimum zone → green, adjacent zone → yellow, far zone → red.
pub(crate) fn meter_gauge_color(value: f32, _min: f32, _max: f32, low: f32, high: f32, optimum: f32) -> Color {
    let green  = Color { r: 100, g: 180, b:  60, a: 255 };
    let yellow = Color { r: 210, g: 175, b:  20, a: 255 };
    let red    = Color { r: 200, g:  60, b:  60, a: 255 };

    // Where does optimum fall?
    let opt_in_low    = optimum <= low;
    let opt_in_high   = optimum >= high;
    let opt_in_middle = !opt_in_low && !opt_in_high;

    let val_in_low    = value < low;
    let val_in_high   = value > high;
    let val_in_middle = !val_in_low && !val_in_high;

    if opt_in_middle {
        if val_in_middle { green } else { yellow }
    } else if opt_in_low {
        if val_in_low { green } else if val_in_middle { yellow } else { red }
    } else {
        // opt_in_high
        if val_in_high { green } else if val_in_middle { yellow } else { red }
    }
}

/// Draw the selected option label and a dropdown arrow (▼) inside a `<select>` box.
///
/// `suppress_primitive` (set by `appearance: none`, BUG-225) drops the native
/// separator line and dropdown arrow; the selected option label is author-visible
/// content and is always painted.
fn emit_select_indicator(b: &LayoutBox, selected_text: &str, suppress_primitive: bool, out: &mut Vec<DisplayCommand>) {
    let s = &b.style;
    let fg = s.color;
    let font_size = s.font_size.clamp(10.0, 14.0);
    let pad = 4.0;
    // Arrow column width (enough for "▼" glyph). When the native arrow is
    // suppressed the label reclaims that column.
    let arrow_w = font_size + pad * 2.0;
    let reserved = if suppress_primitive { 0.0 } else { arrow_w };
    let text_w = (b.rect.width - reserved - pad * 2.0).max(1.0);

    // Selected label — clipped to available width.
    if !selected_text.is_empty() {
        out.push(DisplayCommand::DrawText {
            font_stretch: s.font_stretch,
            rect: Rect::new(b.rect.x + pad, b.rect.y + pad, text_w, b.rect.height - pad * 2.0),
            text: selected_text.to_owned(),
            font_size,
            color: fg,
            font_family: s.font_family.clone(),
            font_weight: s.font_weight,
            font_style: s.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(s.text_orientation)
            } else {
                None
            },
        });
    }

    // Native separator line + dropdown arrow — suppressed under `appearance:none`.
    if !suppress_primitive {
        // Separator line before the arrow.
        let sep_x = b.rect.x + b.rect.width - arrow_w;
        out.push(DisplayCommand::DrawBorder {
            rect: Rect::new(sep_x, b.rect.y, 1.0, b.rect.height),
            widths: [0.0, 0.0, 0.0, 1.0],
            colors: [fg; 4],
            styles: [lumen_layout::BorderStyle::Solid; 4],
            radii: crate::CornerRadii::default(),
        });

        // Dropdown arrow "▼".
        out.push(DisplayCommand::DrawText {
            font_stretch: s.font_stretch,
            rect: Rect::new(sep_x + pad, b.rect.y + pad, arrow_w - pad, b.rect.height - pad * 2.0),
            text: "\u{25BC}".to_owned(),
            font_size: font_size * 0.75,
            color: fg,
            font_family: s.font_family.clone(),
            font_weight: s.font_weight,
            font_style: s.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(s.text_orientation)
            } else {
                None
            },
        });
    }
}

/// CSS Lists L3 §2.1 — renders the `::marker` pseudo-element.
/// Bullet types (disc/circle/square) are drawn as geometric shapes to avoid
/// relying on specific Unicode glyphs in the bundled font.
/// Counter types (decimal/roman/alpha/greek) are rendered as text.
pub(crate) fn emit_list_marker(b: &LayoutBox, out: &mut Vec<DisplayCommand>) {
    let BoxKind::Marker { ref text, ref list_style_type, ref image, .. } = b.kind else { return };
    if !is_paint_visible(b) {
        return;
    }
    let s = &b.style;
    // CSS Lists L3 §2.3 — `list-style-image` takes precedence over the marker
    // type/text: the bullet is replaced by the image. Drawn `contain`-fitted
    // inside the marker box; if the URL is not registered the DrawImage is a no-op.
    if let Some(src) = image
        && !src.is_empty()
    {
        out.push(DisplayCommand::DrawImage {
            rect: b.rect,
            src: src.clone(),
            alt: String::new(),
            object_fit: ObjectFit::Contain,
            object_position: ObjectPosition::default(),
            image_rendering: s.image_rendering,
        });
        return;
    }
    let color = s.color;
    let em = s.font_size;
    let cx = b.rect.x + b.rect.width * 0.5;
    let cy = b.rect.y + b.rect.height * 0.5;
    // CSS Lists L3 §2.1 / Pseudo-elements L4 §14.2 — a non-empty `text` means the
    // marker carries a string: either a counter glyph (decimal/roman/alpha) or an
    // explicit `::marker { content: … }` override. In both cases the string wins over
    // the bullet glyph, so the disc/circle/square shapes only draw when `text` is empty
    // (otherwise a `list-style-type: disc` list with `::marker { content: "→ " }` would
    // paint the disc instead of the arrow — BUG-185).
    match list_style_type {
        ListStyleType::Disc if text.is_empty() => {
            // Filled circle ~0.4em in diameter, centered in marker rect.
            let d = em * 0.40;
            let r = d * 0.5;
            let rect = Rect::new(cx - r, cy - r, d, d);
            let radii = CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r };
            out.push(DisplayCommand::FillRoundedRect { rect, color, radii });
        }
        ListStyleType::Circle if text.is_empty() => {
            // Hollow circle ~0.4em in diameter, border ~0.08em thick.
            let d = em * 0.40;
            let r = d * 0.5;
            let bw = (em * 0.08).max(1.0);
            let rect = Rect::new(cx - r, cy - r, d, d);
            let radii = CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r };
            out.push(DisplayCommand::DrawBorder {
                rect,
                widths: [bw; 4],
                colors: [color; 4],
                styles: [BorderStyle::Solid; 4],
                radii,
            });
        }
        ListStyleType::Square if text.is_empty() => {
            // Filled square ~0.35em side, centered in marker rect.
            let d = em * 0.35;
            let rect = Rect::new(cx - d * 0.5, cy - d * 0.5, d, d);
            out.push(DisplayCommand::FillRect { rect, color });
        }
        _ => {
            // Counter types (decimal, roman, alpha, greek) and `::marker { content }`
            // overrides — render the string.
            if !text.is_empty() {
                out.push(DisplayCommand::DrawText {
                    font_stretch: s.font_stretch,
                    rect: b.rect,
                    text: text.clone(),
                    font_size: em,
                    color,
                    font_family: s.font_family.clone(),
                    font_weight: s.font_weight,
                    font_style: s.font_style,
                    font_features: lumen_layout::style::text_font_features(s),
                    font_palette: palette_selection(s),
                    font_variation_axes: {
                        let mut axes: Vec<([u8; 4], f32)> = s.font_variation_settings
                            .iter().map(|a| (a.tag, a.value)).collect();
                        if s.font_optical_sizing == FontOpticalSizing::Auto
                            && !axes.iter().any(|(t, _)| t == b"opsz")
                        {
                            axes.push((*b"opsz", em));
                        }
                        if s.font_stretch != FontStretch::NORMAL
                            && !axes.iter().any(|(t, _)| t == b"wdth")
                        {
                            axes.push((*b"wdth", s.font_stretch.0 as f32 / 10.0));
                        }
                        axes
                     },
                     tab_size: 0.0,
                     highlight_name: None,
                     text_orientation: if s.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                         Some(s.text_orientation)
                     } else {
                         None
                     },
                 });
             }
         }
     }
}

/// CSS Tables L2 §17.6.1.1 — true when `b` is a table cell that must suppress its
/// borders and background under `empty-cells: hide`. Applies only in the separated-
/// borders model (`border-collapse: separate`) and only when the cell has no in-flow
/// content. Under `border-collapse: collapse` the property has no effect.
pub(crate) fn is_hidden_empty_cell(b: &LayoutBox) -> bool {
    b.style.display == Display::TableCell
        && b.style.empty_cells == EmptyCells::Hide
        && b.style.border_collapse == BorderCollapse::Separate
        && !table_cell_has_content(b)
}

/// True when a table cell has in-flow content: any descendant box that generates
/// text, a replaced element, or a block. Whitespace-only inline runs and `Skip`
/// boxes do not count (CSS Tables L2 §17.6.1.1 "empty" definition).
fn table_cell_has_content(b: &LayoutBox) -> bool {
    b.children.iter().any(box_generates_content)
}

/// Whether a single child box contributes in-flow content for the empty-cell test.
fn box_generates_content(c: &LayoutBox) -> bool {
    match &c.kind {
        BoxKind::Skip => false,
        BoxKind::InlineRun { lines, .. } => lines
            .iter()
            .any(|line| line.iter().any(|f| f.img_src.is_some() || !f.text.trim().is_empty())),
        _ => true,
    }
}

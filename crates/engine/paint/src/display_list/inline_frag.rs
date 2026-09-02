//! P1/SPLIT-DL13: inline-frag/shadows/bg-геометрия — `fn emit_inline_frag_box`
//! … до конца `fn parse_image_set_option` (до `fn select_image_set_url`,
//! которая остаётся в `display_list.rs`). Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-13).

use super::*;

/// Если у box-а видимый `outline` — эмитит `DrawOutline`. Caller гарантирует
/// правильный порядок (outline рисуется ПОВЕРХ контента box-а и его детей,
/// но в **рамках своей stacking phase** — Phase 0 без точного разделения
/// фаз outline эмитится сразу после background/border bounding-box-а у
/// `emit_box_self` и после children в `walk`, чтобы потомки не закрывали
/// его пиксели в случае negative `outline-offset`).
///
/// Per CSS Basic UI L4 §5.4: `OutlineColor::Auto` / `CurrentColor`
/// резолвятся в `style.color` (Phase 0 без UA contrast-цвета).
/// Эмитит per-fragment text-shadow DrawText-команды ПЕРЕД основным
/// DrawText. Несколько теней в списке: spec CSS Text Decoration L3 §6
/// — «the first shadow is on top, subsequent shadows are layered
/// behind it», что в painter's order означает обратный обход
/// (последний рисуется первым, первый — последним за основным
/// текстом). Phase 0 — без `blur`: тень = тот же текст со смещением
/// Рисует фон и рамку inline-элемента для одного `InlineFrag`.
///
/// `container_x` — левый край InlineRun-бокса.
/// `frag.x` — смещение текста от container_x (уже учитывает padding_left + border_left).
/// Фон рисуется от border-box левого края до border-box правого края.
pub(crate) fn emit_inline_frag_box(
    out: &mut Vec<DisplayCommand>,
    container_x: f32,
    line_y: f32,
    line_h: f32,
    frag: &InlineFrag,
) {
    if !frag.is_element_box {
        return;
    }
    let s = &frag.style;
    let bl = s.border_left_width;
    let br = s.border_right_width;
    let bt = s.border_top_width;
    let bb = s.border_bottom_width;

    // Border-box left edge = text_x - padding_left - border_left.
    // Snap to integer CSS pixels for consistent rendering with block-level boxes (BUG-084 partial).
    let box_x = (container_x + frag.x - frag.padding_left - bl).round();
    // Border-box width = border_left + padding_left + text + padding_right + border_right.
    let box_w = (bl + frag.padding_left + frag.width + frag.padding_right + br).round();
    let box_h = line_h.round();
    let box_y = line_y.round();

    let radii = CornerRadii::from_style_and_box(s, box_w, box_h);

    // Background (CSS Backgrounds L3: painted over padding+border area).
    if let Some(CssColor::Rgba(bg)) = s.background_color
        && bg.a > 0
        && box_w > 0.0
    {
        let r = Rect::new(box_x, box_y, box_w, box_h);
        if radii.all_zero() {
            out.push(DisplayCommand::FillRect { rect: r, color: bg });
        } else {
            out.push(DisplayCommand::FillRoundedRect { rect: r, color: bg, radii });
        }
    }

    // Border.
    let has_border = s.border_top_style.is_visible()
        || s.border_right_style.is_visible()
        || s.border_bottom_style.is_visible()
        || s.border_left_style.is_visible();
    if has_border && box_w > 0.0 {
        let cur = s.color;
        out.push(DisplayCommand::DrawBorder {
            rect: Rect::new(box_x, box_y, box_w, box_h),
            widths: [bt, br, bb, bl],
            colors: [
                s.border_top_color.resolve(cur),
                s.border_right_color.resolve(cur),
                s.border_bottom_color.resolve(cur),
                s.border_left_color.resolve(cur),
            ],
            styles: [
                s.border_top_style,
                s.border_right_style,
                s.border_bottom_style,
                s.border_left_style,
            ],
            radii,
        });
    }
}

/// (offset_x, offset_y) и shadow.color (None → currentColor =
/// frag.style.color).
/// Эмитит per-fragment text-shadow DrawText-команды ПЕРЕД основным DrawText.
///
/// * Несколько теней: spec CSS Text Decoration L3 §6 — «the first shadow is
///   on top» — обратный обход (последняя в CSS-списке рисуется первой).
/// * `blur > 0`: DrawText заворачивается в `PushFilter { Blur(sigma) }` /
///   `PopFilter`. Renderer применяет двухпроходный Gaussian GPU-шейдер.
///   sigma = blur / 2.0 (то же соглашение, что box-shadow: CSS Text
///   Decoration L3 §6 — blur-radius = стандартное отклонение × 2).
/// * `blur == 0`: DrawText напрямую, без off-screen pass.
pub(crate) fn emit_text_shadows(
    out: &mut Vec<DisplayCommand>,
    base_rect: Rect,
    line_h: f32,
    frag: &InlineFrag,
) {
    if frag.style.text_shadow.is_empty() {
        return;
    }
    for shadow in frag.style.text_shadow.iter().rev() {
        let color = shadow.color.unwrap_or(frag.style.color);
        let sigma = shadow.blur / 2.0;
        let text_shadow_rect = Rect::new(
            base_rect.x + shadow.offset_x,
            base_rect.y + shadow.offset_y,
            base_rect.width,
            line_h,
        );
        if sigma > 0.0 {
            out.push(DisplayCommand::PushFilter {
                filters: vec![FilterFn::Blur(sigma)],
                bounds: Some(text_shadow_rect),
            });
        }
        out.push(DisplayCommand::DrawText {
            font_stretch: frag.style.font_stretch,
            rect: text_shadow_rect,
            text: frag.text.clone(),
            font_size: frag.style.font_size,
            color,
            font_family: frag.style.font_family.clone(),
            font_weight: frag.style.font_weight,
            font_style: frag.style.font_style,
            // CSS Fonts L4 §7.12: for `auto`, inject opsz = font_size so the renderer
            // normalizes it via fvar like any other axis. Skipped for `none` to let
            // font-variation-settings control opsz directly.
            font_features: lumen_layout::style::text_font_features(&frag.style),
            font_palette: palette_selection(&frag.style),
            font_variation_axes: {
                let mut axes: Vec<([u8; 4], f32)> = frag.style.font_variation_settings
                    .iter().map(|s| (s.tag, s.value)).collect();
                if frag.style.font_optical_sizing == FontOpticalSizing::Auto {
                    let has_opsz = axes.iter().any(|(tag, _)| tag == b"opsz");
                    if !has_opsz {
                        axes.push((*b"opsz", frag.style.font_size));
                    }
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
        if sigma > 0.0 {
            out.push(DisplayCommand::PopFilter);
        }
    }
}

/// CSS Backgrounds L3 §3.8 — `background-clip` clip rect для фона.
/// Phase 0 (без border-radius — углы прямоугольные):
/// * `BorderBox` (initial): `b.rect` без изменений.
/// * `PaddingBox`: shrink на border-widths по всем сторонам.
/// * `ContentBox`: shrink на border + padding.
/// * `Text` (L4): Phase 0 fallback на `BorderBox` (реальный glyph-mask
///   clip требует off-screen alpha-pass, P2 п.4+).
///
/// `max(0.0)` страхует от negative-w/h на очень узких box-ах.
/// Возвращает painting area для background с учётом `clip` значения.
///
/// CSS Backgrounds L3 §3.8: border-box = b.rect; padding-box = rect без border-а;
/// content-box = rect без border-а и padding-а. Text трактуется как border-box (Phase 0).
pub(crate) fn background_clip_rect(b: &LayoutBox, clip: BackgroundClip) -> Rect {
    let s = &b.style;
    match clip {
        BackgroundClip::BorderBox | BackgroundClip::Text => b.rect,
        BackgroundClip::PaddingBox => Rect::new(
            b.rect.x + s.border_left_width,
            b.rect.y + s.border_top_width,
            (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
            (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
        ),
        BackgroundClip::ContentBox => content_box_rect(b),
    }
}

/// Content box of `b` — `b.rect` (the border box) shrunk by borders and padding.
///
/// CSS Box L3 §1: the content box is where a replaced element's own bitmap is
/// painted, so this is the destination rect for `<canvas>` (BUG-099) as well as
/// the `content-box` arm of [`background_clip_rect`].
pub(crate) fn content_box_rect(b: &LayoutBox) -> Rect {
    let s = &b.style;
    Rect::new(
        b.rect.x + s.border_left_width + s.padding_left.px(),
        b.rect.y + s.border_top_width + s.padding_top.px(),
        (b.rect.width
            - s.border_left_width
            - s.border_right_width
            - s.padding_left.px()
            - s.padding_right.px())
        .max(0.0),
        (b.rect.height
            - s.border_top_width
            - s.border_bottom_width
            - s.padding_top.px()
            - s.padding_bottom.px())
        .max(0.0),
    )
}

/// CSS Backgrounds L3 §3.10: clip для `background-color` — last layer's clip (или default).
pub(crate) fn background_color_clip(b: &LayoutBox) -> BackgroundClip {
    b.style.background_layers.last().map_or(BackgroundClip::BorderBox, |l| l.clip)
}

/// CSS Masking L1 §4.6 — the `mask-clip` painting area for a masked element.
///
/// Returns `Some(rect)` for the boxes that shrink the painting area below the
/// border box (`padding-box`, `content-box`, and `fill-box` — the latter maps
/// to the content box for CSS boxes, CSS Box 4 §1); the caller wraps the mask
/// group in a `PushClipRect` / `PopClip` pair around this rect.
///
/// Returns `None` for the values whose painting area equals the element's
/// border-box `b.rect` (`border-box`, plus `stroke-box`/`view-box` which fall
/// back to the border box for CSS boxes) and for `no-clip` (painting is not
/// clipped) — the clip would be a no-op scissor, so unmasked-default rendering
/// stays byte-identical.
///
/// Covers every layer [`rendered_mask_layers`] actually emits, not just the top
/// one: each layer's `mask-clip` bounds that layer's own contribution, and the
/// emitted layers combine by `intersect` (alpha multiplication), so restricting
/// each factor to its own rect is the same as restricting the product to the
/// **intersection** of those rects. A single rect therefore expresses the whole
/// chain exactly. Layers whose clip is a no-op (`border-box` and friends) drop
/// out of the intersection, so the common single-layer case is unchanged.
pub(crate) fn mask_clip_paint_rect(b: &LayoutBox) -> Option<Rect> {
    rendered_mask_layers(b)
        .iter()
        .filter_map(|l| mask_clip_layer_rect(b, l.clip))
        .reduce(intersect_rects)
}

/// `mask-clip` of a single layer → the rect it restricts painting to, or `None`
/// when that value's painting area is the element's border box (`border-box`,
/// plus `stroke-box`/`view-box` which fall back to it for CSS boxes) or when
/// painting is not clipped at all (`no-clip`). A `None` here means the clip
/// would be a no-op scissor, so unmasked-default rendering stays byte-identical.
fn mask_clip_layer_rect(b: &LayoutBox, clip: MaskClip) -> Option<Rect> {
    match clip {
        MaskClip::PaddingBox => Some(background_clip_rect(b, BackgroundClip::PaddingBox)),
        // fill-box has no SVG geometry on a CSS box → object bounding box = content box.
        MaskClip::ContentBox | MaskClip::FillBox => {
            Some(background_clip_rect(b, BackgroundClip::ContentBox))
        }
        // border-box / stroke-box / view-box all reduce to the border box for a
        // CSS box (= `b.rect`); no-clip disables the clip. All → no-op.
        MaskClip::BorderBox | MaskClip::StrokeBox | MaskClip::ViewBox | MaskClip::NoClip => None,
    }
}

/// Пересечение двух прямоугольников. Непересекающиеся дают прямоугольник
/// нулевого размера (не отрицательного): scissor нулевой площади означает
/// «ничего не рисуется» — верный результат для пустого пересечения.
fn intersect_rects(a: Rect, c: Rect) -> Rect {
    let x = a.x.max(c.x);
    let y = a.y.max(c.y);
    let right = (a.x + a.width).min(c.x + c.width);
    let bottom = (a.y + a.height).min(c.y + c.height);
    Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// Converts `background-origin` to the equivalent `BackgroundClip` for rect computation.
///
/// CSS Backgrounds L3 §3.5: background-origin has the same box keywords as background-clip
/// except it never has `text` (text-clip only). The conversion is 1:1 for the three box values.
fn origin_to_clip(o: BackgroundOrigin) -> BackgroundClip {
    match o {
        BackgroundOrigin::BorderBox  => BackgroundClip::BorderBox,
        BackgroundOrigin::PaddingBox => BackgroundClip::PaddingBox,
        BackgroundOrigin::ContentBox => BackgroundClip::ContentBox,
    }
}

/// Computes the background positioning area from `background-origin` (CSS Backgrounds L3 §3.5).
///
/// This rect is used for `background-size` (cover/contain/%) and `background-position` (% offsets).
/// Distinct from the painting/clip area computed by [`background_clip_rect`].
pub(crate) fn background_origin_rect(b: &LayoutBox, origin: BackgroundOrigin) -> Rect {
    background_clip_rect(b, origin_to_clip(origin))
}

/// ASCII case-insensitive `starts_with`.
fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// CSS Images L4 §5 — is `value` an `image-set()` / `-webkit-image-set()` expression?
///
/// Used by [`emit_background_layer`] to decide whether to run resolution
/// selection via [`select_image_set_url`] before emitting a `DrawBackgroundImage`.
#[must_use]
pub fn is_image_set(value: &str) -> bool {
    let v = value.trim_start();
    starts_with_ci(v, "image-set(") || starts_with_ci(v, "-webkit-image-set(")
}

/// Strips an outer `image-set( … )` / `-webkit-image-set( … )` wrapper,
/// returning the comma-separated option list. `None` if `s` is not wrapped.
pub(crate) fn strip_image_set_wrapper(s: &str) -> Option<&str> {
    if !s.ends_with(')') {
        return None;
    }
    for prefix in ["image-set(", "-webkit-image-set("] {
        if starts_with_ci(s, prefix) {
            return Some(&s[prefix.len()..s.len() - 1]);
        }
    }
    None
}

/// Splits `s` on top-level commas — commas inside `(…)` or quotes are ignored.
/// Each returned slice is a subslice of `s` (no allocation of contents). Needed
/// because `url(data:…,…)` and function values may contain literal commas.
pub(crate) fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut in_quote: Option<u8> = None;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match in_quote {
            Some(q) => {
                if c == q {
                    in_quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' => in_quote = Some(c),
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' if depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// Strips matching surrounding single/double quotes from `s` (if present).
fn strip_quotes(s: &str) -> &str {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Parses a CSS `<resolution>` token (first whitespace-separated token of
/// `rest`) into device-pixel-ratio units (dppx). Supports `x` / `dppx`
/// (1× = 1 dppx), `dpi` (÷96), `dpcm` (×2.54/96). `None` if not a resolution.
fn parse_resolution(rest: &str) -> Option<f32> {
    let tok = rest.split_whitespace().next()?;
    let lower = tok.to_ascii_lowercase();
    let (num_str, factor) = if let Some(n) = lower.strip_suffix("dppx") {
        (n, 1.0)
    } else if let Some(n) = lower.strip_suffix("dpcm") {
        (n, 2.54 / 96.0)
    } else if let Some(n) = lower.strip_suffix("dpi") {
        (n, 1.0 / 96.0)
    } else {
        let n = lower.strip_suffix('x')?;
        (n, 1.0)
    };
    let v: f32 = num_str.trim().parse().ok()?;
    Some(v * factor)
}

/// Parses one `image-set()` option `<url-or-string> [<resolution>]` into a
/// `(url, resolution_dppx)` pair. URL is returned with the `url(…)` wrapper
/// and any surrounding quotes stripped (a subslice of `opt`). Missing
/// resolution defaults to `1.0` (1×).
pub(crate) fn parse_image_set_option(opt: &str) -> (&str, f32) {
    let opt = opt.trim();
    let bytes = opt.as_bytes();
    let (url, rest): (&str, &str) = if starts_with_ci(opt, "url(") {
        if let Some(close) = opt.find(')') {
            (strip_quotes(opt[4..close].trim()), opt[close + 1..].trim_start())
        } else {
            (strip_quotes(opt[4..].trim()), "")
        }
    } else if bytes.first() == Some(&b'"') || bytes.first() == Some(&b'\'') {
        let q = bytes[0] as char;
        if let Some(rel) = opt[1..].find(q) {
            (&opt[1..1 + rel], opt[1 + rel + 1..].trim_start())
        } else {
            (&opt[1..], "")
        }
    } else {
        match opt.find(char::is_whitespace) {
            Some(sp) => (&opt[..sp], opt[sp..].trim_start()),
            None => (opt, ""),
        }
    };
    (url, parse_resolution(rest).unwrap_or(1.0))
}


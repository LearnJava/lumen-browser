//! P1/SPLIT-DL15: print-конвейер — `fn build_print_display_list` … до конца
//! `fn clip_path_to_rect` (конец региона перед `fn clip_path_to_shape`,
//! которая остаётся в `display_list.rs`). Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-15).

use super::*;

/// Builds a print display list from paginated layout.
///
/// Each page's fragments are translated to page-relative coordinates using
/// `PushTransform` / `PopTransform`. Pages are separated by `PageBreak` markers.
/// Use `split_at_page_breaks` to get per-page command slices for rendering.
///
/// If a page has `page_box` set, margin-box text fragments (@page headers, footers,
/// page numbers) are emitted as `DrawText` commands positioned at absolute page
/// coordinates (not inside the content-area transform).
///
/// Coordinate convention: page origin = (0, 0) at top-left of content area.
/// Fragment y-offset is relative to the content area, not the page box.
/// Margin-box positions are relative to the page box origin (top-left of full page).
pub fn build_print_display_list(pages: &[Page]) -> DisplayList {
    let mut cmds: DisplayList = Vec::new();
    for (page_idx, page) in pages.iter().enumerate() {
        if page_idx > 0 {
            cmds.push(DisplayCommand::PageBreak);
        }
        for frag in &page.fragments {
            // Translate from document-flow y to page-local y.
            let dy = frag.page_y_offset - frag.layout_box.rect.y;
            let matrix = Mat4::translation_2d(0.0, dy);
            cmds.push(DisplayCommand::PushTransform { matrix });
            walk(&frag.layout_box, &mut cmds, 1.0, None);
            cmds.push(DisplayCommand::PopTransform);
        }
        // Emit margin-box text content (headers, footers, page numbers).
        if let Some(page_box) = &page.page_box {
            for margin_box in page_box.margin_boxes.values() {
                emit_margin_box_text(margin_box, &mut cmds);
            }
        }
    }
    cmds
}

/// Emits `DrawText` commands for each text fragment in a margin-box.
///
/// Positions are absolute page coordinates: `margin_box.x + fragment.x` and
/// `margin_box.y + fragment.y`. Text uses the page default: 10px black,
/// no explicit font family (renderer falls back to bundled Inter).
fn emit_margin_box_text(margin_box: &MarginBox, cmds: &mut DisplayList) {
    let default_font_size = 10.0_f32;
    let text_color = Color { r: 0, g: 0, b: 0, a: 255 };
    for frag in &margin_box.text_fragments {
        if frag.text.is_empty() {
            continue;
        }
        let rect = Rect {
            x: margin_box.x + frag.x,
            y: margin_box.y + frag.y,
            width: frag.width,
            height: frag.height,
        };
        cmds.push(DisplayCommand::DrawText {
            font_stretch: FontStretch::NORMAL,
            rect,
            text: frag.text.clone(),
            font_size: default_font_size,
            color: text_color,
            font_family: Vec::new(),
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            highlight_name: None,
            text_orientation: None,
        });
    }
}

/// Splits a print display list at `PageBreak` markers.
///
/// Returns one `Vec<DisplayCommand>` per page. The `PageBreak` commands are
/// consumed (not included in any page's slice). An empty input yields an empty
/// outer `Vec`. A list with no `PageBreak` yields a single-element outer `Vec`.
pub fn split_at_page_breaks(cmds: Vec<DisplayCommand>) -> Vec<Vec<DisplayCommand>> {
    let mut pages: Vec<Vec<DisplayCommand>> = Vec::new();
    let mut current: Vec<DisplayCommand> = Vec::new();
    for cmd in cmds {
        if matches!(cmd, DisplayCommand::PageBreak) {
            pages.push(current);
            current = Vec::new();
        } else {
            current.push(cmd);
        }
    }
    pages.push(current);
    pages
}

/// Removes background-graphics paint commands from each print page when the
/// user disabled "Background graphics" in the print dialog (CC-8).
///
/// Mirrors Chrome's "Background graphics" print toggle: when `print_backgrounds`
/// is `false`, the CSS-background paint family is stripped — solid background
/// fills (`FillRect`, `FillRoundedRect`), `background-image`s, and the three
/// gradient kinds (linear/radial/conic). Foreground content — text, borders,
/// outlines, `<img>` raster images, and SVG paths — is preserved.
///
/// No-op when `print_backgrounds` is `true`. Operates in place, page by page;
/// `Push*`/`Pop*` nesting stays balanced because only leaf paint commands are
/// removed.
pub fn strip_background_graphics(pages: &mut [Vec<DisplayCommand>], print_backgrounds: bool) {
    if print_backgrounds {
        return;
    }
    for page in pages.iter_mut() {
        page.retain(|cmd| !is_background_graphic(cmd));
    }
}

/// Classifies a [`DisplayCommand`] as a CSS background-graphics paint op —
/// the set removed when "Background graphics" is off (see
/// [`strip_background_graphics`]).
fn is_background_graphic(cmd: &DisplayCommand) -> bool {
    matches!(
        cmd,
        DisplayCommand::FillRect { .. }
            | DisplayCommand::FillRoundedRect { .. }
            | DisplayCommand::DrawBackgroundImage { .. }
            | DisplayCommand::DrawLinearGradient { .. }
            | DisplayCommand::DrawRadialGradient { .. }
            | DisplayCommand::DrawConicGradient { .. }
    )
}

#[derive(Default, Clone)]
pub(crate) struct ScBucket {
    /// PushOpacity / PushBlendMode / PushClipRect — открывают layer-effects
    /// SC-owner-а перед собственным фоном.
    pub(crate) pre: Vec<DisplayCommand>,
    /// CSS 2.1 Appendix E phase 1 — bg/border SC-owner box-а.
    pub(crate) root_bg: Vec<DisplayCommand>,
    /// Фазы 3/4/5 — descendants SC-owner-а кроме child-SC-creating box-ов.
    pub(crate) contents: Vec<DisplayCommand>,
    /// Pop* в обратном порядке к `pre`. Эмитится после `contents` в фазе
    /// `InlineContent`. См. Phase 0 ограничение в docstring
    /// `build_display_list_ordered`.
    pub(crate) post: Vec<DisplayCommand>,
}

/// Which [`ScBucket`] field a [`RawSpan`] was recorded against. `fill_buckets`
/// only ever appends to one field at a time per call, so this plus the SC id
/// is enough to find the field again when `build_display_list_ordered_dpr`
/// flushes buckets into the final command list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BucketField {
    Pre,
    RootBg,
    Contents,
    Post,
}

/// A [`ProvenanceSpan`] before translation to global command-list indices.
/// `range` is local to one `ScBucket` field (`fill_buckets` only ever sees
/// that field's own, not-yet-flushed `Vec`); `build_display_list_ordered_dpr`
/// offsets it by that field's position in the final list once flushed.
pub(crate) struct RawSpan {
    pub(crate) sc: u32,
    pub(crate) field: BucketField,
    pub(crate) range: Range<usize>,
    pub(crate) origin: BoxOrigin,
    pub(crate) fragment: u32,
}

/// Records `[start, end)` of `field` as one `RawSpan` for `origin`, unless
/// empty (a box that emitted nothing this call — e.g. `display:none` subtree,
/// zero-size overflow clip — gets no span rather than a degenerate one).
pub(crate) fn record_span(
    spans: &mut Vec<RawSpan>,
    sc: u32,
    field: BucketField,
    start: usize,
    end: usize,
    origin: BoxOrigin,
    fragment: u32,
) {
    if end > start {
        spans.push(RawSpan { sc, field, range: start..end, origin, fragment });
    }
}

/// CSS Compositing & Blending L1 §5: маппинг style-уровневого `MixBlendMode`
/// (lumen-layout) в paint-уровневый `BlendMode` (lumen-paint). Enum-ы
/// разные, чтобы не тянуть зависимость paint → layout в обратную сторону;
/// варианты совпадают 1:1.
pub(crate) fn map_blend_mode(m: LayoutBlendMode) -> BlendMode {
    match m {
        LayoutBlendMode::Normal => BlendMode::Normal,
        LayoutBlendMode::Multiply => BlendMode::Multiply,
        LayoutBlendMode::Screen => BlendMode::Screen,
        LayoutBlendMode::Overlay => BlendMode::Overlay,
        LayoutBlendMode::Darken => BlendMode::Darken,
        LayoutBlendMode::Lighten => BlendMode::Lighten,
        LayoutBlendMode::ColorDodge => BlendMode::ColorDodge,
        LayoutBlendMode::ColorBurn => BlendMode::ColorBurn,
        LayoutBlendMode::HardLight => BlendMode::HardLight,
        LayoutBlendMode::SoftLight => BlendMode::SoftLight,
        LayoutBlendMode::Difference => BlendMode::Difference,
        LayoutBlendMode::Exclusion => BlendMode::Exclusion,
        LayoutBlendMode::Hue => BlendMode::Hue,
        LayoutBlendMode::Saturation => BlendMode::Saturation,
        LayoutBlendMode::Color => BlendMode::Color,
        LayoutBlendMode::Luminosity => BlendMode::Luminosity,
        LayoutBlendMode::PlusLighter => BlendMode::PlusLighter,
    }
}

/// CSS Overflow L3 §3.2: значения, при которых overflow создаёт clip-bound
/// для содержимого. `Visible` не клипает.
pub(crate) fn overflow_clips(o: Overflow) -> bool {
    matches!(
        o,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    )
}

/// Em-fraction for approximating U+2026 HORIZONTAL ELLIPSIS advance width.
/// Empirically derived from Inter Regular; the outer overflow:hidden clip
/// prevents pixel bleed if the renderer's actual advance differs slightly.
pub(crate) const ELLIPSIS_EM: f32 = 0.65;

/// Центр basic-shape в page-координатах: `at cx cy` (cx — % от ширины,
/// cy — % от высоты border-box) либо дефолт 50% 50% (CSS Shapes L1 §5.1).
pub(crate) fn resolve_shape_center(center: Option<(ShapeValue, ShapeValue)>, r: Rect) -> (f32, f32) {
    center
        .map(|(x, y)| (r.x + x.resolve(r.width), r.y + y.resolve(r.height)))
        .unwrap_or((r.x + r.width * 0.5, r.y + r.height * 0.5))
}

/// CSS Masking L1 §9 — bounding-box rect for a `clip-path` shape relative to
/// the element's border-box `r`. Для `inset(...)` это точное представление;
/// для circle/ellipse/polygon — bounding box (используется fallback-путями;
/// точная форма идёт через `clip_path_to_shape` → `PushClipPath`, BUG-140).
pub(crate) fn clip_path_to_rect(clip: &ClipPath, r: Rect) -> Rect {
    match clip_path_to_shape(clip, r) {
        Some(shape) => shape.bounding_rect(),
        None => {
            let ClipPath::Inset(sides) = clip else { return r };
            let rs = |v: &ShapeValue, basis: f32| v.resolve(basis);
            let (top, right, bottom, left) = match sides.as_slice() {
                [a] => (rs(a, r.height), rs(a, r.width), rs(a, r.height), rs(a, r.width)),
                [tb, rl] => (rs(tb, r.height), rs(rl, r.width), rs(tb, r.height), rs(rl, r.width)),
                [t, rl, b] => (rs(t, r.height), rs(rl, r.width), rs(b, r.height), rs(rl, r.width)),
                [t, ri, b, l] => (rs(t, r.height), rs(ri, r.width), rs(b, r.height), rs(l, r.width)),
                _ => (0.0, 0.0, 0.0, 0.0),
            };
            Rect::new(
                r.x + left,
                r.y + top,
                (r.width - left - right).max(0.0),
                (r.height - top - bottom).max(0.0),
            )
        }
    }
}

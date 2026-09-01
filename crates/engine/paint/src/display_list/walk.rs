//! P1/SPLIT-DL9: `emit_box_self` + 3D-глубина (`establishes_3d_rendering_context`/
//! `is_backface_hidden`/`child_z_depth`/`depth_sorted_child_order`/
//! `depth_order_by_z`) + gap-декорации (`collect_gap_segments`) + центральный
//! обходчик `walk` — риск группы аналогичен RN-6/BT-10 (широко используемая
//! рекурсивная функция, а не изолированный кусок). Вынесено из
//! `display_list.rs` (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL,
//! батч DL-9).

use super::*;

/// Эмитит DisplayCommand-ы для одного box-а БЕЗ рекурсии в детей. Аналог
/// тела `walk` для одного box-а.
pub(crate) fn emit_box_self(
    b: &LayoutBox,
    out: &mut Vec<DisplayCommand>,
    dpr: f32,
    sel: Option<&SelectionHighlight>,
    ov: Option<&CompositorOverride>,
) {
    // opacity:0 → whole-subtree invisible (см. is_opacity_subtree_painted).
    // emit_box_self не идёт в children, но self-content тоже skip-аем.
    if !is_opacity_subtree_painted(b) {
        return;
    }
    // BUG-231: remember where this box's own commands start so an animated
    // background-color / color compositor override can be patched into them
    // afterwards (see `apply_color_override`) without relayout.
    let cmd_start = out.len();
    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::TableRow
        | BoxKind::Table | BoxKind::TableRowGroup => {
            if !is_paint_visible(b) {
                return;
            }
            // CSS Tables L2 §17.6.1.1 — `empty-cells: hide`: an empty cell draws
            // neither borders nor background. Cell has no children to recurse into,
            // so skipping self-emission fully hides it.
            if is_hidden_empty_cell(b) {
                return;
            }
            emit_box_shadows(b, out);
            let s = &b.style;
            let radii = CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    if radii.all_zero() {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    } else {
                        out.push(DisplayCommand::FillRoundedRect { rect: clip, color: bg, radii });
                    }
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
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
            emit_column_rules(b, out);
            emit_outline(b, out);
        }
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, sel, dpr, out);
        }
        BoxKind::InlineBlockRow | BoxKind::InlineSpace | BoxKind::Contents => {}
        BoxKind::Marker { .. } => {
            emit_list_marker(b, out);
        }
        BoxKind::FormControl { kind } => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            let s = &b.style;
            let radii = CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    if radii.all_zero() {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    } else {
                        out.push(DisplayCommand::FillRoundedRect { rect: clip, color: bg, radii });
                    }
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
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
            emit_outline(b, out);
            emit_form_control_indicator(b, kind, out);
        }
        BoxKind::Image { src, alt, is_lazy } => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
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
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: bitmap belongs in the content box, same rule as <canvas>
            // (BUG-099) — painting at the border box slides it under the border.
            if *is_lazy {
                out.push(DisplayCommand::LazyImageSlot {
                    rect: content_box_rect(b),
                    node_id: b.node.index() as u32,
                    src: src.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                });
            } else {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: src.clone(),
                    alt: alt.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Video { src, poster } => {
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
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
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Phase 1: GIF-backed <video> — frame uploaded by shell under "video:{nid}".
            // Non-GIF src or no src: fall back to poster image (Phase 0 behaviour).
            // The shell's tick loop re-registers the current GIF frame under this key
            // on every render tick, so the DrawImage command always shows the live frame.
            // CSS: object-fit — P4 wires ComputedStyle.object_fit to scale the frame.
            // BUG-431: destination is the content box, not the border box.
            let nid = b.node.index();
            let is_gif_src = src.to_ascii_lowercase().ends_with(".gif") && !src.is_empty();
            if is_gif_src {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: format!("video:{nid}"),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            } else if !poster.is_empty() {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: poster.clone(),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Canvas { .. } => {
            // HTML LS §4.12.4: <canvas> is a replaced element. Painter's order:
            // box-shadows → background → bg-image → border → bitmap → outline.
            if !is_paint_visible(b) {
                return;
            }
            emit_box_shadows(b, out);
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            emit_inset_box_shadows(b, out);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
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
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Bitmap is uploaded by the shell under `canvas:{node_id}`. Until JS
            // draws anything the key is unregistered → transparent placeholder.
            // BUG-099: the bitmap belongs in the *content* box — painting it at
            // `b.rect` slid it under the border by the border width.
            let nid = b.node.index();
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: format!("canvas:{nid}"),
                alt: String::new(),
                object_fit: ObjectFit::Fill,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        BoxKind::Audio { controls, .. } => {
            if !is_paint_visible(b) || !controls || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            // Phase 0: render a grey bar representing the audio controls UI.
            let grey = Color { r: 200, g: 200, b: 200, a: 255 };
            out.push(DisplayCommand::FillRect { rect: b.rect, color: grey });
            emit_outline(b, out);
        }
        BoxKind::Iframe { src, .. } => {
            if !is_paint_visible(b) || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            emit_box_shadows(b, out);
            // Phase 0: grey placeholder — no sub-document navigation.
            // Using DrawImage with src as key: unregistered key → grey placeholder
            // (same pattern as Video). The src string identifies this iframe to
            // the shell for potential future navigation.
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width,
                        s.border_right_width,
                        s.border_bottom_width,
                        s.border_left_width,
                    ],
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
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: destination is the content box, not the border box.
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: src.clone(),
                alt: String::new(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        // SVG elements: in the ordered (stacking-context) path `fill_buckets`
        // already recurses into children, so each box paints only its own
        // content here — no child recursion, unlike `walk` (which descends
        // SvgRoot's shape/text children itself).
        BoxKind::SvgRoot { .. } => {
            if is_paint_visible(b)
                && let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
            }
        }
        BoxKind::SvgShape { shape, svg_mask, .. } => {
            match svg_mask.as_deref() {
                Some(mask) => emit_svg_shape_masked(b, shape, mask, out, dpr, sel),
                None => emit_svg_shape(b, shape, out),
            }
        }
        BoxKind::SvgText { text, text_anchor, dominant_baseline, baseline_shift, .. } => {
            emit_svg_text(b, text, *text_anchor, *dominant_baseline, *baseline_shift, out);
        }
    }
    // BUG-231: apply animated background-color / color compositor override to the
    // commands this box just emitted (range `cmd_start..`), before the resize grip.
    if let Some(ov) = ov
        && (ov.background_color.is_some() || ov.color.is_some())
    {
        apply_color_override(b, ov, &mut out[cmd_start..]);
    }
    emit_resize_grip(b, out);
}

/// BUG-231: patch a box's own background fill and currentColor-derived border /
/// outline colours with the compositor override `ov`, in place, without relayout.
///
/// The background fill is identified by its exact clip rect: drop-shadow fills use
/// a different (offset/spread-expanded) rect, so they are left untouched. Borders
/// and outline are re-resolved from the box style against the overridden
/// currentColor. Only fills already present are patched — a transition starting
/// from a transparent background still needs relayout to inject a fill.
fn apply_color_override(b: &LayoutBox, ov: &CompositorOverride, cmds: &mut [DisplayCommand]) {
    if let Some(bg) = ov.background_color {
        let clip = background_clip_rect(b, background_color_clip(b));
        for c in cmds.iter_mut() {
            match c {
                DisplayCommand::FillRect { rect, color } if *rect == clip => *color = bg,
                DisplayCommand::FillRoundedRect { rect, color, .. } if *rect == clip => *color = bg,
                _ => {}
            }
        }
    }
    if let Some(cur) = ov.color {
        let s = &b.style;
        let outline_uses_current = matches!(
            s.outline_color,
            OutlineColor::Auto | OutlineColor::CurrentColor
        );
        for c in cmds.iter_mut() {
            match c {
                DisplayCommand::DrawBorder { colors, .. } => {
                    *colors = [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ];
                }
                DisplayCommand::DrawOutline { color, .. } if outline_uses_current => *color = cur,
                _ => {}
            }
        }
    }
}

/// CSS Transforms L2 §6.1 — does this box establish a **3D rendering context**
/// for its children? When `true`, the children share one 3D coordinate space
/// and are painted in depth order (see [`depth_sorted_child_order`]) instead of
/// being flattened to z=0 individually and painted in document order.
///
/// A box establishes a 3D rendering context iff `transform-style: preserve-3d`.
pub(crate) fn establishes_3d_rendering_context(b: &LayoutBox) -> bool {
    b.style.transform_style == TransformStyle::Preserve3d
}

/// CSS Transforms L2 §5.1 — `backface-visibility: hidden` culls a box once
/// its own 3D transform has rotated its face past 90° from the viewer.
///
/// The box's face normal in its own coordinate space is `(0, 0, 1)`; the
/// linear part of `forward_box_transform` maps it to `(m[8], m[9], m[10])`
/// (translation columns don't affect direction vectors), so `m[10]` alone —
/// the same raw z used by [`child_z_depth`]'s `transform_z` — tells which way
/// the face points: negative means it has flipped into the screen.
pub(crate) fn is_backface_hidden(b: &LayoutBox) -> bool {
    b.style.backface_visibility == BackfaceVisibility::Hidden
        && matches!(forward_box_transform(b), Some(m) if m.0[10] < 0.0)
}

/// Transformed depth of a box's center within its parent's 3D rendering
/// context. Applies the box's own forward transform (`forward_box_transform`,
/// which includes `transform-origin` pivot) to the box-center at z=0 and takes
/// the **raw** transformed z (`Mat4::transform_z`, no perspective divide — see
/// its doc for why). Boxes without a transform sit at z=0. Larger z = nearer
/// the viewer (CSS convention).
fn child_z_depth(b: &LayoutBox) -> f32 {
    match forward_box_transform(b) {
        Some(m) => {
            let cx = b.rect.x + b.rect.width * 0.5;
            let cy = b.rect.y + b.rect.height * 0.5;
            m.transform_z(cx, cy, 0.0)
        }
        None => 0.0,
    }
}

/// CSS Transforms L2 §6.2 — painting order inside a 3D rendering context.
///
/// Returns indices into `children` ordered **back-to-front**: the child with
/// the smallest transformed z ([`child_z_depth`]) is painted first (farthest
/// from the viewer), the largest z last (nearest, so it correctly occludes the
/// others). The sort is **stable** — children at equal depth keep document
/// order, preserving the normal stacking rule for coplanar siblings.
///
/// This is the painter's-algorithm depth sort. Pixel-exact handling of mutually
/// *intersecting* planes (BSP / plane splitting) is a future extension; for the
/// common case of non-intersecting transformed planes this yields correct
/// occlusion. A GPU depth buffer is the alternative; see STATUS-P2.
pub(crate) fn depth_sorted_child_order(children: &[LayoutBox]) -> Vec<usize> {
    let z: Vec<f32> = children.iter().map(child_z_depth).collect();
    depth_order_by_z(&z)
}

/// Pure back-to-front ordering of indices `0..z.len()` by depth `z[i]`.
/// Smallest z first (farthest), largest last (nearest). Stable: equal depths
/// keep their original order. `NaN` depths compare as equal (treated as
/// coplanar) so a degenerate transform never panics or reorders unpredictably.
/// Split out from [`depth_sorted_child_order`] so the ordering logic is unit-
/// testable without constructing a layout tree.
pub(crate) fn depth_order_by_z(z: &[f32]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..z.len()).collect();
    // `sort_by` is stable: coplanar siblings retain document order.
    order.sort_by(|&a, &b| z[a].partial_cmp(&z[b]).unwrap_or(std::cmp::Ordering::Equal));
    order
}

/// Collects `GapSegment`s for `gap-rule-*` rendering in flex/grid containers.
///
/// Scans child box right-edges and top-edges against the container's `column_gap`
/// and `row_gap` values; emits one `GapSegment` per actual gap found. Works for
/// both single-line and multi-line flex, and for grid containers.
///
/// Returns an empty `Vec` when the container is not flex/grid, or when both gap
/// values are zero, or when `gap_rule_style` is `None` / `gap_rule_width` ≤ 0.
fn collect_gap_segments(b: &LayoutBox) -> Vec<GapSegment> {
    let s = &b.style;
    // Only flex/grid containers produce gap rules.
    let is_flex_or_grid = matches!(
        s.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    );
    if !is_flex_or_grid {
        return Vec::new();
    }
    if !s.gap_rule_style.is_visible() || s.gap_rule_width <= 0.0 {
        return Vec::new();
    }

    // Content area of the container (border-box minus border+padding).
    let em = s.font_size;
    let cw = (b.rect.width
        - s.border_left_width
        - s.border_right_width
        - s.padding_left.px()
        - s.padding_right.px())
    .max(0.0);
    let ch = (b.rect.height
        - s.border_top_width
        - s.border_bottom_width
        - s.padding_top.px()
        - s.padding_bottom.px())
    .max(0.0);
    let cx = b.rect.x + s.border_left_width + s.padding_left.px();
    let cy = b.rect.y + s.border_top_width + s.padding_top.px();
    let vp = Size::new(cw, ch);

    let col_gap_px = s.column_gap.resolve_or_zero(em, cw, vp);
    let row_gap_px = s.row_gap.resolve_or_zero(em, ch, vp);

    // Collect in-flow (non-absolutely-positioned, non-skip) children.
    let children: Vec<_> = b
        .children
        .iter()
        .filter(|c| {
            !matches!(c.kind, BoxKind::Skip | BoxKind::Contents | BoxKind::Marker { .. })
                && !matches!(c.style.position, Position::Absolute | Position::Fixed)
        })
        .collect();

    if children.len() < 2 {
        return Vec::new();
    }

    let mut segments: Vec<GapSegment> = Vec::new();
    const EPS: f32 = 1.5; // tolerance for float layout rounding

    if col_gap_px > 0.0 {
        // Collect unique right-edges of children.
        let mut rights: Vec<f32> =
            children.iter().map(|c| c.rect.x + c.rect.width).collect();
        rights.sort_by(|a, x| a.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
        rights.dedup_by(|a, x| (*a - *x).abs() < EPS);

        // For each right-edge, check if a child starts right_edge + col_gap away.
        let lefts: Vec<f32> = children.iter().map(|c| c.rect.x).collect();
        for right in &rights {
            let expected = right + col_gap_px;
            if lefts.iter().any(|l| (*l - expected).abs() < EPS) {
                segments.push(GapSegment {
                    rect: Rect::new(*right, cy, col_gap_px, ch),
                    horizontal: false,
                });
            }
        }
    }

    if row_gap_px > 0.0 {
        // Collect unique bottom-edges of children.
        let mut bottoms: Vec<f32> =
            children.iter().map(|c| c.rect.y + c.rect.height).collect();
        bottoms.sort_by(|a, x| a.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
        bottoms.dedup_by(|a, x| (*a - *x).abs() < EPS);

        let tops: Vec<f32> = children.iter().map(|c| c.rect.y).collect();
        for bottom in &bottoms {
            let expected = bottom + row_gap_px;
            if tops.iter().any(|t| (*t - expected).abs() < EPS) {
                segments.push(GapSegment {
                    rect: Rect::new(cx, *bottom, cw, row_gap_px),
                    horizontal: true,
                });
            }
        }
    }

    segments
}

pub(crate) fn walk(b: &LayoutBox, out: &mut DisplayList, dpr: f32, sel: Option<&SelectionHighlight>) {
    // CSS Color L3 §3.2 — opacity:0 на box-е делает весь subtree после
    // composite полностью прозрачным. Phase 0 эмулирует это pure-pixel
    // skip-ом (отличие от visibility:hidden, где children могут
    // override через `:visible` — opacity-0 такого override не имеет).
    if !is_opacity_subtree_painted(b) {
        return;
    }
    // CSS Transforms L2 §5.1 — `backface-visibility: hidden` culls the box
    // (and its subtree) once its own 3D transform has rotated its face past
    // 90°, so it points away from the viewer.
    if is_backface_hidden(b) {
        return;
    }
    // CSS Positioning L3 §6.3 — position:sticky. Wraps the entire box in a
    // BeginStickyLayer/EndStickyLayer pair so the renderer can apply a
    // scroll-clamped offset at draw time without rebuilding the display list.
    let is_sticky = matches!(b.style.position, Position::Sticky);
    if is_sticky {
        let s = &b.style;
        out.push(DisplayCommand::BeginStickyLayer {
            flow_rect: b.rect,
            top:    s.top.to_px_opt(),
            bottom: s.bottom.to_px_opt(),
            left:   s.left.to_px_opt(),
            right:  s.right.to_px_opt(),
        });
    }
    // CSS Positioning L3 §6.1 — position:fixed. Brackets the box (and subtree)
    // with a BeginFixedLayer/EndFixedLayer pair so the compositor scroll-blit can
    // split it out of the scrollable band (ADR-016 M3.2.1c). No draw-time offset:
    // fixed content is already at viewport-fixed coords (BUG-159), so the markers
    // render as no-ops — they are partition metadata only.
    let is_fixed = matches!(b.style.position, Position::Fixed);
    if is_fixed {
        out.push(DisplayCommand::BeginFixedLayer);
    }
    match &b.kind {
        BoxKind::Skip | BoxKind::Contents => {}
        BoxKind::Block | BoxKind::FlowRoot | BoxKind::TableRow
        | BoxKind::Table | BoxKind::TableRowGroup => {
            // CSS Masking L1 §4: mask-image wraps the entire element (opacity+transform+content).
            // Emitted outermost so the mask applies to the fully composited element.
            // `mask_groups` > 1 — вложенные группы `mask-composite: intersect`
            // (см. `rendered_mask_layers`); закрываются столькими же PopMask.
            let mask_groups = emit_push_mask(out, b);
            let has_mask = mask_groups > 0;
            // CSS Masking L1 §4.6 — `mask-clip` restricts the masked painting to
            // the padding/content box. Pushed inside the mask group; popped before
            // PopMask below.
            let mask_clip = if has_mask { mask_clip_paint_rect(b) } else { None };
            if let Some(clip) = mask_clip {
                out.push(DisplayCommand::PushClipRect { rect: clip });
            }
            // CSS Masking L1 §9: clip-path clips the fully composited element;
            // эмитится ниже — ВНУТРИ PushTransform (BUG-140).
            let has_clip_path = b.style.clip_path.is_some();
            // CSS Compositing & Blending L1 §5: mix-blend-mode wraps opacity so
            // the element (faded by its own opacity) blends against the backdrop
            // (order Clip → Blend → Opacity, mirroring `box_layer_ops`).
            let has_blend = b.style.mix_blend_mode != LayoutBlendMode::Normal;
            if has_blend {
                out.push(DisplayCommand::PushBlendMode {
                    mode: map_blend_mode(b.style.mix_blend_mode),
                    bounds: b.rect,
                });
            }
            // CSS Color L3 §3: opacity < 1.0 creates compositing layer.
            let has_opacity = b.style.opacity < 1.0; // >0.0 already checked above
            if has_opacity {
                out.push(DisplayCommand::PushOpacity { alpha: b.style.opacity, bounds: Some(b.rect) });
            }
            // CSS Transforms L1 §13: forward-матрица применяется до родителя,
            // т.е. PushTransform — ВНУТРИ opacity-layer-а. Применяется ко
            // всему содержимому box-а (включая собственный background/border).
            let transform = forward_box_transform(b);
            if let Some(matrix) = transform {
                out.push(DisplayCommand::PushTransform { matrix });
            }
            // CSS Masking L1 §9 + BUG-140: clip-path задан в локальной системе
            // элемента и переносится его transform-ом — эмитится внутри
            // PushTransform, снаружи filter/backdrop-filter.
            if let Some(clip) = &b.style.clip_path {
                match clip_path_to_shape(clip, b.rect) {
                    Some(shape) => out.push(DisplayCommand::PushClipPath { shape }),
                    None => out.push(DisplayCommand::PushClipRect {
                        rect: clip_path_to_rect(clip, b.rect),
                    }),
                }
            }
            // CSS Filter Effects L1 §6.2 — `backdrop-filter` filters the content
            // already painted *behind* the element, clipped to its border box,
            // before the element's own content paints on top. Emitted after the
            // transform (mirroring `box_layer_ops` ordering) and outermost
            // relative to the element's own `filter`, so the element content
            // composites over the filtered backdrop.
            let has_backdrop = !b.style.backdrop_filter.is_empty();
            if has_backdrop {
                out.push(DisplayCommand::PushBackdropFilter {
                    filters: b.style.backdrop_filter.clone(),
                    bounds: b.rect,
                });
            }
            // CSS Filter Effects L1 §4 — the element's own `filter` wraps the
            // element's full painted output (shadows + background + border +
            // children + outline) as the innermost layer; the matching
            // `PopFilter` applies the chain and composites the result down.
            let has_filter = !b.style.filter.is_empty();
            if has_filter {
                out.push(DisplayCommand::PushFilter {
                    filters: b.style.filter.clone(),
                    bounds: Some(b.rect),
                });
            }
            // CSS Display L3 §4 — `visibility: hidden`: self не рисуется
            // (фон/border/outline/shadow), но children обходятся (inherited
            // visibility, но child может вернуть себя через `:visible`).
            // CSS Tables L2 §17.6.1.1 — `empty-cells: hide` suppresses an empty
            // cell's background and borders the same way (children still walked).
            let self_visible = is_paint_visible(b) && !is_hidden_empty_cell(b);
            if self_visible {
                emit_box_shadows(b, out);
                if let Some(CssColor::Rgba(bg)) = b.style.background_color
                    && bg.a > 0
                {
                    let clip = background_clip_rect(b, background_color_clip(b));
                    if clip.width > 0.0 && clip.height > 0.0 {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    }
                }
                emit_background_image(out, b, dpr);
                emit_inset_box_shadows(b, out);
                let s = &b.style;
                let has_border = s.border_top_style.is_visible()
                    || s.border_right_style.is_visible()
                    || s.border_bottom_style.is_visible()
                    || s.border_left_style.is_visible();
                if has_border {
                    let cur = s.color;
                    out.push(DisplayCommand::DrawBorder {
                        rect: b.rect,
                        widths: [
                            s.border_top_width, s.border_right_width,
                            s.border_bottom_width, s.border_left_width,
                        ],
                        colors: [
                            s.border_top_color.resolve(cur),
                            s.border_right_color.resolve(cur),
                            s.border_bottom_color.resolve(cur),
                            s.border_left_color.resolve(cur),
                        ],
                        styles: [
                            s.border_top_style, s.border_right_style,
                            s.border_bottom_style, s.border_left_style,
                        ],
                        radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                    });
                }
                emit_column_rules(b, out);
            }
            // CSS Overflow L3 §3.2: overflow: hidden/scroll/auto/clip clips
            // descendant content to the padding-box edge. Per-axis: only the
            // clipping axis is constrained; the unconstrained axis uses a large
            // sentinel so the GPU scissor doesn't cut off content in that
            // direction (the renderer clamps to surface bounds automatically).
            // scroll/auto → PushScrollLayer (clip + scroll translate).
            // hidden/clip/paint-contain → PushClipRect (clip only).
            let clip_x = overflow_clips(b.style.overflow_x);
            let clip_y = overflow_clips(b.style.overflow_y);
            let has_overflow_clip = clip_x || clip_y;
            let is_scroll_x = matches!(b.style.overflow_x, Overflow::Scroll | Overflow::Auto);
            let is_scroll_y = matches!(b.style.overflow_y, Overflow::Scroll | Overflow::Auto);
            let use_scroll_layer = (is_scroll_x || is_scroll_y) && has_overflow_clip;
            // Capture padding-box rect for scrollbar geometry (used after PopScrollLayer).
            let scroll_padding_box: Option<(f32, f32, f32, f32)> = if use_scroll_layer {
                let s = &b.style;
                let px = b.rect.x + s.border_left_width;
                let py = b.rect.y + s.border_top_width;
                let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
                let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
                Some((px, py, pw, ph))
            } else {
                None
            };
            if has_overflow_clip {
                const BIG: f32 = 1_000_000.0;
                let s = &b.style;
                let px = b.rect.x + s.border_left_width;
                let py = b.rect.y + s.border_top_width;
                let pw = (b.rect.width - s.border_left_width - s.border_right_width).max(0.0);
                let ph = (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0);
                let mut cr = Rect::new(
                    if clip_x { px } else { -BIG },
                    if clip_y { py } else { -BIG },
                    if clip_x { pw } else { 2.0 * BIG },
                    if clip_y { ph } else { 2.0 * BIG },
                );

                // CSS Overflow L3: overflow-clip-margin расширяет clip region для overflow:clip.
                let is_overflow_clip_x = matches!(b.style.overflow_x, Overflow::Clip);
                let is_overflow_clip_y = matches!(b.style.overflow_y, Overflow::Clip);
                if (is_overflow_clip_x || is_overflow_clip_y)
                    && let Some(margin) = &s.overflow_clip_margin
                    && let Some(margin_px) = margin.resolve(s.font_size, Some(pw.max(ph)), Size::new(pw, ph))
                {
                    if is_overflow_clip_x {
                        cr.x -= margin_px;
                        cr.width += 2.0 * margin_px;
                    }
                    if is_overflow_clip_y {
                        cr.y -= margin_px;
                        cr.height += 2.0 * margin_px;
                    }
                }

                if use_scroll_layer {
                    out.push(DisplayCommand::PushScrollLayer {
                        clip_rect: cr,
                        scroll_x: b.scroll_x,
                        scroll_y: b.scroll_y,
                    });
                } else {
                    out.push(DisplayCommand::PushClipRect { rect: cr });
                }
            }
            // CSS Transforms L2 §6.2: inside a `preserve-3d` 3D rendering
            // context children paint back-to-front by transformed depth;
            // otherwise document order (flat compositing).
            // Special handling for Table: emit table-specific layout (cells, borders, etc).
            if matches!(b.kind, BoxKind::Table) {
                emit_table_box(b, out, dpr);
            } else if establishes_3d_rendering_context(b) {
                for i in depth_sorted_child_order(&b.children) {
                    walk(&b.children[i], out, dpr, sel);
                }
            } else {
                for child in &b.children {
                    walk(child, out, dpr, sel);
                }
            }
            // CSS Gap Decorations L1 — emit gap rules for flex/grid containers.
            if self_visible {
                let gap_segs = collect_gap_segments(b);
                if !gap_segs.is_empty() {
                    let s = &b.style;
                    let ctx = GapDecorationContext {
                        rule_width: s.gap_rule_width,
                        rule_style: s.gap_rule_style,
                        rule_color: s.gap_rule_color.resolve(s.color),
                    };
                    out.extend(emit_gap_rules(&b.children, &gap_segs, &ctx));
                }
            }
            if has_overflow_clip {
                if use_scroll_layer {
                    out.push(DisplayCommand::PopScrollLayer);
                    // Emit scrollbar track + thumb after the scroll layer so they
                    // render at a fixed position (not translated with scrolled content).
                    // BUG-220: shared with the ordered `box_layer_ops` path.
                    if let Some(padding_box) = scroll_padding_box {
                        emit_scrollbars(b, padding_box, is_scroll_x, is_scroll_y, out);
                    }
                } else {
                    out.push(DisplayCommand::PopClip);
                }
            }
            if self_visible {
                // CSS Basic UI L4 §5: outline рисуется поверх контента box-а
                // (включая children), снаружи bounding-box-а. Phase 0 без
                // деления paint phases для outline — эмитим в конце box-walk-а.
                emit_outline(b, out);
            }
            if has_filter {
                out.push(DisplayCommand::PopFilter);
            }
            if has_backdrop {
                out.push(DisplayCommand::PopBackdropFilter);
            }
            if has_clip_path {
                out.push(DisplayCommand::PopClip);
            }
            if transform.is_some() {
                out.push(DisplayCommand::PopTransform);
            }
            if has_opacity {
                out.push(DisplayCommand::PopOpacity);
            }
            if has_blend {
                out.push(DisplayCommand::PopBlendMode);
            }
            if mask_clip.is_some() {
                out.push(DisplayCommand::PopClip);
            }
            for _ in 0..mask_groups {
                out.push(DisplayCommand::PopMask);
            }
        }
        BoxKind::FormControl { kind } => {
            // Replaced element: background + border box (Phase 0, no content).
            if !is_paint_visible(b) {
                return;
            }
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            emit_outline(b, out);
            emit_form_control_indicator(b, kind, out);
        }
        BoxKind::InlineBlockRow => {
            // Анонимный контейнер: нет фона/бордера собственного.
            // Просто рекурсивно рисуем всех дочерних (BoxKind::Block).
            for child in &b.children {
                walk(child, out, dpr, sel);
            }
        }
        BoxKind::InlineSpace => {}
        BoxKind::Marker { .. } => {
            emit_list_marker(b, out);
        }
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, sel, dpr, out);
        }
        BoxKind::Image { src, alt, is_lazy } => {
            // visibility:hidden на `<img>` пропускает всё (no children).
            if !is_paint_visible(b) {
                return;
            }
            // Painter's order для replaced element: фон → bg-image → border → <img>.
            // background/border у `<img>` валидны по CSS — например, для
            // подложки на время загрузки или рамки вокруг картинки.
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: bitmap belongs in the content box, same rule as <canvas>
            // (BUG-099) — painting at the border box slides it under the border.
            if *is_lazy {
                out.push(DisplayCommand::LazyImageSlot {
                    rect: content_box_rect(b),
                    node_id: b.node.index() as u32,
                    src: src.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                });
            } else {
                // object-fit / object-position читаются на render-стадии вместе
                // с известным intrinsic-размером изображения.
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: src.clone(),
                    alt: alt.clone(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Video { src, poster } => {
            // visibility:hidden на `<video>` пропускает всё (no children).
            if !is_paint_visible(b) {
                return;
            }
            // Painter's order для replaced element: фон → bg-image → border → placeholder.
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Phase 1: GIF-backed <video> — frame uploaded by shell under "video:{nid}".
            // Non-GIF src or no src: fall back to poster image (Phase 0 behaviour).
            // The shell's tick loop re-registers the current GIF frame under this key
            // on every render tick, so the DrawImage command always shows the live frame.
            // CSS: object-fit — P4 wires ComputedStyle.object_fit to scale the frame.
            // BUG-431: destination is the content box, not the border box.
            let nid = b.node.index();
            let is_gif_src = src.to_ascii_lowercase().ends_with(".gif") && !src.is_empty();
            if is_gif_src {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: format!("video:{nid}"),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            } else if !poster.is_empty() {
                out.push(DisplayCommand::DrawImage {
                    rect: content_box_rect(b),
                    src: poster.clone(),
                    alt: String::new(),
                    object_fit: b.style.object_fit,
                    object_position: b.style.object_position,
                    image_rendering: b.style.image_rendering,
                });
            }
            emit_outline(b, out);
        }
        BoxKind::Canvas { .. } => {
            // visibility:hidden on <canvas> skips everything (no children).
            if !is_paint_visible(b) {
                return;
            }
            // Painter's order for replaced element: background → bg-image → border → bitmap.
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // Bitmap uploaded by shell under `canvas:{node_id}`; unregistered → transparent.
            // BUG-099: destination is the content box, not the border box.
            let nid = b.node.index();
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: format!("canvas:{nid}"),
                alt: String::new(),
                object_fit: ObjectFit::Fill,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        BoxKind::Audio { controls, .. } => {
            if !is_paint_visible(b) || !controls || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            // Phase 0: grey bar for audio controls UI.
            let grey = Color { r: 200, g: 200, b: 200, a: 255 };
            out.push(DisplayCommand::FillRect { rect: b.rect, color: grey });
            emit_outline(b, out);
        }
        BoxKind::Iframe { src, .. } => {
            if !is_paint_visible(b) || b.rect.width <= 0.0 || b.rect.height <= 0.0 {
                return;
            }
            // Phase 0: grey placeholder — no sub-document navigation.
            // DrawImage with src as key: unregistered key → grey placeholder (same as Video).
            if let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                let clip = background_clip_rect(b, background_color_clip(b));
                if clip.width > 0.0 && clip.height > 0.0 {
                    out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                }
            }
            emit_background_image(out, b, dpr);
            let s = &b.style;
            let has_border = s.border_top_style.is_visible()
                || s.border_right_style.is_visible()
                || s.border_bottom_style.is_visible()
                || s.border_left_style.is_visible();
            if has_border {
                let cur = s.color;
                out.push(DisplayCommand::DrawBorder {
                    rect: b.rect,
                    widths: [
                        s.border_top_width, s.border_right_width,
                        s.border_bottom_width, s.border_left_width,
                    ],
                    colors: [
                        s.border_top_color.resolve(cur),
                        s.border_right_color.resolve(cur),
                        s.border_bottom_color.resolve(cur),
                        s.border_left_color.resolve(cur),
                    ],
                    styles: [
                        s.border_top_style, s.border_right_style,
                        s.border_bottom_style, s.border_left_style,
                    ],
                    radii: CornerRadii::from_style_and_box(s, b.rect.width, b.rect.height),
                });
            }
            // BUG-431: destination is the content box, not the border box.
            out.push(DisplayCommand::DrawImage {
                rect: content_box_rect(b),
                src: src.clone(),
                alt: String::new(),
                object_fit: b.style.object_fit,
                object_position: b.style.object_position,
                image_rendering: b.style.image_rendering,
            });
            emit_outline(b, out);
        }
        BoxKind::SvgRoot { .. } => {
            // SVG root: draw optional background/border, then recurse into shape children.
            if is_paint_visible(b)
                && let Some(bg) = b.style.background_color.and_then(|c| c.to_color_opt())
                && bg.a > 0
            {
                out.push(DisplayCommand::FillRect { rect: b.rect, color: bg });
            }
            // SVG §7.4: the outermost SVG viewport clips its content (UA default
            // `overflow: hidden`) — object-fit: cover / oversized viewBox content
            // must not paint outside the SVG box. BUG-110.
            let s = &b.style;
            let clip = Rect::new(
                b.rect.x + s.border_left_width,
                b.rect.y + s.border_top_width,
                (b.rect.width - s.border_left_width - s.border_right_width).max(0.0),
                (b.rect.height - s.border_top_width - s.border_bottom_width).max(0.0),
            );
            out.push(DisplayCommand::PushClipRect { rect: clip });
            for child in &b.children {
                walk(child, out, dpr, sel);
            }
            out.push(DisplayCommand::PopClip);
        }
        BoxKind::SvgShape { shape, svg_mask, .. } => {
            // CSS: fill, stroke, stroke-width — P4 wires ComputedStyle svg_fill/svg_stroke fields.
            // Default SVG presentation: fill=black (SVG spec §11.2), no stroke.
            match svg_mask.as_deref() {
                Some(mask) => emit_svg_shape_masked(b, shape, mask, out, dpr, sel),
                None => emit_svg_shape(b, shape, out),
            }
        }
        BoxKind::SvgText { text, text_anchor, dominant_baseline, baseline_shift, .. } => {
            // SVG text element: emit DrawText command with proper positioning.
            // CSS: fill, stroke, font-family, font-size — P4 wires ComputedStyle fields.
            // // CSS: text-anchor, dominant-baseline, baseline-shift
            emit_svg_text(b, text, *text_anchor, *dominant_baseline, *baseline_shift, out);
        }
    }
    if is_fixed {
        out.push(DisplayCommand::EndFixedLayer);
    }
    if is_sticky {
        out.push(DisplayCommand::EndStickyLayer);
    }
}

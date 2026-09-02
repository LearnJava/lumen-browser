//! P1/SPLIT-DL15: serialize + name-mapping — `fn serialize_display_list` … до
//! конца `fn blend_mode_name`. Вынесено из `display_list.rs`
//! (`docs/tasks/p1-monolith-split-queue.md` §4, группа DL, батч DL-15).

use super::*;

pub fn serialize_display_list(dl: &[DisplayCommand]) -> String {
    let mut out = String::new();
    for cmd in dl {
        match cmd {
            DisplayCommand::FillRect { rect, color } => {
                out.push_str(&format!(
                    "FillRect ({:.2}, {:.2}, {:.2}, {:.2}) #{:02x}{:02x}{:02x}{:02x}\n",
                    rect.x, rect.y, rect.width, rect.height,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::FillRoundedRect { rect, color, radii } => {
                out.push_str(&format!(
                    "FillRoundedRect ({:.2}, {:.2}, {:.2}, {:.2}) #{:02x}{:02x}{:02x}{:02x} r=[{:.2},{:.2},{:.2},{:.2}]\n",
                    rect.x, rect.y, rect.width, rect.height,
                    color.r, color.g, color.b, color.a,
                    radii.tl, radii.tr, radii.br, radii.bl,
                ));
            }
            DisplayCommand::DrawBorder {
                rect,
                widths: [wt, wr, wb, wl],
                colors: [ct, cr, cb, cl],
                styles: [st, sr, sb, sl],
                radii: _,
            } => {
                out.push_str(&format!(
                    "DrawBorder ({:.2}, {:.2}, {:.2}, {:.2}) \
                     w=[{:.2},{:.2},{:.2},{:.2}] \
                     c=[#{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x},\
                        #{:02x}{:02x}{:02x}{:02x},#{:02x}{:02x}{:02x}{:02x}]",
                    rect.x, rect.y, rect.width, rect.height,
                    wt, wr, wb, wl,
                    ct.r, ct.g, ct.b, ct.a,
                    cr.r, cr.g, cr.b, cr.a,
                    cb.r, cb.g, cb.b, cb.a,
                    cl.r, cl.g, cl.b, cl.a,
                ));
                let any_non_solid = ![*st, *sr, *sb, *sl]
                    .iter()
                    .all(|s| matches!(s, BorderStyle::Solid | BorderStyle::None));
                if any_non_solid {
                    out.push_str(&format!(
                        " s=[{},{},{},{}]",
                        border_style_short(*st),
                        border_style_short(*sr),
                        border_style_short(*sb),
                        border_style_short(*sl),
                    ));
                }
                out.push('\n');
            }
            DisplayCommand::DrawText {
                rect, text, font_size, color, font_family, font_weight, font_style,
                font_stretch, font_variation_axes, font_features, font_palette, tab_size: _,
                highlight_name: _, text_orientation: _,
            } => {
                out.push_str(&format!(
                    "DrawText ({:.2}, {:.2}, {:.2}, {:.2}) {:?} {:.2} #{:02x}{:02x}{:02x}{:02x}",
                    rect.x, rect.y, rect.width, rect.height,
                    text,
                    font_size,
                    color.r, color.g, color.b, color.a,
                ));
                if !font_family.is_empty() {
                    out.push_str(" family=[");
                    for (i, name) in font_family.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&format!("{name:?}"));
                    }
                    out.push(']');
                }
                if *font_weight != FontWeight::NORMAL {
                    out.push_str(&format!(" w={}", font_weight.0));
                }
                if *font_style != FontStyle::Normal {
                    out.push_str(match font_style {
                        FontStyle::Italic => " style=italic",
                        FontStyle::Oblique => " style=oblique",
                        FontStyle::Normal => "",
                    });
                }
                if *font_stretch != FontStretch::NORMAL {
                    // Проценты, как в layout-снапшоте: stretch=75 ≡ condensed.
                    out.push_str(&format!(" stretch={}", font_stretch.as_percent()));
                }
                if !font_variation_axes.is_empty() {
                    out.push_str(" var=[");
                    for (i, (tag, val)) in font_variation_axes.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        let tag_str = std::str::from_utf8(tag).unwrap_or("????");
                        out.push_str(&format!("{tag_str:?}={val}"));
                    }
                    out.push(']');
                }
                if !font_features.is_empty() {
                    out.push_str(" feat=[");
                    for (i, (tag, val)) in font_features.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        let tag_str = std::str::from_utf8(tag).unwrap_or("????");
                        out.push_str(&format!("{tag_str:?}={val}"));
                    }
                    out.push(']');
                }
                match font_palette {
                    None => {}
                    Some(FontPaletteSelection::Light) => out.push_str(" palette=light"),
                    Some(FontPaletteSelection::Dark) => out.push_str(" palette=dark"),
                    Some(FontPaletteSelection::Custom { base_palette, overrides }) => {
                        out.push_str(&format!(
                            " palette=custom(base={base_palette},overrides={})",
                            overrides.len()
                        ));
                    }
                }
                out.push('\n');
            }
            DisplayCommand::DrawOutline { rect, width, style, color, offset } => {
                out.push_str(&format!(
                    "DrawOutline ({:.2}, {:.2}, {:.2}, {:.2}) w={:.2} \
                     s={} #{:02x}{:02x}{:02x}{:02x}",
                    rect.x, rect.y, rect.width, rect.height,
                    width,
                    outline_style_name(*style),
                    color.r, color.g, color.b, color.a,
                ));
                if *offset != 0.0 {
                    out.push_str(&format!(" off={offset:.2}"));
                }
                out.push('\n');
            }
            DisplayCommand::DrawImage { rect, src, alt, object_fit, object_position, .. } => {
                out.push_str(&format!(
                    "DrawImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?} alt={alt:?}",
                    rect.x, rect.y, rect.width, rect.height,
                ));
                if *object_fit != ObjectFit::Fill {
                    out.push_str(&format!(" fit={}", object_fit_name(*object_fit)));
                }
                if *object_position != ObjectPosition::default() {
                    out.push_str(&format!(
                        " pos={} {}",
                        position_component_name(object_position.x),
                        position_component_name(object_position.y),
                    ));
                }
                out.push('\n');
            }
            DisplayCommand::LazyImageSlot { rect, node_id, src, .. } => {
                out.push_str(&format!(
                    "LazyImageSlot ({:.2}, {:.2}, {:.2}, {:.2}) nid={node_id} src={src:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::DrawBackgroundImage { rect, src, size, position, repeat, .. } => {
                out.push_str(&format!(
                    "DrawBackgroundImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?} size={size:?} pos=({:?},{:?}) repeat={repeat:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                    position.x, position.y,
                ));
            }
            DisplayCommand::DrawLinearGradient { rect, angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "DrawLinearGradient ({:.2}, {:.2}, {:.2}, {:.2}) angle={angle_deg:.1}deg stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::DrawRadialGradient {
                rect, center_x_pct, center_y_pct, radius_x, radius_y, stops, repeating,
            } => {
                out.push_str(&format!(
                    "DrawRadialGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({center_x_pct:.2},{center_y_pct:.2}) radii=({radius_x:.2},{radius_y:.2}) stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::DrawConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "DrawConicGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({center_x_pct:.2},{center_y_pct:.2}) from={from_angle_deg:.1}deg stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::PushClipRect { rect } => {
                out.push_str(&format!(
                    "PushClipRect ({:.2}, {:.2}, {:.2}, {:.2})\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushClipRoundedRect { rect, radii } => {
                out.push_str(&format!(
                    "PushClipRoundedRect ({:.2}, {:.2}, {:.2}, {:.2}) radii=[{:.2}, {:.2}, {:.2}, {:.2}]\n",
                    rect.x, rect.y, rect.width, rect.height,
                    radii[0], radii[1], radii[2], radii[3],
                ));
            }
            DisplayCommand::PushClipPath { shape } => {
                match shape {
                    ResolvedClipShape::Circle { cx, cy, r } => {
                        out.push_str(&format!(
                            "PushClipPath circle({cx:.2}, {cy:.2}, r={r:.2})\n"
                        ));
                    }
                    ResolvedClipShape::Ellipse { cx, cy, rx, ry } => {
                        out.push_str(&format!(
                            "PushClipPath ellipse({cx:.2}, {cy:.2}, rx={rx:.2}, ry={ry:.2})\n"
                        ));
                    }
                    ResolvedClipShape::Polygon { verts, even_odd } => {
                        out.push_str(if *even_odd {
                            "PushClipPath polygon evenodd("
                        } else {
                            "PushClipPath polygon("
                        });
                        for (i, (x, y)) in verts.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            out.push_str(&format!("{x:.2} {y:.2}"));
                        }
                        out.push_str(")\n");
                    }
                }
            }
            DisplayCommand::PopClip => {
                out.push_str("PopClip\n");
            }
            DisplayCommand::PushOpacity { alpha, .. } => {
                out.push_str(&format!("PushOpacity {alpha:.3}\n"));
            }
            DisplayCommand::PopOpacity => {
                out.push_str("PopOpacity\n");
            }
            DisplayCommand::PushBlendMode { mode, bounds } => {
                out.push_str(&format!(
                    "PushBlendMode {} bounds=({:.0},{:.0},{:.0},{:.0})\n",
                    blend_mode_name(*mode), bounds.x, bounds.y, bounds.width, bounds.height,
                ));
            }
            DisplayCommand::PopBlendMode => {
                out.push_str("PopBlendMode\n");
            }
            DisplayCommand::DrawLayerSnapshot { id, rect, alpha } => {
                out.push_str(&format!(
                    "DrawLayerSnapshot id={id} ({:.2}, {:.2}, {:.2}, {:.2}) alpha={alpha:.3}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushTransform { matrix } => {
                // 2D affine: x'=a·x+c·y+e, y'=b·x+d·y+f. Печатаем 6 значимых
                // компонент в snapshot-friendly формате — детерминированный
                // обход, не зависящий от Z/W-колонок (Phase 0 — 2D).
                let [a, b, c, d, e, f] = crate::matrix_util::mat4_to_2d_affine(matrix);
                out.push_str(&format!(
                    "PushTransform [{a:.3} {b:.3} {c:.3} {d:.3} {e:.3} {f:.3}]\n"
                ));
            }
            DisplayCommand::PopTransform => {
                out.push_str("PopTransform\n");
            }
            DisplayCommand::PushFilter { filters, bounds } => {
                let names: Vec<&str> = filters.iter().map(filter_fn_name).collect();
                let bounds_str = bounds
                    .map(|b| format!(" bounds=({:.0},{:.0},{:.0},{:.0})", b.x, b.y, b.width, b.height))
                    .unwrap_or_default();
                out.push_str(&format!("PushFilter [{}]{}\n", names.join(", "), bounds_str));
            }
            DisplayCommand::PopFilter => {
                out.push_str("PopFilter\n");
            }
            DisplayCommand::PushBackdropFilter { filters, bounds } => {
                let names: Vec<&str> = filters.iter().map(filter_fn_name).collect();
                out.push_str(&format!(
                    "PushBackdropFilter [{fns}] bounds=({x:.0},{y:.0},{w:.0},{h:.0})\n",
                    fns = names.join(", "),
                    x = bounds.x, y = bounds.y, w = bounds.width, h = bounds.height,
                ));
            }
            DisplayCommand::PopBackdropFilter => {
                out.push_str("PopBackdropFilter\n");
            }
            DisplayCommand::BeginStickyLayer { flow_rect, top, bottom, left, right } => {
                out.push_str(&format!(
                    "BeginStickyLayer flow=({:.0},{:.0},{:.0},{:.0}) top={} bottom={} left={} right={}\n",
                    flow_rect.x, flow_rect.y, flow_rect.width, flow_rect.height,
                    top.map_or("auto".to_string(), |v| format!("{v:.0}")),
                    bottom.map_or("auto".to_string(), |v| format!("{v:.0}")),
                    left.map_or("auto".to_string(), |v| format!("{v:.0}")),
                    right.map_or("auto".to_string(), |v| format!("{v:.0}")),
                ));
            }
            DisplayCommand::EndStickyLayer => {
                out.push_str("EndStickyLayer\n");
            }
            DisplayCommand::BeginFixedLayer => {
                out.push_str("BeginFixedLayer\n");
            }
            DisplayCommand::EndFixedLayer => {
                out.push_str("EndFixedLayer\n");
            }
            DisplayCommand::PushScrollLayer { clip_rect, scroll_x, scroll_y } => {
                out.push_str(&format!(
                    "PushScrollLayer clip=({:.2},{:.2},{:.2},{:.2}) scroll=({:.2},{:.2})\n",
                    clip_rect.x, clip_rect.y, clip_rect.width, clip_rect.height, scroll_x, scroll_y,
                ));
            }
            DisplayCommand::PopScrollLayer => {
                out.push_str("PopScrollLayer\n");
            }
            DisplayCommand::PushMaskImage { rect, src, size, repeat, .. } => {
                out.push_str(&format!(
                    "PushMaskImage ({:.2}, {:.2}, {:.2}, {:.2}) src={src:?} size={size:?} repeat={repeat:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PushMaskLinearGradient { rect, angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "PushMaskLinearGradient ({:.2}, {:.2}, {:.2}, {:.2}) angle={angle_deg:.1} stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, stops.len(),
                ));
            }
            DisplayCommand::PushMaskRadialGradient { rect, center_x_pct, center_y_pct, stops, repeating } => {
                out.push_str(&format!(
                    "PushMaskRadialGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({:.2},{:.2}) stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, center_x_pct, center_y_pct, stops.len(),
                ));
            }
            DisplayCommand::PushMaskConicGradient { rect, center_x_pct, center_y_pct, from_angle_deg, stops, repeating } => {
                out.push_str(&format!(
                    "PushMaskConicGradient ({:.2}, {:.2}, {:.2}, {:.2}) center=({:.2},{:.2}) from={from_angle_deg:.1}deg stops={} repeating={repeating}\n",
                    rect.x, rect.y, rect.width, rect.height, center_x_pct, center_y_pct, stops.len(),
                ));
            }
            DisplayCommand::PopMask => {
                out.push_str("PopMask\n");
            }
            DisplayCommand::PushMaskLayer { rect, mode } => {
                out.push_str(&format!(
                    "PushMaskLayer ({:.2}, {:.2}, {:.2}, {:.2}) mode={mode:?}\n",
                    rect.x, rect.y, rect.width, rect.height,
                ));
            }
            DisplayCommand::PopMaskLayer => {
                out.push_str("PopMaskLayer\n");
            }
            DisplayCommand::DrawSvgPath { vertices, color } => {
                out.push_str(&format!(
                    "DrawSvgPath tris={} #{:02x}{:02x}{:02x}{:02x}\n",
                    vertices.len() / 3,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::DrawSvgFill { contours, color } => {
                let pts: usize = contours.iter().map(std::vec::Vec::len).sum();
                out.push_str(&format!(
                    "DrawSvgFill contours={} pts={} #{:02x}{:02x}{:02x}{:02x}\n",
                    contours.len(),
                    pts,
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::DrawSvgStroke { contours, color, params } => {
                let pts: usize = contours.iter().map(std::vec::Vec::len).sum();
                out.push_str(&format!(
                    "DrawSvgStroke contours={} pts={} w={:.2} dash={} #{:02x}{:02x}{:02x}{:02x}\n",
                    contours.len(),
                    pts,
                    params.half_width * 2.0,
                    params.dasharray.len(),
                    color.r, color.g, color.b, color.a,
                ));
            }
            DisplayCommand::BoxModelOverlay { margin, border, padding, content } => {
                out.push_str(&format!(
                    "BoxModelOverlay margin=({:.0},{:.0},{:.0},{:.0}) border=({:.0},{:.0},{:.0},{:.0}) padding=({:.0},{:.0},{:.0},{:.0}) content=({:.0},{:.0},{:.0},{:.0})\n",
                    margin.x, margin.y, margin.width, margin.height,
                    border.x, border.y, border.width, border.height,
                    padding.x, padding.y, padding.width, padding.height,
                    content.x, content.y, content.width, content.height,
                ));
            }
            DisplayCommand::DrawScrollbar { track_rect, thumb_rect, vertical, .. } => {
                out.push_str(&format!(
                    "DrawScrollbar {} track=({:.0},{:.0},{:.0},{:.0}) thumb=({:.0},{:.0},{:.0},{:.0})\n",
                    if *vertical { "vertical" } else { "horizontal" },
                    track_rect.x, track_rect.y, track_rect.width, track_rect.height,
                    thumb_rect.x, thumb_rect.y, thumb_rect.width, thumb_rect.height,
                ));
            }
            DisplayCommand::PageBreak => {
                out.push_str("PageBreak\n");
            }
            DisplayCommand::DrawCrossFade { dest, src_a, src_b, progress } => {
                out.push_str(&format!(
                    "DrawCrossFade ({:.2}, {:.2}, {:.2}, {:.2}) a={src_a:?} b={src_b:?} p={progress:.3}\n",
                    dest.x, dest.y, dest.width, dest.height,
                ));
            }
        }
    }
    out
}

fn filter_fn_name(f: &FilterFn) -> &'static str {
    match f {
        FilterFn::Blur(_) => "blur",
        FilterFn::Brightness(_) => "brightness",
        FilterFn::Contrast(_) => "contrast",
        FilterFn::Grayscale(_) => "grayscale",
        FilterFn::HueRotate(_) => "hue-rotate",
        FilterFn::Invert(_) => "invert",
        FilterFn::Opacity(_) => "opacity",
        FilterFn::Saturate(_) => "saturate",
        FilterFn::Sepia(_) => "sepia",
    }
}

fn outline_style_name(s: OutlineStyle) -> &'static str {
    match s {
        OutlineStyle::None => "none",
        OutlineStyle::Auto => "auto",
        OutlineStyle::Solid => "solid",
        OutlineStyle::Dashed => "dashed",
        OutlineStyle::Dotted => "dotted",
    }
}

fn blend_mode_name(m: BlendMode) -> &'static str {
    match m {
        BlendMode::Normal => "normal",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
        BlendMode::Hue => "hue",
        BlendMode::Saturation => "saturation",
        BlendMode::Color => "color",
        BlendMode::Luminosity => "luminosity",
        BlendMode::PlusLighter => "plus-lighter",
    }
}

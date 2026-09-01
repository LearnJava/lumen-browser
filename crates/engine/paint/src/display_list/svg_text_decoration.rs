//! P1/SPLIT-DL8: SVG-эмиссия (`emit_svg_shape`/`emit_svg_text`) +
//! text-decoration-эмиссия (skip-ink/wavy/dotted/dashed) + анимированный
//! обходчик `walk_with_anim` (compositor opacity/transform override).
//! Вынесено из `display_list.rs` (`docs/tasks/p1-monolith-split-queue.md`
//! §4, группа DL, батч DL-8).

use super::*;

/// Applies `opacity` (0..1) to the alpha channel of a `Color`.
fn apply_opacity_to_color(color: Color, opacity: f32) -> Color {
    Color { r: color.r, g: color.g, b: color.b, a: (color.a as f32 * opacity).round() as u8 }
}

/// LIB-5 — applies `fill-opacity`/`stroke-opacity` to every stop of a
/// resolved SVG gradient (mirrors `apply_opacity_to_color`, but for the
/// per-stop alpha `DrawLinearGradient`/`DrawRadialGradient` carry instead of
/// one flat color).
fn apply_opacity_to_stops(stops: &[GradientStop], opacity: f32) -> Vec<GradientStop> {
    stops.iter().map(|s| GradientStop { color: apply_opacity_to_color(s.color, opacity), ..s.clone() }).collect()
}

/// LIB-5 — bounding box of a set of already-flattened path contours, in
/// whatever coordinate space they're already in (document px in the
/// non-CTM fast path, local user space under a `PushTransform` CTM — see
/// `emit_svg_shape`'s `geom`/`needs_ctm`). Layout cannot give paint a path's
/// bbox (`svg_shape_bbox` returns `Rect::ZERO` for `SvgShapeKind::Path` —
/// full `d` parsing is deferred to paint), so an `objectBoundingBox` gradient
/// on a `<path>` resolves against this instead of `geom`.
fn contours_bbox(contours: &[Vec<[f32; 2]>]) -> Rect {
    let (mut min_x, mut min_y, mut max_x, mut max_y) =
        (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for c in contours {
        for &[x, y] in c {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if min_x.is_finite() { Rect::new(min_x, min_y, max_x - min_x, max_y - min_y) } else { Rect::ZERO }
}

/// LIB-5 — maps one `SvgGradientDef` coordinate (a fraction for
/// `ObjectBoundingBox`, a user unit for `UserSpaceOnUse`) into the same
/// coordinate space `geom` is already in.
///
/// `ObjectBoundingBox` is always relative to `geom` regardless of CTM state
/// (it's defined relative to the shape's own bbox, which is exactly what
/// `geom` is). `UserSpaceOnUse` needs the split `geom` itself uses: under a
/// `PushTransform` CTM (`needs_ctm`), raw user coordinates are already in the
/// space that transform maps — pass them through unchanged; otherwise apply
/// `xmat` (the shape's full document-space paint matrix, `svg_paint_matrix`)
/// by hand, since no `PushTransform` is active to do it.
fn svg_gradient_point(
    units: SvgGradientUnits,
    x: f32,
    y: f32,
    geom: Rect,
    xmat: [f32; 6],
    needs_ctm: bool,
) -> (f32, f32) {
    match units {
        SvgGradientUnits::ObjectBoundingBox => (geom.x + x * geom.width, geom.y + y * geom.height),
        SvgGradientUnits::UserSpaceOnUse if needs_ctm => (x, y),
        SvgGradientUnits::UserSpaceOnUse => {
            let [a, b, c, d, e, f] = xmat;
            (a * x + c * y + e, b * x + d * y + f)
        }
    }
}

/// LIB-5 — resolves an `SvgGradientDef` (already opacity-adjusted stops) to
/// the `DrawLinearGradient`/`DrawRadialGradient` display command that paints
/// it, positioned against `geom` (see [`svg_gradient_point`]).
///
/// The `DrawLinearGradient` this reuses is CSS's — it always draws its line
/// through the *center* of `rect` at `angle_deg`. An SVG gradient vector
/// that isn't itself center-symmetric about the shape's bbox (an
/// off-center `x1`/`y1`/`x2`/`y2`) is therefore approximated by the
/// through-center line at the same angle — exact for the common symmetric
/// case (SVG's own default `0% 0% 100% 0%` included), approximate otherwise.
fn svg_gradient_command(
    def: &SvgGradientDef,
    stops: Vec<GradientStop>,
    geom: Rect,
    xmat: [f32; 6],
    needs_ctm: bool,
) -> DisplayCommand {
    match def {
        SvgGradientDef::Linear { x1, y1, x2, y2, units, .. } => {
            let (px1, py1) = svg_gradient_point(*units, *x1, *y1, geom, xmat, needs_ctm);
            let (px2, py2) = svg_gradient_point(*units, *x2, *y2, geom, xmat, needs_ctm);
            // atan2(dx, -dy): CSS angle convention (0° = up, clockwise) from
            // the SVG vector's (dx, dy) in standard down-is-positive-y space.
            let angle_deg = (px2 - px1).atan2(py1 - py2).to_degrees();
            DisplayCommand::DrawLinearGradient { rect: geom, angle_deg, stops, repeating: false }
        }
        SvgGradientDef::Radial { cx, cy, r, units, .. } => {
            let (pcx, pcy) = svg_gradient_point(*units, *cx, *cy, geom, xmat, needs_ctm);
            let center_x_pct = if geom.width > 0.0 { (pcx - geom.x) / geom.width } else { 0.5 };
            let center_y_pct = if geom.height > 0.0 { (pcy - geom.y) / geom.height } else { 0.5 };
            let radius_px = match units {
                // SVG L1 §7.10 objectBoundingBox diagonal formula (same one
                // `clip_path_to_shape`'s `ClipPath::Circle` arm uses for CSS
                // `circle()`'s percentage radius, CSS Shapes L1 §5).
                SvgGradientUnits::ObjectBoundingBox => {
                    r * ((geom.width * geom.width + geom.height * geom.height) * 0.5).sqrt()
                }
                SvgGradientUnits::UserSpaceOnUse if needs_ctm => *r,
                SvgGradientUnits::UserSpaceOnUse => r * ((xmat[0].abs() + xmat[3].abs()) * 0.5),
            };
            DisplayCommand::DrawRadialGradient {
                rect: geom,
                center_x_pct,
                center_y_pct,
                radius_x: radius_px.max(0.01),
                radius_y: radius_px.max(0.01),
                stops,
                repeating: false,
            }
        }
    }
}

/// LIB-5 — emits a gradient-filled shape: clip to `clip_push`'s shape, draw
/// the gradient across `geom`, release the clip. Reused by every fill arm
/// (`Rect`/`Circle`/`Ellipse`/`Path`) — only the clip shape and `geom`
/// differ per shape kind.
fn emit_svg_gradient_fill(
    cmds: &mut DisplayList,
    clip_push: DisplayCommand,
    def: &SvgGradientDef,
    opacity: f32,
    geom: Rect,
    xmat: [f32; 6],
    needs_ctm: bool,
) {
    let stops = apply_opacity_to_stops(def.stops(), opacity);
    cmds.push(clip_push);
    cmds.push(svg_gradient_command(def, stops, geom, xmat, needs_ctm));
    cmds.push(DisplayCommand::PopClip);
}

/// Emits paint commands for a single SVG shape using its pre-computed document-space rect.
/// Reads `svg_fill` / `svg_stroke` / `svg_fill_opacity` / `svg_stroke_opacity` /
/// `svg_stroke_width` from `ComputedStyle` — wired by P4 per SVG §11.2/11.3/11.4.
pub(crate) fn emit_svg_shape(b: &LayoutBox, shape: &SvgShapeKind, out: &mut DisplayList) {
    // A zero-size box bbox means "nothing to paint" for the geometry-driven shapes
    // (rect/circle/ellipse/line), whose painted extent equals `b.rect`. Paths are the
    // exception: layout cannot compute a path bbox (it requires full `d` parsing, so
    // `svg_shape_bbox` returns `Rect::ZERO`), and the path is painted from its `d`
    // segments offset by `b.rect.x/y`. Bailing here would drop every `<path>` element.
    if b.rect.width <= 0.0 && b.rect.height <= 0.0 && !matches!(shape, SvgShapeKind::Path { .. }) {
        return;
    }
    let current_color = b.style.color;
    let fill_color = b.style.svg_fill.resolve(current_color)
        .map(|c| apply_opacity_to_color(c, b.style.svg_fill_opacity));
    let stroke_color = b.style.svg_stroke.resolve(current_color)
        .map(|c| apply_opacity_to_color(c, b.style.svg_stroke_opacity));
    let stroke_w = b.style.svg_stroke_width;
    // LIB-5 — `fill: url(#gradient)` clip-and-fills with the resolved
    // gradient instead of `fill_color`'s flat color (see the `fill_color`
    // arms below). `stroke: url(#gradient)` is NOT given the same treatment
    // — a gradient-filled stroke needs clipping to the stroke's own outline
    // (caps/joins/dashes), not the fill shape's, which is materially more
    // work than reusing `PushClipPath` on the fill contour. `stroke_color`
    // above already carries a sensible fallback for it: `SvgPaint::resolve`
    // returns the gradient's first stop as a flat color.
    let fill_gradient = match &b.style.svg_fill {
        SvgPaint::Gradient(g) => Some(g.as_ref()),
        _ => None,
    };

    // CSS Fill & Stroke L3 §6 / SVG 2 §13.7 — `paint-order`. Fill and stroke
    // commands are collected separately so they can be emitted fill-first
    // (default) or stroke-first per the resolved order. Markers are not yet
    // rendered, so only fill↔stroke ordering matters.
    let mut fill_cmds: DisplayList = Vec::new();
    let mut stroke_cmds: DisplayList = Vec::new();

    // BUG-244: the shape carries its full document-space transform (set by layout in
    // `lay_out_svg_element_position`, matrix = viewport ∘ composed). When it contains
    // rotation or skew (off-diagonal b or c ≠ 0), the axis-aligned `b.rect` cannot
    // represent the result — an AABB of a rotated box collapses the rotation. So we
    // paint the shape geometry in its *user* coordinate system wrapped in a
    // `PushTransform` CTM, mirroring how a browser applies the SVG CTM at paint time.
    // Pure translate/scale (b = c = 0) keeps the existing exact `b.rect` path: no
    // transform command, no anti-aliasing change for the common case.
    let xmat: [f32; 6] = match &b.kind {
        BoxKind::SvgShape { svg_paint_matrix, .. } => svg_paint_matrix.matrix,
        _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };
    let has_rot_skew = xmat[1].abs() > 1e-6 || xmat[2].abs() > 1e-6;
    // BUG-424 (a): unlike Rect/Circle/Ellipse/Line, a `<path>`/`<polyline>`/
    // `<polygon>` (all lowered to `Path`) has no scaled `b.rect` — layout collapses
    // it to a zero-size anchor point (BUG-174: its true extent needs the parsed `d`
    // data, which layout does not have). Rect/Circle/Ellipse/Line get their scale
    // baked into `b.rect` by `apply_transform_to_bbox`, so a pure scale with no
    // rotation (`has_rot_skew=false`) already renders correctly via the fast path
    // below. `Path` does not: the common icon-sprite case (`viewBox="0 0 24 24"`
    // scaled to a 14px box, no rotation) left it painting raw `d`-space vertices
    // shifted but never scaled — ~1.7× oversized, mostly clipped away. Route it
    // through the same CTM `PushTransform` as rotate/skew whenever the matrix's
    // linear part isn't the identity scale, so the parser's raw vertices get the
    // viewBox→CSS-px scale applied like every other shape kind already gets.
    let needs_ctm = has_rot_skew
        || (matches!(shape, SvgShapeKind::Path { .. })
            && ((xmat[0] - 1.0).abs() > 1e-6 || (xmat[3] - 1.0).abs() > 1e-6));
    // `geom` is the rect the shape arms draw into; `path_shift` offsets raw `<path>`
    // `d` vertices. Under the CTM path both live in user space (shift = 0, the
    // matrix positions everything); otherwise they are the document-space `b.rect`.
    let (geom, path_shift) = if needs_ctm {
        let user_bbox = match shape {
            SvgShapeKind::Rect { x, y, width, height, .. } => Rect::new(*x, *y, *width, *height),
            SvgShapeKind::Circle { cx, cy, r } => Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r),
            SvgShapeKind::Ellipse { cx, cy, rx, ry } => Rect::new(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry),
            SvgShapeKind::Line { x1, y1, x2, y2 } =>
                Rect::new(x1.min(*x2), y1.min(*y2), (x2 - x1).abs(), (y2 - y1).abs()),
            SvgShapeKind::Path { .. } => Rect::ZERO,
        };
        (user_bbox, (0.0_f32, 0.0_f32))
    } else {
        (b.rect, (b.rect.x, b.rect.y))
    };
    if needs_ctm {
        out.push(DisplayCommand::PushTransform {
            matrix: Mat4::from_2d_affine(xmat[0], xmat[1], xmat[2], xmat[3], xmat[4], xmat[5]),
        });
    }

    match shape {
        SvgShapeKind::Rect { rx, ry, .. } => {
            let has_radius = *rx > 0.0 || *ry > 0.0;
            let r = (*rx).min(geom.width / 2.0);
            let r_y = (*ry).min(geom.height / 2.0);
            let radii = CornerRadii { tl: r, tl_y: r_y, tr: r, tr_y: r_y, br: r, br_y: r_y, bl: r, bl_y: r_y };
            if let Some(g) = fill_gradient {
                let clip = if has_radius {
                    DisplayCommand::PushClipRoundedRect { rect: geom, radii: [r, r, r, r] }
                } else {
                    DisplayCommand::PushClipRect { rect: geom }
                };
                emit_svg_gradient_fill(&mut fill_cmds, clip, g, b.style.svg_fill_opacity, geom, xmat, needs_ctm);
            } else if let Some(fc) = fill_color {
                if has_radius {
                    fill_cmds.push(DisplayCommand::FillRoundedRect { rect: geom, color: fc, radii });
                } else {
                    fill_cmds.push(DisplayCommand::FillRect { rect: geom, color: fc });
                }
            }
            if let Some(sc) = stroke_color && stroke_w > 0.0 {
                let w = stroke_w;
                // SVG 2 §13.7: the stroke is centred on the geometry edge — half its
                // width outside, half inside. `DrawBorder` paints inward from `rect`,
                // so inflate the box by w/2 on every side (outer edge moves out by w/2)
                // and grow the outer radii by w/2; the even-odd ring (BUG-175) puts the
                // inner edge at r − w/2, leaving the centre-line on the original edge.
                // Square corners (no radius) stay square. Fill keeps the original rect.
                let half = w * 0.5;
                let stroke_rect = Rect::new(
                    geom.x - half,
                    geom.y - half,
                    geom.width + w,
                    geom.height + w,
                );
                let (orx, ory) = if has_radius { (r + half, r_y + half) } else { (0.0, 0.0) };
                let stroke_radii = CornerRadii {
                    tl: orx, tl_y: ory, tr: orx, tr_y: ory,
                    br: orx, br_y: ory, bl: orx, bl_y: ory,
                };
                stroke_cmds.push(DisplayCommand::DrawBorder {
                    rect: stroke_rect,
                    widths: [w, w, w, w],
                    colors: [sc, sc, sc, sc],
                    styles: [BorderStyle::Solid; 4],
                    radii: stroke_radii,
                });
            }
        }
        SvgShapeKind::Circle { .. } | SvgShapeKind::Ellipse { .. } => {
            let rx_px = geom.width / 2.0;
            let ry_px = geom.height / 2.0;
            let radii = CornerRadii { tl: rx_px, tl_y: ry_px, tr: rx_px, tr_y: ry_px, br: rx_px, br_y: ry_px, bl: rx_px, bl_y: ry_px };
            if let Some(g) = fill_gradient {
                let clip = DisplayCommand::PushClipPath {
                    shape: ResolvedClipShape::Ellipse {
                        cx: geom.x + rx_px,
                        cy: geom.y + ry_px,
                        rx: rx_px,
                        ry: ry_px,
                    },
                };
                emit_svg_gradient_fill(&mut fill_cmds, clip, g, b.style.svg_fill_opacity, geom, xmat, needs_ctm);
            } else if let Some(fc) = fill_color {
                fill_cmds.push(DisplayCommand::FillRoundedRect { rect: geom, color: fc, radii });
            }
            if let Some(sc) = stroke_color && stroke_w > 0.0 {
                let w = stroke_w;
                // SVG 2 §13.7: stroke centred on the geometry edge (see Rect arm).
                // Inflate the bbox by w/2 so the outer edge of the inward-painted
                // border lands w/2 outside the geometry; the outer radii grow to
                // match (= half the inflated box → still a full ellipse).
                let half = w * 0.5;
                let stroke_rect = Rect::new(
                    geom.x - half,
                    geom.y - half,
                    geom.width + w,
                    geom.height + w,
                );
                let orx = rx_px + half;
                let ory = ry_px + half;
                let stroke_radii = CornerRadii {
                    tl: orx, tl_y: ory, tr: orx, tr_y: ory,
                    br: orx, br_y: ory, bl: orx, bl_y: ory,
                };
                stroke_cmds.push(DisplayCommand::DrawBorder {
                    rect: stroke_rect,
                    widths: [w, w, w, w],
                    colors: [sc, sc, sc, sc],
                    styles: [BorderStyle::Solid; 4],
                    radii: stroke_radii,
                });
            }
        }
        SvgShapeKind::Line { x1, y1, x2, y2 } => {
            // SVG <line> (§9.5): a stroked segment between (x1,y1) and (x2,y2). It
            // has no fill — only the stroke paints. The old code filled `b.rect`,
            // which for a diagonal line is the whole (large) bounding box, painting
            // a solid rectangle instead of a thin diagonal (BUG-189). `geom` is the
            // segment's bbox (doc-space for the axis-aligned fast path, user-space
            // under a rotate/skew CTM — BUG-244); the signs of the user-space
            // endpoints tell us which diagonal of that box the segment runs along.
            // Under a CTM the matrix carries any rotation; the axis-aligned path
            // still approximates a transformed line by its bbox diagonal, the same
            // assumption rect/ellipse strokes make.
            if let Some(sc) = stroke_color
                && stroke_w > 0.0
            {
                let ax = if x1 <= x2 { geom.x } else { geom.x + geom.width };
                let ay = if y1 <= y2 { geom.y } else { geom.y + geom.height };
                let bx = if x1 <= x2 { geom.x + geom.width } else { geom.x };
                let by = if y1 <= y2 { geom.y + geom.height } else { geom.y };
                let mut v: Vec<[f32; 2]> = Vec::with_capacity(6);
                push_thick_segment(&mut v, [ax, ay], [bx, by], stroke_w * 0.5);
                // paint-order is irrelevant (single component), so emit directly.
                out.push(DisplayCommand::DrawSvgPath { vertices: v, color: sc });
            }
        }
        SvgShapeKind::Path { d } => {
            let need_fill   = fill_color.is_some() || fill_gradient.is_some();
            let need_stroke = stroke_color.is_some() && stroke_w > 0.0;
            if need_fill || need_stroke {
                let segs = crate::svg_path::parse_svg_path(d);
                let contours = crate::svg_path::flatten_path(&segs, 0.5);
                if let Some(g) = fill_gradient {
                    // LIB-5: clip to the path's own outline, then draw the
                    // gradient across it — same technique as Rect/Ellipse
                    // above, but the clip shape is an arbitrary polygon
                    // (`ResolvedClipShape::Polygon` takes exactly one
                    // contour) rather than a primitive. A path with more
                    // than one sub-path (`M…Z M…Z`, e.g. a letter with a
                    // hole) clips to only the FIRST — a documented gap, not
                    // a silent one (LIB-5 follow-up: multi-contour clip).
                    let shifted: Vec<Vec<[f32; 2]>> = contours
                        .iter()
                        .filter(|c| c.len() >= 2)
                        .map(|c| {
                            c.iter()
                                .map(|[x, y]| [x + path_shift.0, y + path_shift.1])
                                .collect()
                        })
                        .collect();
                    if let Some(outer) = shifted.first() {
                        let bbox = contours_bbox(&shifted);
                        let clip = DisplayCommand::PushClipPath {
                            shape: ResolvedClipShape::Polygon {
                                verts: outer.iter().map(|[x, y]| (*x, *y)).collect(),
                                even_odd: matches!(b.style.svg_fill_rule, FillRule::EvenOdd),
                            },
                        };
                        emit_svg_gradient_fill(&mut fill_cmds, clip, g, b.style.svg_fill_opacity, bbox, xmat, needs_ctm);
                    }
                } else if let Some(fc) = fill_color {
                    match b.style.svg_fill_rule {
                        // BUG-247 / BUG-173: nonzero fills are emitted as raw
                        // outline contours (`DrawSvgFill`), not a triangle soup.
                        // femtovg/tiny_skia then fill them natively so AA lands
                        // only on the true boundary — a triangle soup made both
                        // rasterisers fringe every internal shared edge (~1px
                        // seams across the fill). wgpu tessellates the same
                        // contours, so its output is unchanged.
                        FillRule::NonZero => {
                            let shifted: Vec<Vec<[f32; 2]>> = contours
                                .iter()
                                .filter(|c| c.len() >= 2)
                                .map(|c| {
                                    c.iter()
                                        .map(|[x, y]| [x + path_shift.0, y + path_shift.1])
                                        .collect()
                                })
                                .collect();
                            if !shifted.is_empty() {
                                fill_cmds.push(DisplayCommand::DrawSvgFill {
                                    contours: shifted,
                                    color: fc,
                                });
                            }
                        }
                        // BUG-245: `fill-rule: evenodd` stays on the scanline
                        // trapezoid decomposition (self-intersecting stars +
                        // concentric rings); femtovg/wgpu have no even-odd
                        // path-fill mode, so it cannot be routed to DrawSvgFill.
                        FillRule::EvenOdd => {
                            let vertices = crate::svg_path::tessellate_fill_even_odd(&contours);
                            if !vertices.is_empty() {
                                let shifted: Vec<[f32; 2]> = vertices
                                    .iter()
                                    .map(|[x, y]| [x + path_shift.0, y + path_shift.1])
                                    .collect();
                                fill_cmds.push(DisplayCommand::DrawSvgPath {
                                    vertices: shifted,
                                    color: fc,
                                });
                            }
                        }
                    }
                }
                if let Some(sc) = stroke_color
                    && stroke_w > 0.0
                {
                    let stroke_params = crate::svg_path::StrokeParams {
                        half_width: stroke_w * 0.5,
                        linecap: match b.style.svg_stroke_linecap {
                            StrokeLinecap::Butt   => crate::svg_path::StrokeLinecap::Butt,
                            StrokeLinecap::Round  => crate::svg_path::StrokeLinecap::Round,
                            StrokeLinecap::Square => crate::svg_path::StrokeLinecap::Square,
                        },
                        linejoin: match b.style.svg_stroke_linejoin {
                            StrokeLinejoin::Miter => crate::svg_path::StrokeLinejoin::Miter,
                            StrokeLinejoin::Round => crate::svg_path::StrokeLinejoin::Round,
                            StrokeLinejoin::Bevel => crate::svg_path::StrokeLinejoin::Bevel,
                        },
                        miterlimit: b.style.svg_stroke_miterlimit,
                        dasharray: b.style.svg_stroke_dasharray.clone(),
                        dashoffset: b.style.svg_stroke_dashoffset,
                    };
                    // BUG-247: emit the raw contours + stroke params instead of
                    // a pre-tessellated triangle soup. femtovg strokes them
                    // natively (AA only on the true boundary, no internal seams);
                    // the CPU/GPU fallbacks re-tessellate with
                    // `tessellate_stroke_ex` for bit-identical output.
                    let shifted: Vec<Vec<[f32; 2]>> = contours
                        .iter()
                        .filter(|c| c.len() >= 2)
                        .map(|c| {
                            c.iter()
                                .map(|[x, y]| [x + path_shift.0, y + path_shift.1])
                                .collect()
                        })
                        .collect();
                    if !shifted.is_empty() {
                        stroke_cmds.push(DisplayCommand::DrawSvgStroke {
                            contours: shifted,
                            color: sc,
                            params: stroke_params,
                        });
                    }
                }
            }
        }
    }

    // Emit fill and stroke in the order dictated by `paint-order`.
    if b.style.paint_order.fill_before_stroke() {
        out.append(&mut fill_cmds);
        out.append(&mut stroke_cmds);
    } else {
        out.append(&mut stroke_cmds);
        out.append(&mut fill_cmds);
    }

    // Close the CTM opened above (BUG-244 rotate/skew, BUG-424 (a) Path scale).
    if needs_ctm {
        out.push(DisplayCommand::PopTransform);
    }
}

/// Emits paint commands for SVG text elements (`<text>`, `<tspan>`, `<textPath>`).
/// Draws text at the specified position with proper horizontal and vertical alignment.
/// Reads `svg_fill` / `svg_stroke` / `font-family` / `font-size` from `ComputedStyle`.
/// // CSS: text-anchor, dominant-baseline, baseline-shift
pub(crate) fn emit_svg_text(
    b: &LayoutBox,
    text: &str,
    text_anchor: SvgTextAnchor,
    dominant_baseline: SvgDominantBaseline,
    baseline_shift: SvgBaselineShift,
    out: &mut DisplayList,
) {
    if text.is_empty() {
        return;
    }

    let current_color = b.style.color;
    let fill_color = b.style.svg_fill.resolve(current_color)
        .map(|c| apply_opacity_to_color(c, b.style.svg_fill_opacity));

    let font_size = b.style.font_size;
    // Phase 1: approximate text width as 0.5 × font-size × char count (typical monospace ratio).
    // Phase 2: replace with real TextMeasurer from lumen-font when available in paint.
    let approx_text_width = font_size * 0.5 * text.chars().count() as f32;

    // Apply text-anchor: adjust x so start/middle/end of text aligns at the SVG `x` position.
    let anchor_offset_x = match text_anchor {
        SvgTextAnchor::Start => 0.0,
        SvgTextAnchor::Middle => -approx_text_width * 0.5,
        SvgTextAnchor::End => -approx_text_width,
    };

    // Apply dominant-baseline: adjust y so the specified baseline aligns at the SVG `y` position.
    // SVG y is the text baseline by default (auto/baseline). Adjustments are approximate.
    let baseline_offset_y = match dominant_baseline {
        SvgDominantBaseline::Auto | SvgDominantBaseline::Baseline => 0.0,
        // middle/central: shift up by ~half em so middle of em-box is at y
        SvgDominantBaseline::Middle | SvgDominantBaseline::Central => -font_size * 0.35,
        // hanging/text-before-edge: shift down so top of cap is at y
        SvgDominantBaseline::Hanging | SvgDominantBaseline::TextBeforeEdge => font_size * 0.2,
        // text-after-edge: shift up so descender bottom is at y
        SvgDominantBaseline::TextAfterEdge => -font_size * 0.8,
    };

    // Apply baseline-shift: an additional vertical offset on top of dominant-baseline.
    // Positive values *raise* the text (smaller y); `sub` lowers, `super` raises.
    // sub/super offsets are approximate fractions of the em (no OS/2 sub/superscript
    // metrics in the paint stage). SVG screen-y grows downward, so a raise is negative.
    let shift_offset_y = match baseline_shift {
        SvgBaselineShift::Baseline => 0.0,
        SvgBaselineShift::Sub => font_size * 0.2,
        SvgBaselineShift::Super => -font_size * 0.4,
        SvgBaselineShift::Length(px) => -px,
        SvgBaselineShift::Percentage(frac) => -frac * font_size,
    };

    if let Some(fc) = fill_color {
        let mut rect = b.rect;
        rect.x += anchor_offset_x;
        rect.y += baseline_offset_y + shift_offset_y;
        rect.width = approx_text_width;
        rect.height = font_size;
        out.push(DisplayCommand::DrawText {
            font_stretch: b.style.font_stretch,
            rect,
            text: text.to_string(),
            font_family: b.style.font_family.clone(),
            font_size,
            color: fc,
            font_weight: b.style.font_weight,
            font_style: b.style.font_style,
            font_variation_axes: vec![],
            font_features: Vec::new(),
            font_palette: None,
            tab_size: b.style.tab_size,
            highlight_name: None,
            text_orientation: if b.style.writing_mode != lumen_layout::style::WritingMode::HorizontalTb {
                Some(b.style.text_orientation)
            } else {
                None
            },
        });
    }
}

/// Эмитит FillRect-ы для активных линий text-decoration. Геометрия —
/// приблизительная: baseline ≈ line_y + font_size * 0.80 (соответствует
/// ascent ratio Inter, на котором рендерер позиционирует глифы). Толщина
/// резолвится через [`resolve_decoration_thickness`] из
/// `text-decoration-thickness` (L3 §2.3). Стиль (`Solid` / `Double` /
/// `Dotted` / `Dashed` / `Wavy`, L3 §2.2) разворачивается в один или
/// несколько FillRect-ов через [`emit_decoration_line`]. Цвет — из
/// `text-decoration-color` с fallback на currentColor (L3 §3).
pub(crate) fn push_text_decoration(out: &mut DisplayList, container_x: f32, line_y: f32, frag: &InlineFrag) {
    let decoration = frag.style.text_decoration_line;
    if decoration.is_empty() || frag.width <= 0.0 {
        return;
    }
    let fs = frag.style.font_size;
    let baseline_y = line_y + fs * 0.80;
    let thickness = resolve_decoration_thickness(frag.style.text_decoration_thickness, fs);
    let style = frag.style.text_decoration_style;
    let x = container_x + frag.x;
    let color = frag.style.text_decoration_color.resolve(frag.style.color);
    let skip_ink = frag.style.text_decoration_skip_ink;

    if decoration.underline {
        // CSS Text Decoration L4 §5.1: text-underline-position.
        // `Under` places the line below all descenders (≈ 25% of font-size below baseline).
        // `Auto`/`FromFont` uses the standard position just below the baseline.
        let base_offset = match frag.style.text_underline_position {
            TextUnderlinePosition::Under => fs * 0.25,
            _ => fs * 0.10,
        };
        // CSS Text Decoration L4 §5.3: text-underline-offset adds an explicit shift.
        let extra = frag.style.text_underline_offset.unwrap_or(0.0);
        let deco_y = baseline_y + base_offset + extra;
        // CSS Text Decoration L4 §3.5: text-decoration-skip-ink.
        // `None` — continuous line; `Auto` — skip under descenders; `All` — skip every char.
        match skip_ink {
            TextDecorationSkipInk::None => {
                emit_decoration_line(out, x, deco_y, frag.width, thickness, color, style);
            }
            TextDecorationSkipInk::Auto => {
                emit_decoration_line_skip_ink(out, SkipInkParams {
                    x, y: deco_y, width: frag.width, thickness, color, style,
                    text: &frag.text, skip_all: false,
                });
            }
            TextDecorationSkipInk::All => {
                emit_decoration_line_skip_ink(out, SkipInkParams {
                    x, y: deco_y, width: frag.width, thickness, color, style,
                    text: &frag.text, skip_all: true,
                });
            }
        }
    }
    if decoration.line_through {
        // line-through sits on the mid-ascent; skip-ink does not apply (spec §3.5).
        let y = baseline_y - fs * 0.30;
        emit_decoration_line(out, x, y, frag.width, thickness, color, style);
    }
    if decoration.overline {
        let y = baseline_y - fs * 0.78;
        // `All` skips over all glyphs including those above/below the line (spec §3.5).
        if skip_ink == TextDecorationSkipInk::All {
            emit_decoration_line_skip_ink(out, SkipInkParams {
                x, y, width: frag.width, thickness, color, style,
                text: &frag.text, skip_all: true,
            });
        } else {
            emit_decoration_line(out, x, y, frag.width, thickness, color, style);
        }
    }
}

/// Резолвит [`TextDecorationThickness`] в device-px по CSS Text Decoration
/// L3 §2.3. `Auto` / `FromFont` — UA дефолт ≈ 7% от font-size (минимум
/// 1px); Phase 0 без font-access для `FromFont`, поэтому тот же default.
/// `Length` — уже resolved-px из cascade. `Percentage` хранится как
/// fraction; spec ссылается на 1em **parent** font-size, Phase 0
/// используем frag.font_size как приближение (документировано в
/// `style.rs`).
fn resolve_decoration_thickness(value: TextDecorationThickness, font_size: f32) -> f32 {
    match value {
        TextDecorationThickness::Auto | TextDecorationThickness::FromFont => {
            (font_size * 0.07).max(1.0)
        }
        TextDecorationThickness::Length(px) => px.max(0.0),
        TextDecorationThickness::Percentage(frac) => (frac * font_size).max(0.0),
    }
}

/// Returns `true` when the character has ink below the alphabetic baseline
/// that would visually cross a standard underline (CSS Text Decoration L4 §3.5).
///
/// Phase 0: covers the most common Latin descenders. Non-Latin scripts and
/// italic `f` are not yet tracked — future work when per-glyph metrics are
/// available at paint time.
pub(crate) fn char_has_ink_descender(ch: char) -> bool {
    // ASCII descenders: g j p q y; Q and J have tails in many typefaces.
    matches!(ch, 'g' | 'j' | 'p' | 'q' | 'y' | 'Q' | 'J')
}

/// Parameters for `emit_decoration_line_skip_ink` — bundles geometry to stay
/// within the 7-argument clippy limit.
pub(crate) struct SkipInkParams<'a> {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) thickness: f32,
    pub(crate) color: Color,
    pub(crate) style: TextDecorationStyle,
    /// Fragment text used to locate descender characters.
    pub(crate) text: &'a str,
    /// `true` for `text-decoration-skip-ink: all` (skip every glyph);
    /// `false` for `auto` (skip only known descenders).
    pub(crate) skip_all: bool,
}

/// Emits a decoration line (underline or overline) that skips over glyphs
/// with ink that would cross it — CSS Text Decoration L4 §3.5
/// `text-decoration-skip-ink`.
///
/// Algorithm: divide the fragment into equal-width character cells based on
/// `width / char_count` (Phase 0 approximation — no per-glyph metrics at
/// paint time). For each cell that needs a gap, clear only the central ink
/// region (≈ 56% of the advance, centred in the cell) rather than the whole
/// cell, so the line stays visible between adjacent skipped glyphs. The
/// remaining segments between merged gaps are then drawn.
pub(crate) fn emit_decoration_line_skip_ink(out: &mut DisplayList, p: SkipInkParams<'_>) {
    let SkipInkParams { x, y, width, thickness, color, style, text, skip_all } = p;
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        emit_decoration_line(out, x, y, width, thickness, color, style);
        return;
    }

    let char_w = width / n as f32;
    // Each skipped glyph clears only the central ink region of its advance
    // cell, not the whole cell: the gap is centred in the cell and spans
    // ≈ 56% of the advance plus a small thickness-scaled clearance, capped
    // at 90% of the cell. This leaves a visible line segment between
    // adjacent glyphs even for runs of consecutive descenders (e.g. "gjpqy"),
    // matching Edge's skip-ink. The previous full-cell + margin gap merged
    // such runs into one giant gap, erasing the line entirely.
    let half_gap = (char_w * 0.28 + thickness * 0.5).min(char_w * 0.45);

    // Build merged gap intervals.
    let mut gaps: Vec<(f32, f32)> = Vec::new();
    for (i, &ch) in chars.iter().enumerate() {
        if skip_all || char_has_ink_descender(ch) {
            let center = x + (i as f32 + 0.5) * char_w;
            let gap_start = (center - half_gap).max(x);
            let gap_end = (center + half_gap).min(x + width);
            if let Some(last) = gaps.last_mut()
                && gap_start <= last.1
            {
                last.1 = last.1.max(gap_end);
                continue;
            }
            gaps.push((gap_start, gap_end));
        }
    }

    if gaps.is_empty() {
        emit_decoration_line(out, x, y, width, thickness, color, style);
        return;
    }

    // Draw segments between gaps.
    let mut seg_x = x;
    for (gap_start, gap_end) in &gaps {
        if seg_x < *gap_start {
            emit_decoration_line(out, seg_x, y, gap_start - seg_x, thickness, color, style);
        }
        seg_x = *gap_end;
    }
    if seg_x < x + width - f32::EPSILON {
        emit_decoration_line(out, seg_x, y, x + width - seg_x, thickness, color, style);
    }
}

/// Эмитит FillRect-ы для одной decoration-линии в выбранном стиле
/// (CSS Text Decoration L3 §2.2). `(x, y)` — верхний левый угол.
///
/// - `Solid` — один rect (initial).
/// - `Double` — два параллельных rect-а с gap = thickness; итого
///   span ≈ 3 × thickness, верхний у `y`, нижний у `y + 2·t`.
/// - `Dotted` — серия квадратиков `thickness × thickness`, шаг
///   `2 × thickness` (gap = thickness). Геометрия UA-defined; выбран
///   простой 1:1 паттерн.
/// - `Dashed` — серия штрихов длиной `2 × thickness`, шаг `3 × thickness`
///   (gap = thickness). UA-defined.
/// - `Wavy` — синусоидальная волна аппроксимируется серией узких
///   axis-aligned столбцов (renderer pipeline без curves): сдвиг
///   центра толщины по `dy = sin(2π · rel_x / λ) · A`, где
///   `A = WAVY_AMPLITUDE_FACTOR · thickness`, `λ =
///   WAVY_WAVELENGTH_FACTOR · thickness`. Шаг между columns =
///   `max(1, thickness · 0.5)` — компромисс между визуальной
///   гладкостью и числом FillRect-ов (≈ 2 sample / thickness CSS px).
///   Толщина каждого column = thickness, ширина = step (или остаток
///   до `x + width`). Видимый ascent/descent от baseline = `A + t/2`.
fn emit_decoration_line(
    out: &mut DisplayList,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
    style: TextDecorationStyle,
) {
    if width <= 0.0 || thickness <= 0.0 {
        return;
    }
    match style {
        TextDecorationStyle::Solid => {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y, width, thickness),
                color,
            });
        }
        TextDecorationStyle::Wavy => {
            emit_wavy_line(out, x, y, width, thickness, color);
        }
        TextDecorationStyle::Double => {
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y, width, thickness),
                color,
            });
            out.push(DisplayCommand::FillRect {
                rect: Rect::new(x, y + 2.0 * thickness, width, thickness),
                color,
            });
        }
        TextDecorationStyle::Dotted => {
            let step = thickness * 2.0;
            let end = x + width;
            let mut cx = x;
            while cx + thickness <= end + f32::EPSILON {
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(cx, y, thickness, thickness),
                    color,
                });
                cx += step;
            }
        }
        TextDecorationStyle::Dashed => {
            let dash = thickness * 2.0;
            let step = thickness * 3.0;
            let end = x + width;
            let mut cx = x;
            while cx < end {
                let w = (end - cx).min(dash);
                if w <= 0.0 {
                    break;
                }
                out.push(DisplayCommand::FillRect {
                    rect: Rect::new(cx, y, w, thickness),
                    color,
                });
                cx += step;
            }
        }
    }
}

/// Амплитуда волны в долях `thickness` — peak-deviation центра от
/// baseline в обе стороны. 1.5×thickness даёт ясно различимую волну
/// без излишнего вертикального expansion за пределы line-box-а.
const WAVY_AMPLITUDE_FACTOR: f32 = 1.5;

/// Длина волны в долях `thickness`. 4×thickness — UA-defined компромисс
/// (Chrome ≈ 3-4×, Firefox ≈ 6×; берём середину). При thickness=1px →
/// период 4px, ~3 цикла на каждые 12 CSS-px font-size.
const WAVY_WAVELENGTH_FACTOR: f32 = 4.0;

/// Аппроксимирует синусоидальную линию серией axis-aligned FillRect-ов:
/// для каждого sampled-X эмитим тонкий столбец `[x, x+step] × [cy+dy-t/2,
/// cy+dy+t/2]`, где `cy = y + t/2` — центр толщины, `dy = sin(2π·rel/λ)·A`.
/// Step выбран `max(1, t·0.5)`: ниже — растёт число FillRect (≈ 2·width/t),
/// выше — лестница становится грубее, что особенно заметно при крутых
/// склонах волны (там `|dy'| → t·A/λ·2π`).
fn emit_wavy_line(
    out: &mut DisplayList,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: Color,
) {
    let amplitude = thickness * WAVY_AMPLITUDE_FACTOR;
    let wavelength = thickness * WAVY_WAVELENGTH_FACTOR;
    let step = (thickness * 0.5).max(1.0);
    let cy = y + thickness * 0.5;
    let end = x + width;
    let mut cx = x;
    while cx < end {
        let w = step.min(end - cx);
        if w <= 0.0 {
            break;
        }
        // Используем центр столбца как sample-точку — это даёт
        // чуть более точную аппроксимацию, чем left-edge sampling.
        let sample_x = cx + w * 0.5;
        let phase = (sample_x - x) / wavelength * std::f32::consts::TAU;
        let dy = phase.sin() * amplitude;
        out.push(DisplayCommand::FillRect {
            rect: Rect::new(cx, cy + dy - thickness * 0.5, w, thickness),
            color,
        });
        cx += step;
    }
}

/// Like `walk` but applies `CompositorAnimFrame` overrides for opacity and transform.
///
/// When a node has an animated opacity or transform, the overridden values replace
/// the style values in the emitted Push* commands. All other paint (FillRect, DrawText,
/// borders, shadows) uses the base style unchanged.
pub(crate) fn walk_with_anim(b: &LayoutBox, anim: Option<&CompositorAnimFrame>, out: &mut DisplayList, dpr: f32) {
    let ov = anim.and_then(|a| a.get(b.node));

    // CSS Transforms L2 §5.1 — backface culling (same rule as `walk`).
    if is_backface_hidden(b) {
        return;
    }
    // CSS Positioning L3 §6.3 — position:sticky (same as in walk).
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
    // CSS Positioning L3 §6.1 — position:fixed (same as in walk).
    let is_fixed = matches!(b.style.position, Position::Fixed);
    if is_fixed {
        out.push(DisplayCommand::BeginFixedLayer);
    }

    // Determine effective opacity: animated override wins over style.
    let effective_opacity = ov.and_then(|o| o.opacity).unwrap_or(b.style.opacity);

    // Skip completely invisible subtrees (same rule as walk, but uses effective opacity).
    if effective_opacity == 0.0 && b.style.opacity == 0.0 {
        // Both animated and static are zero — nothing to paint.
        if !is_opacity_subtree_painted(b) {
            return;
        }
    } else if effective_opacity == 0.0 {
        // Animated to zero — skip this subtree.
        return;
    } else if !is_opacity_subtree_painted(b) && ov.and_then(|o| o.opacity).is_none() {
        // Base style opacity is 0 and no anim override — skip.
        return;
    }

    match &b.kind {
        BoxKind::Skip => {}
        BoxKind::Block => {
            let has_opacity = effective_opacity < 1.0;
            if has_opacity {
                out.push(DisplayCommand::PushOpacity { alpha: effective_opacity, bounds: Some(b.rect) });
            }

            // Determine effective transform: animated override wins over style.
            let transform = if let Some(fns) = ov.and_then(|o| o.transform.as_deref()) {
                let (ox, oy, _) = b.style.transform_origin;
                transform_fns_to_matrix(fns, b.rect.x + ox.resolve(b.rect.width), b.rect.y + oy.resolve(b.rect.height))
            } else {
                forward_box_transform(b)
            };
            if let Some(matrix) = transform {
                out.push(DisplayCommand::PushTransform { matrix });
            }

            // CSS Tables L2 §17.6.1.1 — `empty-cells: hide` suppresses an empty
            // cell's background and borders (children still walked).
            let self_visible = is_paint_visible(b) && !is_hidden_empty_cell(b);
            if self_visible {
                emit_box_shadows(b, out);
                // BUG-231: animated background-color override wins over the base value.
                let base_bg = match b.style.background_color {
                    Some(CssColor::Rgba(c)) => Some(c),
                    _ => None,
                };
                let eff_bg = ov.and_then(|o| o.background_color).or(base_bg);
                if let Some(bg) = eff_bg
                    && bg.a > 0
                {
                    let clip = background_clip_rect(b, background_color_clip(b));
                    if clip.width > 0.0 && clip.height > 0.0 {
                        out.push(DisplayCommand::FillRect { rect: clip, color: bg });
                    }
                }
                emit_inset_box_shadows(b, out);
                let s = &b.style;
                let has_border = s.border_top_style.is_visible()
                    || s.border_right_style.is_visible()
                    || s.border_bottom_style.is_visible()
                    || s.border_left_style.is_visible();
                if has_border {
                    // BUG-231: animated color override resolves currentColor.
                    let cur = ov.and_then(|o| o.color).unwrap_or(s.color);
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
            // CSS Transforms L2 §6.2 — depth-sort children of a 3D rendering
            // context (preserve-3d); else document order. Mirrors `walk`.
            if establishes_3d_rendering_context(b) {
                for i in depth_sorted_child_order(&b.children) {
                    walk_with_anim(&b.children[i], anim, out, dpr);
                }
            } else {
                for child in &b.children {
                    walk_with_anim(child, anim, out, dpr);
                }
            }
            if self_visible {
                emit_outline(b, out);
            }
            if transform.is_some() {
                out.push(DisplayCommand::PopTransform);
            }
            if has_opacity {
                out.push(DisplayCommand::PopOpacity);
            }
        }
        BoxKind::InlineBlockRow => {
            for child in &b.children {
                walk_with_anim(child, anim, out, dpr);
            }
        }
        BoxKind::InlineSpace => {}
        BoxKind::InlineRun { lines, .. } => {
            emit_inline_run(b, lines, None, dpr, out);
        }
        // Image and other kinds: no compositor-offloadable properties, delegate to walk.
        _ => {
            walk(b, out, dpr, None);
        }
    }
    if is_fixed {
        out.push(DisplayCommand::EndFixedLayer);
    }
    if is_sticky {
        out.push(DisplayCommand::EndStickyLayer);
    }
}

// BorderCollapse re-exported from lumen_layout::BorderCollapse (CSS Tables L2 §17.6).
// Use b.style.border_collapse directly — now wired by P4.


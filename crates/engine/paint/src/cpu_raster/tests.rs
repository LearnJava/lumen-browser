use super::*;

mod layers_borders_images;

/// Sample the RGBA8 pixel at `(x, y)` from a rasterized [`Image`].
fn px(img: &Image, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let i = ((y * img.width + x) * 4) as usize;
    (img.data[i], img.data[i + 1], img.data[i + 2], img.data[i + 3])
}

/// BUG-1279: an outline lies entirely outside `rect ± offset`. A 20×20 box at
/// (30,30) with `outline: 10px` covers x = 20..60, not 25..55.
#[test]
fn outline_is_drawn_outside_the_box_edge() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let white = (255, 255, 255, 255);
    let mk = |offset: f32| {
        let cmds = vec![DisplayCommand::DrawOutline {
            rect: rect(30.0, 30.0, 20.0, 20.0),
            width: 10.0,
            style: lumen_layout::style::OutlineStyle::Solid,
            color: blue,
            offset,
        }];
        rasterize_cpu(80, 80, &cmds, &[], 0.0, 0.0).expect("rasterize")
    };
    let img = mk(0.0);
    assert_eq!(px(&img, 21, 40), (0, 0, 255, 255), "outer band starts at x=20");
    assert_eq!(px(&img, 29, 40), (0, 0, 255, 255), "band reaches the box edge");
    assert_eq!(px(&img, 40, 40), white, "box interior untouched");
    assert_eq!(px(&img, 18, 40), white, "nothing beyond 10px");
    assert_eq!(px(&img, 21, 21), (0, 0, 255, 255), "mitred corner is filled");
    // Positive offset moves the whole band away from the box.
    let img = mk(5.0);
    assert_eq!(px(&img, 27, 40), white, "gap between box and outline");
    assert_eq!(px(&img, 19, 40), (0, 0, 255, 255), "band spans x=15..25");
    assert_eq!(px(&img, 16, 40), (0, 0, 255, 255));
}

/// BUG-1405: an outline whose element box lies inside the clip rect still has a stroke band
/// reaching past it — the clip must cut that band (`clip-path: margin-box` + wide outline).
#[test]
fn outline_band_is_cut_by_clip_rect_around_the_box() {
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let white = (255, 255, 255, 255);
    let cmds = vec![
        DisplayCommand::PushClipRect { rect: rect(20.0, 20.0, 40.0, 40.0) },
        DisplayCommand::DrawOutline {
            rect: rect(30.0, 30.0, 20.0, 20.0),
            width: 20.0,
            style: lumen_layout::style::OutlineStyle::Solid,
            color: green,
            offset: 0.0,
        },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(100, 100, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 25, 40), (0, 128, 0, 255), "band inside the clip is painted");
    assert_eq!(px(&img, 10, 40), white, "band beyond the clip is cut");
    assert_eq!(px(&img, 70, 40), white);
}

/// `PushScrollLayer` with a non-zero offset shifts the content up by
/// `scroll_y` inside the clip: a 100px-tall grey block above a green one
/// ends at y=90 after a 10px scroll, and nothing leaks outside the clip.
#[test]
fn scroll_layer_translates_content_inside_clip() {
    let grey = Color { r: 128, g: 128, b: 128, a: 255 };
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushScrollLayer { id: 0, clip_rect: rect(0.0, 0.0, 40.0, 60.0), scroll_x: 0.0, scroll_y: 10.0 },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 40.0, 40.0), color: grey },
        DisplayCommand::FillRect { rect: rect(0.0, 40.0, 40.0, 100.0), color: green },
        DisplayCommand::PopScrollLayer,
    ];
    let img = rasterize_cpu(64, 80, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 10, 5), (128, 128, 128, 255), "grey still covers y<30");
    assert_eq!(px(&img, 10, 25), (128, 128, 128, 255), "grey ends at 40-10=30");
    assert_eq!(px(&img, 10, 35), (0, 128, 0, 255), "green starts at 30");
    assert_eq!(px(&img, 10, 65), (255, 255, 255, 255), "clip stops at y=60");
    assert_eq!(px(&img, 50, 35), (255, 255, 255, 255), "clip stops at x=40");
}

/// BUG-1479: content that lies below the canvas before the scroll shift
/// (`y = 80…100` on a 40×60 canvas, `scrollTop = 70`) still reaches the
/// frame at `y = 10…30`; the canvas-sized layer used to cut it off.
#[test]
fn scroll_layer_shows_content_from_below_the_canvas() {
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushScrollLayer { id: 0, clip_rect: rect(0.0, 0.0, 40.0, 40.0), scroll_x: 0.0, scroll_y: 70.0 },
        DisplayCommand::FillRect { rect: rect(0.0, 80.0, 40.0, 20.0), color: green },
        DisplayCommand::PopScrollLayer,
    ];
    let img = rasterize_cpu(40, 60, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 10, 5), (255, 255, 255, 255), "above the block");
    assert_eq!(px(&img, 10, 15), (0, 128, 0, 255), "block at 80-70=10");
    assert_eq!(px(&img, 10, 35), (255, 255, 255, 255), "below the block, inside the clip");
    assert_eq!(px(&img, 10, 50), (255, 255, 255, 255), "outside the clip");
}

/// Same for a horizontal scroll: content right of the canvas comes into view.
#[test]
fn scroll_layer_shows_content_from_right_of_the_canvas() {
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushScrollLayer { id: 0, clip_rect: rect(0.0, 0.0, 40.0, 40.0), scroll_x: 90.0, scroll_y: 0.0 },
        DisplayCommand::FillRect { rect: rect(100.0, 0.0, 20.0, 40.0), color: green },
        DisplayCommand::PopScrollLayer,
    ];
    let img = rasterize_cpu(50, 40, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 5, 20), (255, 255, 255, 255));
    assert_eq!(px(&img, 15, 20), (0, 128, 0, 255), "block at 100-90=10");
    assert_eq!(px(&img, 35, 20), (255, 255, 255, 255));
}

/// With a zero offset the scroll layer is a plain clip (no extra layer).
#[test]
fn scroll_layer_at_origin_is_plain_clip() {
    let grey = Color { r: 128, g: 128, b: 128, a: 255 };
    let cmds = vec![
        DisplayCommand::PushScrollLayer { id: 0, clip_rect: rect(0.0, 0.0, 20.0, 20.0), scroll_x: 0.0, scroll_y: 0.0 },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 40.0, 40.0), color: grey },
        DisplayCommand::PopScrollLayer,
    ];
    let img = rasterize_cpu(40, 40, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 10, 10), (128, 128, 128, 255));
    assert_eq!(px(&img, 30, 30), (255, 255, 255, 255));
}

/// BUG-1427 — a one-stop gradient reaches the backend as two equal stops
/// (`linear-gradient(green)`); it must paint solid, also when repeating
/// (zero period) and for the radial shape.
#[test]
fn single_colour_gradients_paint_solid() {
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let stop = |position| GradientStop {
        color: green,
        color_space: lumen_layout::ColorSpace::Srgb,
        position,
    };
    for (stops, repeating) in [
        (vec![stop(None), stop(None)], false),
        (vec![stop(Some(lumen_layout::Length::Percent(50.0))), stop(Some(lumen_layout::Length::Percent(50.0)))], false),
        (vec![stop(Some(lumen_layout::Length::Px(50.0))), stop(Some(lumen_layout::Length::Px(50.0)))], true),
    ] {
        let cmds = vec![
            DisplayCommand::DrawLinearGradient {
                rect: rect(0.0, 0.0, 40.0, 10.0),
                angle_deg: 180.0,
                stops: stops.clone(),
                repeating,
            },
            DisplayCommand::DrawRadialGradient {
                rect: rect(0.0, 20.0, 40.0, 10.0),
                center_x_pct: 0.5,
                center_y_pct: 0.5,
                radius_x: 30.0,
                radius_y: 30.0,
                stops,
                repeating,
            },
        ];
        let img = rasterize_cpu(40, 40, &cmds, &[], 0.0, 0.0).expect("rasterize");
        for (x, y) in [(5, 5), (35, 8), (5, 25), (35, 28)] {
            assert_eq!(px(&img, x, y), (0, 128, 0, 255), "({x},{y}) repeating={repeating}");
        }
    }
}

/// `DrawSvgPath` fills the tessellated triangle interior with the solid
/// colour and leaves the background white outside it.
#[test]
fn svg_path_fills_triangle_interior() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    // One triangle with a large axis-aligned interior so the centroid sample
    // is unambiguously inside (avoids antialiased edge pixels).
    let cmds = vec![DisplayCommand::DrawSvgPath {
        vertices: vec![[10.0, 10.0], [50.0, 10.0], [30.0, 50.0]],
        color: red,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Centroid ≈ (30, 23): solidly inside the triangle.
    assert_eq!(px(&img, 30, 23), (255, 0, 0, 255), "interior should be red");
    // Far corner outside the triangle stays white.
    assert_eq!(px(&img, 1, 1), (255, 255, 255, 255), "exterior stays white");
}

/// BUG-247: `DrawSvgStroke` on the CPU rasteriser re-tessellates the stroke
/// contours with `tessellate_stroke_ex` and fills the identical triangle
/// soup the emitter used to bake into a `DrawSvgPath` — so CPU snapshots stay
/// byte-for-byte identical while the femtovg window gets native strokes.
#[test]
fn svg_stroke_cpu_matches_tessellated_path() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let contours = vec![vec![[10.0, 10.0], [54.0, 10.0], [54.0, 54.0], [10.0, 54.0], [10.0, 10.0]]];
    let params = crate::svg_path::StrokeParams {
        half_width: 3.0,
        linecap: crate::svg_path::StrokeLinecap::Butt,
        linejoin: crate::svg_path::StrokeLinejoin::Miter,
        miterlimit: 4.0,
        dasharray: vec![6.0, 4.0],
        dashoffset: 0.0,
    };
    let stroke_cmd = vec![DisplayCommand::DrawSvgStroke {
        contours: contours.clone(),
        color: blue,
        params: params.clone(),
    }];
    let path_cmd = vec![DisplayCommand::DrawSvgPath {
        vertices: crate::svg_path::tessellate_stroke_ex(&contours, &params),
        color: blue,
    }];
    let a = rasterize_cpu(64, 64, &stroke_cmd, &[], 0.0, 0.0).expect("stroke");
    let b = rasterize_cpu(64, 64, &path_cmd, &[], 0.0, 0.0).expect("path");
    assert_eq!(a.data, b.data, "CPU DrawSvgStroke must be byte-identical to the old triangle soup");
}

fn stroke_cmd(text: &str, width: f32, color: Color) -> DisplayCommand {
    stroke_cmd_sized(text, 32.0, width, color)
}

fn stroke_cmd_sized(text: &str, font_size: f32, width: f32, color: Color) -> DisplayCommand {
    DisplayCommand::DrawTextStroke {
        rect: Rect::new(10.0 - width / 2.0, 10.0 - width / 2.0, 100.0 + width, 40.0 + width),
        text: text.into(),
        font_size,
        font_family: Vec::new(),
        font_weight: lumen_layout::FontWeight::new(400),
        font_style: lumen_layout::FontStyle::Normal,
        font_features: Vec::new(),
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        width,
        color,
    }
}

/// `-webkit-text-stroke`: `expand` отдаёт контуры глифов как `DrawSvgStroke` с
/// половиной ширины в `half_width`; пробел контуров не имеет.
#[test]
fn text_stroke_expands_to_glyph_contours() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let Some(DisplayCommand::DrawSvgStroke { contours, params, color }) =
        text_stroke::expand(&stroke_cmd("H", 4.0, blue), None)
    else {
        panic!("DrawTextStroke must expand to DrawSvgStroke");
    };
    assert_eq!(color, blue);
    assert!((params.half_width - 2.0).abs() < 1e-6);
    // «H» — один замкнутый контур.
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0].first(), contours[0].last());
    assert!(text_stroke::expand(&stroke_cmd(" ", 4.0, blue), None).is_none());
    assert!(text_stroke::expand(&stroke_cmd("", 4.0, blue), None).is_none());
}

/// Пиксели обводки: у «O» на средней строке четыре штриха (оба края обоих
/// стволов), между ними — фон: заливка не рисуется.
#[test]
fn text_stroke_paints_outline_not_fill() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let img = rasterize_cpu(160, 128, &[stroke_cmd_sized("O", 96.0, 2.0, blue)], &[], 0.0, 0.0).expect("raster");
    let is_ink = |x: u32, y: u32| px(&img, x, y) == (0, 0, 255, 255);
    let ink = (0..128u32).flat_map(|y| (0..160u32).map(move |x| (x, y))).filter(|&(x, y)| is_ink(x, y)).count();
    assert!(ink > 200, "обводка нарисована: {ink} px");
    // Строка по высоте середины «O»: baseline ≈ 10 + 96·0.97, x-height-середина выше на ~35.
    let y = (10.0 + 96.0 * 0.97 - 35.0) as u32;
    let cols: Vec<u32> = (0..160).filter(|&x| is_ink(x, y)).collect();
    let groups = 1 + cols.windows(2).filter(|w| w[1] > w[0] + 1).count();
    assert!(groups >= 4, "четыре края ствола, а не сплошная заливка: {cols:?}");
    assert_eq!(px(&img, 155, 125), (255, 255, 255, 255));
}

/// `DrawImage` fills its box with the light-grey placeholder quad
/// (`rgba8(217,217,217,255)`), mirroring the GPU renderer's headless fallback
/// when no decoded pixels are registered.
#[test]
fn draw_image_fills_grey_placeholder() {
    use lumen_layout::{ObjectFit, ObjectPosition, ImageRendering};
    let cmds = vec![DisplayCommand::DrawImage {
        rect: Rect::new(10.0, 10.0, 40.0, 30.0),
        src: "missing.png".into(),
        alt: String::new(),
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        image_rendering: ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Interior of the box is the grey placeholder.
    assert_eq!(px(&img, 30, 25), (217, 217, 217, 255), "placeholder grey");
    // Outside the box stays white.
    assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "exterior stays white");
}

/// BUG-249: `PushClipRoundedRect` clips children to the rounded contour —
/// a child fill covering the whole box is cut at the corner (transparent →
/// stays white) while the box centre is painted. Before the fix the clip was
/// a square scissor, so the corner pixel was wrongly filled (TEST-101).
#[test]
fn rounded_clip_cuts_child_corners() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    // 60×60 box at origin, uniform 30px radius → a full circle (radius = w/2).
    let cmds = vec![
        DisplayCommand::PushClipRoundedRect {
            rect: Rect::new(0.0, 0.0, 60.0, 60.0),
            radii: [30.0; 4],
        },
        DisplayCommand::FillRect { rect: Rect::new(0.0, 0.0, 60.0, 60.0), color: red },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Centre is inside the circle → red.
    assert_eq!(px(&img, 30, 30), (255, 0, 0, 255), "centre painted");
    // Top-left corner (1,1) is well outside the circle → clipped, stays white.
    assert_eq!(px(&img, 1, 1), (255, 255, 255, 255), "corner clipped by radius");
}

/// A degenerate path (fewer than 3 vertices) is a no-op, not a panic.
#[test]
fn svg_path_empty_is_noop() {
    let cmds = vec![DisplayCommand::DrawSvgPath {
        vertices: vec![],
        color: Color { r: 0, g: 0, b: 0, a: 255 },
    }];
    let img = rasterize_cpu(8, 8, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 4, 4), (255, 255, 255, 255), "background untouched");
}

/// A two-stop conic (red at 0° → blue at one revolution, centre, from 0°)
/// sweeps clockwise: the top-centre is the start (red), the bottom-centre is
/// half a revolution in (≈ midway red→blue).
#[test]
fn conic_sweeps_first_to_last_stop() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![DisplayCommand::DrawConicGradient {
        rect: Rect { x: 0.0, y: 0.0, width: 64.0, height: 64.0 },
        center_x_pct: 0.5,
        center_y_pct: 0.5,
        from_angle_deg: 0.0,
        stops: vec![
            GradientStop { color: red, color_space: lumen_layout::ColorSpace::Srgb, position: None },
            GradientStop { color: blue, color_space: lumen_layout::ColorSpace::Srgb, position: None },
        ],
        repeating: false,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Top-centre column ≈ start of the sweep → essentially red.
    let (tr, _tg, tb, ta) = px(&img, 32, 2);
    assert!(tr > 200 && tb < 50 && ta == 255, "top is red-ish, got ({tr},{_tg},{tb},{ta})");

    // Bottom-centre column ≈ half a revolution → midway red→blue.
    let (br, _bg, bb, ba) = px(&img, 32, 61);
    assert!(
        (80..=180).contains(&br) && (80..=180).contains(&bb) && ba == 255,
        "bottom is midway, got ({br},{_bg},{bb},{ba})"
    );
}

/// A conic with no stops is a no-op, not a panic.
#[test]
fn conic_empty_stops_noop() {
    let cmds = vec![DisplayCommand::DrawConicGradient {
        rect: Rect { x: 0.0, y: 0.0, width: 8.0, height: 8.0 },
        center_x_pct: 0.5,
        center_y_pct: 0.5,
        from_angle_deg: 0.0,
        stops: vec![],
        repeating: false,
    }];
    let img = rasterize_cpu(8, 8, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 4, 4), (255, 255, 255, 255), "background untouched");
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, width: w, height: h }
}

/// `PushClipRect` confines a following `FillRect` to the clip region;
/// pixels outside the clip keep the white background.
#[test]
fn push_clip_rect_clips_fill() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushClipRect { rect: rect(10.0, 10.0, 20.0, 20.0) },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Inside the clip [10,30) — filled blue.
    assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "inside clip is blue");
    // Outside the clip — background white.
    assert_eq!(px(&img, 45, 45), (255, 255, 255, 255), "outside clip stays white");
    assert_eq!(px(&img, 4, 4), (255, 255, 255, 255), "above-left of clip stays white");
}

/// `PopClip` removes the clip so a later `FillRect` paints everywhere again.
#[test]
fn pop_clip_restores_full_drawing() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let green = Color { r: 0, g: 255, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushClipRect { rect: rect(10.0, 10.0, 10.0, 10.0) },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
        DisplayCommand::PopClip,
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: green },
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // A point that was outside the first (clipped) fill is painted by the
    // second, unclipped fill → green, proving the clip was popped.
    assert_eq!(px(&img, 45, 45), (0, 255, 0, 255), "post-pop fill reaches outside old clip");
}

/// Nested `PushClipRect`s intersect: only the overlap of both clip rects is
/// drawn; regions inside one but not the other are clipped out.
#[test]
fn nested_clip_intersects() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushClipRect { rect: rect(10.0, 10.0, 40.0, 40.0) }, // x,y ∈ [10,50)
        DisplayCommand::PushClipRect { rect: rect(30.0, 30.0, 40.0, 40.0) }, // x,y ∈ [30,70)
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
        DisplayCommand::PopClip,
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Intersection [30,50) — blue.
    assert_eq!(px(&img, 40, 40), (0, 0, 255, 255), "intersection is blue");
    // Inside outer clip only (x=20 < 30) — clipped out by inner.
    assert_eq!(px(&img, 20, 20), (255, 255, 255, 255), "outer-only region clipped");
    // Inside inner clip only (x=60 ≥ 50) — clipped out by outer.
    assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "inner-only region clipped");
}

/// BUG-140 (TEST-109 c0): `PushClipPath` circle обрезает заливку по
/// окружности, а не по её bounding box — точка внутри bbox, но вне круга,
/// остаётся белой.
#[test]
fn clip_path_circle_clips_to_circle_not_bbox() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushClipPath {
            shape: ResolvedClipShape::Circle { cx: 32.0, cy: 32.0, r: 10.0 },
        },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Центр круга — синий.
    assert_eq!(px(&img, 32, 32), (0, 0, 255, 255), "centre inside circle");
    // Угол bbox (24,24): расстояние до центра ≈ 11.3 > 10 — вне круга,
    // но внутри bounding box. Старое bbox-приближение красило его синим.
    assert_eq!(px(&img, 24, 24), (255, 255, 255, 255), "bbox corner outside circle");
    // Далеко за пределами — белый.
    assert_eq!(px(&img, 5, 5), (255, 255, 255, 255), "far exterior stays white");
}

/// BUG-140 (TEST-109 c2): `PushClipPath` polygon (треугольник) обрезает
/// заливку по форме; вне треугольника (но внутри его bbox) — фон.
#[test]
fn clip_path_polygon_clips_to_triangle() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    // Треугольник (32,4) (60,60) (4,60) — как polygon(50% 0, 100% 100%, 0 100%).
    let cmds = vec![
        DisplayCommand::PushClipPath {
            shape: ResolvedClipShape::Polygon {
                verts: vec![(32.0, 4.0), (60.0, 60.0), (4.0, 60.0)],
                even_odd: false,
            },
        },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Центроид (32, 41) — внутри треугольника.
    assert_eq!(px(&img, 32, 41), (255, 0, 0, 255), "triangle interior red");
    // Верхние углы bbox — вне треугольника.
    assert_eq!(px(&img, 8, 8), (255, 255, 255, 255), "top-left outside triangle");
    assert_eq!(px(&img, 56, 8), (255, 255, 255, 255), "top-right outside triangle");
}

/// CSS Shapes L1 §3/§4 — `clip-path` fill-rule. Два перекрывающихся
/// квадрата одним контуром (одинаковая ориентация → область пересечения
/// имеет winding 2). `even_odd: false` (nonzero) заливает её; `even_odd:
/// true` оставляет «дырку» (чётное число пересечений).
#[test]
fn clip_path_polygon_even_odd_hole() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    // Квадрат A (8,8)-(40,40) и квадрат B (24,24)-(56,56), сшитые в один
    // замкнутый контур; пересечение — (24,24)-(40,40), центр (32,32).
    let verts = vec![
        (8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0), (8.0, 8.0),
        (24.0, 24.0), (56.0, 24.0), (56.0, 56.0), (24.0, 56.0), (24.0, 24.0),
    ];
    let make = |even_odd: bool| {
        let cmds = vec![
            DisplayCommand::PushClipPath {
                shape: ResolvedClipShape::Polygon { verts: verts.clone(), even_odd },
            },
            DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
            DisplayCommand::PopClip,
        ];
        rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize")
    };

    let nz = make(false);
    let eo = make(true);

    // Зоны только-A (14,14) и только-B (50,50) заливаются при обоих правилах.
    assert_eq!(px(&nz, 14, 14), (255, 0, 0, 255), "nonzero: square A red");
    assert_eq!(px(&eo, 14, 14), (255, 0, 0, 255), "evenodd: square A red");
    assert_eq!(px(&nz, 50, 50), (255, 0, 0, 255), "nonzero: square B red");
    assert_eq!(px(&eo, 50, 50), (255, 0, 0, 255), "evenodd: square B red");

    // Центр пересечения (32,32): nonzero заливает, evenodd — дырка (фон).
    assert_eq!(px(&nz, 32, 32), (255, 0, 0, 255), "nonzero fills the overlap");
    assert_eq!(px(&eo, 32, 32), (255, 255, 255, 255), "evenodd leaves a hole in the overlap");
}

/// BUG-140 (TEST-109 c0/c1): clip-path эмитится внутри PushTransform —
/// transform-слой переносит уже обрезанный результат, т.е. клип едет
/// вместе с элементом.
#[test]
fn clip_path_carried_through_transform() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::translation_2d(20.0, 0.0) },
        DisplayCommand::PushClipPath {
            shape: ResolvedClipShape::Circle { cx: 16.0, cy: 32.0, r: 8.0 },
        },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
        DisplayCommand::PopClip,
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Круг задан вокруг (16,32); transform сдвигает результат на +20 по X
    // → синий центр в (36,32), а исходная позиция (16,32) — белая.
    assert_eq!(px(&img, 36, 32), (0, 0, 255, 255), "clipped circle translated to x=36");
    assert_eq!(px(&img, 16, 32), (255, 255, 255, 255), "original position stays white");
}

/// `DrawText` paints the bundled Inter glyphs: a large opaque-coloured run
/// must darken/colour some pixels away from the white background. Exact
/// glyph pixels are font-dependent, so we only assert "ink appeared" within
/// the run box, which is enough to catch a regression to the no-op path.
#[test]
fn draw_text_renders_ink() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: rect(2.0, 2.0, 120.0, 40.0),
        text: "Hi".to_string(),
        font_size: 32.0,
        color: blue,
        font_family: Vec::new(),
        font_weight: lumen_layout::FontWeight::default(),
        font_style: lumen_layout::FontStyle::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    }];
    let img = rasterize_cpu(128, 48, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // At least one pixel in the run box must carry blue ink (not white bg).
    let mut inked = false;
    for y in 2..44 {
        for x in 2..120 {
            let (r, g, b, _) = px(&img, x, y);
            if b > r && b > g {
                inked = true;
            }
        }
    }
    assert!(inked, "DrawText produced no blue ink");
}

/// BUG-1339: `letter-spacing` / `word-spacing` раздвигают глифы в растре —
/// правая граница чернил сдвигается вправо на суммарный интервал.
#[test]
fn draw_text_letter_and_word_spacing_shift_ink() {
    let ink_right = |text: &str, letter: f32, word: f32| {
        let cmds = vec![DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: rect(2.0, 2.0, 300.0, 40.0),
            text: text.to_string(),
            font_size: 20.0,
            color: Color { r: 0, g: 0, b: 255, a: 255 },
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::default(),
            font_style: lumen_layout::FontStyle::default(),
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: crate::display_list::spacing_to_fixed(letter),
            word_spacing: crate::display_list::spacing_to_fixed(word),
            highlight_name: None,
            text_orientation: None,
        }];
        let img = rasterize_cpu(320, 48, &cmds, &[], 0.0, 0.0).expect("rasterize");
        (0..320).rev().find(|&x| (0..48).any(|y| { let (r, g, b, _) = px(&img, x, y); b > r && b > g })).unwrap()
    };
    let base = ink_right("A B", 0.0, 0.0);
    assert!(ink_right("A B", 10.0, 0.0) >= base + 19, "letter-spacing: два интервала до последнего глифа");
    assert!(ink_right("A B", 0.0, 30.0) >= base + 29, "word-spacing сдвигает слово после пробела");
}

/// Ph3 writing-mode vertical, Срез 1: `text_orientation: Sideways` rotates
/// the run 90° CW — its ink bounding box should come out taller than wide,
/// the opposite aspect ratio of the same run drawn horizontally (`None`)
/// at the same `rect`.
#[test]
fn draw_text_sideways_rotates_ink_bbox() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let make_cmds = |orientation: Option<TextOrientation>| {
        vec![DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: rect(4.0, 4.0, 120.0, 40.0),
            text: "Hi".to_string(),
            font_size: 32.0,
            color: blue,
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::default(),
            font_style: lumen_layout::FontStyle::default(),
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: orientation,
        }]
    };
    let ink_bbox = |img: &Image| -> (u32, u32, u32, u32) {
        let (mut l, mut t, mut r, mut b) = (img.width, img.height, 0u32, 0u32);
        for y in 0..img.height {
            for x in 0..img.width {
                let (pr, pg, pb, _) = px(img, x, y);
                if pb > pr && pb > pg {
                    l = l.min(x);
                    t = t.min(y);
                    r = r.max(x);
                    b = b.max(y);
                }
            }
        }
        (l, t, r, b)
    };

    let horizontal =
        rasterize_cpu(160, 160, &make_cmds(None), &[], 0.0, 0.0).expect("rasterize");
    let sideways = rasterize_cpu(
        160, 160, &make_cmds(Some(TextOrientation::Sideways)), &[], 0.0, 0.0,
    )
    .expect("rasterize");

    let (hl, ht, hr, hb) = ink_bbox(&horizontal);
    let (sl, st, sr, sb) = ink_bbox(&sideways);
    assert!(hr > hl && hb > ht, "horizontal run produced no ink");
    assert!(sr > sl && sb > st, "sideways run produced no ink");

    let (h_w, h_h) = (hr - hl, hb - ht);
    let (s_w, s_h) = (sr - sl, sb - st);
    assert!(h_w > h_h, "horizontal run should be wider than tall (w={h_w} h={h_h})");
    assert!(s_h > s_w, "sideways run should be taller than wide (w={s_w} h={s_h})");
}

/// BUG-553, срез 62: a rotated run lives inside its column box. The rotation
/// maps the glyph top onto the column's RIGHT edge (`rect.x + rect.width`) and
/// the body grows leftwards; anchored at `rect.x` the whole run fell one
/// line-height to the left of the box (`vertical-lr` text outside its block).
#[test]
fn draw_text_sideways_ink_stays_inside_the_column_box() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: rect(60.0, 10.0, 40.0, 100.0),
        text: "Hi".to_string(),
        font_size: 32.0,
        color: blue,
        font_family: Vec::new(),
        font_weight: lumen_layout::FontWeight::default(),
        font_style: lumen_layout::FontStyle::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: Some(TextOrientation::Sideways),
    }];
    let img = rasterize_cpu(160, 160, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let (l, t, r, b) = ink_bbox_blue(&img);
    assert!(r > l && b > t, "sideways run produced no ink");
    assert!(l >= 60 && r <= 100, "ink x-range {l}..{r} must lie inside the column 60..100");
    assert!(t >= 10, "run must start at the column top, got y={t}");
}

fn ink_bbox_blue(img: &Image) -> (u32, u32, u32, u32) {
    let (mut l, mut t, mut r, mut b) = (img.width, img.height, 0u32, 0u32);
    for y in 0..img.height {
        for x in 0..img.width {
            let (pr, pg, pb, _) = px(img, x, y);
            if pb > pr && pb > pg {
                l = l.min(x);
                t = t.min(y);
                r = r.max(x);
                b = b.max(y);
            }
        }
    }
    (l, t, r, b)
}

/// Ph3 writing-mode vertical, Срез 3: a pure-CJK run under
/// `text_orientation: Mixed` must stay upright — same ink bbox as the same
/// run drawn horizontally (`None`), NOT rotated like `Sideways`. Before
/// Срез 3 `Mixed` was treated as `Sideways` for the whole run, so this
/// would have failed (the CJK glyph would have come out rotated).
#[test]
fn draw_text_mixed_keeps_cjk_upright() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let make_cmds = |orientation: Option<TextOrientation>| {
        vec![DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: rect(4.0, 4.0, 60.0, 60.0),
            text: "日".to_string(),
            font_size: 32.0,
            color: blue,
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::default(),
            font_style: lumen_layout::FontStyle::default(),
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: orientation,
        }]
    };

    let horizontal =
        rasterize_cpu(80, 80, &make_cmds(None), &[], 0.0, 0.0).expect("rasterize");
    let mixed =
        rasterize_cpu(80, 80, &make_cmds(Some(TextOrientation::Mixed)), &[], 0.0, 0.0)
            .expect("rasterize");

    let (hl, ht, hr, hb) = ink_bbox_blue(&horizontal);
    let (ml, mt, mr, mb) = ink_bbox_blue(&mixed);
    assert!(hr > hl && hb > ht, "horizontal CJK glyph produced no ink");
    assert!(mr > ml && mb > mt, "mixed CJK glyph produced no ink");
    assert_eq!((hl, ht, hr, hb), (ml, mt, mr, mb), "mixed CJK glyph should render identically to horizontal (upright, not rotated)");
}

/// Ph3 writing-mode vertical, Срез 3: a pure-Latin run under
/// `text_orientation: Mixed` still rotates 90° CW, same as `Sideways` —
/// only CJK glyphs get the upright exemption.
#[test]
fn draw_text_mixed_rotates_latin_run() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let make_cmds = |orientation: Option<TextOrientation>| {
        vec![DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: rect(4.0, 4.0, 120.0, 40.0),
            text: "Hi".to_string(),
            font_size: 32.0,
            color: blue,
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::default(),
            font_style: lumen_layout::FontStyle::default(),
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: orientation,
        }]
    };

    let sideways = rasterize_cpu(
        160, 160, &make_cmds(Some(TextOrientation::Sideways)), &[], 0.0, 0.0,
    )
    .expect("rasterize");
    let mixed = rasterize_cpu(160, 160, &make_cmds(Some(TextOrientation::Mixed)), &[], 0.0, 0.0)
        .expect("rasterize");

    let (sl, st, sr, sb) = ink_bbox_blue(&sideways);
    let (ml, mt, mr, mb) = ink_bbox_blue(&mixed);
    assert!(sr > sl && sb > st, "sideways Latin run produced no ink");
    assert!(mr > ml && mb > mt, "mixed Latin run produced no ink");
    let (m_w, m_h) = (mr - ml, mb - mt);
    assert!(m_h > m_w, "mixed Latin run should be taller than wide, like sideways (w={m_w} h={m_h})");
    assert_eq!((sl, st, sr, sb), (ml, mt, mr, mb), "mixed pure-Latin run should rotate identically to sideways");
}

/// Empty text is a no-op: the background stays pure white.
#[test]
fn draw_text_empty_is_noop() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: rect(0.0, 0.0, 64.0, 64.0),
        text: String::new(),
        font_size: 20.0,
        color: black,
        font_family: Vec::new(),
        font_weight: lumen_layout::FontWeight::default(),
        font_style: lumen_layout::FontStyle::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 10, 10), (255, 255, 255, 255), "empty text left bg white");
}

/// A rectangular clip drops glyph pixels outside it: text drawn fully to the
/// left of a clip that starts at x=200 leaves the clipped sample untouched.
#[test]
fn draw_text_respects_clip() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        // Clip to the right half; the text sits in the left half → no ink.
        DisplayCommand::PushClipRect { rect: rect(200.0, 0.0, 100.0, 64.0) },
        DisplayCommand::DrawText {
            font_stretch: lumen_layout::FontStretch::NORMAL,
            rect: rect(2.0, 2.0, 180.0, 40.0),
            text: "Hidden".to_string(),
            font_size: 32.0,
            color: black,
            font_family: Vec::new(),
            font_weight: lumen_layout::FontWeight::default(),
            font_style: lumen_layout::FontStyle::default(),
            font_variation_axes: Vec::new(),
            font_features: Vec::new(),
            font_palette: None,
            tab_size: 0.0,
            letter_spacing: 0,
            word_spacing: 0,
            highlight_name: None,
            text_orientation: None,
        },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(320, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Left half (outside clip) must remain white.
    for x in (5..180).step_by(10) {
        assert_eq!(
            px(&img, x, 20),
            (255, 255, 255, 255),
            "glyph pixel at x={x} should be clipped out",
        );
    }
}

/// FONTLOAD-18: a `font_provider` with no `@font-face` registered for the
/// requested family must leave `DrawText` byte-for-byte identical to the
/// no-provider path — every existing golden's determinism depends on this.
#[test]
fn draw_text_with_provider_but_no_match_is_byte_identical() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: rect(2.0, 2.0, 120.0, 40.0),
        text: "Hi".to_string(),
        font_size: 32.0,
        color: black,
        font_family: vec!["Arial".to_string()],
        font_weight: lumen_layout::FontWeight::default(),
        font_style: lumen_layout::FontStyle::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    }];
    let registry = lumen_font::FontRegistry::new();
    let without_provider = rasterize_cpu(128, 48, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let with_empty_provider = rasterize_cpu_with_fonts(
        128, 48, &cmds, &[], 0.0, 0.0,
        Some(&registry as &dyn FontProvider), None,
    )
    .expect("rasterize");
    assert_eq!(
        without_provider.data, with_empty_provider.data,
        "an unmatched family must not change CPU-path rendering",
    );
}

/// FONTLOAD-18: a registered `@font-face` with `size-adjust` reaches the
/// CPU rasterizer — the same bundled Inter bytes drawn at `size-adjust: 2`
/// must ink noticeably more pixels than the unadjusted default (mirrors
/// `push_text_glyphs`'s per-glyph `size-adjust`, applied per-run here).
#[test]
fn draw_text_size_adjust_from_registered_face_widens_ink() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let bundled = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");
    let registry = lumen_font::FontRegistry::new();
    registry.register_from_bytes(
        "BigWebFont",
        400,
        lumen_core::FontStyle::Normal,
        &[],
        bundled.to_vec(),
        None,
        None,
        Some(2.0),
        None,
        Vec::new(),
    );
    let cmds = vec![DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: rect(2.0, 2.0, 300.0, 80.0),
        text: "Hi".to_string(),
        font_size: 20.0,
        color: black,
        font_family: vec!["BigWebFont".to_string()],
        font_weight: lumen_layout::FontWeight::default(),
        font_style: lumen_layout::FontStyle::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    }];
    let count_ink = |img: &Image| {
        let mut n = 0;
        for y in 0..80 {
            for x in 0..300 {
                let (r, g, b, _) = px(img, x, y);
                if r < 200 && g < 200 && b < 200 {
                    n += 1;
                }
            }
        }
        n
    };
    let unadjusted = rasterize_cpu(300, 80, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let adjusted = rasterize_cpu_with_fonts(
        300, 80, &cmds, &[], 0.0, 0.0,
        Some(&registry as &dyn FontProvider), None,
    )
    .expect("rasterize");
    assert!(
        count_ink(&adjusted) > count_ink(&unadjusted) * 2,
        "size-adjust: 2 must ink substantially more pixels than the default face",
    );
}

/// FONTLOAD-19 — mirrors the wgpu renderer's own BUG-434 unit test
/// (`pick_face_for_codepoint_tests::declared_range_wins_over_accidental_cmap_coverage`):
/// two candidates share ONE font file but declare disjoint `unicode-range`.
/// The declared range must win over the fact that the shared file's `cmap`
/// actually covers both ranges, and — unlike the wgpu per-char cascade —
/// the split must land on a UTF-8 char boundary usable for a real
/// `rustybuzz` shaping pass per sub-run.
#[test]
fn itemize_by_cascade_splits_on_declared_unicode_range() {
    let bundled = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");
    let candidate = |ranges: Vec<(u32, u32)>| {
        let face = build_face(bundled).expect("bundled Inter must parse");
        let cmap = face.font.cmap().ok();
        FaceCandidate { bytes: bundled, face, ranges, cmap }
    };
    let cascade = vec![
        candidate(vec![(0x41, 0x5A)]), // A-Z
        candidate(vec![(0x61, 0x7A)]), // a-z
    ];
    let runs = itemize_by_cascade("AbC", &cascade);
    assert_eq!(runs, vec![(0, "A"), (1, "b"), (0, "C")]);
}

/// A single candidate (the overwhelming common case — no `unicode-range`
/// subsetting) must return the whole text as one run without inspecting
/// a single codepoint — the byte-identical fast path this module's docs
/// promise.
#[test]
fn itemize_by_cascade_single_candidate_is_one_run() {
    let bundled = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");
    let face = build_face(bundled).expect("bundled Inter must parse");
    let cmap = face.font.cmap().ok();
    let cascade = vec![FaceCandidate { bytes: bundled, face, ranges: Vec::new(), cmap }];
    let runs = itemize_by_cascade("Hello, world!", &cascade);
    assert_eq!(runs, vec![(0, "Hello, world!")]);
}

/// A codepoint outside every candidate's declared range falls back to
/// candidate 0 (primary) — same fallback `pick_face_for_codepoint` uses
/// on the wgpu side (draws `.notdef` from the primary rather than
/// silently picking an unrelated candidate).
#[test]
fn itemize_by_cascade_falls_back_to_primary_outside_every_range() {
    let bundled = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");
    let candidate = |ranges: Vec<(u32, u32)>| {
        let face = build_face(bundled).expect("bundled Inter must parse");
        let cmap = face.font.cmap().ok();
        FaceCandidate { bytes: bundled, face, ranges, cmap }
    };
    let cascade = vec![candidate(vec![(0x41, 0x5A)]), candidate(vec![(0x61, 0x7A)])];
    let runs = itemize_by_cascade("!", &cascade);
    assert_eq!(runs, vec![(0, "!")]);
}

/// Full-pipeline check that a `font-family` resolving to a BUG-434
/// unicode-range-subsetted pair still draws real ink through
/// [`rasterize_cpu_with_fonts`] end to end (no panic, no silently-empty
/// draw) — `itemize_by_cascade_splits_on_declared_unicode_range` already
/// proves the routing is correct; this proves the cascade actually
/// reaches rendering from `resolve_face_candidates` through to
/// `rasterize_text`'s glyph loop.
#[test]
fn draw_text_renders_through_unicode_range_sibling_cascade() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let bundled = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf");
    let registry = lumen_font::FontRegistry::new();
    registry.register_from_bytes(
        "SplitFont",
        400,
        lumen_core::FontStyle::Normal,
        &[lumen_font::UnicodeRange { start: 0x41, end: 0x5A }], // A-Z
        bundled.to_vec(),
        None,
        None,
        None,
        None,
        Vec::new(),
    );
    registry.register_from_bytes(
        "SplitFont",
        400,
        lumen_core::FontStyle::Normal,
        &[lumen_font::UnicodeRange { start: 0x61, end: 0x7A }], // a-z
        bundled.to_vec(),
        None,
        None,
        None,
        None,
        Vec::new(),
    );
    let cmds = vec![DisplayCommand::DrawText {
        font_stretch: lumen_layout::FontStretch::NORMAL,
        rect: rect(2.0, 2.0, 300.0, 80.0),
        text: "AbC".to_string(),
        font_size: 24.0,
        color: black,
        font_family: vec!["SplitFont".to_string()],
        font_weight: lumen_layout::FontWeight::default(),
        font_style: lumen_layout::FontStyle::default(),
        font_variation_axes: Vec::new(),
        font_features: Vec::new(),
        font_palette: None,
        tab_size: 0.0,
        letter_spacing: 0,
        word_spacing: 0,
        highlight_name: None,
        text_orientation: None,
    }];
    let img = rasterize_cpu_with_fonts(
        300, 80, &cmds, &[], 0.0, 0.0,
        Some(&registry as &dyn FontProvider), None,
    )
    .expect("rasterize");
    let mut has_ink = false;
    'outer: for y in 0..80 {
        for x in 0..300 {
            let (r, g, b, _) = px(&img, x, y);
            if r < 200 && g < 200 && b < 200 {
                has_ink = true;
                break 'outer;
            }
        }
    }
    assert!(has_ink, "cascade over unicode-range siblings must still draw real ink");
}

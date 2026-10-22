use super::*;

/// `PushOpacity { 0.5 }` around an opaque blue fill blends it 50/50 with the
/// white background: result ≈ (127, 127, 255).
#[test]
fn opacity_half_blends_blue_over_white() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
        DisplayCommand::FillRect { rect: rect(10.0, 10.0, 40.0, 40.0), color: blue },
        DisplayCommand::PopOpacity,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");

    let (r, g, b, a) = px(&img, 30, 30);
    assert!(
        (120..=135).contains(&r) && (120..=135).contains(&g) && b > 250 && a == 255,
        "blue at 0.5 opacity over white ≈ (127,127,255), got ({r},{g},{b},{a})",
    );
    // Outside the group's box stays white.
    assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "exterior stays white");
}

/// `PushOpacity { 0.0 }` makes the whole group invisible: the background is
/// untouched.
#[test]
fn opacity_zero_keeps_background() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushOpacity { alpha: 0.0, bounds: None },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: blue },
        DisplayCommand::PopOpacity,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 32, 32), (255, 255, 255, 255), "fully transparent group");
}

/// BUG-1249: соседние боксы с общей дробной границей не оставляют шва —
/// фон родителя не просвечивает.
#[test]
fn adjacent_fractional_fills_leave_no_seam() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 20.0, 80.0), color: red },
        DisplayCommand::FillRect { rect: rect(0.0, 25.72, 20.0, 25.0), color: green },
        DisplayCommand::FillRect { rect: rect(0.0, 50.72, 20.0, 25.0), color: green },
    ];
    let img = rasterize_cpu(32, 80, &cmds, &[], 0.0, 0.0).expect("rasterize");
    for y in 27..75 {
        assert_eq!(px(&img, 10, y), (0, 128, 0, 255), "row {y}");
    }
}

/// Group opacity fades the *whole* subtree by one alpha: two sibling fills in
/// the same group are each blended 50/50 with the background (not stacked or
/// double-faded).
#[test]
fn opacity_group_fades_whole_subtree() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 20.0, 20.0), color: red },
        DisplayCommand::FillRect { rect: rect(20.0, 0.0, 20.0, 20.0), color: green },
        DisplayCommand::PopOpacity,
    ];
    let img = rasterize_cpu(64, 32, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Red box ≈ (255,127,127).
    let (rr, rg, rb, _) = px(&img, 10, 10);
    assert!(
        rr > 250 && (120..=135).contains(&rg) && (120..=135).contains(&rb),
        "red faded, got ({rr},{rg},{rb})",
    );
    // Green box ≈ (127,191,127): blended from (0,128,0) over white at 0.5.
    let (gr, gg, gb, _) = px(&img, 30, 10);
    assert!(
        (120..=135).contains(&gr) && (185..=197).contains(&gg) && (120..=135).contains(&gb),
        "green faded, got ({gr},{gg},{gb})",
    );
}

/// `PushTransform` with an integer translation shifts the group's ink: a blue
/// fill at the origin lands at the translated position, and the original spot
/// is left at the background colour. Integer translate makes the bilinear
/// resample exact, so the moved fill is solid blue.
#[test]
fn transform_translate_shifts_fill() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::translation_2d(20.0, 10.0) },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 20.0, 20.0), color: blue },
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Translated destination (20+5, 10+5) is solid blue.
    assert_eq!(px(&img, 25, 15), (0, 0, 255, 255), "fill moved by (20,10)");
    // The untranslated origin is now empty → background white.
    assert_eq!(px(&img, 5, 5), (255, 255, 255, 255), "origin cleared by translate");
}

/// BUG-1290: `perspective(100px)` на родителе + `translateZ(50px)` у ребёнка —
/// квадрат 20×20 вокруг центра перспективы растёт вдвое (40×40).
#[test]
fn perspective_translate_z_scales_child() {
    use lumen_layout::Mat4;
    let lime = Color { r: 0, g: 255, b: 0, a: 255 };
    let around = |m: Mat4| {
        Mat4::translation_2d(32.0, 32.0).multiply(&m).multiply(&Mat4::translation_2d(-32.0, -32.0))
    };
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: around(Mat4::perspective(100.0)) },
        DisplayCommand::PushTransform { matrix: Mat4::translate_3d(0.0, 0.0, 50.0) },
        DisplayCommand::FillRect { rect: rect(22.0, 22.0, 20.0, 20.0), color: lime },
        DisplayCommand::PopTransform,
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 14, 32), (0, 255, 0, 255), "вдвое шире исходных 22…42 → 12…52");
    assert_eq!(px(&img, 50, 32), (0, 255, 0, 255));
    assert_eq!(px(&img, 8, 32), (255, 255, 255, 255));
}

/// BUG-1290: `perspective · rotateY` — не аффинное: ближняя половина
/// квадрата выше дальней, а сам квадрат не плоский прямоугольник.
#[test]
fn perspective_rotate_y_draws_trapezoid() {
    use lumen_layout::Mat4;
    let lime = Color { r: 0, g: 255, b: 0, a: 255 };
    let m = Mat4::translation_2d(32.0, 32.0)
        .multiply(&Mat4::perspective(60.0))
        .multiply(&Mat4::rotate_y(1.0))
        .multiply(&Mat4::translation_2d(-32.0, -32.0));
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: m },
        DisplayCommand::FillRect { rect: rect(12.0, 12.0, 40.0, 40.0), color: lime },
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let col = |x: u32| (0..64).filter(|&y| px(&img, x, y) == (0, 255, 0, 255)).count();
    let widths: Vec<usize> = (0..64).map(col).filter(|&h| h > 0).collect();
    assert!(widths.len() > 8, "что-то нарисовано");
    let (a, b) = (widths[0], widths[widths.len() - 1]);
    assert!(a.abs_diff(b) > 6, "края трапеции разной высоты: {a} vs {b}");
    assert_eq!(px(&img, 2, 2), (255, 255, 255, 255));
}

/// BUG-1288: блок, лежащий до трансформации за правым краем полотна
/// (`left:900px` на полотне 64), после `translate(-40)` попадает в кадр.
#[test]
fn transform_group_beyond_canvas_right_edge_is_drawn() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::translation_2d(-90.0, 0.0) },
        DisplayCommand::FillRect { rect: rect(100.0, 10.0, 20.0, 20.0), color: blue },
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 15, 20), (0, 0, 255, 255), "block at 100 moved to x=10..30");
    assert_eq!(px(&img, 35, 20), (255, 255, 255, 255), "right of the block stays white");
    assert_eq!(px(&img, 5, 20), (255, 255, 255, 255), "left of the block stays white");
}

/// BUG-1288: отрицательные локальные координаты — блок `left:-100px`
/// с `translate(+120)` лежит на x = 20…40.
#[test]
fn transform_group_with_negative_local_coords_is_drawn() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::translation_2d(120.0, 0.0) },
        DisplayCommand::FillRect { rect: rect(-100.0, 10.0, 20.0, 20.0), color: blue },
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 30, 20), (0, 0, 255, 255), "block at -100 moved to x=20..40");
    assert_eq!(px(&img, 10, 20), (255, 255, 255, 255), "left of the block stays white");
}

/// BUG-1288: поворот на 90° вокруг точки блока, чьи локальные координаты
/// частично отрицательны — видна вся фигура, а не её половина.
#[test]
fn rotated_group_with_partly_negative_local_coords_is_whole() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    // Блок 20×40 в локальных (-10..10, 0..40); поворот 90° вокруг (0,0)
    // + сдвиг (40,0): (x,y) → (40 - y, x) — блок займёт x 0..40, y -10..10;
    // видимая нижняя часть y 0..10.
    let m = lumen_layout::Mat4::translation_2d(40.0, 0.0)
        .multiply(&lumen_layout::Mat4::rotate_2d(std::f32::consts::FRAC_PI_2));
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: m },
        DisplayCommand::FillRect { rect: rect(-10.0, 0.0, 20.0, 40.0), color: blue },
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Локальная точка (-5, 20) (отрицательная x) → (40-20, -5): вне кадра;
    // локальная (5, 20) → (20, 5): внутри.
    assert_eq!(px(&img, 20, 5), (0, 0, 255, 255), "local x>0 half is drawn");
    assert_eq!(px(&img, 20, 15), (255, 255, 255, 255), "below the block stays white");
}

/// BUG-424: an ambient clip must survive a `PushTransform` boundary by being
/// mapped into the layer's pre-transform space.
///
/// This is the icon-sprite shape verbatim: the emitter wraps the icon in
/// `PushClipRect` at its document rect (30,10,14,14) and a `PushTransform`
/// carrying `viewBox`→CSS-px scale plus the same offset, then draws the path
/// in local `viewBox` units (0…24). Before the fix the clip was tested
/// against those local coordinates, the two rects were disjoint, and every
/// stroke of every toolbar icon was culled — the chrome painted blank
/// buttons.
#[test]
fn bug424_clip_maps_into_transform_layer_space() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let scale = 14.0 / 24.0;
    let mut m = lumen_layout::Mat4::scale_2d(scale, scale);
    m.0[12] = 30.0;
    m.0[13] = 10.0;
    let cmds = vec![
        DisplayCommand::PushClipRect { rect: rect(30.0, 10.0, 14.0, 14.0) },
        DisplayCommand::PushTransform { matrix: m },
        // Local viewBox units, as `<symbol viewBox="0 0 24 24">` content is.
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 24.0, 24.0), color: blue },
        DisplayCommand::PopTransform,
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Centre of the icon's document rect is painted.
    let (r, g, b, _) = px(&img, 37, 17);
    assert!(b > 200 && r < 60 && g < 60, "icon must paint inside its clip, got ({r},{g},{b})");
    // The clip still holds: nothing leaks past the icon's document rect.
    assert_eq!(px(&img, 50, 17), (255, 255, 255, 255), "clip must still bound the layer");
}

/// An identity `PushTransform` is a no-op: compositing the layer through the
/// identity affine copies it back byte-for-byte, so the fill is unchanged.
#[test]
fn transform_identity_is_noop() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::IDENTITY },
        DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "identity transform unchanged");
}

/// `scale(2)` about the origin grows the group: a 10×10 fill at the origin
/// covers roughly a 20×20 area, so a sample well inside the scaled region
/// that was white before the scale is now blue.
#[test]
fn transform_scale_grows_fill() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushTransform { matrix: lumen_layout::Mat4::scale_2d(2.0, 2.0) },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 10.0, 10.0), color: blue },
        DisplayCommand::PopTransform,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Source (6,6) — interior of the 10×10 fill — maps to dest (12,12).
    assert_eq!(px(&img, 12, 12), (0, 0, 255, 255), "scaled fill covers (12,12)");
}

/// `mix-blend-mode: multiply` multiplies the source against the backdrop
/// (`result = s·d / 255` per channel). A magenta (255,0,255) layer blended
/// over a yellow (255,255,0) backdrop yields red (255,0,0) where they
/// overlap; outside the source rect the backdrop is unchanged.
#[test]
fn blend_multiply_darkens() {
    let yellow = Color { r: 255, g: 255, b: 0, a: 255 };
    let magenta = Color { r: 255, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: yellow },
        DisplayCommand::PushBlendMode { mode: crate::BlendMode::Multiply, bounds: rect(0.0, 0.0, 32.0, 32.0) },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: magenta },
        DisplayCommand::PopBlendMode,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Overlap: multiply(yellow, magenta) = (255,0,0).
    assert_eq!(px(&img, 10, 10), (255, 0, 0, 255), "multiply yields red over overlap");
    // Outside the source rect: untouched yellow backdrop.
    assert_eq!(px(&img, 50, 50), (255, 255, 0, 255), "backdrop untouched outside source");
}

/// `mix-blend-mode: normal` is plain source-over: an opaque blue fill on
/// white paints solid blue, identical to no blend mode at all.
#[test]
fn blend_normal_is_source_over() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushBlendMode { mode: crate::BlendMode::Normal, bounds: rect(10.0, 10.0, 20.0, 20.0) },
        DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
        DisplayCommand::PopBlendMode,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "normal blend = source-over");
    assert_eq!(px(&img, 50, 50), (255, 255, 255, 255), "exterior stays white");
}

/// `mix-blend-mode: difference` is `|s − d|` per channel. A red (255,0,0)
/// layer over the white (255,255,255) background yields cyan (0,255,255).
#[test]
fn blend_difference_inverts() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushBlendMode { mode: crate::BlendMode::Difference, bounds: rect(0.0, 0.0, 32.0, 32.0) },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: red },
        DisplayCommand::PopBlendMode,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // |red − white| = (0,255,255) cyan.
    assert_eq!(px(&img, 10, 10), (0, 255, 255, 255), "difference of red over white = cyan");
}

/// `filter: blur` spreads ink past the source rect edge. A black square
/// inside a `PushFilter { Blur }` group leaks non-white pixels into the
/// neighbouring band that the unblurred fill never touched, and softens the
/// rect's own interior corner toward grey.
#[test]
fn filter_blur_spreads_ink() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(4.0)], bounds: None },
        DisplayCommand::FillRect { rect: rect(20.0, 20.0, 24.0, 24.0), color: black },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Just outside the original right edge (x=44): unblurred would be pure
    // white; blur leaks darkness here.
    let (r, _, _, _) = px(&img, 46, 32);
    assert!(r < 250, "blur leaks ink past the rect edge (r={r})");
    // Far from the rect stays white.
    assert_eq!(px(&img, 60, 60), (255, 255, 255, 255), "far corner stays white");
}

/// BUG-850: an unclamped blur radius made `box_blur_h`/`box_blur_v`'s
/// accumulator-priming loop cost O(radius) per row/column regardless of
/// the layer's own size — `backdrop-filter: blur(100000px)` on a 1024px
/// layer took 16 s for one screenshot. Reaching this assertion at all,
/// well under the generous bound, is the point — a regression here
/// reintroduces the unbounded cost, not a wrong pixel.
#[test]
fn filter_blur_huge_radius_does_not_blow_up_cost() {
    // Fills the left half black so a fully-saturated blur (radius >> the
    // 256px layer) still produces a measurable, non-white mid-tone deep
    // in the right (white) half — the accumulator-priming fix bounds
    // *cost*, not the output, so an oversaturated blur must still blend.
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(100_000.0)], bounds: None },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 128.0, 256.0), color: black },
        DisplayCommand::PopFilter,
    ];
    let start = std::time::Instant::now();
    let img = rasterize_cpu(256, 256, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 10,
        "blur(100000px) on a 256px layer took {elapsed:?} — the radius clamp regressed",
    );
    let (r, _, _, _) = px(&img, 220, 128);
    assert!(r < 220, "a fully saturated blur must still visibly blend into the far side (r={r})");
}

/// `filter: blur(0)` (radius rounds to 0) is an identity: the fill composites
/// exactly as if no filter were present.
#[test]
fn filter_blur_zero_is_identity() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(0.0)], bounds: None },
        DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "blur(0) leaves the fill intact");
    assert_eq!(px(&img, 50, 50), (255, 255, 255, 255), "exterior stays white");
}

/// BUG-267 reference: the pre-crop full-canvas filter path — draw the rect
/// into a full-size transparent layer, blur the WHOLE canvas, composite at
/// (0,0) over white. The cropped production path must match byte-for-byte.
fn full_canvas_blur_reference(
    width: u32,
    height: u32,
    r: Rect,
    color: Color,
    sigma: f32,
) -> Vec<u8> {
    let mut base = tiny_skia::Pixmap::new(width, height).expect("base");
    base.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
    let mut layer = tiny_skia::Pixmap::new(width, height).expect("layer");
    rasterize_fill_rect(&mut layer, &r, &color, None).expect("fill");
    let blurred = gaussian_blur(&layer, sigma);
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    base.draw_pixmap(0, 0, blurred.as_ref(), &paint, tiny_skia::Transform::identity(), None);
    base.data().to_vec()
}

/// BUG-267: blurring only the dirty-box crop must produce exactly the same
/// pixels as blurring the full canvas — interior shadow case.
#[test]
fn filter_blur_crop_matches_full_canvas_blur() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let shadow = rect(60.0, 40.0, 30.0, 20.0);
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(4.0)], bounds: None },
        DisplayCommand::FillRect { rect: shadow, color: black },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(200, 150, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let reference = full_canvas_blur_reference(200, 150, shadow, black, 4.0);
    assert_eq!(img.data, reference, "cropped blur must be bit-identical to full-canvas blur");
}

/// BUG-267: same equivalence with the ink hugging the canvas corner, where
/// the crop window clamps to the canvas edge and the box-blur edge-replicate
/// clamping kicks in on both paths.
#[test]
fn filter_blur_crop_matches_full_at_canvas_edge() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let shadow = rect(0.0, 0.0, 30.0, 20.0);
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(5.0)], bounds: None },
        DisplayCommand::FillRect { rect: shadow, color: black },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(120, 90, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let reference = full_canvas_blur_reference(120, 90, shadow, black, 5.0);
    assert_eq!(img.data, reference, "edge-hugging cropped blur must match full-canvas blur");
}

/// BUG-267: a filter group whose ink lies fully off-canvas composites
/// nothing — the canvas stays white and nothing panics.
#[test]
fn filter_offcanvas_ink_is_skipped() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(4.0)], bounds: None },
        DisplayCommand::FillRect { rect: rect(-100.0, 10.0, 50.0, 20.0), color: black },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert!(
        img.data.chunks_exact(4).all(|p| p == [255, 255, 255, 255]),
        "off-canvas filtered ink must leave the canvas white"
    );
}

/// BUG-267: an empty filter group (no draws between Push and Pop) is a
/// no-op — the untouched layer is skipped without a full-canvas composite.
#[test]
fn filter_empty_group_is_noop() {
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Blur(4.0)], bounds: None },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert!(
        img.data.chunks_exact(4).all(|p| p == [255, 255, 255, 255]),
        "empty filter group must leave the canvas white"
    );
}

/// BUG-267 (slices B–D) reference scaffolding: white base with a full-canvas
/// gradient backdrop (so destination pixels vary in colour) plus a group
/// layer holding a semi-transparent fill (so blending exercises non-trivial
/// alpha rounding). Mirrors exactly what `rasterize_cpu` does before the
/// group's Pop.
fn crop_reference_scene(
    width: u32,
    height: u32,
    fill: Rect,
    color: Color,
) -> (tiny_skia::Pixmap, tiny_skia::Pixmap) {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let stops = vec![
        GradientStop { color: red, color_space: lumen_layout::ColorSpace::Srgb, position: None },
        GradientStop { color: blue, color_space: lumen_layout::ColorSpace::Srgb, position: None },
    ];
    let mut base = tiny_skia::Pixmap::new(width, height).expect("base");
    base.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
    rasterize_linear_gradient(
        &mut base,
        &rect(0.0, 0.0, width as f32, height as f32),
        90.0,
        &stops,
        false,
        None,
    )
    .expect("backdrop gradient");
    let mut layer = tiny_skia::Pixmap::new(width, height).expect("layer");
    rasterize_fill_rect(&mut layer, &fill, &color, None).expect("fill");
    (base, layer)
}

/// The display-list prefix matching [`crop_reference_scene`]'s backdrop.
fn crop_scene_backdrop_cmds(width: u32, height: u32) -> Vec<DisplayCommand> {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    vec![DisplayCommand::DrawLinearGradient {
        rect: rect(0.0, 0.0, width as f32, height as f32),
        angle_deg: 90.0,
        stops: vec![
            GradientStop { color: red, color_space: lumen_layout::ColorSpace::Srgb, position: None },
            GradientStop { color: blue, color_space: lumen_layout::ColorSpace::Srgb, position: None },
        ],
        repeating: false,
    }]
}

/// BUG-267 (slice D): compositing only the opacity layer's dirty window must
/// match the pre-crop full-canvas composite byte-for-byte.
#[test]
fn opacity_crop_matches_full_canvas() {
    let green = Color { r: 0, g: 160, b: 40, a: 128 };
    let fill = rect(50.0, 30.0, 40.0, 25.0);
    let mut cmds = crop_scene_backdrop_cmds(160, 120);
    cmds.extend([
        DisplayCommand::PushOpacity { alpha: 0.5, bounds: None },
        DisplayCommand::FillRect { rect: fill, color: green },
        DisplayCommand::PopOpacity,
    ]);
    let img = rasterize_cpu(160, 120, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let (mut base, layer) = crop_reference_scene(160, 120, fill, green);
    let paint = tiny_skia::PixmapPaint {
        opacity: 0.5,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    base.draw_pixmap(0, 0, layer.as_ref(), &paint, tiny_skia::Transform::identity(), None);
    assert_eq!(img.data, base.data().to_vec(), "cropped opacity composite must be bit-identical");
}

/// BUG-267 (slice D): the cropped blend composite must match the pre-crop
/// full-canvas composite byte-for-byte — outside the dirty window every CSS
/// blend mode must leave the varied backdrop untouched for the transparent
/// source. Sweeps every supported `mix-blend-mode`.
#[test]
fn blend_crop_matches_full_canvas_all_modes() {
    use crate::BlendMode as B;
    let modes = [
        B::Normal, B::Multiply, B::Screen, B::Overlay, B::Darken, B::Lighten,
        B::ColorDodge, B::ColorBurn, B::HardLight, B::SoftLight, B::Difference,
        B::Exclusion, B::Hue, B::Saturation, B::Color, B::Luminosity, B::PlusLighter,
    ];
    let orange = Color { r: 230, g: 120, b: 10, a: 128 };
    let fill = rect(40.0, 20.0, 50.0, 30.0);
    for mode in modes {
        let mut cmds = crop_scene_backdrop_cmds(160, 120);
        cmds.extend([
            DisplayCommand::PushBlendMode { mode, bounds: fill },
            DisplayCommand::FillRect { rect: fill, color: orange },
            DisplayCommand::PopBlendMode,
        ]);
        let img = rasterize_cpu(160, 120, &cmds, &[], 0.0, 0.0).expect("rasterize");
        let (mut base, layer) = crop_reference_scene(160, 120, fill, orange);
        let paint = tiny_skia::PixmapPaint {
            opacity: 1.0,
            blend_mode: map_blend_mode(mode),
            quality: tiny_skia::FilterQuality::Nearest,
        };
        base.draw_pixmap(0, 0, layer.as_ref(), &paint, tiny_skia::Transform::identity(), None);
        assert_eq!(
            img.data,
            base.data().to_vec(),
            "cropped blend composite must be bit-identical for {mode:?}"
        );
    }
}

/// BUG-267 (slice B): the cropped rounded-rect clip composite must match the
/// pre-crop full-canvas composite byte-for-byte, including the AA coverage
/// multiply along the curved corners.
#[test]
fn clip_rounded_rect_crop_matches_full_canvas() {
    let purple = Color { r: 120, g: 0, b: 200, a: 200 };
    let clip = rect(30.0, 25.0, 60.0, 40.0);
    let radii = [12.0, 8.0, 16.0, 4.0];
    // The fill overflows the clip on every side.
    let fill = rect(20.0, 15.0, 80.0, 60.0);
    let mut cmds = crop_scene_backdrop_cmds(160, 120);
    cmds.extend([
        DisplayCommand::PushClipRoundedRect { rect: clip, radii },
        DisplayCommand::FillRect { rect: fill, color: purple },
        DisplayCommand::PopClip,
    ]);
    let img = rasterize_cpu(160, 120, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let (mut base, mut layer) = crop_reference_scene(160, 120, fill, purple);
    let coverage =
        rasterize_rounded_rect_coverage(&clip, &radii, 160, 120).expect("coverage");
    multiply_alpha_by_mask(&mut layer, &coverage, 0, 0);
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    base.draw_pixmap(0, 0, layer.as_ref(), &paint, tiny_skia::Transform::identity(), None);
    assert_eq!(img.data, base.data().to_vec(), "cropped rounded clip must be bit-identical");
}

/// BUG-267 (slice B): same equivalence for a `clip-path` shape layer.
#[test]
fn clip_shape_crop_matches_full_canvas() {
    let teal = Color { r: 0, g: 140, b: 140, a: 220 };
    let shape = ResolvedClipShape::Circle { cx: 70.0, cy: 55.0, r: 28.0 };
    let fill = rect(30.0, 20.0, 80.0, 70.0);
    let mut cmds = crop_scene_backdrop_cmds(160, 120);
    cmds.extend([
        DisplayCommand::PushClipPath { shape: shape.clone() },
        DisplayCommand::FillRect { rect: fill, color: teal },
        DisplayCommand::PopClip,
    ]);
    let img = rasterize_cpu(160, 120, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let (mut base, mut layer) = crop_reference_scene(160, 120, fill, teal);
    let coverage = rasterize_clip_shape_coverage(&shape, 160, 120).expect("coverage");
    multiply_alpha_by_mask(&mut layer, &coverage, 0, 0);
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    base.draw_pixmap(0, 0, layer.as_ref(), &paint, tiny_skia::Transform::identity(), None);
    assert_eq!(img.data, base.data().to_vec(), "cropped shape clip must be bit-identical");
}

/// BUG-267 (slice C): the cropped mask composite must match the pre-crop
/// full-canvas composite byte-for-byte, with the fill hugging the canvas
/// edge so the crop clamps like the filter edge test.
#[test]
fn mask_crop_matches_full_canvas_at_canvas_edge() {
    let navy = Color { r: 0, g: 30, b: 120, a: 255 };
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let clear = Color { r: 0, g: 0, b: 0, a: 0 };
    let fill = rect(0.0, 0.0, 50.0, 40.0);
    let spec_rect = rect(0.0, 0.0, 50.0, 40.0);
    let stops = vec![
        GradientStop { color: black, color_space: lumen_layout::ColorSpace::Srgb, position: None },
        GradientStop { color: clear, color_space: lumen_layout::ColorSpace::Srgb, position: None },
    ];
    let mut cmds = crop_scene_backdrop_cmds(160, 120);
    cmds.extend([
        DisplayCommand::PushMaskLinearGradient {
            rect: spec_rect,
            angle_deg: 180.0,
            stops: stops.clone(),
            repeating: false,
        },
        DisplayCommand::FillRect { rect: fill, color: navy },
        DisplayCommand::PopMask,
    ]);
    let img = rasterize_cpu(160, 120, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let (mut base, mut layer) = crop_reference_scene(160, 120, fill, navy);
    let spec = MaskSpec::Linear { rect: spec_rect, angle_deg: 180.0, stops, repeating: false };
    let mask = render_mask(&spec, 160, 120).expect("mask");
    multiply_alpha_by_mask(&mut layer, &mask, 0, 0);
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    base.draw_pixmap(0, 0, layer.as_ref(), &paint, tiny_skia::Transform::identity(), None);
    assert_eq!(img.data, base.data().to_vec(), "cropped mask composite must be bit-identical");
}

/// BUG-267 (slice B): clipped ink lying fully off-canvas composites nothing
/// — the canvas stays white and nothing panics (the dirty window misses the
/// canvas, so the whole composite is skipped).
#[test]
fn clip_rounded_rect_offcanvas_ink_is_skipped() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushClipRoundedRect {
            rect: rect(-90.0, 10.0, 40.0, 20.0),
            radii: [4.0; 4],
        },
        DisplayCommand::FillRect { rect: rect(-100.0, 10.0, 50.0, 20.0), color: black },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert!(
        img.data.chunks_exact(4).all(|p| p == [255, 255, 255, 255]),
        "off-canvas clipped ink must leave the canvas white"
    );
}

/// BUG-267 (slice B): a degenerate (zero-size) rounded clip rect clips
/// everything out — the composite is skipped entirely, matching the old
/// full-canvas path that zeroed the masked layer.
#[test]
fn clip_rounded_rect_degenerate_clips_all() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushClipRoundedRect {
            rect: rect(20.0, 20.0, 0.0, 10.0),
            radii: [0.0; 4],
        },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: black },
        DisplayCommand::PopClip,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert!(
        img.data.chunks_exact(4).all(|p| p == [255, 255, 255, 255]),
        "degenerate rounded clip must clip everything out"
    );
}

/// `filter: grayscale(1)` collapses a saturated colour to its luminance.
/// Pure red (1,0,0) → luma 0.2126 → ~54 on all three channels.
#[test]
fn filter_grayscale_full() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Grayscale(1.0)], bounds: None },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: red },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let (r, g, b, a) = px(&img, 10, 10);
    assert_eq!(a, 255);
    assert_eq!(r, g, "grayscale equalises channels");
    assert_eq!(g, b, "grayscale equalises channels");
    assert!((50..=58).contains(&r), "red luma ≈ 54 (got {r})");
}

/// `filter: invert(1)` flips each channel. Black (0,0,0) → white (255,255,255).
#[test]
fn filter_invert_full() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::PushFilter { filters: vec![FilterFn::Invert(1.0)], bounds: None },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 32.0), color: black },
        DisplayCommand::PopFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 10, 10), (255, 255, 255, 255), "invert(black) = white");
}

/// `backdrop-filter: grayscale(1)` desaturates the content already painted
/// behind the element, clipped to the element's border box. A red backdrop
/// turns grey only inside `bounds`; outside `bounds` it stays red.
#[test]
fn backdrop_filter_grayscale_filters_backdrop_in_bounds() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: red },
        DisplayCommand::PushBackdropFilter {
            filters: vec![FilterFn::Grayscale(1.0)],
            bounds: rect(0.0, 0.0, 32.0, 32.0),
        },
        DisplayCommand::PopBackdropFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let (r, g, b, a) = px(&img, 10, 10);
    assert_eq!(a, 255);
    assert_eq!(r, g, "inside bounds: grayscale equalises channels");
    assert_eq!(g, b, "inside bounds: grayscale equalises channels");
    assert!((50..=58).contains(&r), "red luma ≈ 54 inside bounds (got {r})");
    assert_eq!(px(&img, 50, 50), (255, 0, 0, 255), "outside bounds backdrop stays red");
}

/// `backdrop-filter: invert(1)` flips the backdrop within `bounds`. A black
/// backdrop becomes white inside the element's border box, stays black
/// outside it.
#[test]
fn backdrop_filter_invert_filters_backdrop_in_bounds() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 64.0, 64.0), color: black },
        DisplayCommand::PushBackdropFilter {
            filters: vec![FilterFn::Invert(1.0)],
            bounds: rect(0.0, 0.0, 32.0, 32.0),
        },
        DisplayCommand::PopBackdropFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 10, 10), (255, 255, 255, 255), "inside bounds: invert(black) = white");
    assert_eq!(px(&img, 50, 50), (0, 0, 0, 255), "outside bounds backdrop stays black");
}

/// `backdrop-filter: blur` softens a sharp backdrop edge. With a black/white
/// split at x=32 and a blur covering the whole frame, the column at the edge
/// turns mid-grey while the far interior stays close to its original colour.
#[test]
fn backdrop_filter_blur_softens_backdrop_edge() {
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let cmds = vec![
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 32.0, 64.0), color: black },
        DisplayCommand::PushBackdropFilter {
            filters: vec![FilterFn::Blur(4.0)],
            bounds: rect(0.0, 0.0, 64.0, 64.0),
        },
        DisplayCommand::PopBackdropFilter,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // At the edge the blur mixes black and white toward grey.
    let (r, _, _, _) = px(&img, 32, 32);
    assert!((40..=215).contains(&r), "edge column blurred toward grey (r={r})");
    // Far inside the black half stays dark.
    let (rl, _, _, _) = px(&img, 2, 32);
    assert!(rl < 80, "left interior stays near black (r={rl})");
    // Far inside the white half stays light.
    let (rr, _, _, _) = px(&img, 62, 32);
    assert!(rr > 200, "right interior stays near white (r={rr})");
}

/// `mask-image: linear-gradient(to bottom, black, transparent)` fades the
/// element by the gradient's *alpha*. A blue box masked top→bottom stays
/// solid blue where the mask is opaque (top) and disappears toward the
/// background where the mask is transparent (bottom).
#[test]
fn mask_linear_alpha_gradient_fades_box() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let clear = Color { r: 0, g: 0, b: 0, a: 0 };
    let cmds = vec![
        DisplayCommand::PushMaskLinearGradient {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            angle_deg: 180.0, // CSS "to bottom"
            stops: vec![
                GradientStop { color: black, color_space: lumen_layout::ColorSpace::Srgb, position: None },
                GradientStop { color: clear, color_space: lumen_layout::ColorSpace::Srgb, position: None },
            ],
            repeating: false,
        },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 40.0, 40.0), color: blue },
        DisplayCommand::PopMask,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Top: mask alpha ≈ 1 → solid blue.
    let (tr, tg, tb, ta) = px(&img, 20, 1);
    assert!(tr < 20 && tg < 20 && tb > 250 && ta == 255, "top stays blue, got ({tr},{tg},{tb},{ta})");
    // Bottom: mask alpha ≈ 0 → faded to the white background.
    let (br, bg, bb, _) = px(&img, 20, 38);
    assert!(br > 200 && bg > 200 && bb > 200, "bottom fades to white, got ({br},{bg},{bb})");
}

/// `mask-image: radial-gradient(black, transparent)` reveals the element at
/// the centre (opaque mask) and hides it toward the corners (transparent
/// mask).
#[test]
fn mask_radial_reveals_center_hides_corner() {
    let green = Color { r: 0, g: 200, b: 0, a: 255 };
    let black = Color { r: 0, g: 0, b: 0, a: 255 };
    let clear = Color { r: 0, g: 0, b: 0, a: 0 };
    let cmds = vec![
        DisplayCommand::PushMaskRadialGradient {
            rect: rect(0.0, 0.0, 40.0, 40.0),
            center_x_pct: 0.5,
            center_y_pct: 0.5,
            stops: vec![
                GradientStop { color: black, color_space: lumen_layout::ColorSpace::Srgb, position: None },
                GradientStop { color: clear, color_space: lumen_layout::ColorSpace::Srgb, position: None },
            ],
            repeating: false,
        },
        DisplayCommand::FillRect { rect: rect(0.0, 0.0, 40.0, 40.0), color: green },
        DisplayCommand::PopMask,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Centre: mask opaque → green shows.
    let (cr, cg, cb, ca) = px(&img, 20, 20);
    assert!(cr < 10 && cg > 180 && cb < 10 && ca == 255, "centre shows green, got ({cr},{cg},{cb},{ca})");
    // Corner: mask transparent → faded to white.
    let (kr, kg, kb, _) = px(&img, 1, 1);
    assert!(kr > 200 && kg > 200 && kb > 200, "corner fades to white, got ({kr},{kg},{kb})");
}

/// BUG-052: thin border with sub-pixel position must not panic.
///
/// `anti_alias: true` on tiny-skia `fill_rect` trips a `debug_assert!(false)`
/// in `hairline_aa::fill_dot8` when the fixed-point inner span rounds to zero
/// (which happens for thin rects at sub-pixel offsets). `anti_alias: false`
/// avoids the AA path — axis-aligned solid border quads don't need AA (the
/// GPU renderer draws them without per-edge AA anyway).
#[test]
fn draw_border_thin_subpixel_no_panic() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let cmds = vec![DisplayCommand::DrawBorder {
        rect: rect(10.3, 10.7, 50.0, 50.0),
        widths: [0.5, 0.5, 0.5, 0.5],
        colors: [red; 4],
        styles: [lumen_layout::BorderStyle::Solid; 4],
        radii: CornerRadii::default(),
    }];
    // Pre-fix this would panic via debug_assert! in tiny-skia hairline_aa::fill_dot8.
    let img = rasterize_cpu(128, 128, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 64, 64), (255, 255, 255, 255), "interior stays white");
}

/// Dashed border: top edge must have visible dashes (some red pixels) with
/// gaps between them (some white pixels). Interior must remain white.
#[test]
fn draw_border_dashed_top_has_gaps() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    // 100px wide box, 4px top dash border; all other sides None.
    let cmds = vec![DisplayCommand::DrawBorder {
        rect: rect(10.0, 10.0, 100.0, 60.0),
        widths: [4.0, 0.0, 0.0, 0.0],
        colors: [red, red, red, red],
        styles: [
            lumen_layout::BorderStyle::Dashed,
            lumen_layout::BorderStyle::None,
            lumen_layout::BorderStyle::None,
            lumen_layout::BorderStyle::None,
        ],
        radii: CornerRadii::default(),
    }];
    let img = rasterize_cpu(200, 100, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Interior must be white.
    assert_eq!(px(&img, 60, 40), (255, 255, 255, 255), "interior white");
    // The top edge (y=11) should have at least one red pixel and one white pixel
    // to confirm dashes (not a solid band) are drawn.
    let has_red: bool = (10..110).any(|x| {
        let (r, ..) = px(&img, x, 11);
        r > 200
    });
    let has_gap: bool = (10..110).any(|x| {
        let (r, g, b, _) = px(&img, x, 11);
        r > 200 && g > 200 && b > 200
    });
    assert!(has_red, "dashed top border: no red pixels found on top edge");
    assert!(has_gap, "dashed top border: no gaps (white pixels) found on top edge");
}

/// Dotted border: left vertical edge must have visible dots with gaps.
#[test]
fn draw_border_dotted_left_has_gaps() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    // 60px tall box, 4px left dotted border.
    let cmds = vec![DisplayCommand::DrawBorder {
        rect: rect(10.0, 10.0, 80.0, 60.0),
        widths: [0.0, 0.0, 0.0, 4.0],
        colors: [blue; 4],
        styles: [
            lumen_layout::BorderStyle::None,
            lumen_layout::BorderStyle::None,
            lumen_layout::BorderStyle::None,
            lumen_layout::BorderStyle::Dotted,
        ],
        radii: CornerRadii::default(),
    }];
    let img = rasterize_cpu(200, 100, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Interior must be white.
    assert_eq!(px(&img, 60, 40), (255, 255, 255, 255), "interior white");
    // Left edge (x=11) should have at least one blue and one white pixel.
    let has_blue: bool = (10..70).any(|y| {
        let (.., b, _) = px(&img, 11, y);
        b > 200
    });
    let has_gap: bool = (10..70).any(|y| {
        let (r, g, b, _) = px(&img, 11, y);
        r > 200 && g > 200 && b > 200
    });
    assert!(has_blue, "dotted left border: no blue pixels found");
    assert!(has_gap, "dotted left border: no gaps (white pixels) found");
}

/// Dashed border: `BorderStyle::None` sides render nothing (zero colored pixels).
/// `groove` — две полосы разных оттенков (Edge: `border: 6px groove #808080` =
/// `#2c2c2c` ×3, затем `#d4d4d4` ×3), а не сплошная линия.
#[test]
fn draw_border_groove_paints_two_shades() {
    let grey = Color { r: 0x80, g: 0x80, b: 0x80, a: 255 };
    let cmds = vec![DisplayCommand::DrawBorder {
        rect: rect(2.0, 2.0, 40.0, 40.0),
        widths: [6.0; 4],
        colors: [grey; 4],
        styles: [lumen_layout::BorderStyle::Groove; 4],
        radii: CornerRadii::default(),
    }];
    let img = rasterize_cpu(44, 44, &cmds, &[], 0.0, 0.0).expect("rasterize");
    let r = |y: u32| px(&img, 22, y).0;
    assert_eq!([r(2), r(4), r(5), r(7)], [0x2c, 0x2c, 0xd4, 0xd4]);
    assert_eq!(px(&img, 22, 20), (255, 255, 255, 255), "внутренность не закрашена");
}

#[test]
fn draw_border_none_style_renders_nothing() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let cmds = vec![DisplayCommand::DrawBorder {
        rect: rect(10.0, 10.0, 80.0, 60.0),
        widths: [4.0, 4.0, 4.0, 4.0],
        colors: [red; 4],
        styles: [lumen_layout::BorderStyle::None; 4],
        radii: CornerRadii::default(),
    }];
    let img = rasterize_cpu(200, 100, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // All pixels should remain white.
    let colored = (0..200u32)
        .flat_map(|x| (0..100u32).map(move |y| (x, y)))
        .any(|(x, y)| px(&img, x, y).0 < 200);
    assert!(!colored, "BorderStyle::None must render nothing");
}

/// BUG-175: a uniform solid border with border-radius must follow the radius
/// instead of forming a square frame. The extreme box corner is outside the
/// rounded outline (stays white), the middle of each edge carries the border
/// colour, and the interior (padding box) stays empty (the ring has a hole).
#[test]
fn draw_border_rounded_corner_is_not_square() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    // 60×60 box at (10,10), 6px uniform solid border, radius 20px, no bg.
    let r = 20.0;
    let cmds = vec![DisplayCommand::DrawBorder {
        rect: rect(10.0, 10.0, 60.0, 60.0),
        widths: [6.0, 6.0, 6.0, 6.0],
        colors: [red; 4],
        styles: [lumen_layout::BorderStyle::Solid; 4],
        radii: CornerRadii { tl: r, tl_y: r, tr: r, tr_y: r, br: r, br_y: r, bl: r, bl_y: r },
    }];
    let img = rasterize_cpu(100, 100, &cmds, &[], 0.0, 0.0).expect("rasterize");

    // Extreme top-left box corner: rounded outline curves away → white.
    // Pre-fix (square border) this pixel was solid red.
    let (cr, cg, cb, _) = px(&img, 12, 12);
    assert!(cr > 200 && cg > 200 && cb > 200,
        "rounded corner must be empty, got ({cr},{cg},{cb})");
    // Middle of the top edge sits inside the border band → red.
    let (tr_, tg, tb, _) = px(&img, 40, 12);
    assert!(tr_ > 200 && tg < 60 && tb < 60,
        "top edge must be border colour, got ({tr_},{tg},{tb})");
    // Middle of the left edge → red.
    let (lr, lg, lb, _) = px(&img, 12, 40);
    assert!(lr > 200 && lg < 60 && lb < 60,
        "left edge must be border colour, got ({lr},{lg},{lb})");
    // Interior (padding box centre): ring has a hole → white.
    assert_eq!(px(&img, 40, 40), (255, 255, 255, 255), "interior stays white");
}

/// `mask-image: url(...)` has no decoded source on the deterministic CPU path,
/// so the mask is a no-op (alpha = 1.0 everywhere) — the element composites
/// unchanged, mirroring the GPU fallback for an unregistered mask image.
#[test]
fn mask_image_no_source_is_noop() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![
        DisplayCommand::PushMaskImage {
            rect: rect(10.0, 10.0, 20.0, 20.0),
            src: "missing.png".to_string(),
            size: lumen_layout::BackgroundSize::Auto,
            position: lumen_layout::ObjectPosition::background_initial(),
            repeat: lumen_layout::BackgroundRepeat::NO_REPEAT,
            image_rendering: lumen_layout::ImageRendering::Auto,
        },
        DisplayCommand::FillRect { rect: rect(10.0, 10.0, 20.0, 20.0), color: blue },
        DisplayCommand::PopMask,
    ];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 20, 20), (0, 0, 255, 255), "no mask source → element unchanged");
    assert_eq!(px(&img, 50, 50), (255, 255, 255, 255), "exterior stays white");
}

/// BUG-221: `FillRoundedRect` with a pill radius (`border-radius: 999px`)
/// must clamp to half the box — pre-fix the un-clamped cubic control points
/// shot far outside the rect and filled a giant diagonal wedge over the page.
#[test]
fn fill_rounded_rect_pill_radius_clamped_to_box() {
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![DisplayCommand::FillRoundedRect {
        rect: rect(20.0, 20.0, 100.0, 40.0),
        color: blue,
        radii: CornerRadii {
            tl: 999.0, tl_y: 999.0, tr: 999.0, tr_y: 999.0,
            br: 999.0, br_y: 999.0, bl: 999.0, bl_y: 999.0,
        },
    }];
    let img = rasterize_cpu(200, 100, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Pill centre is filled.
    assert_eq!(px(&img, 70, 40), (0, 0, 255, 255), "pill centre is blue");
    // Far outside the box must stay white — the pre-fix explosion painted here.
    assert_eq!(px(&img, 175, 90), (255, 255, 255, 255), "no fill outside the box");
    // Box corner is rounded away by the pill (radius = half height = 20).
    assert_eq!(px(&img, 21, 21), (255, 255, 255, 255), "pill corner is white");
}

/// BUG-221: `DrawImage` paints the decoded picture (here a solid-red 2×2),
/// not the grey `(217,217,217)` placeholder, when its `src` is supplied in
/// the image map.
#[test]
fn draw_image_paints_decoded_pixels() {
    let red_2x2 = Image {
        width: 2,
        height: 2,
        format: lumen_image::PixelFormat::Rgba8,
        data: [255u8, 0, 0, 255].repeat(4),
        icc_profile: None,
        vector_source: None,
    };
    let images = vec![("red.png".to_string(), std::sync::Arc::new(red_2x2))];
    let cmds = vec![DisplayCommand::DrawImage {
        rect: rect(10.0, 10.0, 40.0, 40.0),
        src: "red.png".to_string(),
        alt: String::new(),
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &images, 0.0, 0.0).expect("rasterize");
    let (r, g, b, a) = px(&img, 30, 30);
    assert!(r > 230 && g < 25 && b < 25 && a == 255, "image centre is red, got ({r},{g},{b},{a})");
}

/// BUG-1337: `DrawImage` на дробной позиции привязан к целым пикселям —
/// без полупрозрачной кромки на верхней/нижней строке.
#[test]
fn draw_image_fractional_rect_is_pixel_snapped() {
    let blue = Image {
        width: 1,
        height: 1,
        format: lumen_image::PixelFormat::Rgba8,
        data: vec![0, 0, 255, 255],
        icc_profile: None,
        vector_source: None,
    };
    let images = vec![("b.png".to_string(), std::sync::Arc::new(blue))];
    let cmds = vec![DisplayCommand::DrawImage {
        rect: rect(0.0, 25.72, 20.0, 50.0),
        src: "b.png".to_string(),
        alt: String::new(),
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(30, 100, &cmds, &images, 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 5, 26), (0, 0, 255, 255), "first row is solid");
    assert_eq!(px(&img, 5, 75), (0, 0, 255, 255), "last row is solid");
    assert_eq!(px(&img, 5, 25), (255, 255, 255, 255), "row above stays white");
    assert_eq!(px(&img, 5, 76), (255, 255, 255, 255), "row below stays white");
}

/// BUG-221: a missing `src` still falls back to the grey placeholder.
#[test]
fn draw_image_missing_src_is_placeholder() {
    let cmds = vec![DisplayCommand::DrawImage {
        rect: rect(10.0, 10.0, 40.0, 40.0),
        src: "absent.png".to_string(),
        alt: String::new(),
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 30, 30), (217, 217, 217, 255), "missing image → grey placeholder");
}

/// FLEX-VWM-6: a `<canvas>` JS never drew into (`canvas:{nid}` unregistered) is
/// transparent, not the `<img>` grey placeholder.
#[test]
fn draw_image_unregistered_canvas_is_transparent() {
    let cmds = vec![DisplayCommand::DrawImage {
        rect: rect(10.0, 10.0, 40.0, 40.0),
        src: "canvas:7".to_string(),
        alt: String::new(),
        object_fit: ObjectFit::Fill,
        object_position: ObjectPosition::default(),
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &[], 0.0, 0.0).expect("rasterize");
    assert_eq!(px(&img, 30, 30), (255, 255, 255, 255), "undrawn canvas shows what is behind it");
}

/// BUG-221: the radial gradient is an isotropic **circle** (mirrors femtovg),
/// not the previous anisotropic ellipse. On a wide box a point the same pixel
/// distance right of centre and below centre must therefore land on the same
/// gradient stop → near-identical colour.
#[test]
fn radial_gradient_is_circular_not_elliptical() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let cmds = vec![DisplayCommand::DrawRadialGradient {
        rect: rect(0.0, 0.0, 120.0, 40.0),
        center_x_pct: 0.5,
        center_y_pct: 0.5,
        // Equal radii → a circle (isotropic), the `circle` keyword case.
        radius_x: 63.25,
        radius_y: 63.25,
        stops: vec![
            GradientStop { color: red, color_space: lumen_layout::ColorSpace::Srgb, position: None },
            GradientStop { color: blue, color_space: lumen_layout::ColorSpace::Srgb, position: None },
        ],
        repeating: false,
    }];
    let img = rasterize_cpu(120, 40, &cmds, &[], 0.0, 0.0).expect("rasterize");
    // Centre (60,20) is the first stop (red).
    let (cr, _, cb, _) = px(&img, 60, 20);
    assert!(cr > 200 && cb < 60, "centre is red, got ({cr},_,{cb},_)");
    // 15px right vs 15px below centre — equal radius on a circle → equal colour.
    let (hr, _, hb, _) = px(&img, 75, 20);
    let (vr, _, vb, _) = px(&img, 60, 35);
    assert!(
        hr.abs_diff(vr) < 16 && hb.abs_diff(vb) < 16,
        "isotropic: horizontal ({hr},{hb}) ≈ vertical ({vr},{vb})"
    );
}

/// BUG-1436: a centre position that overflows f32 (`at 9999…9%`) used to fail
/// the whole `--screenshot` with "degenerate radial gradient"; now the box
/// is filled with the last stop (the WPT ref is a plain green square).
#[test]
fn radial_gradient_with_infinite_centre_does_not_fail_the_snapshot() {
    let green = Color { r: 0, g: 128, b: 0, a: 255 };
    let cmds = vec![DisplayCommand::DrawRadialGradient {
        rect: rect(0.0, 0.0, 40.0, 40.0),
        center_x_pct: f32::INFINITY,
        center_y_pct: f32::INFINITY,
        radius_x: 50.0,
        radius_y: 50.0,
        stops: vec![
            GradientStop { color: green, color_space: lumen_layout::ColorSpace::Srgb, position: None },
            GradientStop { color: green, color_space: lumen_layout::ColorSpace::Srgb, position: None },
        ],
        repeating: true,
    }];
    let img = rasterize_cpu(40, 40, &cmds, &[], 0.0, 0.0).expect("must not error");
    assert_eq!(px(&img, 20, 20), (0, 128, 0, 255), "last stop fills the box");
}

fn solid_2x2(rgba: [u8; 4]) -> Image {
    Image {
        width: 2,
        height: 2,
        format: lumen_image::PixelFormat::Rgba8,
        data: rgba.repeat(4),
        icc_profile: None,
        vector_source: None,
    }
}

/// `DrawBackgroundImage` paints the decoded background (solid-blue 2×2) into
/// its painting area on the CPU path. Pre-fix this command was a no-op, so
/// any `background-image`/`image-set()` rendered blank in `--screenshot`.
#[test]
fn draw_background_image_paints_into_area() {
    let images = vec![("blue.png".to_string(), std::sync::Arc::new(solid_2x2([0, 0, 255, 255])))];
    let area = rect(10.0, 10.0, 40.0, 40.0);
    let cmds = vec![DisplayCommand::DrawBackgroundImage {
        rect: area,
        origin_rect: area,
        src: "blue.png".to_string(),
        size: BackgroundSize::Cover,
        position: ObjectPosition::default(),
        repeat: BackgroundRepeat::NO_REPEAT,
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &images, 0.0, 0.0).expect("rasterize");
    let (r, g, b, a) = px(&img, 30, 30);
    assert!(b > 230 && r < 25 && g < 25 && a == 255, "bg centre is blue, got ({r},{g},{b},{a})");
    // The single `cover` tile is clipped to the painting area — outside stays white.
    assert_eq!(px(&img, 55, 55), (255, 255, 255, 255), "outside painting area is white");
}

/// BUG-1296: an SVG background is redrawn at the tile's device size, so the
/// boundary between two stacked rects stays a hard edge instead of the
/// bilinear gradient band a stretched 8×32 raster gives.
#[test]
fn draw_background_image_svg_is_rerasterized_at_tile_size() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="32">
            <rect width="8" height="16" fill="lime"/><rect y="16" width="8" height="16" fill="aqua"/></svg>"#;
    let decoded = lumen_image::decode_svg(svg).expect("svg");
    let images = vec![("a.svg".to_string(), std::sync::Arc::new(decoded))];
    let area = rect(0.0, 0.0, 100.0, 100.0);
    let cmds = vec![DisplayCommand::DrawBackgroundImage {
        rect: area,
        origin_rect: area,
        src: "a.svg".to_string(),
        size: BackgroundSize::Contain,
        position: ObjectPosition::default(),
        repeat: BackgroundRepeat::NO_REPEAT,
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(100, 100, &cmds, &images, 0.0, 0.0).expect("rasterize");
    // contain → 25×100 tile at x 37.5..62.5; the lime/aqua edge is at y = 50.
    for y in [0, 25, 49] {
        assert_eq!(px(&img, 50, y), (0, 255, 0, 255), "lime row {y}");
    }
    for y in [50, 75, 99] {
        assert_eq!(px(&img, 50, y), (0, 255, 255, 255), "aqua row {y}");
    }
}

/// `DrawBackgroundImage` with `repeat` tiles a small image across the whole
/// painting area (geometry shared with the femtovg backend).
#[test]
fn draw_background_image_tiles_on_repeat() {
    let images = vec![("g.png".to_string(), std::sync::Arc::new(solid_2x2([0, 200, 0, 255])))];
    let area = rect(0.0, 0.0, 64.0, 64.0);
    let cmds = vec![DisplayCommand::DrawBackgroundImage {
        rect: area,
        origin_rect: area,
        src: "g.png".to_string(),
        // Auto size = 2×2 intrinsic; Repeat fills the box with tiles.
        size: BackgroundSize::Auto,
        position: ObjectPosition::default(),
        repeat: BackgroundRepeat::REPEAT,
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &images, 0.0, 0.0).expect("rasterize");
    // Both a near-origin and a far corner pixel are painted green (tiled).
    for (x, y) in [(3u32, 3u32), (60, 60)] {
        let (r, g, b, _) = px(&img, x, y);
        assert!(g > 150 && r < 60 && b < 60, "tile at ({x},{y}) green, got ({r},{g},{b})");
    }
}

/// BUG-1303: with a positioning area smaller than the painting area
/// (`background-origin: padding-box` under a border), the repeating grid
/// still covers the strip before the origin box.
#[test]
fn background_repeat_covers_strip_before_origin_box() {
    let images = vec![("g.png".to_string(), std::sync::Arc::new(solid_2x2([0, 200, 0, 255])))];
    let cmds = vec![DisplayCommand::DrawBackgroundImage {
        rect: rect(0.0, 0.0, 64.0, 64.0),
        origin_rect: rect(10.0, 10.0, 44.0, 44.0),
        src: "g.png".to_string(),
        size: BackgroundSize::Auto,
        position: ObjectPosition::default(),
        repeat: BackgroundRepeat::REPEAT,
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 64, &cmds, &images, 0.0, 0.0).expect("rasterize");
    for (x, y) in [(3u32, 3u32), (3, 30), (30, 3), (60, 60)] {
        let (r, g, b, _) = px(&img, x, y);
        assert!(g > 150 && r < 60 && b < 60, "tile at ({x},{y}) green, got ({r},{g},{b})");
    }
}

/// BUG-1343: tiles at a half-pixel offset must not leave AA seams.
#[test]
fn background_tiles_at_fractional_position_have_no_seams() {
    let images = vec![("b.png".to_string(), std::sync::Arc::new(solid_2x2([0, 0, 255, 255])))];
    let area = rect(0.0, 0.0, 64.0, 8.0);
    let cmds = vec![DisplayCommand::DrawBackgroundImage {
        rect: area,
        origin_rect: area,
        src: "b.png".to_string(),
        size: BackgroundSize::Length(lumen_layout::BgSizeAxis::Px(15.0), lumen_layout::BgSizeAxis::Px(8.0)),
        position: ObjectPosition { x: lumen_layout::PositionComponent::Px(2.5), y: lumen_layout::PositionComponent::Px(0.0) },
        repeat: BackgroundRepeat::REPEAT_X,
        image_rendering: lumen_layout::ImageRendering::Auto,
    }];
    let img = rasterize_cpu(64, 8, &cmds, &images, 0.0, 0.0).expect("rasterize");
    for x in 4..60 {
        let (r, g, b, _) = px(&img, x, 4);
        assert!(b > 250 && r < 5 && g < 5, "seam at x={x}: ({r},{g},{b})");
    }
}

/// `DrawCrossFade` blends two images by `progress` on the CPU path: at p=0
/// only `src_a` shows, at p=1 only `src_b`. Pre-fix the command was unhandled.
#[test]
fn draw_cross_fade_respects_progress() {
    let images = vec![
        ("a.png".to_string(), std::sync::Arc::new(solid_2x2([255, 0, 0, 255]))),
        ("b.png".to_string(), std::sync::Arc::new(solid_2x2([0, 255, 0, 255]))),
    ];
    let mk = |p: f32| {
        vec![DisplayCommand::DrawCrossFade {
            dest: rect(10.0, 10.0, 40.0, 40.0),
            src_a: "a.png".to_string(),
            src_b: "b.png".to_string(),
            progress: p,
        }]
    };
    let img0 = rasterize_cpu(64, 64, &mk(0.0), &images, 0.0, 0.0).expect("rasterize");
    let (r, g, b, _) = px(&img0, 30, 30);
    assert!(r > 230 && g < 25 && b < 25, "p=0 → red, got ({r},{g},{b})");
    let img1 = rasterize_cpu(64, 64, &mk(1.0), &images, 0.0, 0.0).expect("rasterize");
    let (r, g, b, _) = px(&img1, 30, 30);
    assert!(g > 230 && r < 25 && b < 25, "p=1 → green, got ({r},{g},{b})");
}

/// BUG-101: середина — единственная точка, где виден дефект композита, и
/// именно её `draw_cross_fade_respects_progress` не проверял: на p=0 и p=1
/// «`a` с альфой `1−p`» случайно совпадает с верным результатом.
///
/// `cross-fade(red, green, 50%)` обязан дать ровно половину каждого цвета
/// при непрозрачном результате. Старый порядок (`a` при `1−p`, затем `b`
/// при `p`) давал `p·b + (1−p)²·a + p(1−p)·фон` = зелёный перевес (≈85,170,0)
/// и дыру в альфе, сквозь которую просвечивал фон.
#[test]
fn draw_cross_fade_midpoint_is_even_mix() {
    let images = vec![
        ("a.png".to_string(), std::sync::Arc::new(solid_2x2([255, 0, 0, 255]))),
        ("b.png".to_string(), std::sync::Arc::new(solid_2x2([0, 255, 0, 255]))),
    ];
    let dl = vec![DisplayCommand::DrawCrossFade {
        dest: rect(10.0, 10.0, 40.0, 40.0),
        src_a: "a.png".to_string(),
        src_b: "b.png".to_string(),
        progress: 0.5,
    }];
    let img = rasterize_cpu(64, 64, &dl, &images, 0.0, 0.0).expect("rasterize");
    let (r, g, b, a) = px(&img, 30, 30);
    assert_eq!(a, 255, "результат cross-fade непрозрачен, got alpha {a}");
    assert!(b < 10, "синего в смеси красного с зелёным нет, got ({r},{g},{b})");
    let delta = i32::from(r) - i32::from(g);
    assert!(
        delta.abs() <= 8,
        "p=0.5 → равные доли красного и зелёного, got ({r},{g},{b})"
    );
}

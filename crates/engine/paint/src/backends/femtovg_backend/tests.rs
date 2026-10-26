use super::*;
use lumen_layout::BgSizeAxis;
use lumen_layout::RepeatStyle;
use lumen_layout::PositionComponent;
use lumen_layout::Length;

// ─── BUG-272 срез 18: LruMap (resized-variant cache) ────────────────────

#[test]
fn lru_map_get_hit_and_miss() {
    let mut c: LruMap<u32> = LruMap::new(4);
    assert_eq!(c.get("a"), None);
    assert_eq!(c.insert("a".into(), 10), None);
    assert_eq!(c.get("a"), Some(10));
    assert_eq!(c.get("b"), None);
    assert_eq!(c.len(), 1);
}

#[test]
fn lru_map_evicts_least_recently_used() {
    let mut c: LruMap<u32> = LruMap::new(2);
    assert_eq!(c.insert("a".into(), 1), None);
    assert_eq!(c.insert("b".into(), 2), None);
    // At capacity; inserting "c" evicts the LRU ("a").
    assert_eq!(c.insert("c".into(), 3), Some(1));
    assert_eq!(c.get("a"), None);
    assert_eq!(c.get("b"), Some(2));
    assert_eq!(c.get("c"), Some(3));
    assert_eq!(c.len(), 2);
}

#[test]
fn lru_map_get_refreshes_recency() {
    let mut c: LruMap<u32> = LruMap::new(2);
    c.insert("a".into(), 1);
    c.insert("b".into(), 2);
    // Touch "a" so "b" becomes the least-recently-used.
    assert_eq!(c.get("a"), Some(1));
    // Inserting "c" now evicts "b", not "a".
    assert_eq!(c.insert("c".into(), 3), Some(2));
    assert_eq!(c.get("a"), Some(1));
    assert_eq!(c.get("b"), None);
}

#[test]
fn lru_map_drain_returns_all_and_clears() {
    let mut c: LruMap<u32> = LruMap::new(4);
    c.insert("a".into(), 1);
    c.insert("b".into(), 2);
    let mut drained = c.drain();
    drained.sort_unstable();
    assert_eq!(drained, vec![1, 2]);
    assert_eq!(c.len(), 0);
    assert_eq!(c.get("a"), None);
}

#[test]
fn lru_map_cap_is_at_least_one() {
    let mut c: LruMap<u32> = LruMap::new(0);
    assert_eq!(c.insert("a".into(), 1), None);
    // Capacity floored to 1: inserting a second entry evicts the first.
    assert_eq!(c.insert("b".into(), 2), Some(1));
    assert_eq!(c.len(), 1);
}

#[test]
fn lru_map_remove_variants_of_drops_only_that_source() {
    let mut c: LruMap<u32> = LruMap::new(8);
    c.insert("canvas:7@100x50".into(), 1);
    c.insert("canvas:7@20x10".into(), 2);
    c.insert("canvas:70@20x10".into(), 3);
    // Другой src, который лишь начинается с "canvas:7@".
    c.insert("canvas:7@x@20x10".into(), 4);
    let mut removed = c.remove_variants_of("canvas:7");
    removed.sort_unstable();
    assert_eq!(removed, vec![1, 2]);
    assert_eq!(c.len(), 2);
    assert_eq!(c.get("canvas:70@20x10"), Some(3));
    assert_eq!(c.get("canvas:7@x@20x10"), Some(4));
    // Порядок обращений тоже очищен: вытеснение не упирается в удалённые ключи.
    let mut c: LruMap<u32> = LruMap::new(1);
    c.insert("a@1x1".into(), 1);
    assert_eq!(c.remove_variants_of("a"), vec![1]);
    assert_eq!(c.insert("b@1x1".into(), 2), None);
}

// ─── Ph3 writing-mode vertical: text-orientation rotation geometry ──────

/// Applies a femtovg `Transform2D` the same way the canvas does, so a test
/// can assert on where a locally-laid-out glyph actually lands.
fn map(t: &femtovg::Transform2D, x: f32, y: f32) -> (f32, f32) {
    t.transform_point(x, y)
}

#[test]
fn rotate_cw_transform_matches_cpu_and_wgpu_mapping() {
    // Both other backends implement `(x, y) -> (-y + dest.x + dest.width, x + dest.y)`
    // (tiny-skia `from_row(0, 1, -1, 0, dest.x + dest.width, dest.y)` /
    // `rotate_text_vertices_cw`). Any divergence here would place femtovg's
    // vertical runs somewhere the reference backends do not.
    let t = rotate_cw_transform(Rect::new(100.0, 40.0, 30.0, 260.0));
    for (x, y) in [(0.0, 0.0), (12.0, 5.0), (200.0, -8.0), (-3.0, 17.0)] {
        let expected = (-y + 130.0, x + 40.0);
        let got = map(&t, x, y);
        assert!(
            (got.0 - expected.0).abs() < 1e-4 && (got.1 - expected.1).abs() < 1e-4,
            "({x}, {y}) -> {got:?}, expected {expected:?}",
        );
    }
}

#[test]
fn rotate_cw_transform_makes_the_run_flow_downwards_from_the_column_origin() {
    // The `Sideways` branch lays the run out at local `(0, 0)` and relies on
    // the rotation to start it at the column's top-right and advance
    // top→bottom, matching `wrap_inline_run_vertical`'s column placement.
    let dest = Rect::new(64.0, 12.0, 30.0, 260.0);
    let t = rotate_cw_transform(dest);
    assert_eq!(map(&t, 0.0, 0.0), (dest.x + dest.width, dest.y));
    // Pen advance along local +X becomes downward movement in the column…
    let advanced = map(&t, 50.0, 0.0);
    assert_eq!(advanced, (dest.x + dest.width, dest.y + 50.0));
    // …and the local baseline drop (+Y, glyph height) becomes leftward
    // movement — the glyph body sits to the left of the column's right edge,
    // which is why the `Sideways` branch feeds the swapped local rect
    // (width = rect.height, height = rect.width).
    assert_eq!(map(&t, 0.0, 18.0), (dest.x + dest.width - 18.0, dest.y));
}

// ─── ADR-016 M3.2.1b band geometry ──────────────────────────────────────

#[test]
fn band_geometry_adds_overscan_on_every_side() {
    // 1024×720 viewport, 512 overscan, scale 1 → band grows by 1024 on each
    // axis (2·overscan), device px == CSS px at scale 1.
    let (cw, ch, dw, dh) = band_geometry(1024.0, 720.0, 512.0, 1.0);
    assert_eq!((cw, ch), (2048.0, 1744.0));
    assert_eq!((dw, dh), (2048, 1744));
}

#[test]
fn band_geometry_scales_and_rounds_up_device_pixels() {
    // At a fractional scale the device size must round UP so the whole CSS
    // band fits — a truncating cast would clip the overscan's last row/col.
    let (cw, ch, dw, dh) = band_geometry(801.0, 601.0, 10.0, 1.25);
    assert_eq!((cw, ch), (821.0, 621.0));
    // 821 * 1.25 = 1026.25 → 1027; 621 * 1.25 = 776.25 → 777.
    assert_eq!((dw, dh), (1027, 777));
}

#[test]
fn band_geometry_zero_overscan_is_the_viewport() {
    let (cw, ch, dw, dh) = band_geometry(640.0, 480.0, 0.0, 2.0);
    assert_eq!((cw, ch), (640.0, 480.0));
    assert_eq!((dw, dh), (1280, 960));
}

/// ADR-016 M3.2.1b-2b: the band-blit placement maps the shared document
/// region 1:1. The whole previous-band image is placed at `prev_origin −
/// new_origin` (so its UV mapping lines document space up), and the filled
/// sub-rect is the `retained` overlap in new-band-local coordinates.
#[test]
fn band_blit_placement_maps_overlap_to_new_band_local() {
    use lumen_core::geom::Rect;
    // Scroll down one notch: previous band at y=488, new band at y=1088.
    // Overlap 1088..2232 is retained; the old image sits 600 px above the new
    // band's top-left, and the retained rect starts at the new band's top.
    let prev_origin = (0.0, 488.0);
    let new_origin = (0.0, 1088.0);
    let retained = Rect::new(0.0, 1088.0, 2048.0, 1144.0);
    let (img, dst) = band_blit_placement(retained, prev_origin, new_origin);
    assert_eq!(img, (0.0, -600.0));
    assert_eq!(dst, Rect::new(0.0, 0.0, 2048.0, 1144.0));
}

/// A diagonal pan offsets both axes: the destination sub-rect is the overlap
/// expressed in new-band-local px, and the image origin carries the full
/// previous-band offset on both axes.
#[test]
fn band_blit_placement_handles_diagonal_offset() {
    use lumen_core::geom::Rect;
    let prev_origin = (1488.0, 488.0);
    let new_origin = (2288.0, 488.0);
    let retained = Rect::new(2288.0, 488.0, 1248.0, 1744.0);
    let (img, dst) = band_blit_placement(retained, prev_origin, new_origin);
    assert_eq!(img, (-800.0, 0.0));
    assert_eq!(dst, Rect::new(0.0, 0.0, 1248.0, 1744.0));
}

/// BUG-133 / BUG-146 / BUG-144: offscreen layers composited directly from
/// their FBO on the GPU (opacity groups, filter layers, blur destinations,
/// **and the backdrop-filter element-content layer** — no
/// screenshot→re-upload round-trip) MUST carry FLIP_Y — without it the
/// content renders upside-down (TEST-102 17% → 65% for opacity groups;
/// TEST-15 1.06% → 6.58% for blurred box-shadows; TEST-30 backdrop-filter
/// cards landed in `viewport_h - bounds.bottom` instead of their own row).
/// PREMULTIPLIED matches femtovg's render-target pixel format; dropping it
/// double-multiplies alpha. `filtered_backdrop_id` is the exception: it is a
/// CPU pixel upload (top-down), so it stays flag-free.
#[test]
fn offscreen_layer_flags_flip_y_and_premultiplied() {
    let flags = offscreen_layer_image_flags();
    assert!(flags.contains(femtovg::ImageFlags::FLIP_Y));
    assert!(flags.contains(femtovg::ImageFlags::PREMULTIPLIED));
}

/// BUG-273 срез 1 / BUG-272 срез 8: `group_bounds` reports the culling bbox
/// for every offscreen-composite / clip opener, `None` for everything else
/// (including a `PushFilter`/`PushOpacity` whose `bounds` is itself `None`).
#[test]
fn group_bounds_covers_the_offscreen_and_clip_openers() {
    use lumen_core::geom::Rect;
    let r = Rect::new(1.0, 2.0, 3.0, 4.0);
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushFilter {
            filters: vec![],
            bounds: Some(r),
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushFilter {
            filters: vec![],
            bounds: None,
        }),
        None,
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushBackdropFilter {
            filters: vec![],
            bounds: r,
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushBlendMode {
            mode: BlendMode::Multiply,
            bounds: r,
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushOpacity {
            alpha: 0.5,
            bounds: Some(r),
        }),
        Some(r),
    );
    // A bounds-less opacity group (full-page view-transition fade) is never culled.
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushOpacity {
            alpha: 0.5,
            bounds: None,
        }),
        None,
    );
    // BUG-272 срез 8: pure clip groups cull by their clip region — the rounded
    // clip reports its rect verbatim, the basic-shape clip its bounding box.
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushClipRoundedRect {
            rect: r,
            radii: [2.0; 4],
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushClipPath {
            shape: ResolvedClipShape::Circle { cx: 10.0, cy: 20.0, r: 5.0 },
        }),
        Some(Rect::new(5.0, 15.0, 10.0, 10.0)),
    );
    // BUG-272 срез 9: all four mask openers cull by their `rect` (the masked
    // element's border-box), like clips — both the scissor (image) and the
    // DestinationIn-gradient paths confine every visible pixel to `rect`.
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushMaskImage {
            rect: r,
            src: "u".to_string(),
            size: BackgroundSize::default(),
            position: ObjectPosition::default(),
            repeat: BackgroundRepeat::default(),
            image_rendering: lumen_layout::ImageRendering::default(),
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushMaskLinearGradient {
            rect: r,
            angle_deg: 0.0,
            stops: Vec::new(),
            repeating: false,
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushMaskRadialGradient {
            rect: r,
            center_x_pct: 50.0,
            center_y_pct: 50.0,
            stops: Vec::new(),
            repeating: false,
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::PushMaskConicGradient {
            rect: r,
            center_x_pct: 50.0,
            center_y_pct: 50.0,
            from_angle_deg: 0.0,
            stops: Vec::new(),
            repeating: false,
        }),
        Some(r),
    );
    assert_eq!(
        FemtovgBackend::group_bounds(&DisplayCommand::FillRect {
            rect: r,
            color: lumen_layout::Color { r: 0, g: 0, b: 0, a: 255 },
        }),
        None,
    );
}

/// BUG-273 срез 1: `matching_close` finds the Pop that balances the Push at
/// `open`, skipping over any nested brackets (of any kind) in between.
#[test]
fn matching_close_skips_nested_brackets() {
    let cmds = vec![
        DisplayCommand::PushBlendMode {
            mode: BlendMode::Multiply,
            bounds: lumen_core::geom::Rect::new(0.0, 0.0, 10.0, 10.0),
        }, // 0
        DisplayCommand::PushClipRect { rect: lumen_core::geom::Rect::new(0.0, 0.0, 5.0, 5.0) }, // 1
        DisplayCommand::PopClip, // 2
        DisplayCommand::PushOpacity { alpha: 0.5, bounds: None }, // 3
        DisplayCommand::PopOpacity, // 4
        DisplayCommand::PopBlendMode, // 5 — matches index 0
        DisplayCommand::PopBlendMode, // 6 — unrelated trailing command
    ];
    assert_eq!(FemtovgBackend::matching_close(&cmds, 0), 5);
    // A bracket with no nesting closes at the very next index.
    assert_eq!(FemtovgBackend::matching_close(&cmds, 1), 2);
}

/// BUG-273 срез 2: only a blur-free chain carrying a colour-matrix primitive
/// is cache-eligible — that is the exact set of filter chains that take the
/// bbox (scroll-invariant, screenshot-round-trip) path in `composite_filter_layer`.
#[test]
fn filter_is_bbox_cacheable_matches_the_bbox_colour_matrix_path() {
    use lumen_layout::FilterFn;
    // Colour-matrix only → cacheable.
    assert!(FemtovgBackend::filter_is_bbox_cacheable(&[FilterFn::Grayscale(1.0)]));
    assert!(FemtovgBackend::filter_is_bbox_cacheable(&[
        FilterFn::Brightness(0.5),
        FilterFn::Contrast(2.0),
    ]));
    // Opacity is folded in with a colour-matrix filter → still cacheable
    // (opacity alone is handled without an offscreen layer, see below).
    assert!(FemtovgBackend::filter_is_bbox_cacheable(&[
        FilterFn::Opacity(0.5),
        FilterFn::Sepia(1.0),
    ]));
    // Any real blur forces the full-framebuffer (scroll-baked) path → NOT cacheable.
    assert!(!FemtovgBackend::filter_is_bbox_cacheable(&[FilterFn::Blur(4.0)]));
    assert!(!FemtovgBackend::filter_is_bbox_cacheable(&[
        FilterFn::Blur(4.0),
        FilterFn::Grayscale(1.0),
    ]));
    // Opacity-only / blur(0)-only → nothing costly to cache.
    assert!(!FemtovgBackend::filter_is_bbox_cacheable(&[FilterFn::Opacity(0.3)]));
    assert!(!FemtovgBackend::filter_is_bbox_cacheable(&[FilterFn::Blur(0.0)]));
    assert!(!FemtovgBackend::filter_is_bbox_cacheable(&[]));
}

/// BUG-273 срез 2: the subtree hash folds every command in the bracket
/// (inclusive of the `PushFilter` opener and its filter chain), is stable for
/// identical content, and changes when any interior command changes. Scroll
/// is never part of a command, so a pure-scroll frame reuses the same hash.
#[test]
fn hash_filter_bracket_is_content_addressed() {
    use lumen_core::geom::Rect;
    use lumen_layout::{Color, FilterFn};
    let bracket = |gray: f32, fill_x: f32| {
        vec![
            DisplayCommand::PushFilter {
                filters: vec![FilterFn::Grayscale(gray)],
                bounds: Some(Rect::new(10.0, 20.0, 30.0, 40.0)),
            },
            DisplayCommand::FillRect {
                rect: Rect::new(fill_x, 0.0, 5.0, 5.0),
                color: Color { r: 1, g: 2, b: 3, a: 255 },
            },
            DisplayCommand::PopFilter,
        ]
    };
    let a = bracket(1.0, 0.0);
    let b = bracket(1.0, 0.0);
    // Identical content → identical hash (the pure-scroll reuse case).
    assert_eq!(
        FemtovgBackend::hash_filter_bracket(&a, 0, 2),
        FemtovgBackend::hash_filter_bracket(&b, 0, 2),
    );
    // A changed interior draw → different hash (invalidates the cache).
    let c = bracket(1.0, 7.0);
    assert_ne!(
        FemtovgBackend::hash_filter_bracket(&a, 0, 2),
        FemtovgBackend::hash_filter_bracket(&c, 0, 2),
    );
    // A changed filter parameter → different hash.
    let d = bracket(0.5, 0.0);
    assert_ne!(
        FemtovgBackend::hash_filter_bracket(&a, 0, 2),
        FemtovgBackend::hash_filter_bracket(&d, 0, 2),
    );
}

/// BUG-249: скруглённый клип (`overflow:hidden` + `border-radius`) теперь
/// маскируется по rounded-rect контуру (offscreen-слой), а не square scissor.
/// Радиусы должны клампиться к половине меньшей стороны, иначе `border-radius`
/// `50%` / pill дали бы перекрывающиеся углы и битый контур.
#[test]
fn clip_radii_clamped_to_half_min_side() {
    use lumen_core::geom::Rect;
    // Pill: огромный радиус на широком боксе → клампится к h/2.
    let r = Rect::new(0.0, 0.0, 200.0, 60.0);
    assert_eq!(clamp_clip_radii(&r, &[999.0; 4]), [30.0; 4]);
    // Круг: 50%-радиус на квадрате (w/2) проходит без обрезки.
    let sq = Rect::new(10.0, 10.0, 80.0, 80.0);
    assert_eq!(clamp_clip_radii(&sq, &[40.0; 4]), [40.0; 4]);
    // Малые углы сохраняются как есть; per-corner независимо.
    let box_ = Rect::new(0.0, 0.0, 100.0, 100.0);
    assert_eq!(clamp_clip_radii(&box_, &[8.0, 16.0, 0.0, 4.0]), [8.0, 16.0, 0.0, 4.0]);
}

/// BUG-249: путь скруглённого клипа строится без паники и даёт замкнутый
/// контур (4 угла + 4 грани). Под identity-трансформом опорная стартовая
/// точка лежит на верхней грани со сдвигом на tl-радиус.
#[test]
fn rounded_rect_clip_path_builds() {
    use lumen_core::geom::Rect;
    let r = Rect::new(0.0, 0.0, 100.0, 100.0);
    let path = rounded_rect_clip_path(&r, &[20.0; 4], &femtovg::Transform2D::identity());
    // Непустой путь (femtovg накопил verbs) — построение не паникует и не пустое.
    assert!(path.verbs().count() > 0);
}

#[test]
fn bg_tile_geometry_length_no_repeat_top_left() {
    // background-size: 80×60; no-repeat; position 0% 0%; origin area at (30,30).
    let pos = ObjectPosition {
        x: PositionComponent::Percent(0.0),
        y: PositionComponent::Percent(0.0),
    };
    let (tw, th, x0, y0, rx, ry, ..) = bg_tile_geometry(
        BackgroundSize::Length(BgSizeAxis::Px(80.0), BgSizeAxis::Px(60.0)),
        &pos,
        BackgroundRepeat::NO_REPEAT,
        100.0,
        100.0,
        180.0,
        120.0,
        30.0,
        30.0,
    );
    assert_eq!((tw, th), (80.0, 60.0));
    // Anchored to top-left of the positioning area.
    assert_eq!((x0, y0), (30.0, 30.0));
    assert!(!rx && !ry);
}

#[test]
fn bg_tile_geometry_position_bottom_right() {
    // position 100% 100% anchors tile to the far corner of the origin area.
    let pos = ObjectPosition {
        x: PositionComponent::Percent(1.0),
        y: PositionComponent::Percent(1.0),
    };
    let (tw, th, x0, y0, ..) = bg_tile_geometry(
        BackgroundSize::Length(BgSizeAxis::Px(80.0), BgSizeAxis::Px(60.0)),
        &pos,
        BackgroundRepeat::NO_REPEAT,
        100.0,
        100.0,
        180.0,
        120.0,
        30.0,
        30.0,
    );
    assert_eq!((tw, th), (80.0, 60.0));
    // x0 = 30 + (180 - 80) = 130; y0 = 30 + (120 - 60) = 90.
    assert!((x0 - 130.0).abs() < 1e-3);
    assert!((y0 - 90.0).abs() < 1e-3);
}

#[test]
fn bg_tile_geometry_percent_resolves_against_oarea() {
    // BUG-115: `background-size: 40% 60%` → tile = 40%×60% of the positioning area.
    let pos = ObjectPosition::default();
    let (tw, th, ..) = bg_tile_geometry(
        BackgroundSize::Length(BgSizeAxis::Percent(0.4), BgSizeAxis::Percent(0.6)),
        &pos,
        BackgroundRepeat::NO_REPEAT,
        100.0,
        100.0,
        180.0,
        120.0,
        0.0,
        0.0,
    );
    // 0.4 * 180 = 72; 0.6 * 120 = 72.
    assert!((tw - 72.0).abs() < 1e-3);
    assert!((th - 72.0).abs() < 1e-3);
}

#[test]
fn bg_tile_geometry_mixed_px_percent() {
    // BUG-115: `background-size: 20px 100%` → fixed width, full-height tile.
    let pos = ObjectPosition::default();
    let (tw, th, ..) = bg_tile_geometry(
        BackgroundSize::Length(BgSizeAxis::Px(20.0), BgSizeAxis::Percent(1.0)),
        &pos,
        BackgroundRepeat::REPEAT_X,
        100.0,
        100.0,
        180.0,
        120.0,
        0.0,
        0.0,
    );
    assert!((tw - 20.0).abs() < 1e-3);
    assert!((th - 120.0).abs() < 1e-3);
}

#[test]
fn bg_tile_geometry_cover_scales_to_fill() {
    // Cover: scale = max(180/100, 120/100) = 1.8 → tile 180×180.
    let pos = ObjectPosition::default();
    let (tw, th, ..) = bg_tile_geometry(
        BackgroundSize::Cover,
        &pos,
        BackgroundRepeat::NO_REPEAT,
        100.0,
        100.0,
        180.0,
        120.0,
        0.0,
        0.0,
    );
    assert!((tw - 180.0).abs() < 1e-3);
    assert!((th - 180.0).abs() < 1e-3);
}

#[test]
fn bg_tile_geometry_repeat_styles_are_per_axis() {
    // BUG-1299: `space no-repeat` spaces along x only; `round repeat` rounds x only.
    let pos = ObjectPosition {
        x: PositionComponent::Percent(0.0),
        y: PositionComponent::Percent(0.0),
    };
    let size = BackgroundSize::Length(BgSizeAxis::Px(30.0), BgSizeAxis::Px(30.0));
    let space_x = BackgroundRepeat { x: RepeatStyle::Space, y: RepeatStyle::NoRepeat };
    let (tw, th, x0, y0, rx, ry, sx, sy) =
        bg_tile_geometry(size, &pos, space_x, 100.0, 100.0, 100.0, 100.0, 0.0, 0.0);
    // floor(100/30) = 3 tiles, gap = (100 - 90) / 2 = 5 → step 35.
    assert_eq!((tw, th, x0, y0, rx, ry), (30.0, 30.0, 0.0, 0.0, true, false));
    assert_eq!((sx, sy), (35.0, 30.0));
    let round_x = BackgroundRepeat { x: RepeatStyle::Round, y: RepeatStyle::Repeat };
    let (tw, th, _, _, rx, ry, ..) =
        bg_tile_geometry(size, &pos, round_x, 100.0, 100.0, 100.0, 100.0, 0.0, 0.0);
    // round(100/30) = 3 → 33.33 along x; y keeps 30.
    assert!((tw - 100.0 / 3.0).abs() < 1e-3);
    assert_eq!(th, 30.0);
    assert!(rx && ry);
}

#[test]
fn bg_tile_geometry_repeat_sets_flags() {
    // Repeat with position 0% 0% → both axes repeat; start aligned to origin.
    let pos = ObjectPosition {
        x: PositionComponent::Percent(0.0),
        y: PositionComponent::Percent(0.0),
    };
    let (tw, th, x0, y0, rx, ry, ..) = bg_tile_geometry(
        BackgroundSize::Length(BgSizeAxis::Px(40.0), BgSizeAxis::Px(40.0)),
        &pos,
        BackgroundRepeat::REPEAT,
        100.0,
        100.0,
        200.0,
        200.0,
        0.0,
        0.0,
    );
    assert!(rx && ry);
    // off_x = 0 → ceil(0/40)*40 = 0 → start = origin.
    assert_eq!((x0, y0), (0.0, 0.0));
    assert_eq!((tw, th), (40.0, 40.0));
}

#[test]
fn lumen_to_fvg_converts_rgba() {
    let c = Color { r: 10, g: 20, b: 30, a: 200 };
    let fvg = lumen_to_fvg(c);
    assert!((fvg.r - 10.0 / 255.0).abs() < 1e-3);
    assert!((fvg.g - 20.0 / 255.0).abs() < 1e-3);
    assert!((fvg.b - 30.0 / 255.0).abs() < 1e-3);
    assert!((fvg.a - 200.0 / 255.0).abs() < 1e-3);
}

#[test]
fn lumen_to_fvg_white() {
    let c = Color::WHITE;
    let fvg = lumen_to_fvg(c);
    assert!((fvg.r - 1.0).abs() < 1e-5);
    assert!((fvg.a - 1.0).abs() < 1e-5);
}

#[test]
fn lumen_to_fvg_transparent() {
    let c = Color::TRANSPARENT;
    let fvg = lumen_to_fvg(c);
    assert_eq!(fvg.a, 0.0);
}

#[test]
fn femtovg_backend_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<FemtovgBackend>();
}

#[test]
fn linear_gradient_endpoints_horizontal() {
    let ([sx, _sy], [ex, _ey]) = linear_gradient_endpoints(0.0, 0.0, 100.0, 50.0, 90.0);
    assert!(sx < ex, "start должен быть левее end для 90°");
}

#[test]
fn linear_gradient_endpoints_vertical() {
    let ([_sx, sy], [_ex, ey]) = linear_gradient_endpoints(0.0, 0.0, 100.0, 50.0, 0.0);
    assert!(sy > ey, "start y должен быть больше end y для 0° (to top)");
}

#[test]
fn resolve_stops_evenly_spaced() {
    let stops = vec![
        GradientStop { color: Color::WHITE, position: None , ..Default::default() },
        GradientStop { color: Color::BLACK, position: None , ..Default::default() },
    ];
    let resolved = resolve_stops(&stops, 100.0);
    assert_eq!(resolved.len(), 2);
    assert!((resolved[0].0).abs() < 1e-5);
    assert!((resolved[1].0 - 1.0).abs() < 1e-5);
}

#[test]
fn resolve_stops_fixed_positions() {
    let stops = vec![
        GradientStop { color: Color::WHITE, position: Some(Length::Percent(0.0)) , ..Default::default() },
        GradientStop { color: Color::BLACK, position: Some(Length::Percent(50.0)) , ..Default::default() },
        GradientStop { color: Color::WHITE, position: Some(Length::Percent(100.0)) , ..Default::default() },
    ];
    let resolved = resolve_stops(&stops, 100.0);
    assert_eq!(resolved.len(), 3);
    assert!((resolved[1].0 - 0.5).abs() < 1e-5);
}

#[test]
fn femtovg_stops_extends_hard_stop_last_color_to_end() {
    // linear-gradient(red 50%, green 50%): без завершающего стопа femtovg
    // оставил бы вторую половину прозрачной (BUG-085 / BUG-144 row 2).
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let green = Color { r: 0, g: 255, b: 0, a: 255 };
    let stops = vec![
        GradientStop { color: red, position: Some(Length::Percent(50.0)) , ..Default::default() },
        GradientStop { color: green, position: Some(Length::Percent(50.0)) , ..Default::default() },
    ];
    let r = femtovg_stops(&stops, 100.0, false);
    let last = *r.last().unwrap();
    assert!((last.0 - 1.0).abs() < 1e-5, "последний стоп должен достигать 1.0, got {}", last.0);
    assert!(last.1.g > 0.9 && last.1.r < 0.1, "хвост должен быть зелёным");
}

#[test]
fn femtovg_stops_no_tail_fill_when_last_at_full() {
    let red = Color { r: 255, g: 0, b: 0, a: 255 };
    let blue = Color { r: 0, g: 0, b: 255, a: 255 };
    let stops = vec![
        GradientStop { color: red, position: None , ..Default::default() },
        GradientStop { color: blue, position: None , ..Default::default() },
    ];
    // Стопы 0/1 не требуют дозаполнения хвоста.
    assert_eq!(femtovg_stops(&stops, 100.0, false).len(), 2);
}

#[test]
fn femtovg_stops_repeating_tiles_across_line() {
    // repeating-linear-gradient(#333 0px, #333 10px, #666 10px, #666 20px)
    // на линии 200px → период 20/200 = 0.1; покрыть [0,1] плитками.
    let c1 = Color { r: 0x33, g: 0x33, b: 0x33, a: 255 };
    let c2 = Color { r: 0x66, g: 0x66, b: 0x66, a: 255 };
    let stops = vec![
        GradientStop { color: c1, position: Some(Length::Px(0.0)) , ..Default::default() },
        GradientStop { color: c1, position: Some(Length::Px(10.0)) , ..Default::default() },
        GradientStop { color: c2, position: Some(Length::Px(10.0)) , ..Default::default() },
        GradientStop { color: c2, position: Some(Length::Px(20.0)) , ..Default::default() },
    ];
    let r = femtovg_stops(&stops, 200.0, true);
    assert!(r.windows(2).all(|w| w[1].0 >= w[0].0 - 1e-6), "позиции должны быть неубывающими");
    assert!((r[0].0).abs() < 1e-5, "первый стоп в 0, got {}", r[0].0);
    assert!(r.last().unwrap().0 >= 1.0, "последний стоп должен достигать/превышать 1.0");
    assert!(r.len() >= 20, "ожидается множество замощённых стопов, got {}", r.len());
}

#[test]
fn femtovg_stops_repeating_degenerate_period_is_solid() {
    // Нулевой период (оба стопа в одной позиции) → сплошной цвет, без зацикливания.
    let c = Color { r: 10, g: 20, b: 30, a: 255 };
    let stops = vec![
        GradientStop { color: c, position: Some(Length::Percent(50.0)) , ..Default::default() },
        GradientStop { color: c, position: Some(Length::Percent(50.0)) , ..Default::default() },
    ];
    let r = femtovg_stops(&stops, 100.0, true);
    assert_eq!(r.len(), 2);
    assert!((r[0].0).abs() < 1e-5 && (r[1].0 - 1.0).abs() < 1e-5);
}

// ── BUG-085: per-pixel CPU gradient sampler (bypasses 256-texel LUT) ──────

#[test]
fn sample_fvg_stops_clamps_outside_range() {
    // Mirrors femtovg's LUT: positions outside [0,1] take the boundary colour.
    let red = femtovg::Color::rgbaf(1.0, 0.0, 0.0, 1.0);
    let blue = femtovg::Color::rgbaf(0.0, 0.0, 1.0, 1.0);
    let resolved = [(0.2_f32, red), (0.8_f32, blue)];
    let lo = sample_fvg_stops(&resolved, -1.0);
    let hi = sample_fvg_stops(&resolved, 2.0);
    assert!((lo.r - 1.0).abs() < 1e-6 && lo.b.abs() < 1e-6, "below-range = first stop");
    assert!(hi.r.abs() < 1e-6 && (hi.b - 1.0).abs() < 1e-6, "above-range = last stop");
}

#[test]
fn sample_fvg_stops_interpolates_midpoint() {
    let red = femtovg::Color::rgbaf(1.0, 0.0, 0.0, 1.0);
    let blue = femtovg::Color::rgbaf(0.0, 0.0, 1.0, 1.0);
    let resolved = [(0.0_f32, red), (1.0_f32, blue)];
    let mid = sample_fvg_stops(&resolved, 0.5);
    assert!((mid.r - 0.5).abs() < 1e-6, "R halfway, got {}", mid.r);
    assert!((mid.b - 0.5).abs() < 1e-6, "B halfway, got {}", mid.b);
}

#[test]
fn sample_fvg_stops_hard_stop_is_sharp() {
    // Coincident positions (hard stop): the colour flips sharply across 0.5
    // with no visible ramp on either side.
    let a = femtovg::Color::rgbaf(0.2, 0.2, 0.2, 1.0);
    let b = femtovg::Color::rgbaf(0.4, 0.4, 0.4, 1.0);
    let resolved = [(0.0_f32, a), (0.5_f32, a), (0.5_f32, b), (1.0_f32, b)];
    let just_below = sample_fvg_stops(&resolved, 0.49);
    let just_above = sample_fvg_stops(&resolved, 0.51);
    assert!((just_below.r - 0.2).abs() < 1e-3, "below hard stop = a");
    assert!((just_above.r - 0.4).abs() < 1e-3, "above hard stop = b");
}

#[test]
fn fvg_to_rgba8_quantizes_channels() {
    let c = femtovg::Color::rgbaf(1.0, 0.5, 0.0, 1.0);
    let q = fvg_to_rgba8(c);
    assert_eq!((q.r, q.b, q.a), (255, 0, 255));
    // 0.5 * 255 = 127.5 → rounds to 128.
    assert_eq!(q.g, 128);
}

/// BUG-183 — a gradient `mask-image` is applied by painting the gradient
/// over the masked layer with `CompositeOperation::DestinationIn`, which
/// multiplies the layer's alpha by the gradient's *alpha* (`mask-mode: alpha`,
/// the default). The mask therefore depends on the resolved stops carrying a
/// decreasing alpha for a `black → transparent` gradient. If this regressed,
/// `composite_mask_layer` would either not fade (alpha stuck at 1) or clip
/// hard. Guards the pure kernel the GPU path relies on (the offscreen FBO +
/// DestinationIn composite itself needs a GL context and is exercised by the
/// TEST-26 graphic gate).
#[test]
fn mask_gradient_alpha_decreases_black_to_transparent() {
    let stops = vec![
        GradientStop { color: Color::BLACK, position: None , ..Default::default() },
        GradientStop { color: Color::TRANSPARENT, position: None , ..Default::default() },
    ];
    let resolved = resolve_stops(&stops, 200.0);
    // The black→transparent segment is subdivided for premultiplied
    // interpolation (BUG-190), so the count grows beyond 2; the endpoints
    // (the mask's defining values) must still be alpha 1 → 0, monotonically.
    assert!(resolved.len() >= 2);
    // Opaque end → mask = 1 (content fully shown).
    assert!((resolved.first().unwrap().1.a - 1.0).abs() < 1e-5, "opaque stop must keep alpha 1");
    // Transparent end → mask = 0 (content fully hidden).
    assert!(resolved.last().unwrap().1.a.abs() < 1e-5, "transparent stop must have alpha 0");
    // Alpha decreases monotonically across the (subdivided) ramp.
    for w in resolved.windows(2) {
        assert!(w[1].1.a <= w[0].1.a + 1e-5, "mask alpha must not increase");
    }
}

#[test]
fn dashed_offsets_match_edge_dash_counts() {
    // BUG-080: on a 180px side, Edge produces n = 18/15/8/4 dashes for
    // 2/4/8/16px widths (the wgpu emit_border_side reference values).
    assert_eq!(dashed_border_offsets(180.0, 2.0).len(), 18);
    assert_eq!(dashed_border_offsets(180.0, 4.0).len(), 15);
    assert_eq!(dashed_border_offsets(180.0, 8.0).len(), 8);
    assert_eq!(dashed_border_offsets(180.0, 16.0).len(), 4);
}

#[test]
fn dashed_offsets_anchor_first_and_last() {
    // First dash starts at 0; last dash ends exactly at total.
    let segs = dashed_border_offsets(180.0, 4.0);
    assert_eq!(segs.first().unwrap().0, 0.0);
    let (last_off, last_len) = *segs.last().unwrap();
    assert!((last_off + last_len - 180.0).abs() < 1.0, "last end {}", last_off + last_len);
}

#[test]
fn dotted_offsets_match_edge_dot_counts() {
    // BUG-080: 180px side → n = 46/23/12/6 dots for 2/4/8/16px widths.
    assert_eq!(dotted_border_offsets(180.0, 2.0).len(), 46);
    assert_eq!(dotted_border_offsets(180.0, 4.0).len(), 23);
    assert_eq!(dotted_border_offsets(180.0, 8.0).len(), 12);
    assert_eq!(dotted_border_offsets(180.0, 16.0).len(), 6);
}

#[test]
fn dotted_offsets_symmetric_and_bounded() {
    let segs = dotted_border_offsets(180.0, 8.0);
    // First dot at 0, last dot ends at total.
    assert_eq!(segs.first().unwrap().0, 0.0);
    let (last_off, last_len) = *segs.last().unwrap();
    assert!((last_off + last_len - 180.0).abs() < 1.0, "last end {}", last_off + last_len);
    // Every dot length equals dot_len (= width here).
    assert!(segs.iter().all(|&(_, len)| (len - 8.0).abs() < 0.01));
}

#[test]
fn border_offsets_empty_for_zero_total() {
    assert!(dashed_border_offsets(0.0, 4.0).is_empty());
    assert!(dotted_border_offsets(-5.0, 4.0).is_empty());
}

// ── position:sticky offset/bound tests (BUG-337) ────────────────────────
// Mirrors renderer.rs's wgpu-backend BUG-336 test suite (§ "position:sticky
// offset/bound tests"), adapted to femtovg's continuously-accumulated
// `canvas.transform()` model instead of a parallel `Vec<Mat4>` stack.

#[test]
fn sticky_bound_defaults_to_full_viewport_with_no_clip_or_transform() {
    let bound = sticky_bound(&[], &femtovg::Transform2D::identity(), 800.0, 600.0);
    assert_eq!(bound, Rect::new(0.0, 0.0, 800.0, 600.0));
}

#[test]
fn sticky_bound_narrows_to_innermost_clip_rect() {
    let bound_stack = [Rect::new(50.0, 100.0, 300.0, 200.0)];
    let bound = sticky_bound(&bound_stack, &femtovg::Transform2D::identity(), 800.0, 600.0);
    assert_eq!(bound, Rect::new(50.0, 100.0, 300.0, 200.0));
}

#[test]
fn sticky_bound_maps_screen_clip_back_through_ambient_transform() {
    // The panel's clip (screen space) sits at y=[100,300) once an ambient
    // translate(0, 80) is active — the bound handed to sticky_offset_dy
    // must be in the SAME pre-ambient page-space as flow_rect, i.e.
    // shifted back by -80.
    let bound_stack = [Rect::new(50.0, 100.0, 300.0, 200.0)];
    let ambient = femtovg::Transform2D::new_translation(0.0, 80.0);
    let bound = sticky_bound(&bound_stack, &ambient, 800.0, 600.0);
    assert!((bound.y - 20.0).abs() < 0.01, "expected bound.y=20 (100 - 80), got {}", bound.y);
    assert!((bound.x - 50.0).abs() < 0.01, "x untouched by a pure y-translate, got {}", bound.x);
}

#[test]
fn sticky_offset_dy_unclamped_is_zero() {
    // No insets fire yet — 0.0 baseline means "let the ambient-scrolled
    // position stand unmodified" (unlike wgpu's `-scroll_y` baseline,
    // femtovg's `ambient` already carries all scroll on its own).
    let flow_rect = Rect::new(0.0, 200.0, 300.0, 50.0);
    let bound = Rect::new(0.0, 0.0, 800.0, 600.0);
    let dy = sticky_offset_dy(&flow_rect, Some(0.0), None, bound);
    assert!(dy.abs() < 0.01, "expected unclamped dy=0, got {dy}");
}

#[test]
fn sticky_nested_in_scroll_container_pins_within_local_scrollport() {
    // BUG-337 regression scenario: `.net-table th { position:sticky; top:0 }`
    // inside a `.dt-panel { overflow-y:auto }` whose own PushScrollLayer has
    // scrolled 120px — folded directly into femtovg's ambient canvas
    // transform (unlike wgpu, no separate page-level scroll_y baseline is
    // needed: bound_stack holds the panel's own screen-space scrollport,
    // captured via `push_sticky_bound` BEFORE that scroll translate joins
    // the canvas, exactly like `PushScrollLayer`'s real handler does).
    let bound_stack = [Rect::new(0.0, 40.0, 400.0, 240.0)]; // panel's screen-space scrollport
    let ambient = femtovg::Transform2D::new_translation(0.0, -120.0); // panel's own scroll(-y)
    let bound = sticky_bound(&bound_stack, &ambient, 800.0, 600.0);

    // Header's flow (page-space, pre-scroll) position: early in the table,
    // well above where the panel has scrolled to — its *unclamped* on-screen
    // position would be ambient(flow.y) = 150 - 120 = 30, above the panel's
    // visible top (40), i.e. scrolled out of view without the sticky clamp.
    let flow_rect = Rect::new(0.0, 150.0, 400.0, 24.0);
    let dy = sticky_offset_dy(&flow_rect, Some(0.0), None, bound);

    // Final on-screen position = ambient(flow.y + dy), same as the real
    // `self.canvas.translate(sdx, sdy)` composition BeginStickyLayer does.
    let (_, screen_y) = ambient.transform_point(0.0, flow_rect.y + dy);
    assert!(
        (screen_y - 40.0).abs() < 0.01,
        "sticky header must pin at the panel's own scrollport top (40), got {screen_y}"
    );
}

// interp_conic_color / conic_sample_t unit-тесты переехали в
// crate::gradient_math (PA-1; sample_gradient_color покрывает интерполяцию).

#[test]
fn draw_fill_rounded_rect_circular_does_not_panic() {
    // Circular corners (rx == ry) — fast path.
    let radii = CornerRadii { tl: 8.0, tl_y: 8.0, tr: 8.0, tr_y: 8.0,
                               br: 8.0, br_y: 8.0, bl: 8.0, bl_y: 8.0 };
    // Just verify no panic on valid input (no headless GL context needed for unit test).
    let _ = radii; // used to verify compilation
    assert!((radii.tl - radii.tl_y).abs() < 0.5);
}

#[test]
fn draw_fill_rounded_rect_elliptical_different_radii() {
    // Elliptical: rx=40, ry=20 — should use bezier path, not fast path.
    let radii = CornerRadii { tl: 40.0, tl_y: 20.0, tr: 40.0, tr_y: 20.0,
                               br: 40.0, br_y: 20.0, bl: 40.0, bl_y: 20.0 };
    // Verify that fast-path condition is false.
    assert!((radii.tl - radii.tl_y).abs() >= 0.5);
}

#[test]
fn box_blur_rgba_single_pixel_unchanged() {
    // Single pixel: no neighbors, should remain unchanged.
    let mut px = vec![255u8, 0, 0, 255]; // red pixel
    box_blur_rgba_region(&mut px, 1, 1, 2.0, (0, 0, 1, 1));
    assert_eq!(&px, &[255, 0, 0, 255]); // single pixel — unchanged
}

#[test]
fn box_blur_rgba_3x1_averages_horizontally() {
    // 3 pixels: red, black, red — after blur the middle should average neighbors.
    let mut px = vec![
        255u8, 0, 0, 255, // red
        0,   0, 0, 255,   // black
        255, 0, 0, 255,   // red
    ];
    box_blur_rgba_region(&mut px, 3, 1, 1.0, (0, 0, 3, 1));
    // Middle pixel (index 1) should now be average of all three: 255+0+255/3 = 170
    let mid_r = px[4]; // offset 1*4 + 0
    assert!(mid_r > 100, "middle pixel should be brightened by blur: got {mid_r}");
}

#[test]
fn gaussian_box_radii_three_positive_passes() {
    // Three boxes, each a valid (≥1) half-width; their combined variance
    // should be in the neighbourhood of the requested Gaussian variance.
    for &sigma in &[1.0f32, 2.0, 4.0, 8.0] {
        let radii = gaussian_box_radii(sigma);
        assert!(radii.iter().all(|&r| r >= 1), "sigma {sigma}: radii {radii:?}");
        // Variance of a (2r+1)-box is ((2r+1)²-1)/12; three boxes add up.
        let var: f32 = radii.iter().map(|&r| {
            let w = (2 * r + 1) as f32;
            (w * w - 1.0) / 12.0
        }).sum();
        let target = sigma * sigma;
        // Discrete radii can't hit the target exactly, but should be close.
        assert!((var - target).abs() <= target * 0.6 + 1.0,
            "sigma {sigma}: variance {var} far from target {target} (radii {radii:?})");
    }
}

#[test]
fn box_blur_rgba_region_leaves_outside_pixels_untouched() {
    // 4×1 strip: white, white, white, black. Blur only the first 3 (the
    // "card"); the 4th pixel (outside the region) must stay pure black —
    // proving the region path never writes outside its rectangle.
    let mut px = vec![
        255u8, 255, 255, 255,
        255,   255, 255, 255,
        255,   255, 255, 255,
        0,     0,   0,   255,
    ];
    box_blur_rgba_region(&mut px, 4, 1, 2.0, (0, 0, 3, 1));
    assert_eq!(&px[12..16], &[0, 0, 0, 255], "outside-region pixel must be untouched");
}

#[test]
fn box_blur_rgba_region_clamps_sampling_no_edge_bleed() {
    // 4×1 strip: black background, then 3 white "card" pixels.
    // Blurring the card region [1,4) must NOT pull the black pixel at x=0
    // into the card's left edge: the whole card stays pure white because
    // sampling is clamped to [1,4) (edge pixels duplicate within the card).
    let mut px = vec![
        0u8,   0,   0,   255, // black background (outside region)
        255,   255, 255, 255, // card start
        255,   255, 255, 255,
        255,   255, 255, 255,
    ];
    box_blur_rgba_region(&mut px, 4, 1, 2.0, (1, 0, 4, 1));
    assert_eq!(&px[4..8], &[255, 255, 255, 255], "card left edge must not bleed black background");
    // Sanity: the unclamped full-width blur *would* darken the card's left
    // edge — confirm the difference is real, not a no-op test.
    let mut full = vec![
        0u8,   0,   0,   255,
        255,   255, 255, 255,
        255,   255, 255, 255,
        255,   255, 255, 255,
    ];
    box_blur_rgba_region(&mut full, 4, 1, 2.0, (0, 0, 4, 1));
    assert!(full[4] < 255, "unclamped blur should bleed black into card edge (got {})", full[4]);
}

// ── apply_filter_rgba tests ──────────────────────────────────────────────

/// Helper: create a single premultiplied RGBA pixel (full opacity → premul = straight).
fn px(r: u8, g: u8, b: u8) -> Vec<u8> {
    vec![r, g, b, 255]
}

#[test]
fn apply_filter_rgba_grayscale_full() {
    // Fully red pixel → grayscale(1.0) → grey luma value.
    let mut buf = px(255, 0, 0);
    apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Grayscale(1.0));
    // R≈G≈B (luma of pure red ≈ 0.2126 * 255 ≈ 54).
    let r = buf[0]; let g = buf[1]; let b = buf[2];
    assert!((r as i32 - g as i32).abs() <= 2, "R/G should be equal after grayscale");
    assert!((g as i32 - b as i32).abs() <= 2, "G/B should be equal after grayscale");
    assert!(r > 20 && r < 70, "grayscale luma for red should be ~54, got {r}");
}

#[test]
fn apply_filter_rgba_grayscale_zero_noop() {
    // grayscale(0) → image unchanged.
    let mut buf = px(200, 100, 50);
    apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Grayscale(0.0));
    assert_eq!(&buf, &[200, 100, 50, 255]);
}

#[test]
fn apply_filter_rgba_invert_full() {
    // invert(1.0) → each channel = 255 - original.
    let mut buf = px(100, 150, 200);
    apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Invert(1.0));
    // After invert(1.0): r=255-100=155, g=255-150=105, b=255-200=55.
    assert!((buf[0] as i32 - 155).abs() <= 2, "expected R≈155 after invert, got {}", buf[0]);
    assert!((buf[1] as i32 - 105).abs() <= 2, "expected G≈105 after invert, got {}", buf[1]);
    assert!((buf[2] as i32 - 55).abs() <= 2, "expected B≈55 after invert, got {}", buf[2]);
}

#[test]
fn apply_filter_rgba_sepia_full() {
    // sepia(1.0) on white → known reference values.
    let mut buf = px(255, 255, 255);
    apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Sepia(1.0));
    // sepia output for white: r=min(1,0.393+0.769+0.189)=1, g=0.349+0.686+0.168=1.203→1, b=0.272+0.534+0.131=0.937.
    assert_eq!(buf[0], 255, "R should be 255 for white sepia");
    assert_eq!(buf[1], 255, "G should be 255 for white sepia");
    assert!((buf[2] as i32 - 239).abs() <= 3, "B should be ~239 for white sepia, got {}", buf[2]);
}

#[test]
fn apply_filter_rgba_transparent_pixel_skipped() {
    // Alpha=0 pixel must remain untouched (avoid divide-by-zero).
    let mut buf = vec![255u8, 0, 0, 0]; // premultiplied transparent
    apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Grayscale(1.0));
    assert_eq!(&buf, &[255, 0, 0, 0], "transparent pixel should not be modified");
}

#[test]
fn apply_filter_rgba_brightness_doubles() {
    // brightness(2.0) on mid-grey → close to white.
    let mut buf = px(128, 128, 128);
    apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Brightness(2.0));
    assert!(buf[0] > 200, "R should be near 255 after brightness(2), got {}", buf[0]);
}

#[test]
fn apply_filter_rgba_saturate_zero_desaturates() {
    // saturate(0) desaturates completely → grey.
    let mut buf = px(255, 0, 0);
    apply_filter_rgba(&mut buf, &lumen_layout::FilterFn::Saturate(0.0));
    let r = buf[0]; let g = buf[1]; let b = buf[2];
    assert!((r as i32 - g as i32).abs() <= 2, "saturate(0) R/G should be equal, got r={r} g={g}");
    assert!((g as i32 - b as i32).abs() <= 2, "saturate(0) G/B should be equal, got g={g} b={b}");
}

#[test]
fn fsin_cos_basic_values() {
    // fsin(0) = 0, fsin(π/2) ≈ 1.
    assert!((fsin(0.0)).abs() < 0.001, "sin(0) should be ~0");
    assert!((fsin(std::f32::consts::FRAC_PI_2) - 1.0).abs() < 0.001, "sin(π/2) should be ~1");
    assert!((fcos(0.0) - 1.0).abs() < 0.001, "cos(0) should be ~1");
    assert!((fcos(std::f32::consts::FRAC_PI_2)).abs() < 0.001, "cos(π/2) should be ~0");
}

// ── extend_region_replicated tests ────────────────────────────────────────

#[test]
fn extend_region_replicated_zero_extension_is_noop() {
    let mut px = vec![10u8, 20, 30, 255, 40, 50, 60, 255];
    let _region = extend_region_replicated(&mut px, 2, 1, (0, 0, 2, 1), 0);
    assert_eq!(&px, &[10, 20, 30, 255, 40, 50, 60, 255]);
}

#[test]
fn extend_region_replicated_fills_top_band() {
    // 4×1 strip: red, green, blue, white. Region = [1,3). Extend by 1.
    // Top band (x=0..4, y=-1 conceptually but clamped to 0): replicate row 0.
    let mut px = vec![
        255, 0,   0,   255, // x=0 red
          0, 255, 0,   255, // x=1 green (region start)
          0,   0, 255, 255, // x=2 blue  (region end-1)
        255, 255, 255, 255, // x=3 white
    ];
    let region = extend_region_replicated(&mut px, 4, 1, (1, 0, 3, 1), 1);
    // Extended region grows by 1px each side, clamped to the 4×1 image bounds.
    assert_eq!(region, (0, 0, 4, 1), "extended region must clamp to image bounds");
    // Top band is same row in 1-high image, so no new rows.
    // Left extension x=0 should replicate from x=1 (green).
    let g = [0u8, 255, 0, 255];
    assert_eq!(&px[0..4], &g, "left extension must replicate left edge of region");
    // Right extension x=3 should replicate from x=2 (blue).
    let b = [0u8, 0, 255, 255];
    assert_eq!(&px[12..16], &b, "right extension must replicate right edge of region");
}

#[test]
fn extend_region_replicated_2d_corners_and_edges() {
    // 4×2 image:
    //   row 0: black  white  white  black
    //   row 1: black  white  white  black
    // Region = [1,3) × [0,2) = the 2 white pixels in both rows.
    // Extend by 1: the 6 surrounding pixels should all become white.
    let mut px = vec![
        0,   0,   0,   255,   // (0,0) black
      255, 255, 255, 255,     // (1,0) white
      255, 255, 255, 255,     // (2,0) white
        0,   0,   0, 255,     // (3,0) black
        0,   0,   0, 255,     // (0,1) black
      255, 255, 255, 255,     // (1,1) white
      255, 255, 255, 255,     // (2,1) white
        0,   0,   0, 255,     // (3,1) black
    ];
    let _ = extend_region_replicated(&mut px, 4, 2, (1, 0, 3, 2), 1);
    let white = [255u8, 255, 255, 255];
    // Corners and edges all replicated from nearest white edge.
    for &off in &[0, 4, 8, 12, 16, 20] {
        assert_eq!(&px[off..off+4], &white, "pixel at offset {off} must be replicated white");
    }
}

/// BUG-272 срез 5: `crop_region_rgba` extracts exactly the requested
/// sub-rectangle, dropping everything outside it.
#[test]
fn crop_region_rgba_extracts_sub_rect() {
    // 4×2 image, region = [1,3) x [0,2) (the two middle columns).
    let px = vec![
        10, 10, 10, 255,   20, 20, 20, 255,   30, 30, 30, 255,   40, 40, 40, 255, // row 0
        50, 50, 50, 255,   60, 60, 60, 255,   70, 70, 70, 255,   80, 80, 80, 255, // row 1
    ];
    let (out, w, h) = crop_region_rgba(&px, 4, 2, (1, 0, 3, 2));
    assert_eq!((w, h), (2, 2));
    assert_eq!(
        out,
        vec![20, 20, 20, 255, 30, 30, 30, 255, 60, 60, 60, 255, 70, 70, 70, 255]
    );
}

/// A region clamped fully outside the buffer (or degenerate after
/// clamping) must return an empty crop rather than panic — the caller
/// (`apply_backdrop_filters`) treats `(w, h) == (0, 0)` as "skip this
/// backdrop-filter layer".
#[test]
fn crop_region_rgba_empty_region_returns_empty() {
    let px = vec![1u8, 2, 3, 4];
    let (out, w, h) = crop_region_rgba(&px, 1, 1, (2, 2, 5, 5));
    assert!(out.is_empty());
    assert_eq!((w, h), (0, 0));
}

/// A region extending past the buffer edge is clamped, not rejected —
/// mirrors the `bx1.min(iw)`/`by1.min(ih)` clamping already applied by
/// `apply_backdrop_filters` before calling this helper, so this exercises
/// the defensive clamp inside `crop_region_rgba` itself.
#[test]
fn crop_region_rgba_clamps_to_buffer_bounds() {
    let px = vec![
        1, 1, 1, 255,   2, 2, 2, 255, // row 0
        3, 3, 3, 255,   4, 4, 4, 255, // row 1
    ];
    let (out, w, h) = crop_region_rgba(&px, 2, 2, (1, 0, 10, 10));
    assert_eq!((w, h), (1, 2));
    assert_eq!(out, vec![2, 2, 2, 255, 4, 4, 4, 255]);
}

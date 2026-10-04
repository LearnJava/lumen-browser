use super::*;

// ──────── CSS Scroll Snap L1 ────────

#[test]
fn scroll_snap_type_none() {
    let root = lay("<p>x</p>", "p { scroll-snap-type: none; }");
    assert_eq!(first_p_style(&root).scroll_snap_type.axis, ScrollSnapAxis::None);
}

#[test]
fn scroll_snap_type_x_mandatory() {
    let root = lay("<p>x</p>", "p { scroll-snap-type: x mandatory; }");
    let s = first_p_style(&root);
    assert_eq!(s.scroll_snap_type.axis, ScrollSnapAxis::X);
    assert_eq!(s.scroll_snap_type.strictness, ScrollSnapStrictness::Mandatory);
}

#[test]
fn scroll_snap_align_single_keyword() {
    let root = lay("<p>x</p>", "p { scroll-snap-align: center; }");
    let s = first_p_style(&root);
    assert_eq!(s.scroll_snap_align.block, ScrollSnapAlignKeyword::Center);
    assert_eq!(s.scroll_snap_align.inline, ScrollSnapAlignKeyword::Center);
}

#[test]
fn scroll_snap_align_two_keywords() {
    let root = lay("<p>x</p>", "p { scroll-snap-align: start end; }");
    let s = first_p_style(&root);
    assert_eq!(s.scroll_snap_align.block, ScrollSnapAlignKeyword::Start);
    assert_eq!(s.scroll_snap_align.inline, ScrollSnapAlignKeyword::End);
}

#[test]
fn scroll_snap_stop_always() {
    let root = lay("<p>x</p>", "p { scroll-snap-stop: always; }");
    assert_eq!(first_p_style(&root).scroll_snap_stop, ScrollSnapStop::Always);
}

#[test]
fn scroll_margin_individual() {
    let root = lay("<p>x</p>", "p { scroll-margin-top: 10px; scroll-margin-left: 5px; }");
    let s = first_p_style(&root);
    assert!((s.scroll_margin_top - 10.0).abs() < 1e-6);
    assert!((s.scroll_margin_left - 5.0).abs() < 1e-6);
}

#[test]
fn scroll_margin_shorthand_4_values() {
    let root = lay("<p>x</p>", "p { scroll-margin: 1px 2px 3px 4px; }");
    let s = first_p_style(&root);
    assert!((s.scroll_margin_top - 1.0).abs() < 1e-6);
    assert!((s.scroll_margin_right - 2.0).abs() < 1e-6);
    assert!((s.scroll_margin_bottom - 3.0).abs() < 1e-6);
    assert!((s.scroll_margin_left - 4.0).abs() < 1e-6);
}

#[test]
fn scroll_padding_shorthand_1_value() {
    let root = lay("<p>x</p>", "p { scroll-padding: 5px; }");
    let s = first_p_style(&root);
    assert!((s.scroll_padding_top - 5.0).abs() < 1e-6);
    assert!((s.scroll_padding_right - 5.0).abs() < 1e-6);
    assert!((s.scroll_padding_bottom - 5.0).abs() < 1e-6);
    assert!((s.scroll_padding_left - 5.0).abs() < 1e-6);
}

// ──────── CSS Overscroll Behavior L1 ────────

#[test]
fn overscroll_behavior_contain() {
    let root = lay("<p>x</p>", "p { overscroll-behavior: contain; }");
    let s = first_p_style(&root);
    assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::Contain);
    assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::Contain);
}

#[test]
fn overscroll_behavior_two_values() {
    let root = lay("<p>x</p>", "p { overscroll-behavior: contain none; }");
    let s = first_p_style(&root);
    assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::Contain);
    assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::None);
}

#[test]
fn overscroll_behavior_individual_axis() {
    let root = lay("<p>x</p>", "p { overscroll-behavior-x: none; overscroll-behavior-y: auto; }");
    let s = first_p_style(&root);
    assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::None);
    assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::Auto);
}

// BUG-516: `overscroll-behavior-block`/`-inline` — logical longhands, must
// resolve to the physical `_x`/`_y` pair through `resolve_overscroll_
// behavior_logical_properties` (`style/logical.rs`), same shape as
// `overflow-block`/`-inline` (BUG-505).
#[test]
fn overscroll_behavior_logical_horizontal_tb() {
    let root = lay(
        "<p>x</p>",
        "p { overscroll-behavior-block: contain; overscroll-behavior-inline: none; }",
    );
    let s = first_p_style(&root);
    assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::Contain);
    assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::None);
}

#[test]
fn overscroll_behavior_logical_vertical_rl_swaps_axes() {
    let root = lay(
        "<p>x</p>",
        "p { writing-mode: vertical-rl; overscroll-behavior-block: contain; overscroll-behavior-inline: none; }",
    );
    let s = first_p_style(&root);
    assert_eq!(s.overscroll_behavior_x, OverscrollBehavior::Contain);
    assert_eq!(s.overscroll_behavior_y, OverscrollBehavior::None);
}

#[test]
fn scroll_snap_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { scroll-snap-type: x mandatory; }",
    );
    let p = nested_p_style(&root);
    // Не наследуется.
    assert_eq!(p.scroll_snap_type.axis, ScrollSnapAxis::None);
}

// ──────── collect_snap_containers / find_snap_target ────────

fn make_snap_container(
    w: f32,
    h: f32,
    axis: ScrollSnapAxis,
    strictness: ScrollSnapStrictness,
) -> SnapContainer {
    SnapContainer {
        node: lumen_dom::NodeId::from_index(0),
        snap_type: ScrollSnapType { axis, strictness },
        rect: lumen_core::geom::Rect { x: 0.0, y: 0.0, width: w, height: h },
        scroll_padding_top: 0.0,
        scroll_padding_right: 0.0,
        scroll_padding_bottom: 0.0,
        scroll_padding_left: 0.0,
        points: Vec::new(),
    }
}

fn snap_pt(y: f32) -> SnapPoint {
    SnapPoint { node: lumen_dom::NodeId::from_index(1), snap_x: None, snap_y: Some(y), stop_always: false }
}

#[test]
fn find_snap_target_mandatory_y() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
    );
    sc.points = vec![snap_pt(0.0), snap_pt(720.0), snap_pt(1440.0)];
    // Target 400 → nearest is 0 (dist=160000) vs 720 (dist=102400) → snap 720.
    let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 400.0));
    assert!(result.is_some());
    let (_, sy) = result.unwrap();
    assert!((sy - 720.0).abs() < 1e-3, "expected 720, got {sy}");
}

#[test]
fn find_snap_target_mandatory_first_section() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
    );
    sc.points = vec![snap_pt(0.0), snap_pt(720.0), snap_pt(1440.0)];
    // Target 300 → nearest is 0 (dist=90000) vs 720 (dist=176400) → snap 0.
    let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 300.0));
    assert!(result.is_some());
    let (_, sy) = result.unwrap();
    assert!((sy - 0.0).abs() < 1e-3, "expected 0, got {sy}");
}

#[test]
fn find_snap_target_proximity_within_threshold() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Proximity,
    );
    sc.points = vec![snap_pt(720.0)];
    // Proximity threshold = 720 * 0.5 = 360. Target 450 → dist from 720 = 270 ≤ 360 → snaps.
    let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 450.0));
    assert!(result.is_some());
    let (_, sy) = result.unwrap();
    assert!((sy - 720.0).abs() < 1e-3, "expected 720, got {sy}");
}

#[test]
fn find_snap_target_proximity_out_of_threshold() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Proximity,
    );
    sc.points = vec![snap_pt(720.0)];
    // Proximity threshold = 360. Target 200 → dist from 720 = 520 > 360 → no snap.
    let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 200.0));
    assert!(result.is_none(), "should not snap when beyond proximity threshold");
}

#[test]
fn find_snap_target_stop_always_barrier_viewport() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
    );
    sc.points = vec![
        SnapPoint { node: lumen_dom::NodeId::from_index(1), snap_x: None, snap_y: Some(720.0), stop_always: true },
        snap_pt(1440.0),
    ];
    // Scrolling from 0 to 1500 would pass 720 (stop_always) → forced to 720.
    let result = find_snap_target(&sc, (0.0, 0.0), (0.0, 1500.0));
    assert!(result.is_some());
    let (_, sy) = result.unwrap();
    assert!((sy - 720.0).abs() < 1e-3, "stop_always barrier should force snap to 720, got {sy}");
}

#[test]
fn find_snap_target_no_points_returns_none() {
    let sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
    );
    assert!(find_snap_target(&sc, (0.0, 0.0), (0.0, 400.0)).is_none());
}

// ──────── find_snapped_nodes (CSS Scroll Snap L2 events) ────────

fn snap_pt_node(idx: u32, x: Option<f32>, y: Option<f32>) -> SnapPoint {
    SnapPoint {
        node: lumen_dom::NodeId::from_index(idx as usize),
        snap_x: x,
        snap_y: y,
        stop_always: false,
    }
}

#[test]
fn find_snapped_nodes_empty_container_is_default() {
    let sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
    );
    let t = find_snapped_nodes(&sc, (0.0, 0.0));
    assert_eq!(t, SnapTargets::default());
}

#[test]
fn find_snapped_nodes_block_axis_picks_nearest() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Y, ScrollSnapStrictness::Mandatory,
    );
    sc.points = vec![
        snap_pt_node(1, None, Some(0.0)),
        snap_pt_node(2, None, Some(720.0)),
        snap_pt_node(3, None, Some(1440.0)),
    ];
    // Scroll at 700 → nearest block snap is node 2 (720).
    let t = find_snapped_nodes(&sc, (0.0, 700.0));
    assert_eq!(t.block, Some(lumen_dom::NodeId::from_index(2)));
    // Y-only container does not snap on the inline axis.
    assert_eq!(t.inline, None);
}

#[test]
fn find_snapped_nodes_both_axes() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Both, ScrollSnapStrictness::Mandatory,
    );
    sc.points = vec![
        snap_pt_node(1, Some(0.0), Some(0.0)),
        snap_pt_node(2, Some(500.0), Some(720.0)),
    ];
    // Inline near 480 → node 2 (x=500); block near 30 → node 1 (y=0).
    let t = find_snapped_nodes(&sc, (480.0, 30.0));
    assert_eq!(t.inline, Some(lumen_dom::NodeId::from_index(2)));
    assert_eq!(t.block, Some(lumen_dom::NodeId::from_index(1)));
}

#[test]
fn find_snapped_nodes_x_only_ignores_block() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::X, ScrollSnapStrictness::Mandatory,
    );
    sc.points = vec![
        snap_pt_node(1, Some(0.0), Some(0.0)),
        snap_pt_node(2, Some(1024.0), Some(720.0)),
    ];
    let t = find_snapped_nodes(&sc, (900.0, 700.0));
    assert_eq!(t.inline, Some(lumen_dom::NodeId::from_index(2)));
    assert_eq!(t.block, None);
}

#[test]
fn find_snapped_nodes_skips_points_without_axis_offset() {
    let mut sc = make_snap_container(
        1024.0, 720.0, ScrollSnapAxis::Both, ScrollSnapStrictness::Mandatory,
    );
    // Node 1 snaps only on block; node 2 only on inline.
    sc.points = vec![
        snap_pt_node(1, None, Some(0.0)),
        snap_pt_node(2, Some(300.0), None),
    ];
    let t = find_snapped_nodes(&sc, (290.0, 10.0));
    assert_eq!(t.inline, Some(lumen_dom::NodeId::from_index(2)));
    assert_eq!(t.block, Some(lumen_dom::NodeId::from_index(1)));
}

#[test]
fn collect_snap_containers_empty_when_no_snap_type() {
    let root = lay(
        "<div><p>first</p><p>second</p></div>",
        "div { width: 1024px; height: 720px; overflow: scroll; }",
    );
    // No scroll-snap-type → empty containers list.
    let containers = collect_snap_containers(&root);
    assert!(containers.is_empty(), "expected no snap containers");
}

#[test]
fn collect_snap_containers_finds_y_mandatory() {
    let root = lay(
        "<div><p>first</p><p>second</p></div>",
        "div { width: 1024px; height: 720px; overflow: scroll; scroll-snap-type: y mandatory; } p { height: 720px; scroll-snap-align: start; }",
    );
    let containers = collect_snap_containers(&root);
    // At least one snap container should be found (the div).
    assert!(!containers.is_empty(), "expected a snap container");
    let sc = &containers[0];
    assert_eq!(sc.snap_type.axis, ScrollSnapAxis::Y);
    assert_eq!(sc.snap_type.strictness, ScrollSnapStrictness::Mandatory);
}

// ──────── mask-* + scrollbar-* ────────

/// Топовый (первый) слой маски `<p>` — все mask-longhand-ы теперь живут
/// в `mask_layers` (CSS Masking L1 §4.9).
fn first_p_mask(root: &LayoutBox) -> MaskLayer {
    first_p_style(root)
        .mask_layers
        .first()
        .cloned()
        .expect("mask layer")
}

#[test]
fn mask_image_url() {
    let root = lay("<p>x</p>", "p { mask-image: url(\"mask.png\"); }");
    assert_eq!(
        first_p_mask(&root).image,
        BackgroundImage::Url("mask.png".into())
    );
}

#[test]
fn mask_image_none_clears() {
    let root = lay("<p>x</p>", "p { mask-image: url(m.png); mask-image: none; }");
    assert_eq!(first_p_mask(&root).image, BackgroundImage::None);
}

#[test]
fn mask_repeat_no_repeat() {
    let root = lay("<p>x</p>", "p { mask-repeat: no-repeat; }");
    assert_eq!(first_p_mask(&root).repeat, BackgroundRepeat::NoRepeat);
}

#[test]
fn mask_size_cover() {
    let root = lay("<p>x</p>", "p { mask-size: cover; }");
    assert_eq!(first_p_mask(&root).size, BackgroundSize::Cover);
}

#[test]
fn mask_mode_default_is_alpha() {
    let root = lay("<p>x</p>", "p { mask-image: linear-gradient(black, white); }");
    assert_eq!(first_p_mask(&root).mode, MaskMode::Alpha);
}

#[test]
fn mask_mode_luminance() {
    let root = lay("<p>x</p>", "p { mask-mode: luminance; }");
    assert_eq!(first_p_mask(&root).mode, MaskMode::Luminance);
}

#[test]
fn mask_mode_alpha_keyword() {
    let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: alpha; }");
    assert_eq!(first_p_mask(&root).mode, MaskMode::Alpha);
}

#[test]
fn mask_mode_match_source_resolves_to_alpha() {
    let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: match-source; }");
    assert_eq!(first_p_mask(&root).mode, MaskMode::Alpha);
}

#[test]
fn mask_mode_invalid_keeps_previous() {
    let root = lay("<p>x</p>", "p { mask-mode: luminance; mask-mode: bogus; }");
    assert_eq!(first_p_mask(&root).mode, MaskMode::Luminance);
}

#[test]
fn mask_mode_not_inherited() {
    // `first_p_style` returns the outer div block; drill into its child <p>.
    let root = lay("<div><p>x</p></div>", "div { mask-mode: luminance; }");
    let div = &root
        .children
        .iter()
        .find(|c| matches!(&c.kind, BoxKind::Block))
        .expect("div block");
    assert_eq!(
        div.style.mask_layers.first().expect("div mask layer").mode,
        MaskMode::Luminance,
        "div carries the rule"
    );
    let p = div
        .children
        .iter()
        .find(|c| matches!(&c.kind, BoxKind::Block))
        .expect("p block");
    assert!(
        p.style.mask_layers.is_empty(),
        "child does not inherit the mask"
    );
}

// ──────── CSS Masking L1 §4.9 — multi-layer masks + `mask` shorthand ────────

#[test]
fn mask_image_list_creates_one_layer_per_image() {
    let root = lay(
        "<p>x</p>",
        "p { mask-image: url(a.png), linear-gradient(black, white), none; }",
    );
    let layers = &first_p_style(&root).mask_layers;
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].image, BackgroundImage::Url("a.png".into()));
    assert!(matches!(layers[1].image, BackgroundImage::Gradient(_)));
    assert_eq!(layers[2].image, BackgroundImage::None);
}

#[test]
fn mask_longhands_cycle_over_layers() {
    // 3 слоя, 2 значения repeat → cycling: no-repeat, repeat-x, no-repeat.
    let root = lay(
        "<p>x</p>",
        "p { mask-image: url(a.png), url(b.png), url(c.png);
             mask-repeat: no-repeat, repeat-x; }",
    );
    let layers = &first_p_style(&root).mask_layers;
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(layers[1].repeat, BackgroundRepeat::RepeatX);
    assert_eq!(layers[2].repeat, BackgroundRepeat::NoRepeat);
}

#[test]
fn mask_composite_list_per_layer() {
    let root = lay(
        "<p>x</p>",
        "p { mask-image: url(a.png), url(b.png);
             mask-composite: intersect, subtract; }",
    );
    let layers = &first_p_style(&root).mask_layers;
    assert_eq!(layers[0].composite, MaskComposite::Intersect);
    assert_eq!(layers[1].composite, MaskComposite::Subtract);
}

#[test]
fn mask_composite_default_is_add() {
    let root = lay("<p>x</p>", "p { mask-image: url(a.png); }");
    assert_eq!(first_p_mask(&root).composite, MaskComposite::Add);
}

#[test]
fn mask_clip_and_origin_lists() {
    let root = lay(
        "<p>x</p>",
        "p { mask-image: url(a.png), url(b.png);
             mask-origin: content-box, padding-box;
             mask-clip: no-clip, fill-box; }",
    );
    let layers = &first_p_style(&root).mask_layers;
    assert_eq!(layers[0].origin, BackgroundOrigin::ContentBox);
    assert_eq!(layers[1].origin, BackgroundOrigin::PaddingBox);
    assert_eq!(layers[0].clip, MaskClip::NoClip);
    assert_eq!(layers[1].clip, MaskClip::FillBox);
}

#[test]
fn mask_longhand_without_image_creates_a_layer() {
    // Longhand без `mask-image` не должен теряться: создаётся один слой
    // с initial-значениями и применённым longhand-ом.
    let root = lay("<p>x</p>", "p { mask-repeat: no-repeat; }");
    let layers = &first_p_style(&root).mask_layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].image, BackgroundImage::None);
    assert_eq!(layers[0].repeat, BackgroundRepeat::NoRepeat);
}

#[test]
fn mask_shorthand_single_layer() {
    let root = lay(
        "<p>x</p>",
        "p { mask: url(m.png) center / cover no-repeat content-box luminance intersect; }",
    );
    let m = first_p_mask(&root);
    assert_eq!(m.image, BackgroundImage::Url("m.png".into()));
    assert_eq!(m.size, BackgroundSize::Cover);
    assert_eq!(m.repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(m.origin, BackgroundOrigin::ContentBox);
    // Один <geometry-box> задаёт и origin, и clip.
    assert_eq!(m.clip, MaskClip::ContentBox);
    assert_eq!(m.mode, MaskMode::Luminance);
    assert_eq!(m.composite, MaskComposite::Intersect);
}

#[test]
fn mask_shorthand_two_geometry_boxes() {
    let root = lay("<p>x</p>", "p { mask: url(m.png) padding-box no-clip; }");
    let m = first_p_mask(&root);
    assert_eq!(m.origin, BackgroundOrigin::PaddingBox);
    assert_eq!(m.clip, MaskClip::NoClip);
}

#[test]
fn mask_shorthand_no_clip_before_geometry_box() {
    // `||` — порядок свободный: `no-clip` занимает слот clip, поэтому
    // следующий <geometry-box> обязан попасть в origin, а не затереть clip.
    let root = lay("<p>x</p>", "p { mask: url(m.png) no-clip padding-box; }");
    let m = first_p_mask(&root);
    assert_eq!(m.origin, BackgroundOrigin::PaddingBox);
    assert_eq!(m.clip, MaskClip::NoClip);
}

#[test]
fn mask_shorthand_two_geometry_boxes_fill_origin_then_clip() {
    let root = lay("<p>x</p>", "p { mask: url(m.png) padding-box content-box; }");
    let m = first_p_mask(&root);
    assert_eq!(m.origin, BackgroundOrigin::PaddingBox);
    assert_eq!(m.clip, MaskClip::ContentBox);
}

#[test]
fn mask_shorthand_resets_unspecified_longhands() {
    let root = lay(
        "<p>x</p>",
        "p { mask-repeat: no-repeat; mask-mode: luminance; mask: url(m.png); }",
    );
    let m = first_p_mask(&root);
    assert_eq!(m.repeat, BackgroundRepeat::Repeat, "reset to initial");
    assert_eq!(m.mode, MaskMode::Alpha, "reset to initial");
}

#[test]
fn mask_shorthand_multi_layer() {
    let root = lay(
        "<p>x</p>",
        "p { mask: url(a.png) no-repeat, linear-gradient(black, white) subtract; }",
    );
    let layers = &first_p_style(&root).mask_layers;
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].image, BackgroundImage::Url("a.png".into()));
    assert_eq!(layers[0].repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(layers[0].composite, MaskComposite::Add);
    assert!(matches!(layers[1].image, BackgroundImage::Gradient(_)));
    assert_eq!(layers[1].composite, MaskComposite::Subtract);
}

#[test]
fn mask_shorthand_none_clears_the_image() {
    let root = lay("<p>x</p>", "p { mask-image: url(a.png); mask: none; }");
    let layers = &first_p_style(&root).mask_layers;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].image, BackgroundImage::None);
}

#[test]
fn scrollbar_width_thin() {
    let root = lay("<p>x</p>", "p { scrollbar-width: thin; }");
    assert_eq!(first_p_style(&root).scrollbar_width, ScrollbarWidth::Thin);
}

#[test]
fn scrollbar_width_none() {
    let root = lay("<p>x</p>", "p { scrollbar-width: none; }");
    assert_eq!(first_p_style(&root).scrollbar_width, ScrollbarWidth::None);
}

#[test]
fn scrollbar_width_inherited() {
    let root = lay("<div><p>x</p></div>", "div { scrollbar-width: thin; }");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.scrollbar_width, ScrollbarWidth::Thin);
}

#[test]
fn scrollbar_color_pair() {
    let root = lay(
        "<p>x</p>",
        "p { scrollbar-color: red blue; }",
    );
    let (thumb, track) = first_p_style(&root).scrollbar_color.unwrap();
    assert_eq!(thumb, Color { r: 255, g: 0, b: 0, a: 255 });
    assert_eq!(track, Color { r: 0, g: 0, b: 255, a: 255 });
}

#[test]
fn scrollbar_color_with_rgb_functions() {
    let root = lay(
        "<p>x</p>",
        "p { scrollbar-color: rgb(100, 100, 100) rgb(200, 200, 200); }",
    );
    let (thumb, _) = first_p_style(&root).scrollbar_color.unwrap();
    assert_eq!(thumb, Color { r: 100, g: 100, b: 100, a: 255 });
}

#[test]
fn scrollbar_color_auto() {
    let root = lay("<p>x</p>", "p { scrollbar-color: red blue; scrollbar-color: auto; }");
    assert!(first_p_style(&root).scrollbar_color.is_none());
}

#[test]
fn scrollbar_gutter_stable() {
    let root = lay("<p>x</p>", "p { scrollbar-gutter: stable; }");
    assert_eq!(first_p_style(&root).scrollbar_gutter, ScrollbarGutter::Stable);
}

#[test]
fn scrollbar_gutter_stable_both_edges() {
    let root = lay("<p>x</p>", "p { scrollbar-gutter: stable both-edges; }");
    assert_eq!(
        first_p_style(&root).scrollbar_gutter,
        ScrollbarGutter::StableBothEdges
    );
}

// ──────── scrollbar-gutter layout algorithm ────────

/// `scrollbar-gutter: stable` + `overflow-y: scroll` reserves 12px (auto gutter)
/// in the inline axis so children are narrower than the container's content edge.
#[test]
fn scrollbar_gutter_stable_reduces_child_width() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    // 200 border-box → content = 200; minus 12 gutter = 188.
    assert!((div.rect.width - 200.0).abs() < 0.01, "div={}", div.rect.width);
    assert!((p.rect.width - 188.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// `scrollbar-gutter: auto` (default) with overlay scrollbars = no gutter reserved.
#[test]
fn scrollbar_gutter_auto_no_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    // No gutter reserved: child fills full content width.
    assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// `scrollbar-width: none` suppresses the gutter even with `scrollbar-gutter: stable`.
#[test]
fn scrollbar_gutter_stable_none_no_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; scrollbar-width: none; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// `scrollbar-gutter: stable both-edges` reserves gutter on start AND end of
/// the inline axis (2 × 12 = 24 px).
#[test]
fn scrollbar_gutter_stable_both_edges_double_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable both-edges; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    // 200 − 12*2 = 176.
    assert!((p.rect.width - 176.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// `stable both-edges` must not just narrow the child but also shift it past
/// the mirrored start-edge gutter — plain `stable`'s end-edge-only reservation
/// leaves the child flush against the same start edge, but `both-edges`
/// spec-requires the content to start further in (WPT
/// `scrollbar-gutter-001.html` asserts `container.offsetLeft <
/// content.offsetLeft`, BUG-504).
#[test]
fn scrollbar_gutter_stable_both_edges_shifts_child_start_edge() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable both-edges; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    assert!(
        (p.rect.x - (div.rect.x + 12.0)).abs() < 0.01,
        "div.x={} p.x={}",
        div.rect.x,
        p.rect.x
    );
}

/// Plain `stable` (end-edge-only) must NOT shift the child's start edge —
/// only `both-edges` mirrors the gutter onto the start.
#[test]
fn scrollbar_gutter_stable_single_edge_no_start_shift() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    assert!(
        (p.rect.x - div.rect.x).abs() < 0.01,
        "div.x={} p.x={}",
        div.rect.x,
        p.rect.x
    );
}

/// Under `direction: rtl`, plain `stable`'s single-edge gutter lands on the
/// physical *left* (inline-end there), so unlike the LTR case the child's
/// start edge DOES shift — by the full unit, not half (WPT
/// `css/css-overflow/scrollbar-gutter-rtl-001.html` "overflow scroll,
/// scrollbar-gutter stable": `container.offsetLeft < content.offsetLeft`).
#[test]
fn scrollbar_gutter_stable_rtl_shifts_child_start_edge() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; direction: rtl; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    assert!(
        (p.rect.x - (div.rect.x + 12.0)).abs() < 0.01,
        "div.x={} p.x={}",
        div.rect.x,
        p.rect.x
    );
}

/// `stable both-edges` shifts the start edge by one unit regardless of
/// direction — the reservation is symmetric on both physical sides, so RTL
/// must behave identically to the existing LTR
/// `scrollbar_gutter_stable_both_edges_shifts_child_start_edge` case.
#[test]
fn scrollbar_gutter_stable_both_edges_rtl_shifts_child_start_edge() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable both-edges; direction: rtl; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    assert!(
        (p.rect.x - (div.rect.x + 12.0)).abs() < 0.01,
        "div.x={} p.x={}",
        div.rect.x,
        p.rect.x
    );
}

/// `scrollbar-width: thin` uses 6 px gutter instead of 12.
#[test]
fn scrollbar_gutter_stable_thin_reduces_by_6() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: scroll; scrollbar-gutter: stable; scrollbar-width: thin; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    // 200 − 6 = 194.
    assert!((p.rect.width - 194.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// Without `overflow-y: scroll/auto`, `scrollbar-gutter: stable` has no effect.
#[test]
fn scrollbar_gutter_stable_no_scroll_no_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; scrollbar-gutter: stable; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// Block-axis gutter: `overflow-x: scroll` + `scrollbar-gutter: stable` reserves
/// space for the horizontal scrollbar, so a `%`-height child shrinks by 12 px
/// while the container's own border-box height stays put.
#[test]
fn scrollbar_gutter_block_stable_reduces_child_height() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable; } p { height: 100%; }",
    );
    let div = first_element_child(&root);
    let p = first_element_child(div);
    // 200 content-box → minus 12 block gutter = 188.
    assert!((div.rect.height - 200.0).abs() < 0.01, "div={}", div.rect.height);
    assert!((p.rect.height - 188.0).abs() < 0.01, "p child={}", p.rect.height);
}

/// `both-edges` doubles the block-axis gutter too, same as the inline axis —
/// see `scrollbar_gutter_block`'s doc comment (corrected BUG-504 remainder,
/// part 5: WPT `scrollbar-gutter-vertical-{lr,rl}-001.html` requires the
/// doubling to hold on the axis where it's actually the block axis). This
/// `div` is `horizontal-tb` (default), so `scrollbar_gutter_block` is only
/// exercised here as `layout_dispatch.rs`'s cross-axis leak (§ predicates.rs
/// doc comment) — the writing-mode-correct exercise of this doubling lives in
/// `vertical.rs`'s own test module. 200 − 2×12 = 176.
#[test]
fn scrollbar_gutter_block_both_edges_double_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable both-edges; } p { height: 100%; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.height - 176.0).abs() < 0.01, "p child={}", p.rect.height);
}

/// `scrollbar-width: thin` uses a 6 px block-axis gutter. 200 − 6 = 194.
#[test]
fn scrollbar_gutter_block_thin_reduces_by_6() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable; scrollbar-width: thin; } p { height: 100%; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.height - 194.0).abs() < 0.01, "p child={}", p.rect.height);
}

/// Without `overflow-x: scroll/auto`, block-axis `scrollbar-gutter: stable` has
/// no effect: the `%`-height child fills the full content height.
#[test]
fn scrollbar_gutter_block_no_scroll_no_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { height: 200px; scrollbar-gutter: stable; } p { height: 100%; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.height - 200.0).abs() < 0.01, "p child={}", p.rect.height);
}

/// `scrollbar-width: none` suppresses the block-axis gutter even with
/// `overflow-x: scroll` + `scrollbar-gutter: stable`.
#[test]
fn scrollbar_gutter_block_width_none_no_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable; scrollbar-width: none; } p { height: 100%; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.height - 200.0).abs() < 0.01, "p child={}", p.rect.height);
}

/// `overflow-y: hidden` still establishes a scroll container (CSS Overflow L3
/// §3.3 — programmatically scrollable via script even without a painted
/// scrollbar), so `scrollbar-gutter: stable` reserves its gutter the same as
/// `scroll`/`auto`. WPT `css/css-overflow/scrollbar-gutter-001.html` "overflow
/// hidden, scrollbar-gutter stable" (BUG-504).
#[test]
fn scrollbar_gutter_stable_reduces_child_width_overflow_hidden() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: hidden; scrollbar-gutter: stable; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.width - 188.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// `overflow-y: visible` never establishes a scroll container, so
/// `scrollbar-gutter: stable` has no effect regardless.
#[test]
fn scrollbar_gutter_stable_no_reduction_overflow_visible() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow-y: visible; scrollbar-gutter: stable; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// `overflow: clip` explicitly disables the scrolling machinery (CSS Overflow
/// L3 §3.4) — it can never show a scrollbar, so `scrollbar-gutter: stable`
/// must not reserve a gutter for it either.
#[test]
fn scrollbar_gutter_stable_no_reduction_overflow_clip() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { width: 200px; overflow: clip; scrollbar-gutter: stable; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.width - 200.0).abs() < 0.01, "p child={}", p.rect.width);
}

/// Block-axis mirror of `scrollbar_gutter_stable_reduces_child_width_overflow_hidden`:
/// `overflow-x: hidden` + `scrollbar-gutter: stable` reserves the block-axis gutter.
#[test]
fn scrollbar_gutter_block_stable_reduces_child_height_overflow_hidden() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { height: 200px; overflow-x: hidden; scrollbar-gutter: stable; } p { height: 100%; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.height - 188.0).abs() < 0.01, "p child={}", p.rect.height);
}

// ──────── scrollbar-gutter propagation from `:root` to the viewport ────────

/// `scrollbar-gutter` on `:root` (`<html>`) reserves its gutter against the
/// **viewport**, not just its own children — `document.documentElement`
/// itself comes out narrower, and `<body>` must NOT be narrowed a *second*
/// time between `<html>` and `<body>` (BUG-504, `scrollbar-gutter-propagation-*`).
#[test]
fn scrollbar_gutter_root_stable_propagates_to_viewport_width() {
    let root = lay_full("<p>x</p>", "html { scrollbar-gutter: stable; } body { margin: 0; }");
    let html_box = first_element_child(&root);
    let body_box = first_element_child(html_box);
    assert!((html_box.rect.width - 788.0).abs() < 0.01, "html={}", html_box.rect.width);
    assert!(
        (body_box.rect.width - html_box.rect.width).abs() < 0.01,
        "body={} html={} (body must not be narrowed a second time)",
        body_box.rect.width,
        html_box.rect.width
    );
}

/// `stable both-edges` on `:root` mirrors the same double-reservation and
/// start-edge shift that a plain scrolling element gets
/// (`scrollbar_gutter_stable_both_edges_shifts_child_start_edge`), applied to
/// `<html>` against the viewport instead of a child against its container.
#[test]
fn scrollbar_gutter_root_stable_both_edges_shifts_viewport_start_edge() {
    let root = lay_full(
        "<p>x</p>",
        "html { scrollbar-gutter: stable both-edges; } body { margin: 0; }",
    );
    let html_box = first_element_child(&root);
    assert!((html_box.rect.width - 776.0).abs() < 0.01, "html={}", html_box.rect.width);
    assert!((html_box.rect.x - 12.0).abs() < 0.01, "html.x={}", html_box.rect.x);
}

/// `scrollbar-gutter` declared on `<body>` (or deeper) must NOT propagate to
/// the viewport — only `:root`'s own value counts (WPT
/// `scrollbar-gutter-propagation-006.html`).
#[test]
fn scrollbar_gutter_body_only_does_not_propagate_to_viewport() {
    let root = lay_full(
        "<p>x</p>",
        "body { scrollbar-gutter: stable; overflow-y: scroll; margin: 0; }",
    );
    let html_box = first_element_child(&root);
    assert!((html_box.rect.width - 800.0).abs() < 0.01, "html={}", html_box.rect.width);
}

// ──────── transform-origin / perspective / list-style-* / transition-* ────────

#[test]
fn transform_origin_x_y_z() {
    let root = lay("<p>x</p>", "p { transform-origin: 10px 20px 30px; }");
    let o = first_p_style(&root).transform_origin;
    assert_eq!(o.0, PositionComponent::Px(10.0));
    assert_eq!(o.1, PositionComponent::Px(20.0));
    assert!((o.2 - 30.0).abs() < 1e-5);
}

#[test]
fn transform_origin_single_value_y_defaults_to_center() {
    // CSS Transforms L1 §6: single value applies to x, y defaults to center (50%).
    let root = lay("<p>x</p>", "p { transform-origin: 50px; }");
    let o = first_p_style(&root).transform_origin;
    assert_eq!(o.0, PositionComponent::Px(50.0));
    assert_eq!(o.1, PositionComponent::Percent(0.5));
}

#[test]
fn transform_origin_not_inherited() {
    let root = lay("<div><p>x</p></div>", "div { transform-origin: 10px 20px; }");
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    // Non-inherited: <p> gets initial value 50% 50%.
    assert_eq!(p.style.transform_origin.0, PositionComponent::Percent(0.5));
    assert_eq!(p.style.transform_origin.1, PositionComponent::Percent(0.5));
    assert_eq!(div.style.transform_origin.0, PositionComponent::Px(10.0));
    assert_eq!(div.style.transform_origin.1, PositionComponent::Px(20.0));
}

#[test]
fn perspective_length() {
    let root = lay("<p>x</p>", "p { perspective: 800px; }");
    assert_eq!(first_p_style(&root).perspective, Some(800.0));
}

#[test]
fn perspective_none() {
    let root = lay("<p>x</p>", "p { perspective: 800px; perspective: none; }");
    assert_eq!(first_p_style(&root).perspective, None);
}

#[test]
fn perspective_zero_treated_as_none() {
    let root = lay("<p>x</p>", "p { perspective: 0px; }");
    assert_eq!(first_p_style(&root).perspective, None);
}

#[test]
fn list_style_type_decimal() {
    let root = lay("<p>x</p>", "p { list-style-type: decimal; }");
    assert_eq!(first_p_style(&root).list_style_type, ListStyleType::Decimal);
}

#[test]
fn list_style_type_none() {
    let root = lay("<p>x</p>", "p { list-style-type: none; }");
    assert_eq!(first_p_style(&root).list_style_type, ListStyleType::None);
}

#[test]
fn list_style_type_lower_roman() {
    let root = lay("<p>x</p>", "p { list-style-type: lower-roman; }");
    assert_eq!(first_p_style(&root).list_style_type, ListStyleType::LowerRoman);
}

#[test]
fn list_style_position_inside() {
    let root = lay("<p>x</p>", "p { list-style-position: inside; }");
    assert_eq!(first_p_style(&root).list_style_position, ListStylePosition::Inside);
}

#[test]
fn list_style_image_url() {
    let root = lay("<p>x</p>", "p { list-style-image: url(\"bullet.png\"); }");
    assert_eq!(
        first_p_style(&root).list_style_image,
        Some("bullet.png".to_string())
    );
}

#[test]
fn list_style_shorthand_combines() {
    let root = lay("<p>x</p>", "p { list-style: square inside; }");
    let s = first_p_style(&root);
    assert_eq!(s.list_style_type, ListStyleType::Square);
    assert_eq!(s.list_style_position, ListStylePosition::Inside);
}

#[test]
fn list_style_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { list-style-type: square; }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert_eq!(p.style.list_style_type, ListStyleType::Square);
}

#[test]
fn transition_property_single() {
    let root = lay("<p>x</p>", "p { transition-property: opacity; }");
    assert_eq!(
        first_p_style(&root).transition_properties,
        vec!["opacity".to_string()]
    );
}

#[test]
fn transition_property_list() {
    let root = lay("<p>x</p>", "p { transition-property: opacity, transform, color; }");
    let s = first_p_style(&root);
    assert_eq!(s.transition_properties.len(), 3);
    assert_eq!(s.transition_properties[0], "opacity");
    assert_eq!(s.transition_properties[2], "color");
}

#[test]
fn transition_property_none_clears() {
    let root = lay(
        "<p>x</p>",
        "p { transition-property: opacity; transition-property: none; }",
    );
    assert!(first_p_style(&root).transition_properties.is_empty());
}

#[test]
fn transition_duration_seconds_and_ms() {
    let root = lay("<p>x</p>", "p { transition-duration: 0.5s, 200ms, 1s; }");
    let durations = &first_p_style(&root).transition_durations;
    assert_eq!(durations.len(), 3);
    assert!((durations[0] - 0.5).abs() < 1e-5);
    assert!((durations[1] - 0.2).abs() < 1e-5);
    assert!((durations[2] - 1.0).abs() < 1e-5);
}

#[test]
fn transition_delay_parses() {
    let root = lay("<p>x</p>", "p { transition-delay: 100ms; }");
    let s = first_p_style(&root);
    assert!((s.transition_delays[0] - 0.1).abs() < 1e-5);
}

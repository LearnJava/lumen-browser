use super::*;

// ──────── clip-path / transform / filter ────────

pub(crate) fn first_p_style(root: &LayoutBox) -> &ComputedStyle {
    let p = root
        .children
        .iter()
        .find(|c| matches!(&c.kind, BoxKind::Block))
        .expect("p block");
    &p.style
}

#[test]
fn clip_path_inset_parses() {
    let root = lay("<p>x</p>", "p { clip-path: inset(10px 20px 30px 40px); }");
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Inset(parts)) => {
            assert_eq!(
                parts,
                vec![
                    ShapeValue::Px(10.0),
                    ShapeValue::Px(20.0),
                    ShapeValue::Px(30.0),
                    ShapeValue::Px(40.0)
                ]
            );
        }
        _ => panic!("expected Inset, got {cp:?}"),
    }
}

#[test]
fn clip_path_circle_with_center() {
    let root = lay("<p>x</p>", "p { clip-path: circle(50px at 100px 200px); }");
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Circle { radius, center }) => {
            assert_eq!(radius, ShapeValue::Px(50.0));
            assert_eq!(center, Some((ShapeValue::Px(100.0), ShapeValue::Px(200.0))));
        }
        _ => panic!("expected Circle, got {cp:?}"),
    }
}

/// BUG-140: `circle(40% at 50% 50%)` (TEST-109 c0) раньше молча
/// отбрасывался целиком — проценты не парсились.
#[test]
fn clip_path_circle_percent() {
    let root = lay("<p>x</p>", "p { clip-path: circle(40% at 50% 50%); }");
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Circle { radius, center }) => {
            assert_eq!(radius, ShapeValue::Pct(40.0));
            assert_eq!(center, Some((ShapeValue::Pct(50.0), ShapeValue::Pct(50.0))));
        }
        _ => panic!("expected Circle, got {cp:?}"),
    }
}

#[test]
fn clip_path_ellipse() {
    let root = lay("<p>x</p>", "p { clip-path: ellipse(30px 60px); }");
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Ellipse { rx, ry, center: None }) => {
            assert_eq!(rx, ShapeValue::Px(30.0));
            assert_eq!(ry, ShapeValue::Px(60.0));
        }
        _ => panic!("expected Ellipse, got {cp:?}"),
    }
}

#[test]
fn clip_path_polygon() {
    let root = lay(
        "<p>x</p>",
        "p { clip-path: polygon(0 0, 100px 0, 50px 100px); }",
    );
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Polygon(verts, rule)) => {
            assert_eq!(verts.len(), 3);
            assert_eq!(verts[0], (ShapeValue::Px(0.0), ShapeValue::Px(0.0)));
            assert_eq!(verts[1], (ShapeValue::Px(100.0), ShapeValue::Px(0.0)));
            assert_eq!(verts[2], (ShapeValue::Px(50.0), ShapeValue::Px(100.0)));
            assert_eq!(rule, FillRule::NonZero, "default fill-rule = nonzero");
        }
        _ => panic!("expected Polygon, got {cp:?}"),
    }
}

/// BUG-140: `polygon(50% 0%, 100% 100%, 0% 100%)` (TEST-109 c2) раньше
/// молча отбрасывался целиком — проценты не парсились.
#[test]
fn clip_path_polygon_percent() {
    let root = lay(
        "<p>x</p>",
        "p { clip-path: polygon(50% 0%, 100% 100%, 0% 100%); }",
    );
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Polygon(verts, _)) => {
            assert_eq!(verts.len(), 3);
            assert_eq!(verts[0], (ShapeValue::Pct(50.0), ShapeValue::Pct(0.0)));
            assert_eq!(verts[1], (ShapeValue::Pct(100.0), ShapeValue::Pct(100.0)));
            assert_eq!(verts[2], (ShapeValue::Pct(0.0), ShapeValue::Pct(100.0)));
        }
        _ => panic!("expected Polygon, got {cp:?}"),
    }
}

#[test]
fn clip_path_path_triangle() {
    // CSS Shapes L1 §4 — path() флэттится в полигон; прямые сегменты
    // (M/L/Z) сохраняют вершины 1:1.
    let root = lay(
        "<p>x</p>",
        r#"p { clip-path: path("M 0 0 L 100 0 L 50 80 Z"); }"#,
    );
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Path(pts, rule)) => {
            assert!(pts.contains(&(0.0, 0.0)));
            assert!(pts.contains(&(100.0, 0.0)));
            assert!(pts.contains(&(50.0, 80.0)));
            assert_eq!(rule, FillRule::NonZero, "default fill-rule = nonzero");
        }
        _ => panic!("expected Path, got {cp:?}"),
    }
}

#[test]
fn clip_path_path_with_fill_rule() {
    // CSS Shapes L1 §4 — опциональный fill-rule перед строкой пути
    // сохраняется и управляет заливкой самопересекающихся путей.
    let root = lay(
        "<p>x</p>",
        r#"p { clip-path: path(evenodd, "M 0 0 L 10 0 L 10 10 Z"); }"#,
    );
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Path(_, rule)) => {
            assert_eq!(rule, FillRule::EvenOdd, "evenodd должен сохраниться");
        }
        _ => panic!("expected Path, got {cp:?}"),
    }
}

#[test]
fn clip_path_polygon_evenodd() {
    // CSS Shapes L1 §3 — polygon() принимает опциональный fill-rule.
    let root = lay(
        "<p>x</p>",
        "p { clip-path: polygon(evenodd, 0 0, 100px 0, 50px 100px); }",
    );
    let cp = first_p_style(&root).clip_path.clone();
    match cp {
        Some(ClipPath::Polygon(verts, rule)) => {
            assert_eq!(verts.len(), 3, "fill-rule не должен поглотить вершину");
            assert_eq!(rule, FillRule::EvenOdd);
        }
        _ => panic!("expected Polygon, got {cp:?}"),
    }
}

#[test]
fn clip_path_path_degenerate_rejected() {
    // Путь без замкнутой области (< 3 точек) не создаёт клип.
    let root = lay("<p>x</p>", r#"p { clip-path: path("M 0 0"); }"#);
    assert_eq!(first_p_style(&root).clip_path, None);
}

#[test]
fn clip_path_none_clears() {
    let root = lay("<p>x</p>", "p { clip-path: circle(50px); clip-path: none; }");
    assert_eq!(first_p_style(&root).clip_path, None);
}

#[test]
fn transform_translate() {
    let root = lay("<p>x</p>", "p { transform: translate(10px, 20px); }");
    let t = first_p_style(&root).transform.clone();
    assert_eq!(t, vec![TransformFn::Translate(10.0, 20.0)]);
}

#[test]
fn transform_rotate_normalizes_to_radians() {
    let root = lay("<p>x</p>", "p { transform: rotate(90deg); }");
    let t = first_p_style(&root).transform.clone();
    match &t[..] {
        [TransformFn::Rotate(rad)] => {
            assert!((rad - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        }
        _ => panic!("expected single Rotate, got {t:?}"),
    }
}

#[test]
fn transform_scale_single_arg_uniform() {
    let root = lay("<p>x</p>", "p { transform: scale(1.5); }");
    let t = first_p_style(&root).transform.clone();
    assert_eq!(t, vec![TransformFn::Scale(1.5, 1.5)]);
}

#[test]
fn transform_scale_two_args() {
    let root = lay("<p>x</p>", "p { transform: scale(2, 0.5); }");
    let t = first_p_style(&root).transform.clone();
    assert_eq!(t, vec![TransformFn::Scale(2.0, 0.5)]);
}

#[test]
fn transform_matrix() {
    let root = lay("<p>x</p>", "p { transform: matrix(1, 0, 0, 1, 50, 100); }");
    let t = first_p_style(&root).transform.clone();
    assert_eq!(
        t,
        vec![TransformFn::Matrix([1.0, 0.0, 0.0, 1.0, 50.0, 100.0])]
    );
}

#[test]
fn transform_list_multiple() {
    let root = lay(
        "<p>x</p>",
        "p { transform: translate(10px, 0) rotate(45deg) scale(2); }",
    );
    let t = first_p_style(&root).transform.clone();
    assert_eq!(t.len(), 3);
    assert!(matches!(t[0], TransformFn::Translate(_, _)));
    assert!(matches!(t[1], TransformFn::Rotate(_)));
    assert!(matches!(t[2], TransformFn::Scale(_, _)));
}

#[test]
fn transform_none_clears() {
    let root = lay(
        "<p>x</p>",
        "p { transform: rotate(45deg); transform: none; }",
    );
    assert!(first_p_style(&root).transform.is_empty());
}

#[test]
fn translate_prop_xy() {
    let root = lay("<p>x</p>", "p { translate: 10px 20px; }");
    assert_eq!(first_p_style(&root).translate, Some((10.0, 20.0)));
}

#[test]
fn translate_prop_single_value_defaults_y_to_zero() {
    let root = lay("<p>x</p>", "p { translate: 5px; }");
    assert_eq!(first_p_style(&root).translate, Some((5.0, 0.0)));
}

#[test]
fn translate_prop_none_clears() {
    let root = lay("<p>x</p>", "p { translate: 10px; translate: none; }");
    assert_eq!(first_p_style(&root).translate, None);
}

#[test]
fn rotate_prop_degrees() {
    let root = lay("<p>x</p>", "p { rotate: 90deg; }");
    let r = first_p_style(&root).rotate.expect("rotate should be Some");
    assert!((r - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "expected π/2, got {r}");
}

#[test]
fn rotate_prop_none_clears() {
    let root = lay("<p>x</p>", "p { rotate: 45deg; rotate: none; }");
    assert_eq!(first_p_style(&root).rotate, None);
}

#[test]
fn scale_prop_uniform() {
    let root = lay("<p>x</p>", "p { scale: 2; }");
    assert_eq!(first_p_style(&root).scale, Some((2.0, 2.0)));
}

#[test]
fn scale_prop_non_uniform() {
    let root = lay("<p>x</p>", "p { scale: 1.5 0.5; }");
    assert_eq!(first_p_style(&root).scale, Some((1.5, 0.5)));
}

#[test]
fn scale_prop_none_clears() {
    let root = lay("<p>x</p>", "p { scale: 2; scale: none; }");
    assert_eq!(first_p_style(&root).scale, None);
}

#[test]
fn individual_transforms_not_inherited() {
    // div has all three individual props; nested p should NOT inherit them
    let root = lay(
        "<div><p>x</p></div>",
        "div { translate: 10px; rotate: 45deg; scale: 2; } p { color: red; }",
    );
    // first_p_style returns the first Block child = the div wrapper
    // then its child = the p block. We need the p inside div.
    let div_box = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).expect("div");
    assert_eq!(div_box.style.translate, Some((10.0, 0.0)));
    let p_box = div_box.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).expect("p");
    assert_eq!(p_box.style.translate, None, "translate must not be inherited");
    assert_eq!(p_box.style.rotate, None, "rotate must not be inherited");
    assert_eq!(p_box.style.scale, None, "scale must not be inherited");
}

/// BUG-188 / TEST-46 regression: individual transform properties compose with
/// the `transform` property in the spec order (translate → rotate → scale →
/// transform), all wrapped by the shared `transform-origin` pivot
/// (CSS Transforms L2 §3). For the TEST-46 `t-individual-plus-transform` box
/// (`translate: 15px 0; scale: 0.9; transform: rotate(15deg)`) this means:
/// the box centre — which is also the default `50% 50%` pivot — must map to
/// `centre + (15, 0)` (scale/rotate keep the pivot fixed, only the leading
/// translate moves it), and the linear part must be `scale(0.9)·rotate(15deg)`.
/// Locks the composition so a future refactor can't silently reorder it; the
/// remaining TEST-46 pixel diff is font-parity (BUG-128), not transform math.
#[test]
fn individual_plus_transform_composes_translate_then_scale_then_rotate() {
    let root = lay(
        "<div>x</div>",
        "div { width: 80px; height: 80px; translate: 15px 0px; scale: 0.9; \
               transform: rotate(15deg); }",
    );
    let div = root
        .children
        .iter()
        .find(|c| matches!(&c.kind, BoxKind::Block))
        .expect("div box");
    let m = forward_box_transform(div).expect("transformed box has a matrix");

    // Box centre = default transform-origin pivot.
    let cx = div.rect.x + div.rect.width / 2.0;
    let cy = div.rect.y + div.rect.height / 2.0;
    let (mx, my) = m.transform_point_2d(cx, cy);
    // Centre moves by exactly the individual `translate` (scale+rotate pivot
    // about the centre, so they leave it fixed). Wrong order/pivot would shift it.
    assert!(
        (mx - (cx + 15.0)).abs() < 0.05 && (my - cy).abs() < 0.05,
        "centre must map to centre+(15,0); got ({mx}, {my}) vs ({}, {cy})",
        cx + 15.0
    );

    // Linear part = scale(0.9) · rotate(15deg). cos15≈0.96593, sin15≈0.25882.
    let (lx, ly) = m.transform_point_2d(cx + 1.0, cy);
    let a = lx - mx; // d(x')/dx
    let b = ly - my; // d(y')/dx
    assert!(
        (a - 0.9 * 0.96593).abs() < 1e-3 && (b - 0.9 * 0.25882).abs() < 1e-3,
        "linear column must be scale(0.9)·rotate(15deg); got a={a}, b={b}"
    );
}

/// BUG-125 / TEST-76 regression: CSS Motion Path L1 places the box's
/// `offset-anchor` (default `auto` = `transform-origin` = centre) ONTO the
/// path point — not the box's top-left corner. The path coordinate origin is
/// the box's normal position, so the centre of a box on
/// `offset-path: path("M 0 0 L 960 0")` at `offset-distance: 480px` must map
/// to `rect_topleft + (480, 0)`. Without the `T(-anchor)` term the box sat
/// half-a-box down-and-right of Edge (the original 3.18% TEST-76 diff).
#[test]
fn motion_path_centres_anchor_on_path_point() {
    let root = lay(
        "<div>x</div>",
        r#"div { width: 40px; height: 40px; offset-path: path("M 0 0 L 960 0"); offset-distance: 480px; offset-rotate: 0deg; }"#,
    );
    let div = root
        .children
        .iter()
        .find(|c| matches!(&c.kind, BoxKind::Block))
        .expect("div box");
    let m = forward_box_transform(div).expect("motion-path box has a matrix");

    // Box centre (= default anchor) must land on the path point, which is
    // `rect_topleft + (480, 0)` — NOT `rect_topleft + centre + (480, 0)`.
    let cx = div.rect.x + div.rect.width / 2.0;
    let cy = div.rect.y + div.rect.height / 2.0;
    let (mx, my) = m.transform_point_2d(cx, cy);
    let (ex, ey) = (div.rect.x + 480.0, div.rect.y);
    assert!(
        (mx - ex).abs() < 0.05 && (my - ey).abs() < 0.05,
        "anchor must map to path point ({ex}, {ey}); got ({mx}, {my})"
    );
}

#[test]
fn filter_blur() {
    let root = lay("<p>x</p>", "p { filter: blur(5px); }");
    let f = first_p_style(&root).filter.clone();
    assert_eq!(f, vec![FilterFn::Blur(5.0)]);
}

#[test]
fn filter_percentage_normalized() {
    let root = lay("<p>x</p>", "p { filter: grayscale(50%); }");
    let f = first_p_style(&root).filter.clone();
    match &f[..] {
        [FilterFn::Grayscale(v)] => assert!((v - 0.5).abs() < 1e-5),
        _ => panic!("expected Grayscale, got {f:?}"),
    }
}

#[test]
fn filter_chain() {
    let root = lay(
        "<p>x</p>",
        "p { filter: blur(2px) brightness(1.2) saturate(0.8); }",
    );
    let f = first_p_style(&root).filter.clone();
    assert_eq!(f.len(), 3);
    assert!(matches!(f[0], FilterFn::Blur(_)));
    assert!(matches!(f[1], FilterFn::Brightness(_)));
    assert!(matches!(f[2], FilterFn::Saturate(_)));
}

#[test]
fn filter_hue_rotate_radians() {
    let root = lay("<p>x</p>", "p { filter: hue-rotate(180deg); }");
    let f = first_p_style(&root).filter.clone();
    match &f[..] {
        [FilterFn::HueRotate(rad)] => {
            assert!((rad - std::f32::consts::PI).abs() < 1e-5);
        }
        _ => panic!("expected HueRotate, got {f:?}"),
    }
}

#[test]
fn filter_none_clears() {
    let root = lay("<p>x</p>", "p { filter: blur(5px); filter: none; }");
    assert!(first_p_style(&root).filter.is_empty());
}

#[test]
fn filter_unknown_skipped() {
    let root = lay("<p>x</p>", "p { filter: blur(5px) zomg(1); brightness(1); }");
    // zomg() игнорируется, остальное парсится.
    let f = first_p_style(&root).filter.clone();
    // brightness вне filter declaration — отдельный selector? Нет,
    // оно в той же декларации `filter: blur(5px) zomg(1)` — zomg
    // skipped, blur остался.
    assert!(matches!(f[0], FilterFn::Blur(_)));
}

#[test]
fn clip_transform_filter_not_inherited() {
    // Эти свойства не наследуются.
    let root = lay(
        "<div><p>x</p></div>",
        "div { clip-path: circle(50px); transform: rotate(45deg); filter: blur(5px); }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert!(p.style.clip_path.is_none());
    assert!(p.style.transform.is_empty());
    assert!(p.style.filter.is_empty());
    assert!(div.style.clip_path.is_some());
    assert!(!div.style.transform.is_empty());
    assert!(!div.style.filter.is_empty());
}

// ──────── backdrop-filter ────────

#[test]
fn backdrop_filter_blur_parsed() {
    let root = lay("<p>x</p>", "p { backdrop-filter: blur(10px); }");
    let f = first_p_style(&root).backdrop_filter.clone();
    assert_eq!(f, vec![FilterFn::Blur(10.0)]);
}

#[test]
fn backdrop_filter_grayscale_percentage() {
    let root = lay("<p>x</p>", "p { backdrop-filter: grayscale(80%); }");
    let f = first_p_style(&root).backdrop_filter.clone();
    match &f[..] {
        [FilterFn::Grayscale(v)] => assert!((v - 0.8).abs() < 1e-5),
        _ => panic!("expected Grayscale(0.8), got {f:?}"),
    }
}

#[test]
fn backdrop_filter_chain() {
    let root = lay(
        "<p>x</p>",
        "p { backdrop-filter: blur(4px) brightness(1.5) saturate(2); }",
    );
    let f = first_p_style(&root).backdrop_filter.clone();
    assert_eq!(f.len(), 3);
    assert!(matches!(f[0], FilterFn::Blur(_)));
    assert!(matches!(f[1], FilterFn::Brightness(_)));
    assert!(matches!(f[2], FilterFn::Saturate(_)));
}

#[test]
fn backdrop_filter_none_clears() {
    let root = lay("<p>x</p>", "p { backdrop-filter: blur(5px); backdrop-filter: none; }");
    assert!(first_p_style(&root).backdrop_filter.is_empty());
}

#[test]
fn backdrop_filter_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { backdrop-filter: blur(5px); }",
    );
    let div = root.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    let p = div.children.iter().find(|c| matches!(&c.kind, BoxKind::Block)).unwrap();
    assert!(!div.style.backdrop_filter.is_empty(), "div должен иметь backdrop-filter");
    assert!(p.style.backdrop_filter.is_empty(), "p не наследует backdrop-filter");
}

#[test]
fn backdrop_filter_and_filter_independent() {
    let root = lay(
        "<p>x</p>",
        "p { filter: invert(1); backdrop-filter: blur(8px); }",
    );
    let s = first_p_style(&root);
    assert!(!s.filter.is_empty(), "filter должен быть установлен");
    assert!(!s.backdrop_filter.is_empty(), "backdrop-filter должен быть установлен");
    assert!(matches!(s.filter[0], FilterFn::Invert(_)));
    assert!(matches!(s.backdrop_filter[0], FilterFn::Blur(_)));
}

// ──────── gap / aspect-ratio ────────

#[test]
fn gap_shorthand_single_value() {
    let root = lay("<p>x</p>", "p { gap: 10px; }");
    let s = first_p_style(&root);
    assert_eq!(s.row_gap, Length::Px(10.0));
    assert_eq!(s.column_gap, Length::Px(10.0));
}

#[test]
fn gap_shorthand_two_values() {
    let root = lay("<p>x</p>", "p { gap: 10px 20px; }");
    let s = first_p_style(&root);
    assert_eq!(s.row_gap, Length::Px(10.0));
    assert_eq!(s.column_gap, Length::Px(20.0));
}

#[test]
fn row_gap_individual() {
    let root = lay("<p>x</p>", "p { row-gap: 15px; }");
    assert_eq!(first_p_style(&root).row_gap, Length::Px(15.0));
}

#[test]
fn column_gap_individual() {
    let root = lay("<p>x</p>", "p { column-gap: 25px; }");
    assert_eq!(first_p_style(&root).column_gap, Length::Px(25.0));
}

#[test]
fn gap_em_stores_typed() {
    // em хранится как Length::Em и разрешается при layout относительно font-size.
    let root = lay("<p>x</p>", "p { font-size: 20px; gap: 1.5em; }");
    let s = first_p_style(&root);
    assert_eq!(s.row_gap, Length::Em(1.5));
}

#[test]
fn gap_negative_clamped_to_zero() {
    // gap не может быть отрицательным — хранится как Px(0.0).
    let root = lay("<p>x</p>", "p { gap: -5px; }");
    assert_eq!(first_p_style(&root).row_gap, Length::Px(0.0));
}

#[test]
fn aspect_ratio_single_number() {
    let root = lay("<p>x</p>", "p { aspect-ratio: 1.5; }");
    assert_eq!(first_p_style(&root).aspect_ratio, Some((1.5, 1.0)));
}

#[test]
fn aspect_ratio_w_h_pair() {
    let root = lay("<p>x</p>", "p { aspect-ratio: 16 / 9; }");
    assert_eq!(first_p_style(&root).aspect_ratio, Some((16.0, 9.0)));
}

#[test]
fn aspect_ratio_auto() {
    let root = lay("<p>x</p>", "p { aspect-ratio: auto; }");
    assert_eq!(first_p_style(&root).aspect_ratio, None);
}

#[test]
fn aspect_ratio_negative_rejected() {
    let root = lay("<p>x</p>", "p { aspect-ratio: -1 / 2; }");
    assert_eq!(first_p_style(&root).aspect_ratio, None);
}

#[test]
fn aspect_ratio_invalid_kept_unchanged() {
    let root = lay("<p>x</p>", "p { aspect-ratio: 16 / abc; }");
    assert_eq!(first_p_style(&root).aspect_ratio, None);
}

// ──────── CSS Multi-column L1 ────────

#[test]
fn column_count_integer() {
    let root = lay("<p>x</p>", "p { column-count: 3; }");
    assert_eq!(first_p_style(&root).column_count, Some(3));
}

#[test]
fn column_count_auto() {
    let root = lay("<p>x</p>", "p { column-count: auto; }");
    assert_eq!(first_p_style(&root).column_count, None);
}

#[test]
fn column_count_zero_rejected() {
    let root = lay("<p>x</p>", "p { column-count: 0; }");
    assert_eq!(first_p_style(&root).column_count, None);
}

#[test]
fn column_width_length() {
    let root = lay("<p>x</p>", "p { column-width: 200px; }");
    assert_eq!(first_p_style(&root).column_width, Some(Length::Px(200.0)));
}

#[test]
fn column_width_auto() {
    let root = lay("<p>x</p>", "p { column-width: auto; }");
    assert_eq!(first_p_style(&root).column_width, None);
}

#[test]
fn columns_shorthand_both() {
    let root = lay("<p>x</p>", "p { columns: 200px 3; }");
    let s = first_p_style(&root);
    assert_eq!(s.column_width, Some(Length::Px(200.0)));
    assert_eq!(s.column_count, Some(3));
}

#[test]
fn columns_shorthand_width_only() {
    let root = lay("<p>x</p>", "p { columns: 250px; }");
    let s = first_p_style(&root);
    assert_eq!(s.column_width, Some(Length::Px(250.0)));
    assert_eq!(s.column_count, None);
}

#[test]
fn columns_shorthand_count_only() {
    let root = lay("<p>x</p>", "p { columns: 4; }");
    let s = first_p_style(&root);
    assert_eq!(s.column_count, Some(4));
    assert_eq!(s.column_width, None);
}

#[test]
fn column_rule_individual() {
    let root = lay(
        "<p>x</p>",
        "p { column-rule-width: 2px; column-rule-style: solid; }",
    );
    let s = first_p_style(&root);
    assert!((s.column_rule_width - 2.0).abs() < 1e-6);
    assert_eq!(s.column_rule_style, BorderStyle::Solid);
}

#[test]
fn column_rule_shorthand() {
    let root = lay("<p>x</p>", "p { column-rule: 3px dashed; }");
    let s = first_p_style(&root);
    assert!((s.column_rule_width - 3.0).abs() < 1e-6);
    assert_eq!(s.column_rule_style, BorderStyle::Dashed);
}

#[test]
fn column_span_all() {
    let root = lay("<p>x</p>", "p { column-span: all; }");
    assert!(first_p_style(&root).column_span_all);
}

#[test]
fn column_fill_balance() {
    let root = lay("<p>x</p>", "p { column-fill: balance; }");
    assert!(first_p_style(&root).column_fill_balance);
}

#[test]
fn break_before_avoid() {
    let root = lay("<p>x</p>", "p { break-before: avoid; }");
    assert_eq!(first_p_style(&root).break_before, BreakValue::Avoid);
}

#[test]
fn break_after_page() {
    let root = lay("<p>x</p>", "p { break-after: page; }");
    assert_eq!(first_p_style(&root).break_after, BreakValue::Page);
}

#[test]
fn break_inside_avoid_column() {
    let root = lay("<p>x</p>", "p { break-inside: avoid-column; }");
    assert_eq!(first_p_style(&root).break_inside, BreakValue::Avoid);
}

#[test]
fn column_count_not_inherited() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { column-count: 3; }",
    );
    // Дочерний p не должен унаследовать column-count (CSS Multi-column L1 §3.2 — не наследуется).
    let p_style = nested_p_style(&root);
    assert_eq!(p_style.column_count, None);
}

// ──────── CSS Environment Variables L1 — env() ────────

#[test]
fn env_fallback_used_when_unknown() {
    // env() с unknown name + fallback → fallback применяется.
    let root = lay(
        "<p>x</p>",
        "p { padding: env(safe-area-inset-top, 12px); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(12.0));
}

#[test]
fn env_without_fallback_invalidates_decl() {
    // env() с unknown name и без fallback — декларация невалидна.
    let root = lay(
        "<p>x</p>",
        "p { padding: env(safe-area-inset-top); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(0.0));
}

#[test]
fn env_with_indices_ignored_phase0() {
    // `env(name 0, fallback)` — индекс игнорируется, имя = name.
    let root = lay(
        "<p>x</p>",
        "p { padding: env(viewport-segment-width 0 0, 25px); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(25.0));
}

#[test]
fn env_inside_calc() {
    // calc(env(...) + 5px) — env разворачивается до calc(); resolve = 15px.
    let root = lay(
        "<p>x</p>",
        "p { padding: calc(env(safe-area-inset-top, 10px) + 5px); }",
    );
    let vp = Size::new(800.0, 600.0);
    let v = first_p_style(&root).padding_top.resolve_or_zero(16.0, 0.0, vp);
    assert!((v - 15.0).abs() < 1e-6, "got {v}");
}

#[test]
fn env_inside_var_fallback() {
    // var(--foo, env(name, 8px)) — env как fallback внутри var().
    let root = lay(
        "<p>x</p>",
        "p { padding: var(--missing, env(safe-area-inset-top, 8px)); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(8.0));
}

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

/// `both-edges` is undefined for the block axis: only one gutter unit reserved
/// (unlike the inline axis, which doubles it). 200 − 12 = 188.
#[test]
fn scrollbar_gutter_block_both_edges_single_reduction() {
    let root = lay(
        "<div><p>x</p></div>",
        "div { height: 200px; overflow-x: scroll; scrollbar-gutter: stable both-edges; } p { height: 100%; }",
    );
    let p = first_element_child(first_element_child(&root));
    assert!((p.rect.height - 188.0).abs() < 0.01, "p child={}", p.rect.height);
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

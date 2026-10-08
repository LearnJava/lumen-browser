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

/// Явный `offset-anchor: left top` садит на точку пути верхний-левый угол
/// бокса (а не центр, как при `auto`).
#[test]
fn motion_path_explicit_anchor_places_corner_on_path_point() {
    let root = lay(
        "<div>x</div>",
        r#"div { width: 40px; height: 40px; offset-path: path("M 0 0 L 960 0"); offset-distance: 480px; offset-rotate: 0deg; offset-anchor: left top; }"#,
    );
    let div = root
        .children
        .iter()
        .find(|c| matches!(&c.kind, BoxKind::Block))
        .expect("div box");
    let m = forward_box_transform(div).expect("motion-path box has a matrix");
    let (mx, my) = m.transform_point_2d(div.rect.x, div.rect.y);
    let (ex, ey) = (div.rect.x + 480.0, div.rect.y);
    assert!(
        (mx - ex).abs() < 0.05 && (my - ey).abs() < 0.05,
        "top-left anchor must map to path point ({ex}, {ey}); got ({mx}, {my})"
    );
}

/// `offset-rotate: 90deg` вращает бокс вокруг `offset-anchor`: точка справа
/// от центра уезжает вниз от точки пути (CW в Y-down).
#[test]
fn motion_path_rotate_angle_turns_box_around_anchor() {
    let root = lay(
        "<div>x</div>",
        r#"div { width: 40px; height: 40px; offset-path: path("M 0 0 L 960 0"); offset-distance: 480px; offset-rotate: 90deg; }"#,
    );
    let div = root
        .children
        .iter()
        .find(|c| matches!(&c.kind, BoxKind::Block))
        .expect("div box");
    let m = forward_box_transform(div).expect("motion-path box has a matrix");
    let cx = div.rect.x + div.rect.width / 2.0;
    let cy = div.rect.y + div.rect.height / 2.0;
    let (mx, my) = m.transform_point_2d(cx + 20.0, cy);
    let (ex, ey) = (div.rect.x + 480.0, div.rect.y + 20.0);
    assert!(
        (mx - ex).abs() < 0.05 && (my - ey).abs() < 0.05,
        "90deg must rotate right-edge point below the path point ({ex}, {ey}); got ({mx}, {my})"
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
    assert!((*s.column_rule_width.first() - 2.0).abs() < 1e-6);
    assert_eq!(*s.column_rule_style.first(), BorderStyle::Solid);
}

#[test]
fn column_rule_shorthand() {
    let root = lay("<p>x</p>", "p { column-rule: 3px dashed; }");
    let s = first_p_style(&root);
    assert!((*s.column_rule_width.first() - 3.0).abs() < 1e-6);
    assert_eq!(*s.column_rule_style.first(), BorderStyle::Dashed);
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
        "p { padding: env(unknown-name, 12px); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(12.0));
}

#[test]
fn env_without_fallback_invalidates_decl() {
    // env() с unknown name и без fallback — декларация невалидна.
    let root = lay(
        "<p>x</p>",
        "p { padding: env(unknown-name); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(0.0));
}

#[test]
fn env_with_indices_unknown_name_uses_fallback() {
    // `viewport-segment-*` в реестре нет (один сегмент) → fallback.
    let root = lay(
        "<p>x</p>",
        "p { padding: env(viewport-segment-width 0 0, 25px); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(25.0));
}

#[test]
fn env_safe_area_inset_is_zero_and_ignores_fallback() {
    // UA-реестр: safe-area-inset-* определены (0px на десктопе) —
    // fallback не используется (Chrome: `env(safe-area-inset-top, 20px)` = 0).
    let root = lay(
        "<p>x</p>",
        "p { padding-top: env(safe-area-inset-top, 20px);            padding-right: env(safe-area-inset-right, 21px);            padding-bottom: env(safe-area-inset-bottom, 22px);            padding-left: env(safe-area-inset-left, 23px); }",
    );
    let st = first_p_style(&root);
    assert_eq!(st.padding_top, Length::Px(0.0));
    assert_eq!(st.padding_right, Length::Px(0.0));
    assert_eq!(st.padding_bottom, Length::Px(0.0));
    assert_eq!(st.padding_left, Length::Px(0.0));
}

#[test]
fn env_safe_area_without_fallback_resolves_to_zero() {
    let root = lay("<p>x</p>", "p { padding: env(safe-area-inset-top); }");
    assert_eq!(first_p_style(&root).padding_top, Length::Px(0.0));
    let root = lay(
        "<p>x</p>",
        "p { padding-top: 7px; padding-top: calc(env(safe-area-inset-top) + 3px); }",
    );
    let vp = Size::new(800.0, 600.0);
    let v = first_p_style(&root).padding_top.resolve_or_zero(16.0, 0.0, vp);
    assert!((v - 3.0).abs() < 1e-6, "got {v}");
}

#[test]
fn env_keyboard_inset_is_zero_when_keyboard_hidden() {
    for name in [
        "keyboard-inset-top",
        "keyboard-inset-right",
        "keyboard-inset-bottom",
        "keyboard-inset-left",
        "keyboard-inset-width",
        "keyboard-inset-height",
    ] {
        let css = format!("p {{ padding-top: env({name}, 9px); }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(first_p_style(&root).padding_top, Length::Px(0.0), "{name}");
    }
}

#[test]
fn env_titlebar_area_falls_back_outside_window_controls_overlay() {
    // titlebar-area-* определены только в режиме window-controls-overlay.
    let root = lay("<p>x</p>", "p { padding-top: env(titlebar-area-height, 11px); }");
    assert_eq!(first_p_style(&root).padding_top, Length::Px(11.0));
}

#[test]
fn env_indexed_access_to_non_indexed_variable_uses_fallback() {
    // safe-area-inset-top — скаляр: лишний индекс = несовпадение размерности.
    let root = lay("<p>x</p>", "p { padding-top: env(safe-area-inset-top 0, 13px); }");
    assert_eq!(first_p_style(&root).padding_top, Length::Px(13.0));
}

#[test]
fn env_safe_area_inside_var_fallback_and_custom_property() {
    let root = lay(
        "<p>x</p>",
        "p { --inset: env(safe-area-inset-left, 4px); padding-left: var(--inset); }",
    );
    assert_eq!(first_p_style(&root).padding_left, Length::Px(0.0));
}

#[test]
fn env_inside_calc() {
    // calc(env(...) + 5px) — env разворачивается до calc(); resolve = 15px.
    let root = lay(
        "<p>x</p>",
        "p { padding: calc(env(unknown-name, 10px) + 5px); }",
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
        "p { padding: var(--missing, env(unknown-name, 8px)); }",
    );
    assert_eq!(first_p_style(&root).padding_top, Length::Px(8.0));
}

#[test]
fn env_unknown_name_without_fallback_computes_to_unset() {
    // BUG-514: правильный env() с неизвестным именем и без fallback — invalid
    // at computed-value time: свойство становится `unset` (background-color не
    // наследуется → transparent), а не держит предыдущую декларацию `green`.
    let root = lay(
        "<p>x</p>",
        "p { background-color: green; background-color: env(unknown); }",
    );
    assert_eq!(first_p_style(&root).background_color, None);
}

#[test]
fn env_iacvt_on_inherited_property_takes_parent_value() {
    // `unset` для наследуемого свойства = inherit: цвет родителя, не `blue`.
    let root = lay(
        "<p>x</p>",
        "body { color: rgb(255, 0, 0); } p { color: rgb(0, 0, 255); color: env(unknown); }",
    );
    assert_eq!(first_p_style(&root).color, root.style.color);
    assert_eq!(
        root.style.color,
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255
        }
    );
}

#[test]
fn malformed_env_drops_declaration_keeps_previous() {
    // Кривой env() — ошибка разбора: декларации нет, `green` остаётся.
    for bad in [
        "env(10px)",
        "env(env(test))",
        "env(test, {)",
        "env(test 0.1, blue)",
        "env()",
    ] {
        let css = format!("p {{ background-color: green; background-color: {bad}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(
            first_p_style(&root).background_color,
            Some(CssColor::Rgba(Color {
                r: 0,
                g: 128,
                b: 0,
                a: 255
            })),
            "{bad}"
        );
    }
}

#[test]
fn env_function_name_is_case_insensitive() {
    let root = lay(
        "<p>x</p>",
        "p { background-color: green; background-color: ENV(test, blue); }",
    );
    assert_eq!(
        first_p_style(&root).background_color,
        Some(CssColor::Rgba(Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255
        }))
    );
}

#[test]
fn var_parse_errors_drop_declaration_keeps_previous() {
    // Parse-time ошибки в значении с var() — декларации нет (не IACVT/unset):
    // повторный `!important`, кривая голова var(), bad-string.
    for bad in [
        "var(--a) !important !important",
        "var(--a ())",
        "var(--a(),)",
        "var(--a, \"\n",
    ] {
        let css = format!("p {{ --a: red; background-color: green; background-color: {bad}; }}");
        let root = lay("<p>x</p>", &css);
        assert_eq!(
            first_p_style(&root).background_color,
            Some(CssColor::Rgba(Color {
                r: 0,
                g: 128,
                b: 0,
                a: 255
            })),
            "{bad:?}"
        );
    }
}

#[test]
fn var_unknown_without_fallback_computes_to_unset() {
    let root = lay(
        "<p>x</p>",
        "p { background-color: green; background-color: var(--missing); }",
    );
    assert_eq!(first_p_style(&root).background_color, None);
}

#[test]
fn custom_function_named_argument_syntax_grammar() {
    // CSS Functions and Mixins L1 / csswg-drafts#11749 (WPT
    // `dashed-function-named-arg.tentative.html`).
    use crate::style::env_calls_well_formed as ok;
    for good in [
        "--func(myident)",
        "--func(--myident)",
        "--func(--)",
        "--func(50px --myident:)",
        "--func({--myident:})",
        "--func({ --myident : })",
        "--func(10px, { --myident : })",
    ] {
        assert!(ok(good), "{good} must be accepted");
    }
    for bad in [
        "--func(--myident:)",
        "--func( --myident:)",
        "--func(--myident :)",
        "--func(--myident: )",
        "--func( --myident : )",
        "--func(10px, --myident : )",
        "--a(--b(--x: 1))",
    ] {
        assert!(!ok(bad), "{bad} must be rejected");
    }
}

#[test]
fn env_calls_well_formed_grammar() {
    use crate::style::env_calls_well_formed as ok;
    for good in [
        "red",
        "env(test)",
        "env( test )",
        "env(-test)",
        "env(--test)",
        "env(test, 10px)",
        "env(test,)",
        "env(test, {})",
        "env(test /**/, blue)",
        "env(test 0)",
        "env(test 0 1 2 3 4, green)",
        "env(test, env(another, blue))",
        "calc(env(a, 1px) + 2px)",
        "xenv(10px)",
        "\"env(10px)\"",
        "var(--x)",
        "var(--x,)",
        "var( --x , env(test, 1px))",
    ] {
        assert!(ok(good), "{good} must be accepted");
    }
    for bad in [
        "env()",
        "env(10px)",
        "env(env(test))",
        "env(test, {)",
        "env(test1 test2, green)",
        "env(test 0.1, green)",
        "env(test -1, green)",
        "env(safe-area-inset-top ())",
        "env(safe-area-inset-top(),)",
        "env(test, env(10px))",
        "env(test",
        "var(--x ())",
        "var(--x(),)",
        "var(x)",
        "var(--a, env(10px))",
    ] {
        assert!(!ok(bad), "{bad} must be rejected");
    }
}

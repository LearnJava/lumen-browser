//! Compat Standard §3 легаси-градиенты (CSS-WEBKIT-GRADIENT).

use super::*;
use crate::style::{parse_webkit_gradient, webkit_gradient_value_valid};

fn linear(css: &str) -> (f32, Option<GradientCorner>, Vec<GradientStop>, bool) {
    match parse_background_gradient(css) {
        ParsedGradient::Linear { angle_deg, corner, stops, repeating } => (angle_deg, corner, stops, repeating),
        other => panic!("expected linear for {css}, got {other:?}"),
    }
}

#[test]
fn linear_start_side_is_opposite_of_standard_end() {
    // top = начало сверху = `to bottom`.
    assert_eq!(linear("-webkit-linear-gradient(top, red, blue)").0, 180.0);
    assert_eq!(linear("-webkit-linear-gradient(bottom, red, blue)").0, 0.0);
    assert_eq!(linear("-webkit-linear-gradient(left, red, blue)").0, 90.0);
    assert_eq!(linear("-webkit-linear-gradient(right, red, blue)").0, 270.0);
    // Без направления — `to bottom`.
    assert_eq!(linear("-webkit-linear-gradient(red, blue)").0, 180.0);
}

#[test]
fn linear_corner_start_maps_to_opposite_corner() {
    let (_, c, _, _) = linear("-webkit-linear-gradient(top left, red, blue)");
    assert_eq!(c, Some(GradientCorner::DiagBottomRight));
    let (_, c, _, _) = linear("-webkit-linear-gradient(right top, red, blue)");
    assert_eq!(c, Some(GradientCorner::DiagBottomLeft));
    let (_, c, _, _) = linear("-webkit-linear-gradient(bottom right, red, blue)");
    assert_eq!(c, Some(GradientCorner::DiagTopLeft));
    let (_, c, _, _) = linear("-webkit-linear-gradient(left bottom, red, blue)");
    assert_eq!(c, Some(GradientCorner::DiagTopRight));
    // Линия идёт вдоль диагонали 180×80: вектор (180, 80) → ≈ 114° по CSS.
    let ang = GradientCorner::DiagBottomRight.angle_deg(180.0, 80.0);
    assert!((ang - 113.96).abs() < 0.05, "{ang}");
    let sq = GradientCorner::DiagTopLeft.angle_deg(100.0, 100.0);
    assert!((sq - 315.0).abs() < 0.01, "{sq}");
}

#[test]
fn linear_angle_zero_points_east_counterclockwise() {
    assert_eq!(linear("-webkit-linear-gradient(0deg, red, blue)").0, 90.0);
    assert_eq!(linear("-webkit-linear-gradient(90deg, red, blue)").0, 0.0);
    assert_eq!(linear("-webkit-linear-gradient(0, red, blue)").0, 90.0);
    assert_eq!(linear("-webkit-linear-gradient(-45deg, red, blue)").0, 135.0);
}

#[test]
fn repeating_linear_keeps_flag_and_stops() {
    let (a, _, stops, rep) = linear("-webkit-repeating-linear-gradient(left, red 0, blue 20px)");
    assert_eq!(a, 90.0);
    assert!(rep);
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[1].position, Some(Length::Px(20.0)));
}

#[test]
fn linear_rejects_standard_to_syntax() {
    assert!(parse_webkit_gradient("-webkit-linear-gradient(to bottom, red, blue)").is_none());
    assert!(parse_webkit_gradient("-webkit-linear-gradient(red)").is_none());
    assert!(parse_webkit_gradient("-webkit-linear-gradient(top left bottom, red, blue)").is_none());
}

#[test]
fn radial_position_shape_size() {
    let g = parse_webkit_gradient("-webkit-radial-gradient(10% 20%, circle closest-side, red, blue)").unwrap();
    let ParsedGradient::Radial { center_x_pct, center_y_pct, shape, size, stops, repeating } = g else {
        panic!("expected radial")
    };
    assert!((center_x_pct - 0.1).abs() < 1e-6 && (center_y_pct - 0.2).abs() < 1e-6);
    assert_eq!((shape, size, stops.len(), repeating), (RadialShape::Circle, RadialSize::ClosestSide, 2, false));

    let g = parse_webkit_gradient("-webkit-radial-gradient(top left, cover, red, blue)").unwrap();
    let ParsedGradient::Radial { center_x_pct, center_y_pct, size, .. } = g else { panic!() };
    assert_eq!((center_x_pct, center_y_pct, size), (0.0, 0.0, RadialSize::FarthestCorner));

    let g = parse_webkit_gradient("-webkit-repeating-radial-gradient(contain, red, blue 10px)").unwrap();
    let ParsedGradient::Radial { size, repeating, .. } = g else { panic!() };
    assert_eq!((size, repeating), (RadialSize::ClosestSide, true));
}

#[test]
fn radial_grammar_matches_wpt_compat() {
    // webkit-radial-gradient-radii / -comma.
    let valid = |v: &str| webkit_gradient_value_valid(v);
    assert!(valid("-webkit-radial-gradient(1px 2px, 3% 4%, red, blue)"));
    assert!(valid("-webkit-radial-gradient(1px 2px, 0% 4%, red, blue)"));
    assert!(!valid("-webkit-radial-gradient(1px 2px, -3% 4%, red, blue)"));
    assert!(!valid("-webkit-radial-gradient(1px 2px, 3% -4%, red, blue)"));
    assert!(!valid("-webkit-radial-gradient(1px 2px, -3% -4%, red, blue)"));
    assert!(!valid("-webkit-radial-gradient(1px 2px, 3% 4% red, blue)"));
    // `at` — только стандартный синтаксис.
    assert!(!valid("-webkit-radial-gradient(circle at center, red, blue)"));
    // Не легаси-значения не трогаем; `-webkit-gradient()` с calc() принимается.
    assert!(valid("url(a.png), linear-gradient(red, blue)"));
    assert!(valid("-webkit-gradient(linear, left top, left bottom, color-stop(calc(0.5 + 0.001 * sign(1em - 1px)), blue))"));
}

#[test]
fn old_gradient_axis_aligned_uses_edge_coordinates() {
    let (a, c, stops, _) =
        linear("-webkit-gradient(linear, left top, left bottom, from(red), color-stop(0.5, lime), to(blue))");
    assert_eq!((a, c, stops.len()), (180.0, None, 3));
    assert_eq!(stops[1].position, Some(Length::Percent(50.0)));
    assert_eq!(stops[2].position, Some(Length::Percent(100.0)));

    // Вверх: порядок ступеней обращается, позиции — от верхнего края.
    let (a, _, stops, _) = linear("-webkit-gradient(linear, 0 100%, 0 0, from(red), to(blue))");
    assert_eq!(a, 180.0);
    assert_eq!(stops[0].color, parse_color("blue").unwrap());
    assert_eq!(stops[0].position, Some(Length::Percent(0.0)));

    // Пиксельные координаты.
    let (a, _, stops, _) = linear("-webkit-gradient(linear, 10 0, 50 0, from(red), to(blue))");
    assert_eq!(a, 90.0);
    assert_eq!(stops[0].position, Some(Length::Px(10.0)));
    assert_eq!(stops[1].position, Some(Length::Px(50.0)));
}

#[test]
fn old_gradient_diagonal_corners_use_corner_form() {
    let (_, c, stops, _) = linear("-webkit-gradient(linear, left top, right bottom, from(red), to(blue))");
    assert_eq!(c, Some(GradientCorner::DiagBottomRight));
    assert_eq!(stops[1].position, Some(Length::Percent(100.0)));
}

#[test]
fn old_gradient_radial_stops_are_px_radii() {
    let g = parse_webkit_gradient(
        "-webkit-gradient(radial, 50% 50%, 10, 50% 50%, 60, from(red), to(blue))",
    )
    .unwrap();
    let ParsedGradient::Radial { center_x_pct, shape, stops, .. } = g else { panic!() };
    assert_eq!((center_x_pct, shape), (0.5, RadialShape::Circle));
    assert_eq!(stops[0].position, Some(Length::Px(10.0)));
    assert_eq!(stops[1].position, Some(Length::Px(60.0)));
}

#[test]
fn old_gradient_rejects_garbage() {
    assert!(parse_webkit_gradient("-webkit-gradient(conic, left top, left bottom, from(red))").is_none());
    assert!(parse_webkit_gradient("-webkit-gradient(linear, left top, from(red))").is_none());
    assert!(parse_webkit_gradient("-webkit-gradient(linear, left top, left bottom, stop(red))").is_none());
    assert!(parse_webkit_gradient("-webkit-gradient(radial, 0 0, -1, 0 0, 5, from(red))").is_none());
}

#[test]
fn background_image_cascade_accepts_prefixed_gradient() {
    let s = style_of_div("background-image: -webkit-linear-gradient(left, green 50%, red 50%)");
    let BackgroundImage::Gradient(ParsedGradient::Linear { angle_deg, .. }) = &s.background_layers[0].image else {
        panic!("expected linear gradient layer, got {:?}", s.background_layers);
    };
    assert_eq!(*angle_deg, 90.0);
    // Шорткат `background` тоже распознаёт легаси-функцию как <image>.
    let s = style_of_div("background: -webkit-gradient(linear, left top, left bottom, from(red), to(blue)) no-repeat");
    assert!(matches!(
        s.background_layers[0].image,
        BackgroundImage::Gradient(ParsedGradient::Linear { .. })
    ));
}

fn style_of_div(css: &str) -> ComputedStyle {
    let doc = lumen_html_parser::parse(r#"<div></div>"#);
    let sheet = lumen_css_parser::parse(&format!("div {{ {css} }}"));
    let root = ComputedStyle::root();
    let body = doc.body().expect("body");
    let child = doc.get(body).children.first().copied().expect("div");
    compute_style(&doc, child, &sheet, &root, Size { width: 800.0, height: 600.0 }, false)
}

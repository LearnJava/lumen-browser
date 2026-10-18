use super::super::shape_resolve::{resolve_shape, FloatBoxes, Prim, Rect4};
use super::super::FloatContext;

// ── LAYOUT-VFLOAT-3: shape-outside вертикальных float-ов ──

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect4 {
    Rect4 { x0, y0, x1, y1 }
}

/// Float 100×60 без рамок и полей, радиусы углов `radii`.
fn boxes(radii: [(f32, f32); 4]) -> FloatBoxes {
    let r = rect(0.0, 0.0, 100.0, 60.0);
    FloatBoxes {
        margin: r,
        border: r,
        padding: r,
        content: r,
        radii,
        margins: [0.0; 4],
        borders: [0.0; 4],
        paddings: [0.0; 4],
        em: 16.0,
        threshold: 0.0,
    }
}

#[test]
fn rotate_swaps_axes_of_an_ellipse() {
    let p = Prim::Ellipse { cx: 10.0, cy: 20.0, rx: 5.0, ry: 3.0 };
    let Prim::Ellipse { cx, cy, rx, ry } = p.rotate(100.0, false) else { panic!() };
    assert_eq!((cx, cy, rx, ry), (20.0, 10.0, 3.0, 5.0));
    let Prim::Ellipse { cx, cy, rx, ry } = p.rotate(100.0, true) else { panic!() };
    assert_eq!((cx, cy, rx, ry), (20.0, 90.0, 3.0, 5.0), "vertical-rl: block-ось от правого края");
}

#[test]
fn rotate_moves_rrect_corner_radii_with_their_corners() {
    let p = Prim::RRect {
        x0: 0.0,
        y0: 0.0,
        x1: 40.0,
        y1: 20.0,
        radii: [(8.0, 4.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)],
    };
    // vertical-lr: физ. левый-верхний угол остаётся (inline-старт, block-старт), радиусы меняются осями.
    let Prim::RRect { x1, y1, radii, .. } = p.rotate(40.0, false) else { panic!() };
    assert_eq!((x1, y1), (20.0, 40.0));
    assert_eq!(radii[0], (4.0, 8.0));
    // vertical-rl: физ. левый-верхний угол уходит к block-концу, то есть в слот x0·y1 (индекс 3).
    let Prim::RRect { radii, .. } = p.rotate(40.0, true) else { panic!() };
    assert_eq!(radii[3], (4.0, 8.0));
    assert_eq!(radii[0], (0.0, 0.0));
}

#[test]
fn rrect_extent_follows_the_corner_and_is_half_open() {
    let p = Prim::RRect { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0, radii: [(20.0, 20.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)] };
    assert_eq!(p.extent_at(0.0, 0.0), Some((20.0, 100.0)));
    assert_eq!(p.extent_at(20.0, 0.0), Some((0.0, 100.0)));
    assert_eq!(p.extent_at(100.0, 0.0), None, "нижняя кромка не входит");
    assert_eq!(p.extent_at(-1.0, 0.0), None);
}

#[test]
fn shape_margin_grows_the_extent() {
    let p = Prim::Ellipse { cx: 50.0, cy: 50.0, rx: 50.0, ry: 50.0 };
    let (l, r) = p.extent_at(50.0, 10.0).unwrap();
    assert!((l + 10.0).abs() < 0.01 && (r - 110.0).abs() < 0.01, "{l} {r}");
}

#[test]
fn circle_with_position_keywords() {
    let prims = resolve_shape("circle(30px at left 40px top 20px)", &boxes([(0.0, 0.0); 4])).unwrap();
    let [Prim::Ellipse { cx, cy, rx, ry }] = prims.as_slice() else { panic!("{prims:?}") };
    assert_eq!((*cx, *cy, *rx, *ry), (40.0, 20.0, 30.0, 30.0));
}

#[test]
fn circle_farthest_side_reaches_the_far_edge() {
    let prims = resolve_shape("circle(farthest-side at 10px 10px)", &boxes([(0.0, 0.0); 4])).unwrap();
    let [Prim::Ellipse { rx, .. }] = prims.as_slice() else { panic!("{prims:?}") };
    assert_eq!(*rx, 90.0, "дальняя сторона по x — 100 - 10");
}

#[test]
fn inset_with_elliptical_round() {
    let prims = resolve_shape("inset(10px round 20px / 10px)", &boxes([(0.0, 0.0); 4])).unwrap();
    let [Prim::RRect { x0, y0, x1, y1, radii }] = prims.as_slice() else { panic!("{prims:?}") };
    assert_eq!((*x0, *y0, *x1, *y1), (10.0, 10.0, 90.0, 50.0));
    assert!(radii.iter().all(|r| *r == (20.0, 10.0)), "{radii:?}");
}

#[test]
fn bare_shape_box_keeps_the_rounded_corners() {
    let prims = resolve_shape("border-box", &boxes([(10.0, 10.0); 4])).unwrap();
    let [Prim::RRect { x1, radii, .. }] = prims.as_slice() else { panic!("{prims:?}") };
    assert_eq!(*x1, 100.0);
    assert_eq!(radii[2], (10.0, 10.0));
}

#[test]
fn unknown_value_is_not_a_shape() {
    assert!(resolve_shape("none", &boxes([(0.0, 0.0); 4])).is_none());
    assert!(resolve_shape("circle(10px) circle(20px)", &boxes([(0.0, 0.0); 4])).is_none());
    assert!(resolve_shape("bogus(1px)", &boxes([(0.0, 0.0); 4])).is_none());
}

#[test]
fn linear_gradient_keeps_only_opaque_bands() {
    // Левая половина непрозрачная, правая прозрачная: контур — полоса 0..50.
    let prims = resolve_shape("linear-gradient(to right, black 50%, transparent 50%)", &boxes([(0.0, 0.0); 4])).unwrap();
    let ext: Vec<_> = prims.iter().filter_map(|p| p.extent_at(30.0, 0.0)).collect();
    assert!(!ext.is_empty());
    let right = ext.iter().map(|e| e.1).fold(f32::MIN, f32::max);
    assert!((right - 50.0).abs() < 1.0, "{ext:?}");
}

/// Левый float 100×100 с круговым контуром в логическом кадре.
fn circle_ctx() -> FloatContext {
    let mut fc = FloatContext::new();
    fc.add_left(0.0, 100.0, 100.0);
    fc.mark_last_shaped(true);
    fc.add_shape_prims(
        vec![Prim::Ellipse { cx: 50.0, cy: 50.0, rx: 50.0, ry: 50.0 }],
        true,
        (0.0, 100.0),
        (0.0, 100.0),
        0.0,
    );
    fc
}

#[test]
fn contour_narrows_the_band_by_the_shape_not_the_rectangle() {
    let fc = circle_ctx();
    let (l, _) = fc.line_band(50.0, 51.0, 0.0, 300.0);
    assert!(l > 99.0 && l <= 100.0, "в середине контур упирается в край поля: {l}");
    let (l, _) = fc.line_band(0.0, 1.0, 0.0, 300.0);
    assert!(l < 60.0, "у касательной кромки строка почти свободна: {l}");
}

#[test]
fn contours_survive_a_change_of_block_frame() {
    let fc = circle_ctx();
    let rebased = fc.rebased_block(10.0);
    for y in [0.0_f32, 15.0, 40.0, 70.0] {
        assert_eq!(fc.line_band(y + 10.0, y + 11.0, 0.0, 300.0), rebased.line_band(y, y + 1.0, 0.0, 300.0), "y = {y}");
    }
}

#[test]
fn contours_survive_a_nested_context_and_absorption() {
    let child = circle_ctx();
    let mut parent = FloatContext::new();
    parent.absorb_vertical(&child, 10.0, 5.0);
    let (cl, _) = child.line_band(50.0, 51.0, 0.0, 300.0);
    let (pl, _) = parent.line_band(60.0, 61.0, 0.0, 300.0);
    assert!((pl - (cl + 5.0)).abs() < 0.01, "поглощённый float сдвинут на (di, db): {pl} vs {cl}");

    let mut outer = FloatContext::new();
    outer.absorb_vertical(&child, 10.0, 5.0);
    let inner = outer.inheriting_vertical(10.0, 5.0);
    let (il, _) = inner.line_band(50.0, 51.0, 0.0, 300.0);
    assert!((il - cl).abs() < 0.01, "унаследованный контур виден в кадре потомка: {il} vs {cl}");
}

// ── раскладка: ряд inline-block обтекает контур float-а ──

struct Em;
impl crate::TextMeasurer for Em {
    fn char_width(&self, _: char, size: f32) -> f32 {
        size
    }
    fn line_gap_px(&self, _: f32) -> f32 {
        0.0
    }
}

fn row_rects(css: &str, ids: &[&str]) -> Vec<lumen_core::geom::Rect> {
    let html = r#"<div id="w"><div id="f"></div><div id="a" class="ib"></div><div id="b" class="ib"></div></div>"#;
    let css = format!(
        "body{{margin:0}}#w{{font:10px/10px X;writing-mode:vertical-lr;width:100px;height:100px}}\
         #f{{float:left;width:50px;height:50px}}.ib{{display:inline-block;width:10px;height:30px}}{css}"
    );
    let doc = lumen_html_parser::parse(html);
    let sheet = lumen_css_parser::parse(&css);
    let root = super::super::layout_measured(&doc, &sheet, lumen_core::geom::Size::new(800.0, 600.0), &Em);
    ids.iter()
        .map(|id| super::find_by_id_all(&root, &doc, id).unwrap_or_else(|| panic!("нет #{id}")).rect)
        .collect()
}

#[test]
fn inline_block_row_wraps_a_circular_contour() {
    // Прямоугольный float 50×50: первая колонка (block 0..10) стартует на inline 50. С кругом
    // r=25 край на полосе 0..10 — в точке block 10, 25 + 25·sqrt(1 - 0.6²) = 45.
    let plain = row_rects("", &["a"]);
    let shaped = row_rects("#f{shape-outside:circle(25px at 25px 25px)}", &["a"]);
    assert_eq!(plain[0].y, 50.0, "{:?}", plain[0]);
    assert!((shaped[0].y - 45.0).abs() < 0.1, "{:?}", shaped[0]);
    assert_eq!(shaped[0].x, 0.0, "{:?}", shaped[0]);
}

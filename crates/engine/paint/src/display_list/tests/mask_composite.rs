//! BUG-1406 — `mask-size` / `mask-position` / `mask-repeat` / `mask-composite` и
//! `url()`-маска: display list и CPU-растр.

use super::text_and_images::build;
use super::*;

fn names(dl: &DisplayList) -> Vec<&'static str> {
    dl.iter()
        .filter_map(|c| match c {
            DisplayCommand::PushMaskLayer { .. } => Some("PushMaskLayer"),
            DisplayCommand::PopMaskLayer => Some("PopMaskLayer"),
            DisplayCommand::PushMaskLinearGradient { .. } => Some("PushLinear"),
            DisplayCommand::PopMask => Some("PopMask"),
            _ => None,
        })
        .collect()
}

#[test]
fn gradient_with_mask_size_uses_composite_path() {
    let dl = build(
        "<div></div>",
        "div { width: 100px; height: 60px; background: green; \
         mask-image: linear-gradient(black, black); mask-size: 50px 50px; mask-repeat: no-repeat; }",
    );
    assert_eq!(names(&dl), vec!["PushMaskLayer", "PopMaskLayer"]);
}

#[test]
fn plain_gradient_keeps_push_mask_path() {
    let dl = build(
        "<div></div>",
        "div { width: 100px; height: 60px; background: green; mask-image: linear-gradient(black, black); }",
    );
    assert_eq!(names(&dl), vec!["PushLinear", "PopMask"]);
}

#[test]
fn add_and_bottom_intersect_use_composite_path() {
    for css in [
        "mask-image: url(a.png), linear-gradient(black, transparent);",
        "mask-image: url(a.png), linear-gradient(black, transparent); mask-composite: intersect, intersect;",
        "mask-image: url(a.png), none; mask-composite: intersect, add;",
    ] {
        let dl = build("<div></div>", &format!("div {{ width: 100px; height: 60px; {css} }}"));
        assert_eq!(names(&dl).first(), Some(&"PushMaskLayer"), "{css}");
        assert!(!names(&dl).contains(&"PopMask"), "{css}");
    }
}

#[test]
fn mask_position_default_is_top_left() {
    let layer = lumen_layout::MaskLayer::default();
    assert_eq!(layer.position, lumen_layout::ObjectPosition::background_initial());
}

#[cfg(feature = "cpu-render")]
mod raster {
    use super::*;

    fn render(css: &str, images: &[(String, std::sync::Arc<lumen_image::Image>)]) -> lumen_image::Image {
        let dl = build(
            "<div></div>",
            &format!("div {{ width: 200px; height: 200px; background: rgb(0,255,0); {css} }}"),
        );
        crate::cpu_raster::rasterize_cpu(220, 220, &dl, images, 0.0, 0.0).expect("rasterize")
    }

    fn green(img: &lumen_image::Image) -> usize {
        img.data
            .chunks_exact(4)
            .filter(|p| p[0] < 40 && p[1] > 215 && p[2] < 40)
            .count()
    }

    fn is_green(img: &lumen_image::Image, x: u32, y: u32) -> bool {
        let i = ((y * img.width + x) * 4) as usize;
        img.data[i] < 40 && img.data[i + 1] > 215
    }

    const BLACK: &str = "linear-gradient(black, black)";

    #[test]
    fn size_and_no_repeat_limit_the_mask_to_one_tile() {
        let img = render(
            &format!("mask-image: {BLACK}; mask-size: 50px 50px; mask-repeat: no-repeat;"),
            &[],
        );
        assert_eq!(green(&img), 2500);
        assert!(is_green(&img, 10, 10));
        assert!(!is_green(&img, 100, 100));
    }

    #[test]
    fn position_moves_the_tile() {
        let img = render(
            &format!(
                "mask-image: {BLACK}; mask-size: 50px 50px; mask-repeat: no-repeat; mask-position: right bottom;"
            ),
            &[],
        );
        assert_eq!(green(&img), 2500);
        assert!(is_green(&img, 190, 190));
        assert!(!is_green(&img, 10, 10));
    }

    #[test]
    fn repeat_tiles_the_whole_box() {
        let img = render(&format!("mask-image: {BLACK}; mask-size: 50px 50px;"), &[]);
        assert_eq!(green(&img), 40_000);
    }

    #[test]
    fn shorthand_sets_size_position_repeat() {
        let img = render(&format!("mask: {BLACK} 0 0 / 50px 50px no-repeat;"), &[]);
        assert_eq!(green(&img), 2500);
    }

    fn two_layers(op: &str) -> usize {
        green(&render(
            &format!(
                "mask-image: {BLACK}, {BLACK}; mask-size: 100px 100px, 50px 50px; \
                 mask-repeat: no-repeat; mask-composite: {op};"
            ),
            &[],
        ))
    }

    #[test]
    fn composite_operators_combine_layers() {
        assert_eq!(two_layers("add"), 10_000);
        assert_eq!(two_layers("intersect"), 2500);
        assert_eq!(two_layers("subtract"), 7500);
        assert_eq!(two_layers("exclude"), 7500);
    }

    /// 50×50 родитель с маской и абсолютно позиционированный потомок 100×100.
    fn overflowing_child(mask: &str) -> usize {
        let dl = build(
            "<div class=p><div class=c></div></div>",
            &format!(
                ".p {{ width: 50px; height: 50px; position: absolute; left: 0; top: 0; {mask} }}                  .c {{ position: absolute; left: 0; top: 0; width: 100px; height: 100px; background: rgb(0,255,0); }}"
            ),
        );
        green(&crate::cpu_raster::rasterize_cpu(220, 220, &dl, &[], 0.0, 0.0).expect("rasterize"))
    }

    #[test]
    fn mask_covers_positioned_descendants() {
        // CSS Masking §4.1: `mask-image` образует stacking context — потомок,
        // выступающий из рамки, лежит внутри группы маски и обрезается по ней.
        assert_eq!(overflowing_child(&format!("mask-image: {BLACK};")), 2500);
    }

    #[test]
    fn no_clip_mask_tiles_continue_outside_the_box() {
        // `mask-clip: no-clip`: плитки `repeat` идут за границы элемента, поэтому
        // выступающий потомок остаётся виден целиком.
        assert_eq!(overflowing_child(&format!("mask-image: {BLACK}; mask-clip: no-clip;")), 10_000);
        // С `no-repeat` плитка одна — за рамкой маска прозрачна.
        assert_eq!(
            overflowing_child(&format!("mask-image: {BLACK}; mask-clip: no-clip; mask-repeat: no-repeat;")),
            2500
        );
    }

    #[test]
    fn url_mask_uses_the_decoded_image_alpha() {
        // 100×100: верхняя половина прозрачна, нижняя непрозрачна.
        let mut data = vec![0u8; 100 * 100 * 4];
        for px in data.chunks_exact_mut(4).skip(100 * 50) {
            px.copy_from_slice(&[0, 0, 255, 255]);
        }
        let image = lumen_image::Image {
            width: 100,
            height: 100,
            format: lumen_image::PixelFormat::Rgba8,
            data,
            icc_profile: None,
            vector_source: None,
        };
        let dl = build(
            "<div></div>",
            "div { width: 100px; height: 100px; background: rgb(0,255,0); mask-image: url(m.png); }",
        );
        let img = crate::cpu_raster::rasterize_cpu(
            120,
            120,
            &dl,
            &[("m.png".to_string(), std::sync::Arc::new(image))],
            0.0,
            0.0,
        )
        .expect("rasterize");
        assert_eq!(green(&img), 5000);
    }
}

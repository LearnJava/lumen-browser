//! SVG-FILTER: `filter: url(#id)` → граф `<filter>` в `PushFilter` и итоговый растр.

use super::*;
use super::text_and_images::build;
use lumen_layout::{FePrimitive, FilterFn};

fn url_filter(list: &DisplayList) -> Option<lumen_layout::FilterUrl> {
    list.iter().find_map(|c| match c {
        DisplayCommand::PushFilter { filters, .. } => filters.iter().find_map(|f| match f {
            FilterFn::Url(u) => Some(u.clone()),
            _ => None,
        }),
        _ => None,
    })
}

#[test]
fn svg_rect_attribute_resolves_filter_graph() {
    let list = build(
        r##"<svg width=100 height=100><filter id=f><feFlood flood-color="#00f" flood-opacity=".5"/>
        <feOffset dx=3 dy=4 result=o /><feGaussianBlur in=o stdDeviation="1 2"/></filter>
        <rect width=50 height=50 fill=red filter="url(#f)" /></svg>"##,
        "",
    );
    let u = url_filter(&list).expect("PushFilter с url()");
    let g = u.graph.expect("граф разрешён");
    assert!(g.filter_units_obb && !g.primitive_units_obb);
    assert_eq!(g.region, (-0.1, -0.1, 1.2, 1.2));
    assert_eq!(g.primitives.len(), 3);
    assert!(matches!(g.primitives[1].kind, FePrimitive::Offset { dx, dy, .. } if dx == 3.0 && dy == 4.0));
    assert!(matches!(g.primitives[2].kind, FePrimitive::GaussianBlur { sx, sy, .. } if sx == 1.0 && sy == 2.0));
    assert!(g.primitives[0].linear_rgb, "color-interpolation-filters по умолчанию linearRGB");
}

#[test]
fn css_filter_property_on_html_box_and_missing_reference() {
    let list = build(
        r##"<svg width=0 height=0><filter id=f><feFlood/></filter></svg>
        <div style="width:50px;height:50px;filter:url(#f) blur(2px)"></div>
        <div style="width:50px;height:50px;filter:url(#nope)"></div>"##,
        "",
    );
    let mut urls = list.iter().filter_map(|c| match c {
        DisplayCommand::PushFilter { filters, .. } => Some(filters.clone()),
        _ => None,
    });
    let first = urls.next().expect("первый фильтр");
    assert!(matches!(&first[0], FilterFn::Url(u) if u.graph.is_some()));
    assert!(matches!(first[1], FilterFn::Blur(_)));
    let second = urls.next().expect("второй фильтр");
    assert!(matches!(&second[0], FilterFn::Url(u) if u.graph.is_none()));
}

#[test]
fn new_primitives_parse_with_defaults_and_taint() {
    let list = build(
        r##"<svg width=100 height=100><filter id=f>
        <feFlood flood-color="currentcolor"/>
        <feConvolveMatrix kernelMatrix="1 1 1 1 1 1 1 1 1" divisor="0"/>
        <feConvolveMatrix kernelMatrix="1 2"/>
        <feMorphology operator="dilate" radius="2 3"/>
        <feDisplacementMap in2="SourceGraphic" scale="10" xChannelSelector="G"/>
        <feTurbulence baseFrequency="0.05" numOctaves="3" seed="2.4" type="fractalNoise"/>
        <feDiffuseLighting lighting-color="red"><fePointLight x=1 y=2 z=3/></feDiffuseLighting>
        <feSpecularLighting specularExponent="200"><feDistantLight/></feSpecularLighting>
        </filter><rect width=50 height=50 filter="url(#f)" /></svg>"##,
        "",
    );
    let g = url_filter(&list).and_then(|u| u.graph).expect("граф разрешён");
    let k = |i: usize| &g.primitives[i].kind;
    assert!(g.primitives[0].tainted && !g.primitives[1].tainted);
    // Нулевой divisor заменяется суммой ядра; ядро неверной длины — прозрачность.
    assert!(matches!(k(1), FePrimitive::ConvolveMatrix { divisor, target: (1, 1), .. } if *divisor == 9.0));
    assert!(matches!(k(2), FePrimitive::ConvolveMatrix { kernel, .. } if kernel.is_empty()));
    assert!(matches!(k(3), FePrimitive::Morphology { dilate: true, rx, ry, .. } if *rx == 2.0 && *ry == 3.0));
    assert!(matches!(k(4), FePrimitive::DisplacementMap { x_channel: 1, y_channel: 3, scale, .. } if *scale == 10.0));
    assert!(matches!(k(5), FePrimitive::Turbulence { octaves: 3, seed: 2, fractal: true, .. }));
    assert!(matches!(k(6), FePrimitive::Lighting { light: Some(lumen_layout::FeLight::Point { .. }), .. }));
    assert!(matches!(k(7), FePrimitive::Lighting { light: None, .. }), "specularExponent вне 1..128");
}

#[cfg(feature = "cpu-render")]
mod raster {
    use super::*;

    fn render(html: &str) -> lumen_image::Image {
        let list = build(html, "");
        crate::cpu_raster::rasterize_cpu(200, 120, &list, &[], 0.0, 0.0).expect("rasterize")
    }

    fn px(img: &lumen_image::Image, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * img.width + x) * 4) as usize;
        [img.data[i], img.data[i + 1], img.data[i + 2]]
    }

    #[test]
    fn flood_covers_filter_region_and_source_is_replaced() {
        let img = render(
            r##"<svg width=200 height=120><filter id=f x=0 y=0 width=1 height=1><feFlood flood-color="#00f"/></filter>
            <rect x=20 y=20 width=50 height=50 fill=red filter="url(#f)" /></svg>"##,
        );
        assert_eq!(px(&img, 40, 40), [0, 0, 255]);
        assert_eq!(px(&img, 10, 10), [255, 255, 255]);
    }

    #[test]
    fn filter_ink_is_clipped_by_enclosing_overflow() {
        let img = render(
            r##"<div style="overflow:hidden;width:60px;height:60px"><svg width=200 height=120 style="display:block">
            <filter id=f x=0 y=0 width=1 height=1><feFlood flood-color="#00f"/></filter>
            <rect x=0 y=0 width=150 height=100 fill=red filter="url(#f)" /></svg></div>"##,
        );
        assert_eq!(px(&img, 30, 30), [0, 0, 255]);
        assert_eq!(px(&img, 100, 30), [255, 255, 255]);
    }

    #[test]
    fn composite_in_keeps_flood_only_inside_source() {
        let img = render(
            r##"<svg width=200 height=120><filter id=f><feFlood flood-color="#0f0" result=a />
            <feComposite in=a in2=SourceGraphic operator=in /></filter>
            <circle cx=60 cy=60 r=30 fill=red filter="url(#f)" /></svg>"##,
        );
        assert_eq!(px(&img, 60, 60), [0, 255, 0]);
        // Угол области фильтра вне круга — фон.
        assert_eq!(px(&img, 34, 34), [255, 255, 255]);
    }

    #[test]
    fn dangling_reference_leaves_element_unfiltered() {
        let img = render(
            r##"<svg width=200 height=120><rect x=20 y=20 width=50 height=50 fill=red filter="url(#nope)" /></svg>"##,
        );
        assert_eq!(px(&img, 40, 40), [255, 0, 0]);
    }

    #[test]
    fn mask_clips_the_filtered_ink_outside_the_box() {
        let img = render(
            r##"<svg width=200 height=120><mask id=m><rect width=100 height=100 fill=white /></mask>
            <rect x=20 y=10 width=80 height=60 fill=red filter="blur(10px)" mask="url(#m)" /></svg>"##,
        );
        // Размытие вылезает за правый край прямоугольника (x = 100), а маска покрывает 0..100:
        // правее x = 100 (за маской) фон остаётся белым, внутри маски блюр виден.
        assert_eq!(px(&img, 108, 40), [255, 255, 255]);
        assert!(px(&img, 90, 40)[1] < 250, "размытие внутри маски видно");
    }
}

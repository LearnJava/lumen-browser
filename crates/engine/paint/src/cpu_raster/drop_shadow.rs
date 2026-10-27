//! `filter: drop-shadow()` (Filter Effects L1 §«drop-shadow()») на окне слоя.
//!
//! Тень — альфа-маска источника, закрашенная цветом тени, размытая
//! `gaussian_blur` и сдвинутая на `(dx, dy)`; источник рисуется поверх. Окно
//! (`cur` в [`super::svg_filter_chain_window`]) уже раздуто на
//! [`super::filter_chain_reach`], поэтому сдвинутая тень не обрезается.

use lumen_layout::Color;

/// Применяет тень к окну `src` (premultiplied RGBA8). Размеры — в px окна.
pub(super) fn apply(src: &tiny_skia::Pixmap, dx: f32, dy: f32, sigma: f32, color: Color) -> tiny_skia::Pixmap {
    let mut shadow = src.clone();
    let ca = u32::from(color.a);
    for px in shadow.data_mut().chunks_exact_mut(4) {
        let sa = (u32::from(px[3]) * ca + 127) / 255;
        px[0] = ((u32::from(color.r) * sa + 127) / 255) as u8;
        px[1] = ((u32::from(color.g) * sa + 127) / 255) as u8;
        px[2] = ((u32::from(color.b) * sa + 127) / 255) as u8;
        px[3] = sa as u8;
    }
    let shadow = if sigma > 0.0 { super::gaussian_blur(&shadow, sigma) } else { shadow };
    let Some(mut out) = tiny_skia::Pixmap::new(src.width(), src.height()) else {
        return src.clone();
    };
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    let id = tiny_skia::Transform::identity();
    out.draw_pixmap(dx.round() as i32, dy.round() as i32, shadow.as_ref(), &paint, id, None);
    out.draw_pixmap(0, 0, src.as_ref(), &paint, id, None);
    out
}

/// Охват тени в px окна: смещение и три радиуса ядра блюра (та же формула, что у
/// [`super::filter_chain_reach`] для `blur()`), с каждой стороны.
pub(super) fn reach(dx: f32, dy: f32, sigma: f32) -> f32 {
    let radius = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round().max(0.0);
    dx.abs().max(dy.abs()) + 3.0 * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(pm: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * pm.width() + x) * 4) as usize;
        let d = pm.data();
        [d[i], d[i + 1], d[i + 2], d[i + 3]]
    }

    #[test]
    fn shadow_lands_offset_under_source() {
        let mut src = tiny_skia::Pixmap::new(40, 20).unwrap();
        for y in 2..8 {
            for x in 2..8 {
                let i = ((y * 40 + x) * 4) as usize;
                src.data_mut()[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        let green = Color { r: 0, g: 128, b: 0, a: 255 };
        let out = apply(&src, 20.0, 0.0, 0.0, green);
        assert_eq!(px(&out, 4, 4), [0, 0, 255, 255], "источник сверху");
        assert_eq!(px(&out, 24, 4), [0, 128, 0, 255], "тень справа");
        assert_eq!(px(&out, 14, 4), [0, 0, 0, 0], "между ними пусто");
    }

    #[test]
    fn reach_covers_offset_and_blur() {
        assert_eq!(reach(0.0, 0.0, 0.0), 0.0);
        assert_eq!(reach(-30.0, 10.0, 0.0), 30.0);
        assert!(reach(0.0, 0.0, 4.0) >= 9.0);
    }
}

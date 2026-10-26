//! `feImage` со ссылкой на картинку (Filter Effects L1 §«feImage»): изображение
//! вписывается в подобласть примитива по `preserveAspectRatio`.

use lumen_image::Image;
use lumen_layout::FeImageFit;

use super::{Img, PxRect};

/// Рисует `image` в рабочую область `w × h` внутри подобласти `sub`. Вне
/// подобласти — прозрачный чёрный. SVG-источник перерастеризуется сразу в
/// целевой размер, растровый масштабируется билинейно.
pub(super) fn render(image: &Image, fit: &FeImageFit, sub: PxRect, w: usize, h: usize) -> Img {
    let mut out = Img::new(w, h);
    let (dw, dh) = ((sub.x1 - sub.x0) as f32, (sub.y1 - sub.y0) as f32);
    let (iw, ih) = (image.width as f32, image.height as f32);
    if dw <= 0.0 || dh <= 0.0 || iw <= 0.0 || ih <= 0.0 || w == 0 || h == 0 {
        return out;
    }
    let (pw, ph) = if fit.stretch {
        (dw, dh)
    } else {
        let (kx, ky) = (dw / iw, dh / ih);
        let k = if fit.slice { kx.max(ky) } else { kx.min(ky) };
        (iw * k, ih * k)
    };
    let px = sub.x0 as f32 + (dw - pw) * fit.align_x;
    let py = sub.y0 as f32 + (dh - ph) * fit.align_y;

    let vector = image.vector_source.as_ref().and_then(|src| {
        let (tw, th) = (pw.round().max(1.0) as u32, ph.round().max(1.0) as u32);
        lumen_image::rasterize_svg(src, tw, th).ok()
    });
    let raster = vector.as_ref().unwrap_or(image);
    let Some(src) = crate::cpu_raster::image_to_pixmap(raster) else { return out };
    let Some(mut canvas) = tiny_skia::Pixmap::new(w as u32, h as u32) else { return out };

    // Видимая часть: размещение ∩ подобласть (без размазывания краёв `Pad`).
    let (x0, y0) = (px.max(sub.x0 as f32), py.max(sub.y0 as f32));
    let (x1, y1) = ((px + pw).min(sub.x1 as f32), (py + ph).min(sub.y1 as f32));
    let Some(rect) = tiny_skia::Rect::from_ltrb(x0, y0, x1, y1) else { return out };
    let sx = pw / raster.width as f32;
    let sy = ph / raster.height as f32;
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Pattern::new(
            src.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bilinear,
            1.0,
            tiny_skia::Transform::from_row(sx, 0.0, 0.0, sy, px, py),
        ),
        anti_alias: false,
        ..tiny_skia::Paint::default()
    };
    canvas.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);

    for (o, p) in out.d.iter_mut().zip(canvas.data().chunks_exact(4)) {
        *o = [
            f32::from(p[0]) / 255.0,
            f32::from(p[1]) / 255.0,
            f32::from(p[2]) / 255.0,
            f32::from(p[3]) / 255.0,
        ];
    }
    out
}

//! BUG-1290: проективный композит слоя `PushTransform` для CPU-растра.
//!
//! tiny-skia умеет только аффинные `Transform`, а `perspective`/`rotateX/Y`
//! дают гомографию (деление на `w`). Слой группы лежит в плоскости `z = 0`, так
//! что его отображение на полотно — матрица 3×3 из столбцов/строк `0, 1, 3`
//! полной `Mat4` ([`homography`]). Композит идёт обратным выборщиком: для
//! каждого пикселя полотна обратная гомография даёт точку слоя, из которой
//! билинейно берётся premultiplied-цвет.

use lumen_core::geom::Rect;
use lumen_layout::Mat4;

/// Гомография 3×3 по строкам: `[x', y', w']ᵀ = H · [x, y, 1]ᵀ`.
pub(crate) type Homography = [f64; 9];

/// Сужение `Mat4` (column-major) до отображения плоскости `z = 0`: z-строка и
/// z-столбец отбрасываются — слой плоский, его глубина после проекции не нужна.
pub(crate) fn homography(m: &Mat4) -> Homography {
    let m = &m.0;
    let f = |i: usize| f64::from(m[i]);
    [f(0), f(4), f(12), f(1), f(5), f(13), f(3), f(7), f(15)]
}

/// Произведение `a · b` (сначала `b`, затем `a`).
pub(crate) fn mul(a: &Homography, b: &Homography) -> Homography {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    out
}

/// Обратная матрица; `None` для вырожденной (или почти) гомографии.
pub(crate) fn invert(h: &Homography) -> Option<Homography> {
    let [a, b, c, d, e, f, g, i, j] = *h;
    let (co0, co1, co2) = (e * j - f * i, f * g - d * j, d * i - e * g);
    let det = a * co0 + b * co1 + c * co2;
    let scale = h.iter().fold(0.0f64, |m, v| m.max(v.abs())).powi(3);
    if !det.is_finite() || det.abs() <= 1e-12 * scale.max(1e-12) {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        co0 * inv,
        (c * i - b * j) * inv,
        (b * f - c * e) * inv,
        co1 * inv,
        (a * j - c * g) * inv,
        (c * d - a * f) * inv,
        co2 * inv,
        (b * g - a * i) * inv,
        (a * e - b * d) * inv,
    ])
}

/// Аффинный образ гомографии `(a, b, c, d, e, f)` (`x' = a·x + c·y + e`), если
/// она не зависит от `w` — тогда слой композитится обычным `Transform`.
pub(crate) fn as_affine(h: &Homography) -> Option<[f32; 6]> {
    let w = h[8];
    if !w.is_finite() || w.abs() < 1e-9 {
        return None;
    }
    let n: Vec<f64> = h.iter().map(|v| v / w).collect();
    if n[6].abs() > 1e-7 || n[7].abs() > 1e-7 {
        return None;
    }
    Some([n[0] as f32, n[3] as f32, n[1] as f32, n[4] as f32, n[2] as f32, n[5] as f32])
}

/// Отображает точку слоя; `None`, если она за плоскостью камеры (`w' <= 0`).
fn project(h: &Homography, x: f64, y: f64) -> Option<(f64, f64)> {
    let w = h[6] * x + h[7] * y + h[8];
    (w > 1e-9).then(|| ((h[0] * x + h[1] * y + h[2]) / w, (h[3] * x + h[4] * y + h[5]) / w))
}

/// Композитит `src` (слой в локальных координатах, габарит чернил `dirty`)
/// на `dst` через гомографию `h` (`source-over`, premultiplied RGBA8).
/// `clip` — прямоугольник в координатах `dst`, вне которого пиксели не пишутся.
/// Возвращает прямоугольник `(l, t, r, b)` записанной области.
pub(crate) fn composite(
    dst: &mut tiny_skia::Pixmap,
    src: &tiny_skia::Pixmap,
    h: &Homography,
    clip: Option<Rect>,
    dirty: (f32, f32, f32, f32),
) -> Option<(f32, f32, f32, f32)> {
    let inv = invert(h)?;
    let (dw, dh) = (dst.width() as i64, dst.height() as i64);
    let (sw, sh) = (src.width() as i64, src.height() as i64);

    // Габарит результата: образ четырёх углов чернил; угол за камерой делает
    // его неограниченным — тогда берём всё полотно.
    let (l, t, r, b) = (f64::from(dirty.0), f64::from(dirty.1), f64::from(dirty.2), f64::from(dirty.3));
    let corners = [project(h, l, t), project(h, r, t), project(h, l, b), project(h, r, b)];
    let (mut x0, mut y0, mut x1, mut y1) = (0i64, 0i64, dw, dh);
    if corners.iter().all(Option::is_some) {
        let pts: Vec<(f64, f64)> = corners.into_iter().flatten().collect();
        let min = |f: fn(&(f64, f64)) -> f64| pts.iter().map(f).fold(f64::INFINITY, f64::min);
        let max = |f: fn(&(f64, f64)) -> f64| pts.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
        x0 = x0.max((min(|p| p.0) - 1.0).floor() as i64);
        y0 = y0.max((min(|p| p.1) - 1.0).floor() as i64);
        x1 = x1.min((max(|p| p.0) + 1.0).ceil() as i64);
        y1 = y1.min((max(|p| p.1) + 1.0).ceil() as i64);
    }
    if let Some(c) = clip {
        x0 = x0.max(f64::from(c.x).floor() as i64);
        y0 = y0.max(f64::from(c.y).floor() as i64);
        x1 = x1.min(f64::from(c.x + c.width).ceil() as i64);
        y1 = y1.min(f64::from(c.y + c.height).ceil() as i64);
    }
    if x0 >= x1 || y0 >= y1 {
        return None;
    }

    let src_px = src.data();
    let texel = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= sw || y >= sh {
            return [0.0; 4];
        }
        let i = ((y * sw + x) * 4) as usize;
        [f32::from(src_px[i]), f32::from(src_px[i + 1]), f32::from(src_px[i + 2]), f32::from(src_px[i + 3])]
    };
    let dst_px = dst.data_mut();
    for py in y0..y1 {
        for px in x0..x1 {
            let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
            if let Some(c) = clip
                && (cx < f64::from(c.x)
                    || cy < f64::from(c.y)
                    || cx > f64::from(c.x + c.width)
                    || cy > f64::from(c.y + c.height))
            {
                continue;
            }
            let z = inv[6] * cx + inv[7] * cy + inv[8];
            // `1 / z` — это `w'` прямого отображения: точка за камерой не рисуется.
            if z <= 1e-12 {
                continue;
            }
            let u = (inv[0] * cx + inv[1] * cy + inv[2]) / z - 0.5;
            let v = (inv[3] * cx + inv[4] * cy + inv[5]) / z - 0.5;
            if u <= -1.0 || v <= -1.0 || u >= sw as f64 || v >= sh as f64 {
                continue;
            }
            let (ux, vy) = (u.floor(), v.floor());
            let (fx, fy) = ((u - ux) as f32, (v - vy) as f32);
            let (ix, iy) = (ux as i64, vy as i64);
            let (t00, t10, t01, t11) =
                (texel(ix, iy), texel(ix + 1, iy), texel(ix, iy + 1), texel(ix + 1, iy + 1));
            let mut s = [0.0f32; 4];
            for k in 0..4 {
                let top = t00[k] + (t10[k] - t00[k]) * fx;
                let bot = t01[k] + (t11[k] - t01[k]) * fx;
                s[k] = top + (bot - top) * fy;
            }
            if s[3] < 0.5 {
                continue;
            }
            let i = ((py * dw + px) * 4) as usize;
            let inv_a = 1.0 - s[3] / 255.0;
            for k in 0..4 {
                let d = f32::from(dst_px[i + k]);
                dst_px[i + k] = (s[k] + d * inv_a).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Some((x0 as f32, y0 as f32, x1 as f32, y1 as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Homography = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    fn solid(w: u32, h: u32, x: u32, y: u32, sw: u32, sh: u32) -> tiny_skia::Pixmap {
        let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
        for yy in y..y + sh {
            for xx in x..x + sw {
                let i = ((yy * w + xx) * 4) as usize;
                pm.data_mut()[i..i + 4].copy_from_slice(&[0, 255, 0, 255]);
            }
        }
        pm
    }

    fn alpha_at(pm: &tiny_skia::Pixmap, x: u32, y: u32) -> u8 {
        pm.data()[((y * pm.width() + x) * 4 + 3) as usize]
    }

    #[test]
    fn invert_round_trips() {
        let h = [1.5, 0.2, 10.0, -0.1, 0.8, 4.0, 0.001, 0.002, 1.0];
        let p = mul(&h, &invert(&h).unwrap());
        for (got, want) in p.iter().zip(IDENTITY) {
            assert!((got - want).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn perspective_at_plane_is_affine_identity() {
        // CSS `perspective(100px)` на плоскости z=0 — тождество.
        let m = Mat4::perspective(100.0);
        assert_eq!(as_affine(&homography(&m)), Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
    }

    #[test]
    fn translate_z_under_perspective_scales() {
        // perspective(100) · translateZ(50) → масштаб 2 вокруг начала координат.
        let m = Mat4::perspective(100.0).multiply(&Mat4::translate_3d(0.0, 0.0, 50.0));
        let [a, _, _, d, e, f] = as_affine(&homography(&m)).unwrap();
        assert!((a - 2.0).abs() < 1e-5 && (d - 2.0).abs() < 1e-5, "{a} {d}");
        assert!(e.abs() < 1e-4 && f.abs() < 1e-4);
    }

    #[test]
    fn rotate_y_under_perspective_is_not_affine() {
        let m = Mat4::perspective(100.0).multiply(&Mat4::rotate_y(1.0));
        assert!(as_affine(&homography(&m)).is_none());
    }

    #[test]
    fn composite_identity_copies_pixels() {
        let src = solid(20, 20, 5, 5, 10, 10);
        let mut dst = tiny_skia::Pixmap::new(20, 20).unwrap();
        composite(&mut dst, &src, &IDENTITY, None, (5.0, 5.0, 15.0, 15.0)).unwrap();
        assert_eq!(alpha_at(&dst, 10, 10), 255);
        assert_eq!(alpha_at(&dst, 2, 2), 0);
        assert_eq!(alpha_at(&dst, 17, 10), 0);
    }

    #[test]
    fn composite_rotate_y_narrows_the_far_edge() {
        // Квадрат 40×40 с центром (50, 50), повёрнутый вокруг оси Y в перспективе:
        // ближний край шире дальнего, площадь меньше исходной.
        let src = solid(100, 100, 30, 30, 40, 40);
        let around = |m: Mat4| {
            Mat4::translation_2d(50.0, 50.0).multiply(&m).multiply(&Mat4::translation_2d(-50.0, -50.0))
        };
        let m = around(Mat4::perspective(120.0).multiply(&Mat4::rotate_y(1.0)));
        let mut dst = tiny_skia::Pixmap::new(100, 100).unwrap();
        composite(&mut dst, &src, &homography(&m), None, (30.0, 30.0, 70.0, 70.0)).unwrap();
        let ink: u32 = (0..100)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .filter(|&(x, y)| alpha_at(&dst, x, y) == 255)
            .count() as u32;
        assert!(ink > 200 && ink < 1600, "ink = {ink}");
        let col_height = |x: u32| (0..100).filter(|&y| alpha_at(&dst, x, y) == 255).count();
        let heights: Vec<usize> = (0..100).map(col_height).filter(|&h| h > 0).collect();
        let (first, last) = (heights[0], heights[heights.len() - 1]);
        assert_ne!(first, last, "трапеция: края разной высоты ({first} vs {last})");
    }

    #[test]
    fn composite_respects_clip() {
        let src = solid(20, 20, 0, 0, 20, 20);
        let mut dst = tiny_skia::Pixmap::new(20, 20).unwrap();
        composite(&mut dst, &src, &IDENTITY, Some(Rect::new(0.0, 0.0, 10.0, 20.0)), (0.0, 0.0, 20.0, 20.0));
        assert_eq!(alpha_at(&dst, 5, 5), 255);
        assert_eq!(alpha_at(&dst, 15, 5), 0);
    }
}

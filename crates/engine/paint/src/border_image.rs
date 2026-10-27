//! CSS Backgrounds L3 §6.2–6.5 — геометрия `border-image`: девять клеток картинки и их тайлинг.
//!
//! Один чистый расчёт на все растеризаторы: [`border_image_quads`] получает размер картинки
//! (известен только растеризатору) и отдаёт пары «прямоугольник в пикселях картинки →
//! прямоугольник на странице». Растеризатор лишь рисует каждую пару.

use lumen_core::geom::Rect;
use serde::{Deserialize, Serialize};

/// Один срез `border-image-slice`: число — пиксели картинки, процент — доля её размера.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SliceValue {
    Number(f32),
    Percent(f32),
}

/// Ширина стороны `border-image-width`, уже приведённая эмиттером к px.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum WidthValue {
    Px(f32),
    /// Размер соответствующего среза картинки.
    Auto,
}

/// `border-image-repeat` по одной оси.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Stretch,
    Repeat,
    Round,
    Space,
}

/// Всё, что нужно растеризатору, кроме размеров рамки и самой картинки.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BorderImageSpec {
    /// top, right, bottom, left.
    pub slice: [SliceValue; 4],
    pub fill: bool,
    /// top, right, bottom, left.
    pub width: [WidthValue; 4],
    /// `[horizontal, vertical]`.
    pub repeat: [RepeatMode; 2],
}

/// Одна нарисованная клетка/плитка: `src` в пикселях картинки, `dst` на странице.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderImageQuad {
    pub src: Rect,
    pub dst: Rect,
}

/// Потолок числа плиток на одну клетку: узкий срез и широкая рамка не должны раздувать список.
const MAX_TILES: usize = 1024;

fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

fn nonempty(rc: &Rect) -> bool {
    rc.width > 0.0 && rc.height > 0.0
}

/// Клетки и плитки `border-image` для `area` (граница плюс outset) и картинки `img_w × img_h`.
///
/// Ширины сторон (`spec.width`) эмиттер уже перевёл в px; `Auto` берёт размер среза. Порядок
/// рисования: средняя клетка (`fill`), края, углы.
pub fn border_image_quads(
    spec: &BorderImageSpec,
    area: Rect,
    img_w: f32,
    img_h: f32,
) -> Vec<BorderImageQuad> {
    let mut out = Vec::new();
    if img_w <= 0.0 || img_h <= 0.0 || area.width <= 0.0 || area.height <= 0.0 {
        return out;
    }
    // §6.2 — срезы: проценты от размера картинки, значения больше размера читаются как 100%.
    let sl = |v: SliceValue, dim: f32| match v {
        SliceValue::Number(n) => n,
        SliceValue::Percent(p) => p / 100.0 * dim,
    }
    .clamp(0.0, dim);
    let st = sl(spec.slice[0], img_h);
    let sr = sl(spec.slice[1], img_w);
    let sb = sl(spec.slice[2], img_h);
    let sl_ = sl(spec.slice[3], img_w);

    // §6.3 — ширины; `auto` берёт размер среза. Потом пропорциональное сжатие под область.
    let wd = |w: WidthValue, slice_px: f32| match w {
        WidthValue::Px(v) => v.max(0.0),
        WidthValue::Auto => slice_px,
    };
    let mut wt = wd(spec.width[0], st);
    let mut wr = wd(spec.width[1], sr);
    let mut wb = wd(spec.width[2], sb);
    let mut wl = wd(spec.width[3], sl_);
    let sum_h = wl + wr;
    let sum_v = wt + wb;
    let mut f = 1.0_f32;
    if sum_h > 0.0 {
        f = f.min(area.width / sum_h);
    }
    if sum_v > 0.0 {
        f = f.min(area.height / sum_v);
    }
    if f < 1.0 {
        wt *= f;
        wr *= f;
        wb *= f;
        wl *= f;
    }

    let (ax, ay, aw, ah) = (area.x, area.y, area.width, area.height);
    let mid_dw = (aw - wl - wr).max(0.0);
    let mid_dh = (ah - wt - wb).max(0.0);
    let mid_sw = (img_w - sl_ - sr).max(0.0);
    let mid_sh = (img_h - st - sb).max(0.0);

    // Масштаб плитки вдоль края: высота плитки верхнего/нижнего края = её ширина рамки,
    // ширина плитки сохраняет пропорции среза (то же для боковых краёв).
    let scale_x = if st > 0.0 {
        wt / st
    } else if sb > 0.0 {
        wb / sb
    } else {
        1.0
    };
    let scale_y = if sl_ > 0.0 {
        wl / sl_
    } else if sr > 0.0 {
        wr / sr
    } else {
        1.0
    };
    let [rep_h, rep_v] = spec.repeat;

    // Средняя клетка — под краями, поэтому первой.
    if spec.fill && nonempty(&r(0.0, 0.0, mid_sw, mid_sh)) {
        tile_region(
            &mut out,
            r(sl_, st, mid_sw, mid_sh),
            r(ax + wl, ay + wt, mid_dw, mid_dh),
            (mid_sw * scale_x, mid_sh * scale_y),
            rep_h,
            rep_v,
        );
    }
    // Верх и низ: по горизонтали режим `repeat[0]`, по вертикали — растяжение на ширину рамки.
    if st > 0.0 && wt > 0.0 {
        tile_region(
            &mut out,
            r(sl_, 0.0, mid_sw, st),
            r(ax + wl, ay, mid_dw, wt),
            (mid_sw * scale_x, wt),
            rep_h,
            RepeatMode::Stretch,
        );
    }
    if sb > 0.0 && wb > 0.0 {
        let sx = wb / sb;
        tile_region(
            &mut out,
            r(sl_, img_h - sb, mid_sw, sb),
            r(ax + wl, ay + ah - wb, mid_dw, wb),
            (mid_sw * sx, wb),
            rep_h,
            RepeatMode::Stretch,
        );
    }
    // Левый и правый края: растяжение по ширине рамки, по вертикали — `repeat[1]`.
    if sl_ > 0.0 && wl > 0.0 {
        tile_region(
            &mut out,
            r(0.0, st, sl_, mid_sh),
            r(ax, ay + wt, wl, mid_dh),
            (wl, mid_sh * scale_y),
            RepeatMode::Stretch,
            rep_v,
        );
    }
    if sr > 0.0 && wr > 0.0 {
        let sy = wr / sr;
        tile_region(
            &mut out,
            r(img_w - sr, st, sr, mid_sh),
            r(ax + aw - wr, ay + wt, wr, mid_dh),
            (wr, mid_sh * sy),
            RepeatMode::Stretch,
            rep_v,
        );
    }
    // Углы — растягиваются на размер углов рамки.
    let corners = [
        (r(0.0, 0.0, sl_, st), r(ax, ay, wl, wt)),
        (r(img_w - sr, 0.0, sr, st), r(ax + aw - wr, ay, wr, wt)),
        (r(0.0, img_h - sb, sl_, sb), r(ax, ay + ah - wb, wl, wb)),
        (r(img_w - sr, img_h - sb, sr, sb), r(ax + aw - wr, ay + ah - wb, wr, wb)),
    ];
    for (src, dst) in corners {
        if nonempty(&src) && nonempty(&dst) {
            out.push(BorderImageQuad { src, dst });
        }
    }
    out
}

/// Раскладывает срез `src` по области `dst` плитками размера `tile` (в px страницы) по правилам
/// `repeat` на каждой оси; плитки обрезаются по `dst`, а `src` обрезается пропорционально.
fn tile_region(
    out: &mut Vec<BorderImageQuad>,
    src: Rect,
    dst: Rect,
    tile: (f32, f32),
    mode_x: RepeatMode,
    mode_y: RepeatMode,
) {
    if !nonempty(&src) || !nonempty(&dst) {
        return;
    }
    let xs = axis_tiles(dst.width, tile.0, mode_x);
    let ys = axis_tiles(dst.height, tile.1, mode_y);
    for &(ty, th) in &ys {
        for &(tx, tw) in &xs {
            if out.len() >= MAX_TILES * 9 {
                return;
            }
            // Плитка в координатах dst; пересечение с dst и пропорциональная обрезка src.
            let x0 = tx.max(0.0);
            let y0 = ty.max(0.0);
            let x1 = (tx + tw).min(dst.width);
            let y1 = (ty + th).min(dst.height);
            if x1 <= x0 || y1 <= y0 || tw <= 0.0 || th <= 0.0 {
                continue;
            }
            let u0 = (x0 - tx) / tw;
            let u1 = (x1 - tx) / tw;
            let v0 = (y0 - ty) / th;
            let v1 = (y1 - ty) / th;
            out.push(BorderImageQuad {
                src: r(
                    src.x + u0 * src.width,
                    src.y + v0 * src.height,
                    (u1 - u0) * src.width,
                    (v1 - v0) * src.height,
                ),
                dst: r(dst.x + x0, dst.y + y0, x1 - x0, y1 - y0),
            });
        }
    }
}

/// Начала и размеры плиток вдоль одной оси длиной `len` (координаты от начала области).
fn axis_tiles(len: f32, tile: f32, mode: RepeatMode) -> Vec<(f32, f32)> {
    if len <= 0.0 {
        return Vec::new();
    }
    if mode == RepeatMode::Stretch || tile <= 0.0 || !tile.is_finite() {
        return vec![(0.0, len)];
    }
    match mode {
        RepeatMode::Stretch => vec![(0.0, len)],
        RepeatMode::Repeat => {
            // Плитки центрируются; крайние обрезаются.
            let first = (len - tile) / 2.0;
            let back = (first / tile).ceil().max(0.0);
            let start = first - back * tile;
            let mut v = Vec::new();
            let mut x = start;
            while x < len && v.len() < MAX_TILES {
                v.push((x, tile));
                x += tile;
            }
            v
        }
        RepeatMode::Round => {
            let n = (len / tile).round().max(1.0).min(MAX_TILES as f32);
            let w = len / n;
            (0..n as usize).map(|i| (i as f32 * w, w)).collect()
        }
        RepeatMode::Space => {
            let n = (len / tile).floor().min(MAX_TILES as f32);
            if n < 1.0 {
                return Vec::new();
            }
            let gap = (len - n * tile) / (n + 1.0);
            (0..n as usize).map(|i| (gap + i as f32 * (tile + gap), tile)).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(slice: f32, width: f32, rep: RepeatMode) -> BorderImageSpec {
        BorderImageSpec {
            slice: [SliceValue::Number(slice); 4],
            fill: false,
            width: [WidthValue::Px(width); 4],
            repeat: [rep; 2],
        }
    }

    #[test]
    fn stretch_gives_eight_cells() {
        let q = border_image_quads(&spec(10.0, 5.0, RepeatMode::Stretch), r(0.0, 0.0, 100.0, 60.0), 30.0, 30.0);
        assert_eq!(q.len(), 8);
        // Верхний левый угол: срез 10×10 на рамку 5×5.
        let tl = q.iter().find(|c| c.dst.x == 0.0 && c.dst.y == 0.0).unwrap();
        assert_eq!(tl.src, r(0.0, 0.0, 10.0, 10.0));
        assert_eq!(tl.dst, r(0.0, 0.0, 5.0, 5.0));
    }

    #[test]
    fn fill_adds_middle_cell() {
        let mut s = spec(10.0, 5.0, RepeatMode::Stretch);
        s.fill = true;
        let q = border_image_quads(&s, r(0.0, 0.0, 100.0, 60.0), 30.0, 30.0);
        assert_eq!(q.len(), 9);
        assert_eq!(q[0].dst, r(5.0, 5.0, 90.0, 50.0));
        assert_eq!(q[0].src, r(10.0, 10.0, 10.0, 10.0));
    }

    #[test]
    fn widths_are_scaled_down_to_fit() {
        // 2*60 > 100: коэффициент 100/120 по ширине.
        let q = border_image_quads(&spec(10.0, 60.0, RepeatMode::Stretch), r(0.0, 0.0, 100.0, 400.0), 30.0, 30.0);
        let tl = q.iter().find(|c| c.dst.x == 0.0 && c.dst.y == 0.0).unwrap();
        assert!((tl.dst.width - 50.0).abs() < 1e-3);
        assert!((tl.dst.height - 50.0).abs() < 1e-3);
    }

    #[test]
    fn round_fits_whole_tiles() {
        // Верх: срез 10×10 → рамка 10, плитка 10 px; область 95 → 10 плиток по 9.5.
        let q = border_image_quads(&spec(10.0, 10.0, RepeatMode::Round), r(0.0, 0.0, 120.0, 100.0), 30.0, 30.0);
        let top: Vec<_> = q.iter().filter(|c| c.dst.y == 0.0 && c.dst.x >= 10.0 && c.dst.x < 110.0).collect();
        assert_eq!(top.len(), 10);
        assert!((top[0].dst.width - 10.0).abs() < 1e-3);
    }

    #[test]
    fn space_distributes_gaps() {
        // Область верха 100 px, плитка 30 → 3 плитки, зазор (100-90)/4 = 2.5.
        let s = BorderImageSpec {
            slice: [SliceValue::Number(30.0), SliceValue::Number(0.0), SliceValue::Number(0.0), SliceValue::Number(0.0)],
            fill: false,
            width: [WidthValue::Px(30.0), WidthValue::Px(0.0), WidthValue::Px(0.0), WidthValue::Px(0.0)],
            repeat: [RepeatMode::Space, RepeatMode::Stretch],
        };
        let q = border_image_quads(&s, r(0.0, 0.0, 100.0, 60.0), 30.0, 90.0);
        assert_eq!(q.len(), 3);
        assert!((q[0].dst.x - 2.5).abs() < 1e-3);
        assert!((q[1].dst.x - 35.0).abs() < 1e-3);
    }

    #[test]
    fn repeat_centres_and_clips_tiles() {
        // Верх: длина 100, плитка 30: центральная в 35..65, соседние 5..35 и 65..95, крайние обрезаны до 0..5 и 95..100.
        let s = BorderImageSpec {
            slice: [SliceValue::Number(30.0), SliceValue::Number(0.0), SliceValue::Number(0.0), SliceValue::Number(0.0)],
            fill: false,
            width: [WidthValue::Px(30.0), WidthValue::Px(0.0), WidthValue::Px(0.0), WidthValue::Px(0.0)],
            repeat: [RepeatMode::Repeat, RepeatMode::Stretch],
        };
        let q = border_image_quads(&s, r(0.0, 0.0, 100.0, 60.0), 30.0, 90.0);
        assert_eq!(q.len(), 5);
        let first = q.iter().map(|c| c.dst.x).fold(f32::MAX, f32::min);
        assert_eq!(first, 0.0);
        let total: f32 = q.iter().map(|c| c.dst.width).sum();
        assert!((total - 100.0).abs() < 1e-3);
    }

    #[test]
    fn percent_slice_and_auto_width() {
        let s = BorderImageSpec {
            slice: [SliceValue::Percent(50.0); 4],
            fill: false,
            width: [WidthValue::Auto; 4],
            repeat: [RepeatMode::Stretch; 2],
        };
        let q = border_image_quads(&s, r(0.0, 0.0, 200.0, 200.0), 40.0, 40.0);
        // Срез 20×20, ширина auto = 20: углы 20×20; середины клеток пусты (50+50 = 100%).
        assert_eq!(q.len(), 4);
        assert!(q.iter().all(|c| c.dst.width == 20.0 && c.dst.height == 20.0));
    }

    #[test]
    fn empty_image_or_area_yields_nothing() {
        let s = spec(10.0, 5.0, RepeatMode::Stretch);
        assert!(border_image_quads(&s, r(0.0, 0.0, 10.0, 10.0), 0.0, 10.0).is_empty());
        assert!(border_image_quads(&s, r(0.0, 0.0, 0.0, 10.0), 10.0, 10.0).is_empty());
    }
}

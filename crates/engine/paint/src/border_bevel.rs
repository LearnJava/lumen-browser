//! Объёмные стили рамки `groove` / `ridge` / `inset` / `outset` — геометрия и цвета,
//! общие для всех рендер-бэкендов (CSS Backgrounds L3 §4.2).
//!
//! Один источник истины, как `dash_math` для dashed/dotted: бэкенды (`cpu_raster`,
//! wgpu `renderer`, `femtovg_backend`) получают от [`paint_bevel_sides`] список
//! `(Rect, Color)` и закрашивают его своим примитивом заливки.
//!
//! Правила сняты с Edge (`msedge --headless --screenshot` страниц с `border: Npx <style>`):
//! - толщина стороны — целое число пикселей (`floor`, но не меньше 1 у ненулевой), границы
//!   бокса округляются до пикселя — Edge рисует `1.5px` как 1px, `2.5px` как 2px;
//! - `groove` / `ridge` — сторона из двух полос вдоль толщины; в экранном порядке (меньшие
//!   x/y первыми) первая занимает `ceil(N / 2)` px, вторая — остаток, на всех четырёх
//!   сторонах одинаково. `groove` = тёмная, затем светлая; `ridge` = светлая, затем тёмная.
//!   Толщина 1px — цвет рамки как есть;
//! - `inset` / `outset` — сторона одним оттенком: `inset` = тёмный сверху/слева, светлый
//!   снизу/справа; `outset` наоборот. Оттенки — `Color::Dark()/Light()` Chromium
//!   ([`groove_shades`]);
//! - стык двух объёмных сторон — диагональ от внешнего угла к внутреннему; пиксели диагонали
//!   смешиваются по доле покрытия (4×4 подвыборки). Стык с необъёмной (или нулевой)
//!   соседней стороной — прямой: объёмная сторона идёт до края бокса.

use lumen_core::geom::Rect;
use lumen_layout::{BorderStyle, Color};

use crate::gap_decorations::groove_shades;

/// Сторона рамки; значение — индекс в массивах `[top, right, bottom, left]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BevelSide {
    Top = 0,
    Right = 1,
    Bottom = 2,
    Left = 3,
}

/// `true` для `groove` / `ridge` / `inset` / `outset`.
#[must_use]
pub fn is_bevel(style: BorderStyle) -> bool {
    matches!(
        style,
        BorderStyle::Groove | BorderStyle::Ridge | BorderStyle::Inset | BorderStyle::Outset
    )
}

/// Толщина стороны в целых пикселях, как её рисует Edge.
fn snap_width(w: f32) -> i32 {
    if w <= 0.0 {
        0
    } else {
        (w.floor() as i32).max(1)
    }
}

/// Цвет полосы стороны на расстоянии `offset` px от её экранного начала (меньшие x/y).
fn band_color(
    side: BevelSide,
    thickness: i32,
    offset: i32,
    color: Color,
    style: BorderStyle,
) -> Color {
    let (dark, light) = groove_shades(color);
    match style {
        BorderStyle::Groove | BorderStyle::Ridge => {
            if thickness < 2 {
                return color;
            }
            let first = (thickness + 1) / 2;
            let (a, b) = if style == BorderStyle::Groove {
                (dark, light)
            } else {
                (light, dark)
            };
            if offset < first {
                a
            } else {
                b
            }
        }
        _ => {
            let top_left = matches!(side, BevelSide::Top | BevelSide::Left);
            if top_left == (style == BorderStyle::Inset) {
                dark
            } else {
                light
            }
        }
    }
}

/// Доля горизонтальной стороны в пикселе стыка. `i` — расстояние пикселя от внешнего
/// вертикального края, `j` — от внешнего горизонтального; `v`/`h` — толщины вертикальной и
/// горизонтальной сторон. Диагональ идёт от внешнего угла `(0, 0)` к внутреннему `(v, h)`.
fn horizontal_coverage(i: i32, j: i32, v: i32, h: i32) -> f32 {
    const N: i32 = 4;
    // Удвоенные доли: подвыборка на самой диагонали считается за половину
    // (пиксель диагонали квадратного угла — ровно 50/50, как у Edge).
    let mut hits2 = 0;
    for a in 0..N {
        for b in 0..N {
            let si = i as f32 + (a as f32 + 0.5) / N as f32;
            let sj = j as f32 + (b as f32 + 0.5) / N as f32;
            hits2 += if si >= v as f32 {
                2
            } else if sj >= h as f32 {
                0
            } else {
                let (lhs, rhs) = (sj * v as f32, si * h as f32);
                if (lhs - rhs).abs() < 1e-4 {
                    1
                } else {
                    i32::from(lhs < rhs) * 2
                }
            };
        }
    }
    hits2 as f32 / (2 * N * N) as f32
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let m = |x: u8, y: u8| (f32::from(x) * t + f32::from(y) * (1.0 - t)).round() as u8;
    Color {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: m(a.a, b.a),
    }
}

/// Целочисленный бокс рамки и толщины сторон `[top, right, bottom, left]`.
struct Geom {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    w: [i32; 4],
}

/// Прямоугольники одной стороны объёмной рамки. Пиксели стыка со смежной объёмной
/// стороной принадлежат горизонтальной стороне (`Top`/`Bottom`): вертикальная их не рисует.
fn side_pieces(
    side: BevelSide,
    g: &Geom,
    colors: [Color; 4],
    styles: [BorderStyle; 4],
) -> Vec<(Rect, Color)> {
    let idx = |s: BevelSide| s as usize;
    let thickness = g.w[idx(side)];
    let joins = |s: BevelSide| is_bevel(styles[idx(s)]) && g.w[idx(s)] > 0;
    let mut out: Vec<(Rect, Color)> = Vec::new();
    let mut push = |x: i32, y: i32, w: i32, h: i32, c: Color| {
        if w > 0 && h > 0 {
            out.push((Rect::new(x as f32, y as f32, w as f32, h as f32), c));
        }
    };
    let own = |off: i32| band_color(side, thickness, off, colors[idx(side)], styles[idx(side)]);
    match side {
        BevelSide::Top | BevelSide::Bottom => {
            let top = side == BevelSide::Top;
            let h = thickness;
            let origin = if top { g.y0 } else { g.y1 - h };
            for r in 0..h {
                let y = origin + r;
                // Расстояние строки от внешнего горизонтального края.
                let j = if top { r } else { h - 1 - r };
                let c_own = own(r);
                let mut x_from = g.x0;
                let mut x_to = g.x1;
                // Левый стык (соседка `Left`) и правый (`Right`).
                for (is_left, neighbour) in [(true, BevelSide::Left), (false, BevelSide::Right)] {
                    if !joins(neighbour) {
                        continue;
                    }
                    let v = g.w[idx(neighbour)];
                    // Сколько пикселей от внешнего края не отданы горизонтальной стороне целиком.
                    let mut edge = 0;
                    for i in 0..v {
                        let cov = horizontal_coverage(i, j, v, h);
                        if cov < 1.0 {
                            edge = i + 1;
                        }
                        if cov > 0.0 && cov < 1.0 {
                            let off = if is_left { i } else { v - 1 - i };
                            let c_nb = band_color(
                                neighbour,
                                v,
                                off,
                                colors[idx(neighbour)],
                                styles[idx(neighbour)],
                            );
                            let x = if is_left { g.x0 + i } else { g.x1 - 1 - i };
                            push(x, y, 1, 1, mix(c_own, c_nb, cov));
                        }
                    }
                    if is_left {
                        x_from = g.x0 + edge;
                    } else {
                        x_to = g.x1 - edge;
                    }
                }
                push(x_from, y, x_to - x_from, 1, c_own);
            }
        }
        BevelSide::Left | BevelSide::Right => {
            let left = side == BevelSide::Left;
            let v = thickness;
            let origin = if left { g.x0 } else { g.x1 - v };
            for cidx in 0..v {
                let x = origin + cidx;
                // Расстояние столбца от внешнего вертикального края.
                let i = if left { cidx } else { v - 1 - cidx };
                let c_own = own(cidx);
                let mut y_from = g.y0;
                let mut y_to = g.y1;
                for (is_top, neighbour) in [(true, BevelSide::Top), (false, BevelSide::Bottom)] {
                    if !joins(neighbour) {
                        continue;
                    }
                    let h = g.w[idx(neighbour)];
                    // Пиксели стыка, где есть хоть доля горизонтальной стороны, рисует она.
                    let mut edge = 0;
                    for j in 0..h {
                        if horizontal_coverage(i, j, v, h) > 0.0 {
                            edge = j + 1;
                        }
                    }
                    if is_top {
                        y_from = g.y0 + edge;
                    } else {
                        y_to = g.y1 - edge;
                    }
                }
                push(x, y_from, 1, y_to - y_from, c_own);
            }
        }
    }
    merge_strips(out, matches!(side, BevelSide::Top | BevelSide::Bottom))
}

/// Склеивает соседние полоски одного цвета и одной протяжённости.
fn merge_strips(strips: Vec<(Rect, Color)>, horizontal: bool) -> Vec<(Rect, Color)> {
    let mut out: Vec<(Rect, Color)> = Vec::with_capacity(strips.len());
    for (r, c) in strips {
        if let Some((last, lc)) = out.last_mut() {
            let mergeable = *lc == c
                && if horizontal {
                    last.x == r.x && last.width == r.width && last.y + last.height == r.y
                } else {
                    last.y == r.y && last.height == r.height && last.x + last.width == r.x
                };
            if mergeable {
                if horizontal {
                    last.height += r.height;
                } else {
                    last.width += r.width;
                }
                continue;
            }
        }
        out.push((r, c));
    }
    out
}

/// Закрашивает объёмные стороны рамки через `fill` и возвращает стили, в которых эти стороны
/// заменены на `None`: бэкенд дорисовывает остальные стороны своим обычным путём и не
/// рисует объёмные повторно. `rect`/`widths`/`colors`/`styles` — поля `DrawBorder`
/// (порядок `[top, right, bottom, left]`).
pub fn paint_bevel_sides(
    rect: Rect,
    widths: [f32; 4],
    colors: [Color; 4],
    styles: [BorderStyle; 4],
    mut fill: impl FnMut(Rect, Color),
) -> [BorderStyle; 4] {
    let mut rest = styles;
    if !styles.iter().any(|s| is_bevel(*s)) {
        return rest;
    }
    let g = Geom {
        x0: rect.x.round() as i32,
        y0: rect.y.round() as i32,
        x1: (rect.x + rect.width).round() as i32,
        y1: (rect.y + rect.height).round() as i32,
        w: widths.map(snap_width),
    };
    for side in [
        BevelSide::Top,
        BevelSide::Right,
        BevelSide::Bottom,
        BevelSide::Left,
    ] {
        let i = side as usize;
        if !is_bevel(styles[i]) {
            continue;
        }
        if g.w[i] > 0 && g.x1 > g.x0 && g.y1 > g.y0 {
            for (piece, color) in side_pieces(side, &g, colors, styles) {
                fill(piece, color);
            }
        }
        rest[i] = BorderStyle::None;
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    const GREY: Color = Color {
        r: 0x80,
        g: 0x80,
        b: 0x80,
        a: 255,
    };
    const DARK: Color = Color {
        r: 0x2c,
        g: 0x2c,
        b: 0x2c,
        a: 255,
    };
    const LIGHT: Color = Color {
        r: 0xd4,
        g: 0xd4,
        b: 0xd4,
        a: 255,
    };

    type Pixels = Vec<Vec<Option<Color>>>;

    /// Раскрашивает бокс `w`×`h` в карту пикселей `[y][x]` (`None` — не закрашен).
    fn paint(w: usize, h: usize, widths: [f32; 4], style: BorderStyle, color: Color) -> Pixels {
        let mut px = vec![vec![None; w]; h];
        let rect = Rect::new(0.0, 0.0, w as f32, h as f32);
        let rest = paint_bevel_sides(rect, widths, [color; 4], [style; 4], |r, c| {
            for row in px.iter_mut().skip(r.y as usize).take(r.height as usize) {
                for cell in row.iter_mut().skip(r.x as usize).take(r.width as usize) {
                    assert!(cell.is_none(), "пиксель закрашен дважды: {r:?}");
                    *cell = Some(c);
                }
            }
        });
        assert!(rest.iter().all(|s| *s == BorderStyle::None));
        px
    }

    fn col(px: &Pixels, x: usize, ys: std::ops::Range<usize>) -> Vec<Option<Color>> {
        ys.map(|y| px[y][x]).collect()
    }

    fn row(px: &Pixels, y: usize, xs: std::ops::Range<usize>) -> Vec<Option<Color>> {
        xs.map(|x| px[y][x]).collect()
    }

    /// 52×52, рамка 6px `#808080` (Edge): `2c2c2c` ×3, затем `d4d4d4` ×3 на каждой стороне.
    #[test]
    fn groove_and_ridge_match_edge_pixels() {
        let g = paint(52, 52, [6.0; 4], BorderStyle::Groove, GREY);
        let want = [DARK, DARK, DARK, LIGHT, LIGHT, LIGHT].map(Some);
        assert_eq!(col(&g, 26, 0..6), want);
        assert_eq!(col(&g, 26, 46..52), want);
        assert_eq!(row(&g, 26, 0..6), want);
        assert_eq!(row(&g, 26, 46..52), want);
        // Внутренность не тронута.
        assert!(g[26][26].is_none());
        let r = paint(52, 52, [6.0; 4], BorderStyle::Ridge, GREY);
        assert_eq!(
            col(&r, 26, 0..6),
            [LIGHT, LIGHT, LIGHT, DARK, DARK, DARK].map(Some)
        );
    }

    /// Нечётная толщина: первая полоса `ceil(5/2) = 3` px, вторая 2 px — и у нижней стороны.
    #[test]
    fn odd_width_first_band_is_the_larger_one_on_every_side() {
        let c = rgb(0xc0, 0x30, 0x20);
        let (dark, light) = (rgb(0x6c, 0x1b, 0x12), rgb(0xff, 0x3f, 0x2a));
        let g = paint(45, 45, [5.0; 4], BorderStyle::Groove, c);
        let want = [dark, dark, dark, light, light].map(Some);
        assert_eq!(col(&g, 22, 0..5), want);
        assert_eq!(col(&g, 22, 40..45), want);
    }

    /// `inset`: тёмный сверху/слева, светлый снизу/справа (даже 1px); `outset` наоборот.
    #[test]
    fn inset_and_outset_are_one_shade_per_side() {
        let i = paint(30, 30, [1.0; 4], BorderStyle::Inset, GREY);
        assert_eq!(
            (i[0][15], i[29][15], i[15][0], i[15][29]),
            (Some(DARK), Some(LIGHT), Some(DARK), Some(LIGHT))
        );
        let o = paint(30, 30, [4.0; 4], BorderStyle::Outset, GREY);
        assert_eq!(
            (o[1][15], o[28][15], o[15][1], o[15][28]),
            (Some(LIGHT), Some(DARK), Some(LIGHT), Some(DARK))
        );
    }

    /// 1px `groove` — цвет как есть; `black` получает `#545454`/`#A8A8A8`; дробная толщина
    /// округляется вниз (`2.5px` → 2px).
    #[test]
    fn thin_groove_keeps_the_colour_and_widths_floor() {
        let g = paint(20, 20, [1.0; 4], BorderStyle::Groove, GREY);
        assert_eq!(g[0][10], Some(GREY));
        let b = paint(20, 20, [3.0; 4], BorderStyle::Ridge, rgb(0, 0, 0));
        let (l, d) = (rgb(0xa8, 0xa8, 0xa8), rgb(0x54, 0x54, 0x54));
        assert_eq!(col(&b, 10, 0..3), [l, l, d].map(Some));
        let f = paint(30, 30, [2.5; 4], BorderStyle::Ridge, GREY);
        assert_eq!(col(&f, 15, 0..3), [Some(LIGHT), Some(DARK), None]);
    }

    /// Разная толщина соседей: кольцо закрашено целиком и без наложений (assert в `paint`).
    #[test]
    fn unequal_widths_cover_the_ring_without_overlap() {
        let px = paint(40, 30, [6.0, 3.0, 4.0, 8.0], BorderStyle::Groove, GREY);
        for (y, line) in px.iter().enumerate() {
            for (x, p) in line.iter().enumerate() {
                let ring = x < 8 || y < 6 || x >= 37 || y >= 26;
                assert_eq!(p.is_some(), ring, "({x},{y})");
            }
        }
    }

    /// Объёмная сторона рядом с необъёмной идёт до края бокса (прямой стык); возвращённые
    /// стили у объёмных сторон — `None`, у остальных прежние.
    #[test]
    fn bevel_next_to_plain_side_is_square_and_styles_are_split() {
        let mut painted = Vec::new();
        let rest = paint_bevel_sides(
            Rect::new(0.0, 0.0, 20.0, 20.0),
            [4.0; 4],
            [GREY; 4],
            [
                BorderStyle::Groove,
                BorderStyle::Solid,
                BorderStyle::None,
                BorderStyle::Inset,
            ],
            |r, c| painted.push((r, c)),
        );
        assert_eq!(
            rest,
            [
                BorderStyle::None,
                BorderStyle::Solid,
                BorderStyle::None,
                BorderStyle::None
            ]
        );
        // Top: сосед справа — Solid, значит строка 0 идёт до правого края (x = 20).
        assert!(painted
            .iter()
            .any(|(r, _)| r.y == 0.0 && r.x + r.width == 20.0));
        let untouched = paint_bevel_sides(
            Rect::new(0.0, 0.0, 9.0, 9.0),
            [3.0; 4],
            [GREY; 4],
            [BorderStyle::Solid; 4],
            |_, _| panic!("solid is not a bevel"),
        );
        assert_eq!(untouched, [BorderStyle::Solid; 4]);
    }
}

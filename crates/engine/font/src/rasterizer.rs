//! Glyph rasterizer: outline → grayscale bitmap.
//!
//! Phase 0 — простой, медленный, корректный путь:
//! 1. Контур обходится с учётом on-curve / off-curve флагов; квадратичные
//!    Безье разворачиваются в 8 коротких отрезков.
//! 2. Bitmap размером bbox + 1px padding на сторону.
//! 3. Каждый пиксель сэмплируется 4×4 раза, для каждого сэмпла —
//!    ray-casting point-in-polygon с even-odd правилом (как в SVG/PDF
//!    при отсутствии fill-rule). Покрытие → 8-битный grayscale.
//!
//! Замены / оптимизации в дальнейшем:
//! - адаптивная subdivision Безье (сейчас фиксированные 8 шагов);
//! - scanline-based active-edge-table (O(edges · log edges · height)
//!   вместо O(width · height · 16 · edges));
//! - SDF-подход для масштабируемого рендера.

use crate::glyf::{Contour, Glyph, Outline};

#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// `width × height` байт, row-major, по 1 байту на пиксель (coverage 0..255).
    pub pixels: Vec<u8>,
    /// Где левый край bitmap-а относительно origin'а глифа (cursor X),
    /// в пикселях. Обычно совпадает с `floor(bbox.x_min × scale) − padding`.
    pub left: f32,
    /// Сколько пикселей верхний край bitmap-а находится НАД baseline-ом.
    /// Положительное число = bitmap above baseline. Обычно совпадает с
    /// `ceil(bbox.y_max × scale) + padding`.
    pub top: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Rasterizer {
    pub pixel_size: f32,
    pub units_per_em: u16,
}

impl Rasterizer {
    pub fn new(pixel_size: f32, units_per_em: u16) -> Self {
        assert!(units_per_em > 0, "units_per_em must be > 0");
        Self {
            pixel_size,
            units_per_em,
        }
    }

    pub fn scale(&self) -> f32 {
        self.pixel_size / self.units_per_em as f32
    }

    /// Растеризует simple-glyph. Возвращает `None` для composite-глифов
    /// и пустого outline.
    pub fn rasterize(&self, glyph: &Glyph) -> Option<Bitmap> {
        let Outline::Simple(contours) = &glyph.outline else {
            return None;
        };
        if contours.is_empty() {
            return None;
        }

        let scale = self.scale();
        let pad = 1.0_f32;

        let x_min = (glyph.bbox.x_min as f32 * scale - pad).floor() as i32;
        let y_min = (glyph.bbox.y_min as f32 * scale - pad).floor() as i32;
        let x_max = (glyph.bbox.x_max as f32 * scale + pad).ceil() as i32;
        let y_max = (glyph.bbox.y_max as f32 * scale + pad).ceil() as i32;
        let width = (x_max - x_min) as u32;
        let height = (y_max - y_min) as u32;
        if width == 0 || height == 0 {
            return None;
        }

        let mut edges: Vec<Edge> = Vec::new();
        for contour in contours {
            walk_contour(contour, scale, x_min as f32, y_max as f32, &mut edges);
        }

        let mut pixels = vec![0u8; (width as usize) * (height as usize)];
        fill_pixels(&edges, width, height, &mut pixels);
        Some(Bitmap {
            width,
            height,
            pixels,
            left: x_min as f32,
            top: y_max as f32,
        })
    }
}

type Point = (f32, f32);
type Edge = (f32, f32, f32, f32); // (x1, y1, x2, y2) в pixel space (Y вниз)

fn walk_contour(
    contour: &Contour,
    scale: f32,
    bitmap_x_min: f32,
    bitmap_y_max: f32,
    edges: &mut Vec<Edge>,
) {
    let pts = &contour.points;
    let n = pts.len();
    if n < 2 {
        return;
    }

    // Перевод font-units (Y вверх) → bitmap pixels (Y вниз).
    let to_pixel = |i: usize| -> Point {
        let p = &pts[i];
        (
            p.x as f32 * scale - bitmap_x_min,
            bitmap_y_max - p.y as f32 * scale,
        )
    };

    let first_on = (0..n).find(|&i| pts[i].on_curve);
    let (start_idx, init_anchor) = match first_on {
        Some(i) => (i, to_pixel(i)),
        None => {
            // Все точки off-curve → синтетический якорь в середине pts[n-1]/pts[0].
            (n - 1, midpoint(to_pixel(n - 1), to_pixel(0)))
        }
    };

    let mut anchor = init_anchor;
    let mut pending: Option<Point> = None;

    for offset in 1..=n {
        let i = (start_idx + offset) % n;
        let p = to_pixel(i);
        let on = pts[i].on_curve;

        if on {
            match pending.take() {
                None => edges.push((anchor.0, anchor.1, p.0, p.1)),
                Some(c) => flatten_quad(anchor, c, p, edges),
            }
            anchor = p;
        } else if let Some(c) = pending {
            let m = midpoint(c, p);
            flatten_quad(anchor, c, m, edges);
            anchor = m;
            pending = Some(p);
        } else {
            pending = Some(p);
        }
    }

    // Замыкаем контур обратно к синтетическому якорю, если все точки были off-curve.
    if first_on.is_none()
        && let Some(c) = pending
    {
        flatten_quad(anchor, c, init_anchor, edges);
    }
}

fn midpoint(a: Point, b: Point) -> Point {
    ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
}

fn flatten_quad(p0: Point, c: Point, p2: Point, out: &mut Vec<Edge>) {
    const STEPS: usize = 8;
    let mut prev = p0;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let inv = 1.0 - t;
        let x = inv * inv * p0.0 + 2.0 * inv * t * c.0 + t * t * p2.0;
        let y = inv * inv * p0.1 + 2.0 * inv * t * c.1 + t * t * p2.1;
        out.push((prev.0, prev.1, x, y));
        prev = (x, y);
    }
}

fn fill_pixels(edges: &[Edge], width: u32, height: u32, pixels: &mut [u8]) {
    const N: u32 = 4;
    let total = N * N;
    for py in 0..height {
        for px in 0..width {
            let mut inside = 0u32;
            for sy in 0..N {
                let y = py as f32 + (sy as f32 + 0.5) / N as f32;
                for sx in 0..N {
                    let x = px as f32 + (sx as f32 + 0.5) / N as f32;
                    if point_inside(edges, x, y) {
                        inside += 1;
                    }
                }
            }
            pixels[(py * width + px) as usize] = (inside * 255 / total) as u8;
        }
    }
}

/// Ray-casting от точки (px, py) вправо. Считаем пересечения с рёбрами
/// по правилу half-open Y: [min_y, max_y). Чётное число пересечений —
/// снаружи, нечётное — внутри.
fn point_inside(edges: &[Edge], px: f32, py: f32) -> bool {
    let mut crossings = 0u32;
    for &(x1, y1, x2, y2) in edges {
        let (ax, ay, bx, by) = if y1 <= y2 {
            (x1, y1, x2, y2)
        } else {
            (x2, y2, x1, y1)
        };
        if py < ay || py >= by {
            continue;
        }
        let dy = by - ay;
        if dy == 0.0 {
            continue;
        }
        let t = (py - ay) / dy;
        let xint = ax + t * (bx - ax);
        if xint > px {
            crossings += 1;
        }
    }
    crossings & 1 == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyf::{BoundingBox, Contour, Glyph, Outline, OutlinePoint};

    fn pt(x: i16, y: i16, on: bool) -> OutlinePoint {
        OutlinePoint {
            x,
            y,
            on_curve: on,
        }
    }

    fn coverage_at(bm: &Bitmap, x: u32, y: u32) -> u8 {
        bm.pixels[(y * bm.width + x) as usize]
    }

    #[test]
    fn rasterize_filled_triangle() {
        // Треугольник 100×100 font units: (0,0), (100,0), (50,100).
        // pixel_size = 100, units_per_em = 100 → scale 1.0 → bitmap 102×102 (с 1px padding).
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 100,
                y_max: 100,
            },
            outline: Outline::Simple(vec![Contour {
                points: vec![pt(0, 0, true), pt(100, 0, true), pt(50, 100, true)],
            }]),
        };
        let bm = Rasterizer::new(100.0, 100).rasterize(&glyph).unwrap();
        assert_eq!(bm.width, 102);
        assert_eq!(bm.height, 102);

        // Центр должен быть внутри (apex в font вверху → после Y-flip он сверху bitmap-а).
        assert!(coverage_at(&bm, 51, 60) > 200, "center should be filled");
        // Левый край сильно за треугольником.
        assert!(coverage_at(&bm, 1, 50) < 30, "outside-left should be empty");
        // Верхний левый угол (точка над apex'ом).
        assert!(coverage_at(&bm, 5, 5) < 30, "above-apex should be empty");
    }

    #[test]
    fn composite_glyph_returns_none() {
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 10,
                y_max: 10,
            },
            outline: Outline::Composite(Vec::new()),
        };
        assert!(Rasterizer::new(16.0, 1000).rasterize(&glyph).is_none());
    }

    #[test]
    fn empty_outline_returns_none() {
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 0,
                y_max: 0,
            },
            outline: Outline::Simple(Vec::new()),
        };
        assert!(Rasterizer::new(16.0, 1000).rasterize(&glyph).is_none());
    }

    #[test]
    fn even_odd_rule_makes_hole_in_donut() {
        // Внешний квадрат + внутренний квадрат, оба counter-clockwise.
        // Even-odd: внутренность внешнего без внутреннего = «бублик».
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 100,
                y_max: 100,
            },
            outline: Outline::Simple(vec![
                Contour {
                    points: vec![
                        pt(0, 0, true),
                        pt(100, 0, true),
                        pt(100, 100, true),
                        pt(0, 100, true),
                    ],
                },
                Contour {
                    points: vec![
                        pt(30, 30, true),
                        pt(70, 30, true),
                        pt(70, 70, true),
                        pt(30, 70, true),
                    ],
                },
            ]),
        };
        let bm = Rasterizer::new(100.0, 100).rasterize(&glyph).unwrap();
        // Точка между кольцами (например, (20, 50)) должна быть заполнена.
        assert!(coverage_at(&bm, 20, 50) > 200, "ring should be filled");
        // Точка в центре «дырки» — пусто.
        assert!(coverage_at(&bm, 51, 51) < 30, "hole should be empty");
    }

    #[test]
    fn quad_bezier_with_off_curve_control() {
        // Сегмент on (0,0) — off (50,100) — on (100,0): кривая, поднимающаяся
        // и опускающаяся. Дополнительный сегмент on (100,0) → on (0, 0)
        // замыкает контур (нижнее ребро).
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 100,
                y_max: 100,
            },
            outline: Outline::Simple(vec![Contour {
                points: vec![pt(0, 0, true), pt(50, 100, false), pt(100, 0, true)],
            }]),
        };
        let bm = Rasterizer::new(100.0, 100).rasterize(&glyph).unwrap();
        // На y=20 над основанием (что в pixel space — высоко-низко после flip)
        // ожидаем заполнение.
        let mid_x = bm.width / 2;
        assert!(coverage_at(&bm, mid_x, bm.height - 10) > 100);
    }

    #[test]
    fn quad_bezier_with_two_off_curve_implies_midpoint() {
        // Контур: on(0,0), off(50,100), off(100,50), on(100,0). Между двумя
        // off-curve подразумевается on-curve в midpoint(75, 75) — формирует
        // S-подобную кривую. Главное — что парсер не падает и что-то рисует.
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 100,
                y_max: 100,
            },
            outline: Outline::Simple(vec![Contour {
                points: vec![
                    pt(0, 0, true),
                    pt(50, 100, false),
                    pt(100, 50, false),
                    pt(100, 0, true),
                ],
            }]),
        };
        let bm = Rasterizer::new(100.0, 100).rasterize(&glyph).unwrap();
        // Внутри bbox у пикселя в районе основания есть покрытие.
        assert!(coverage_at(&bm, 30, bm.height - 5) > 50);
    }

    #[test]
    fn scale_changes_bitmap_size() {
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 0,
                x_max: 1000,
                y_max: 1000,
            },
            outline: Outline::Simple(vec![Contour {
                points: vec![
                    pt(0, 0, true),
                    pt(1000, 0, true),
                    pt(1000, 1000, true),
                    pt(0, 1000, true),
                ],
            }]),
        };
        // units_per_em=1000, pixel_size=16 → scale 0.016 → 16×16 (+ 1px padding) = 18×18.
        let bm = Rasterizer::new(16.0, 1000).rasterize(&glyph).unwrap();
        assert_eq!(bm.width, 18);
        assert_eq!(bm.height, 18);
        // Центр квадрата полностью заполнен.
        assert!(coverage_at(&bm, 9, 9) > 240);
    }
}

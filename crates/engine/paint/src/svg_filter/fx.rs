//! Примитивы SVG-фильтра с окрестностью или шумом: `feTile`, `feMorphology`,
//! `feConvolveMatrix`, `feDisplacementMap`, `feTurbulence`, освещение
//! (Filter Effects L1 §«Filter primitives»). Все работают над [`Img`] —
//! premultiplied f32 RGBA рабочей области.

use lumen_layout::{FeEdgeMode, FeLight, FeLighting};

use super::{Img, PxRect};

/// `feTile`: подобласть входа `from` замощает всё изображение.
pub(super) fn tile(src: &Img, from: PxRect) -> Img {
    let mut out = Img::new(src.w, src.h);
    let (tw, th) = (from.x1 - from.x0, from.y1 - from.y0);
    if tw <= 0 || th <= 0 {
        return out;
    }
    for y in 0..src.h as i32 {
        let sy = from.y0 + (y - from.y0).rem_euclid(th);
        for x in 0..src.w as i32 {
            let sx = from.x0 + (x - from.x0).rem_euclid(tw);
            if (0..src.w as i32).contains(&sx) && (0..src.h as i32).contains(&sy) {
                out.d[y as usize * src.w + x as usize] = src.d[sy as usize * src.w + sx as usize];
            }
        }
    }
    out
}

/// `feMorphology`: минимум (`erode`) или максимум (`dilate`) по окну
/// `(2·rx+1) × (2·ry+1)` в каждом канале; за краем изображения — прозрачность.
pub(super) fn morphology(src: &Img, dilate: bool, rx: usize, ry: usize) -> Img {
    let pass = |img: &Img, r: usize, horizontal: bool| -> Img {
        if r == 0 {
            return img.clone();
        }
        let mut out = Img::new(img.w, img.h);
        let (len, lines) = if horizontal { (img.w, img.h) } else { (img.h, img.w) };
        let idx = |line: usize, i: usize| if horizontal { line * img.w + i } else { i * img.w + line };
        for line in 0..lines {
            for i in 0..len {
                let lo = i as isize - r as isize;
                let hi = i as isize + r as isize;
                let mut acc = if dilate { [0.0f32; 4] } else { [f32::INFINITY; 4] };
                for j in lo..=hi {
                    let p = if (0..len as isize).contains(&j) { img.d[idx(line, j as usize)] } else { [0.0; 4] };
                    for c in 0..4 {
                        acc[c] = if dilate { acc[c].max(p[c]) } else { acc[c].min(p[c]) };
                    }
                }
                out.d[idx(line, i)] = acc;
            }
        }
        out
    };
    pass(&pass(src, rx, true), ry, false)
}

/// Параметры `feConvolveMatrix`.
pub(super) struct Convolve<'a> {
    pub order: (usize, usize),
    pub kernel: &'a [f32],
    pub divisor: f32,
    pub bias: f32,
    pub target: (i32, i32),
    pub edge: FeEdgeMode,
    pub preserve_alpha: bool,
}

/// `feConvolveMatrix` (Filter Effects L1 §«feConvolveMatrix»): свёртка по
/// premultiplied цвету; с `preserveAlpha` — по непремультиплицированному, альфа
/// проходит без изменений.
pub(super) fn convolve(src: &Img, p: &Convolve<'_>) -> Img {
    let (ox, oy) = (p.order.0 as i32, p.order.1 as i32);
    let (w, h) = (src.w as i32, src.h as i32);
    let unpre = |px: [f32; 4]| {
        if p.preserve_alpha && px[3] > 0.0 { [px[0] / px[3], px[1] / px[3], px[2] / px[3], px[3]] } else { px }
    };
    let fetch = |x: i32, y: i32| -> [f32; 4] {
        let (x, y) = match p.edge {
            FeEdgeMode::Duplicate => (x.clamp(0, w - 1), y.clamp(0, h - 1)),
            FeEdgeMode::Wrap => (x.rem_euclid(w), y.rem_euclid(h)),
            FeEdgeMode::None => {
                if x < 0 || y < 0 || x >= w || y >= h {
                    return [0.0; 4];
                }
                (x, y)
            }
        };
        unpre(src.d[y as usize * src.w + x as usize])
    };
    let mut out = Img::new(src.w, src.h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for j in 0..oy {
                for i in 0..ox {
                    let k = p.kernel[((oy - 1 - j) * ox + (ox - 1 - i)) as usize];
                    if k == 0.0 {
                        continue;
                    }
                    let s = fetch(x - p.target.0 + i, y - p.target.1 + j);
                    for c in 0..4 {
                        acc[c] += s[c] * k;
                    }
                }
            }
            let centre = src.d[y as usize * src.w + x as usize];
            let alpha = if p.preserve_alpha {
                centre[3]
            } else {
                (acc[3] / p.divisor + p.bias).clamp(0.0, 1.0)
            };
            let mut px = [0.0f32; 4];
            for c in 0..3 {
                // Как в Blink: `bias` прибавляется к premultiplied цвету без умножения на альфу.
                let v = acc[c] / p.divisor + p.bias;
                px[c] = if p.preserve_alpha { v.clamp(0.0, 1.0) * alpha } else { v.clamp(0.0, alpha) };
            }
            px[3] = alpha;
            out.d[y as usize * src.w + x as usize] = px;
        }
    }
    out
}

/// `feDisplacementMap`: пиксель `P(x, y)` берётся из `src` в
/// `(x + sx·(XC − ½), y + sy·(YC − ½))`, `XC`/`YC` — непремультиплицированные
/// каналы карты `map`; за краем — прозрачность.
pub(super) fn displacement(src: &Img, map: &Img, scale: (f32, f32), channels: (usize, usize)) -> Img {
    let mut out = Img::new(src.w, src.h);
    let (w, h) = (src.w as i32, src.h as i32);
    for y in 0..h {
        for x in 0..w {
            let m = map.d[y as usize * src.w + x as usize];
            let a = m[3];
            let chan = |c: usize| if c == 3 { a } else if a > 0.0 { m[c] / a } else { 0.0 };
            let sx = (x as f32 + scale.0 * (chan(channels.0) - 0.5)).round() as i32;
            let sy = (y as f32 + scale.1 * (chan(channels.1) - 0.5)).round() as i32;
            if (0..w).contains(&sx) && (0..h).contains(&sy) {
                out.d[y as usize * src.w + x as usize] = src.d[sy as usize * src.w + sx as usize];
            }
        }
    }
    out
}

// ── feTurbulence: эталонная реализация Perlin-шума из спецификации ──────────

const B_SIZE: usize = 0x100;
const BM: i32 = 0xff;
const PERLIN_N: i32 = 0x1000;
const RAND_M: i64 = 2_147_483_647;
const RAND_A: i64 = 16807;
const RAND_Q: i64 = 127_773;
const RAND_R: i64 = 2836;

struct Noise {
    lattice: [usize; B_SIZE + B_SIZE + 2],
    gradient: Box<[[[f64; 2]; B_SIZE + B_SIZE + 2]; 4]>,
}

fn random(seed: i64) -> i64 {
    let r = RAND_A * (seed % RAND_Q) - RAND_R * (seed / RAND_Q);
    if r <= 0 { r + RAND_M } else { r }
}

impl Noise {
    fn new(seed: i32) -> Self {
        let mut seed = i64::from(seed);
        if seed <= 0 {
            seed = -(seed % (RAND_M - 1)) + 1;
        }
        seed = seed.min(RAND_M - 1);
        let mut n = Noise {
            lattice: [0; B_SIZE + B_SIZE + 2],
            gradient: Box::new([[[0.0; 2]; B_SIZE + B_SIZE + 2]; 4]),
        };
        for k in 0..4 {
            for i in 0..B_SIZE {
                n.lattice[i] = i;
                for j in 0..2 {
                    seed = random(seed);
                    n.gradient[k][i][j] = ((seed % (B_SIZE as i64 * 2)) - B_SIZE as i64) as f64 / B_SIZE as f64;
                }
                let s = (n.gradient[k][i][0].powi(2) + n.gradient[k][i][1].powi(2)).sqrt();
                n.gradient[k][i][0] /= s;
                n.gradient[k][i][1] /= s;
            }
        }
        for i in (1..B_SIZE).rev() {
            let k = n.lattice[i];
            seed = random(seed);
            let j = (seed % B_SIZE as i64) as usize;
            n.lattice[i] = n.lattice[j];
            n.lattice[j] = k;
        }
        for i in 0..B_SIZE + 2 {
            n.lattice[B_SIZE + i] = n.lattice[i];
            for k in 0..4 {
                n.gradient[k][B_SIZE + i] = n.gradient[k][i];
            }
        }
        n
    }

    fn noise2(&self, ch: usize, vec: [f64; 2], stitch: Option<&Stitch>) -> f64 {
        let t = vec[0] + f64::from(PERLIN_N);
        let mut bx0 = (t as i32) & BM;
        let mut bx1 = (bx0 + 1) & BM;
        let rx0 = t - f64::from(t as i32);
        let rx1 = rx0 - 1.0;
        let t = vec[1] + f64::from(PERLIN_N);
        let mut by0 = (t as i32) & BM;
        let mut by1 = (by0 + 1) & BM;
        let ry0 = t - f64::from(t as i32);
        let ry1 = ry0 - 1.0;
        if let Some(s) = stitch {
            if bx0 >= s.wrap_x {
                bx0 -= s.width;
            }
            if bx1 >= s.wrap_x {
                bx1 -= s.width;
            }
            if by0 >= s.wrap_y {
                by0 -= s.height;
            }
            if by1 >= s.wrap_y {
                by1 -= s.height;
            }
        }
        let (bx0, bx1, by0, by1) = ((bx0 & BM) as usize, (bx1 & BM) as usize, (by0 & BM) as usize, (by1 & BM) as usize);
        let (i, j) = (self.lattice[bx0], self.lattice[bx1]);
        let (b00, b10, b01, b11) =
            (self.lattice[i + by0], self.lattice[j + by0], self.lattice[i + by1], self.lattice[j + by1]);
        let s_curve = |t: f64| t * t * (3.0 - 2.0 * t);
        let (sx, sy) = (s_curve(rx0), s_curve(ry0));
        let g = &self.gradient[ch];
        let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
        let u = rx0 * g[b00][0] + ry0 * g[b00][1];
        let v = rx1 * g[b10][0] + ry0 * g[b10][1];
        let a = lerp(sx, u, v);
        let u = rx0 * g[b01][0] + ry1 * g[b01][1];
        let v = rx1 * g[b11][0] + ry1 * g[b11][1];
        lerp(sy, a, lerp(sx, u, v))
    }
}

struct Stitch {
    width: i32,
    height: i32,
    wrap_x: i32,
    wrap_y: i32,
}

/// Параметры `feTurbulence`; `point` переводит пиксель рабочей области в user space.
pub(super) struct Turbulence<'a> {
    pub fx: f64,
    pub fy: f64,
    pub octaves: u32,
    pub seed: i32,
    pub stitch: bool,
    pub fractal: bool,
    /// Тайл для `stitchTiles` в user space: `(x, y, w, h)`.
    pub tile: (f64, f64, f64, f64),
    pub point: &'a dyn Fn(usize, usize) -> (f64, f64),
}

/// Шум в каждом пикселе; результат — непремультиплицированный RGBA, переведённый
/// в premultiplied. Отрицательная частота или нулевое число октав — прозрачность.
pub(super) fn turbulence(w: usize, h: usize, p: &Turbulence<'_>) -> Img {
    let mut out = Img::new(w, h);
    if p.fx < 0.0 || p.fy < 0.0 || p.octaves == 0 {
        return out;
    }
    let noise = Noise::new(p.seed);
    let (mut fx, mut fy) = (p.fx, p.fy);
    let (tx, ty, tw, th) = p.tile;
    if p.stitch {
        let adjust = |f: f64, size: f64| {
            if f == 0.0 {
                return f;
            }
            let lo = (size * f).floor() / size;
            let hi = (size * f).ceil() / size;
            if f / lo < hi / f { lo } else { hi }
        };
        fx = adjust(fx, tw);
        fy = adjust(fy, th);
    }
    for y in 0..h {
        for x in 0..w {
            let pt = (p.point)(x, y);
            let mut px = [0.0f32; 4];
            for (ch, o) in px.iter_mut().enumerate() {
                let mut stitch = p.stitch.then(|| {
                    let width = (tw * fx + 0.5) as i32;
                    let height = (th * fy + 0.5) as i32;
                    Stitch {
                        width,
                        height,
                        wrap_x: (tx * fx + f64::from(PERLIN_N) + f64::from(width)) as i32,
                        wrap_y: (ty * fy + f64::from(PERLIN_N) + f64::from(height)) as i32,
                    }
                });
                let mut vec = [pt.0 * fx, pt.1 * fy];
                let (mut sum, mut ratio) = (0.0f64, 1.0f64);
                for _ in 0..p.octaves {
                    let n = noise.noise2(ch, vec, stitch.as_ref());
                    sum += if p.fractal { n } else { n.abs() } / ratio;
                    vec = [vec[0] * 2.0, vec[1] * 2.0];
                    ratio *= 2.0;
                    if let Some(s) = stitch.as_mut() {
                        s.width *= 2;
                        s.wrap_x = 2 * s.wrap_x - PERLIN_N;
                        s.height *= 2;
                        s.wrap_y = 2 * s.wrap_y - PERLIN_N;
                    }
                }
                *o = (if p.fractal { (sum + 1.0) / 2.0 } else { sum }).clamp(0.0, 1.0) as f32;
            }
            let a = px[3];
            out.d[y * w + x] = [px[0] * a, px[1] * a, px[2] * a, a];
        }
    }
    out
}

// ── освещение ───────────────────────────────────────────────────────────────

/// Параметры `feDiffuseLighting` / `feSpecularLighting` в пикселях рабочей области.
pub(super) struct Lighting {
    pub kind: FeLighting,
    pub surface_scale: f32,
    pub color: [f32; 3],
    pub light: FeLight,
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 { [v[0] / l, v[1] / l, v[2] / l] } else { v }
}

/// Освещение с картой высот из альфа-канала `src` (карта нормалей по ядру Собеля
/// с краевыми вариантами, Filter Effects L1 §«feDiffuseLighting»).
pub(super) fn lighting(src: &Img, p: &Lighting) -> Img {
    let (w, h) = (src.w as i32, src.h as i32);
    let alpha = |x: i32, y: i32| src.d[y as usize * src.w + x as usize][3];
    let mut out = Img::new(src.w, src.h);
    for y in 0..h {
        for x in 0..w {
            // Нормаль: веса строк/столбцов (1, 2, 1), на краю — только существующие.
            let (xl, xr) = ((x - 1).max(0), (x + 1).min(w - 1));
            let (yt, yb) = ((y - 1).max(0), (y + 1).min(h - 1));
            let weight = |v: i32, c: i32| if v == c { 2.0 } else { 1.0 };
            let (mut nx, mut sw_y) = (0.0f32, 0.0f32);
            for yy in yt..=yb {
                let wy = weight(yy, y);
                nx += wy * (alpha(xr, yy) - alpha(xl, yy));
                sw_y += wy;
            }
            let (mut ny, mut sw_x) = (0.0f32, 0.0f32);
            for xx in xl..=xr {
                let wx = weight(xx, x);
                ny += wx * (alpha(xx, yb) - alpha(xx, yt));
                sw_x += wx;
            }
            let fx = if xr > xl { 2.0 / ((xr - xl) as f32 * sw_y) } else { 0.0 };
            let fy = if yb > yt { 2.0 / ((yb - yt) as f32 * sw_x) } else { 0.0 };
            let normal = normalize([-p.surface_scale * fx * nx, -p.surface_scale * fy * ny, 1.0]);
            let z = p.surface_scale * alpha(x, y);
            let (l, lcol) = match p.light {
                FeLight::Distant { azimuth, elevation } => {
                    let (az, el) = (azimuth.to_radians(), elevation.to_radians());
                    ([az.cos() * el.cos(), az.sin() * el.cos(), el.sin()], p.color)
                }
                FeLight::Point { pos } => {
                    (normalize([pos[0] - x as f32, pos[1] - y as f32, pos[2] - z]), p.color)
                }
                FeLight::Spot { pos, points_at, exponent, cone } => {
                    let l = normalize([pos[0] - x as f32, pos[1] - y as f32, pos[2] - z]);
                    let s = normalize([points_at[0] - pos[0], points_at[1] - pos[1], points_at[2] - pos[2]]);
                    let minus_l_dot_s = -(l[0] * s[0] + l[1] * s[1] + l[2] * s[2]);
                    let lit = minus_l_dot_s > 0.0 && cone.is_none_or(|c| minus_l_dot_s >= c.abs().to_radians().cos());
                    let f = if lit { minus_l_dot_s.powf(exponent) } else { 0.0 };
                    (l, p.color.map(|c| c * f))
                }
            };
            let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            out.d[y as usize * src.w + x as usize] = match p.kind {
                FeLighting::Diffuse { kd } => {
                    let f = kd * dot(normal, l).max(0.0);
                    [(lcol[0] * f).min(1.0), (lcol[1] * f).min(1.0), (lcol[2] * f).min(1.0), 1.0]
                }
                FeLighting::Specular { ks, exponent } => {
                    let half = normalize([l[0], l[1], l[2] + 1.0]);
                    let f = ks * dot(normal, half).max(0.0).powf(exponent);
                    let c = lcol.map(|c| (c * f).min(1.0));
                    [c[0], c[1], c[2], c[0].max(c[1]).max(c[2])]
                }
            };
        }
    }
    out
}

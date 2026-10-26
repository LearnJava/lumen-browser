//! Исполнитель графа SVG-фильтра (Filter Effects L1 §«Filter primitives») над
//! растром слоя — общая математика CPU-растеризатора.
//!
//! Слой — полноразмерный pixmap в координатах страницы (1 px = 1 CSS px), как
//! у остальных групповых эффектов `cpu_raster`. Рабочая область — область
//! фильтра (`filterUnits`, пересечённая с холстом), все промежуточные
//! изображения — premultiplied f32 RGBA в sRGB; примитив, считающий в
//! `linearRGB` (значение по умолчанию), переводит входы в линейное
//! пространство и результат обратно. Вне подобласти примитива результат —
//! прозрачный чёрный. Поворот/скос CTM не учитывается: фильтр работает в
//! осях страницы с масштабом `a`/`d` матрицы user space.

use lumen_core::geom::Rect;
use lumen_layout::{
    FeBlendMode, FeColorMatrix, FeCompositeOp, FeInput, FeLight, FePrimitive, FeTransfer, FilterUrl,
    SvgFilter,
};

use crate::display_list::BlendMode;

mod fx;

/// Premultiplied f32 RGBA изображение `w × h`.
#[derive(Clone)]
struct Img {
    w: usize,
    h: usize,
    d: Vec<[f32; 4]>,
}

impl Img {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, d: vec![[0.0; 4]; w * h] }
    }
}

/// Прямоугольник в пикселях рабочей области (полуоткрытый `[x0, x1) × [y0, y1)`).
#[derive(Clone, Copy, PartialEq)]
struct PxRect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl PxRect {
    fn union(self, o: PxRect) -> PxRect {
        PxRect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

/// Результат примитива: картинка и её подобласть.
struct Res {
    img: Img,
    sub: PxRect,
}

/// Окно результата в координатах холста.
pub(crate) struct Windowed {
    pub pixmap: tiny_skia::Pixmap,
    pub x: i32,
    pub y: i32,
}

/// Применяет граф `url.graph` к `src` (pixmap-окно с началом `(ox, oy)` в
/// координатах холста `canvas_w × canvas_h`). `bounds` — border box элемента.
/// `None` — граф не определён (ссылка в никуда): вызывающий пропускает фильтр.
/// Пустая область/пустой граф дают `Some` с прозрачным окном нулевого размера
/// (`pixmap` 1×1), т.е. элемент не виден.
pub(crate) fn apply(
    src: &tiny_skia::Pixmap,
    origin: (i32, i32),
    canvas: (u32, u32),
    url: &FilterUrl,
    bounds: Rect,
) -> Option<Windowed> {
    let graph = url.graph.as_ref()?;
    let g = Geometry::new(url, bounds);
    let Some(region) = region_px(graph, &g, canvas) else {
        return empty_window();
    };
    let (w, h) = ((region.x1 - region.x0) as usize, (region.y1 - region.y0) as usize);
    // SourceGraphic = пересечение слоя-источника с областью фильтра.
    let mut source = Img::new(w, h);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let data = src.data();
    for y in 0..h as i32 {
        let sy = region.y0 + y - origin.1;
        if sy < 0 || sy >= sh {
            continue;
        }
        for x in 0..w as i32 {
            let sx = region.x0 + x - origin.0;
            if sx < 0 || sx >= sw {
                continue;
            }
            let i = (sy * sw + sx) as usize * 4;
            source.d[y as usize * w + x as usize] = [
                f32::from(data[i]) / 255.0,
                f32::from(data[i + 1]) / 255.0,
                f32::from(data[i + 2]) / 255.0,
                f32::from(data[i + 3]) / 255.0,
            ];
        }
    }
    let local_region = PxRect { x0: 0, y0: 0, x1: w as i32, y1: h as i32 };
    let out = run_graph(graph, &g, &source, region, local_region);
    let mut pm = tiny_skia::Pixmap::new(w as u32, h as u32)?;
    let od = pm.data_mut();
    for (i, px) in out.d.iter().enumerate() {
        let a = px[3].clamp(0.0, 1.0);
        for c in 0..3 {
            od[i * 4 + c] = (px[c].clamp(0.0, a) * 255.0).round() as u8;
        }
        od[i * 4 + 3] = (a * 255.0).round() as u8;
    }
    Some(Windowed { pixmap: pm, x: region.x0, y: region.y0 })
}

fn empty_window() -> Option<Windowed> {
    Some(Windowed { pixmap: tiny_skia::Pixmap::new(1, 1)?, x: 0, y: 0 })
}

/// Отображение user space → страница: `x' = sx·x + tx`, `y' = sy·y + ty`.
struct Geometry {
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
    /// bbox элемента в user space `(x, y, w, h)`.
    bbox: (f32, f32, f32, f32),
}

impl Geometry {
    fn new(url: &FilterUrl, bounds: Rect) -> Self {
        match url.placement {
            Some(p) => Self {
                sx: p.matrix[0],
                sy: p.matrix[3],
                tx: p.matrix[4] + bounds.x,
                ty: p.matrix[5] + bounds.y,
                bbox: p.user_bbox,
            },
            None => Self {
                sx: 1.0,
                sy: 1.0,
                tx: bounds.x,
                ty: bounds.y,
                bbox: (0.0, 0.0, bounds.width, bounds.height),
            },
        }
    }

    /// Прямоугольник user space → страница.
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
        (self.sx * x + self.tx, self.sy * y + self.ty, self.sx * w, self.sy * h)
    }
}

fn to_px(r: (f32, f32, f32, f32)) -> PxRect {
    PxRect {
        x0: r.0.round() as i32,
        y0: r.1.round() as i32,
        x1: (r.0 + r.2).round() as i32,
        y1: (r.1 + r.3).round() as i32,
    }
}

/// Область фильтра в пикселях холста, пересечённая с холстом; `None` — пусто.
fn region_px(graph: &SvgFilter, g: &Geometry, canvas: (u32, u32)) -> Option<PxRect> {
    let (rx, ry, rw, rh) = graph.region;
    let (bx, by, bw, bh) = g.bbox;
    let user = if graph.filter_units_obb {
        if bw <= 0.0 || bh <= 0.0 {
            return None;
        }
        (bx + rx * bw, by + ry * bh, rw * bw, rh * bh)
    } else {
        (rx, ry, rw, rh)
    };
    if user.2 <= 0.0 || user.3 <= 0.0 {
        return None;
    }
    let r = to_px(g.rect(user.0, user.1, user.2, user.3));
    let r = PxRect { x0: r.x0.max(0), y0: r.y0.max(0), x1: r.x1.min(canvas.0 as i32), y1: r.y1.min(canvas.1 as i32) };
    (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
}

fn run_graph(graph: &SvgFilter, g: &Geometry, source: &Img, region: PxRect, local: PxRect) -> Img {
    let (w, h) = (source.w, source.h);
    let source_alpha = {
        let mut s = Img::new(w, h);
        for (o, i) in s.d.iter_mut().zip(&source.d) {
            *o = [0.0, 0.0, 0.0, i[3]];
        }
        s
    };
    let (bx, by, bw, bh) = g.bbox;
    let mut results: Vec<Res> = Vec::with_capacity(graph.primitives.len());
    let mut last: Option<usize> = None;
    let mut tainted: Vec<bool> = Vec::with_capacity(graph.primitives.len());

    for (idx, prim) in graph.primitives.iter().enumerate() {
        // Входы примитива (до вычисления подобласти: по ним считается её умолчание).
        let inputs_of: Vec<FeInput> = match &prim.kind {
            FePrimitive::Flood { .. } => vec![],
            FePrimitive::Turbulence { .. } => vec![],
            FePrimitive::Unsupported { input } => vec![*input],
            FePrimitive::Offset { input, .. }
            | FePrimitive::Tile { input }
            | FePrimitive::Morphology { input, .. }
            | FePrimitive::ConvolveMatrix { input, .. }
            | FePrimitive::Lighting { input, .. }
            | FePrimitive::GaussianBlur { input, .. }
            | FePrimitive::ColorMatrix { input, .. }
            | FePrimitive::ComponentTransfer { input, .. }
            | FePrimitive::DropShadow { input, .. } => vec![*input],
            FePrimitive::Composite { a, b, .. } | FePrimitive::Blend { a, b, .. } => vec![*a, *b],
            FePrimitive::DisplacementMap { input, map, .. } => vec![*input, *map],
            FePrimitive::Merge(v) => v.clone(),
        };
        let fetch = |inp: FeInput, results: &[Res]| -> (Img, PxRect) {
            match inp {
                FeInput::SourceGraphic => (source.clone(), local),
                FeInput::SourceAlpha => (source_alpha.clone(), local),
                FeInput::Empty => (Img::new(w, h), local),
                FeInput::Previous => match last {
                    Some(i) => (results[i].img.clone(), results[i].sub),
                    None => (source.clone(), local),
                },
                FeInput::Result(i) => match results.get(i) {
                    Some(r) => (r.img.clone(), r.sub),
                    None => (source.clone(), local),
                },
            }
        };
        let fetched: Vec<(Img, PxRect)> = inputs_of.iter().map(|&i| fetch(i, &results)).collect();
        let input_tainted = |inp: FeInput| match inp {
            FeInput::Previous => last.is_some_and(|i| tainted[i]),
            FeInput::Result(i) => tainted.get(i).copied().unwrap_or(false),
            _ => false,
        };
        let is_tainted = prim.tainted || inputs_of.iter().any(|&i| input_tainted(i));
        // feTile замощает всю область фильтра, а не подобласть входа (как в Blink).
        let default_sub = if matches!(prim.kind, FePrimitive::Tile { .. }) {
            local
        } else {
            fetched.iter().map(|f| f.1).reduce(PxRect::union).unwrap_or(local)
        };

        // Подобласть: заданные x/y/width/height поверх умолчания.
        let mut sub = default_sub;
        let obb = graph.primitive_units_obb;
        let ux = |v: f32| if obb { bx + v * bw } else { v };
        let uy = |v: f32| if obb { by + v * bh } else { v };
        let uw = |v: f32| if obb { v * bw } else { v };
        let uh = |v: f32| if obb { v * bh } else { v };
        if prim.x.is_some() || prim.y.is_some() || prim.width.is_some() || prim.height.is_some() {
            let cur = (
                (default_sub.x0 + region.x0) as f32,
                (default_sub.y0 + region.y0) as f32,
                (default_sub.x1 - default_sub.x0) as f32,
                (default_sub.y1 - default_sub.y0) as f32,
            );
            let x = prim.x.map_or(cur.0, |v| g.sx * ux(v) + g.tx);
            let y = prim.y.map_or(cur.1, |v| g.sy * uy(v) + g.ty);
            let wd = prim.width.map_or(cur.2, |v| g.sx * uw(v));
            let ht = prim.height.map_or(cur.3, |v| g.sy * uh(v));
            let r = to_px((x - region.x0 as f32, y - region.y0 as f32, wd, ht));
            sub = PxRect { x0: r.x0.max(0), y0: r.y0.max(0), x1: r.x1.min(w as i32), y1: r.y1.min(h as i32) };
        }

        let lin = prim.linear_rgb;
        // Масштаб чисел примитива (`dx`, `stdDeviation`) в px страницы.
        let kx = g.sx.abs() * if obb { bw } else { 1.0 };
        let ky = g.sy.abs() * if obb { bh } else { 1.0 };
        let conv_in = |img: &Img| if lin { to_linear(img) } else { img.clone() };
        let conv_out = |img: Img| if lin { to_srgb(&img) } else { img };

        let out: Img = match &prim.kind {
            FePrimitive::Flood { color, opacity } => {
                let a = opacity.clamp(0.0, 1.0) * f32::from(color.a) / 255.0;
                let px = [
                    f32::from(color.r) / 255.0 * a,
                    f32::from(color.g) / 255.0 * a,
                    f32::from(color.b) / 255.0 * a,
                    a,
                ];
                let mut img = Img::new(w, h);
                img.d.fill(px);
                img
            }
            FePrimitive::Offset { dx, dy, .. } => {
                offset(&fetched[0].0, (dx * kx).round() as i32, (dy * ky).round() as i32)
            }
            FePrimitive::GaussianBlur { sx, sy, .. } => {
                conv_out(blur(&conv_in(&fetched[0].0), sx * kx, sy * ky))
            }
            FePrimitive::ColorMatrix { matrix, .. } => {
                conv_out(color_matrix(&conv_in(&fetched[0].0), matrix))
            }
            FePrimitive::ComponentTransfer { funcs, .. } => {
                conv_out(component_transfer(&conv_in(&fetched[0].0), funcs))
            }
            FePrimitive::Composite { op, .. } => {
                conv_out(composite(&conv_in(&fetched[0].0), &conv_in(&fetched[1].0), *op))
            }
            FePrimitive::Blend { mode, .. } => {
                conv_out(blend(&conv_in(&fetched[0].0), &conv_in(&fetched[1].0), *mode))
            }
            FePrimitive::Merge(_) => {
                let mut acc = Img::new(w, h);
                for (img, _) in &fetched {
                    acc = composite(&conv_in(img), &acc, FeCompositeOp::Over);
                }
                conv_out(acc)
            }
            FePrimitive::DropShadow { dx, dy, sx, sy, color, opacity, .. } => {
                let input = conv_in(&fetched[0].0);
                let mut mask = Img::new(w, h);
                for (m, p) in mask.d.iter_mut().zip(&input.d) {
                    *m = [0.0, 0.0, 0.0, p[3]];
                }
                let mask = offset(&blur(&mask, sx * kx, sy * ky), (dx * kx).round() as i32, (dy * ky).round() as i32);
                let a = opacity.clamp(0.0, 1.0) * f32::from(color.a) / 255.0;
                let mut c = [
                    f32::from(color.r) / 255.0,
                    f32::from(color.g) / 255.0,
                    f32::from(color.b) / 255.0,
                ];
                if lin {
                    c = c.map(srgb_to_linear);
                }
                let mut shadow = mask;
                for p in &mut shadow.d {
                    let al = p[3] * a;
                    *p = [c[0] * al, c[1] * al, c[2] * al, al];
                }
                conv_out(composite(&input, &shadow, FeCompositeOp::Over))
            }
            FePrimitive::Tile { .. } => fx::tile(&fetched[0].0, fetched[0].1),
            FePrimitive::Morphology { dilate, rx, ry, .. } => {
                let (rx, ry) = (rx * kx, ry * ky);
                if rx < 0.0 || ry < 0.0 {
                    Img::new(w, h)
                } else {
                    fx::morphology(&fetched[0].0, *dilate, rx.round() as usize, ry.round() as usize)
                }
            }
            FePrimitive::ConvolveMatrix { kernel, order, divisor, bias, target, edge, preserve_alpha, .. } => {
                if kernel.is_empty() {
                    Img::new(w, h)
                } else {
                    let p = fx::Convolve {
                        order: *order,
                        kernel,
                        divisor: *divisor,
                        bias: *bias,
                        target: *target,
                        edge: *edge,
                        preserve_alpha: *preserve_alpha,
                    };
                    conv_out(fx::convolve(&conv_in(&fetched[0].0), &p))
                }
            }
            FePrimitive::DisplacementMap { map, .. } if input_tainted(*map) => fetched[0].0.clone(),
            FePrimitive::DisplacementMap { scale, x_channel, y_channel, .. } => {
                conv_out(fx::displacement(&fetched[0].0, &conv_in(&fetched[1].0), (scale * kx, scale * ky), (*x_channel, *y_channel)))
            }
            FePrimitive::Turbulence { fx: bfx, fy: bfy, octaves, seed, stitch, fractal } => {
                let to_user = |px: f32, py: f32| {
                    (f64::from((px - g.tx) / g.sx), f64::from((py - g.ty) / g.sy))
                };
                let (x0, y0) = to_user((sub.x0 + region.x0) as f32, (sub.y0 + region.y0) as f32);
                let (x1, y1) = to_user((sub.x1 + region.x0) as f32, (sub.y1 + region.y0) as f32);
                let point = |x: usize, y: usize| to_user((x as i32 + region.x0) as f32, (y as i32 + region.y0) as f32);
                let p = fx::Turbulence {
                    fx: f64::from(*bfx),
                    fy: f64::from(*bfy),
                    octaves: *octaves,
                    seed: *seed,
                    stitch: *stitch,
                    fractal: *fractal,
                    tile: (x0, y0, x1 - x0, y1 - y0),
                    point: &point,
                };
                conv_out(fx::turbulence(w, h, &p))
            }
            FePrimitive::Lighting { kind, surface_scale, color, light, .. } => match light {
                None => Img::new(w, h),
                Some(light) => {
                    let (ux0, uy0) = (region.x0 as f32, region.y0 as f32);
                    let zk = if obb { ((bw * bw + bh * bh) / 2.0).sqrt() } else { 1.0 }
                        * ((g.sx * g.sx + g.sy * g.sy) / 2.0).sqrt();
                    let place = |p: &[f32; 3]| {
                        [g.sx * ux(p[0]) + g.tx - ux0, g.sy * uy(p[1]) + g.ty - uy0, p[2] * zk]
                    };
                    let light = match light {
                        FeLight::Distant { .. } => *light,
                        FeLight::Point { pos } => FeLight::Point { pos: place(pos) },
                        FeLight::Spot { pos, points_at, exponent, cone } => FeLight::Spot {
                            pos: place(pos),
                            points_at: place(points_at),
                            exponent: *exponent,
                            cone: *cone,
                        },
                    };
                    let mut c = [f32::from(color.r) / 255.0, f32::from(color.g) / 255.0, f32::from(color.b) / 255.0];
                    if lin {
                        c = c.map(srgb_to_linear);
                    }
                    let p = fx::Lighting { kind: *kind, surface_scale: *surface_scale, color: c, light };
                    conv_out(fx::lighting(&fetched[0].0, &p))
                }
            },
            FePrimitive::Unsupported { .. } => fetched[0].0.clone(),
        };
        let mut out = out;
        clip_to(&mut out, sub);
        results.push(Res { img: out, sub });
        tainted.push(is_tainted);
        last = Some(idx);
    }
    match last {
        Some(i) => results.swap_remove(i).img,
        None => Img::new(w, h),
    }
}

fn clip_to(img: &mut Img, r: PxRect) {
    for y in 0..img.h as i32 {
        for x in 0..img.w as i32 {
            if x < r.x0 || x >= r.x1 || y < r.y0 || y >= r.y1 {
                img.d[y as usize * img.w + x as usize] = [0.0; 4];
            }
        }
    }
}

fn offset(src: &Img, dx: i32, dy: i32) -> Img {
    let mut out = Img::new(src.w, src.h);
    for y in 0..src.h as i32 {
        let sy = y - dy;
        if sy < 0 || sy >= src.h as i32 {
            continue;
        }
        for x in 0..src.w as i32 {
            let sx = x - dx;
            if sx < 0 || sx >= src.w as i32 {
                continue;
            }
            out.d[y as usize * src.w + x as usize] = src.d[sy as usize * src.w + sx as usize];
        }
    }
    out
}

// ── цветовые пространства ──────────────────────────────────────────────────

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn map_color(img: &Img, f: impl Fn(f32) -> f32) -> Img {
    let mut out = Img::new(img.w, img.h);
    for (o, p) in out.d.iter_mut().zip(&img.d) {
        let a = p[3];
        if a <= 0.0 {
            continue;
        }
        *o = [f(p[0] / a) * a, f(p[1] / a) * a, f(p[2] / a) * a, a];
    }
    out
}

fn to_linear(img: &Img) -> Img {
    map_color(img, |c| srgb_to_linear(c.clamp(0.0, 1.0)))
}

fn to_srgb(img: &Img) -> Img {
    map_color(img, |c| linear_to_srgb(c.clamp(0.0, 1.0)))
}

// ── размытие ───────────────────────────────────────────────────────────────

fn blur(src: &Img, sx: f32, sy: f32) -> Img {
    let mut cur = src.clone();
    if sx > 0.0 {
        cur = blur_axis(&cur, sx, true);
    }
    if sy > 0.0 {
        cur = blur_axis(&cur, sy, false);
    }
    cur
}

/// Размытие вдоль одной оси: тройной box blur при σ ≥ 2 (Filter Effects L1
/// §«feGaussianBlur»), иначе — честное гауссово ядро. За краем — прозрачность.
fn blur_axis(src: &Img, sigma: f32, horizontal: bool) -> Img {
    let (len, lines) = if horizontal { (src.w, src.h) } else { (src.h, src.w) };
    let idx = |line: usize, i: usize| if horizontal { line * src.w + i } else { i * src.w + line };
    let mut out = Img::new(src.w, src.h);
    let mut buf = vec![[0.0f32; 4]; len];
    let mut tmp = vec![[0.0f32; 4]; len];
    for line in 0..lines {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = src.d[idx(line, i)];
        }
        if sigma >= 2.0 {
            let d = (sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0 + 0.5).floor() as usize;
            let d = d.max(1);
            if d % 2 == 1 {
                for _ in 0..3 {
                    box_pass(&buf, &mut tmp, d, d / 2);
                    std::mem::swap(&mut buf, &mut tmp);
                }
            } else {
                box_pass(&buf, &mut tmp, d, d / 2);
                std::mem::swap(&mut buf, &mut tmp);
                box_pass(&buf, &mut tmp, d, d / 2 - 1);
                std::mem::swap(&mut buf, &mut tmp);
                box_pass(&buf, &mut tmp, d + 1, d.div_ceil(2));
                std::mem::swap(&mut buf, &mut tmp);
            }
        } else {
            gauss_pass(&buf, &mut tmp, sigma);
            std::mem::swap(&mut buf, &mut tmp);
        }
        for (i, b) in buf.iter().enumerate() {
            out.d[idx(line, i)] = *b;
        }
    }
    out
}

/// Окно `[i − left, i − left + size)` среднего; вне ряда — нули.
fn box_pass(src: &[[f32; 4]], dst: &mut [[f32; 4]], size: usize, left: usize) {
    let n = src.len();
    let mut prefix = vec![[0.0f32; 4]; n + 1];
    for i in 0..n {
        for c in 0..4 {
            prefix[i + 1][c] = prefix[i][c] + src[i][c];
        }
    }
    let inv = 1.0 / size as f32;
    for (i, o) in dst.iter_mut().enumerate() {
        let lo = (i as isize - left as isize).clamp(0, n as isize) as usize;
        let hi = (i as isize - left as isize + size as isize).clamp(0, n as isize) as usize;
        for c in 0..4 {
            o[c] = (prefix[hi][c] - prefix[lo][c]) * inv;
        }
    }
}

fn gauss_pass(src: &[[f32; 4]], dst: &mut [[f32; 4]], sigma: f32) {
    let r = (sigma * 3.0).ceil() as isize;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    let n = src.len() as isize;
    for (i, o) in dst.iter_mut().enumerate() {
        let mut acc = [0.0f32; 4];
        for (j, w) in k.iter().enumerate() {
            let s = i as isize + j as isize - r;
            if (0..n).contains(&s) {
                for c in 0..4 {
                    acc[c] += src[s as usize][c] * w;
                }
            }
        }
        *o = acc;
    }
}

// ── поэлементные примитивы ─────────────────────────────────────────────────

fn color_matrix(src: &Img, m: &FeColorMatrix) -> Img {
    let mat: [f32; 20] = match m {
        FeColorMatrix::Matrix(v) => *v,
        FeColorMatrix::Saturate(s) => [
            0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s, 0.0, 0.0,
            0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s, 0.0, 0.0,
            0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ],
        FeColorMatrix::HueRotate(deg) => {
            let (sin, cos) = deg.to_radians().sin_cos();
            [
                0.213 + cos * 0.787 - sin * 0.213, 0.715 - cos * 0.715 - sin * 0.715, 0.072 - cos * 0.072 + sin * 0.928, 0.0, 0.0,
                0.213 - cos * 0.213 + sin * 0.143, 0.715 + cos * 0.285 + sin * 0.140, 0.072 - cos * 0.072 - sin * 0.283, 0.0, 0.0,
                0.213 - cos * 0.213 - sin * 0.787, 0.715 - cos * 0.715 + sin * 0.715, 0.072 + cos * 0.928 + sin * 0.072, 0.0, 0.0,
                0.0, 0.0, 0.0, 1.0, 0.0,
            ]
        }
        FeColorMatrix::LuminanceToAlpha => [
            0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0,
            0.2125, 0.7154, 0.0721, 0.0, 0.0,
        ],
    };
    let mut out = Img::new(src.w, src.h);
    for (o, p) in out.d.iter_mut().zip(&src.d) {
        let a = p[3];
        let (r, g, b) = if a > 0.0 { (p[0] / a, p[1] / a, p[2] / a) } else { (0.0, 0.0, 0.0) };
        let row = |i: usize| {
            (mat[i * 5] * r + mat[i * 5 + 1] * g + mat[i * 5 + 2] * b + mat[i * 5 + 3] * a + mat[i * 5 + 4])
                .clamp(0.0, 1.0)
        };
        let na = row(3);
        *o = [row(0) * na, row(1) * na, row(2) * na, na];
    }
    out
}

fn transfer(f: &FeTransfer, c: f32) -> f32 {
    match f {
        FeTransfer::Identity => c,
        FeTransfer::Table(v) => {
            let n = v.len() - 1;
            if n == 0 {
                return v[0];
            }
            let k = ((c * n as f32).floor() as usize).min(n - 1);
            v[k] + (c - k as f32 / n as f32) * n as f32 * (v[k + 1] - v[k])
        }
        FeTransfer::Discrete(v) => {
            let n = v.len();
            v[((c * n as f32).floor() as usize).min(n - 1)]
        }
        FeTransfer::Linear { slope, intercept } => slope * c + intercept,
        FeTransfer::Gamma { amplitude, exponent, offset } => amplitude * c.powf(*exponent) + offset,
    }
}

fn component_transfer(src: &Img, funcs: &[FeTransfer; 4]) -> Img {
    let mut out = Img::new(src.w, src.h);
    for (o, p) in out.d.iter_mut().zip(&src.d) {
        let a = p[3];
        let (r, g, b) = if a > 0.0 { (p[0] / a, p[1] / a, p[2] / a) } else { (0.0, 0.0, 0.0) };
        let na = transfer(&funcs[3], a).clamp(0.0, 1.0);
        let ch = |i: usize, c: f32| transfer(&funcs[i], c).clamp(0.0, 1.0) * na;
        *o = [ch(0, r), ch(1, g), ch(2, b), na];
    }
    out
}

fn composite(a: &Img, b: &Img, op: FeCompositeOp) -> Img {
    let mut out = Img::new(a.w, a.h);
    for ((o, pa), pb) in out.d.iter_mut().zip(&a.d).zip(&b.d) {
        let (aa, ab) = (pa[3], pb[3]);
        match op {
            FeCompositeOp::Arithmetic([k1, k2, k3, k4]) => {
                let mut v = [0.0f32; 4];
                for c in 0..4 {
                    v[c] = (k1 * pa[c] * pb[c] + k2 * pa[c] + k3 * pb[c] + k4).clamp(0.0, 1.0);
                }
                for c in 0..3 {
                    v[c] = v[c].min(v[3]);
                }
                *o = v;
            }
            _ => {
                let (fa, fb) = match op {
                    FeCompositeOp::Over => (1.0, 1.0 - aa),
                    FeCompositeOp::In => (ab, 0.0),
                    FeCompositeOp::Out => (1.0 - ab, 0.0),
                    FeCompositeOp::Atop => (ab, 1.0 - aa),
                    FeCompositeOp::Xor => (1.0 - ab, 1.0 - aa),
                    FeCompositeOp::Arithmetic(_) => unreachable!(),
                };
                for c in 0..4 {
                    o[c] = pa[c] * fa + pb[c] * fb;
                }
            }
        }
    }
    out
}

fn blend(top: &Img, back: &Img, mode: FeBlendMode) -> Img {
    let bm = match mode {
        FeBlendMode::Normal => BlendMode::Normal,
        FeBlendMode::Multiply => BlendMode::Multiply,
        FeBlendMode::Screen => BlendMode::Screen,
        FeBlendMode::Darken => BlendMode::Darken,
        FeBlendMode::Lighten => BlendMode::Lighten,
        FeBlendMode::Overlay => BlendMode::Overlay,
        FeBlendMode::ColorDodge => BlendMode::ColorDodge,
        FeBlendMode::ColorBurn => BlendMode::ColorBurn,
        FeBlendMode::HardLight => BlendMode::HardLight,
        FeBlendMode::SoftLight => BlendMode::SoftLight,
        FeBlendMode::Difference => BlendMode::Difference,
        FeBlendMode::Exclusion => BlendMode::Exclusion,
        FeBlendMode::Hue => BlendMode::Hue,
        FeBlendMode::Saturation => BlendMode::Saturation,
        FeBlendMode::Color => BlendMode::Color,
        FeBlendMode::Luminosity => BlendMode::Luminosity,
    };
    let unpre = |p: &[f32; 4]| {
        let a = p[3];
        if a > 0.0 { [p[0] / a, p[1] / a, p[2] / a, a] } else { [0.0; 4] }
    };
    let mut out = Img::new(top.w, top.h);
    for ((o, pt), pb) in out.d.iter_mut().zip(&top.d).zip(&back.d) {
        let r = crate::blend_modes::mix_blend_rgba(bm, unpre(pt), unpre(pb));
        *o = [r[0] * r[3], r[1] * r[3], r[2] * r[3], r[3]];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_layout::{Color, SvgFilterPrimitive};

    fn prim(kind: FePrimitive) -> SvgFilterPrimitive {
        SvgFilterPrimitive { kind, x: None, y: None, width: None, height: None, linear_rgb: false, tainted: false }
    }

    fn url(graph: SvgFilter) -> FilterUrl {
        FilterUrl { id: "f".into(), graph: Some(std::sync::Arc::new(graph)), placement: None }
    }

    fn red_square() -> tiny_skia::Pixmap {
        let mut pm = tiny_skia::Pixmap::new(40, 40).unwrap();
        for y in 10..20 {
            for x in 10..20 {
                let i = (y * 40 + x) * 4;
                pm.data_mut()[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        pm
    }

    fn px(w: &Windowed, x: i32, y: i32) -> [u8; 4] {
        let (lx, ly) = ((x - w.x) as usize, (y - w.y) as usize);
        let i = (ly * w.pixmap.width() as usize + lx) * 4;
        w.pixmap.data()[i..i + 4].try_into().unwrap()
    }

    fn filter(prims: Vec<SvgFilterPrimitive>) -> SvgFilter {
        SvgFilter {
            filter_units_obb: true,
            primitive_units_obb: false,
            region: (-0.5, -0.5, 2.0, 2.0),
            primitives: prims,
        }
    }

    #[test]
    fn flood_fills_filter_region_only() {
        let g = filter(vec![prim(FePrimitive::Flood { color: Color { r: 0, g: 0, b: 255, a: 255 }, opacity: 1.0 })]);
        let w = apply(&red_square(), (0, 0), (40, 40), &url(g), Rect::new(10.0, 10.0, 10.0, 10.0)).unwrap();
        // Область: x ∈ [5, 25).
        assert_eq!(px(&w, 12, 12), [0, 0, 255, 255]);
        assert_eq!(px(&w, 5, 5), [0, 0, 255, 255]);
        assert_eq!(w.x, 5);
        assert_eq!(w.pixmap.width(), 20);
    }

    #[test]
    fn offset_moves_source() {
        let g = filter(vec![prim(FePrimitive::Offset { input: FeInput::SourceGraphic, dx: 5.0, dy: 0.0 })]);
        let w = apply(&red_square(), (0, 0), (40, 40), &url(g), Rect::new(10.0, 10.0, 10.0, 10.0)).unwrap();
        assert_eq!(px(&w, 12, 12), [0, 0, 0, 0]);
        assert_eq!(px(&w, 17, 12), [255, 0, 0, 255]);
        assert_eq!(px(&w, 22, 12), [255, 0, 0, 255]);
        assert_eq!(px(&w, 24, 12), [255, 0, 0, 255]);
    }

    #[test]
    fn blur_spreads_alpha_and_conserves_mass() {
        let g = filter(vec![prim(FePrimitive::GaussianBlur { input: FeInput::SourceGraphic, sx: 3.0, sy: 3.0 })]);
        let w = apply(&red_square(), (0, 0), (40, 40), &url(g), Rect::new(10.0, 10.0, 10.0, 10.0)).unwrap();
        let total: f32 = w.pixmap.data().chunks(4).map(|p| f32::from(p[3]) / 255.0).sum();
        // Часть массы уходит за край области фильтра (5 px запаса при σ = 3).
        assert!(total > 90.0 && total <= 100.5, "mass {total}");
        assert!(px(&w, 9, 15)[3] > 0 && px(&w, 15, 15)[3] < 255);
    }

    #[test]
    fn color_matrix_luminance_to_alpha_and_composite_in() {
        let g = filter(vec![
            prim(FePrimitive::Flood { color: Color { r: 0, g: 255, b: 0, a: 255 }, opacity: 1.0 }),
            prim(FePrimitive::Composite { a: FeInput::Previous, b: FeInput::SourceGraphic, op: FeCompositeOp::In }),
        ]);
        let w = apply(&red_square(), (0, 0), (40, 40), &url(g), Rect::new(10.0, 10.0, 10.0, 10.0)).unwrap();
        assert_eq!(px(&w, 12, 12), [0, 255, 0, 255]);
        assert_eq!(px(&w, 7, 7), [0, 0, 0, 0]);
    }

    #[test]
    fn missing_graph_is_none() {
        let u = FilterUrl { id: "x".into(), graph: None, placement: None };
        assert!(apply(&red_square(), (0, 0), (40, 40), &u, Rect::new(0.0, 0.0, 1.0, 1.0)).is_none());
    }

    fn run(prims: Vec<SvgFilterPrimitive>) -> Windowed {
        apply(&red_square(), (0, 0), (40, 40), &url(filter(prims)), Rect::new(10.0, 10.0, 10.0, 10.0)).unwrap()
    }

    fn flood(color: Color) -> SvgFilterPrimitive {
        prim(FePrimitive::Flood { color, opacity: 1.0 })
    }

    #[test]
    fn tile_repeats_input_subregion_over_whole_region() {
        let mut seed = flood(Color { r: 0, g: 0, b: 255, a: 255 });
        seed.x = Some(0.0);
        seed.y = Some(0.0);
        seed.width = Some(5.0);
        seed.height = Some(5.0);
        let w = run(vec![seed, prim(FePrimitive::Tile { input: FeInput::Previous })]);
        assert_eq!(px(&w, 22, 22), [0, 0, 255, 255]);
        assert_eq!(px(&w, 6, 6), [0, 0, 255, 255]);
    }

    #[test]
    fn displacement_map_shifts_by_channel() {
        // G = 1.0 → сдвиг +scale/2 по X; B = 0.5 → по Y нет сдвига.
        let map = flood(Color { r: 0, g: 255, b: 128, a: 255 });
        let disp = prim(FePrimitive::DisplacementMap {
            input: FeInput::SourceGraphic,
            map: FeInput::Previous,
            scale: 10.0,
            x_channel: 1,
            y_channel: 2,
        });
        let w = run(vec![map, disp]);
        assert_eq!(px(&w, 7, 12), [255, 0, 0, 255]);
        assert_eq!(px(&w, 16, 12), [0, 0, 0, 0]);
    }

    #[test]
    fn morphology_dilates_and_erodes() {
        let morph = |dilate| {
            prim(FePrimitive::Morphology { input: FeInput::SourceGraphic, dilate, rx: 2.0, ry: 2.0 })
        };
        let d = run(vec![morph(true)]);
        assert_eq!(px(&d, 8, 12), [255, 0, 0, 255]);
        assert_eq!(px(&d, 7, 12)[3], 0);
        let e = run(vec![morph(false)]);
        assert_eq!(px(&e, 11, 12)[3], 0);
        assert_eq!(px(&e, 13, 12), [255, 0, 0, 255]);
    }

    #[test]
    fn convolve_identity_kernel_keeps_source_and_bad_kernel_is_transparent() {
        let conv = |kernel: Vec<f32>| {
            prim(FePrimitive::ConvolveMatrix {
                input: FeInput::SourceGraphic,
                order: (3, 3),
                kernel,
                divisor: 1.0,
                bias: 0.0,
                target: (1, 1),
                edge: lumen_layout::FeEdgeMode::Duplicate,
                preserve_alpha: false,
            })
        };
        let w = run(vec![conv(vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0])]);
        assert_eq!(px(&w, 12, 12), [255, 0, 0, 255]);
        assert_eq!(px(&w, 9, 12)[3], 0);
        let w = run(vec![conv(Vec::new())]);
        assert_eq!(px(&w, 12, 12), [0, 0, 0, 0]);
    }

    #[test]
    fn diffuse_lighting_overhead_light_on_flat_surface_is_full_colour() {
        let lit = prim(FePrimitive::Lighting {
            input: FeInput::SourceGraphic,
            kind: lumen_layout::FeLighting::Diffuse { kd: 1.0 },
            surface_scale: 1.0,
            color: Color { r: 0, g: 255, b: 0, a: 255 },
            light: Some(FeLight::Distant { azimuth: 0.0, elevation: 90.0 }),
        });
        let w = run(vec![lit]);
        assert_eq!(px(&w, 15, 15), [0, 255, 0, 255]);
        let dark = prim(FePrimitive::Lighting {
            input: FeInput::SourceGraphic,
            kind: lumen_layout::FeLighting::Diffuse { kd: 1.0 },
            surface_scale: 1.0,
            color: Color { r: 0, g: 255, b: 0, a: 255 },
            light: None,
        });
        assert_eq!(px(&run(vec![dark]), 15, 15), [0, 0, 0, 0]);
    }

    #[test]
    fn turbulence_is_deterministic_and_varies() {
        let t = |seed| {
            prim(FePrimitive::Turbulence { fx: 0.1, fy: 0.1, octaves: 2, seed, stitch: false, fractal: true })
        };
        let a = run(vec![t(1)]);
        let b = run(vec![t(1)]);
        let c = run(vec![t(2)]);
        assert_eq!(a.pixmap.data(), b.pixmap.data());
        assert_ne!(a.pixmap.data(), c.pixmap.data());
        assert_ne!(px(&a, 6, 6), px(&a, 17, 11));
    }

    #[test]
    fn linear_rgb_round_trip_is_identity_for_merge_of_source() {
        let mut p = prim(FePrimitive::Merge(vec![FeInput::SourceGraphic]));
        p.linear_rgb = true;
        let w = apply(&red_square(), (0, 0), (40, 40), &url(filter(vec![p])), Rect::new(10.0, 10.0, 10.0, 10.0)).unwrap();
        assert_eq!(px(&w, 12, 12), [255, 0, 0, 255]);
    }
}

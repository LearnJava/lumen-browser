//! Glyph rasterizer: outline → grayscale bitmap.
//!
//! Путь:
//! 1. Контур обходится с учётом on-curve / off-curve флагов; квадратичные
//!    Безье разворачиваются в 8 коротких отрезков.
//! 2. Bitmap размером bbox + 1px padding на сторону.
//! 3. Покрытие считается по сетке 4×4 сэмпла на пиксель с even-odd правилом
//!    (как в SVG/PDF при отсутствии fill-rule) — сканлайнами с активным
//!    списком рёбер: рёбра раскладываются по сэмпл-строке входа счётной
//!    сортировкой (O(рёбра + height · 4)), дальше на каждую сэмпл-строку
//!    O(active · log active) на её пересечения и по одному спану на интервал
//!    «внутри». Покрытие → 8-битный grayscale.
//!
//! Замены / оптимизации в дальнейшем:
//! - адаптивная subdivision Безье (сейчас фиксированные 8 шагов);
//! - SDF-подход для масштабируемого рендера.

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

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

        // BUG-423: заголовочный bbox бывает вывернутым (`x_max < x_min`) —
        // такой встречается у composite-глифов реальных шрифтов. Раньше это
        // давало отрицательную ширину и `None`, то есть букву без чернил при
        // верном advance. Считаем бокс по точкам: для квадратичных Безье он
        // надмножество истинного, обрезать глиф не может.
        let bbox = if glyph.bbox.is_inverted() {
            crate::glyf::bbox_from_contours(contours)?
        } else {
            glyph.bbox
        };

        let x_min = (bbox.x_min as f32 * scale - pad).floor() as i32;
        let y_min = (bbox.y_min as f32 * scale - pad).floor() as i32;
        let x_max = (bbox.x_max as f32 * scale + pad).ceil() as i32;
        let y_max = (bbox.y_max as f32 * scale + pad).ceil() as i32;
        // BUG-283: a corrupt/inverted glyph bbox (x_max < x_min or y_max <
        // y_min) makes `x_max - x_min` negative; casting that straight to
        // `u32` wraps it near `u32::MAX` and the pixel buffer allocation
        // below aborts the process. An upper bound also guards against a
        // valid but extreme bbox/pixel_size combination requesting a
        // runaway allocation — no legitimate glyph bitmap approaches it.
        const MAX_GLYPH_DIM: i32 = 8192;
        let width_i32 = x_max - x_min;
        let height_i32 = y_max - y_min;
        if width_i32 <= 0
            || height_i32 <= 0
            || width_i32 > MAX_GLYPH_DIM
            || height_i32 > MAX_GLYPH_DIM
        {
            return None;
        }
        let width = width_i32 as u32;
        let height = height_i32 as u32;

        let mut pixels = vec![0u8; (width as usize) * (height as usize)];
        SCRATCH.with_borrow_mut(|sc| {
            // `edges` живёт в том же scratch, что и остальные буферы, поэтому
            // на время заполнения вынимается из него (иначе `&sc.edges` и
            // `&mut sc` пересекаются).
            let mut edges = std::mem::take(&mut sc.edges);
            edges.clear();
            for contour in contours {
                walk_contour(contour, scale, x_min as f32, y_max as f32, &mut edges);
            }
            fill_pixels_in(sc, &edges, width, height, &mut pixels);
            sc.edges = edges;
        });
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

/// Число сэмплов на сторону пикселя (сетка N×N). Значение — часть контракта
/// покрытия: изменение сдвинуло бы все эталоны текста.
const N: u32 = 4;

/// Индекс первого сэмпла строки, чей `x` не меньше `a`.
///
/// Сэмплы стоят в точках `x(s) = px + (sx + 0.5)/N`, где `s = px·N + sx`. При
/// `N = 4` дробная часть — одно из `0.125/0.375/0.625/0.875`, то есть точное
/// двоичное число, а `px` — целое; сумма точна, и она же равна `(s + 0.5)/N`.
/// Поэтому предикат `x(s) >= a` из посэмплового прохода решается в f64 без
/// потери: `s >= N·a − 0.5`, а `f64` вмещает и `a`, и произведение точно.
///
/// `as i64` в Rust насыщает, поэтому `±inf` (пересечение вырожденного ребра)
/// даёт границу за пределами строки, а не UB.
fn first_sample_at_or_after(a: f32) -> i64 {
    (f64::from(N) * f64::from(a) - 0.5).ceil() as i64
}

/// Прибавить покрытие сэмплов `[s0, s1)` к строке пикселей: краевые пиксели —
/// напрямую в `cov`, целиком накрытая середина — разностной записью в `runs`
/// (её префиксная сумма снимается один раз на пиксельную строку).
///
/// Разностный массив и есть причина, по которой цена спана постоянна: без
/// него середина стоила бы по итерации на пиксель на каждой из `N` сэмпл-строк.
fn add_span(s0: i64, s1: i64, sample_max: i64, cov: &mut [u32], runs: &mut [i32]) {
    let n = i64::from(N);
    let s0 = s0.max(0);
    let s1 = s1.min(sample_max);
    if s0 >= s1 {
        return;
    }
    let p0 = (s0 / n) as usize;
    let p1 = ((s1 - 1) / n) as usize;
    if p0 == p1 {
        cov[p0] += (s1 - s0) as u32;
        return;
    }
    cov[p0] += (n - s0 % n) as u32;
    cov[p1] += ((s1 - 1) % n + 1) as u32;
    if p1 > p0 + 1 {
        runs[p0 + 1] += N as i32;
        runs[p1] -= N as i32;
    }
}

/// [`fill_pixels_in`] на своих буферах — вход гейтов, которым нужен
/// произвольный набор рёбер, а не глиф.
#[cfg(test)]
fn fill_pixels(edges: &[Edge], width: u32, height: u32, pixels: &mut [u8]) {
    let mut scratch = Scratch::default();
    fill_pixels_in(&mut scratch, edges, width, height, pixels);
}

/// Рабочие буферы растеризации одного глифа. Все они одноразовые по смыслу —
/// заполняются и выбрасываются, — но глифов на кадре сотни, поэтому живут в
/// [`SCRATCH`] и переиспользуются: аллокация каждого стоила 88 нс, то есть
/// 0.27 мс на корпус (BUG-405 срез 17).
#[derive(Default)]
struct Scratch {
    /// Рёбра контуров глифа в pixel space (заполняет `walk_contour`).
    edges: Vec<Edge>,
    /// Нормализованные рёбра вместе с индексом их сэмпл-строки входа —
    /// промежуточный вид перед раскладкой по строкам.
    keyed: Vec<(u32, Edge)>,
    /// Те же рёбра, разложенные по сэмпл-строке входа (счётная сортировка).
    rowed: Vec<Edge>,
    /// Конец диапазона рёбер каждой сэмпл-строки в `rowed`.
    row_end: Vec<u32>,
    /// Рёбра, пересекающие текущую сэмпл-строку.
    active: Vec<Edge>,
    /// x-пересечения текущей сэмпл-строки.
    xs: Vec<f32>,
    /// Покрытие пикселей текущей строки в сэмплах.
    cov: Vec<u32>,
    /// Разностная запись целиком накрытых пикселей (см. `add_span`).
    runs: Vec<i32>,
}

thread_local! {
    /// Буферы переиспользуются между глифами ОДНОГО потока. Результат от
    /// этого не зависит ни в чём: каждый вход [`fill_pixels_in`] очищает всё,
    /// что читает, — общий буфер здесь только способ не звать аллокатор
    /// десять раз на глиф.
    static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::new(Scratch::default());
}

/// Заполняет покрытие сканлайнами: на каждую сэмпл-строку считается список
/// x-пересечений активных рёбер, а не ray-cast из каждого сэмпла по всем
/// рёбрам (BUG-405 срез 3). Сами сэмплы строки не обходятся поштучно —
/// отсортированные пересечения сразу дают интервалы «внутри», и покрытие
/// кладётся спанами (срез 15).
///
/// Результат **побитово** тот же, что у прежнего перебора
/// (`fill_pixels_reference` в тестах): те же выражения пересечения, то же
/// half-open правило `[min_y, max_y)` и тот же критерий «пересечение справа»
/// (`xint > x`) — сэмпл внутри, когда число пересечений строго правее нечётно,
/// а это чётность `len - k`, где `k` — сколько пересечений ≤ x. Сортировка не
/// меняет сами значения, поэтому чётность сохраняется.
///
/// Интервалы выводятся из той же чётности: после `j`-го пересечения курсор
/// `k = j + 1`, значит сэмпл внутри, когда `len − j − 1` нечётно. Отсюда
/// интервалы `[xs[j], xs[j+1])` по таким `j`, плюс начальный `(−∞, xs[0])`
/// при нечётном `len` (вырожденный контур — прежний код и там считал левые
/// сэмплы внутренними).
fn fill_pixels_in(sc: &mut Scratch, edges: &[Edge], width: u32, height: u32, pixels: &mut [u8]) {
    let total = N * N;
    let rows = (height as usize) * (N as usize);

    // Рёбра в нормальном виде (ay < by) — прежний код выполнял этот разворот
    // и отсев горизонтальных внутри самого внутреннего цикла, то есть
    // width·height·16 раз на ребро.
    //
    // Порядок ввода рёбер в активный список — по сэмпл-строке, на которой
    // ребро впервые становится активным. Прежде его давала сортировка по
    // верхней координате (1.57 мс из 6.70 мс `fill_pixels` на корпусе,
    // BUG-405 срез 17), но сама координата дальше не нужна: нужен только
    // индекс строки входа, а он целый и ограничен высотой bitmap-а, поэтому
    // рёбра раскладываются счётной сортировкой по нему. От порядка рёбер
    // внутри строки результат не зависит — набор пересечений строки тот же,
    // а `xs` всё равно сортируется.
    sc.keyed.clear();
    sc.row_end.clear();
    sc.row_end.resize(rows, 0);
    for &(x1, y1, x2, y2) in edges {
        let e = if y1 <= y2 { (x1, y1, x2, y2) } else { (x2, y2, x1, y1) };
        // dy == 0 (в т.ч. NaN-координата) не даёт пересечений ни на одной
        // строке: прежний код отбрасывал такое ребро проверкой `dy == 0.0`
        // либо получал NaN-`xint`, который не проходил `xint > px`.
        if e.3 - e.1 > 0.0 {
            // `first_sample_at_or_after` решает тот же предикат `y(s) >= ay`,
            // что прежний проход решал сравнением `norm[next].1 <= y` — точно,
            // см. её док. Ребро с индексом за последней строкой не
            // активировалось бы никогда: прежний цикл до него не доходил.
            let row = first_sample_at_or_after(e.1).clamp(0, rows as i64) as usize;
            if row < rows {
                sc.row_end[row] += 1;
                sc.keyed.push((row as u32, e));
            }
        }
    }
    // Префиксная сумма (исключающая): `row_end[r]` временно держит начало
    // диапазона строки `r`, а раскладка ниже доводит его до конца диапазона.
    let mut acc = 0_u32;
    for end in sc.row_end.iter_mut() {
        let count = *end;
        *end = acc;
        acc += count;
    }
    sc.rowed.clear();
    sc.rowed.resize(acc as usize, (0.0, 0.0, 0.0, 0.0));
    for &(row, e) in &sc.keyed {
        let slot = &mut sc.row_end[row as usize];
        sc.rowed[*slot as usize] = e;
        *slot += 1;
    }

    sc.active.clear();
    sc.cov.clear();
    sc.cov.resize(width as usize, 0);
    sc.runs.clear();
    sc.runs.resize(width as usize, 0);
    let sample_max = i64::from(width) * i64::from(N);

    // Сэмпл-строки идут строго по возрастанию y — это и позволяет вести
    // активный список одним проходом.
    let mut row_start = 0_u32;
    let mut s = 0_usize;
    for py in 0..height {
        sc.cov.fill(0);
        sc.runs.fill(0);
        for sy in 0..N {
            let y = py as f32 + (sy as f32 + 0.5) / N as f32;
            let row_end = sc.row_end[s];
            if row_start != row_end {
                sc.active
                    .extend_from_slice(&sc.rowed[row_start as usize..row_end as usize]);
                row_start = row_end;
            }
            s += 1;
            sc.active.retain(|e| e.3 > y);
            if sc.active.is_empty() {
                continue;
            }
            sc.xs.clear();
            for &(ax, ay, bx, by) in &sc.active {
                let t = (y - ay) / (by - ay);
                let xint = ax + t * (bx - ax);
                if !xint.is_nan() {
                    sc.xs.push(xint);
                }
            }
            if sc.xs.is_empty() {
                continue;
            }
            sc.xs.sort_unstable_by(f32::total_cmp);
            // Интервалы «внутри» между пересечениями — вывод той же чётности,
            // что вёл посэмпловый курсор (см. док функции).
            let len = sc.xs.len();
            let mut j = if len & 1 == 1 {
                add_span(
                    i64::MIN,
                    first_sample_at_or_after(sc.xs[0]),
                    sample_max,
                    &mut sc.cov,
                    &mut sc.runs,
                );
                1
            } else {
                0
            };
            while j + 1 < len {
                add_span(
                    first_sample_at_or_after(sc.xs[j]),
                    first_sample_at_or_after(sc.xs[j + 1]),
                    sample_max,
                    &mut sc.cov,
                    &mut sc.runs,
                );
                j += 2;
            }
        }
        let row = py as usize * width as usize;
        let mut run = 0_i32;
        for (px, &c) in sc.cov.iter().enumerate() {
            run += sc.runs[px];
            debug_assert!(run >= 0, "разностная запись спанов ушла в минус");
            let c = c + run as u32;
            pixels[row + px] = (c * 255 / total) as u8;
        }
    }
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

    /// Прежний (до BUG-405 срез 3) перебор: ray-cast из каждого сэмпла по
    /// ВСЕМ рёбрам. Оставлен эталоном идентичности — сканлайн обязан давать
    /// побитово тот же bitmap, иначе поедут все эталоны текста.
    fn fill_pixels_reference(edges: &[Edge], width: u32, height: u32, pixels: &mut [u8]) {
        const N: u32 = 4;
        let total = N * N;
        for py in 0..height {
            for px in 0..width {
                let mut inside = 0u32;
                for sy in 0..N {
                    let y = py as f32 + (sy as f32 + 0.5) / N as f32;
                    for sx in 0..N {
                        let x = px as f32 + (sx as f32 + 0.5) / N as f32;
                        if point_inside_reference(edges, x, y) {
                            inside += 1;
                        }
                    }
                }
                pixels[(py * width + px) as usize] = (inside * 255 / total) as u8;
            }
        }
    }

    /// Ray-casting от точки (px, py) вправо, half-open правило `[min_y, max_y)`.
    /// Чётное число пересечений — снаружи, нечётное — внутри.
    fn point_inside_reference(edges: &[Edge], px: f32, py: f32) -> bool {
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

    /// Растеризует глиф прежним перебором — по тем же рёбрам и той же
    /// геометрии bitmap-а, что и `Rasterizer::rasterize`.
    fn rasterize_reference(r: &Rasterizer, glyph: &Glyph) -> Option<Bitmap> {
        let mut bm = r.rasterize(glyph)?;
        let scale = r.scale();
        let Outline::Simple(contours) = &glyph.outline else {
            return None;
        };
        let mut edges: Vec<Edge> = Vec::new();
        for contour in contours {
            walk_contour(contour, scale, bm.left, bm.top, &mut edges);
        }
        let mut pixels = vec![0u8; (bm.width as usize) * (bm.height as usize)];
        fill_pixels_reference(&edges, bm.width, bm.height, &mut pixels);
        bm.pixels = pixels;
        Some(bm)
    }

    fn assets_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("assets")
            .join("fonts")
    }

    /// Гейт среза 3 BUG-405: сканлайн-заполнение обязано быть побитово
    /// равно прежнему перебору. Проверяется на реальном шрифте (Inter) —
    /// синтетические треугольники не содержат ни кривых, ни дыр, ни
    /// почти-горизонтальных рёбер, где расходится обработка краёв.
    #[test]
    fn scanline_fill_matches_bruteforce_on_real_font() {
        let bytes = std::fs::read(assets_dir().join("Inter-Regular.ttf"))
            .expect("assets/fonts/Inter-Regular.ttf");
        let font = crate::Font::parse(&bytes).expect("Inter parses");
        let upem = font.head().expect("head").units_per_em;
        let mut compared = 0;
        for size in [11.0_f32, 13.0, 16.0, 24.0, 48.0] {
            let r = Rasterizer::new(size, upem);
            for gid in 1_u16..200 {
                let Ok(Some(glyph)) = font.glyph_resolved(gid) else {
                    continue;
                };
                if !matches!(glyph.outline, Outline::Simple(_)) {
                    continue;
                }
                let (Some(got), Some(want)) = (r.rasterize(&glyph), rasterize_reference(&r, &glyph))
                else {
                    continue;
                };
                assert_eq!(
                    (got.width, got.height),
                    (want.width, want.height),
                    "glyph {gid} @ {size}px: размер bitmap-а"
                );
                assert_eq!(
                    got.pixels, want.pixels,
                    "glyph {gid} @ {size}px: покрытие разошлось со старым перебором"
                );
                compared += 1;
            }
        }
        assert!(compared > 400, "сравнено слишком мало глифов: {compared}");
    }

    /// Гейт среза 15: заливка спанами обязана совпасть с перебором и на
    /// НЕЧЁТНОМ числе пересечений в строке.
    ///
    /// Замкнутый контур реального шрифта даёт чётное число всегда, поэтому
    /// гейт выше эту ветку не задействует ни разу — а именно в ней спан
    /// начинается не с пересечения, а от левого края (курсор `k = 0` при
    /// нечётной длине уже «внутри»). Рёбра здесь задаются напрямую, потому что
    /// из точек глифа (координаты `i16`, контур замыкается обходом) такой
    /// набор не построить.
    #[test]
    fn span_fill_matches_bruteforce_on_odd_crossing_rows() {
        // (рёбра, ширина, высота) — во всех наборах число пересечений строки
        // нечётно, то есть ветка «слева от первого пересечения — внутри»
        // исполняется на каждой сэмпл-строке.
        let cases: Vec<(Vec<Edge>, u32, u32)> = vec![
            // Одно ребро через всю высоту: слева от него — «внутри».
            (vec![(12.0, -1.0, 8.0, 11.0)], 20, 10),
            // Три ребра: спаны и слева от первого, и между вторым и третьим.
            (
                vec![(2.0, -1.0, 3.0, 11.0), (9.0, -1.0, 9.5, 11.0), (15.0, -1.0, 14.0, 11.0)],
                20,
                10,
            ),
            // Первое пересечение левее bitmap-а — начальный спан обрезается.
            (vec![(-8.0, -1.0, -6.0, 11.0)], 12, 6),
            // Пересечение правее bitmap-а — залита вся строка.
            (vec![(40.0, -1.0, 44.0, 11.0)], 12, 6),
            // NaN-координата: ребро отбрасывается обеими реализациями, но
            // остаток набора остаётся нечётным.
            (
                vec![(f32::NAN, -1.0, f32::NAN, 11.0), (7.0, -1.0, 7.0, 11.0)],
                14,
                8,
            ),
        ];
        for (i, (edges, w, h)) in cases.iter().enumerate() {
            let mut got = vec![0u8; (*w as usize) * (*h as usize)];
            let mut want = vec![0u8; (*w as usize) * (*h as usize)];
            fill_pixels(edges, *w, *h, &mut got);
            fill_pixels_reference(edges, *w, *h, &mut want);
            assert_eq!(got, want, "набор {i}: спаны разошлись с перебором");
        }
        // Утверждение о задействовании: первый набор обязан дать чернила
        // слева от ребра — иначе «совпало» означало бы «оба пусты».
        let mut ink = vec![0u8; 20 * 10];
        fill_pixels(&[(12.0, -1.0, 8.0, 11.0)], 20, 10, &mut ink);
        assert!(ink[0] > 0, "левый край строки обязан быть внутри");
        assert_eq!(ink[19], 0, "правый край строки обязан быть снаружи");
    }

    /// Тот же гейт на псевдослучайных наборах рёбер: спан может обрезаться с
    /// обеих сторон, кончаться внутри пикселя, совпадать с соседним. Перебор
    /// эталон, генератор детерминированный (свой LCG, без зависимостей).
    #[test]
    fn span_fill_matches_bruteforce_on_random_edges() {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut rnd = |lo: f32, hi: f32| lo + (next() >> 40) as f32 / 16_777_216.0 * (hi - lo);
        for case in 0..300 {
            let w = 1 + (case % 17) as u32;
            let h = 1 + (case % 11) as u32;
            let n = 1 + case % 9;
            let edges: Vec<Edge> = (0..n)
                .map(|_| {
                    (
                        rnd(-4.0, w as f32 + 4.0),
                        rnd(-4.0, h as f32 + 4.0),
                        rnd(-4.0, w as f32 + 4.0),
                        rnd(-4.0, h as f32 + 4.0),
                    )
                })
                .collect();
            let mut got = vec![0u8; (w as usize) * (h as usize)];
            let mut want = vec![0u8; (w as usize) * (h as usize)];
            fill_pixels(&edges, w, h, &mut got);
            fill_pixels_reference(&edges, w, h, &mut want);
            assert_eq!(got, want, "случай {case}: {edges:?}");
        }
    }

    /// Гейт среза 17: рёбра ЗА пределами сетки сэмпл-строк.
    ///
    /// Счётная раскладка кладёт ребро в строку его входа; у ребра выше
    /// bitmap-а индекс отрицателен (прижимается к нулю), у ребра ниже
    /// последней сэмпл-строки — за концом массива, и такое ребро выбрасывается
    /// (прежний проход до него просто не доходил). Оба случая реальный шрифт
    /// не даёт: контур лежит внутри bbox с padding-ом, — поэтому вход
    /// рукотворный.
    #[test]
    fn counting_order_handles_edges_outside_the_sample_grid() {
        // Пара рёбер поперёк всей высоты (входят выше нулевой сэмпл-строки —
        // индекс входа отрицателен) даёт чернила в полосе [4, 8). Третье ребро
        // целиком ниже последней сэмпл-строки (h = 6, последняя строка
        // y = 5.875): его нельзя ни активировать, ни прижать к последней
        // строке — прижатое, оно дало бы пересечение x = 6 ВНУТРИ полосы,
        // то есть лишнюю смену чётности.
        let edges: Vec<Edge> = vec![
            (4.0, -3.0, 4.0, 9.0),
            (8.0, -3.0, 8.0, 9.0),
            (6.0, 6.5, 6.0, 9.0),
        ];
        let (w, h) = (12_u32, 6_u32);
        let mut got = vec![0u8; (w * h) as usize];
        let mut want = vec![0u8; (w * h) as usize];
        fill_pixels(&edges, w, h, &mut got);
        fill_pixels_reference(&edges, w, h, &mut want);
        assert_eq!(got, want, "рёбра вне сетки разошлись с перебором");
        // Утверждение о задействовании: полоса между парой обязана быть
        // залита, иначе «совпало» означало бы «оба пусты».
        assert_eq!(coverage_at_slice(&got, w, 5, 3), 255, "полоса обязана быть залита");
        assert_eq!(coverage_at_slice(&got, w, 0, 3), 0, "вне полосы чернил нет");
    }

    fn coverage_at_slice(px: &[u8], width: u32, x: u32, y: u32) -> u8 {
        px[(y * width + x) as usize]
    }

    /// Гейт среза 17, второе плечо: рабочие буферы обязаны переживать глиф.
    ///
    /// Вывод от переиспользования не зависит вовсе, поэтому дифф-тесты выше
    /// проходят и с аллокацией на каждый глиф — механизм гейтится тем, что
    /// после первой растеризации буферы не пусты, а вторая их не перевыделяет.
    #[test]
    fn raster_scratch_survives_between_glyphs() {
        fn scratch_capacity() -> usize {
            SCRATCH.with_borrow(|s| {
                s.edges.capacity()
                    + s.keyed.capacity()
                    + s.rowed.capacity()
                    + s.row_end.capacity()
                    + s.active.capacity()
                    + s.xs.capacity()
                    + s.cov.capacity()
                    + s.runs.capacity()
            })
        }
        // Свой поток: `SCRATCH` — thread-local, а тесты крейта идут
        // параллельно и прогрели бы буферы соседним тестом.
        std::thread::spawn(|| {
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
            let r = Rasterizer::new(100.0, 100);
            assert_eq!(scratch_capacity(), 0, "холодный поток: буферы ещё не выделены");
            r.rasterize(&glyph).expect("треугольник растеризуется");
            let warm = scratch_capacity();
            assert!(warm > 0, "буферы обязаны пережить глиф, иначе аллокация на каждый");
            r.rasterize(&glyph).expect("треугольник растеризуется");
            assert_eq!(scratch_capacity(), warm, "тот же глиф не должен перевыделять буферы");
        })
        .join()
        .expect("поток гейта");
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

    /// BUG-283: инвертированный bbox (x_max < x_min) заворачивал `width` в
    /// число, близкое к `u32::MAX`, и валил процесс на
    /// `vec![0u8; width * height]`.
    ///
    /// BUG-423 уточнил ответ: вместо `None` (буква молча пропадала со
    /// страницы) бокс пересчитывается по точкам контура. Защита от
    /// переполнения сохранена — пересчитанный бокс ограничен координатами
    /// точек, а `MAX_GLYPH_DIM` по-прежнему режет экстремальные случаи.
    #[test]
    fn inverted_bbox_falls_back_to_bbox_from_points() {
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 100,
                y_min: 0,
                x_max: 0,
                y_max: 100,
            },
            outline: Outline::Simple(vec![Contour {
                points: vec![pt(0, 0, true), pt(100, 0, true), pt(50, 100, true)],
            }]),
        };
        let bm = Rasterizer::new(100.0, 100).rasterize(&glyph).expect("bbox по точкам");
        // Тот же размер, что у честного bbox (0,0)-(100,100) — см.
        // `rasterize_filled_triangle`.
        assert_eq!((bm.width, bm.height), (102, 102));
        assert!(coverage_at(&bm, 51, 60) > 200, "центр треугольника залит");
    }

    /// Тот же класс бага по оси Y (y_max < y_min).
    #[test]
    fn inverted_bbox_y_axis_falls_back_to_bbox_from_points() {
        let glyph = Glyph {
            bbox: BoundingBox {
                x_min: 0,
                y_min: 100,
                x_max: 100,
                y_max: 0,
            },
            outline: Outline::Simple(vec![Contour {
                points: vec![pt(0, 0, true), pt(100, 0, true), pt(50, 100, true)],
            }]),
        };
        let bm = Rasterizer::new(100.0, 100).rasterize(&glyph).expect("bbox по точкам");
        assert_eq!((bm.width, bm.height), (102, 102));
        assert!(coverage_at(&bm, 51, 60) > 200, "центр треугольника залит");
    }

    /// Корректный, но экстремальный bbox/pixel_size не должен запрашивать
    /// неограниченный буфер — верхняя граница возвращает `None`.
    #[test]
    fn oversized_bbox_returns_none_instead_of_huge_allocation() {
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
        // units_per_em=1000, pixel_size=100000 → scale 100 → bitmap ~100000×100000.
        assert!(Rasterizer::new(100_000.0, 1000).rasterize(&glyph).is_none());
    }
}

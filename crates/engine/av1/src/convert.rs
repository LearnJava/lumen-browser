//! Преобразование YUV → RGBA8 в фиксированной точке.
//!
//! Все коэффициенты (диапазон, матрица, разрядность) сводятся к таблицам по
//! значению отсчёта: на пиксель остаются три-четыре обращения к таблице и
//! сложение, поэтому кадр 1080p конвертируется за единицы миллисекунд на потоке.
//! Цветность растягивается «ближайшим соседом» (как и раньше в AVIF-декодере).

use crate::{Error, Layout, Picture, Plane, Samples};

/// Дробные биты таблиц: значения хранятся как `канал[0..255] * 65536`.
const FRAC: u32 = 16;
/// Кадры меньше этого числа пикселей конвертируются в одном потоке.
const PARALLEL_MIN_PIXELS: usize = 1 << 18;

/// Отсчёт плоскости как индекс таблицы.
trait Sample: Copy + Send + Sync {
    fn ix(self) -> usize;
}

impl Sample for u8 {
    #[inline(always)]
    fn ix(self) -> usize {
        usize::from(self)
    }
}

impl Sample for u16 {
    #[inline(always)]
    fn ix(self) -> usize {
        usize::from(self)
    }
}

/// Размер таблицы: все разрядности AV1 (≤ 12 бит) умещаются в 4096 значений;
/// фиксированный размер позволяет индексировать маской без проверки границ.
const TABLE_LEN: usize = 1 << 12;

type Table = Box<[i32; TABLE_LEN]>;

/// Таблицы вкладов отсчётов в каналы (Q16, шкала 0..255).
struct Tables {
    /// Яркость → нормированное значение (для identity — любая плоскость).
    y: Table,
    /// Cr → вклад в R.
    rcr: Table,
    /// Cb → вклад в G.
    gcb: Table,
    /// Cr → вклад в G.
    gcr: Table,
    /// Cb → вклад в B.
    bcb: Table,
}

impl Tables {
    fn new(pic: &Picture) -> Self {
        let bpc = u32::from(pic.bit_depth);
        let n = 1usize << bpc;
        let maxv = (n - 1) as f64;
        let scale = f64::from(1u32 << (bpc - 8));
        // (Kr, Kb) по matrix_coefficients (ITU-T H.273).
        let (kr, kb) = match pic.matrix {
            4..=6 => (0.299_f64, 0.114_f64),
            7 => (0.212, 0.087),
            9 | 10 => (0.2627, 0.0593),
            _ => (0.2126, 0.0722),
        };
        let kg = 1.0 - kr - kb;
        let (yoff, ymul, cmul, half) = if pic.full_range {
            (0.0, 255.0 / maxv, 255.0 / maxv, f64::from(1u32 << bpc) / 2.0)
        } else {
            (16.0 * scale, 255.0 / (219.0 * scale), 255.0 / (224.0 * scale), 128.0 * scale)
        };
        let q = f64::from(1u32 << FRAC);
        let table = |f: &dyn Fn(f64) -> f64| -> Table {
            let mut t: Table = Box::new([0; TABLE_LEN]);
            for (s, slot) in t.iter_mut().enumerate().take(n) {
                *slot = (f(s as f64) * q).round() as i32;
            }
            t
        };
        let cb = |s: f64| (s - half) * cmul;
        Self {
            y: table(&|s| (s - yoff) * ymul),
            rcr: table(&|s| 2.0 * (1.0 - kr) * cb(s)),
            gcb: table(&|s| -2.0 * kb * (1.0 - kb) / kg * cb(s)),
            gcr: table(&|s| -2.0 * kr * (1.0 - kr) / kg * cb(s)),
            bcb: table(&|s| 2.0 * (1.0 - kb) * cb(s)),
        }
    }
}

#[inline(always)]
fn to_u8(v: i32) -> u8 {
    ((v + (1 << (FRAC - 1))) >> FRAC).clamp(0, 255) as u8
}

/// Индекс таблицы из отсчёта (значения вне разрядности дают лишь неверный цвет).
#[inline(always)]
fn tx<S: Sample>(s: S) -> usize {
    s.ix() & (TABLE_LEN - 1)
}

/// Входные плоскости и параметры одной конвертации.
struct Job<'a, S: Sample> {
    w: usize,
    layout: Layout,
    identity: bool,
    t: &'a Tables,
    y: &'a [S],
    u: &'a [S],
    v: &'a [S],
    /// Ширина плоскости цветности (шаг строки).
    cw: usize,
}

impl<S: Sample> Job<'_, S> {
    /// Заполняет `out` — строки `row0..row0 + out.len() / (w * 4)`.
    fn rows(&self, row0: usize, out: &mut [u8]) {
        let w = self.w;
        let t = self.t;
        for (dy, orow) in out.chunks_exact_mut(w * 4).enumerate() {
            let y = row0 + dy;
            let yrow = &self.y[y * w..(y + 1) * w];
            if self.layout == Layout::I400 {
                for (px, &ys) in orow.chunks_exact_mut(4).zip(yrow) {
                    let l = to_u8(t.y[tx(ys)]);
                    px.copy_from_slice(&[l, l, l, 255]);
                }
                continue;
            }
            let cy = if self.layout == Layout::I420 { y / 2 } else { y };
            let urow = &self.u[cy * self.cw..(cy + 1) * self.cw];
            let vrow = &self.v[cy * self.cw..(cy + 1) * self.cw];
            if self.layout == Layout::I444 {
                for (((px, &ys), &us), &vs) in orow.chunks_exact_mut(4).zip(yrow).zip(urow).zip(vrow) {
                    let (us, vs) = (tx(us), tx(vs));
                    let yv = t.y[tx(ys)];
                    if self.identity {
                        // GBR: Y — зелёный, U — синий, V — красный, один масштаб.
                        px.copy_from_slice(&[to_u8(t.y[vs]), to_u8(yv), to_u8(t.y[us]), 255]);
                    } else {
                        px.copy_from_slice(&[
                            to_u8(yv + t.rcr[vs]),
                            to_u8(yv + t.gcb[us] + t.gcr[vs]),
                            to_u8(yv + t.bcb[us]),
                            255,
                        ]);
                    }
                }
                continue;
            }
            // 4:2:0 / 4:2:2: цветность общая для пары соседних пикселей строки.
            for (((opair, ypair), &us), &vs) in orow.chunks_mut(8).zip(yrow.chunks(2)).zip(urow).zip(vrow) {
                let (us, vs) = (tx(us), tx(vs));
                let (dr, dg, db) = (t.rcr[vs], t.gcb[us] + t.gcr[vs], t.bcb[us]);
                for (px, &ys) in opair.chunks_exact_mut(4).zip(ypair) {
                    let yv = t.y[tx(ys)];
                    px.copy_from_slice(&[to_u8(yv + dr), to_u8(yv + dg), to_u8(yv + db), 255]);
                }
            }
        }
    }
}

/// Запускает `Job` на всех ядрах (для больших кадров) или на текущем потоке.
fn run<S: Sample>(job: &Job<'_, S>, height: usize, out: &mut [u8]) {
    let w = job.w;
    let threads = if w * height >= PARALLEL_MIN_PIXELS {
        std::thread::available_parallelism().map_or(1, |n| n.get()).min(8)
    } else {
        1
    };
    if threads <= 1 {
        job.rows(0, out);
        return;
    }
    let rows_per = height.div_ceil(threads);
    std::thread::scope(|s| {
        for (i, chunk) in out.chunks_mut(rows_per * w * 4).enumerate() {
            s.spawn(move || job.rows(i * rows_per, chunk));
        }
    });
}

fn as_u8(s: &Samples) -> Option<&[u8]> {
    match s {
        Samples::U8(v) => Some(v),
        Samples::U16(_) => None,
    }
}

fn as_u16(s: &Samples) -> Option<&[u16]> {
    match s {
        Samples::U16(v) => Some(v),
        Samples::U8(_) => None,
    }
}

/// Плоскость `i` как срез или пустой срез (монохром без цветности).
fn plane_or_empty<S>(planes: &[Plane], i: usize, get: fn(&Samples) -> Option<&[S]>) -> &[S] {
    planes.get(i).and_then(|p| get(&p.samples)).unwrap_or(&[])
}

impl Picture {
    /// Размер RGBA-буфера для [`to_rgba8_into`](Self::to_rgba8_into), байт.
    #[must_use]
    pub fn rgba_len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    /// Конвертирует кадр в RGBA8 (4 байта на пиксель, row-major, альфа = 255).
    ///
    /// # Errors
    /// [`Error::Mismatch`], если плоскости не соответствуют размеру кадра.
    pub fn to_rgba8(&self) -> Result<Vec<u8>, Error> {
        let mut out = vec![0u8; self.rgba_len()];
        self.to_rgba8_into(&mut out)?;
        Ok(out)
    }

    /// То же, но в готовый буфер длиной [`rgba_len`](Self::rgba_len) — для видео,
    /// где буфер переиспользуется между кадрами.
    ///
    /// # Errors
    /// [`Error::Mismatch`], если длина буфера или плоскости не совпадают с кадром.
    pub fn to_rgba8_into(&self, out: &mut [u8]) -> Result<(), Error> {
        let (w, h) = (self.width as usize, self.height as usize);
        let (cw, ch) = self.layout.chroma_size(w, h);
        if out.len() != self.rgba_len() {
            return Err(Error::Mismatch(format!("буфер {} байт, нужно {}", out.len(), self.rgba_len())));
        }
        let want = [(w, h), (cw, ch), (cw, ch)];
        let ok = self.planes.len() == self.layout.plane_count()
            && self.planes.iter().zip(want).all(|(p, (pw, ph))| {
                p.width == pw
                    && p.height == ph
                    && match &p.samples {
                        Samples::U8(s) => s.len() == pw * ph,
                        Samples::U16(s) => s.len() == pw * ph,
                    }
            });
        if !ok {
            return Err(Error::Mismatch("плоскости не соответствуют размеру кадра".into()));
        }
        let t = Tables::new(self);
        let identity = self.matrix == 0 && self.layout == Layout::I444;
        macro_rules! go {
            ($get:ident) => {{
                let get = $get;
                let job = Job {
                    w,
                    layout: self.layout,
                    identity,
                    t: &t,
                    y: plane_or_empty(&self.planes, 0, get),
                    u: plane_or_empty(&self.planes, 1, get),
                    v: plane_or_empty(&self.planes, 2, get),
                    cw,
                };
                run(&job, h, out);
            }};
        }
        match self.planes.first().map(|p| &p.samples) {
            Some(Samples::U8(_)) => go!(as_u8),
            Some(Samples::U16(_)) => go!(as_u16),
            None => return Err(Error::Mismatch("кадр без плоскостей".into())),
        }
        Ok(())
    }

    /// Накладывает альфу из отдельного кадра (яркость монохромной плоскости AVIF-элемента
    /// `auxl`) на готовый RGBA-буфер; `premultiplied` — цвет уже умножен на альфу.
    ///
    /// # Errors
    /// [`Error::Mismatch`], если размер альфы не совпадает с цветом.
    pub fn apply_alpha(&self, rgba: &mut [u8], alpha: &Picture, premultiplied: bool) -> Result<(), Error> {
        if alpha.width != self.width || alpha.height != self.height || rgba.len() != self.rgba_len() {
            return Err(Error::Mismatch("размер альфа-канала не совпадает с цветом".into()));
        }
        let bpc = u32::from(alpha.bit_depth);
        let (off, den) = if alpha.full_range {
            (0i32, (1i32 << bpc) - 1)
        } else {
            (16 << (bpc - 8), 219 << (bpc - 8))
        };
        let a8 = |v: u32| -> u8 { ((((v as i32 - off) * 255) + den / 2) / den).clamp(0, 255) as u8 };
        let luma = |i: usize| -> u8 {
            match alpha.planes.first().map(|p| &p.samples) {
                Some(Samples::U8(s)) => a8(u32::from(s[i])),
                Some(Samples::U16(s)) => a8(u32::from(s[i])),
                None => 255,
            }
        };
        for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
            let a = luma(i);
            px[3] = a;
            if premultiplied && a > 0 && a < 255 {
                let a = u32::from(a);
                for c in &mut px[..3] {
                    *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Ok(())
    }
}

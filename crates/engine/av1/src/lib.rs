//! Общий AV1-декодер Lumen (ADR-035 п.2): безопасная обёртка над `rav1d`
//! (memory-safe порт dav1d) и преобразование YUV → RGBA.
//!
//! Один крейт обслуживает и неподвижные AVIF (`lumen-image`), и видео
//! (`UX-VIDEO-NATIVE`): весь `unsafe` над C-ABI `dav1d_*` собран здесь, снаружи —
//! безопасный [`Decoder`] с потоковым интерфейсом «отправь temporal unit →
//! забери готовые кадры».
//!
//! ```text
//! Decoder::new(&Config::video()) ── send(tu, pts) ──► next_picture() ─► Picture
//!                                   drain() в конце потока, flush() при перемотке
//! Picture::to_rgba8() / to_rgba8_into(&mut buf)      ─► RGBA8, row-major
//! ```
//!
//! Декодирование не требует системных библиотек: `rav1d` собран без `asm`
//! (нужен nasm), остаётся портируемый Rust-путь. Многопоточность — потоки
//! самого rav1d (число задаёт [`Config::threads`]).

mod convert;

use core::ptr::NonNull;
use std::collections::VecDeque;
use std::mem::MaybeUninit;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::picture::Dav1dPicture;

/// Код `EAGAIN` в dav1d (`DAV1D_ERR(EAGAIN)`): «нужно больше данных / заберите кадры».
const EAGAIN: i32 = -11;

/// Ошибка AV1-декодера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Вызов `dav1d_*` вернул ошибку (`what` — имя вызова, `code` — код rav1d).
    Decoder {
        /// Имя вызова rav1d.
        what: &'static str,
        /// Код ошибки (отрицательный errno).
        code: i32,
    },
    /// Кадр не поддерживается (нулевой размер, неизвестная глубина цвета).
    Unsupported(String),
    /// Данные кончились, а кадра нет (для одиночных картинок).
    NoPicture,
    /// Размеры или планы разных кадров не сходятся (цвет и альфа AVIF).
    Mismatch(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decoder { what, code } => write!(f, "rav1d: {what} ({code})"),
            Self::Unsupported(s) | Self::Mismatch(s) => write!(f, "rav1d: {s}"),
            Self::NoPicture => write!(f, "rav1d: кадр не получен"),
        }
    }
}

impl std::error::Error for Error {}

/// Настройки декодера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Число потоков rav1d; 0 — по числу логических ядер.
    pub threads: u32,
    /// Максимум кадров в конвейере кадровой параллельности; 0 — выбрать
    /// автоматически (чем больше, тем выше пропускная способность и задержка).
    pub max_frame_delay: u32,
    /// Предел площади кадра в пикселях: защита от декомпрессионной бомбы.
    pub frame_size_limit: u32,
    /// Накладывать синтез зернистости плёнки (film grain) — по спецификации да.
    pub apply_grain: bool,
}

impl Config {
    /// Неподвижная картинка: один поток, без конвейера кадров, ≤ 64 Мпикс
    /// (~256 МБ RGBA).
    #[must_use]
    pub const fn still() -> Self {
        Self { threads: 1, max_frame_delay: 1, frame_size_limit: 64 * 1024 * 1024, apply_grain: true }
    }

    /// Видео: потоки по числу ядер, автоматическая глубина конвейера, ≤ 8K.
    #[must_use]
    pub const fn video() -> Self {
        Self { threads: 0, max_frame_delay: 0, frame_size_limit: 8192 * 4320, apply_grain: true }
    }
}

/// Раскладка цветовых плоскостей.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Только яркость.
    I400,
    /// 4:2:0 — цветность вдвое меньше по обеим осям.
    I420,
    /// 4:2:2 — цветность вдвое меньше по горизонтали.
    I422,
    /// 4:4:4 — полная цветность.
    I444,
}

impl Layout {
    fn from_raw(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::I400),
            1 => Some(Self::I420),
            2 => Some(Self::I422),
            3 => Some(Self::I444),
            _ => None,
        }
    }

    /// Размеры плоскости цветности для кадра `w`×`h`.
    #[must_use]
    pub const fn chroma_size(self, w: usize, h: usize) -> (usize, usize) {
        match self {
            Self::I420 => (w.div_ceil(2), h.div_ceil(2)),
            Self::I422 => (w.div_ceil(2), h),
            Self::I400 | Self::I444 => (w, h),
        }
    }

    /// Число плоскостей: 1 для монохрома, иначе 3.
    #[must_use]
    pub const fn plane_count(self) -> usize {
        if matches!(self, Self::I400) { 1 } else { 3 }
    }
}

/// Отсчёты плоскости: 8 бит — байты, 10/12 бит — `u16`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Samples {
    /// 8-битные отсчёты.
    U8(Vec<u8>),
    /// 10/12-битные отсчёты (значение в младших битах).
    U16(Vec<u16>),
}

/// Плоскость кадра без отступов: строки идут подряд, шаг равен ширине.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plane {
    /// Ширина в отсчётах.
    pub width: usize,
    /// Высота в строках.
    pub height: usize,
    /// Отсчёты, `width * height` штук.
    pub samples: Samples,
}

/// Декодированный кадр (YUV, как отдал rav1d; копия — не держит буферы декодера).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Ширина кадра в пикселях.
    pub width: u32,
    /// Высота кадра в пикселях.
    pub height: u32,
    /// Разрядность отсчёта: 8, 10 или 12.
    pub bit_depth: u8,
    /// Раскладка плоскостей.
    pub layout: Layout,
    /// Полный диапазон (`true`) или ограниченный 16–235 (`false`).
    pub full_range: bool,
    /// `matrix_coefficients` по ITU-T H.273 (0 — identity/GBR, 1 — BT.709, 5/6 — BT.601, 9 — BT.2020).
    pub matrix: u8,
    /// `color_primaries` по ITU-T H.273.
    pub primaries: u8,
    /// `transfer_characteristics` по ITU-T H.273.
    pub transfer: u8,
    /// Метка времени, переданная в [`Decoder::send`] с кадром, из которого получен этот.
    pub pts: i64,
    /// Плоскости Y, U, V (одна, если [`Layout::I400`]).
    pub planes: Vec<Plane>,
}

/// Закрывает контекст rav1d на любом выходе.
struct CtxGuard(Option<Dav1dContext>);

impl Drop for CtxGuard {
    fn drop(&mut self) {
        // SAFETY: контекст получен из `dav1d_open` и закрывается ровно один раз;
        // `dav1d_close` сам обнуляет переданную ссылку.
        unsafe { rav1d::src::lib::dav1d_close(Some(NonNull::from(&mut self.0))) };
    }
}

/// Потоковый AV1-декодер.
///
/// Порядок использования: [`send`](Self::send) для каждого temporal unit
/// (в MP4/WebM это пакет контейнера, в AVIF — элемент изображения) и
/// [`next_picture`](Self::next_picture) в цикле до `None`. Кадры выходят в порядке показа.
/// В конце потока — [`drain`](Self::drain), при перемотке — [`flush`](Self::flush).
pub struct Decoder {
    ctx: CtxGuard,
    ready: VecDeque<Picture>,
}

// SAFETY: контекст rav1d не привязан к потоку создания; `Decoder` владеет им
// единолично, а все вызовы идут через `&mut self` (по одному потоку за раз).
unsafe impl Send for Decoder {}

impl Decoder {
    /// Создаёт декодер.
    ///
    /// # Errors
    /// [`Error::Decoder`], если rav1d не смог открыть контекст.
    pub fn new(config: &Config) -> Result<Self, Error> {
        let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: `dav1d_default_settings` полностью инициализирует переданную структуру.
        let mut settings = unsafe {
            rav1d::src::lib::dav1d_default_settings(NonNull::from(&mut settings).cast());
            settings.assume_init()
        };
        let threads = if config.threads == 0 {
            std::thread::available_parallelism().map_or(1, |n| n.get())
        } else {
            config.threads as usize
        };
        settings.n_threads = i32::try_from(threads.min(64)).unwrap_or(1);
        settings.max_frame_delay = i32::try_from(config.max_frame_delay).unwrap_or(0);
        settings.frame_size_limit = config.frame_size_limit;
        settings.apply_grain = i32::from(config.apply_grain);
        // Выдаём только старший пространственный слой (масштабируемый AV1 в вебе не нужен).
        settings.all_layers = 0;

        let mut guard = CtxGuard(None);
        // SAFETY: оба указателя валидны на запись/чтение на время вызова.
        let rc = unsafe {
            rav1d::src::lib::dav1d_open(Some(NonNull::from(&mut guard.0)), Some(NonNull::from(&mut settings)))
        };
        if rc.0 < 0 || guard.0.is_none() {
            return Err(Error::Decoder { what: "open", code: rc.0 });
        }
        Ok(Self { ctx: guard, ready: VecDeque::new() })
    }

    /// Отправляет temporal unit (последовательность OBU) с меткой `pts`.
    ///
    /// Готовые кадры накапливаются во внутренней очереди — забирать их через
    /// [`next_picture`](Self::next_picture).
    ///
    /// # Errors
    /// [`Error::Decoder`] — данные повреждены; [`Error::Unsupported`] — кадр не поддерживается.
    pub fn send(&mut self, data: &[u8], pts: i64) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }
        let mut buf = MaybeUninit::<Dav1dData>::uninit();
        // SAFETY: `dav1d_data_create` пишет в `buf` и возвращает буфер длины `data.len()`.
        let mut buf = unsafe {
            let dst = rav1d::src::lib::dav1d_data_create(Some(NonNull::from(&mut buf).cast()), data.len());
            if dst.is_null() {
                return Err(Error::Decoder { what: "data_create", code: -12 });
            }
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
            buf.assume_init()
        };
        buf.m.timestamp = pts;

        let mut result = Ok(());
        loop {
            if buf.sz > 0 {
                // SAFETY: контекст и данные валидны; `sz` уменьшается по мере потребления.
                let rc = unsafe { rav1d::src::lib::dav1d_send_data(self.ctx.0, Some(NonNull::from(&mut buf))) };
                if rc.0 < 0 && rc.0 != EAGAIN {
                    result = Err(Error::Decoder { what: "send_data", code: rc.0 });
                    break;
                }
            }
            match self.fetch() {
                Ok(Some(p)) => self.ready.push_back(p),
                Ok(None) => {
                    if buf.sz == 0 {
                        break;
                    }
                }
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        // SAFETY: освобождает остаток данных (no-op, если всё потреблено).
        unsafe { rav1d::src::lib::dav1d_data_unref(Some(NonNull::from(&mut buf))) };
        result
    }

    /// Следующий готовый кадр в порядке показа; `None` — нужны новые данные.
    pub fn next_picture(&mut self) -> Option<Picture> {
        self.ready.pop_front()
    }

    /// Конец потока: вытолкнуть кадры, застрявшие в конвейере кадровой параллельности.
    /// После вызова кадры читаются через [`next_picture`](Self::next_picture).
    ///
    /// # Errors
    /// [`Error::Decoder`] / [`Error::Unsupported`] — как в [`send`](Self::send).
    pub fn drain(&mut self) -> Result<(), Error> {
        // Первый `get_picture` после `send_data` лишь переводит rav1d в режим слива,
        // второй подряд начинает отдавать остаток — поэтому ждём два «пусто» подряд.
        let mut idle = 0;
        while idle < 2 {
            match self.fetch()? {
                Some(p) => {
                    self.ready.push_back(p);
                    idle = 0;
                }
                None => idle += 1,
            }
        }
        Ok(())
    }

    /// Сброс состояния при перемотке: отбрасывает очередь и кадры в конвейере.
    /// Следующим должен идти ключевой кадр (с sequence header).
    pub fn flush(&mut self) {
        self.ready.clear();
        if let Some(ctx) = self.ctx.0 {
            // SAFETY: контекст валиден и принадлежит `self`.
            unsafe { rav1d::src::lib::dav1d_flush(ctx) };
        }
    }

    /// Один `dav1d_get_picture`: `Ok(None)` на `EAGAIN`.
    fn fetch(&mut self) -> Result<Option<Picture>, Error> {
        let mut out = MaybeUninit::<Dav1dPicture>::uninit();
        // SAFETY: `dav1d_get_picture` всегда пишет в `out`.
        let rc = unsafe { rav1d::src::lib::dav1d_get_picture(self.ctx.0, Some(NonNull::from(&mut out).cast())) };
        if rc.0 == EAGAIN {
            return Ok(None);
        }
        if rc.0 < 0 {
            return Err(Error::Decoder { what: "get_picture", code: rc.0 });
        }
        // SAFETY: при коде 0 картинка инициализирована.
        let mut pic = unsafe { out.assume_init() };
        let copied = copy_picture(&pic);
        // SAFETY: картинка получена из `dav1d_get_picture` и освобождается ровно один раз.
        unsafe { rav1d::src::lib::dav1d_picture_unref(Some(NonNull::from(&mut pic))) };
        copied.map(Some)
    }
}

/// Одиночное декодирование (AVIF и подобное): один temporal unit → первый кадр.
///
/// # Errors
/// Любая ошибка [`Decoder`]; [`Error::NoPicture`], если кадра нет.
pub fn decode_still(data: &[u8], frame_size_limit: u32) -> Result<Picture, Error> {
    let mut dec = Decoder::new(&Config { frame_size_limit, ..Config::still() })?;
    dec.send(data, 0)?;
    if let Some(p) = dec.next_picture() {
        return Ok(p);
    }
    dec.drain()?;
    dec.next_picture().ok_or(Error::NoPicture)
}

fn copy_picture(pic: &Dav1dPicture) -> Result<Picture, Error> {
    let w = pic.p.w as usize;
    let h = pic.p.h as usize;
    let bpc = pic.p.bpc as u32;
    let layout = Layout::from_raw(pic.p.layout);
    let Some(layout) = layout.filter(|_| w > 0 && h > 0 && matches!(bpc, 8 | 10 | 12)) else {
        return Err(Error::Unsupported(format!("неподдерживаемый кадр {w}x{h} bpc={bpc} layout={}", pic.p.layout)));
    };
    let (matrix, primaries, transfer, full_range) = match pic.seq_hdr {
        // SAFETY: seq_hdr принадлежит картинке и жив до `picture_unref`.
        Some(p) => unsafe {
            let s = p.as_ref();
            (s.mtrx as u8, s.pri as u8, s.trc as u8, s.color_range != 0)
        },
        // Нет заголовка: считаем «не задано» (2) и полный диапазон, как в AVIF-декодере.
        None => (2, 2, 2, true),
    };
    let (cw, ch) = layout.chroma_size(w, h);
    let mut planes = Vec::with_capacity(layout.plane_count());
    for i in 0..layout.plane_count() {
        let (pw, ph) = if i == 0 { (w, h) } else { (cw, ch) };
        let stride = pic.stride[usize::from(i != 0)];
        let base = pic.data[i].ok_or_else(|| Error::Unsupported("нет плоскости".into()))?;
        let bytes_per = if bpc == 8 { 1 } else { 2 };
        let mut s8 = Vec::new();
        let mut s16 = Vec::new();
        if bpc == 8 {
            s8.reserve_exact(pw * ph);
        } else {
            s16.reserve_exact(pw * ph);
        }
        for y in 0..ph {
            // SAFETY: rav1d гарантирует `ph` строк по `stride` байт, в каждой минимум `pw` отсчётов.
            let row = unsafe {
                core::slice::from_raw_parts(base.as_ptr().cast::<u8>().offset(y as isize * stride), pw * bytes_per)
            };
            if bpc == 8 {
                s8.extend_from_slice(row);
            } else {
                s16.extend(row.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
            }
        }
        let samples = if bpc == 8 { Samples::U8(s8) } else { Samples::U16(s16) };
        planes.push(Plane { width: pw, height: ph, samples });
    }
    Ok(Picture {
        width: pic.p.w as u32,
        height: pic.p.h as u32,
        bit_depth: bpc as u8,
        layout,
        full_range,
        matrix,
        primaries,
        transfer,
        pts: pic.m.timestamp,
        planes,
    })
}

#[cfg(test)]
mod tests;

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use gif::{DecodeOptions, Decoder};
use weezl::BitOrder;
use weezl::decode::Decoder as LzwDecoder;
use weezl::LzwStatus;
use crate::{Image, PixelFormat};

/// GIF сигнатура: "GIF87a" или "GIF89a" (6 байтов).
pub const GIF_SIGNATURE_LEN: usize = 6;
pub const GIF87A_SIGNATURE: &[u8; 6] = b"GIF87a";
pub const GIF89A_SIGNATURE: &[u8; 6] = b"GIF89a";

/// Ошибки декодирования GIF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GifError {
    /// Первые 6 байтов не равны "GIF87a" или "GIF89a".
    InvalidSignature,
    /// Ошибка при чтении GIF структуры.
    DecodeError(String),
    /// GIF не содержит кадров (пусто).
    NoFrames,
    /// Неподдерживаемая кодировка пикселей (обычно используется паллетированная, но конвертируем в RGBA).
    UnsupportedEncoding,
}

impl core::fmt::Display for GifError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "не GIF: сигнатура не совпала"),
            Self::DecodeError(s) => write!(f, "GIF декодирование: {s}"),
            Self::NoFrames => write!(f, "GIF: нет кадров"),
            Self::UnsupportedEncoding => write!(f, "GIF: неподдерживаемая кодировка"),
        }
    }
}

impl std::error::Error for GifError {}

/// Проверяет, является ли начало `bytes` валидной GIF сигнатурой (GIF87a или GIF89a).
pub fn is_gif(bytes: &[u8]) -> bool {
    if bytes.len() < GIF_SIGNATURE_LEN {
        return false;
    }
    bytes[..6] == GIF87A_SIGNATURE[..] || bytes[..6] == GIF89A_SIGNATURE[..]
}

/// Переводит задержку кадра из сотых долей секунды (GIF spec §23.c.vi) в миллисекунды.
/// Значение 0 браузеры трактуют как ~10 cs (100 мс) — воспроизводим это поведение.
#[must_use]
const fn delay_cs_to_ms(delay_cs: u16) -> u64 {
    let cs = if delay_cs == 0 { 10 } else { delay_cs as u64 };
    cs * 10
}

/// Количество повторений анимации GIF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifLoopCount {
    /// Анимация воспроизводится ровно N раз (N ≥ 1).
    Finite(u16),
    /// Анимация воспроизводится бесконечно (Netscape extension loop_count = 0).
    Infinite,
}

/// Возвращает индекс кадра для `elapsed_ms` по массиву задержек `delays_cs`.
///
/// Чистая функция над метаданными таймингов — выделена из [`AnimatedGif::frame_index_at`]
/// для юнит-тестирования без реальных GIF-байтов.
///
/// - `GifLoopCount::Infinite` — время берётся по модулю суммарной длительности.
/// - `GifLoopCount::Finite(n)` — после `n` повторений останавливается на последнем кадре.
/// - Пустой `delays_cs` → всегда 0 (безопасный fallback).
#[must_use]
fn frame_index_for(delays_cs: &[u16], loop_count: GifLoopCount, elapsed_ms: u64) -> usize {
    if delays_cs.is_empty() {
        return 0;
    }
    let total_ms: u64 = delays_cs.iter().map(|&cs| delay_cs_to_ms(cs)).sum();
    if total_ms == 0 {
        return 0;
    }

    let effective_ms = match loop_count {
        GifLoopCount::Infinite => elapsed_ms % total_ms,
        GifLoopCount::Finite(n) => {
            let max_ms = total_ms.saturating_mul(u64::from(n));
            if elapsed_ms >= max_ms {
                // Animation ended — hold last frame.
                return delays_cs.len() - 1;
            }
            elapsed_ms % total_ms
        }
    };

    let mut acc = 0u64;
    for (i, &cs) in delays_cs.iter().enumerate() {
        acc += delay_cs_to_ms(cs);
        if effective_ms < acc {
            return i;
        }
    }
    delays_cs.len() - 1
}

/// Опции контейнерного прохода: LZW-данные кадров **не** разжимаются на месте, а выдаются
/// как есть (`skip_frame_decoding`), чтобы пиксели каждого кадра распаковывал
/// [`decode_frame_rgba`] строго до заполнения кадрового буфера.
///
/// BUG-396: у `gif::Decoder` со встроенным LZW нет способа «дочитать кадр и перейти к
/// следующему» — `next_frame_info` докручивает LZW-поток текущего кадра до конца, даже если
/// все пиксели уже выданы. Файлы, чей энкодер наращивает ширину кода по early-change-правилу
/// (giflib и производные: ширина растёт на один код раньше, чем ждёт `weezl`), несут после
/// последнего пикселя биты, которые декодер прочитывает как несуществующий код — вся анимация
/// падала `invalid code in LZW stream`, хотя пиксели всех кадров декодируются верно. Контейнерный
/// проход без LZW этих битов не касается, а пиксельный останавливается на `width × height`
/// пикселях и до хвоста не доходит — ровно так же ведут себя Chromium и Pillow.
fn container_options() -> DecodeOptions {
    let mut options = DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    options.skip_frame_decoding(true);
    options
}

/// Байтов на пиксель в RGBA8-выходе.
const RGBA_CHANNELS: usize = 4;

/// Байтов на запись глобальной/локальной палитры GIF (RGB-триплет).
const PALETTE_CHANNELS: usize = 3;

/// Допустимый диапазон LZW minimum code size (GIF spec §22.c.ii — до 8 бит на индекс;
/// `gif`/`weezl` принимают до 11, повторяем их границу, чтобы не сузить приём файлов).
const LZW_MIN_CODE_SIZE_RANGE: core::ops::RangeInclusive<u8> = 1..=11;

/// Порядок строк, в котором чересстрочный (interlaced) GIF выдаёт их из потока:
/// четыре прохода — каждый восьмой от 0, каждый восьмой от 4, каждый четвёртый от 2,
/// каждый второй от 1 (GIF spec §20.c.ii). `rows[i]` — экранная строка для `i`-й
/// строки потока.
fn interlace_row_order(height: usize) -> Vec<usize> {
    let mut rows = Vec::with_capacity(height);
    for (start, step) in [(0usize, 8usize), (4, 8), (2, 4), (1, 2)] {
        let mut row = start;
        while row < height {
            rows.push(row);
            row += step;
        }
    }
    rows
}

/// Распаковывает LZW-поток кадра в `out` (по индексу палитры на байт) и возвращает число
/// записанных байтов. Останавливается, как только `out` заполнен, — хвост потока за
/// последним пикселем не читается вовсе (BUG-396: энкодеры семейства giflib оставляют там
/// биты, которые `weezl` прочитывает как несуществующий код).
///
/// # BUG-787: почему распаковка своя, а не `gif::FrameDecoder`
///
/// `gif` 0.14.2 в `decode_lzw_encoded_frame_into_buffer` крутит
/// `loop { … if bytes_written > 0 || status == NoProgress { return } }`, а `weezl` после
/// end-кода навсегда отвечает `(0, 0, Done)` (`decode.rs::advance`, ранний выход по
/// `has_ended`). Кадр, чей поток кончился раньше, чем заполнен буфер, вешает этот цикл
/// НАВСЕГДА — на 78-байтном GIF процесс не возвращал управление за 60 с. Класс — DoS:
/// путь пользовательский (`<img>` из сети). Здесь цикл завершается по трём условиям сразу:
/// буфер заполнен, декодер сообщил `Done`/`NoProgress`, либо итерация не сдвинула ни вход,
/// ни выход. Апстрим не чинен: 0.14.2 — последняя версия на crates.io на 2026-08-20.
///
/// # Errors
/// - [`GifError::DecodeError`] — `weezl` отверг код в потоке до того, как кадр заполнен.
fn lzw_decode_into(min_code_size: u8, data: &[u8], out: &mut [u8]) -> Result<usize, GifError> {
    let mut decoder = LzwDecoder::new(BitOrder::Lsb, min_code_size);
    let mut in_pos = 0usize;
    let mut out_pos = 0usize;

    while out_pos < out.len() {
        let (input, output) = (
            data.get(in_pos..).unwrap_or_default(),
            out.get_mut(out_pos..).unwrap_or_default(),
        );
        let result = decoder.decode_bytes(input, output);
        in_pos = in_pos.saturating_add(result.consumed_in);
        out_pos = out_pos.saturating_add(result.consumed_out);

        // Кадр заполнен — что бы декодер ни сообщил про хвост, он уже не наш.
        if out_pos >= out.len() {
            break;
        }
        match result.status {
            // Единственная ветка, продолжающая цикл, и только когда шаг что-то сдвинул:
            // иначе следующая итерация повторит его байт в байт (это и есть зависание).
            Ok(LzwStatus::Ok) if result.consumed_in > 0 || result.consumed_out > 0 => {}
            Ok(_) => break,
            Err(e) => return Err(GifError::DecodeError(e.to_string())),
        }
    }

    Ok(out_pos)
}

/// Раскладывает один кадр в RGBA8: распаковывает LZW, применяет палитру кадра (или глобальную)
/// и transparent-index, при `frame.interlaced` расставляет строки по местам.
///
/// Пишет в префикс `out` длиной `frame.width × frame.height × 4` — как это делал
/// `gif::FrameDecoder`, чей конвертер тоже игнорировал `frame.left`/`frame.top`. Пиксель,
/// чьего индекса нет в палитре, не трогается (в `out` он останется нулём = прозрачным
/// чёрным) — тоже поведение `gif`.
///
/// # Errors
/// - [`GifError::DecodeError`] — недопустимый LZW minimum code size, слишком маленький `out`
///   или поток, оборвавшийся раньше, чем заполнен кадр (BUG-787).
fn decode_frame_rgba(
    frame: &gif::Frame<'_>,
    global_palette: Option<&[u8]>,
    out: &mut [u8],
) -> Result<(), GifError> {
    let frame_w = frame.width as usize;
    let frame_h = frame.height as usize;
    let pixels = frame_w
        .checked_mul(frame_h)
        .ok_or_else(|| GifError::DecodeError("переполнение размера кадра".to_string()))?;
    let rgba_len = pixels
        .checked_mul(RGBA_CHANNELS)
        .ok_or_else(|| GifError::DecodeError("переполнение размера кадра".to_string()))?;
    let out = out
        .get_mut(..rgba_len)
        .ok_or_else(|| GifError::DecodeError("буфер кадра меньше самого кадра".to_string()))?;

    // Первый байт данных кадра — LZW minimum code size, дальше сам поток
    // (`skip_frame_decoding` отдаёт кадр именно в таком виде).
    let (&min_code_size, data) = frame.buffer.split_first().unwrap_or((&2, &[]));
    if !LZW_MIN_CODE_SIZE_RANGE.contains(&min_code_size) {
        return Err(GifError::DecodeError(format!(
            "недопустимый LZW minimum code size: {min_code_size}"
        )));
    }

    let mut indexed = vec![0u8; pixels];
    let written = lzw_decode_into(min_code_size, data, &mut indexed)?;
    if written < pixels {
        return Err(GifError::DecodeError(format!(
            "LZW-поток кадра оборван: {written} из {pixels} пикселей"
        )));
    }

    let palette = frame.palette.as_deref().or(global_palette).unwrap_or_default();
    let transparent = frame.transparent;
    let interlace = frame.interlaced.then(|| interlace_row_order(frame_h));

    for src_row in 0..frame_h {
        let dst_row = interlace
            .as_ref()
            .map_or(src_row, |rows| rows.get(src_row).copied().unwrap_or(src_row));
        let src_start = src_row * frame_w;
        let dst_start = dst_row * frame_w * RGBA_CHANNELS;
        let (Some(src), Some(dst)) = (
            indexed.get(src_start..src_start + frame_w),
            out.get_mut(dst_start..dst_start + frame_w * RGBA_CHANNELS),
        ) else {
            return Err(GifError::DecodeError(format!("строка {dst_row} вне кадра")));
        };
        for (rgba, &idx) in dst.chunks_exact_mut(RGBA_CHANNELS).zip(src) {
            let offset = usize::from(idx) * PALETTE_CHANNELS;
            let Some(color) = palette.get(offset..offset + PALETTE_CHANNELS) else {
                continue;
            };
            rgba[0] = color[0];
            rgba[1] = color[1];
            rgba[2] = color[2];
            rgba[3] = if transparent == Some(idx) { 0x00 } else { 0xFF };
        }
    }

    Ok(())
}

/// Ленивое состояние декодера: живой forward-only `gif::Decoder` над `Arc<[u8]>`-байтами,
/// его позиция и кэш последнего выданного кадра.
///
/// GIF-кадры взаимозависимы (disposal composited поверх предыдущих), поэтому произвольный
/// доступ к кадру `N` требует последовательного декода `0..=N`. Курсор держит декодер живым,
/// чтобы forward-воспроизведение стоило один декод кадра на переход, а не `O(N)` каждый раз.
/// При запросе кадра «позади» курсора (wrap на 0 в цикле или обратный seek) декодер
/// пересоздаётся с начала.
struct GifCursor {
    /// Живой контейнерный декодер, спозиционированный так, что следующий читаемый кадр имеет
    /// индекс `next_idx`. Выдаёт кадры с несжатыми LZW-данными (см. [`container_options`]).
    /// В `Box`, чтобы объёмный `gif::Decoder` не раздувал `AnimatedGif` при простое (курсор `None`
    /// всё равно резервирует место под самый большой вариант `Option`).
    reader: Box<Decoder<Cursor<Arc<[u8]>>>>,
    /// Глобальная палитра файла (RGB-триплеты) — подставляется кадрам без локальной.
    global_palette: Option<Vec<u8>>,
    /// Индекс следующего кадра, который выдаст `reader` (число уже прочитанных кадров).
    next_idx: usize,
    /// Кэш последнего выданного кадра `(индекс, пиксели)` — обслуживает повторный запрос
    /// того же кадра без пересоздания декодера.
    last: Option<(usize, Image)>,
}

impl GifCursor {
    /// Создаёт новый forward-декодер с позиции нулевого кадра.
    fn new(encoded: &Arc<[u8]>) -> Result<Self, GifError> {
        let reader = container_options()
            .read_info(Cursor::new(Arc::clone(encoded)))
            .map_err(|e| GifError::DecodeError(e.to_string()))?;
        let global_palette = reader
            .global_palette()
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec);
        Ok(Self {
            reader: Box::new(reader),
            global_palette,
            next_idx: 0,
            last: None,
        })
    }
}

/// Анимированный GIF с **ленивым** декодированием кадров.
///
/// BUG-272 срез 19: вместо eager-декода всех кадров в память при загрузке хранятся только
/// закодированные байты (`Arc<[u8]>`, разделяемые между копиями) и per-frame задержки
/// (дешёвые метаданные). Пиксели кадра декодируются по запросу через forward-курсор
/// ([`GifCursor`]) и держатся резидентно в объёме ~одного кадра, а не всех `N`. Для
/// многокадровых крупных GIF это снимает `O(N)`-пик пиксельной памяти.
pub struct AnimatedGif {
    /// Закодированные GIF-байты, разделяемые между клонами `AnimatedGif` и живыми курсорами.
    encoded: Arc<[u8]>,
    /// Логическая ширина экрана GIF (Logical Screen Descriptor), пикселей.
    pub width: u32,
    /// Логическая высота экрана GIF, пикселей.
    pub height: u32,
    /// Количество повторений анимации.
    pub loop_count: GifLoopCount,
    /// Задержка каждого кадра в сотых долях секунды, в порядке отображения. Всегда непустой
    /// (гарантирует [`decode_gif_animated`]). Декодируется один раз при загрузке.
    delays_cs: Vec<u16>,
    /// Ленивое состояние декодера. `Mutex` даёт `Send + Sync` (GIF хранится за `Arc` и
    /// шарится между потоком-загрузчиком и UI); `None` до первого запроса кадра.
    cursor: Mutex<Option<GifCursor>>,
}

impl Clone for AnimatedGif {
    /// Клонирует метаданные (Arc-указатель на байты + `delays_cs`); ленивый курсор
    /// не копируется — клон стартует с чистого состояния декодера. Дёшево: без копии пикселей.
    fn clone(&self) -> Self {
        Self {
            encoded: Arc::clone(&self.encoded),
            width: self.width,
            height: self.height,
            loop_count: self.loop_count,
            delays_cs: self.delays_cs.clone(),
            cursor: Mutex::new(None),
        }
    }
}

impl core::fmt::Debug for AnimatedGif {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AnimatedGif")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("loop_count", &self.loop_count)
            .field("frame_count", &self.delays_cs.len())
            .field("encoded_len", &self.encoded.len())
            .finish()
    }
}

impl AnimatedGif {
    /// Количество кадров анимации (всегда ≥ 1).
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.delays_cs.len()
    }

    /// Задержка кадра `idx` в миллисекундах. Индекс за границей клампится к последнему кадру.
    #[must_use]
    pub fn frame_delay_ms(&self, idx: usize) -> u64 {
        let idx = idx.min(self.delays_cs.len().saturating_sub(1));
        self.delays_cs.get(idx).copied().map_or(0, delay_cs_to_ms)
    }

    /// Суммарная длительность одного прохода анимации в миллисекундах.
    #[must_use]
    pub fn total_cycle_ms(&self) -> u64 {
        self.delays_cs.iter().map(|&cs| delay_cs_to_ms(cs)).sum()
    }

    /// Резидентный объём памяти GIF в байтах: закодированные байты плюс закэшированный
    /// в курсоре кадр (если есть). Используется диагностикой памяти (`LUMEN_MEM_REPORT`);
    /// в отличие от старого eager-хранилища не растёт как `N × width × height × 4`.
    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        let cached = self
            .cursor
            .lock()
            .ok()
            .and_then(|g| g.as_ref().and_then(|c| c.last.as_ref().map(|(_, img)| img.data.len())))
            .unwrap_or(0);
        self.encoded.len() + cached
    }

    /// Возвращает индекс кадра для `elapsed_ms` миллисекунд от начала анимации.
    ///
    /// - `GifLoopCount::Infinite` — время берётся по модулю суммарной длительности.
    /// - `GifLoopCount::Finite(n)` — после `n` повторений останавливается на последнем кадре.
    #[must_use]
    pub fn frame_index_at(&self, elapsed_ms: u64) -> usize {
        frame_index_for(&self.delays_cs, self.loop_count, elapsed_ms)
    }

    /// Декодирует и возвращает пиксели кадра `idx` (RGBA8, полный экранный буфер
    /// `width × height` с применёнными composite/disposal-операциями).
    ///
    /// Forward-запросы (`idx ≥` позиции курсора) стоят один декод кадра на переход;
    /// запрос кадра «позади» курсора пересоздаёт декодер с начала. Индекс за границей
    /// клампится к последнему кадру.
    ///
    /// # Errors
    /// - [`GifError::DecodeError`] — ошибка декодера или недостижимый кадр.
    #[allow(clippy::expect_used)]  // унаследовано, docs/lint-policy.md §10
    pub fn frame_image(&self, idx: usize) -> Result<Image, GifError> {
        let idx = idx.min(self.delays_cs.len().saturating_sub(1));
        let frame_bytes = (self.width as usize) * (self.height as usize) * 4;

        let mut guard = self
            .cursor
            .lock()
            .map_err(|_| GifError::DecodeError("GIF-курсор отравлен".to_string()))?;

        // Reuse the live decoder only if we can reach `idx` by reading forward, or if the
        // requested frame is exactly the cached last one. Otherwise reset to frame 0.
        let can_reuse = match guard.as_ref() {
            Some(c) => c.next_idx <= idx || c.last.as_ref().is_some_and(|(li, _)| *li == idx),
            None => false,
        };
        if !can_reuse {
            *guard = Some(GifCursor::new(&self.encoded)?);
        }
        let cursor = guard.as_mut().expect("cursor set above");

        // Serve a repeated request for the same frame from the cache.
        if let Some((li, img)) = cursor.last.as_ref()
            && *li == idx
        {
            return Ok(img.clone());
        }

        // Read forward until frame `idx` has been consumed; intermediate frames must be
        // decoded too (disposal makes each frame depend on its predecessors).
        let mut buffer = Vec::new();
        while cursor.next_idx <= idx {
            // Disjoint field borrows: `frame` borrows `cursor.reader`, the palette is
            // `cursor.global_palette`.
            let global_palette = cursor.global_palette.as_deref();
            let Some(frame) = cursor
                .reader
                .read_next_frame()
                .map_err(|e| GifError::DecodeError(e.to_string()))?
            else {
                break;
            };
            buffer = vec![0u8; frame_bytes];
            decode_frame_rgba(frame, global_palette, &mut buffer)?;
            cursor.next_idx += 1;
        }

        if buffer.len() != frame_bytes {
            return Err(GifError::DecodeError(format!("кадр {idx} недостижим")));
        }

        let image = Image {
            width: self.width,
            height: self.height,
            format: PixelFormat::Rgba8,
            data: buffer,
            icc_profile: None,
        };
        cursor.last = Some((idx, image.clone()));
        Ok(image)
    }

    /// Возвращает пиксели кадра для `elapsed_ms` миллисекунд от начала анимации.
    ///
    /// # Errors
    /// - [`GifError::DecodeError`] — ошибка декодера или недостижимый кадр.
    pub fn frame_at(&self, elapsed_ms: u64) -> Result<Image, GifError> {
        self.frame_image(self.frame_index_at(elapsed_ms))
    }
}

/// Декодирует GIF файл и возвращает первый кадр.
///
/// Для анимированных GIF используйте [`decode_gif_animated`] — эта функция
/// возвращает только первый кадр (frame 0).
///
/// # Errors
/// - [`GifError::InvalidSignature`] — не валидная GIF сигнатура.
/// - [`GifError::DecodeError`] — ошибка при парсинге GIF структуры.
/// - [`GifError::NoFrames`] — GIF не содержит кадров.
pub fn decode_gif(bytes: &[u8]) -> Result<Image, GifError> {
    decode_gif_animated(bytes)?.frame_image(0)
}

/// Декодирует метаданные GIF (размер, цикличность, per-frame задержки) и возвращает
/// [`AnimatedGif`] с **ленивым** декодированием пиксельных кадров.
///
/// Кадры не материализуются в память при загрузке: проход идёт по контейнеру и собирает лишь
/// задержки, LZW-данные при этом не распаковываются вовсе (см. [`container_options`]), а сами
/// кадры декодируются по запросу через [`AnimatedGif::frame_image`]. Пиксели в RGBA8 выдаёт
/// [`decode_frame_rgba`] — палитра, transparent-index и deinterlace применяются им же
/// (BUG-787: `gif::FrameDecoder` зависал на кадре с оборванным LZW-потоком).
///
/// # Shell integration handoff
/// Шелл вызывает `gif.frame_index_at(elapsed_ms)` на каждом render-тике, и при смене индекса —
/// `gif.frame_image(idx)`, передавая пиксели в `DrawImage`. Forward-воспроизведение стоит один
/// декод кадра на переход (курсор держит декодер живым).
///
/// # Errors
/// - [`GifError::InvalidSignature`] — не валидная GIF сигнатура.
/// - [`GifError::DecodeError`] — ошибка при парсинге GIF структуры.
/// - [`GifError::NoFrames`] — GIF не содержит кадров.
pub fn decode_gif_animated(bytes: &[u8]) -> Result<AnimatedGif, GifError> {
    if !is_gif(bytes) {
        return Err(GifError::InvalidSignature);
    }

    let encoded: Arc<[u8]> = Arc::from(bytes);

    let mut reader = container_options()
        .read_info(Cursor::new(Arc::clone(&encoded)))
        .map_err(|e| GifError::DecodeError(e.to_string()))?;

    let width = u32::from(reader.width());
    let height = u32::from(reader.height());

    if width == 0 || height == 0 {
        return Err(GifError::DecodeError("нулевой размер GIF".to_string()));
    }

    let loop_count = match reader.repeat() {
        gif::Repeat::Finite(n) => GifLoopCount::Finite(n),
        gif::Repeat::Infinite => GifLoopCount::Infinite,
    };

    // Metadata pass: walk the container to record every frame's delay. `skip_frame_decoding`
    // means the LZW payload is stepped over, not unpacked — no pixel buffer is allocated here
    // at all, and a frame whose trailing LZW bits the decoder cannot parse (BUG-396) does not
    // sink the whole animation.
    let mut delays_cs = Vec::new();

    while let Some(frame) = reader
        .next_frame_info()
        .map_err(|e| GifError::DecodeError(e.to_string()))?
    {
        delays_cs.push(frame.delay);
    }

    if delays_cs.is_empty() {
        return Err(GifError::NoFrames);
    }

    Ok(AnimatedGif {
        encoded,
        width,
        height,
        loop_count,
        delays_cs,
        cursor: Mutex::new(None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_gif ───────────────────────────────────────────────────────────────

    #[test]
    fn gif_signature_87a_detected() {
        let bytes = b"GIF87a\x00\x00\x00\x00\x00\x00";
        assert!(is_gif(bytes), "GIF87a должен быть распознан");
    }

    #[test]
    fn gif_signature_89a_detected() {
        let bytes = b"GIF89a\x00\x00\x00\x00\x00\x00";
        assert!(is_gif(bytes), "GIF89a должен быть распознан");
    }

    #[test]
    fn not_gif_signature_rejected() {
        let bytes = b"NOTGIF\x00\x00\x00\x00\x00\x00";
        assert!(!is_gif(bytes), "не-GIF должен быть отклонён");
    }

    #[test]
    fn short_bytes_rejected() {
        let bytes = b"GIF87";
        assert!(!is_gif(bytes), "слишком короткие байты должны быть отклонены");
    }

    // ── decode_gif / decode_gif_animated — error paths ──────────────────────

    #[test]
    fn invalid_signature_error_in_decode() {
        let bytes = b"NOTGIF\x00\x00\x00\x00\x00\x00";
        match decode_gif(bytes) {
            Err(GifError::InvalidSignature) => {}
            r => panic!("ожидалась InvalidSignature, получено {r:?}"),
        }
    }

    #[test]
    fn malformed_gif_decode_error() {
        let bytes = b"GIF87a\xFF\xFF\xFF";
        match decode_gif(bytes) {
            Err(GifError::DecodeError(_)) => {}
            r => panic!("ожидалась DecodeError, получено {r:?}"),
        }
    }

    #[test]
    fn invalid_signature_error_in_decode_animated() {
        let bytes = b"NOTGIF\x00\x00\x00\x00\x00\x00";
        assert!(matches!(decode_gif_animated(bytes), Err(GifError::InvalidSignature)));
    }

    // ── delay_cs_to_ms ───────────────────────────────────────────────────────

    #[test]
    fn delay_ms_nonzero() {
        assert_eq!(delay_cs_to_ms(10), 100); // 10 cs = 100 ms
    }

    #[test]
    fn delay_ms_zero_treated_as_100ms() {
        assert_eq!(delay_cs_to_ms(0), 100); // 10 cs fallback × 10 ms
    }

    #[test]
    fn delay_ms_large() {
        assert_eq!(delay_cs_to_ms(100), 1000); // 100 cs = 1000 ms
    }

    // ── frame_index_for (pure timing math) ───────────────────────────────────

    // frame0=100ms, frame1=200ms, frame2=300ms → total 600ms
    const THREE_INFINITE: [u16; 3] = [10, 20, 30];

    #[test]
    fn frame_index_at_start() {
        assert_eq!(frame_index_for(&THREE_INFINITE, GifLoopCount::Infinite, 0), 0);
    }

    #[test]
    fn frame_index_at_middle_of_first() {
        assert_eq!(frame_index_for(&THREE_INFINITE, GifLoopCount::Infinite, 50), 0);
    }

    #[test]
    fn frame_index_at_boundary_second() {
        // frame0 = 100 ms; frame1 starts at 100 ms
        assert_eq!(frame_index_for(&THREE_INFINITE, GifLoopCount::Infinite, 100), 1);
    }

    #[test]
    fn frame_index_at_boundary_third() {
        // frame0=100 + frame1=200 = 300 ms → frame2
        assert_eq!(frame_index_for(&THREE_INFINITE, GifLoopCount::Infinite, 300), 2);
    }

    #[test]
    fn frame_index_loops_infinite() {
        // total = 600 ms; at 600 ms wraps back to frame 0
        assert_eq!(frame_index_for(&THREE_INFINITE, GifLoopCount::Infinite, 600), 0);
        assert_eq!(frame_index_for(&THREE_INFINITE, GifLoopCount::Infinite, 650), 0);
        assert_eq!(frame_index_for(&THREE_INFINITE, GifLoopCount::Infinite, 700), 1);
    }

    #[test]
    fn frame_index_finite_one_loop_clamps() {
        // total = 300 ms, 1 loop → stops at last frame after 300 ms
        let d = [10u16, 20];
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(1), 0), 0);
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(1), 100), 1);
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(1), 1_000_000), 1);
    }

    #[test]
    fn frame_index_finite_two_loops() {
        // each frame 100 ms; 2 loops = 400 ms total
        let d = [10u16, 10];
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(2), 0), 0);
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(2), 100), 1);
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(2), 200), 0); // loop 2 starts
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(2), 300), 1);
        assert_eq!(frame_index_for(&d, GifLoopCount::Finite(2), 500), 1); // clamped past end
    }

    #[test]
    fn frame_index_empty_returns_zero() {
        assert_eq!(frame_index_for(&[], GifLoopCount::Infinite, 0), 0);
        assert_eq!(frame_index_for(&[], GifLoopCount::Infinite, 99999), 0);
    }

    // ── GifLoopCount ─────────────────────────────────────────────────────────

    #[test]
    fn loop_count_finite_eq() {
        assert_eq!(GifLoopCount::Finite(3), GifLoopCount::Finite(3));
        assert_ne!(GifLoopCount::Finite(3), GifLoopCount::Finite(4));
    }

    #[test]
    fn loop_count_infinite_ne_finite() {
        assert_ne!(GifLoopCount::Infinite, GifLoopCount::Finite(0));
    }

    // ── lazy decode round-trip on a real synthetic GIF ───────────────────────

    /// Encodes a 2×1, two-frame GIF: frame0 = [red, green], frame1 = [blue, yellow],
    /// delays 10 cs / 20 cs, infinite loop. Each frame has ≤2 distinct colours so the
    /// RGBA round-trip through the palette is lossless.
    fn two_frame_gif() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = gif::Encoder::new(&mut out, 2, 1, &[]).expect("encoder");
            enc.set_repeat(gif::Repeat::Infinite).expect("repeat");

            let mut px0 = [255u8, 0, 0, 255, 0, 255, 0, 255];
            let mut f0 = gif::Frame::from_rgba(2, 1, &mut px0);
            f0.delay = 10;
            enc.write_frame(&f0).expect("frame0");

            let mut px1 = [0u8, 0, 255, 255, 255, 255, 0, 255];
            let mut f1 = gif::Frame::from_rgba(2, 1, &mut px1);
            f1.delay = 20;
            enc.write_frame(&f1).expect("frame1");
        }
        out
    }

    #[test]
    fn lazy_metadata_decoded_without_pixels() {
        let bytes = two_frame_gif();
        let gif = decode_gif_animated(&bytes).expect("decode");
        assert_eq!(gif.frame_count(), 2);
        assert_eq!(gif.width, 2);
        assert_eq!(gif.height, 1);
        assert_eq!(gif.loop_count, GifLoopCount::Infinite);
        assert_eq!(gif.total_cycle_ms(), 300); // 100 + 200
        assert_eq!(gif.frame_delay_ms(0), 100);
        assert_eq!(gif.frame_delay_ms(1), 200);
        // No frame has been materialised yet → resident memory is just encoded bytes.
        assert_eq!(gif.resident_bytes(), bytes.len());
    }

    #[test]
    fn lazy_frame_pixels_match_source() {
        let bytes = two_frame_gif();
        let gif = decode_gif_animated(&bytes).expect("decode");

        let f0 = gif.frame_image(0).expect("frame0");
        assert_eq!(f0.width, 2);
        assert_eq!(f0.height, 1);
        assert_eq!(f0.data, vec![255, 0, 0, 255, 0, 255, 0, 255]);

        let f1 = gif.frame_image(1).expect("frame1");
        assert_eq!(f1.data, vec![0, 0, 255, 255, 255, 255, 0, 255]);
    }

    #[test]
    fn lazy_backward_access_resets_cursor() {
        let bytes = two_frame_gif();
        let gif = decode_gif_animated(&bytes).expect("decode");

        // Forward then backward (loop wrap) — cursor must reset and still be correct.
        let f1 = gif.frame_image(1).expect("frame1");
        let f0 = gif.frame_image(0).expect("frame0 after reset");
        assert_eq!(f0.data, vec![255, 0, 0, 255, 0, 255, 0, 255]);
        assert_eq!(f1.data, vec![0, 0, 255, 255, 255, 255, 0, 255]);

        // Repeated request for the same frame is served from cache, identical bytes.
        let f0_again = gif.frame_image(0).expect("frame0 cached");
        assert_eq!(f0_again.data, f0.data);
    }

    #[test]
    fn lazy_out_of_range_clamps_to_last() {
        let bytes = two_frame_gif();
        let gif = decode_gif_animated(&bytes).expect("decode");
        let clamped = gif.frame_image(99).expect("clamped");
        let last = gif.frame_image(1).expect("last");
        assert_eq!(clamped.data, last.data);
    }

    #[test]
    fn frame_at_returns_correct_frame() {
        let bytes = two_frame_gif();
        let gif = decode_gif_animated(&bytes).expect("decode");
        // at 100 ms → frame 1
        let frame = gif.frame_at(100).expect("frame_at");
        assert_eq!(frame.data, vec![0, 0, 255, 255, 255, 255, 0, 255]);
    }

    #[test]
    fn decode_gif_returns_first_frame() {
        let bytes = two_frame_gif();
        let img = decode_gif(&bytes).expect("first frame");
        assert_eq!(img.data, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    }

    // ── BUG-396: early-change LZW tail / frame without a preceding GCE ───────

    /// Байт-в-байт `tests/wpt/gif/reset-no-gce.gif` (upstream WPT, 90 байт): 2×2, три кадра,
    /// глобальная палитра `[чёрный, красный, зелёный, синий]`.
    ///
    /// * кадр 0 — GCE `disposal=2`, `transparent_index=0` → сплошной красный, непрозрачный;
    /// * кадр 1 — GCE `disposal=2`, `transparent_index=2` → залит зелёным = прозрачный целиком;
    /// * кадр 2 — **GCE отсутствует** (это и проверяет upstream) → прозрачного индекса нет,
    ///   сплошной зелёный, непрозрачный.
    ///
    /// LZW-данные всех трёх кадров (`8c a3 00` / `94 a5 00`) энкодер записал по early-change-
    /// правилу: ширина кода растёт на один код раньше, чем её наращивает `weezl`, поэтому
    /// хвостовые биты за последним пикселем читаются как несуществующий код.
    const RESET_NO_GCE_GIF: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
        0x02, 0x00, 0x02, 0x00, 0xf1, 0x00, 0x00, // 2×2, GCT из 4 цветов
        0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0xff, // палитра
        0x21, 0xf9, 0x04, 0x09, 0x14, 0x00, 0x00, 0x00, // GCE кадра 0
        0x2c, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, // дескриптор кадра 0
        0x02, 0x03, 0x8c, 0xa3, 0x00, 0x00, // LZW кадра 0
        0x21, 0xf9, 0x04, 0x09, 0x14, 0x00, 0x02, 0x00, // GCE кадра 1
        0x2c, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, // дескриптор кадра 1
        0x02, 0x03, 0x94, 0xa5, 0x00, 0x00, // LZW кадра 1
        0x2c, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, // дескриптор кадра 2, без GCE
        0x02, 0x03, 0x94, 0xa5, 0x00, 0x00, // LZW кадра 2
        0x3b, // trailer
    ];

    #[test]
    fn bug396_early_change_lzw_tail_does_not_kill_animation() {
        let gif = decode_gif_animated(RESET_NO_GCE_GIF).expect("три кадра должны декодироваться");
        assert_eq!(gif.frame_count(), 3, "кадр без GCE тоже считается");
        assert_eq!((gif.width, gif.height), (2, 2));
        // Кадры 0 и 1 несут delay=20 cs, у кадра 2 GCE нет → delay=0 → браузерные 100 мс.
        assert_eq!(gif.frame_delay_ms(0), 200);
        assert_eq!(gif.frame_delay_ms(1), 200);
        assert_eq!(gif.frame_delay_ms(2), 100);
    }

    #[test]
    fn bug396_frame_without_gce_has_no_transparent_index() {
        let gif = decode_gif_animated(RESET_NO_GCE_GIF).expect("decode");

        let f0 = gif.frame_image(0).expect("кадр 0");
        assert_eq!(f0.data, [255, 0, 0, 255].repeat(4), "кадр 0 — красный непрозрачный");

        // transparent_index=2 совпал с заливкой → все четыре пикселя прозрачны.
        let f1 = gif.frame_image(1).expect("кадр 1");
        assert!(
            f1.data.chunks_exact(4).all(|px| px[3] == 0),
            "кадр 1 должен быть полностью прозрачным, получено {:?}",
            f1.data
        );

        // Ключ теста: у кадра 2 нет предшествующего GCE, значит прозрачного индекса нет —
        // тот же зелёный пиксель обязан остаться непрозрачным.
        let f2 = gif.frame_image(2).expect("кадр 2");
        assert_eq!(f2.data, [0, 255, 0, 255].repeat(4), "кадр 2 — зелёный непрозрачный");
    }

    // ── BUG-787: LZW-поток кадра кончается раньше, чем заполнен кадр ────────

    /// Минимизированное (`cargo fuzz tmin`, 78 байт) репро BUG-787: синтаксически
    /// правдоподобный GIF89a 1×1, у которого первый же LZW-код кадра — end code.
    ///
    /// Кадр объявлен размером в один пиксель, а поток за ним не выдаёт ни одного индекса:
    /// `35 44` при `min_code_size = 2` читается как код 5 = END (`CLEAR = 4`, `END = 5`).
    /// В `gif` 0.14.2 такой кадр вешает `decode_lzw_encoded_frame_into_buffer` навсегда
    /// (см. комментарий у [`lzw_decode_into`]); здесь он обязан дать ошибку и вернуться.
    const LZW_ENDS_EARLY_GIF: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // "GIF89a"
        0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, // 1×1, GCT из 2 цветов
        0x00, 0x7f, 0x00, 0x00, 0x00, 0x00, // палитра
        0x21, 0xf9, 0x04, 0x60, 0x07, 0x00, 0xff, 0x00, // GCE
        0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, // дескриптор кадра
        0x02, 0x02, 0x35, 0x44, // LZW: min code size 2, подблок из двух байт
        0x01, 0x20, // подблок из одного байта
        0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, //
        0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, //
        0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, // хвост подблока
        0x00, // терминатор блоков
        0x3b, // trailer
    ];

    #[test]
    fn bug787_frame_with_truncated_lzw_stream_errors_instead_of_hanging() {
        // Контейнерный проход обязан отработать: кадр объявлен, метаданные читаются.
        let gif = decode_gif_animated(LZW_ENDS_EARLY_GIF).expect("метаданные декодируются");
        assert_eq!(gif.frame_count(), 1);
        assert_eq!((gif.width, gif.height), (1, 1));

        // Ключ теста: пиксельный проход обязан ВЕРНУТЬСЯ, притом ошибкой.
        match gif.frame_image(0) {
            Err(GifError::DecodeError(msg)) => {
                assert!(msg.contains("оборван"), "ожидалось сообщение об обрыве, получено {msg}");
            }
            r => panic!("ожидалась DecodeError, получено {r:?}"),
        }
        match decode_gif(LZW_ENDS_EARLY_GIF) {
            Err(GifError::DecodeError(_)) => {}
            r => panic!("ожидалась DecodeError из decode_gif, получено {r:?}"),
        }
        // Точка входа фаззера (`fuzz/fuzz_targets/fuzz_image.rs`) — ровно она и висла;
        // CI проигрывает те же байты из `fuzz/regressions/fuzz_image-gif-lzw-hang`.
        assert!(
            crate::decode(LZW_ENDS_EARLY_GIF).is_err(),
            "lumen_image::decode обязан вернуть ошибку, а не крутиться"
        );
    }

    // ── чересстрочные кадры (deinterlace) ───────────────────────────────────

    #[test]
    fn interlace_row_order_matches_spec_passes() {
        // Порядок из GIF spec §20.c.ii, сверен с `gif::reader::converter::InterlaceIterator`.
        assert_eq!(interlace_row_order(1), vec![0]);
        assert_eq!(interlace_row_order(3), vec![0, 2, 1]);
        assert_eq!(interlace_row_order(5), vec![0, 4, 2, 1, 3]);
        assert_eq!(interlace_row_order(8), vec![0, 4, 2, 6, 1, 3, 5, 7]);
        assert_eq!(interlace_row_order(9), vec![0, 8, 4, 2, 6, 1, 3, 5, 7]);
        assert_eq!(interlace_row_order(11), vec![0, 8, 4, 2, 6, 10, 1, 3, 5, 7, 9]);
        // Перестановка: каждая строка встречается ровно один раз.
        let mut sorted = interlace_row_order(23);
        sorted.sort_unstable();
        assert_eq!(sorted, (0..23).collect::<Vec<_>>());
    }

    #[test]
    fn interlaced_frame_rows_land_in_screen_order() {
        // 1×8, глобальная палитра из 8 оттенков красного: индекс i → (i, 0, 0).
        let palette: Vec<u8> = (0..8u8).flat_map(|i| [i, 0, 0]).collect();
        // Пиксели идут в порядке ПОТОКА, а декодер обязан разложить их по экранным строкам:
        // на i-й позиции потока лежит индекс, равный её экранной строке.
        let stream_pixels: Vec<u8> = interlace_row_order(8)
            .into_iter()
            .map(|row| u8::try_from(row).expect("строка < 8"))
            .collect();
        assert_eq!(stream_pixels, vec![0, 4, 2, 6, 1, 3, 5, 7]);

        let mut out = Vec::new();
        {
            let mut enc = gif::Encoder::new(&mut out, 1, 8, &palette).expect("encoder");
            let mut frame = gif::Frame::from_indexed_pixels(1, 8, stream_pixels, None);
            frame.interlaced = true;
            enc.write_frame(&frame).expect("frame");
        }

        let gif = decode_gif_animated(&out).expect("decode");
        let img = gif.frame_image(0).expect("кадр 0");
        let expected: Vec<u8> = (0..8u8).flat_map(|row| [row, 0, 0, 255]).collect();
        assert_eq!(img.data, expected, "строки чересстрочного кадра должны встать по местам");
    }

    #[test]
    fn frame_pixel_outside_palette_stays_transparent() {
        // Палитра из двух цветов, а кадр ссылается на индекс 3 — его в палитре нет
        // (индекс влезает в min code size 2, но за таблицу цветов выходит). Меньше двух
        // записей палитру брать нельзя: `gif::Encoder` дополняет её до степени двойки,
        // и «отсутствующий» индекс попал бы в дописанный нулевой цвет.
        let palette: Vec<u8> = vec![9, 9, 9, 1, 2, 3];
        let mut out = Vec::new();
        {
            let mut enc = gif::Encoder::new(&mut out, 2, 1, &palette).expect("encoder");
            let frame = gif::Frame::from_indexed_pixels(2, 1, vec![0u8, 3], None);
            enc.write_frame(&frame).expect("frame");
        }
        let gif = decode_gif_animated(&out).expect("decode");
        let img = gif.frame_image(0).expect("кадр 0");
        assert_eq!(img.data, vec![9, 9, 9, 255, 0, 0, 0, 0], "пиксель вне палитры — прозрачный");
    }

    #[test]
    fn animated_gif_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AnimatedGif>();
    }
}

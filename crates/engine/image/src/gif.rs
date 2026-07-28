use std::io::Cursor;
use std::sync::{Arc, Mutex};
use gif::{DecodeOptions, Decoder};
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

/// Ленивое состояние декодера: живой forward-only `gif::Decoder` над `Arc<[u8]>`-байтами,
/// его позиция и кэш последнего выданного кадра.
///
/// GIF-кадры взаимозависимы (disposal composited поверх предыдущих), поэтому произвольный
/// доступ к кадру `N` требует последовательного декода `0..=N`. Курсор держит декодер живым,
/// чтобы forward-воспроизведение стоило один декод кадра на переход, а не `O(N)` каждый раз.
/// При запросе кадра «позади» курсора (wrap на 0 в цикле или обратный seek) декодер
/// пересоздаётся с начала.
struct GifCursor {
    /// Живой декодер, спозиционированный так, что следующий читаемый кадр имеет индекс `next_idx`.
    /// В `Box`, чтобы объёмный `gif::Decoder` не раздувал `AnimatedGif` при простое (курсор `None`
    /// всё равно резервирует место под самый большой вариант `Option`).
    reader: Box<Decoder<Cursor<Arc<[u8]>>>>,
    /// Индекс следующего кадра, который выдаст `reader` (число уже прочитанных кадров).
    next_idx: usize,
    /// Кэш последнего выданного кадра `(индекс, пиксели)` — обслуживает повторный запрос
    /// того же кадра без пересоздания декодера.
    last: Option<(usize, Image)>,
}

impl GifCursor {
    /// Создаёт новый forward-декодер с позиции нулевого кадра.
    fn new(encoded: &Arc<[u8]>) -> Result<Self, GifError> {
        let mut options = DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let reader = options
            .read_info(Cursor::new(Arc::clone(encoded)))
            .map_err(|e| GifError::DecodeError(e.to_string()))?;
        Ok(Self { reader: Box::new(reader), next_idx: 0, last: None })
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
            let has_frame = cursor
                .reader
                .next_frame_info()
                .map_err(|e| GifError::DecodeError(e.to_string()))?
                .is_some();
            if !has_frame {
                break;
            }
            buffer = vec![0u8; frame_bytes];
            cursor
                .reader
                .read_into_buffer(&mut buffer)
                .map_err(|e| GifError::DecodeError(e.to_string()))?;
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
/// Кадры не материализуются в память при загрузке: за один проход собираются лишь задержки
/// (пиксели проходного декода сразу отбрасываются, пиковая память здесь — один кадр, а не вся
/// анимация), а сами кадры декодируются по запросу через [`AnimatedGif::frame_image`].
/// Использует `gif` крейт с `ColorOutput::RGBA` — палитра и disposal обрабатываются
/// автоматически; каждый кадр разворачивается в полный экранный прямоугольник `width × height`.
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

    let mut options = DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);

    let mut reader = options
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

    // Metadata pass: iterate every frame to record its delay. Pixels are decoded into a single
    // reused buffer and discarded, so peak memory here is one frame, not the whole animation.
    let frame_bytes = (width * height * 4) as usize;
    let mut discard = vec![0u8; frame_bytes];
    let mut delays_cs = Vec::new();

    while let Some(frame) = reader
        .next_frame_info()
        .map_err(|e| GifError::DecodeError(e.to_string()))?
    {
        delays_cs.push(frame.delay);
        reader
            .read_into_buffer(&mut discard)
            .map_err(|e| GifError::DecodeError(e.to_string()))?;
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

    #[test]
    fn animated_gif_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AnimatedGif>();
    }
}

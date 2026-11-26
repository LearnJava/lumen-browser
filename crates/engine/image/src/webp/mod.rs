//! WebP декодер (VP8 lossy + VP8L lossless) поверх `image-webp` 0.2.
//!
//! Поддерживает:
//! - VP8 (lossy, baseline) → YUV → RGBA8
//! - VP8L (lossless) → RGBA8
//! - Анимированные WebP (ANIM/ANMF): статический путь декодирует первый кадр,
//!   проигрывание всей анимации — [`AnimatedWebp`] (BUG-1853).

use std::io::{BufReader, Cursor};
use std::sync::{Arc, Mutex};

use image_webp::WebPDecoder;

use crate::LoopCount;
use crate::{Image, PixelFormat};

/// Начало WebP-контейнера: первые 4 байта файла.
pub const WEBP_RIFF: [u8; 4] = *b"RIFF";

/// Магическое слово на байтах 8–11 WebP-файла: `WEBP`.
pub const WEBP_TAG: [u8; 4] = *b"WEBP";

/// Ошибка декодирования WebP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebpError(pub String);

impl core::fmt::Display for WebpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WebpError {}

/// Проверяет WebP-сигнатуру без полной валидации.
///
/// Формат контейнера RIFF: байты 0–3 = `RIFF`, байты 8–11 = `WEBP`.
/// Размер поля (байты 4–7) не проверяется.
#[must_use]
pub fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes[..4] == WEBP_RIFF && bytes[8..12] == WEBP_TAG
}

/// Декодирует WebP-файл в RGBA8 (4 байта на пиксель, row-major).
///
/// Возвращает `(ширина, высота, rgba8_data)`.
///
/// Phase 0: анимированные WebP — читается только первый кадр (frame 0);
/// полная анимация запланирована на Wave 3.
///
/// # Errors
/// [`WebpError`] с текстом диагностики.
pub fn decode_webp(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), WebpError> {
    let reader = BufReader::new(Cursor::new(bytes));
    let mut decoder =
        WebPDecoder::new(reader).map_err(|e| WebpError(format!("WebP header: {e}")))?;

    let (width, height) = decoder.dimensions();
    let has_alpha = decoder.has_alpha();

    let buf_size = decoder
        .output_buffer_size()
        .ok_or_else(|| WebpError("WebP: нулевые размеры изображения".to_string()))?;

    let mut raw = vec![0u8; buf_size];
    decoder
        .read_image(&mut raw)
        .map_err(|e| WebpError(format!("WebP decode: {e}")))?;

    // image-webp отдаёт RGB8 если нет альфа-канала; нормализуем в RGBA8.
    let rgba8 = if has_alpha {
        raw
    } else {
        let mut out = Vec::with_capacity(width as usize * height as usize * 4);
        for rgb in raw.chunks_exact(3) {
            out.extend_from_slice(rgb);
            out.push(255);
        }
        out
    };

    Ok((width, height, rgba8))
}

/// Возвращает беззнаковое 24-битное little-endian число из трёх байтов.
#[must_use]
const fn read_u24_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8
}

/// Проходит по RIFF-чанкам файла и собирает длительность каждого `ANMF`-кадра
/// в миллисекундах (поле Duration — байты 12–15 заголовка кадра, u24 LE).
///
/// `image-webp` отдаёт только суммарную `loop_duration()`, а `frame_index_at`
/// нужны per-frame задержки; контейнерный проход стоит O(чанков) и пикселей не
/// касается. Нечётные размеры чанка дополняются выравнивающим байтом (RIFF §Spec).
#[must_use]
fn scan_anmf_durations_ms(bytes: &[u8]) -> Vec<u32> {
    let mut delays = Vec::new();
    let file_len = bytes.len();
    // Граница RIFF-контейнера: 8 байт заголовка + размер из поля Size (не верим,
    // если он вылезает за файл — режем по фактической длине).
    let riff_end = file_len.min(8 + usize::try_from(read_u32_le(bytes, 4)).unwrap_or(usize::MAX));
    let mut pos = 12usize;
    while pos + 8 <= riff_end {
        let size = read_u32_le(bytes, pos + 4) as usize;
        let payload = pos + 8;
        if payload > riff_end {
            break;
        }
        if &bytes[pos..pos + 4] == b"ANMF" && size >= 16 && payload + 16 <= file_len {
            delays.push(read_u24_le(&bytes[payload + 12..payload + 15]));
        }
        pos = payload + size + (size & 1);
    }
    delays
}

/// Читает little-endian `u32` из `bytes[offset..offset + 4]` (без паник на коротких хвостах).
const fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    if bytes.len() < offset + 4 {
        return 0;
    }
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

/// Возвращает индекс кадра для `elapsed_ms` по массиву задержек `delays_ms`.
///
/// Чистая функция над метаданными таймингов — webp-аналог `gif::frame_index_for`
/// (BUG-1853), отличия: задержки уже в миллисекундах (WebP хранит их честно,
/// без gif-квирка «0 cs → 100 ms»), кадр с нулевой задержкой не отображается
/// (условие `effective_ms < acc` перепрыгивает его).
///
/// - [`LoopCount::Infinite`] — время берётся по модулю суммарной длительности.
/// - [`LoopCount::Finite(n)`] — после `n` повторений останавливается на последнем кадре.
/// - Пустой `delays_ms` или нулевой суммарный цикл → всегда 0.
#[must_use]
fn frame_index_for_ms(delays_ms: &[u32], loop_count: LoopCount, elapsed_ms: u64) -> usize {
    if delays_ms.is_empty() {
        return 0;
    }
    let total_ms: u64 = delays_ms.iter().copied().map(u64::from).sum();
    if total_ms == 0 {
        return 0;
    }

    let effective_ms = match loop_count {
        LoopCount::Infinite => elapsed_ms % total_ms,
        LoopCount::Finite(n) => {
            let max_ms = total_ms.saturating_mul(u64::from(n));
            if elapsed_ms >= max_ms {
                return delays_ms.len() - 1;
            }
            elapsed_ms % total_ms
        }
    };

    let mut acc = 0u64;
    for (i, &d) in delays_ms.iter().enumerate() {
        acc += u64::from(d);
        if effective_ms < acc {
            return i;
        }
    }
    delays_ms.len() - 1
}

/// Ленивое состояние декодера анимированного WebP: живой forward-only
/// `image_webp::WebPDecoder` над `Arc<[u8]>`-байтами и кэш последнего кадра.
///
/// Кадры анимации композитятся декодером во внутренний канвас, поэтому доступ к
/// кадру `N` требует последовательного декода `0..=N` — курсор держит декодер
/// живым, чтобы forward-переход стоил один декод кадра (как GIF-курсор `crate::gif`).
/// При запросе кадра «позади» курсора декодер сбрасывается `reset_animation()`.
struct WebpCursor {
    decoder: Box<WebPDecoder<Cursor<Arc<[u8]>>>>,
    /// Индекс следующего кадра, который выдаст `read_frame`.
    next_idx: usize,
    /// Кэш последнего выданного кадра `(индекс, пиксели)`.
    last: Option<(usize, Image)>,
}

/// Анимированный WebP с **ленивым** декодированием кадров (BUG-1853).
///
/// Зеркало [`crate::gif::AnimatedGif`]: хранятся закодированные байты (`Arc<[u8]>`),
/// per-frame задержки из ANMF-заголовков и счётчик повторений; пиксели кадра
/// декодируются по запросу через forward-курсор и держатся резидентно в объёме
/// ~одного кадра. Канвасная композитация кадров — внутри `image-webp`.
pub struct AnimatedWebp {
    /// Закодированные WebP-байты, разделяемые между клонами и курсорами.
    encoded: Arc<[u8]>,
    /// Ширина канваса анимации (VP8X), пикселей.
    pub width: u32,
    /// Высота канваса анимации (VP8X), пикселей.
    pub height: u32,
    /// Количество повторений анимации (ANIM loop).
    pub loop_count: LoopCount,
    /// Длительность каждого кадра в миллисекундах, в порядке отображения.
    /// Всегда непустой (гарантирует [`decode_webp_animated`]).
    delays_ms: Vec<u32>,
    /// Ленивое состояние декодера. `Mutex` даёт `Send + Sync` (анимация хранится
    /// за `Arc` и шарится между потоком-загрузчиком и UI); `None` до первого запроса.
    cursor: Mutex<Option<WebpCursor>>,
}

impl Clone for AnimatedWebp {
    /// Клонирует метаданные (Arc-указатель на байты + задержки); курсор не
    /// копируется — клон стартует с чистого состояния декодера.
    fn clone(&self) -> Self {
        Self {
            encoded: Arc::clone(&self.encoded),
            width: self.width,
            height: self.height,
            loop_count: self.loop_count,
            delays_ms: self.delays_ms.clone(),
            cursor: Mutex::new(None),
        }
    }
}

impl core::fmt::Debug for AnimatedWebp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AnimatedWebp")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("loop_count", &self.loop_count)
            .field("frame_count", &self.delays_ms.len())
            .field("encoded_len", &self.encoded.len())
            .finish()
    }
}

impl AnimatedWebp {
    /// Количество кадров анимации (всегда ≥ 1).
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.delays_ms.len()
    }

    /// Задержка кадра `idx` в миллисекундах. Индекс за границей клампится к последнему кадру.
    #[must_use]
    pub fn frame_delay_ms(&self, idx: usize) -> u64 {
        let idx = idx.min(self.delays_ms.len().saturating_sub(1));
        u64::from(self.delays_ms[idx])
    }

    /// Суммарная длительность одного прохода анимации в миллисекундах.
    #[must_use]
    pub fn total_cycle_ms(&self) -> u64 {
        self.delays_ms.iter().copied().map(u64::from).sum()
    }

    /// Резидентный объём памяти в байтах: закодированные байты плюс
    /// закэшированный в курсоре кадр (если есть) — зеркало [`crate::gif::AnimatedGif::resident_bytes`].
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
    #[must_use]
    pub fn frame_index_at(&self, elapsed_ms: u64) -> usize {
        frame_index_for_ms(&self.delays_ms, self.loop_count, elapsed_ms)
    }

    /// Декодирует и возвращает пиксели кадра `idx` (RGBA8, полный канвас
    /// `width × height` с применённой композитацией предыдущих кадров).
    ///
    /// Forward-запросы (`idx ≥` позиции курсора) стоят один декод кадра на переход;
    /// запрос кадра «позади» сбрасывает декодер и переигрывает с начала. Индекс за
    /// границей клампится к последнему кадру.
    ///
    /// # Errors
    /// [`WebpError`] — отравленный курсор или ошибка декодера.
    pub fn frame_image(&self, idx: usize) -> Result<Image, WebpError> {
        let idx = idx.min(self.delays_ms.len().saturating_sub(1));

        let mut guard = self
            .cursor
            .lock()
            .map_err(|_| WebpError("WebP-курсор отравлен".to_string()))?;

        if guard.is_none() {
            // Повторное открытие тех же проверенных байтов: декодер обязан
            // подняться, но паника здесь запрещена линтами — ошибка наружу.
            let decoder = WebPDecoder::new(Cursor::new(Arc::clone(&self.encoded)))
                .map_err(|e| WebpError(format!("WebP header: {e}")))?;
            *guard = Some(WebpCursor { decoder: Box::new(decoder), next_idx: 0, last: None });
        }
        let cursor = guard
            .as_mut()
            .ok_or_else(|| WebpError("WebP-курсор не создан".to_string()))?;

        // Запрос позади курсора — сброс и переигрывание с нулевого кадра.
        if cursor.next_idx > idx {
            cursor.decoder.reset_animation();
            cursor.next_idx = 0;
        }
        if cursor.last.as_ref().is_none_or(|(i, _)| *i != idx) {
            loop {
                let buf_size = cursor
                    .decoder
                    .output_buffer_size()
                    .ok_or_else(|| WebpError("WebP: нулевые размеры канваса".to_string()))?;
                let has_alpha = cursor.decoder.has_alpha();
                let mut raw = vec![0u8; buf_size];
                cursor
                    .decoder
                    .read_frame(&mut raw)
                    .map_err(|e| WebpError(format!("WebP decode: {e}")))?;
                let (w, h) = cursor.decoder.dimensions();
                let frame = if has_alpha {
                    raw
                } else {
                    let mut rgba = Vec::with_capacity(raw.len() / 3 * 4);
                    for rgb in raw.chunks_exact(3) {
                        rgba.extend_from_slice(rgb);
                        rgba.push(255);
                    }
                    rgba
                };
                cursor.last = Some((
                    cursor.next_idx,
                    Image {
                        width: w,
                        height: h,
                        format: PixelFormat::Rgba8,
                        data: frame,
                        icc_profile: None,
                        vector_source: None,
                    },
                ));
                cursor.next_idx += 1;
                if cursor.next_idx > idx {
                    break;
                }
            }
        }
        let img = match cursor.last.as_ref() {
            Some((_, img)) => img,
            None => return Err(WebpError("WebP: кадр не декодирован".to_string())),
        };
        Ok(img.clone())
    }

    /// Декодирует первый кадр (для начальной отрисовки до первого тика).
    ///
    /// # Errors
    /// [`WebpError`] — ошибка декодера.
    pub fn first_frame(&self) -> Result<Image, WebpError> {
        self.frame_image(0)
    }
}

/// Декодирует метаданные анимированного WebP без декода пикселей кадров.
///
/// Возвращает [`AnimatedWebp`] с ленивым курсором; пиксели выдаёт
/// [`AnimatedWebp::frame_image`]. Для неанимированного файла — `None`
/// (статический путь — [`decode_webp`]).
///
/// # Errors
/// [`WebpError`] — байты не открываются декодером, файл не анимирован,
/// контейнерный проход расходится с числом кадров декодера.
pub fn decode_webp_animated(bytes: &[u8]) -> Result<AnimatedWebp, WebpError> {
    let encoded: Arc<[u8]> = Arc::from(bytes);
    let decoder = WebPDecoder::new(BufReader::new(Cursor::new(&*encoded)))
        .map_err(|e| WebpError(format!("WebP header: {e}")))?;

    if !decoder.is_animated() {
        return Err(WebpError("WebP: файл не анимирован".to_string()));
    }
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(WebpError("WebP: нулевые размеры канваса".to_string()));
    }
    let loop_count = match decoder.loop_count() {
        image_webp::LoopCount::Forever => LoopCount::Infinite,
        image_webp::LoopCount::Times(n) => LoopCount::Finite(n.get()),
    };
    let num_frames = decoder.num_frames();

    // Per-frame задержки берём своим контейнерным проходом (image-webp отдаёт
    // только сумму); расхождение с числом кадров декодера — битый контейнер.
    let delays_ms = scan_anmf_durations_ms(&encoded);
    if delays_ms.len() != usize::try_from(num_frames).unwrap_or(usize::MAX) {
        return Err(WebpError(format!(
            "WebP: ANMF-проход нашёл {} кадров, декодер — {num_frames}",
            delays_ms.len()
        )));
    }

    Ok(AnimatedWebp {
        encoded,
        width,
        height,
        loop_count,
        delays_ms,
        cursor: Mutex::new(None),
    })
}

/// Многокадровая анимация: GIF ([`crate::gif::AnimatedGif`]) или WebP
/// ([`AnimatedWebp`]) — общий тип тика анимаций в shell (BUG-1853).
#[derive(Clone, Debug)]
pub enum AnimatedImage {
    /// Анимированный GIF.
    Gif(crate::gif::AnimatedGif),
    /// Анимированный WebP.
    Webp(AnimatedWebp),
}

/// Ошибка декодирования кадра [`AnimatedImage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimatedImageError {
    /// GIF-ветка.
    Gif(crate::gif::GifError),
    /// WebP-ветка.
    Webp(WebpError),
}

impl core::fmt::Display for AnimatedImageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Gif(e) => write!(f, "{e}"),
            Self::Webp(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AnimatedImageError {}

impl AnimatedImage {
    /// Количество кадров анимации (всегда ≥ 1).
    #[must_use]
    pub fn frame_count(&self) -> usize {
        match self {
            Self::Gif(a) => a.frame_count(),
            Self::Webp(a) => a.frame_count(),
        }
    }

    /// Ширина канваса анимации, пикселей.
    #[must_use]
    pub fn width(&self) -> u32 {
        match self {
            Self::Gif(a) => a.width,
            Self::Webp(a) => a.width,
        }
    }

    /// Высота канваса анимации, пикселей.
    #[must_use]
    pub fn height(&self) -> u32 {
        match self {
            Self::Gif(a) => a.height,
            Self::Webp(a) => a.height,
        }
    }

    /// Количество повторений анимации.
    #[must_use]
    pub fn loop_count(&self) -> crate::LoopCount {
        match self {
            Self::Gif(a) => a.loop_count,
            Self::Webp(a) => a.loop_count,
        }
    }

    /// Задержка кадра `idx` в миллисекундах (клампится к последнему кадру).
    #[must_use]
    pub fn frame_delay_ms(&self, idx: usize) -> u64 {
        match self {
            Self::Gif(a) => a.frame_delay_ms(idx),
            Self::Webp(a) => a.frame_delay_ms(idx),
        }
    }

    /// Суммарная длительность одного прохода анимации в миллисекундах.
    #[must_use]
    pub fn total_cycle_ms(&self) -> u64 {
        match self {
            Self::Gif(a) => a.total_cycle_ms(),
            Self::Webp(a) => a.total_cycle_ms(),
        }
    }

    /// Резидентный объём памяти в байтах (закодированные байты + кэш курсора).
    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        match self {
            Self::Gif(a) => a.resident_bytes(),
            Self::Webp(a) => a.resident_bytes(),
        }
    }

    /// Индекс кадра для `elapsed_ms` миллисекунд от начала анимации.
    #[must_use]
    pub fn frame_index_at(&self, elapsed_ms: u64) -> usize {
        match self {
            Self::Gif(a) => a.frame_index_at(elapsed_ms),
            Self::Webp(a) => a.frame_index_at(elapsed_ms),
        }
    }

    /// Декодирует и возвращает пиксели кадра `idx` (RGBA8, полный канвас).
    ///
    /// # Errors
    /// [`AnimatedImageError`] — ошибка декодера соответствующего формата.
    pub fn frame_image(&self, idx: usize) -> Result<Image, AnimatedImageError> {
        match self {
            Self::Gif(a) => a.frame_image(idx).map_err(AnimatedImageError::Gif),
            Self::Webp(a) => a.frame_image(idx).map_err(AnimatedImageError::Webp),
        }
    }

    /// Декодирует первый кадр (для начальной отрисовки до первого тика).
    ///
    /// # Errors
    /// [`AnimatedImageError`] — ошибка декодера соответствующего формата.
    pub fn first_frame(&self) -> Result<Image, AnimatedImageError> {
        self.frame_image(0)
    }
}

/// Реализация [`lumen_core::ext::ImageDecoder`] для WebP.
///
/// Подключается в `lumen-image::decode()` как дополнительный диспетчер и
/// регистрируется в `supported_mime_types()` для фильтрации `<source type>`.
pub struct WebpImageDecoder;

impl lumen_core::ext::ImageDecoder for WebpImageDecoder {
    fn format_name(&self) -> &'static str {
        "webp"
    }

    fn sniff(&self, bytes: &[u8]) -> bool {
        is_webp(bytes)
    }

    fn mime_types(&self) -> &'static [&'static str] {
        &["image/webp"]
    }

    fn decode_rgba8(
        &self,
        bytes: &[u8],
    ) -> std::result::Result<(u32, u32, Vec<u8>), String> {
        decode_webp(bytes).map_err(|e| e.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ext::ImageDecoder as _;

    // Minimal 1×1 RGBA WebP (VP8L lossless) generated with WebPEncoder.
    // Created once via `generate_fixture_webp()` below and hard-coded as bytes
    // so the test-suite has zero external file dependencies.
    fn make_1x1_rgba_webp(r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        use image_webp::{ColorType, WebPEncoder};
        let mut out = Vec::new();
        WebPEncoder::new(&mut out)
            .encode(&[r, g, b, a], 1, 1, ColorType::Rgba8)
            .expect("encode 1×1 WebP");
        out
    }

    fn make_4x4_rgb_webp() -> Vec<u8> {
        use image_webp::{ColorType, WebPEncoder};
        // 4×4 solid green (no alpha)
        let data: Vec<u8> = (0..16).flat_map(|_| [0u8, 200u8, 0u8]).collect();
        let mut out = Vec::new();
        WebPEncoder::new(&mut out)
            .encode(&data, 4, 4, ColorType::Rgb8)
            .expect("encode 4×4 RGB WebP");
        out
    }

    #[test]
    fn signature_positive() {
        let webp = make_1x1_rgba_webp(255, 0, 0, 255);
        assert!(is_webp(&webp), "закодированный WebP должен иметь RIFF/WEBP сигнатуру");
    }

    #[test]
    fn signature_negative_png() {
        let png_sig = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert!(!is_webp(png_sig));
    }

    #[test]
    fn signature_too_short() {
        assert!(!is_webp(b"RIFF"));
        assert!(!is_webp(b""));
    }

    #[test]
    fn decode_rgba_roundtrip() {
        let webp = make_1x1_rgba_webp(200, 100, 50, 128);
        let (w, h, rgba) = decode_webp(&webp).expect("decode 1×1 RGBA WebP");
        assert_eq!(w, 1);
        assert_eq!(h, 1);
        assert_eq!(rgba.len(), 4);
        // VP8L lossless → pixel-exact
        assert_eq!(rgba[0], 200, "R");
        assert_eq!(rgba[1], 100, "G");
        assert_eq!(rgba[2], 50, "B");
        assert_eq!(rgba[3], 128, "A");
    }

    #[test]
    fn decode_rgb_expands_to_rgba8() {
        let webp = make_4x4_rgb_webp();
        let (w, h, rgba) = decode_webp(&webp).expect("decode 4×4 RGB WebP");
        assert_eq!(w, 4);
        assert_eq!(h, 4);
        assert_eq!(rgba.len(), 4 * 4 * 4, "RGBA8: 4 bytes/pixel");
        // Every 4th byte (alpha) must be 255
        for i in (3..rgba.len()).step_by(4) {
            assert_eq!(rgba[i], 255, "alpha byte at {i} должен быть 255");
        }
    }

    #[test]
    fn decode_invalid_returns_error() {
        let result = decode_webp(b"not a webp at all");
        assert!(result.is_err(), "невалидные байты должны вернуть ошибку");
    }

    #[test]
    fn image_decoder_trait_sniff() {
        let webp = make_1x1_rgba_webp(0, 0, 255, 255);
        let dec = WebpImageDecoder;
        assert!(dec.sniff(&webp));
        assert!(!dec.sniff(b"not webp"));
    }

    #[test]
    fn image_decoder_trait_mime_types() {
        let dec = WebpImageDecoder;
        assert!(dec.mime_types().contains(&"image/webp"));
    }

    #[test]
    fn image_decoder_trait_decode_rgba8() {
        let webp = make_1x1_rgba_webp(10, 20, 30, 40);
        let dec = WebpImageDecoder;
        let (w, h, rgba) = dec.decode_rgba8(&webp).expect("decode via trait");
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba.len(), 4);
    }

    // --- Анимированный WebP (BUG-1853) ---

    /// RIFF-чанк: fourcc + LE-размер + полезная нагрузка + выравнивающий байт.
    fn riff_chunk(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + payload.len() + (payload.len() & 1));
        out.extend_from_slice(fourcc);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        if payload.len() & 1 == 1 {
            out.push(0);
        }
        out
    }

    /// Кодирует VP8L-кадр `size × size` сплошного цвета и вырезает из
    /// однокадрового файла сырой VP8L-чанк (энкодер image-webp анимацию не пишет).
    fn make_solid_vp8l_payload(size: u32, color: [u8; 4]) -> Vec<u8> {
        use image_webp::{ColorType, WebPEncoder};
        let data: Vec<u8> = (0..size * size).flat_map(|_| color).collect();
        let mut file = Vec::new();
        WebPEncoder::new(&mut file)
            .encode(&data, size, size, ColorType::Rgba8)
            .expect("encode VP8L frame");
        // file = "RIFF" + size + "WEBP" + [чанк]; чанк = fourcc + size + payload.
        let chunk_size =
            u32::from_le_bytes([file[16], file[17], file[18], file[19]]) as usize;
        assert_eq!(&file[12..16], b"VP8L", "энкодер должен выдать VP8L-чанк");
        file[20..20 + chunk_size].to_vec()
    }

    /// Собирает минимальный анимированный WebP: VP8X(anim+alpha, canvas `size²`)
    /// + ANIM(loop) + на каждый кадр ANMF(полный канвас, `duration_ms`, VP8L).
    fn make_animated_webp(size: u32, durations_ms: &[u32], colors: &[[u8; 4]], loop_count: u16) -> Vec<u8> {
        assert_eq!(durations_ms.len(), colors.len());
        let mut vp8x = vec![0x12u8, 0, 0, 0]; // flags: Animation | Alpha
        vp8x.extend_from_slice(&(size - 1).to_le_bytes()[..3]);
        vp8x.extend_from_slice(&(size - 1).to_le_bytes()[..3]);
        let mut body = riff_chunk(b"VP8X", &vp8x);
        let mut anim = Vec::new();
        anim.extend_from_slice(&[0, 0, 0, 0]); // фон: прозрачный чёрный
        anim.extend_from_slice(&loop_count.to_le_bytes());
        body.extend_from_slice(&riff_chunk(b"ANIM", &anim));
        for (&d, color) in durations_ms.iter().zip(colors) {
            let mut header = vec![0u8; 16]; // x=y=0, w-1/h-1 ниже, duration, flags=0
            header[6..9].copy_from_slice(&(size - 1).to_le_bytes()[..3]);
            header[9..12].copy_from_slice(&(size - 1).to_le_bytes()[..3]);
            header[12..15].copy_from_slice(&d.to_le_bytes()[..3]);
            let mut anmf = header;
            anmf.extend_from_slice(&riff_chunk(b"VP8L", &make_solid_vp8l_payload(size, *color)));
            body.extend_from_slice(&riff_chunk(b"ANMF", &anmf));
        }
        let mut file = Vec::new();
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(b"WEBP");
        file.extend_from_slice(&body);
        file
    }

    /// Допуск пиксельного сравнения в фикстурах: VP8L-декодер image-webp на
    /// синтетических многокадровых файлах стабильно округляет на ±1 (30 → 29).
    /// На реальном файле портала (BUG-1853, VP8+ALPH) декод Lumen совпал с PIL
    /// попиксельно — зонд 2026-11-26, 0 различий на 25 984 пикселях; тесты ниже
    /// проверяют механику анимации (кадры, курсор, часы), а не точность кодека.
    fn assert_pixel_close(got: &[u8], want: [u8; 4], label: &str) {
        for (i, (g, w)) in got[..4].iter().zip(want).enumerate() {
            assert!(
                g.abs_diff(w) <= 1,
                "{label}: канал {i} — {g} против {w}, весь пиксель {:?}",
                &got[..4]
            );
        }
    }

    #[test]
    fn animated_metadata_and_frames() {
        let webp = make_animated_webp(4, &[50, 70], &[[30, 30, 30, 255], [230, 230, 230, 255]], 0);
        let anim = decode_webp_animated(&webp).expect("decode animated");
        assert_eq!(anim.frame_count(), 2);
        assert_eq!((anim.width, anim.height), (4, 4));
        assert_eq!(anim.loop_count, LoopCount::Infinite);
        assert_eq!(anim.frame_delay_ms(0), 50);
        assert_eq!(anim.frame_delay_ms(1), 70);
        assert_eq!(anim.total_cycle_ms(), 120);

        let f0 = anim.frame_image(0).expect("frame 0");
        assert_eq!((f0.width, f0.height), (4, 4));
        assert_pixel_close(&f0.data, [30, 30, 30, 255], "кадр 0 — тёмно-серый");
        let f1 = anim.frame_image(1).expect("frame 1");
        assert_pixel_close(&f1.data, [230, 230, 230, 255], "кадр 1 — светло-серый");
        // Повторный запрос того же кадра — из кэша курсора, пиксели те же.
        assert_eq!(anim.frame_image(1).expect("cache"), f1);
    }

    #[test]
    fn animated_backward_seek_replays_from_start() {
        let webp = make_animated_webp(4, &[50, 70], &[[30, 30, 30, 255], [230, 230, 230, 255]], 0);
        let anim = decode_webp_animated(&webp).expect("decode animated");
        assert_pixel_close(&anim.frame_image(1).expect("f1").data, [230, 230, 230, 255], "f1");
        // Назад — сброс курсора и переигрывание с нулевого кадра.
        assert_pixel_close(&anim.frame_image(0).expect("f0").data, [30, 30, 30, 255], "f0");
        assert_pixel_close(
            &anim.frame_image(1).expect("f1 again").data,
            [230, 230, 230, 255],
            "f1 again",
        );
    }

    #[test]
    fn animated_clone_starts_fresh_cursor() {
        let webp = make_animated_webp(4, &[50], &[[90, 90, 90, 255]], 3);
        let anim = decode_webp_animated(&webp).expect("decode animated");
        assert_pixel_close(&anim.frame_image(0).expect("f0").data, [90, 90, 90, 255], "f0");
        let clone = anim.clone();
        assert_pixel_close(
            &clone.frame_image(0).expect("clone f0").data,
            [90, 90, 90, 255],
            "clone f0",
        );
        assert_eq!(clone.loop_count, LoopCount::Finite(3));
    }

    #[test]
    fn animated_static_file_rejected() {
        let webp = make_1x1_rgba_webp(1, 2, 3, 4);
        let err = decode_webp_animated(&webp).expect_err("статический файл не анимация");
        assert_eq!(err, WebpError("WebP: файл не анимирован".to_string()));
    }

    #[test]
    fn animated_frame_index_clock() {
        let webp = make_animated_webp(4, &[50, 70], &[[30, 30, 30, 255], [230, 230, 230, 255]], 0);
        let anim = decode_webp_animated(&webp).expect("decode animated");
        // Infinite: модуль суммарного цикла 120 мс; 250 % 120 = 10 → кадр 0.
        assert_eq!(anim.frame_index_at(30), 0);
        assert_eq!(anim.frame_index_at(60), 1);
        assert_eq!(anim.frame_index_at(120), 0);
        assert_eq!(anim.frame_index_at(250), 0);
    }

    #[test]
    fn animated_finite_loop_holds_last_frame() {
        let webp = make_animated_webp(4, &[50, 70], &[[30, 30, 30, 255], [230, 230, 230, 255]], 2);
        let anim = decode_webp_animated(&webp).expect("decode animated");
        // Цикл 120 мс × 2 = 240; после — держим последний кадр.
        assert_eq!(anim.frame_index_at(239), 1);
        assert_eq!(anim.frame_index_at(240), 1);
        assert_eq!(anim.frame_index_at(10_000), 1);
    }

    #[test]
    fn scan_anmf_durations_walks_container() {
        let webp = make_animated_webp(4, &[10, 20, 30], &[[1; 4], [2; 4], [3; 4]], 0);
        assert_eq!(scan_anmf_durations_ms(&webp), vec![10, 20, 30]);
        assert_eq!(scan_anmf_durations_ms(b"RIFF"), Vec::<u32>::new());
    }
}

use std::io::Cursor;
use gif::DecodeOptions;
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

/// Один кадр анимированного GIF.
#[derive(Debug, Clone)]
pub struct AnimatedFrame {
    /// Декодированное изображение кадра в RGBA8, полный экранный буфер `width × height`.
    pub image: Image,
    /// Задержка перед следующим кадром в сотых долях секунды (GIF spec §23.c.vi).
    /// 0 интерпретируется браузерами как ~10 cs (стандартное поведение Chrome/Firefox).
    pub delay_cs: u16,
}

impl AnimatedFrame {
    /// Возвращает задержку в миллисекундах.
    /// Значение 0 кодируется как 100 мс — стандартное browser-поведение.
    #[must_use]
    pub fn delay_ms(&self) -> u64 {
        let cs = if self.delay_cs == 0 { 10 } else { u64::from(self.delay_cs) };
        cs * 10
    }
}

/// Количество повторений анимации GIF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifLoopCount {
    /// Анимация воспроизводится ровно N раз (N ≥ 1).
    Finite(u16),
    /// Анимация воспроизводится бесконечно (Netscape extension loop_count = 0).
    Infinite,
}

/// Анимированный GIF: кадры + размер + метаданные цикличности.
#[derive(Debug, Clone)]
pub struct AnimatedGif {
    /// Кадры в порядке отображения. Всегда непустой (гарантирует [`decode_gif_animated`]).
    pub frames: Vec<AnimatedFrame>,
    /// Логическая ширина экрана GIF (Logical Screen Descriptor), пикселей.
    pub width: u32,
    /// Логическая высота экрана GIF, пикселей.
    pub height: u32,
    /// Количество повторений анимации.
    pub loop_count: GifLoopCount,
}

impl AnimatedGif {
    /// Возвращает индекс кадра для `elapsed_ms` миллисекунд от начала анимации.
    ///
    /// - `GifLoopCount::Infinite` — время берётся по модулю суммарной длительности.
    /// - `GifLoopCount::Finite(n)` — после `n` повторений останавливается на последнем кадре.
    /// - Пустой `frames` → всегда 0 (безопасный fallback).
    #[must_use]
    pub fn frame_index_at(&self, elapsed_ms: u64) -> usize {
        if self.frames.is_empty() {
            return 0;
        }
        let total_ms: u64 = self.frames.iter().map(AnimatedFrame::delay_ms).sum();
        if total_ms == 0 {
            return 0;
        }

        let effective_ms = match self.loop_count {
            GifLoopCount::Infinite => elapsed_ms % total_ms,
            GifLoopCount::Finite(n) => {
                let max_ms = total_ms.saturating_mul(u64::from(n));
                if elapsed_ms >= max_ms {
                    // Animation ended — hold last frame.
                    return self.frames.len() - 1;
                }
                elapsed_ms % total_ms
            }
        };

        let mut acc = 0u64;
        for (i, frame) in self.frames.iter().enumerate() {
            acc += frame.delay_ms();
            if effective_ms < acc {
                return i;
            }
        }
        self.frames.len() - 1
    }

    /// Возвращает кадр для `elapsed_ms` миллисекунд от начала анимации.
    #[must_use]
    pub fn frame_at(&self, elapsed_ms: u64) -> &AnimatedFrame {
        &self.frames[self.frame_index_at(elapsed_ms)]
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
    Ok(decode_gif_animated(bytes)?
        .frames
        .into_iter()
        .next()
        .ok_or(GifError::NoFrames)?
        .image)
}

/// Декодирует все кадры GIF и возвращает [`AnimatedGif`].
///
/// Использует `gif` крейт с `ColorOutput::RGBA` — цветовая палитра и disposal method
/// обрабатываются автоматически. Каждый кадр разворачивается в полный экранный прямоугольник
/// `width × height` (Logical Screen size) с применёнными composite-операциями disposal.
///
/// # Shell integration handoff
/// Шелл вызывает `gif.frame_at(elapsed_ms)` на каждом render-тике для получения
/// текущего кадра и передаёт `&frame.image` в `DrawImage`. Для перерисовки GIF
/// планируется `winit::EventLoop::set_control_flow(Poll)` или таймер через `EventLoopProxy`.
///
/// # Errors
/// - [`GifError::InvalidSignature`] — не валидная GIF сигнатура.
/// - [`GifError::DecodeError`] — ошибка при парсинге GIF структуры.
/// - [`GifError::NoFrames`] — GIF не содержит кадров.
pub fn decode_gif_animated(bytes: &[u8]) -> Result<AnimatedGif, GifError> {
    if !is_gif(bytes) {
        return Err(GifError::InvalidSignature);
    }

    let mut options = DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);

    let mut reader = options
        .read_info(Cursor::new(bytes))
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

    let frame_bytes = (width * height * 4) as usize;
    let mut frames = Vec::new();

    loop {
        let frame_info = reader
            .next_frame_info()
            .map_err(|e| GifError::DecodeError(e.to_string()))?;

        let Some(frame) = frame_info else { break };
        let delay_cs = frame.delay;

        let mut buffer = vec![0u8; frame_bytes];
        reader
            .read_into_buffer(&mut buffer)
            .map_err(|e| GifError::DecodeError(e.to_string()))?;

        frames.push(AnimatedFrame {
            image: Image {
                width,
                height,
                format: PixelFormat::Rgba8,
                data: buffer,
                icc_profile: None,
            },
            delay_cs,
        });
    }

    if frames.is_empty() {
        return Err(GifError::NoFrames);
    }

    Ok(AnimatedGif { frames, width, height, loop_count })
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

    // ── AnimatedFrame::delay_ms ──────────────────────────────────────────────

    fn make_frame(delay_cs: u16) -> AnimatedFrame {
        AnimatedFrame {
            image: Image {
                width: 1,
                height: 1,
                format: PixelFormat::Rgba8,
                data: vec![255, 0, 0, 255],
                icc_profile: None,
            },
            delay_cs,
        }
    }

    #[test]
    fn delay_ms_nonzero() {
        let frame = make_frame(10); // 10 cs = 100 ms
        assert_eq!(frame.delay_ms(), 100);
    }

    #[test]
    fn delay_ms_zero_treated_as_100ms() {
        let frame = make_frame(0);
        assert_eq!(frame.delay_ms(), 100); // 10 cs fallback × 10 ms = 100 ms
    }

    #[test]
    fn delay_ms_large() {
        let frame = make_frame(100); // 100 cs = 1000 ms
        assert_eq!(frame.delay_ms(), 1000);
    }

    // ── AnimatedGif::frame_index_at ──────────────────────────────────────────

    fn three_frame_infinite() -> AnimatedGif {
        // frame0=100ms, frame1=200ms, frame2=300ms → total 600ms
        AnimatedGif {
            frames: vec![make_frame(10), make_frame(20), make_frame(30)],
            width: 1,
            height: 1,
            loop_count: GifLoopCount::Infinite,
        }
    }

    #[test]
    fn frame_index_at_start() {
        let gif = three_frame_infinite();
        assert_eq!(gif.frame_index_at(0), 0);
    }

    #[test]
    fn frame_index_at_middle_of_first() {
        let gif = three_frame_infinite();
        assert_eq!(gif.frame_index_at(50), 0);
    }

    #[test]
    fn frame_index_at_boundary_second() {
        let gif = three_frame_infinite();
        // frame0 = 100 ms; frame1 starts at 100 ms
        assert_eq!(gif.frame_index_at(100), 1);
    }

    #[test]
    fn frame_index_at_boundary_third() {
        let gif = three_frame_infinite();
        // frame0=100 + frame1=200 = 300 ms → frame2
        assert_eq!(gif.frame_index_at(300), 2);
    }

    #[test]
    fn frame_index_loops_infinite() {
        let gif = three_frame_infinite();
        // total = 600 ms; at 600 ms wraps back to frame 0
        assert_eq!(gif.frame_index_at(600), 0);
        assert_eq!(gif.frame_index_at(650), 0);
        assert_eq!(gif.frame_index_at(700), 1);
    }

    #[test]
    fn frame_index_finite_one_loop_clamps() {
        let gif = AnimatedGif {
            frames: vec![make_frame(10), make_frame(20)],
            width: 1,
            height: 1,
            loop_count: GifLoopCount::Finite(1),
        };
        // total = 300 ms, 1 loop → stops at last frame after 300 ms
        assert_eq!(gif.frame_index_at(0), 0);
        assert_eq!(gif.frame_index_at(100), 1);
        assert_eq!(gif.frame_index_at(1_000_000), 1);
    }

    #[test]
    fn frame_index_finite_two_loops() {
        let gif = AnimatedGif {
            frames: vec![make_frame(10), make_frame(10)],
            width: 1,
            height: 1,
            loop_count: GifLoopCount::Finite(2),
        };
        // each frame 100 ms; 2 loops = 400 ms total
        assert_eq!(gif.frame_index_at(0), 0);
        assert_eq!(gif.frame_index_at(100), 1);
        assert_eq!(gif.frame_index_at(200), 0); // loop 2 starts
        assert_eq!(gif.frame_index_at(300), 1);
        assert_eq!(gif.frame_index_at(500), 1); // clamped past end
    }

    #[test]
    fn frame_index_empty_returns_zero() {
        let gif = AnimatedGif {
            frames: vec![],
            width: 1,
            height: 1,
            loop_count: GifLoopCount::Infinite,
        };
        assert_eq!(gif.frame_index_at(0), 0);
        assert_eq!(gif.frame_index_at(99999), 0);
    }

    #[test]
    fn frame_at_returns_correct_delay() {
        let gif = three_frame_infinite();
        // at 100 ms → frame 1 (delay_cs=20)
        assert_eq!(gif.frame_at(100).delay_cs, 20);
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
}

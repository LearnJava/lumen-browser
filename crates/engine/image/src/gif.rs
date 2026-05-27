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

/// Декодирует GIF файл и возвращает первый кадр.
///
/// На этом этапе поддерживается только первый кадр (frame 0).
/// Анимация будет реализована в Wave 3.
///
/// # Errors
/// - [`GifError::InvalidSignature`] — не валидная GIF сигнатура.
/// - [`GifError::DecodeError`] — ошибка при парсинге GIF структуры.
/// - [`GifError::NoFrames`] — GIF не содержит кадров.
pub fn decode_gif(bytes: &[u8]) -> Result<Image, GifError> {
    if !is_gif(bytes) {
        return Err(GifError::InvalidSignature);
    }

    let mut decoder = DecodeOptions::new();
    decoder.set_color_output(gif::ColorOutput::RGBA);

    let mut reader = decoder
        .read_info(Cursor::new(bytes))
        .map_err(|e| GifError::DecodeError(e.to_string()))?;

    // Получаем размер (из logical screen).
    let width = reader.width() as u32;
    let height = reader.height() as u32;

    if width == 0 || height == 0 {
        return Err(GifError::DecodeError("нулевой размер GIF".to_string()));
    }

    // Читаем первый кадр.
    let _frame = reader
        .next_frame_info()
        .map_err(|e| GifError::DecodeError(e.to_string()))?
        .ok_or(GifError::NoFrames)?;

    let mut buffer = vec![0u8; (width * height * 4) as usize];
    reader
        .read_into_buffer(&mut buffer)
        .map_err(|e| GifError::DecodeError(e.to_string()))?;

    Ok(Image {
        width,
        height,
        format: PixelFormat::Rgba8,
        data: buffer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

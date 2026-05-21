use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::{Image, PixelFormat};

pub fn decode_jpeg(bytes: &[u8]) -> Result<Image, JpegError> {
    // Pass 1: read headers to determine input colorspace and dimensions.
    // ZCursor wraps &[u8] cheaply so a second decode from the start is fine.
    let mut probe = JpegDecoder::new(ZCursor::new(bytes));
    probe.decode_headers().map_err(|e| JpegError(e.to_string()))?;

    let input_cs = probe.input_colorspace().unwrap_or(ColorSpace::Unknown);
    let (width, height) = probe
        .dimensions()
        .ok_or_else(|| JpegError("no dimensions after decode_headers".into()))?;

    // Grayscale JPEGs: force Luma output so we keep 1 byte/pixel.
    // zune-jpeg's default output is RGB (even for Y-only JPEGs).
    let is_gray = matches!(input_cs, ColorSpace::Luma | ColorSpace::LumaA);
    let options = if is_gray {
        DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma)
    } else {
        DecoderOptions::default()
    };

    // Pass 2: full decode with correct output colorspace.
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let pixels = decoder.decode().map_err(|e| JpegError(e.to_string()))?;

    let format = if is_gray { PixelFormat::Gray8 } else { PixelFormat::Rgb8 };

    Ok(Image { width: width as u32, height: height as u32, format, data: pixels })
}

/// Ошибка декодирования JPEG (обёртка над zune-jpeg).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpegError(pub String);

impl core::fmt::Display for JpegError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JpegError {}

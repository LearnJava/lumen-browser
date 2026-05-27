use std::io::Write;
use zune_core::bytestream::ZCursor;
use zune_core::options::DecoderOptions;
use zune_core::result::DecodingResult;
use zune_png::PngDecoder;

use crate::{DecodeError, Image, PixelFormat, PNG_SIGNATURE};

pub fn decode_png(bytes: &[u8]) -> Result<Image, DecodeError> {
    if bytes.len() < PNG_SIGNATURE.len() || bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(DecodeError::InvalidSignature);
    }

    let options = DecoderOptions::default().png_set_strip_to_8bit(true);
    let mut decoder = PngDecoder::new_with_options(ZCursor::new(bytes), options);

    let result = decoder.decode().map_err(|e| DecodeError::Decode(e.to_string()))?;

    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| DecodeError::Decode("no dimensions after decode".into()))?;

    let pixels: Vec<u8> = match result {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.into_iter().map(|x| (x >> 8) as u8).collect(),
        _ => return Err(DecodeError::Decode("unexpected pixel depth from zune-png".into())),
    };

    if width == 0 || height == 0 {
        return Err(DecodeError::Decode("zero dimension".into()));
    }

    let bpp = pixels.len() / (width * height);
    let format = match bpp {
        1 => PixelFormat::Gray8,
        2 => PixelFormat::GrayAlpha8,
        3 => PixelFormat::Rgb8,
        4 => PixelFormat::Rgba8,
        _ => return Err(DecodeError::Decode(format!("unexpected bytes per pixel: {bpp}"))),
    };

    Ok(Image { width: width as u32, height: height as u32, format, data: pixels })
}

/// Кодирует RGBA8 изображение в PNG формат.
///
/// # Errors
/// Returns `Err` if the image format is not RGBA8 or if PNG encoding fails.
pub fn encode_png_rgba8(img: &Image) -> Result<Vec<u8>, crate::ImageError> {
    if img.format != PixelFormat::Rgba8 {
        return Err(crate::ImageError::UnknownFormat);
    }

    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, img.width, img.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| crate::ImageError::Png(crate::DecodeError::Decode(e.to_string())))?;
    writer
        .write_image_data(&img.data)
        .map_err(|e| crate::ImageError::Png(crate::DecodeError::Decode(e.to_string())))?;
    writer
        .finish()
        .map_err(|e| crate::ImageError::Png(crate::DecodeError::Decode(e.to_string())))?;
    Ok(out)
}

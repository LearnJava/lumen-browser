use std::io::Read;
use zune_core::bytestream::ZCursor;
use zune_core::options::DecoderOptions;
use zune_core::result::DecodingResult;
use zune_png::PngDecoder;

use crate::{DecodeError, Image, IccProfile, PixelFormat, PNG_SIGNATURE};

/// Ищет и парсит iCCP chunk в PNG данных.
fn parse_png_icc_profile(bytes: &[u8]) -> Option<IccProfile> {
    if bytes.len() < 8 { return None; }
    let mut offset = 8; // Пропускаем PNG signature

    loop {
        if offset + 8 > bytes.len() { break; } // Нет места для chunk header + CRC

        // Читаем length и chunk type
        let chunk_len = u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
        let chunk_type = &bytes[offset + 4..offset + 8];

        if chunk_type == b"iCCP" {
            // Нашли iCCP chunk
            let chunk_data_start = offset + 8;
            if chunk_data_start + chunk_len > bytes.len() { return None; }
            let chunk_data = &bytes[chunk_data_start..chunk_data_start + chunk_len];

            // iCCP структура: [profile-name (1-79)][null (1)][compression (1)][compressed-data]
            if chunk_data.is_empty() { return None; }
            let null_pos = chunk_data.iter().position(|&b| b == 0)?;
            if null_pos + 2 > chunk_data.len() { return None; }

            let compression_method = chunk_data[null_pos + 1];
            if compression_method != 0 { return None; } // Only deflate (0) supported

            let compressed_data = &chunk_data[null_pos + 2..];
            if compressed_data.is_empty() { return None; }

            // Decompress using flate2
            let mut decompressed = Vec::new();
            let mut decoder = flate2::read::DeflateDecoder::new(compressed_data);
            if decoder.read_to_end(&mut decompressed).is_ok() && decompressed.len() >= 128 {
                return Some(IccProfile { data: decompressed });
            }
            return None;
        }

        // Переходим к следующему chunk
        offset += 8 + chunk_len + 4; // header (8) + data (chunk_len) + CRC (4)
    }

    None
}

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

    let icc_profile = parse_png_icc_profile(bytes);

    Ok(Image { width: width as u32, height: height as u32, format, data: pixels, icc_profile })
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

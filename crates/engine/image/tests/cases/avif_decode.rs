//! AVIF: декодирование через `avif-parse` + `rav1d` против эталона ffmpeg (libdav1d/libaom).

use lumen_image::decode_avif;

fn max_channel_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

#[test]
fn color_420_matches_reference() {
    let (w, h, rgba) = decode_avif(include_bytes!("../fixtures/testsrc_64x48.avif")).unwrap();
    assert_eq!((w, h), (64, 48));
    let reference = include_bytes!("../fixtures/testsrc_64x48.rgba");
    assert_eq!(rgba.len(), reference.len());
    // Апсемплинг цветности у ffmpeg (bicubic) и у нас (nearest) различается у резких границ.
    let bad = rgba.chunks(4).zip(reference.chunks(4)).filter(|(a, b)| max_channel_diff(a, b) > 40).count();
    assert!(bad * 20 < (w * h) as usize, "слишком много расхождений: {bad}");
}

#[test]
fn alpha_item_becomes_alpha_channel() {
    // 20x20, отдельный монохромный alpha-item, во всех пикселях 128 (по ffmpeg).
    let (w, h, rgba) = decode_avif(include_bytes!("../fixtures/transparent_20x20.avif")).unwrap();
    assert_eq!((w, h), (20, 20));
    assert!(rgba.chunks(4).all(|p| p[3].abs_diff(128) <= 1), "альфа должна быть ~128");
}

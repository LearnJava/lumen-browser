//! Тесты AV1-обёртки: кадры сверяются с эталоном ffmpeg 9 (libdav1d) по Adler-32
//! (`ffmpeg -f framecrc`), а RGBA — с независимой float-формулой.

use lumen_media_demux::{Codec, Demuxer, TrackKind};

use super::*;

const AV1_IDENTITY: &[u8] = include_bytes!("../../media-demux/tests/data/av1.mp4");

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (0u32, 0u32); // av_adler32_update(0, …) в ffmpeg: начальное a = 0
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Пакеты AV1-дорожки: `(pts_us, данные)`.
fn av1_packets(mp4: &[u8]) -> Vec<(i64, Vec<u8>)> {
    let mut demux = Demuxer::open(mp4).unwrap();
    let track = demux
        .tracks()
        .iter()
        .find(|t| t.kind == TrackKind::Video && t.codec == Codec::Av1)
        .map(|t| t.id)
        .expect("AV1-дорожка");
    let mut out = Vec::new();
    while let Some(p) = demux.next_packet().unwrap() {
        if p.track == track {
            out.push((p.pts_us, p.data));
        }
    }
    out
}

fn decode_all(mp4: &[u8], config: &Config) -> Vec<Picture> {
    let mut dec = Decoder::new(config).unwrap();
    let mut pics = Vec::new();
    for (pts, data) in av1_packets(mp4) {
        dec.send(&data, pts).unwrap();
        while let Some(p) = dec.next_picture() {
            pics.push(p);
        }
    }
    dec.drain().unwrap();
    while let Some(p) = dec.next_picture() {
        pics.push(p);
    }
    pics
}

/// Плоскости кадра в «сыром» виде ffmpeg (`rawvideo`, little-endian).
fn raw_bytes(p: &Picture) -> Vec<u8> {
    let mut v = Vec::new();
    for plane in &p.planes {
        match &plane.samples {
            Samples::U8(s) => v.extend_from_slice(s),
            Samples::U16(s) => v.extend(s.iter().flat_map(|x| x.to_le_bytes())),
        }
    }
    v
}

fn reference_crcs(framecrc: &str) -> Vec<u32> {
    framecrc
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| u32::from_str_radix(l.rsplit(',').next().unwrap().trim().trim_start_matches("0x"), 16).unwrap())
        .collect()
}

macro_rules! clip_test {
    ($name:ident, $file:literal, $bd:expr, $layout:expr) => {
        #[test]
        fn $name() {
            let mp4 = include_bytes!(concat!("../tests/data/", $file, ".mp4"));
            let crcs = reference_crcs(include_str!(concat!("../tests/data/", $file, ".framecrc")));
            let pics = decode_all(mp4, &Config::video());
            assert_eq!(pics.len(), crcs.len(), "число кадров");
            for (i, (p, crc)) in pics.iter().zip(&crcs).enumerate() {
                assert_eq!((p.width, p.height, p.bit_depth, p.layout), (64, 48, $bd, $layout));
                assert_eq!(adler32(&raw_bytes(p)), *crc, "кадр {i} отличается от ffmpeg/libdav1d");
            }
            // Кадры идут в порядке показа, метки — те, что отправили вместе с пакетами.
            let pts: Vec<i64> = pics.iter().map(|p| p.pts).collect();
            let mut sent: Vec<i64> = av1_packets(mp4).iter().map(|p| p.0).collect();
            sent.sort_unstable();
            assert_eq!(pts, sent);
        }
    };
}

clip_test!(yuv420_8bit_matches_ffmpeg, "clip_420_8bit", 8, Layout::I420);
clip_test!(yuv420_10bit_matches_ffmpeg, "clip_420_10bit", 10, Layout::I420);
clip_test!(yuv420_12bit_matches_ffmpeg, "clip_420_12bit", 12, Layout::I420);
clip_test!(yuv422_8bit_matches_ffmpeg, "clip_422_8bit", 8, Layout::I422);
clip_test!(monochrome_matches_ffmpeg, "clip_gray_8bit", 8, Layout::I400);

#[test]
fn single_thread_and_multithread_agree() {
    let mp4 = include_bytes!("../tests/data/clip_420_8bit.mp4");
    let one = decode_all(mp4, &Config { threads: 1, max_frame_delay: 1, ..Config::video() });
    let many = decode_all(mp4, &Config { threads: 4, max_frame_delay: 0, ..Config::video() });
    assert_eq!(one, many);
}

/// Эталонное преобразование: textbook-формула в f64, цветность — ближайший сосед.
fn reference_rgba(p: &Picture) -> Vec<u8> {
    let (w, h) = (p.width as usize, p.height as usize);
    let (kr, kb) = match p.matrix {
        4..=6 => (0.299, 0.114),
        7 => (0.212, 0.087),
        9 | 10 => (0.2627, 0.0593),
        _ => (0.2126, 0.0722),
    };
    let kg = 1.0 - kr - kb;
    let sc = f64::from(1u32 << (p.bit_depth - 8));
    let maxv = f64::from((1u32 << p.bit_depth) - 1);
    let (yoff, yden, chalf, cden) =
        if p.full_range { (0.0, maxv, (maxv + 1.0) / 2.0, maxv) } else { (16.0 * sc, 219.0 * sc, 128.0 * sc, 224.0 * sc) };
    let get = |i: usize, x: usize, y: usize| -> f64 {
        let pl = &p.planes[i];
        let ix = y * pl.width + x;
        match &pl.samples {
            Samples::U8(s) => f64::from(s[ix]),
            Samples::U16(s) => f64::from(s[ix]),
        }
    };
    let mut out = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let yy = (get(0, x, y) - yoff) / yden;
            let (r, g, b) = if p.layout == Layout::I400 {
                (yy, yy, yy)
            } else {
                let (cx, cy) = match p.layout {
                    Layout::I420 => (x / 2, y / 2),
                    Layout::I422 => (x / 2, y),
                    _ => (x, y),
                };
                let cb = (get(1, cx, cy) - chalf) / cden;
                let cr = (get(2, cx, cy) - chalf) / cden;
                let r = yy + 2.0 * (1.0 - kr) * cr;
                let b = yy + 2.0 * (1.0 - kb) * cb;
                (r, (yy - kr * r - kb * b) / kg, b)
            };
            let o = (y * w + x) * 4;
            for (k, v) in [r, g, b].into_iter().enumerate() {
                out[o + k] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

fn assert_rgba_close(a: &[u8], b: &[u8], what: &str) {
    assert_eq!(a.len(), b.len());
    let worst = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
    assert!(worst <= 1, "{what}: расхождение с эталоном {worst} > 1");
}

macro_rules! rgba_test {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            let mp4 = include_bytes!(concat!("../tests/data/", $file, ".mp4"));
            for p in decode_all(mp4, &Config::video()) {
                assert_rgba_close(&p.to_rgba8().unwrap(), &reference_rgba(&p), $file);
            }
        }
    };
}

rgba_test!(rgba_420_8bit_within_one, "clip_420_8bit");
rgba_test!(rgba_420_10bit_within_one, "clip_420_10bit");
rgba_test!(rgba_420_12bit_within_one, "clip_420_12bit");
rgba_test!(rgba_422_within_one, "clip_422_8bit");
rgba_test!(rgba_monochrome_within_one, "clip_gray_8bit");

#[test]
fn identity_matrix_maps_planes_to_gbr() {
    // av1.mp4 — gbrp полного диапазона: Y — зелёный, U — синий, V — красный.
    let pics = decode_all(AV1_IDENTITY, &Config::video());
    assert!(!pics.is_empty());
    for p in &pics {
        assert_eq!((p.layout, p.matrix, p.full_range), (Layout::I444, 0, true));
        let rgba = p.to_rgba8().unwrap();
        let plane = |i: usize| match &p.planes[i].samples {
            Samples::U8(s) => s.clone(),
            Samples::U16(_) => panic!("ожидались 8 бит"),
        };
        let (g, b, r) = (plane(0), plane(1), plane(2));
        for (i, px) in rgba.chunks(4).enumerate() {
            assert_eq!(px, [r[i], g[i], b[i], 255], "пиксель {i}");
        }
    }
}

#[test]
fn flush_allows_restart_from_keyframe() {
    let mp4 = include_bytes!("../tests/data/clip_420_8bit.mp4");
    let packets = av1_packets(mp4);
    let first = decode_all(mp4, &Config::video()).remove(0);
    let mut dec = Decoder::new(&Config::video()).unwrap();
    for (pts, data) in packets.iter().take(4) {
        dec.send(data, *pts).unwrap();
    }
    dec.flush();
    assert!(dec.next_picture().is_none(), "flush сбрасывает очередь");
    dec.send(&packets[0].1, packets[0].0).unwrap();
    dec.drain().unwrap();
    assert_eq!(dec.next_picture().unwrap(), first);
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let mut dec = Decoder::new(&Config::still()).unwrap();
    let r = dec.send(&[0xff; 64], 0).and_then(|()| dec.drain());
    assert!(r.is_err() || dec.next_picture().is_none());
    assert!(decode_still(&[0u8; 8], 1 << 20).is_err());
    assert!(dec.send(&[], 0).is_ok(), "пустой пакет — не ошибка");
}

#[test]
fn rgba_buffer_length_is_checked() {
    let p = decode_all(include_bytes!("../tests/data/clip_420_8bit.mp4"), &Config::video()).remove(0);
    assert_eq!(p.rgba_len(), 64 * 48 * 4);
    assert!(matches!(p.to_rgba8_into(&mut [0u8; 16]), Err(Error::Mismatch(_))));
}

fn gray_picture(w: u32, h: u32, v: u8, full_range: bool) -> Picture {
    Picture {
        width: w,
        height: h,
        bit_depth: 8,
        layout: Layout::I400,
        full_range,
        matrix: 2,
        primaries: 2,
        transfer: 2,
        pts: 0,
        planes: vec![Plane { width: w as usize, height: h as usize, samples: Samples::U8(vec![v; (w * h) as usize]) }],
    }
}

#[test]
fn alpha_plane_sets_alpha_and_unpremultiplies() {
    let color = gray_picture(2, 2, 128, true);
    let mut rgba = color.to_rgba8().unwrap();
    assert_eq!(&rgba[..4], [128, 128, 128, 255]);
    // Полный диапазон, значение 128 → альфа 128; без premultiplied цвет не трогаем.
    color.apply_alpha(&mut rgba, &gray_picture(2, 2, 128, true), false).unwrap();
    assert_eq!(&rgba[..4], [128, 128, 128, 128]);
    // Premultiplied: 128 / (128/255) ≈ 255.
    let mut rgba = color.to_rgba8().unwrap();
    color.apply_alpha(&mut rgba, &gray_picture(2, 2, 128, true), true).unwrap();
    assert_eq!(&rgba[..4], [255, 255, 255, 128]);
    // Ограниченный диапазон: 16 → 0, 235 → 255.
    let mut rgba = color.to_rgba8().unwrap();
    color.apply_alpha(&mut rgba, &gray_picture(2, 2, 235, false), false).unwrap();
    assert_eq!(rgba[3], 255);
    color.apply_alpha(&mut rgba, &gray_picture(2, 2, 16, false), false).unwrap();
    assert_eq!(rgba[3], 0);
    // Размеры должны совпадать.
    assert!(matches!(color.apply_alpha(&mut rgba, &gray_picture(3, 2, 1, true), false), Err(Error::Mismatch(_))));
}

#[test]
fn odd_dimensions_use_rounded_up_chroma() {
    let (w, h) = (5usize, 3usize);
    let (cw, ch) = Layout::I420.chroma_size(w, h);
    assert_eq!((cw, ch), (3, 2));
    let fill = |n: usize, k: usize| Samples::U8((0..n).map(|i| (16 + (i * k) % 200) as u8).collect());
    let p = Picture {
        width: w as u32,
        height: h as u32,
        bit_depth: 8,
        layout: Layout::I420,
        full_range: false,
        matrix: 1,
        primaries: 1,
        transfer: 1,
        pts: 0,
        planes: vec![
            Plane { width: w, height: h, samples: fill(w * h, 37) },
            Plane { width: cw, height: ch, samples: fill(cw * ch, 53) },
            Plane { width: cw, height: ch, samples: fill(cw * ch, 71) },
        ],
    };
    assert_rgba_close(&p.to_rgba8().unwrap(), &reference_rgba(&p), "5x3 4:2:0");
}

//! Замер скорости: `cargo run --release -p lumen-av1 --example bench -- clip.mp4 [потоков]`.
//!
//! Демуксирует первую AV1-дорожку MP4/WebM, декодирует все кадры (rav1d) и
//! конвертирует каждый в RGBA, печатает кадры/с и отношение к реальному времени.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use lumen_av1::{Config, Decoder};
use lumen_media_demux::{Codec, Demuxer, TrackKind};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("путь к файлу");
    let threads: u32 = args.next().map_or(0, |s| s.parse().expect("число потоков"));
    let bytes = std::fs::read(&path).expect("чтение файла");

    let mut demux = Demuxer::open(&bytes).expect("демуксер");
    let track = demux
        .tracks()
        .iter()
        .find(|t| t.kind == TrackKind::Video && t.codec == Codec::Av1)
        .map(|t| t.id)
        .expect("AV1-дорожка");
    let duration_s = demux.duration_us().unwrap_or(0) as f64 / 1e6;
    let mut packets = Vec::new();
    while let Some(p) = demux.next_packet().expect("пакет") {
        if p.track == track {
            packets.push(p);
        }
    }

    let mut dec = Decoder::new(&Config { threads, ..Config::video() }).expect("декодер");
    let (mut frames, mut decode_only, mut rgba_buf) = (0u32, 0.0f64, Vec::new());
    let start = Instant::now();
    let mut handle = |dec: &mut Decoder, frames: &mut u32, decode_only: &mut f64| {
        while let Some(pic) = dec.next_picture() {
            let t = Instant::now();
            rgba_buf.resize(pic.rgba_len(), 0);
            pic.to_rgba8_into(&mut rgba_buf).expect("RGBA");
            *decode_only += t.elapsed().as_secs_f64();
            *frames += 1;
        }
    };
    for p in &packets {
        dec.send(&p.data, p.pts_us).expect("send");
        handle(&mut dec, &mut frames, &mut decode_only);
    }
    dec.drain().expect("drain");
    handle(&mut dec, &mut frames, &mut decode_only);
    let total = start.elapsed().as_secs_f64();
    let rgba = decode_only;
    let dec_only = total - rgba;
    println!(
        "{frames} кадров, ролик {duration_s:.2} с: всего {total:.2} с ({:.1} кадр/с, {:.2}x реального времени); \
         декодирование {dec_only:.2} с, YUV→RGBA {rgba:.2} с ({:.1} мс/кадр)",
        f64::from(frames) / total,
        duration_s / total,
        rgba * 1e3 / f64::from(frames.max(1)),
    );
}

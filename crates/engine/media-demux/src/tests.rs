//! Сверка с ffprobe 9 (`-show_streams -show_packets -count_packets`): кодеки,
//! размеры, число пакетов, длительность, ключевые кадры и метки времени.
//! Файлы — `tests/wpt/media/*` и свои фикстуры в `tests/data`.

use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn wpt(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tests/wpt/media").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn data(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn all_packets(d: &mut Demuxer<'_>) -> Vec<Packet> {
    let mut v = Vec::new();
    while let Some(p) = d.next_packet().expect("packet") {
        v.push(p);
    }
    v
}

fn per_track(ps: &[Packet]) -> BTreeMap<u32, Vec<&Packet>> {
    let mut m: BTreeMap<u32, Vec<&Packet>> = BTreeMap::new();
    for p in ps {
        m.entry(p.track).or_default().push(p);
    }
    m
}

/// Ожидание по дорожке: (вид, кодек, строка кодека, w, h, Гц, каналы, пакетов).
type Want = (TrackKind, Codec, &'static str, u32, u32, u32, u16, usize);

fn check(bytes: &[u8], container: Container, dur_us: Option<i64>, want: &[Want]) {
    let mut d = Demuxer::open(bytes).expect("open");
    assert_eq!(d.container(), container);
    match (d.duration_us(), dur_us) {
        (Some(got), Some(w)) => assert!((got - w).abs() <= 20_000, "duration {got} vs {w}"),
        (g, w) => assert_eq!(g, w),
    }
    let tracks = d.tracks().to_vec();
    assert_eq!(tracks.len(), want.len(), "{tracks:?}");
    let ps = all_packets(&mut d);
    let by = per_track(&ps);
    for (t, w) in tracks.iter().zip(want) {
        let got = (t.kind, t.codec, t.codec_string.as_str(), t.width, t.height, t.sample_rate, t.channels);
        assert_eq!(got, (w.0, w.1, w.2, w.3, w.4, w.5, w.6), "track {}", t.id);
        assert_eq!(by.get(&t.id).map_or(0, Vec::len), w.7, "packets of track {}", t.id);
    }
}

const V: TrackKind = TrackKind::Video;
const A: TrackKind = TrackKind::Audio;

#[test]
fn mp4_h264_aac_matches_ffprobe() {
    check(
        &wpt("test-1s.mp4"),
        Container::Mp4,
        Some(1_029_200),
        &[
            (V, Codec::H264, "avc1.64000d", 320, 240, 0, 0, 31),
            (A, Codec::Aac, "mp4a.40.2", 0, 0, 44100, 2, 45),
        ],
    );
}

#[test]
fn mp4_extradata_is_avcc_and_asc() {
    let b = wpt("test-1s.mp4");
    let d = Demuxer::open(&b).expect("open");
    let t = d.tracks();
    assert_eq!(t[0].extradata[0], 1, "avcC configurationVersion");
    assert_eq!(&t[0].extradata[1..4], &[0x64, 0x00, 0x0d]);
    // AudioSpecificConfig: AAC-LC (2) 44100 Гц (индекс 4) 2 канала.
    assert_eq!(t[1].extradata[..2], [0x12, 0x10]);
}

#[test]
fn mp4_other_samples_match_ffprobe() {
    check(
        &wpt("A4.mp4"),
        Container::Mp4,
        Some(3_065_034),
        &[
            (V, Codec::H264, "avc1.42c00d", 320, 240, 0, 0, 90),
            (A, Codec::Aac, "mp4a.40.2", 0, 0, 44100, 1, 132),
        ],
    );
}

#[test]
fn mp4_hevc_av1() {
    check(
        &data("hevc.mp4"),
        Container::Mp4,
        Some(1_000_000),
        &[(V, Codec::Hevc, "hev1.1.6.L30.90", 64, 64, 0, 0, 10)],
    );
    check(
        &data("av1.mp4"),
        Container::Mp4,
        Some(1_000_000),
        &[(V, Codec::Av1, "av01.1.00M.08", 64, 64, 0, 0, 10)],
    );
}

#[test]
fn webm_vp9_opus_matches_ffprobe() {
    check(
        &wpt("test-1s.webm"),
        Container::WebM,
        Some(1_008_000),
        &[
            (V, Codec::Vp9, "vp09", 320, 240, 0, 0, 30),
            (A, Codec::Opus, "opus", 0, 0, 48000, 2, 51),
        ],
    );
}

#[test]
fn webm_vp8_vorbis_matches_ffprobe() {
    let b = wpt("test-av-384k-44100Hz-1ch-320x240-30fps-10kfr.webm");
    let d = Demuxer::open(&b).expect("open");
    assert_eq!(d.duration_us(), Some(2_023_000));
    let t = d.tracks();
    assert_eq!((t[0].codec, t[0].width, t[0].height), (Codec::Vp8, 320, 240));
    assert_eq!((t[1].codec, t[1].sample_rate, t[1].channels), (Codec::Vorbis, 44100, 1));
    // Три заголовка Vorbis в упаковке Xiph: 2, размеры двух первых, затем данные.
    assert_eq!(t[1].extradata[0], 2);
}

#[test]
fn matroska_doctype_is_told_from_webm() {
    let b = wpt("white.webm");
    assert_eq!(Demuxer::open(&b).expect("open").container(), Container::Matroska);
}

#[test]
fn ogg_vorbis_opus_theora_flac_family() {
    check(
        &wpt("sound_0.oga"),
        Container::Ogg,
        Some(50_000),
        &[(A, Codec::Vorbis, "vorbis", 0, 0, 44100, 1, 4)],
    );
    check(
        &data("opus.ogg"),
        Container::Ogg,
        Some(1_006_500),
        &[(A, Codec::Opus, "opus", 0, 0, 48000, 1, 51)],
    );
    check(
        &data("theora.ogv"),
        Container::Ogg,
        Some(1_000_000),
        &[(V, Codec::Theora, "theora", 64, 64, 0, 0, 10)],
    );
}

#[test]
fn mp4_with_b_frames_has_monotonic_dts_and_reordered_pts() {
    let b = data("bframes.mp4");
    let mut d = Demuxer::open(&b).expect("open");
    let ps = all_packets(&mut d);
    let v = &per_track(&ps)[&d.tracks()[0].id];
    assert_eq!(v.len(), 10);
    assert!(v.windows(2).all(|w| w[0].dts_us < w[1].dts_us), "dts строго растёт");
    assert!(v.windows(2).any(|w| w[1].pts_us < w[0].pts_us), "pts переставлен");
    assert!(v.iter().all(|p| p.pts_us >= p.dts_us), "pts не раньше dts");
    assert_eq!(v.iter().map(|p| p.pts_us).min(), Some(0));
    assert!(v[0].keyframe && v[1..].iter().all(|p| !p.keyframe));
    // 10 кадров по 10 к/с: сетка pts кратна 100 мс.
    let mut pts: Vec<i64> = v.iter().map(|p| p.pts_us).collect();
    pts.sort_unstable();
    assert_eq!(pts, (0..10).map(|i| i * 100_000).collect::<Vec<_>>());
}

#[test]
fn fragmented_mp4_is_read_across_fragments() {
    let b = data("frag.mp4");
    let mut d = Demuxer::open(&b).expect("open");
    assert_eq!(d.container(), Container::Mp4);
    let t = d.tracks().to_vec();
    assert_eq!(t.len(), 2);
    assert_eq!((t[0].codec, t[1].codec), (Codec::H264, Codec::Aac));
    let ps = all_packets(&mut d);
    let by = per_track(&ps);
    assert_eq!(by[&t[0].id].len(), 10);
    assert_eq!(by[&t[1].id].len(), 45);
    // Видео — 10 кадров по 40 мс подряд из нескольких фрагментов.
    let dts: Vec<i64> = by[&t[0].id].iter().map(|p| p.dts_us).collect();
    assert!(dts.windows(2).all(|w| w[1] > w[0]));
    assert!(by[&t[0].id].iter().all(|p| !p.data.is_empty()));
}

#[test]
fn keyframes_and_timestamps_of_simple_blocks() {
    let b = wpt("test-1s.webm");
    let mut d = Demuxer::open(&b).expect("open");
    let ps = all_packets(&mut d);
    let by = per_track(&ps);
    let v = &by[&1];
    assert!(v[0].keyframe && v[1..].iter().all(|p| !p.keyframe));
    assert!(by[&2].iter().all(|p| p.keyframe));
    assert!(ps.windows(2).all(|w| w[0].track != w[1].track || w[0].pts_us <= w[1].pts_us));
}

#[test]
fn matroska_block_group_keyframes_come_from_the_payload() {
    // Все блоки этого файла лежат в BlockGroup; ffprobe: ключевые — пакеты 1 и 60.
    let b = wpt("400x300-red-resize-200x150-green.webm");
    let mut d = Demuxer::open(&b).expect("open");
    let ps = all_packets(&mut d);
    let keys: Vec<usize> = ps.iter().enumerate().filter(|(_, p)| p.keyframe).map(|(i, _)| i + 1).collect();
    assert_eq!(keys, [1, 60]);
}

fn first_of(ps: &[Packet], track: u32) -> &Packet {
    ps.iter().find(|p| p.track == track).expect("track has packets")
}

#[test]
fn mp4_seek_lands_on_sync_sample_not_after_target() {
    let b = wpt("green-at-15.mp4");
    let mut d = Demuxer::open(&b).expect("open");
    let total = all_packets(&mut d).len();
    d.seek_us(20_000_000).expect("seek");
    let ps = all_packets(&mut d);
    let p = first_of(&ps, 1);
    assert!(p.keyframe);
    assert_eq!(p.pts_us, 16_666_667, "ключевой кадр 16.667 с — ближайший до 20 с");
    assert_eq!(ps.len(), total - 500);
    d.seek_us(0).expect("seek");
    assert_eq!(all_packets(&mut d).len(), total, "перемотка в начало после конца");
    d.seek_us(1_000_000_000).expect("seek");
    let ps = all_packets(&mut d);
    assert_eq!(first_of(&ps, 1).pts_us, 25_000_000, "за концом — последний ключевой кадр");
}

#[test]
fn mp4_seek_audio_starts_at_covering_packet() {
    let b = wpt("test-1s.mp4");
    let mut d = Demuxer::open(&b).expect("open");
    d.seek_us(500_000).expect("seek");
    let ps = all_packets(&mut d);
    let a = first_of(&ps, 2);
    assert!(a.pts_us <= 500_000 && a.pts_us + a.duration_us > 500_000, "{a:?}");
}

#[test]
fn webm_seek_lands_on_keyframe_not_after_target() {
    let b = wpt("green-at-15.webm");
    let mut d = Demuxer::open(&b).expect("open");
    d.seek_us(20_000_000).expect("seek");
    let ps = all_packets(&mut d);
    let p = first_of(&ps, 1);
    assert!(p.keyframe, "{p:?}");
    assert!(p.pts_us <= 20_000_000 && p.pts_us >= 19_000_000, "{p:?}");
    d.seek_us(0).expect("seek");
    assert_eq!(first_of(&all_packets(&mut d), 1).pts_us, 0);
}

#[test]
fn ogg_seek_starts_at_covering_packet() {
    let b = wpt("sound_5.oga");
    let mut d = Demuxer::open(&b).expect("open");
    let total = all_packets(&mut d).len();
    d.seek_us(2_500_000).expect("seek");
    let ps = all_packets(&mut d);
    let id = d.tracks()[0].id;
    let p = first_of(&ps, id);
    assert!(p.pts_us <= 2_500_000 && p.pts_us >= 2_500_000 - 25_000, "{p:?}");
    assert!(ps.len() < total && ps.len() > total / 3);
}

#[test]
fn unknown_and_broken_input_is_an_error_not_a_panic() {
    assert_eq!(Demuxer::open(b"").err(), Some(DemuxError::UnknownContainer));
    assert_eq!(Demuxer::open(b"RIFF....WAVEfmt ").err(), Some(DemuxError::UnknownContainer));
    for name in ["test-1s.mp4", "test-1s.webm", "sound_0.oga"] {
        let full = wpt(name);
        for cut in [4, 12, 64, full.len() / 3, full.len() / 2, full.len() - 1] {
            // Обрезанный файл: открывается или нет, но читается без паники.
            if let Ok(mut d) = Demuxer::open(&full[..cut]) {
                while let Ok(Some(_)) = d.next_packet() {}
            }
        }
    }
    // Мусор после сигнатуры.
    let mut junk = b"\x00\x00\x00\x18ftypisom".to_vec();
    junk.extend(std::iter::repeat_n(0xFF, 64));
    assert!(Demuxer::open(&junk).is_err());
}

#[test]
fn ticks_round_to_nearest_microsecond() {
    assert_eq!(ticks_to_us(1, 3), 333_333);
    assert_eq!(ticks_to_us(2, 3), 666_667);
    assert_eq!(ticks_to_us(-1, 3), -333_333);
    assert_eq!(ticks_to_us(90_000, 90_000), 1_000_000);
    assert_eq!(ticks_to_us(5, 0), 0);
}

#[test]
fn sniff_known_signatures() {
    assert_eq!(sniff(b"OggS\0\0\0\0"), Some(Container::Ogg));
    assert_eq!(sniff(&[0x1A, 0x45, 0xDF, 0xA3, 0, 0]), Some(Container::Matroska));
    assert_eq!(sniff(b"\0\0\0\x20ftypisom"), Some(Container::Mp4));
    assert_eq!(sniff(b"ID3\x04\0\0\0\0\0\0"), None);
}

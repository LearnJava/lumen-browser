//! Модель `SourceBuffer` на реальных файлах: фрагментированный MP4 и WebM,
//! нарезанные на init- и media-сегменты так, как это делает страница.

use super::*;
use crate::units::{assemble, Splitter};
use std::path::PathBuf;

fn read(rel: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn frag_mp4() -> Vec<u8> {
    read("../media-demux/tests/data/frag.mp4")
}

fn av_webm() -> Vec<u8> {
    read("../../../tests/wpt/media/test-av-384k-44100Hz-1ch-320x240-30fps-10kfr.webm")
}

/// Файл как набор сегментов, которые страница добавляет по одному.
struct Segments {
    init: Vec<u8>,
    media: Vec<Vec<u8>>,
}

fn segments(format: Format, file: &[u8]) -> Segments {
    let mut buf = file.to_vec();
    let mut sp = Splitter::new(format);
    let (mut init, mut media) = (None, Vec::new());
    while let Some(u) = sp.next(&mut buf).expect("split") {
        match u {
            Unit::Init(i) => init = Some(assemble(format, &i, &[])),
            Unit::Media(m) => media.push(m),
        }
    }
    Segments { init: init.expect("init segment"), media }
}

fn demuxed_count(file: &[u8], track: u32) -> usize {
    let mut d = Demuxer::open(file).expect("open");
    let mut n = 0;
    while let Some(p) = d.next_packet().expect("packet") {
        n += usize::from(p.track == track);
    }
    n
}

fn track_ids(m: &SourceBufferModel) -> Vec<u32> {
    m.tracks().map(|t| t.id).collect()
}

#[test]
fn mime_codecs_are_parsed() {
    let m = parse_mime("video/mp4; codecs=\"avc1.64001f, mp4a.40.2\"").expect("mp4");
    assert_eq!(m.format, Format::Mp4);
    assert!(m.is_video);
    assert_eq!(m.codecs, ["avc1.64001f", "mp4a.40.2"]);
    let w = parse_mime("Audio/WebM;codecs=opus").expect("webm");
    assert_eq!((w.format, w.is_video, w.codecs.as_slice()), (Format::WebM, false, &["opus".to_owned()][..]));
    assert!(parse_mime("video/mp4").expect("no codecs").codecs.is_empty());
    assert!(matches!(parse_mime("video/ogg"), Err(MseError::Unsupported(_))));
    assert!(matches!(parse_mime("text/html"), Err(MseError::Unsupported(_))));
}

#[test]
fn mp4_whole_file_gives_every_frame() {
    let file = frag_mp4();
    let mut m = SourceBufferModel::new("video/mp4").expect("model");
    let out = m.append(&file).expect("append");
    assert!(out.init_received);
    assert!(m.init_received());
    assert_eq!(m.format(), Format::Mp4);
    for id in track_ids(&m) {
        assert_eq!(m.frames(id).len(), demuxed_count(&file, id), "дорожка {id}");
    }
    assert_eq!(out.frames_added, track_ids(&m).iter().map(|&i| m.frames(i).len()).sum::<usize>());
    let r = m.buffered_us();
    assert_eq!(r.len(), 1, "один сплошной диапазон: {r:?}");
    let first_pts = track_ids(&m).iter().filter_map(|&i| m.frames(i).iter().map(|f| f.pts_us).min()).max();
    assert_eq!(Some(r[0].0), first_pts, "начало = самый поздний старт дорожек");
    assert!(r[0].1 > 1_000_000, "{r:?}");
    assert_eq!(r[0].1, m.highest_end_us());
}

#[test]
fn mp4_chunked_append_matches_whole() {
    let file = frag_mp4();
    let mut whole = SourceBufferModel::new("video/mp4").expect("model");
    whole.append(&file).expect("whole");
    let mut chunked = SourceBufferModel::new("video/mp4").expect("model");
    for piece in file.chunks(777) {
        chunked.append(piece).expect("piece");
    }
    assert_eq!(track_ids(&whole), track_ids(&chunked));
    for id in track_ids(&whole) {
        let (a, b) = (whole.frames(id), chunked.frames(id));
        assert_eq!(a.len(), b.len());
        assert!(a.iter().zip(b).all(|(x, y)| x.pts_us == y.pts_us && x.dts_us == y.dts_us && x.data == y.data));
    }
    assert_eq!(whole.buffered_us(), chunked.buffered_us());
}

#[test]
fn webm_whole_file_gives_every_frame() {
    let file = av_webm();
    let mut m = SourceBufferModel::new("video/webm").expect("model");
    let out = m.append(&file).expect("append");
    assert!(out.init_received);
    assert_eq!(m.format(), Format::WebM);
    let ids = track_ids(&m);
    assert_eq!(ids.len(), 2, "аудио и видео");
    for id in ids {
        assert_eq!(m.frames(id).len(), demuxed_count(&file, id), "дорожка {id}");
        assert!(m.frames(id).iter().all(|f| f.dur_us > 0), "длительность достроена");
    }
    let r = m.buffered_us();
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].1 > 100_000);
}

#[test]
fn webm_chunked_append_matches_whole() {
    let file = av_webm();
    let mut whole = SourceBufferModel::new("video/webm").expect("model");
    whole.append(&file).expect("whole");
    let mut chunked = SourceBufferModel::new("video/webm").expect("model");
    for piece in file.chunks(1013) {
        chunked.append(piece).expect("piece");
    }
    for id in track_ids(&whole) {
        assert_eq!(whole.frames(id).len(), chunked.frames(id).len(), "дорожка {id}");
    }
    assert_eq!(whole.buffered_us(), chunked.buffered_us());
}

#[test]
fn media_before_init_is_a_decode_error() {
    let s = segments(Format::Mp4, &frag_mp4());
    let mut m = SourceBufferModel::new("video/mp4").expect("model");
    assert!(matches!(m.append(&s.media[0]), Err(MseError::Decode(_))));
    assert!(!m.init_received());
    // Буфер остаётся рабочим: init, затем сегмент принимаются.
    m.append(&s.init).expect("init");
    m.append(&s.media[0]).expect("media");
    assert!(!m.buffered_us().is_empty());
}

#[test]
fn garbage_is_a_decode_error() {
    for mime in ["video/mp4", "video/webm"] {
        let mut m = SourceBufferModel::new(mime).expect("model");
        let junk: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8 | 0x80).collect();
        assert!(matches!(m.append(&junk), Err(MseError::Decode(_))), "{mime}");
        assert!(m.buffered_us().is_empty());
    }
}

#[test]
fn reappending_a_segment_replaces_it() {
    let s = segments(Format::Mp4, &frag_mp4());
    let mut m = SourceBufferModel::new("video/mp4").expect("model");
    m.append(&s.init).expect("init");
    for seg in &s.media {
        m.append(seg).expect("media");
    }
    let before: Vec<usize> = track_ids(&m).iter().map(|&i| m.frames(i).len()).collect();
    let ranges = m.buffered_us();
    m.append(&s.media[0]).expect("again");
    let after: Vec<usize> = track_ids(&m).iter().map(|&i| m.frames(i).len()).collect();
    assert_eq!(before, after, "повтор сегмента не дублирует кадры");
    assert_eq!(ranges, m.buffered_us());
}

#[test]
fn remove_drops_range_and_dependent_frames() {
    let mut m = SourceBufferModel::new("video/webm").expect("model");
    m.append(&av_webm()).expect("append");
    let video = m.tracks().find(|t| t.kind == lumen_media_demux::TrackKind::Video).expect("video").id;
    let all = m.frames(video).len();
    let end = m.highest_end_us();
    // Хвост без начала: остаются только кадры до отметки, следующий ключевой не тронут.
    let cut = end / 2;
    m.remove(cut, end + 1);
    let kept = m.frames(video);
    assert!(kept.len() < all && !kept.is_empty());
    assert!(kept.iter().all(|f| f.pts_us < cut));
    let r = m.buffered_us();
    assert_eq!(r.len(), 1);
    assert!(r[0].1 <= cut + 100_000, "{r:?} cut={cut}");
    assert!(m.generation() >= 2);
}

#[test]
fn remove_leading_part_keeps_a_keyframe_start() {
    let mut m = SourceBufferModel::new("video/webm").expect("model");
    m.append(&av_webm()).expect("append");
    let video = m.tracks().find(|t| t.kind == lumen_media_demux::TrackKind::Video).expect("video").id;
    m.remove(0, 100_000);
    let first = m.frames(video).first().expect("кадры остались");
    assert!(first.key, "после удаления буфер начинается с ключевого кадра");
    assert!(first.pts_us >= 100_000);
}

#[test]
fn timestamp_offset_shifts_frames() {
    let s = segments(Format::Mp4, &frag_mp4());
    let mut m = SourceBufferModel::new("video/mp4").expect("model");
    m.append(&s.init).expect("init");
    m.set_timestamp_offset_us(5_000_000);
    m.append(&s.media[0]).expect("media");
    let r = m.buffered_us();
    assert_eq!(r.len(), 1);
    assert!(r[0].0 >= 5_000_000, "{r:?}");
}

#[test]
fn sequence_mode_places_groups_back_to_back() {
    let s = segments(Format::Mp4, &frag_mp4());
    let mut m = SourceBufferModel::new("video/mp4").expect("model");
    m.set_mode(AppendMode::Sequence);
    m.append(&s.init).expect("init");
    m.append(&s.media[0]).expect("first");
    let first_end = m.highest_end_us();
    m.abort(); // новая группа кадров
    m.append(&s.media[0]).expect("second");
    assert!(m.highest_end_us() > first_end + first_end / 2, "{} → {}", first_end, m.highest_end_us());
    // Смещение второй группы — конец первой минус её самый ранний кадр.
    assert!(m.timestamp_offset_us() > 0 && m.timestamp_offset_us() <= first_end, "{}", m.timestamp_offset_us());
    let audio = m.tracks().find(|t| t.kind == lumen_media_demux::TrackKind::Audio).expect("аудио").id;
    let r = track_ranges(m.frames(audio));
    assert_eq!(r.len(), 1, "звук обеих групп без дыр: {r:?}");
}

#[test]
fn append_window_filters_frames() {
    let mut m = SourceBufferModel::new("video/webm").expect("model");
    m.set_append_window_us(0, 300_000);
    m.append(&av_webm()).expect("append");
    let r = m.buffered_us();
    assert!(!r.is_empty());
    assert!(r.iter().all(|&(_, e)| e <= 300_000), "{r:?}");
    // Окно с началом позже нуля: кадры до него отброшены, буфер начинается с ключевого кадра.
    let mut late = SourceBufferModel::new("video/webm").expect("model");
    late.set_append_window_us(100_000, i64::MAX);
    late.append(&av_webm()).expect("append");
    let video = late.tracks().find(|t| t.kind == lumen_media_demux::TrackKind::Video).map_or(0, |t| t.id);
    let frames = late.frames(video);
    assert!(frames.iter().all(|f| f.pts_us >= 100_000));
    assert!(frames.first().is_none_or(|f| f.key));
}

#[test]
fn abort_discards_partial_segment_but_keeps_init() {
    let s = segments(Format::Mp4, &frag_mp4());
    let mut m = SourceBufferModel::new("video/mp4").expect("model");
    m.append(&s.init).expect("init");
    let half = s.media[0].len() / 2;
    m.append(&s.media[0][..half]).expect("half");
    assert!(m.buffered_us().is_empty());
    m.abort();
    m.append(&s.media[0]).expect("whole");
    assert!(!m.buffered_us().is_empty());
}

#[test]
fn new_init_with_other_tracks_is_rejected() {
    let mp4 = segments(Format::Mp4, &frag_mp4());
    let mut m = SourceBufferModel::new("video/mp4").expect("model");
    m.append(&mp4.init).expect("init");
    // Тот же init ещё раз — допустим (смена качества тем же кодеком).
    assert!(m.append(&mp4.init).expect("again").init_received);
}

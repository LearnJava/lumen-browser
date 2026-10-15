//! WebM / Matroska поверх `matroska-demuxer`.

use std::io::Cursor;

use matroska_demuxer::{Frame, MatroskaFile, TrackEntry, TrackType};

use crate::codec;
use crate::{Codec, Container, DemuxError, Packet, Result, TrackInfo, TrackKind};

type File<'a> = MatroskaFile<Cursor<&'a [u8]>>;

pub(crate) struct Mkv<'a> {
    bytes: &'a [u8],
    file: File<'a>,
    container: Container,
    tracks: Vec<TrackInfo>,
    /// Номер дорожки Matroska → длительность кадра по умолчанию, мкс.
    default_dur_us: Vec<i64>,
    /// Наносекунд в тике метки времени.
    scale_ns: u64,
    duration_us: Option<i64>,
    frame: Frame,
    /// Пакет, прочитанный при перемотке и ещё не выданный.
    pending: Option<Packet>,
    /// Ключевые кадры видеодорожек для перемотки (строится при первой перемотке).
    video_keys: Option<Vec<(u32, i64)>>,
}

fn open_file(bytes: &[u8]) -> Result<File<'_>> {
    MatroskaFile::open(Cursor::new(bytes))
        .map_err(|e| DemuxError::Malformed(format!("matroska: {e}")))
}

impl<'a> Mkv<'a> {
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self> {
        let file = open_file(bytes)?;
        let container = if file.ebml_header().doc_type() == "webm" {
            Container::WebM
        } else {
            Container::Matroska
        };
        let scale_ns = file.info().timestamp_scale().get();
        let duration_us = file
            .info()
            .duration()
            .map(|d| (d * scale_ns as f64 / 1000.0).round() as i64);
        let mut tracks = Vec::new();
        let mut default_dur_us = Vec::new();
        for t in file.tracks() {
            if let Some(info) = track_info(t) {
                default_dur_us.push(t.default_duration().map_or(0, |d| (d.get() / 1000) as i64));
                tracks.push(info);
            }
        }
        if tracks.is_empty() {
            return Err(DemuxError::Unsupported(
                "в Matroska нет поддерживаемых видео- и аудиодорожек".into(),
            ));
        }
        Ok(Self {
            bytes,
            file,
            container,
            tracks,
            default_dur_us,
            scale_ns,
            duration_us,
            frame: Frame::default(),
            pending: None,
            video_keys: None,
        })
    }

    pub(crate) fn container(&self) -> Container {
        self.container
    }

    pub(crate) fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    pub(crate) fn duration_us(&self) -> Option<i64> {
        self.duration_us
    }

    fn to_us(&self, ticks: u64) -> i64 {
        i64::try_from(u128::from(ticks) * u128::from(self.scale_ns) / 1000).unwrap_or(i64::MAX)
    }

    /// Пакет из прочитанного кадра; `None` — дорожка не наша (субтитры и т. п.).
    fn packet_from(&self, frame: &mut Frame) -> Option<Packet> {
        let idx = self
            .tracks
            .iter()
            .position(|t| u64::from(t.id) == frame.track)?;
        let pts = self.to_us(frame.timestamp);
        let duration_us = frame
            .duration
            .map_or(self.default_dur_us[idx], |d| self.to_us(d));
        let t = &self.tracks[idx];
        // Блок в BlockGroup не несёт флага ключевого кадра (признак — отсутствие
        // ReferenceBlock, читатель его не отдаёт): смотрим в сам кадр.
        let keyframe = frame
            .is_keyframe
            .unwrap_or_else(|| is_keyframe_by_payload(t, &frame.data));
        Some(Packet {
            track: t.id,
            pts_us: pts,
            dts_us: pts,
            duration_us,
            keyframe,
            data: std::mem::take(&mut frame.data),
        })
    }

    fn read_packet(&mut self) -> Result<Option<Packet>> {
        let mut frame = std::mem::take(&mut self.frame);
        let r = loop {
            let more = self
                .file
                .next_frame(&mut frame)
                .map_err(|e| DemuxError::Malformed(format!("matroska: {e}")))?;
            if !more {
                break None;
            }
            if let Some(p) = self.packet_from(&mut frame) {
                break Some(p);
            }
        };
        self.frame = frame;
        Ok(r)
    }

    pub(crate) fn next_packet(&mut self) -> Result<Option<Packet>> {
        match self.pending.take() {
            Some(p) => Ok(Some(p)),
            None => self.read_packet(),
        }
    }

    /// Моменты ключевых кадров видеодорожек (дорожка, мкс) — один проход по
    /// файлу, результат помнится.
    fn video_keyframes(&mut self) -> Result<&[(u32, i64)]> {
        if self.video_keys.is_none() {
            let mut file = open_file(self.bytes)?;
            let mut frame = Frame::default();
            let mut keys = Vec::new();
            while file
                .next_frame(&mut frame)
                .map_err(|e| DemuxError::Malformed(format!("matroska: {e}")))?
            {
                if let Some(p) = self.packet_from(&mut frame) {
                    let video = self.tracks.iter().any(|t| t.id == p.track && t.kind == TrackKind::Video);
                    if video && p.keyframe {
                        keys.push((p.track, p.pts_us));
                    }
                }
            }
            self.video_keys = Some(keys);
        }
        Ok(self.video_keys.as_deref().unwrap_or(&[]))
    }

    /// Перемотка до ближайшего ключевого кадра не позже цели (общего для всех
    /// видеодорожек — берётся самый ранний); аудио идёт с того же места.
    ///
    /// Читатель `matroska-demuxer` перематывает по `Cues` до первого кадра
    /// *после* метки и на файлах без полного индекса отдаёт пустой поток, а
    /// ключевой кадр нужен именно до цели, поэтому индекс ключевых кадров
    /// строим сами, а начало потока находим пропуском кадров с начала файла.
    pub(crate) fn seek_us(&mut self, target_us: i64) -> Result<()> {
        let target_us = target_us.max(0);
        let video: Vec<u32> = self
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Video)
            .map(|t| t.id)
            .collect();
        let keys = self.video_keyframes()?;
        let start_us = video
            .iter()
            .map(|id| {
                keys.iter()
                    .filter(|(t, us)| t == id && *us <= target_us)
                    .map(|&(_, us)| us)
                    .max()
                    .unwrap_or(0)
            })
            .min()
            .unwrap_or(target_us);
        self.file = open_file(self.bytes)?;
        self.pending = None;
        while let Some(p) = self.read_packet()? {
            if p.pts_us >= start_us {
                self.pending = Some(p);
                break;
            }
        }
        Ok(())
    }
}

/// Ключевой ли кадр, судя по данным: VP8 (бит 0 первого байта), VP9
/// (`frame_type` в несжатом заголовке), H.264/HEVC (IDR/IRAP NAL с префиксом
/// длины 4 байта), AV1 (есть Sequence Header OBU). Аудио — всегда да.
fn is_keyframe_by_payload(t: &TrackInfo, d: &[u8]) -> bool {
    match t.codec {
        _ if t.kind == TrackKind::Audio => true,
        Codec::Vp8 => d.first().is_some_and(|b| b & 1 == 0),
        Codec::Vp9 => vp9_is_key(d),
        Codec::H264 | Codec::Hevc => {
            let mut rest = d;
            while rest.len() > 4 {
                let n = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
                let nal = rest[4];
                let key = if t.codec == Codec::H264 {
                    nal & 0x1F == 5
                } else {
                    (16..=21).contains(&((nal >> 1) & 0x3F))
                };
                if key {
                    return true;
                }
                match rest.get(4 + n..) {
                    Some(r) => rest = r,
                    None => break,
                }
            }
            false
        }
        Codec::Av1 => {
            // OBU: тип в битах 6..3, бит 2 — расширение (+1 байт), бит 1 — есть size (leb128).
            let mut i = 0;
            while let Some(&h) = d.get(i) {
                let ty = (h >> 3) & 0xF;
                if ty == 1 {
                    return true;
                }
                if h & 0x2 == 0 {
                    break; // нет поля размера — длины не узнать
                }
                let mut j = i + 1 + usize::from(h & 0x4 != 0);
                let (mut len, mut shift) = (0usize, 0);
                while let Some(&b) = d.get(j) {
                    len |= usize::from(b & 0x7F) << shift;
                    shift += 7;
                    j += 1;
                    if b & 0x80 == 0 || shift > 28 {
                        break;
                    }
                }
                i = j + len;
            }
            false
        }
        _ => false,
    }
}

/// VP9: `frame_marker(2) profile_low(1) profile_high(1) [reserved(1) если
/// profile 3] show_existing_frame(1) frame_type(1)`; `frame_type == 0` — ключевой.
fn vp9_is_key(d: &[u8]) -> bool {
    let Some(&b0) = d.first() else {
        return false;
    };
    let profile = ((b0 >> 4) & 1) | (((b0 >> 5) & 1) << 1);
    let mut bit = 4; // позиция (с MSB) после marker+profile
    if profile == 3 {
        bit += 1;
    }
    let get = |pos: usize| d.get(pos / 8).map(|b| (b >> (7 - pos % 8)) & 1);
    if get(bit) != Some(0) {
        return false; // show_existing_frame
    }
    get(bit + 1) == Some(0)
}

fn track_info(t: &TrackEntry) -> Option<TrackInfo> {
    let id = u32::try_from(t.track_number().get()).ok()?;
    let private = t.codec_private().unwrap_or(&[]).to_vec();
    let kind = match t.track_type() {
        TrackType::Video => TrackKind::Video,
        TrackType::Audio => TrackKind::Audio,
        _ => return None,
    };
    let (codec, codec_string, extradata) = match (kind, t.codec_id()) {
        (TrackKind::Video, "V_MPEG4/ISO/AVC") => {
            (Codec::H264, codec::avc_string(b"avc1", &private), private)
        }
        (TrackKind::Video, "V_MPEGH/ISO/HEVC") => {
            (Codec::Hevc, codec::hevc_string(b"hvc1", &private), private)
        }
        (TrackKind::Video, "V_AV1") => (Codec::Av1, codec::av1_string_from_config(&private), private),
        (TrackKind::Video, "V_VP9") => (Codec::Vp9, "vp09".to_string(), private),
        (TrackKind::Video, "V_VP8") => (Codec::Vp8, "vp8".to_string(), private),
        (TrackKind::Audio, "A_AAC") => {
            let aot = codec::aot_from_asc(&private).unwrap_or(2);
            (Codec::Aac, codec::aac_string(aot), private)
        }
        (TrackKind::Audio, "A_MPEG/L3") => (Codec::Mp3, "mp3".to_string(), private),
        (TrackKind::Audio, "A_OPUS") => (Codec::Opus, "opus".to_string(), private),
        (TrackKind::Audio, "A_VORBIS") => (Codec::Vorbis, "vorbis".to_string(), private),
        (TrackKind::Audio, "A_FLAC") => (Codec::Flac, "flac".to_string(), private),
        (_, other) => (Codec::Other, other.to_string(), private),
    };
    let (mut width, mut height, mut sample_rate, mut channels) = (0, 0, 0, 0);
    if let Some(v) = t.video() {
        width = u32::try_from(v.pixel_width().get()).unwrap_or(0);
        height = u32::try_from(v.pixel_height().get()).unwrap_or(0);
    }
    if let Some(a) = t.audio() {
        sample_rate = a.sampling_frequency() as u32;
        channels = u16::try_from(a.channels().get()).unwrap_or(0);
    }
    Some(TrackInfo {
        id,
        kind,
        codec,
        codec_string,
        extradata,
        width,
        height,
        sample_rate,
        channels,
    })
}

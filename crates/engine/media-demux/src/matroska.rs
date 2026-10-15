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

    pub(crate) fn next_packet(&mut self) -> Result<Option<Packet>> {
        loop {
            let more = self
                .file
                .next_frame(&mut self.frame)
                .map_err(|e| DemuxError::Malformed(format!("matroska: {e}")))?;
            if !more {
                return Ok(None);
            }
            let Some(idx) = self
                .tracks
                .iter()
                .position(|t| u64::from(t.id) == self.frame.track)
            else {
                continue; // субтитры, чаптеры и прочее
            };
            let pts = self.to_us(self.frame.timestamp);
            let duration_us = self
                .frame
                .duration
                .map_or(self.default_dur_us[idx], |d| self.to_us(d));
            let t = &self.tracks[idx];
            let keyframe = self.frame.is_keyframe.unwrap_or(t.kind == TrackKind::Audio);
            return Ok(Some(Packet {
                track: t.id,
                pts_us: pts,
                dts_us: pts,
                duration_us,
                keyframe,
                data: std::mem::take(&mut self.frame.data),
            }));
        }
    }

    pub(crate) fn seek_us(&mut self, target_us: i64) -> Result<()> {
        let ticks = u64::try_from(target_us.max(0)).unwrap_or(0) * 1000 / self.scale_ns.max(1);
        if self.file.seek(ticks).is_err() {
            // Нет Cues/SeekHead: начинаем с начала, пакеты до цели читатель отбросит сам.
            self.file = open_file(self.bytes)?;
        }
        Ok(())
    }
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

//! Ogg (Vorbis, Opus, Theora, FLAC) поверх крейта `ogg`; длительность
//! Vorbis-пакетов считает `lewton` (заголовки), у Opus она в TOC-байте.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;

use lewton::header::{IdentHeader, SetupHeader};
use ogg::PacketReader;

use crate::{ticks_to_us, Codec, DemuxError, Packet, Result, TrackInfo, TrackKind};

/// Частота отсчётов Opus в Ogg (RFC 7845) — всегда 48 кГц.
const OPUS_RATE: u64 = 48_000;

enum Kind {
    Vorbis { ident: Option<Box<IdentHeader>>, setup: Option<Box<SetupHeader>> },
    Opus { pre_skip: u16 },
    Theora { kf_shift: u32, fps_num: u32, fps_den: u32 },
    Flac { rate: u32 },
    /// Skeleton, Speex, Kate и прочее — пакеты отбрасываются.
    Skip,
}

struct Stream {
    serial: u32,
    kind: Kind,
    /// Всего заголовочных пакетов у потока.
    header_total: usize,
    headers_left: usize,
    headers: Vec<Vec<u8>>,
    /// Индекс в `tracks`; `None` для `Skip` и до разбора заголовков.
    track: Option<usize>,
    /// Позиция потока в единицах его шкалы (отсчёты/кадры).
    pos: i64,
    seen_data: bool,
}

pub(crate) struct OggDemux<'a> {
    bytes: &'a [u8],
    reader: PacketReader<Cursor<&'a [u8]>>,
    streams: Vec<Stream>,
    tracks: Vec<TrackInfo>,
    duration_us: Option<i64>,
    /// Пакеты уже прочитанной, но не завершённой страницы.
    partial: Vec<ogg::Packet>,
    queue: VecDeque<Packet>,
}

fn read_err(e: ogg::OggReadError) -> DemuxError {
    DemuxError::Malformed(format!("ogg: {e}"))
}

impl<'a> OggDemux<'a> {
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self> {
        let mut d = Self {
            bytes,
            reader: PacketReader::new(Cursor::new(bytes)),
            streams: Vec::new(),
            tracks: Vec::new(),
            duration_us: None,
            partial: Vec::new(),
            queue: VecDeque::new(),
        };
        d.header_phase(true)?;
        if d.tracks.is_empty() {
            return Err(DemuxError::Unsupported(
                "в Ogg нет Vorbis/Opus/Theora/FLAC потоков".into(),
            ));
        }
        d.duration_us = d.scan_duration();
        Ok(d)
    }

    pub(crate) fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    pub(crate) fn duration_us(&self) -> Option<i64> {
        self.duration_us
    }

    /// Читает заголовочные пакеты всех потоков и останавливается на первом
    /// пакете данных (он остаётся в `partial`). `detect` — первый проход
    /// (создаёт потоки и дорожки), иначе — повторный после перемотки.
    fn header_phase(&mut self, detect: bool) -> Result<()> {
        loop {
            let Some(pkt) = self.reader.read_packet().map_err(read_err)? else {
                return Ok(());
            };
            let serial = pkt.stream_serial();
            let mut idx = self.streams.iter().position(|s| s.serial == serial);
            if idx.is_none() && detect && pkt.first_in_stream() {
                self.streams.push(Stream::detect(serial, &pkt.data));
                idx = Some(self.streams.len() - 1);
                let s = &mut self.streams[idx.unwrap_or(0)];
                if s.headers_left == 0 {
                    s.finish(&mut self.tracks);
                }
                continue;
            }
            let Some(idx) = idx else { continue };
            let s = &mut self.streams[idx];
            if matches!(s.kind, Kind::Skip) {
                continue;
            }
            if s.headers_left > 0 {
                s.headers_left -= 1;
                if detect {
                    s.headers.push(pkt.data);
                    if s.headers_left == 0 {
                        s.finish(&mut self.tracks);
                    }
                }
                continue;
            }
            self.partial.push(pkt);
            return Ok(());
        }
    }

    /// Все пакеты одной страницы.
    fn read_batch(&mut self) -> Result<Option<Vec<ogg::Packet>>> {
        let mut batch = std::mem::take(&mut self.partial);
        while !batch.last().is_some_and(ogg::Packet::last_in_page) {
            match self.reader.read_packet().map_err(read_err)? {
                Some(p) => batch.push(p),
                None => break,
            }
        }
        Ok((!batch.is_empty()).then_some(batch))
    }

    fn fill_queue(&mut self) -> Result<bool> {
        while self.queue.is_empty() {
            let Some(batch) = self.read_batch()? else {
                return Ok(false);
            };
            let Some(first) = batch.first() else { continue };
            let serial = first.stream_serial();
            let page_gp = batch.last().map_or(u64::MAX, ogg::Packet::absgp_page);
            let Some(s) = self.streams.iter_mut().find(|s| s.serial == serial) else {
                continue;
            };
            let Some(track) = s.track else { continue };
            let id = self.tracks[track].id;
            let datas: Vec<Vec<u8>> = batch.into_iter().map(|p| p.data).collect();
            self.queue.extend(s.timestamp(id, datas, page_gp));
        }
        Ok(true)
    }

    pub(crate) fn next_packet(&mut self) -> Result<Option<Packet>> {
        if self.queue.is_empty() && !self.fill_queue()? {
            return Ok(None);
        }
        Ok(self.queue.pop_front())
    }

    pub(crate) fn seek_us(&mut self, target_us: i64) -> Result<()> {
        self.reader = PacketReader::new(Cursor::new(self.bytes));
        self.partial.clear();
        self.queue.clear();
        for s in &mut self.streams {
            s.headers_left = s.header_total;
            s.pos = 0;
            s.seen_data = false;
        }
        self.header_phase(false)?;
        // Видео пишем с ближайшего ключевого кадра, аудио — с пакета, покрывающего цель.
        let mut held: Vec<Packet> = Vec::new();
        let mut out: VecDeque<Packet> = VecDeque::new();
        while out.is_empty() {
            if !self.fill_queue()? {
                break;
            }
            while let Some(p) = self.queue.pop_front() {
                let video = self
                    .tracks
                    .iter()
                    .any(|t| t.id == p.track && t.kind == TrackKind::Video);
                if p.pts_us + p.duration_us > target_us || (video && p.pts_us > target_us) {
                    out.push_back(p);
                } else if video {
                    if p.keyframe {
                        held.clear();
                    }
                    held.push(p);
                }
            }
        }
        self.queue = held.into_iter().chain(out).collect();
        Ok(())
    }

    /// Последняя гранула-позиция каждого потока → длительность.
    fn scan_duration(&self) -> Option<i64> {
        let mut last: HashMap<u32, u64> = HashMap::new();
        let b = self.bytes;
        let mut pos = 0usize;
        while pos + 27 <= b.len() && &b[pos..pos + 4] == b"OggS" {
            let mut gp = [0u8; 8];
            gp.copy_from_slice(&b[pos + 6..pos + 14]);
            let gp = u64::from_le_bytes(gp);
            let serial = u32::from_le_bytes([b[pos + 14], b[pos + 15], b[pos + 16], b[pos + 17]]);
            let nseg = usize::from(b[pos + 26]);
            let Some(table) = b.get(pos + 27..pos + 27 + nseg) else {
                break;
            };
            let body: usize = table.iter().map(|&s| usize::from(s)).sum();
            if gp != u64::MAX {
                last.insert(serial, gp);
            }
            pos += 27 + nseg + body;
        }
        self.streams
            .iter()
            .filter(|s| s.track.is_some())
            .filter_map(|s| s.granule_to_us(*last.get(&s.serial)?))
            .max()
    }
}

impl Stream {
    /// Распознаёт кодек по первому пакету потока.
    fn detect(serial: u32, first: &[u8]) -> Self {
        let (kind, remaining) = if first.starts_with(b"\x01vorbis") {
            (Kind::Vorbis { ident: None, setup: None }, 2)
        } else if first.starts_with(b"OpusHead") {
            (Kind::Opus { pre_skip: 0 }, 1)
        } else if first.starts_with(b"\x80theora") {
            (
                Kind::Theora { kf_shift: 0, fps_num: 1, fps_den: 1 },
                2,
            )
        } else if first.starts_with(b"\x7fFLAC") {
            let n = first
                .get(7..9)
                .map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]])));
            (Kind::Flac { rate: 0 }, n)
        } else {
            (Kind::Skip, 0)
        };
        Self {
            serial,
            kind,
            header_total: remaining + 1,
            headers_left: remaining,
            headers: vec![first.to_vec()],
            track: None,
            pos: 0,
            seen_data: false,
        }
    }

    /// Заголовки прочитаны: разобрать их и завести дорожку.
    fn finish(&mut self, tracks: &mut Vec<TrackInfo>) {
        let id = self.serial;
        let h0 = self.headers.first().map(Vec::as_slice).unwrap_or(&[]);
        let info = match &mut self.kind {
            Kind::Skip => None,
            Kind::Vorbis { ident, setup } => {
                let parsed = lewton::header::read_header_ident(h0).ok();
                if let Some(i) = &parsed {
                    *setup = self.headers.get(2).and_then(|h| {
                        lewton::header::read_header_setup(
                            h,
                            i.audio_channels,
                            (i.blocksize_0, i.blocksize_1),
                        )
                        .ok()
                        .map(Box::new)
                    });
                }
                parsed.map(|i| {
                    let info = TrackInfo {
                        id,
                        kind: TrackKind::Audio,
                        codec: Codec::Vorbis,
                        codec_string: "vorbis".to_string(),
                        extradata: xiph_lacing(&self.headers),
                        width: 0,
                        height: 0,
                        sample_rate: i.audio_sample_rate,
                        channels: u16::from(i.audio_channels),
                    };
                    *ident = Some(Box::new(i));
                    info
                })
            }
            Kind::Opus { pre_skip } => {
                *pre_skip = h0.get(10..12).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
                Some(TrackInfo {
                    id,
                    kind: TrackKind::Audio,
                    codec: Codec::Opus,
                    codec_string: "opus".to_string(),
                    extradata: h0.to_vec(),
                    width: 0,
                    height: 0,
                    sample_rate: OPUS_RATE as u32,
                    channels: u16::from(h0.get(9).copied().unwrap_or(2)),
                })
            }
            Kind::Theora { kf_shift, fps_num, fps_den } => {
                let be = |a: usize, n: usize| {
                    h0.get(a..a + n)
                        .map(|b| b.iter().fold(0u32, |v, &x| (v << 8) | u32::from(x)))
                };
                match (be(14, 3), be(17, 3), be(22, 4), be(26, 4), be(40, 2)) {
                    (Some(w), Some(h), Some(n), Some(d), Some(sh)) if n > 0 && d > 0 => {
                        *fps_num = n;
                        *fps_den = d;
                        *kf_shift = (sh >> 5) & 0x1f;
                        Some(TrackInfo {
                            id,
                            kind: TrackKind::Video,
                            codec: Codec::Theora,
                            codec_string: "theora".to_string(),
                            extradata: xiph_lacing(&self.headers),
                            width: w,
                            height: h,
                            sample_rate: 0,
                            channels: 0,
                        })
                    }
                    _ => None,
                }
            }
            Kind::Flac { rate } => {
                // 0x7F "FLAC" мажор минор число-заголовков "fLaC" [блок STREAMINFO].
                let info = h0.get(17..51).map(<[u8]>::to_vec);
                let (r, ch) = info.as_deref().map_or((0, 0), |s| {
                    let packed = u32::from_be_bytes([s[10], s[11], s[12], s[13]]);
                    (packed >> 12, ((packed >> 9) & 7) + 1)
                });
                *rate = r;
                (r > 0).then(|| TrackInfo {
                    id,
                    kind: TrackKind::Audio,
                    codec: Codec::Flac,
                    codec_string: "flac".to_string(),
                    extradata: info.unwrap_or_default(),
                    width: 0,
                    height: 0,
                    sample_rate: r,
                    channels: u16::try_from(ch).unwrap_or(2),
                })
            }
        };
        if let Some(info) = info {
            self.track = Some(tracks.len());
            tracks.push(info);
        }
    }

    fn granule_to_us(&self, gp: u64) -> Option<i64> {
        let g = i64::try_from(gp).ok()?;
        match &self.kind {
            Kind::Vorbis { ident: Some(i), .. } => Some(ticks_to_us(g, u64::from(i.audio_sample_rate))),
            Kind::Opus { pre_skip } => {
                Some(ticks_to_us((g - i64::from(*pre_skip)).max(0), OPUS_RATE))
            }
            Kind::Flac { rate } if *rate > 0 => Some(ticks_to_us(g, u64::from(*rate))),
            Kind::Theora { kf_shift, fps_num, fps_den } => {
                let mask = (1i64 << *kf_shift) - 1;
                let frames = (g >> *kf_shift) + (g & mask);
                Some(ticks_to_us(frames * i64::from(*fps_den), u64::from(*fps_num)))
            }
            _ => None,
        }
    }

    /// Пакеты одной страницы → [`Packet`] с метками времени.
    fn timestamp(&mut self, id: u32, datas: Vec<Vec<u8>>, page_gp: u64) -> Vec<Packet> {
        let n = datas.len() as i64;
        // Кодеки без длительности в самом пакете делят приращение гранулы поровну.
        let even = |pos: i64| match i64::try_from(page_gp) {
            Ok(g) if g > pos && n > 0 => (g - pos) / n,
            _ => 0,
        };
        let even_dur = even(self.pos);
        let mut out = Vec::with_capacity(datas.len());
        for data in datas {
            let (dur_ticks, key, scale): (i64, bool, (u64, i64)) = match &self.kind {
                Kind::Vorbis { ident: Some(i), setup: Some(st) } => {
                    let d = if self.seen_data {
                        lewton::audio::get_decoded_sample_count(i, st, &data)
                            .map_or(0, |c| c as i64)
                    } else {
                        0
                    };
                    (d, true, (u64::from(i.audio_sample_rate), 1))
                }
                Kind::Opus { .. } => (opus_samples(&data), true, (OPUS_RATE, 1)),
                Kind::Theora { fps_num, fps_den, .. } => {
                    if data.first().is_some_and(|b| b & 0x80 != 0) {
                        continue; // заголовок посреди потока
                    }
                    let key = data.first().is_some_and(|b| b & 0x40 == 0);
                    (1, key, (u64::from(*fps_num), i64::from(*fps_den)))
                }
                Kind::Flac { rate } => (even_dur, true, (u64::from(*rate), 1)),
                _ => continue,
            };
            let pts = ticks_to_us(self.pos * scale.1, scale.0);
            let end = ticks_to_us((self.pos + dur_ticks) * scale.1, scale.0);
            out.push(Packet {
                track: id,
                pts_us: pts,
                dts_us: pts,
                duration_us: end - pts,
                keyframe: key,
                data,
            });
            self.pos += dur_ticks;
            self.seen_data = true;
        }
        out
    }
}

/// Длительность Opus-пакета в отсчётах 48 кГц по TOC (RFC 6716 §3.1).
fn opus_samples(data: &[u8]) -> i64 {
    let Some(&toc) = data.first() else { return 0 };
    let config = toc >> 3;
    let frame = match config {
        0..=11 => [480, 960, 1920, 2880][usize::from(config & 3)],
        12..=15 => [480, 960][usize::from(config & 1)],
        _ => [120, 240, 480, 960][usize::from(config & 3)],
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => i64::from(data.get(1).map_or(0, |b| b & 0x3f)),
    };
    frame * frames
}

/// Три заголовка в упаковке Xiph (как `CodecPrivate` Vorbis в Matroska):
/// число-1, длины первых двух, затем сами заголовки.
fn xiph_lacing(headers: &[Vec<u8>]) -> Vec<u8> {
    if headers.len() < 3 {
        return headers.concat();
    }
    let mut out = vec![2u8];
    for h in &headers[..2] {
        let mut n = h.len();
        while n >= 255 {
            out.push(255);
            n -= 255;
        }
        out.push(n as u8);
    }
    for h in headers {
        out.extend_from_slice(h);
    }
    out
}

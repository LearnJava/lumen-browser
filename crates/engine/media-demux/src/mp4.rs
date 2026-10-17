//! MP4 / ISOBMFF: таблицы `moov` разбирает `mp4parse`, фрагменты `moof`
//! (`mp4parse` их не читает) и описание видеозаписей `stsd` (HEVC, `vpcC`,
//! `av1C` — `mp4parse` скрывает сырые боксы) — свой разбор в этом файле.

use std::collections::HashMap;
use std::io::Cursor;

use mp4parse::{
    AudioCodecSpecific, CodecType, SampleEntry, TimeOffsetVersion, Track, TrackType,
};

use crate::bytes::{self, Be, BoxRef};
use crate::codec;
use crate::{ticks_to_us, Codec, DemuxError, Packet, Result, TrackInfo, TrackKind};

/// Верхняя граница числа сэмплов: защита от файлов-бомб с гигантскими счётчиками.
const MAX_SAMPLES: usize = 16_000_000;

struct Sample {
    track: usize,
    offset: u64,
    size: u32,
    pts_us: i64,
    dts_us: i64,
    dur_us: i64,
    key: bool,
}

/// Сэмпл до перевода в микросекунды (в тиках шкалы дорожки).
struct RawSample {
    offset: u64,
    size: u32,
    dts: i64,
    cto: i64,
    dur: i64,
    key: bool,
}

/// Состояние дорожки при сборке индекса.
struct TrackState {
    id: u32,
    timescale: u64,
    /// `media_time` первой правки `elst`, тики дорожки.
    shift: i64,
    /// Пустая правка в начале, мкс.
    empty_us: i64,
    next_dts: i64,
    raw: Vec<RawSample>,
}

/// Умолчания из `trex` (для фрагментов).
#[derive(Clone, Copy, Default)]
struct Trex {
    duration: u32,
    size: u32,
    flags: u32,
}

pub(crate) struct Mp4<'a> {
    bytes: &'a [u8],
    tracks: Vec<TrackInfo>,
    duration_us: Option<i64>,
    /// В порядке смещений в файле.
    samples: Vec<Sample>,
    cursor: usize,
    /// Нижняя граница смещения по дорожке после перемотки.
    floor: Vec<u64>,
}

impl<'a> Mp4<'a> {
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self> {
        let ctx = mp4parse::read_mp4(&mut Cursor::new(bytes))
            .map_err(|e| DemuxError::Malformed(format!("mp4: {e}")))?;
        let top = bytes::children(bytes, 0);
        let moov = top
            .iter()
            .find(|b| &b.kind == b"moov")
            .ok_or_else(|| DemuxError::Malformed("нет бокса moov".into()))?;
        let movie_ts = ctx.timescale.map_or(0, |t| t.0);
        let trex = read_trex(moov);

        let mut tracks = Vec::new();
        let mut states: Vec<TrackState> = Vec::new();
        let mut table_end_us = 0i64;
        let mut encrypted = false;
        for t in ctx.tracks.iter() {
            let Some(id) = t.track_id else { continue };
            let kind = match t.track_type {
                TrackType::Video => TrackKind::Video,
                TrackType::Audio => TrackKind::Audio,
                _ => continue,
            };
            let Some(timescale) = t.timescale.map(|s| s.0).filter(|&s| s > 0) else {
                continue;
            };
            let info = match kind {
                TrackKind::Video => video_info(moov, id, t),
                TrackKind::Audio => audio_info(id, t),
            };
            let Some(info) = info else {
                encrypted |= is_protected(t);
                continue;
            };
            let shift = t.media_time.map_or(0, |m| i64::try_from(m.0).unwrap_or(0));
            let empty_us = match (t.empty_duration, movie_ts) {
                (Some(e), ts) if ts > 0 => ticks_to_us(i64::try_from(e.0).unwrap_or(0), ts),
                _ => 0,
            };
            let raw = table_samples(t)?;
            if let Some(d) = t.edited_duration.filter(|_| movie_ts > 0) {
                let d = ticks_to_us(i64::try_from(d.0).unwrap_or(0), movie_ts);
                table_end_us = table_end_us.max(d);
            } else if let Some(d) = t.duration {
                table_end_us =
                    table_end_us.max(ticks_to_us(i64::try_from(d.0).unwrap_or(0), timescale));
            }
            let next_dts = raw.last().map_or(0, |s| s.dts + s.dur);
            tracks.push(info);
            states.push(TrackState {
                id,
                timescale,
                shift,
                empty_us,
                next_dts,
                raw,
            });
        }
        if tracks.is_empty() {
            return Err(DemuxError::Unsupported(if encrypted {
                "зашифрованные дорожки (DRM)".into()
            } else {
                "в MP4 нет видео- и аудиодорожек".into()
            }));
        }

        let mut fragmented = false;
        for moof in top.iter().filter(|b| &b.kind == b"moof") {
            fragmented = true;
            read_moof(moof, &trex, &mut states)?;
        }

        let mut samples = Vec::new();
        let mut fragment_end_us = 0i64;
        for (idx, st) in states.iter().enumerate() {
            for r in &st.raw {
                let dts_us = ticks_to_us(r.dts - st.shift, st.timescale) + st.empty_us;
                let pts_us = ticks_to_us(r.dts + r.cto - st.shift, st.timescale) + st.empty_us;
                let dur_us = ticks_to_us(r.dur, st.timescale);
                fragment_end_us = fragment_end_us.max(pts_us + dur_us);
                samples.push(Sample {
                    track: idx,
                    offset: r.offset,
                    size: r.size,
                    pts_us,
                    dts_us,
                    dur_us,
                    key: r.key,
                });
            }
        }
        samples.sort_by_key(|s| s.offset);
        let duration_us = if fragmented || table_end_us == 0 {
            Some(fragment_end_us.max(table_end_us).max(header_duration_us(moov)))
        } else {
            Some(table_end_us)
        };
        let floor = vec![0; tracks.len()];
        Ok(Self {
            bytes,
            tracks,
            duration_us,
            samples,
            cursor: 0,
            floor,
        })
    }

    pub(crate) fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    pub(crate) fn duration_us(&self) -> Option<i64> {
        self.duration_us
    }

    pub(crate) fn next_packet(&mut self) -> Result<Option<Packet>> {
        while let Some(s) = self.samples.get(self.cursor) {
            self.cursor += 1;
            if s.offset < self.floor[s.track] {
                continue;
            }
            let data = usize::try_from(s.offset)
                .ok()
                .and_then(|o| Some(o..o.checked_add(usize::try_from(s.size).ok()?)?))
                .and_then(|r| self.bytes.get(r))
                .ok_or_else(|| DemuxError::Malformed("сэмпл за пределами файла".into()))?;
            return Ok(Some(Packet {
                track: self.tracks[s.track].id,
                pts_us: s.pts_us,
                dts_us: s.dts_us,
                duration_us: s.dur_us,
                keyframe: s.key,
                data: data.to_vec(),
            }));
        }
        Ok(None)
    }

    pub(crate) fn seek_us(&mut self, target_us: i64) -> Result<()> {
        let mut floor: Vec<Option<u64>> = vec![None; self.tracks.len()];
        let mut first: Vec<u64> = vec![u64::MAX; self.tracks.len()];
        for s in &self.samples {
            first[s.track] = first[s.track].min(s.offset);
            if s.key && s.pts_us <= target_us {
                floor[s.track] = Some(s.offset);
            }
        }
        self.floor = floor
            .iter()
            .zip(&first)
            .map(|(f, &fst)| f.unwrap_or(fst))
            .collect();
        let start = self.floor.iter().copied().min().unwrap_or(0);
        self.cursor = self.samples.partition_point(|s| s.offset < start);
        Ok(())
    }
}

fn is_protected(t: &Track) -> bool {
    matches!(
        t.stsd.as_ref().and_then(|s| s.descriptions.first()),
        Some(SampleEntry::Video(v)) if !v.protection_info.is_empty()
    ) || matches!(
        t.stsd.as_ref().and_then(|s| s.descriptions.first()),
        Some(SampleEntry::Audio(a)) if !a.protection_info.is_empty()
    )
}

fn audio_info(id: u32, t: &Track) -> Option<TrackInfo> {
    let SampleEntry::Audio(a) = t.stsd.as_ref()?.descriptions.first()? else {
        return None;
    };
    if !a.protection_info.is_empty() {
        return None;
    }
    let mut sample_rate = a.samplerate as u32;
    let mut channels = u16::try_from(a.channelcount).unwrap_or(0);
    let (codec, codec_string, extradata) = match (&a.codec_type, &a.codec_specific) {
        (CodecType::AAC, AudioCodecSpecific::ES_Descriptor(es)) => {
            let asc = es.decoder_specific_data.to_vec();
            let aot = es
                .audio_object_type
                .or_else(|| codec::aot_from_asc(&asc))
                .unwrap_or(2);
            if let Some(r) = es.audio_sample_rate {
                sample_rate = r;
            }
            if let Some(c) = es.audio_channel_count {
                channels = c;
            }
            (Codec::Aac, codec::aac_string(aot), asc)
        }
        (CodecType::MP3, _) => (Codec::Mp3, "mp3".to_string(), Vec::new()),
        (CodecType::Opus, AudioCodecSpecific::OpusSpecificBox(o)) => {
            let mut head = Vec::new();
            mp4parse::serialize_opus_header(o, &mut head).ok()?;
            // OpusHead: версия(1) каналы(1) pre-skip(2) вход-частота(4) …
            if let Some(&c) = head.get(9) {
                channels = u16::from(c);
            }
            sample_rate = 48_000;
            (Codec::Opus, "opus".to_string(), head)
        }
        (CodecType::FLAC, AudioCodecSpecific::FLACSpecificBox(f)) => {
            let info = f
                .blocks
                .iter()
                .find(|b| b.block_type == 0)
                .map(|b| b.data.to_vec())
                .unwrap_or_default();
            (Codec::Flac, "flac".to_string(), info)
        }
        _ => return None,
    };
    Some(TrackInfo {
        id,
        kind: TrackKind::Audio,
        codec,
        codec_string,
        extradata,
        width: 0,
        height: 0,
        sample_rate,
        channels,
    })
}

/// Видеодорожка по сырому `stsd`: тип записи, размеры и конфигурационный бокс.
fn video_info(moov: &BoxRef<'_>, id: u32, t: &Track) -> Option<TrackInfo> {
    if is_protected(t) {
        return None;
    }
    let entry = video_entry(moov, id)?;
    let fourcc = entry.kind;
    let config = |kind: &[u8; 4]| -> Option<Vec<u8>> {
        // Содержимое записи: 78 байт полей `VisualSampleEntry`, дальше боксы.
        let body = entry.payload.get(78..)?;
        bytes::find(body, 0, kind).map(|b| b.payload.to_vec())
    };
    let mut width = 0;
    let mut height = 0;
    if let Some(body) = entry.payload.get(24..28) {
        width = u32::from(u16::from_be_bytes([body[0], body[1]]));
        height = u32::from(u16::from_be_bytes([body[2], body[3]]));
    }
    if let Some(h) = t.tkhd.as_ref().filter(|_| width == 0 || height == 0) {
        width = h.width >> 16;
        height = h.height >> 16;
    }
    let (codec, codec_string, extradata) = match &fourcc {
        b"avc1" | b"avc3" => {
            let c = config(b"avcC")?;
            (Codec::H264, codec::avc_string(&fourcc, &c), c)
        }
        b"hvc1" | b"hev1" => {
            let c = config(b"hvcC")?;
            (Codec::Hevc, codec::hevc_string(&fourcc, &c), c)
        }
        b"av01" => {
            let c = config(b"av1C")?;
            (Codec::Av1, codec::av1_string_from_config(&c), c)
        }
        b"vp09" => {
            let c = config(b"vpcC").unwrap_or_default();
            (Codec::Vp9, codec::vp9_string(&c), c)
        }
        b"vp08" => (Codec::Vp8, "vp8".to_string(), config(b"vpcC").unwrap_or_default()),
        _ => return None,
    };
    Some(TrackInfo {
        id,
        kind: TrackKind::Video,
        codec,
        codec_string,
        extradata,
        width,
        height,
        sample_rate: 0,
        channels: 0,
    })
}

/// Первая запись `stsd` дорожки `id` как `BoxRef` (тип = fourcc записи).
fn video_entry<'a>(moov: &BoxRef<'a>, id: u32) -> Option<BoxRef<'a>> {
    for trak in bytes::children(moov.payload, moov.payload_start) {
        if &trak.kind != b"trak" || track_id_of(&trak) != Some(id) {
            continue;
        }
        let stsd = bytes::find_path(
            trak.payload,
            trak.payload_start,
            &[b"mdia", b"minf", b"stbl", b"stsd"],
        )?;
        // FullBox (4) + entry_count (4), затем записи.
        let entries = stsd.payload.get(8..)?;
        return bytes::children(entries, stsd.payload_start + 8)
            .into_iter()
            .next();
    }
    None
}

fn track_id_of(trak: &BoxRef<'_>) -> Option<u32> {
    let tkhd = bytes::find(trak.payload, trak.payload_start, b"tkhd")?;
    let mut r = Be::new(tkhd.payload);
    let version = r.u8()?;
    r.skip(3)?;
    r.skip(if version == 1 { 16 } else { 8 })?;
    r.u32()
}

/// Длительность фильма из заголовков: `mvex/mehd` (фрагментированный файл), иначе
/// `mvhd`. Нужна, пока кадров ещё нет (MSE: init-сегмент без `moof`). `0` — не задана.
fn header_duration_us(moov: &BoxRef<'_>) -> i64 {
    let Some(mvhd) = bytes::find(moov.payload, moov.payload_start, b"mvhd") else { return 0 };
    // mvhd: v0 — ctime, mtime (по 4), timescale (4), duration (4); v1 — 8, 8, 4, 8.
    let mut r = Be::new(mvhd.payload);
    let parsed = (|| {
        let version = r.u8()?;
        r.skip(3)?;
        r.skip(if version == 1 { 16 } else { 8 })?;
        let timescale = r.u32()?;
        let duration = if version == 1 { r.u64()? } else { u64::from(r.u32()?) };
        Some((timescale, duration))
    })();
    let Some((timescale, mvhd_duration)) = parsed.filter(|(ts, _)| *ts > 0) else { return 0 };
    let mehd_duration = bytes::find(moov.payload, moov.payload_start, b"mvex")
        .and_then(|mvex| bytes::find(mvex.payload, mvex.payload_start, b"mehd"))
        .and_then(|mehd| {
            let mut r = Be::new(mehd.payload);
            let version = r.u8()?;
            r.skip(3)?;
            if version == 1 { r.u64() } else { r.u32().map(u64::from) }
        })
        .unwrap_or(0);
    let d = if mehd_duration > 0 { mehd_duration } else { mvhd_duration };
    if d == 0 || d == u64::from(u32::MAX) || d == u64::MAX {
        return 0;
    }
    ticks_to_us(i64::try_from(d).unwrap_or(0), u64::from(timescale))
}

fn read_trex(moov: &BoxRef<'_>) -> HashMap<u32, Trex> {
    let mut out = HashMap::new();
    let Some(mvex) = bytes::find(moov.payload, moov.payload_start, b"mvex") else {
        return out;
    };
    for b in bytes::children(mvex.payload, mvex.payload_start) {
        if &b.kind != b"trex" {
            continue;
        }
        let mut r = Be::new(b.payload);
        let parsed = (|| {
            r.skip(4)?;
            let id = r.u32()?;
            r.skip(4)?;
            Some((id, r.u32()?, r.u32()?, r.u32()?))
        })();
        if let Some((id, duration, size, flags)) = parsed {
            out.insert(id, Trex { duration, size, flags });
        }
    }
    out
}

/// Сэмплы дорожки из таблиц `stts/ctts/stsz/stsc/stco/stss`.
fn table_samples(t: &Track) -> Result<Vec<RawSample>> {
    let (Some(stts), Some(stsz), Some(stsc), Some(stco)) =
        (&t.stts, &t.stsz, &t.stsc, &t.stco)
    else {
        return Ok(Vec::new());
    };
    let stts_total: u64 = stts.samples.iter().map(|e| u64::from(e.sample_count)).sum();
    let count = if stsz.sample_size == 0 {
        stsz.sample_sizes.len()
    } else {
        usize::try_from(stts_total).unwrap_or(usize::MAX)
    };
    if count > MAX_SAMPLES {
        return Err(DemuxError::Malformed("слишком много сэмплов".into()));
    }
    let size_of = |i: usize| {
        if stsz.sample_size == 0 {
            stsz.sample_sizes.get(i).copied().unwrap_or(0)
        } else {
            stsz.sample_size
        }
    };

    // Тайминг: dts/длительность по stts.
    let mut timing: Vec<(i64, i64)> = Vec::with_capacity(count);
    let mut dts = 0i64;
    'stts: for e in stts.samples.iter() {
        for _ in 0..e.sample_count {
            if timing.len() == count {
                break 'stts;
            }
            timing.push((dts, i64::from(e.sample_delta)));
            dts += i64::from(e.sample_delta);
        }
    }
    // Смещения: по чанкам из stsc.
    let mut offsets: Vec<u64> = Vec::with_capacity(count);
    let runs = &stsc.samples;
    'chunks: for (ci, &chunk_off) in stco.offsets.iter().enumerate() {
        let chunk_no = u32::try_from(ci + 1).unwrap_or(u32::MAX);
        let per_chunk = runs
            .iter()
            .take_while(|r| r.first_chunk <= chunk_no)
            .last()
            .map_or(0, |r| r.samples_per_chunk);
        let mut off = chunk_off;
        for _ in 0..per_chunk {
            if offsets.len() == count {
                break 'chunks;
            }
            offsets.push(off);
            off += u64::from(size_of(offsets.len() - 1));
        }
    }
    let n = timing.len().min(offsets.len()).min(count);

    let mut cto = vec![0i64; n];
    if let Some(ctts) = &t.ctts {
        let mut i = 0usize;
        'ctts: for e in ctts.samples.iter() {
            let v = match e.time_offset {
                TimeOffsetVersion::Version0(v) => i64::from(v),
                TimeOffsetVersion::Version1(v) => i64::from(v),
            };
            for _ in 0..e.sample_count {
                if i == n {
                    break 'ctts;
                }
                cto[i] = v;
                i += 1;
            }
        }
    }
    let mut key = vec![t.stss.is_none(); n];
    if let Some(stss) = &t.stss {
        for &s in stss.samples.iter() {
            if let Some(k) = usize::try_from(s).ok().and_then(|s| s.checked_sub(1)).and_then(|s| key.get_mut(s)) {
                *k = true;
            }
        }
    }
    Ok((0..n)
        .map(|i| RawSample {
            offset: offsets[i],
            size: size_of(i),
            dts: timing[i].0,
            cto: cto[i],
            dur: timing[i].1,
            key: key[i],
        })
        .collect())
}

/// Фрагмент `moof`: дописывает сэмплы в `states`.
fn read_moof(moof: &BoxRef<'_>, trex: &HashMap<u32, Trex>, states: &mut [TrackState]) -> Result<()> {
    let bad = || DemuxError::Malformed("повреждённый moof".into());
    for traf in bytes::children(moof.payload, moof.payload_start) {
        if &traf.kind != b"traf" {
            continue;
        }
        let parts = bytes::children(traf.payload, traf.payload_start);
        let tfhd = parts.iter().find(|b| &b.kind == b"tfhd").ok_or_else(bad)?;
        let mut r = Be::new(tfhd.payload);
        let flags = r.u32().ok_or_else(bad)? & 0x00ff_ffff;
        let track_id = r.u32().ok_or_else(bad)?;
        let base = if flags & 0x1 != 0 {
            r.u64().ok_or_else(bad)?
        } else {
            moof.start as u64
        };
        if flags & 0x2 != 0 {
            r.skip(4).ok_or_else(bad)?;
        }
        let mut def = trex.get(&track_id).copied().unwrap_or_default();
        if flags & 0x8 != 0 {
            def.duration = r.u32().ok_or_else(bad)?;
        }
        if flags & 0x10 != 0 {
            def.size = r.u32().ok_or_else(bad)?;
        }
        if flags & 0x20 != 0 {
            def.flags = r.u32().ok_or_else(bad)?;
        }
        let Some(st) = states.iter_mut().find(|s| s.id == track_id) else {
            continue; // дорожка, которую мы не отдаём (субтитры и т. п.)
        };
        if let Some(tfdt) = parts.iter().find(|b| &b.kind == b"tfdt") {
            let mut r = Be::new(tfdt.payload);
            let version = r.u8().ok_or_else(bad)?;
            r.skip(3).ok_or_else(bad)?;
            let t = if version == 1 {
                r.u64().ok_or_else(bad)?
            } else {
                u64::from(r.u32().ok_or_else(bad)?)
            };
            st.next_dts = i64::try_from(t).map_err(|_| bad())?;
        }
        let mut next_off = base;
        for trun in parts.iter().filter(|b| &b.kind == b"trun") {
            let mut r = Be::new(trun.payload);
            let vf = r.u32().ok_or_else(bad)?;
            let (version, flags) = (vf >> 24, vf & 0x00ff_ffff);
            let n = r.u32().ok_or_else(bad)? as usize;
            if n > MAX_SAMPLES || st.raw.len() + n > MAX_SAMPLES {
                return Err(DemuxError::Malformed("слишком много сэмплов".into()));
            }
            if flags & 0x1 != 0 {
                let d = i64::from(r.i32().ok_or_else(bad)?);
                next_off = u64::try_from(i64::try_from(base).map_err(|_| bad())? + d)
                    .map_err(|_| bad())?;
            }
            let first_flags = if flags & 0x4 != 0 {
                Some(r.u32().ok_or_else(bad)?)
            } else {
                None
            };
            for i in 0..n {
                let dur = if flags & 0x100 != 0 { r.u32().ok_or_else(bad)? } else { def.duration };
                let size = if flags & 0x200 != 0 { r.u32().ok_or_else(bad)? } else { def.size };
                let sflags = if flags & 0x400 != 0 {
                    r.u32().ok_or_else(bad)?
                } else if i == 0 {
                    first_flags.unwrap_or(def.flags)
                } else {
                    def.flags
                };
                let cto = if flags & 0x800 != 0 {
                    let v = r.u32().ok_or_else(bad)?;
                    if version == 0 { i64::from(v) } else { i64::from(v as i32) }
                } else {
                    0
                };
                st.raw.push(RawSample {
                    offset: next_off,
                    size,
                    dts: st.next_dts,
                    cto,
                    dur: i64::from(dur),
                    key: sflags & 0x0001_0000 == 0,
                });
                st.next_dts += i64::from(dur);
                next_off += u64::from(size);
            }
        }
    }
    Ok(())
}

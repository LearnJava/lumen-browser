//! Модель `SourceBuffer` из Media Source Extensions (COMPAT-MSE).
//!
//! Крейт ничего не декодирует и не знает про JavaScript: он принимает байты из
//! `appendBuffer`, режет их на сегменты ([`units`]), разбирает кадры
//! демуксером [`lumen_media_demux`], хранит их по дорожкам и отвечает на то, что
//! спрашивает страница и плеер:
//!
//! ```text
//! appendBuffer(bytes) ─► Splitter ─► init segment  ─► дорожки (TrackInfo)
//!                                 └► media segment ─► Demuxer(init ++ сегмент) ─► кадры
//!                                                       │  timestampOffset, appendWindow,
//!                                                       │  вытеснение перекрытых кадров
//!                                                       ▼
//!   buffered() · remove() · frames(track) · highest_end_us()
//! ```
//!
//! Сегмент разбирается обычным демуксером, которому отдают `init ++ сегмент`
//! (для WebM — с собранным заголовком `Segment`): так MSE получает те же метки
//! времени, признаки ключевых кадров и конфигурации кодеков, что и воспроизведение
//! файла, без второго разборщика контейнеров. Время везде в микросекундах.

mod media_source;
mod units;

pub use media_source::{
    MediaSourceCore, MseReader, ReaderPoll, SharedMediaSource, TRACK_STRIDE, create_source, drop_source, source,
};

use std::fmt;
use std::sync::Arc;

use lumen_media_demux::{Demuxer, TrackInfo};

use crate::units::{Init, Unit};

/// Контейнер потока байтов (MSE Byte Stream Format Registry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// ISO BMFF: фрагментированный MP4 (`video/mp4`, `audio/mp4`).
    Mp4,
    /// WebM (`video/webm`, `audio/webm`).
    WebM,
}

/// Режим вставки кадров (`SourceBuffer.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppendMode {
    /// Метки времени берутся из потока (плюс `timestampOffset`).
    #[default]
    Segments,
    /// Каждая новая группа кадров встаёт сразу за концом предыдущей.
    Sequence,
}

/// Разобранный MIME-тип для `addSourceBuffer` / `isTypeSupported`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeType {
    /// Контейнер.
    pub format: Format,
    /// `video` или `audio` — первая часть MIME.
    pub is_video: bool,
    /// Кодеки из параметра `codecs`, как написаны (`avc1.64001f`, `mp4a.40.2`).
    pub codecs: Vec<String>,
}

/// Ошибка модели.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MseError {
    /// MIME-тип или набор дорожек не поддержан (`NotSupportedError`).
    Unsupported(String),
    /// Байты не разбираются как поток нужного контейнера: страница получает
    /// `error` и `MediaSource.endOfStream("decode")` (MSE «append error algorithm»).
    Decode(String),
}

impl fmt::Display for MseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(m) => write!(f, "не поддерживается: {m}"),
            Self::Decode(m) => write!(f, "ошибка разбора потока: {m}"),
        }
    }
}

impl std::error::Error for MseError {}

/// Разобрать MIME-тип вида `video/mp4; codecs="avc1.64001f, mp4a.40.2"`.
///
/// # Errors
/// [`MseError::Unsupported`] — контейнер не MP4/WebM или MIME-тип повреждён.
pub fn parse_mime(mime: &str) -> Result<MimeType, MseError> {
    let mut parts = mime.split(';');
    let essence = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let (format, is_video) = match essence.as_str() {
        "video/mp4" => (Format::Mp4, true),
        "audio/mp4" => (Format::Mp4, false),
        "video/webm" => (Format::WebM, true),
        "audio/webm" => (Format::WebM, false),
        other => return Err(MseError::Unsupported(format!("тип «{other}» не годится для MSE"))),
    };
    let mut codecs = Vec::new();
    for param in parts {
        let Some((name, value)) = param.split_once('=') else { continue };
        if name.trim().eq_ignore_ascii_case("codecs") {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            codecs.extend(value.split(',').map(str::trim).filter(|c| !c.is_empty()).map(str::to_owned));
        }
    }
    Ok(MimeType { format, is_video, codecs })
}

/// Закодированный кадр в буфере.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Момент показа (с учётом `timestampOffset`), мкс.
    pub pts_us: i64,
    /// Момент декодирования, мкс.
    pub dts_us: i64,
    /// Длительность, мкс (никогда не 0: недостающая достраивается по соседям).
    pub dur_us: i64,
    /// Точка входа: с неё можно начинать декодирование.
    pub key: bool,
    /// Данные кадра так, как лежат в контейнере (см. `Packet::data`).
    pub data: Arc<[u8]>,
}

impl Frame {
    /// Конец показа, мкс.
    #[must_use]
    pub fn end_us(&self) -> i64 {
        self.pts_us.saturating_add(self.dur_us)
    }
}

struct TrackBuf {
    info: TrackInfo,
    /// В порядке декодирования.
    frames: Vec<Frame>,
    /// Ждём ключевой кадр: до него кадры отбрасываются.
    need_rap: bool,
}

/// Итог одного `append`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppendOutcome {
    /// В этой порции пришёл init-сегмент (страница ждёт `loadedmetadata` /
    /// `sourceopen`-цепочку и `duration`).
    pub init_received: bool,
    /// Сколько кадров добавлено в буфер (после окна добавления и вытеснения).
    pub frames_added: usize,
}

/// Состояние одного `SourceBuffer`: разбор, дорожки и кадры.
pub struct SourceBufferModel {
    format: Format,
    splitter: units::Splitter,
    pending: Vec<u8>,
    init: Option<Init>,
    tracks: Vec<TrackBuf>,
    mode: AppendMode,
    timestamp_offset_us: i64,
    window_start_us: i64,
    window_end_us: i64,
    /// Следующий кадр открывает новую группу (после `abort()` и в начале).
    group_start: bool,
    /// Конец последнего принятого кадра: сюда встаёт следующая группа в `Sequence`.
    group_end_us: i64,
    generation: u64,
}

impl SourceBufferModel {
    /// Буфер под MIME-тип `mime`.
    ///
    /// # Errors
    /// [`MseError::Unsupported`] — см. [`parse_mime`].
    pub fn new(mime: &str) -> Result<Self, MseError> {
        let parsed = parse_mime(mime)?;
        Ok(Self::with_format(parsed.format))
    }

    /// Буфер под известный контейнер.
    #[must_use]
    pub fn with_format(format: Format) -> Self {
        Self {
            format,
            splitter: units::Splitter::new(format),
            pending: Vec::new(),
            init: None,
            tracks: Vec::new(),
            mode: AppendMode::Segments,
            timestamp_offset_us: 0,
            window_start_us: 0,
            window_end_us: i64::MAX,
            group_start: true,
            group_end_us: 0,
            generation: 0,
        }
    }

    /// Контейнер буфера.
    #[must_use]
    pub fn format(&self) -> Format {
        self.format
    }

    /// Дорожки последнего init-сегмента; пусто, пока он не пришёл.
    pub fn tracks(&self) -> impl Iterator<Item = &TrackInfo> {
        self.tracks.iter().map(|t| &t.info)
    }

    /// Получен ли init-сегмент (`[[first initialization segment received]]`).
    #[must_use]
    pub fn init_received(&self) -> bool {
        self.init.is_some()
    }

    /// Счётчик изменений буфера: растёт при каждом добавлении и удалении кадров.
    /// Плеер по нему понимает, что данные для перемотки изменились.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Кадры дорожки `track` в порядке декодирования.
    #[must_use]
    pub fn frames(&self, track: u32) -> &[Frame] {
        self.tracks.iter().find(|t| t.info.id == track).map_or(&[], |t| &t.frames)
    }

    /// Сколько байт кадров лежит в буфере (для `QuotaExceededError` и вытеснения).
    #[must_use]
    pub fn buffered_bytes(&self) -> usize {
        self.tracks.iter().flat_map(|t| &t.frames).map(|f| f.data.len()).sum()
    }

    /// Режим вставки.
    #[must_use]
    pub fn mode(&self) -> AppendMode {
        self.mode
    }

    /// Сменить режим вставки. Смена на `Sequence` делает следующую группу
    /// продолжением предыдущей.
    pub fn set_mode(&mut self, mode: AppendMode) {
        if mode == AppendMode::Sequence && self.mode != AppendMode::Sequence {
            self.group_start = true;
        }
        self.mode = mode;
    }

    /// `timestampOffset`, мкс.
    #[must_use]
    pub fn timestamp_offset_us(&self) -> i64 {
        self.timestamp_offset_us
    }

    /// Задать `timestampOffset`, мкс (в `Segments` действует на следующие кадры).
    pub fn set_timestamp_offset_us(&mut self, us: i64) {
        self.timestamp_offset_us = us;
    }

    /// Окно добавления `[start, end)`, мкс.
    #[must_use]
    pub fn append_window_us(&self) -> (i64, i64) {
        (self.window_start_us, self.window_end_us)
    }

    /// Задать окно добавления; кадры вне него в буфер не попадают.
    pub fn set_append_window_us(&mut self, start: i64, end: i64) {
        self.window_start_us = start;
        self.window_end_us = end;
    }

    /// `abort()`: выбросить недоразобранный сегмент, следующая группа кадров
    /// начинается с ключевого кадра. Принятый init-сегмент остаётся.
    pub fn abort(&mut self) {
        self.pending.clear();
        self.group_start = true;
        for t in &mut self.tracks {
            t.need_rap = true;
        }
    }

    /// Добавить байты.
    ///
    /// # Errors
    /// [`MseError::Decode`] — байты не разбираются или media-сегмент пришёл до
    /// init-сегмента; разбор при этом сбрасывается, уже принятые кадры остаются.
    pub fn append(&mut self, data: &[u8]) -> Result<AppendOutcome, MseError> {
        self.pending.extend_from_slice(data);
        let mut out = AppendOutcome::default();
        loop {
            let unit = match self.splitter.next(&mut self.pending) {
                Ok(Some(u)) => u,
                Ok(None) => return Ok(out),
                Err(e) => return Err(self.fail(e)),
            };
            let done = match unit {
                Unit::Init(init) => self.on_init(init, &mut out),
                Unit::Media(bytes) => self.on_media(&bytes, &mut out),
            };
            if let Err(e) = done {
                return Err(self.fail(e.to_string()));
            }
        }
    }

    fn fail(&mut self, why: String) -> MseError {
        self.pending.clear();
        self.splitter = units::Splitter::new(self.format);
        MseError::Decode(why)
    }

    fn on_init(&mut self, init: Init, out: &mut AppendOutcome) -> Result<(), MseError> {
        let file = units::assemble(self.format, &init, &[]);
        let demux = Demuxer::open(&file).map_err(|e| MseError::Decode(e.to_string()))?;
        let infos: Vec<TrackInfo> = demux.tracks().to_vec();
        if self.tracks.is_empty() {
            self.tracks = infos.into_iter().map(|info| TrackBuf { info, frames: Vec::new(), need_rap: true }).collect();
        } else {
            let same = infos.len() == self.tracks.len()
                && infos.iter().zip(&self.tracks).all(|(n, t)| n.id == t.info.id && n.kind == t.info.kind);
            if !same {
                return Err(MseError::Decode("новый init-сегмент меняет набор дорожек".into()));
            }
            for (t, n) in self.tracks.iter_mut().zip(infos) {
                t.info = n;
                t.need_rap = true;
            }
        }
        self.init = Some(init);
        out.init_received = true;
        Ok(())
    }

    fn on_media(&mut self, bytes: &[u8], out: &mut AppendOutcome) -> Result<(), MseError> {
        let init = self.init.as_ref().ok_or_else(|| MseError::Decode("media-сегмент раньше init-сегмента".into()))?;
        let file = units::assemble(self.format, init, bytes);
        let mut demux = Demuxer::open(&file).map_err(|e| MseError::Decode(e.to_string()))?;
        let mut per_track: Vec<Vec<Frame>> = vec![Vec::new(); self.tracks.len()];
        while let Some(p) = demux.next_packet().map_err(|e| MseError::Decode(e.to_string()))? {
            let Some(idx) = self.tracks.iter().position(|t| t.info.id == p.track) else { continue };
            per_track[idx].push(Frame {
                pts_us: p.pts_us,
                dts_us: p.dts_us,
                dur_us: p.duration_us.max(0),
                key: p.keyframe,
                data: Arc::from(p.data),
            });
        }
        for frames in &mut per_track {
            frames.sort_by_key(|f| f.dts_us);
            fill_durations(frames);
        }
        if let (AppendMode::Sequence, true, Some(first)) =
            (self.mode, self.group_start, per_track.iter().flatten().map(|f| f.pts_us).min())
        {
            self.timestamp_offset_us = self.group_end_us - first;
        }
        if per_track.iter().any(|f| !f.is_empty()) {
            self.group_start = false;
        }
        for (track, frames) in self.tracks.iter_mut().zip(per_track) {
            out.frames_added += absorb(
                track,
                frames,
                self.timestamp_offset_us,
                (self.window_start_us, self.window_end_us),
                &mut self.group_end_us,
            );
        }
        self.generation += 1;
        Ok(())
    }

    /// `remove(start, end)`: убрать кадры, чей показ начинается в `[start, end)`, и
    /// зависящие от них кадры до следующего ключевого.
    pub fn remove(&mut self, start_us: i64, end_us: i64) {
        for t in &mut self.tracks {
            remove_range(&mut t.frames, start_us, end_us);
        }
        self.generation += 1;
    }

    /// Конец самого дальнего кадра, мкс; 0 — буфер пуст.
    #[must_use]
    pub fn highest_end_us(&self) -> i64 {
        self.tracks.iter().flat_map(|t| t.frames.iter().map(Frame::end_us)).max().unwrap_or(0)
    }

    /// `SourceBuffer.buffered`: диапазоны, покрытые кадрами **всех** дорожек
    /// буфера (пересечение), мкс. Пустой, пока нет init-сегмента.
    #[must_use]
    pub fn buffered_us(&self) -> Vec<(i64, i64)> {
        let mut acc: Option<Vec<(i64, i64)>> = None;
        for t in &self.tracks {
            let r = track_ranges(&t.frames);
            acc = Some(match acc {
                None => r,
                Some(a) => intersect(&a, &r),
            });
        }
        acc.unwrap_or_default()
    }
}

/// Кадры без длительности (WebM без `Duration`) получают её по соседу: разность
/// меток показа; у последнего — как у предыдущего.
fn fill_durations(frames: &mut [Frame]) {
    let mut by_pts: Vec<usize> = (0..frames.len()).collect();
    by_pts.sort_by_key(|&i| frames[i].pts_us);
    let mut last = 0;
    for w in 0..by_pts.len() {
        let i = by_pts[w];
        if frames[i].dur_us == 0 {
            let next = by_pts.get(w + 1).map_or(0, |&j| frames[j].pts_us - frames[i].pts_us);
            frames[i].dur_us = if next > 0 { next } else { last };
        }
        last = frames[i].dur_us;
    }
}

/// Принять кадры одного media-сегмента в дорожку; вернуть, сколько осело.
fn absorb(track: &mut TrackBuf, frames: Vec<Frame>, offset: i64, window: (i64, i64), group_end: &mut i64) -> usize {
    let mut fresh: Vec<Frame> = Vec::with_capacity(frames.len());
    for mut f in frames {
        f.pts_us += offset;
        f.dts_us += offset;
        let inside = f.pts_us >= window.0 && f.end_us() <= window.1;
        if !inside {
            track.need_rap = true;
            continue;
        }
        if track.need_rap && !f.key {
            continue;
        }
        track.need_rap = false;
        fresh.push(f);
    }
    let (Some(lo), Some(hi)) = (fresh.iter().map(|f| f.pts_us).min(), fresh.iter().map(Frame::end_us).max()) else {
        return 0;
    };
    remove_range(&mut track.frames, lo, hi);
    *group_end = (*group_end).max(hi);
    let added = fresh.len();
    track.frames.extend(fresh);
    track.frames.sort_by_key(|f| f.dts_us);
    added
}

/// Убрать кадры с показом в `[start, end)` и некючевые кадры, оставшиеся без
/// опорного кадра, до следующего ключевого.
fn remove_range(frames: &mut Vec<Frame>, start: i64, end: i64) {
    let mut dropping = false;
    frames.retain(|f| {
        if f.pts_us >= start && f.pts_us < end {
            dropping = true;
            false
        } else if dropping && !f.key {
            false
        } else {
            dropping = false;
            true
        }
    });
}

/// Диапазоны показа одной дорожки: соседние кадры склеиваются, если зазор не
/// больше длительности предыдущего кадра (округление меток в контейнере).
fn track_ranges(frames: &[Frame]) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = frames.iter().map(|f| (f.pts_us, f.end_us())).collect();
    spans.sort_unstable();
    let mut out: Vec<(i64, i64)> = Vec::new();
    let mut tolerance = 0;
    for (s, e) in spans {
        match out.last_mut() {
            Some(cur) if s <= cur.1 + tolerance => cur.1 = cur.1.max(e),
            _ => out.push((s, e)),
        }
        tolerance = (e - s).max(1_000);
    }
    out
}

/// Пересечение двух упорядоченных наборов диапазонов.
fn intersect(a: &[(i64, i64)], b: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (s, e) = (a[i].0.max(b[j].0), a[i].1.min(b[j].1));
        if s < e {
            out.push((s, e));
        }
        if a[i].1 < b[j].1 { i += 1 } else { j += 1 }
    }
    out
}

#[cfg(test)]
mod tests;

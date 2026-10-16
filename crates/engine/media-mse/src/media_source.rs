//! `MediaSource` целиком: набор `SourceBuffer` одного объекта страницы и общий
//! для JS-слоя и плеера реестр таких объектов.
//!
//! Страница (поток V8) добавляет байты и читает `buffered`; плеер оболочки читает
//! пакеты ([`MseReader`]). Оба держат один [`SharedMediaSource`]; блокировка короткая —
//! разбор одного сегмента или выдача одного пакета.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use lumen_media_demux::{Packet, TrackInfo, TrackKind};

use crate::{Frame, MseError, SourceBufferModel};

/// Дорожки разных `SourceBuffer` нумеруются независимо (в fMP4 обе — `1`), поэтому
/// плееру они отдаются под номером `buffer * TRACK_STRIDE + id`.
pub const TRACK_STRIDE: u32 = 1000;

/// Состояние `MediaSource` без JS-обвязки.
#[derive(Default)]
pub struct MediaSourceCore {
    buffers: Vec<SourceBufferModel>,
    /// `MediaSource.duration`, мкс; `None` — `NaN` (ещё не задана).
    duration_us: Option<i64>,
    /// `endOfStream()` вызван: новых данных не будет, конец буфера — конец потока.
    ended: bool,
}

/// Общий дескриптор `MediaSource`.
pub type SharedMediaSource = Arc<Mutex<MediaSourceCore>>;

impl MediaSourceCore {
    /// `addSourceBuffer(type)`: номер нового буфера.
    ///
    /// # Errors
    /// [`MseError::Unsupported`] — контейнер не MP4/WebM.
    pub fn add_buffer(&mut self, mime: &str) -> Result<usize, MseError> {
        self.buffers.push(SourceBufferModel::new(mime)?);
        Ok(self.buffers.len() - 1)
    }

    /// `removeSourceBuffer`: буфер выбывает, номера остальных сохраняются
    /// (на месте остаётся пустая модель того же контейнера).
    pub fn remove_buffer(&mut self, index: usize) {
        if let Some(b) = self.buffers.get_mut(index) {
            *b = SourceBufferModel::with_format(b.format());
        }
    }

    /// Буфер под номером `index`.
    #[must_use]
    pub fn buffer(&self, index: usize) -> Option<&SourceBufferModel> {
        self.buffers.get(index)
    }

    /// Буфер под номером `index` для записи.
    pub fn buffer_mut(&mut self, index: usize) -> Option<&mut SourceBufferModel> {
        self.buffers.get_mut(index)
    }

    /// Сколько буферов создано.
    #[must_use]
    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    /// `MediaSource.duration = …`, мкс (`None` — `NaN`).
    pub fn set_duration_us(&mut self, us: Option<i64>) {
        self.duration_us = us;
    }

    /// Длительность, заданная страницей, мкс.
    #[must_use]
    pub fn duration_us(&self) -> Option<i64> {
        self.duration_us
    }

    /// `endOfStream()` / сброс при `sourceopen`.
    pub fn set_ended(&mut self, ended: bool) {
        self.ended = ended;
    }

    /// Вызван ли `endOfStream()`.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Конец самого дальнего кадра среди всех буферов, мкс.
    #[must_use]
    pub fn highest_end_us(&self) -> i64 {
        self.buffers.iter().map(SourceBufferModel::highest_end_us).max().unwrap_or(0)
    }

    /// Длительность для плеера: заданная страницей, а без неё — по данным.
    #[must_use]
    pub fn effective_duration_us(&self) -> i64 {
        let data = self.highest_end_us();
        self.duration_us.map_or(data, |d| d.max(0))
    }

    /// Дорожки всех буферов в нумерации плеера ([`TRACK_STRIDE`]).
    #[must_use]
    pub fn tracks(&self) -> Vec<TrackInfo> {
        let mut out = Vec::new();
        for (bi, b) in self.buffers.iter().enumerate() {
            for t in b.tracks() {
                let mut t = t.clone();
                t.id = player_track_id(bi, t.id);
                out.push(t);
            }
        }
        out
    }

    /// Есть ли хотя бы один видеокадр: с этого момента плеер может открыть сессию.
    #[must_use]
    pub fn has_video_frames(&self) -> bool {
        self.tracks()
            .iter()
            .any(|t| t.kind == TrackKind::Video && !self.frames_of(t.id).is_empty())
    }

    /// Кадры дорожки в нумерации плеера.
    fn frames_of(&self, player_track: u32) -> &[Frame] {
        let (bi, id) = split_track_id(player_track);
        self.buffers.get(bi).map_or(&[], |b| b.frames(id))
    }

    /// Следующий (по `dts`) кадр после `cursors` среди всех дорожек.
    fn peek(&self, cursors: &HashMap<u32, i64>) -> Option<(u32, &Frame)> {
        let mut best: Option<(u32, &Frame)> = None;
        for t in self.tracks() {
            let from = cursors.get(&t.id).copied().unwrap_or(i64::MIN);
            let frames = self.frames_of(t.id);
            let i = frames.partition_point(|f| f.dts_us < from);
            if let Some(f) = frames.get(i)
                && best.is_none_or(|(_, b)| f.dts_us < b.dts_us)
            {
                best = Some((t.id, f));
            }
        }
        best
    }
}

fn player_track_id(buffer: usize, id: u32) -> u32 {
    u32::try_from(buffer).unwrap_or(0).saturating_mul(TRACK_STRIDE).saturating_add(id)
}

fn split_track_id(player_track: u32) -> (usize, u32) {
    ((player_track / TRACK_STRIDE) as usize, player_track % TRACK_STRIDE)
}

/// Что вернул [`MseReader::next_packet`].
#[derive(Debug)]
pub enum ReaderPoll {
    /// Следующий пакет.
    Packet(Packet),
    /// Буфер кончился, но поток не закрыт: дальше будут `appendBuffer`.
    Pending,
    /// После `endOfStream()` данных больше нет.
    End,
}

/// Курсор плеера по кадрам `MediaSource` (аналог `Demuxer::next_packet` + `seek_us`).
pub struct MseReader {
    shared: SharedMediaSource,
    /// Следующий читаемый `dts` по дорожке (в нумерации плеера).
    cursors: HashMap<u32, i64>,
}

impl MseReader {
    /// Читатель с начала буфера.
    #[must_use]
    pub fn new(shared: SharedMediaSource) -> Self {
        Self { shared, cursors: HashMap::new() }
    }

    fn lock(&self) -> MutexGuard<'_, MediaSourceCore> {
        // Отравленная блокировка: модель остаётся пригодной (операции атомарны по смыслу).
        self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Дорожки в нумерации плеера.
    #[must_use]
    pub fn tracks(&self) -> Vec<TrackInfo> {
        self.lock().tracks()
    }

    /// Длительность, мкс.
    #[must_use]
    pub fn duration_us(&self) -> i64 {
        self.lock().effective_duration_us()
    }

    /// Закрыт ли поток (`endOfStream`).
    #[must_use]
    pub fn ended(&self) -> bool {
        self.lock().ended()
    }

    /// Следующий пакет по порядку `dts` среди дорожек.
    pub fn next_packet(&mut self) -> ReaderPoll {
        let core = self.lock();
        let Some((track, f)) = core.peek(&self.cursors) else {
            return if core.ended() { ReaderPoll::End } else { ReaderPoll::Pending };
        };
        let packet = Packet {
            track,
            pts_us: f.pts_us,
            dts_us: f.dts_us,
            duration_us: f.dur_us,
            keyframe: f.key,
            data: f.data.to_vec(),
        };
        drop(core);
        self.cursors.insert(track, packet.dts_us.saturating_add(1));
        ReaderPoll::Packet(packet)
    }

    /// Перемотка: видеодорожка — на ближайший ключевой кадр не позже `target_us`
    /// (а если такого нет — на первый кадр), остальные — на кадр, который
    /// показывается в `target_us`, но не раньше точки старта видео.
    pub fn seek_us(&mut self, target_us: i64) {
        let core = self.lock();
        let mut cursors = HashMap::new();
        let mut start = target_us;
        let tracks = core.tracks();
        for t in tracks.iter().filter(|t| t.kind == TrackKind::Video) {
            let frames = core.frames_of(t.id);
            let key = frames.iter().rposition(|f| f.key && f.pts_us <= target_us).or_else(|| (!frames.is_empty()).then_some(0));
            if let Some(i) = key {
                cursors.insert(t.id, frames[i].dts_us);
                start = start.min(frames[i].pts_us);
            }
        }
        for t in tracks.iter().filter(|t| t.kind != TrackKind::Video) {
            let frames = core.frames_of(t.id);
            let i = frames.iter().rposition(|f| f.pts_us <= start).unwrap_or(0);
            if let Some(f) = frames.get(i) {
                cursors.insert(t.id, f.dts_us);
            }
        }
        drop(core);
        self.cursors = cursors;
    }
}

// ── Реестр объектов ────────────────────────────────────────────────────────────

struct Hub {
    next: u64,
    sources: HashMap<u64, SharedMediaSource>,
}

fn hub() -> &'static Mutex<Hub> {
    static HUB: OnceLock<Mutex<Hub>> = OnceLock::new();
    HUB.get_or_init(|| Mutex::new(Hub { next: 1, sources: HashMap::new() }))
}

fn hub_lock() -> MutexGuard<'static, Hub> {
    hub().lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Завести новый `MediaSource` и вернуть его номер.
#[must_use]
pub fn create_source() -> u64 {
    let mut h = hub_lock();
    let id = h.next;
    h.next += 1;
    h.sources.insert(id, SharedMediaSource::default());
    id
}

/// `MediaSource` по номеру.
#[must_use]
pub fn source(id: u64) -> Option<SharedMediaSource> {
    hub_lock().sources.get(&id).cloned()
}

/// Забыть `MediaSource` (страница выгружена или объект собран).
pub fn drop_source(id: u64) {
    hub_lock().sources.remove(&id);
}

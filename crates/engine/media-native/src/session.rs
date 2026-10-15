//! Сессия воспроизведения: один демуксер, декодер видео и (если есть) декодер звука.
//!
//! Видео и звук читают **один** курсор демуксера: пакет чужой дорожки
//! откладывается в её очередь. Поэтому `frame_at` идёт вперёд без повторного
//! чтения файла (в отличие от пути «перемотать и декодировать с ключевого
//! кадра на каждый запрос»), а `decode_audio_pcm` продолжает с того места, куда
//! дошло видео.

use std::collections::VecDeque;
use std::sync::Arc;

use lumen_core::ext::{AudioTrackInfo, VideoDecodeSession};
use lumen_media_demux::{Demuxer, Packet, TrackInfo, TrackKind};

use crate::registry::{AudioPcmDecoder, DecodedFrame, Registry, VideoFrameDecoder};

/// Прыжок вперёд дальше этого от последнего декодированного кадра выполняется
/// перемоткой к ключевому кадру, а не декодированием всех кадров подряд.
const FORWARD_SEEK_THRESHOLD_US: i64 = 2_000_000;

/// Демуксер вместе с байтами, на которые он ссылается.
struct Source {
    // Поле объявлено первым, чтобы демуксер уничтожался раньше байтов.
    demux: Demuxer<'static>,
    _bytes: Arc<[u8]>,
}

impl Source {
    fn open(bytes: &[u8]) -> Result<Self, String> {
        let owned: Arc<[u8]> = Arc::from(bytes);
        // SAFETY: срез указывает в кучу `Arc<[u8]>`, который лежит в `Source`
        // рядом с демуксером и не изменяется до его уничтожения; перемещение
        // `Arc` не двигает данные, а порядок полей гарантирует, что `demux`
        // (единственный держатель среза) освобождается раньше `_bytes`.
        let slice: &'static [u8] = unsafe { std::slice::from_raw_parts(owned.as_ptr(), owned.len()) };
        let demux = Demuxer::open(slice).map_err(|e| format!("контейнер: {e}"))?;
        Ok(Self { demux, _bytes: owned })
    }
}

struct AudioPath {
    track: u32,
    decoder: Box<dyn AudioPcmDecoder>,
    info: AudioTrackInfo,
    /// Пакеты, прочитанные видео-стороной, но ещё не декодированные.
    queue: VecDeque<Packet>,
    /// Декодированные, но не отданные сэмплы (интерливленные).
    carry: VecDeque<i16>,
}

/// Насколько звук может отставать от последнего запрошенного кадра: пакеты,
/// закончившиеся раньше, устарели (перемотка вперёд внутри окна декодирования
/// без перечитывания файла) и отбрасываются.
const AUDIO_LAG_US: i64 = 400_000;

pub(crate) struct NativeSession {
    src: Source,
    video_track: u32,
    video: Box<dyn VideoFrameDecoder>,
    audio: Option<AudioPath>,
    track_size: (u32, u32),
    duration_us: Option<i64>,
    /// Видеопакеты, прочитанные звуковой стороной.
    vqueue: VecDeque<Packet>,
    /// Декодированные кадры, ещё не показанные (по возрастанию `pts`).
    pics: VecDeque<Box<dyn DecodedFrame>>,
    /// Последний показанный кадр и его RGBA (считается по первому запросу).
    current: Option<Box<dyn DecodedFrame>>,
    rgba: Option<Vec<u8>>,
    /// `pts` самого первого кадра файла: раньше него показывать нечего.
    first_pts_us: Option<i64>,
    /// Наибольший `pts` из уже декодированных кадров.
    head_us: i64,
    /// Следующий пакет — первый после открытия или перемотки.
    restart: bool,
    demux_eof: bool,
    drained: bool,
    /// Позиция последнего `frame_at`, мкс: по ней отсеивается устаревший звук.
    clock_us: i64,
}

impl NativeSession {
    pub(crate) fn open(registry: &'static Registry, bytes: &[u8]) -> Result<Self, String> {
        let src = Source::open(bytes)?;
        let tracks: Vec<TrackInfo> = src.demux.tracks().to_vec();

        let mut unsupported: Option<&TrackInfo> = None;
        let mut picked = None;
        for t in tracks.iter().filter(|t| t.kind == TrackKind::Video) {
            match registry.video_for(t.codec, &t.codec_string) {
                Some(backend) => {
                    picked = Some((t, backend));
                    break;
                }
                None => {
                    unsupported.get_or_insert(t);
                }
            }
        }
        let Some((vt, backend)) = picked else {
            return Err(match unsupported {
                Some(t) => format!("нет декодера для видео {}", describe(t)),
                None => "в контейнере нет видеодорожки".to_string(),
            });
        };
        let video = backend.create(vt)?;

        // Звук необязателен: дорожка, которую нечем декодировать, остаётся немой.
        let audio = tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio)
            .find_map(|t| registry.audio_for(t.codec, &t.codec_string).map(|b| (t, b)))
            .and_then(|(t, backend)| {
                let decoder = backend.create(t).ok()?;
                Some(AudioPath {
                    track: t.id,
                    decoder,
                    info: AudioTrackInfo { sample_rate: t.sample_rate, channels: t.channels },
                    queue: VecDeque::new(),
                    carry: VecDeque::new(),
                })
            });

        let duration_us = src.demux.duration_us();
        let mut session = Self {
            src,
            video_track: vt.id,
            video,
            audio,
            track_size: (vt.width, vt.height),
            duration_us,
            vqueue: VecDeque::new(),
            pics: VecDeque::new(),
            current: None,
            rgba: None,
            first_pts_us: None,
            head_us: 0,
            restart: true,
            demux_eof: false,
            drained: false,
            clock_us: 0,
        };
        // Первый кадр читается сразу: размер картинки известен только по нему.
        session.show_at(0)?;
        if session.current.is_none() {
            return Err("видеодорожка не дала ни одного кадра".to_string());
        }
        Ok(session)
    }

    /// Прочитать один пакет контейнера и разложить по очередям дорожек.
    fn read_demux(&mut self) -> Result<(), String> {
        match self.src.demux.next_packet() {
            Ok(Some(p)) => {
                if p.track == self.video_track {
                    self.vqueue.push_back(p);
                } else if let Some(a) = self.audio.as_mut().filter(|a| a.track == p.track) {
                    a.queue.push_back(p);
                }
                Ok(())
            }
            Ok(None) => {
                self.demux_eof = true;
                Ok(())
            }
            Err(e) => {
                // Повреждённый хвост файла не должен зацикливать чтение.
                self.demux_eof = true;
                Err(format!("контейнер: {e}"))
            }
        }
    }

    fn collect_frames(&mut self) {
        while let Some(f) = self.video.next_frame() {
            self.head_us = self.head_us.max(f.pts_us());
            self.first_pts_us.get_or_insert(f.pts_us());
            self.pics.push_back(f);
        }
    }

    /// Отдать декодеру следующий видеопакет (или слить конвейер в конце файла).
    fn pump_video(&mut self) -> Result<(), String> {
        let packet = loop {
            if let Some(p) = self.vqueue.pop_front() {
                break Some(p);
            }
            if self.demux_eof {
                break None;
            }
            self.read_demux()?;
        };
        match packet {
            Some(p) => {
                let restart = std::mem::take(&mut self.restart);
                let result = self.video.send(&p, restart);
                if result.is_err() && restart {
                    self.restart = true;
                }
                self.collect_frames();
                result
            }
            None => {
                self.drained = true;
                let result = self.video.drain();
                self.collect_frames();
                result
            }
        }
    }

    fn needs_seek(&self, target_us: i64) -> bool {
        match &self.current {
            Some(cur) => {
                let backwards = target_us < cur.pts_us() && self.first_pts_us != Some(cur.pts_us());
                backwards || target_us > self.head_us.saturating_add(FORWARD_SEEK_THRESHOLD_US)
            }
            None => false,
        }
    }

    fn seek(&mut self, target_us: i64) -> Result<(), String> {
        self.src.demux.seek_us(target_us).map_err(|e| format!("перемотка: {e}"))?;
        self.vqueue.clear();
        self.pics.clear();
        self.current = None;
        self.rgba = None;
        self.video.flush();
        if let Some(a) = self.audio.as_mut() {
            a.queue.clear();
            a.carry.clear();
            a.decoder.reset();
        }
        self.head_us = target_us;
        self.restart = true;
        self.demux_eof = false;
        self.drained = false;
        Ok(())
    }

    /// Сделать текущим последний кадр с `pts <= target_us` (а если такого нет —
    /// самый первый кадр файла).
    fn show_at(&mut self, target_us: i64) -> Result<(), String> {
        self.clock_us = target_us;
        if self.needs_seek(target_us) {
            self.seek(target_us)?;
        }
        loop {
            while let Some(front) = self.pics.front() {
                if front.pts_us() <= target_us || self.current.is_none() {
                    self.current = self.pics.pop_front();
                    self.rgba = None;
                } else {
                    break;
                }
            }
            // Следующий кадр уже позже `target_us`: текущий найден.
            if !self.pics.is_empty() || self.drained {
                return Ok(());
            }
            self.pump_video()?;
        }
    }

    fn next_audio_packet(&mut self) -> Result<Option<Packet>, String> {
        loop {
            if let Some(p) = self.audio.as_mut().and_then(|a| a.queue.pop_front()) {
                return Ok(Some(p));
            }
            if self.demux_eof {
                return Ok(None);
            }
            self.read_demux()?;
        }
    }
}

fn describe(t: &TrackInfo) -> String {
    if t.codec_string.is_empty() { format!("{:?}", t.codec) } else { t.codec_string.clone() }
}

impl VideoDecodeSession for NativeSession {
    fn dimensions(&self) -> (u32, u32) {
        self.current.as_ref().map_or(self.track_size, |f| (f.width(), f.height()))
    }

    fn duration_secs(&self) -> Option<f64> {
        self.duration_us.map(|us| us as f64 / 1_000_000.0)
    }

    fn frame_at(&mut self, secs: f64) -> Result<Vec<u8>, String> {
        let target_us = if secs.is_finite() { (secs.max(0.0) * 1_000_000.0) as i64 } else { 0 };
        self.show_at(target_us)?;
        if self.rgba.is_none() {
            let frame = self.current.as_ref().ok_or_else(|| "нет кадра на этой позиции".to_string())?;
            self.rgba = Some(frame.to_rgba8()?);
        }
        self.rgba.clone().ok_or_else(|| "нет кадра на этой позиции".to_string())
    }

    fn audio_track(&self) -> Option<AudioTrackInfo> {
        self.audio.as_ref().map(|a| a.info)
    }

    fn decode_audio_pcm(&mut self, max_samples: usize) -> Result<Vec<i16>, String> {
        let channels = usize::from(
            self.audio.as_ref().ok_or_else(|| "в контейнере нет аудиодорожки".to_string())?.info.channels,
        )
        .max(1);
        let want = max_samples.saturating_mul(channels);
        let mut last_error = None;
        while self.audio.as_ref().is_some_and(|a| a.carry.len() < want) {
            let Some(packet) = self.next_audio_packet()? else { break };
            let Some(a) = self.audio.as_mut() else { break };
            let end = packet.pts_us.saturating_add(packet.duration_us.max(20_000));
            if end < self.clock_us.saturating_sub(AUDIO_LAG_US) {
                a.decoder.reset();
                a.carry.clear();
                continue;
            }
            match a.decoder.decode(&packet) {
                Ok(pcm) => a.carry.extend(pcm),
                Err(e) => last_error = Some(e),
            }
        }
        let a = self.audio.as_mut().ok_or_else(|| "в контейнере нет аудиодорожки".to_string())?;
        let n = want.min(a.carry.len());
        let out: Vec<i16> = a.carry.drain(..n).collect();
        match (out.is_empty(), last_error) {
            (true, Some(e)) => Err(e),
            _ => Ok(out),
        }
    }
}


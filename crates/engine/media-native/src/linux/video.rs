//! Видео H.264 / HEVC через конвейер GStreamer (аппаратный VA-API или программный
//! `gst-libav` — выбирает `decodebin` по рангу).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use lumen_media_demux::{Packet, TrackInfo};

use super::pipeline::{Pipeline, Sample};
use crate::gst_caps::{video_caps, VIDEO_PIPELINE};
use crate::registry::{DecodedFrame, VideoFrameDecoder};
use crate::reorder::PtsQueue;
use crate::rgba::{pack_rgba, RgbaFrame};

/// Пока в декодере не больше стольких пакетов без кадра, `next_frame` не ждёт:
/// задержка вывода у декодеров (буфер опорных кадров, потоки `avdec`) — до 16 кадров.
const NO_WAIT_IN_FLIGHT: usize = 16;
/// Больше этого — декодер явно отстаёт: ждём кадр, а не копим пакеты.
const BLOCK_IN_FLIGHT: usize = 48;
/// Ожидание кадра между порогами.
const SHORT_WAIT: Duration = Duration::from_millis(2);
/// Ожидание кадра, когда декодер отстаёт.
const LONG_WAIT: Duration = Duration::from_secs(1);
/// Сколько ждать оставшиеся кадры после конца потока.
const DRAIN_WAIT: Duration = Duration::from_secs(5);
/// Шаг опроса `appsink` при сливе.
const DRAIN_POLL: Duration = Duration::from_millis(50);

/// Декодер одной видеодорожки.
pub(super) struct GstVideoDecoder {
    caps: String,
    /// `None` после перемотки (`flush`): пересоздаётся на следующем пакете.
    pipe: Option<Pipeline>,
    pts: PtsQueue,
    /// Кадры, вынутые из конвейера при `drain`.
    ready: VecDeque<Box<dyn DecodedFrame>>,
}

/// Образец `appsink` → кадр с очередной меткой показа.
fn frame_from(sample: &Sample, pts: &mut PtsQueue) -> Option<Box<dyn DecodedFrame>> {
    let width = u32::try_from(sample.caps_int(c"width")?).ok()?;
    let height = u32::try_from(sample.caps_int(c"height")?).ok()?;
    let rgba = sample.with_data(|data| pack_rgba(data, width, height))?.ok()?;
    Some(Box::new(RgbaFrame { pts_us: pts.frame(), width, height, rgba }))
}

impl GstVideoDecoder {
    pub(super) fn new(track: &TrackInfo) -> Result<Self, String> {
        let caps = video_caps(track)?;
        let pipe = Pipeline::new(VIDEO_PIPELINE, &caps)?;
        Ok(Self { caps, pipe: Some(pipe), pts: PtsQueue::default(), ready: VecDeque::new() })
    }
}

impl VideoFrameDecoder for GstVideoDecoder {
    fn send(&mut self, packet: &Packet, _restart: bool) -> Result<(), String> {
        if self.pipe.is_none() {
            self.pts.clear();
            self.ready.clear();
            self.pipe = Some(Pipeline::new(VIDEO_PIPELINE, &self.caps)?);
        }
        let Some(pipe) = self.pipe.as_ref() else { return Err("конвейер GStreamer не создан".to_string()) };
        if let Some(error) = pipe.error() {
            // Упавший конвейер не воскресить: следующий пакет откроет новый.
            self.pipe = None;
            return Err(error);
        }
        pipe.push(&packet.data)?;
        self.pts.sent(packet.pts_us);
        Ok(())
    }

    fn next_frame(&mut self) -> Option<Box<dyn DecodedFrame>> {
        if let Some(frame) = self.ready.pop_front() {
            return Some(frame);
        }
        let wait = match self.pts.in_flight() {
            0 => return None,
            1..=NO_WAIT_IN_FLIGHT => Duration::ZERO,
            ..=BLOCK_IN_FLIGHT => SHORT_WAIT,
            _ => LONG_WAIT,
        };
        let sample = self.pipe.as_ref()?.pull(wait)?;
        frame_from(&sample, &mut self.pts)
    }

    fn drain(&mut self) -> Result<(), String> {
        let Some(pipe) = self.pipe.as_ref() else { return Ok(()) };
        pipe.end_of_stream();
        // Декодеры сливают внутренние очереди: забираем всё до конца потока.
        let deadline = Instant::now() + DRAIN_WAIT;
        loop {
            match pipe.pull(DRAIN_POLL) {
                Some(sample) => {
                    if let Some(frame) = frame_from(&sample, &mut self.pts) {
                        self.ready.push_back(frame);
                    }
                }
                None if pipe.is_eos() || Instant::now() >= deadline => break,
                None => {}
            }
        }
        match pipe.error() {
            Some(error) if self.ready.is_empty() => Err(error),
            _ => Ok(()),
        }
    }

    fn flush(&mut self) {
        self.pipe = None;
        self.pts.clear();
        self.ready.clear();
    }
}

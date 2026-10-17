//! AAC через конвейер GStreamer (`avdec_aac` из gst-libav или `faad`): сырые кадры
//! AAC из MP4 → PCM i16.

use std::time::Duration;

use lumen_media_demux::{Packet, TrackInfo};

use super::pipeline::{Pipeline, Sample};
use crate::gst_caps::{aac_caps, audio_pipeline};
use crate::registry::AudioPcmDecoder;

/// Пока декодер не выдал ни одного блока, ждём первый до этого предела: запуск
/// конвейера и загрузка плагина занимают заметное время.
const FIRST_OUTPUT_WAIT: Duration = Duration::from_secs(1);
/// После первого блока вывод идёт сразу за вводом; столько ждём его на каждом пакете.
const STEADY_WAIT: Duration = Duration::from_millis(50);

/// Декодер одной звуковой дорожки AAC.
pub(super) struct GstAacDecoder {
    caps: String,
    pipeline: String,
    pipe: Pipeline,
    /// Конвейер уже выдавал PCM: дальше ждём вывод недолго.
    had_output: bool,
}

/// Копия байтов образца как интерливленные `i16` (S16LE на любой машине Lumen — little-endian).
fn append_samples(sample: &Sample, out: &mut Vec<i16>) {
    let _ = sample.with_data(|data| {
        out.extend(data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])));
    });
}

impl GstAacDecoder {
    pub(super) fn new(track: &TrackInfo) -> Result<Self, String> {
        let caps = aac_caps(track)?;
        let pipeline = audio_pipeline(track.sample_rate, track.channels);
        let pipe = Pipeline::new(&pipeline, &caps)?;
        Ok(Self { caps, pipeline, pipe, had_output: false })
    }

    fn restart(&mut self) -> Result<(), String> {
        self.pipe = Pipeline::new(&self.pipeline, &self.caps)?;
        self.had_output = false;
        Ok(())
    }
}

impl AudioPcmDecoder for GstAacDecoder {
    fn decode(&mut self, packet: &Packet) -> Result<Vec<i16>, String> {
        if let Some(error) = self.pipe.error() {
            // Упавший конвейер не воскресить: начинаем заново, ошибку отдаём вызывающему.
            self.restart()?;
            return Err(error);
        }
        self.pipe.push(&packet.data)?;
        let mut pcm = Vec::new();
        let first_wait = if self.had_output { STEADY_WAIT } else { FIRST_OUTPUT_WAIT };
        // Первый блок ждём с таймаутом, остальное, что уже готово, забираем без ожидания.
        if let Some(sample) = self.pipe.pull(first_wait) {
            self.had_output = true;
            append_samples(&sample, &mut pcm);
            while let Some(more) = self.pipe.pull(Duration::ZERO) {
                append_samples(&more, &mut pcm);
            }
        }
        Ok(pcm)
    }

    fn reset(&mut self) {
        // Перемотка: декодер забывает перекрытие окон — проще и надёжнее новый конвейер.
        // Если создать не удалось, следующий `decode` увидит ошибку на старом.
        let _ = self.restart();
    }
}

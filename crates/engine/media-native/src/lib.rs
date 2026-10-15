//! Собственный `VideoDecoder` Lumen — «lumen-native» (ADR-035 п.1): видео и звук
//! без FFmpeg.
//!
//! ```text
//! байты ─► lumen-media-demux (MP4 / WebM / Ogg) ─► пакеты
//!             │                                     │
//!             └─ NativeSession ◄─ registry::Registry ┘  ── видео: VideoBackend (AV1 → rav1d, …)
//!                  │                                  └─ звук:  AudioBackend (Vorbis → lewton, …)
//!                  └─ VideoDecodeSession: frame_at / decode_audio_pcm / audio_track
//! ```
//!
//! Что именно открывается, решает [`registry::Registry`] — по кодекам, которые
//! *на этой машине* умеет открыть хотя бы один бэкенд. Тот же реестр отвечает на
//! `canPlayType` ([`NativeVideoDecoder::can_play_type`]), поэтому страница не
//! получит «probably» на то, что не заиграет.

pub mod registry;

mod av1;
mod session;
mod vorbis;

use lumen_core::ext::{VideoDecodeSession, VideoDecoder};

use crate::registry::Registry;

/// `VideoDecoder` на встроенных декодерах и декодерах ОС (без FFmpeg).
#[derive(Debug, Clone, Copy)]
pub struct NativeVideoDecoder {
    registry: &'static Registry,
}

impl Default for NativeVideoDecoder {
    fn default() -> Self {
        Self { registry: registry::builtin() }
    }
}

impl NativeVideoDecoder {
    /// Декодер поверх произвольного реестра (тесты, подмена бэкендов).
    #[must_use]
    pub fn with_registry(registry: &'static Registry) -> Self {
        Self { registry }
    }
}

impl VideoDecoder for NativeVideoDecoder {
    fn decoder_name(&self) -> &'static str {
        "lumen-native"
    }

    fn mime_types(&self) -> &'static [&'static str] {
        &["video/mp4", "video/webm", "video/ogg"]
    }

    fn can_play_type(&self, mime: &str) -> &'static str {
        self.registry.can_play_type(mime)
    }

    fn open(&self, bytes: &[u8]) -> Result<Box<dyn VideoDecodeSession>, String> {
        Ok(Box::new(session::NativeSession::open(self.registry, bytes)?))
    }
}

#[cfg(test)]
mod tests;

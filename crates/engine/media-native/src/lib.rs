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

mod annexb;
mod av1;
#[cfg(windows)]
mod mf;
mod session;
mod vorbis;
mod yuv;

use lumen_core::ext::{VideoDecodeSession, VideoDecoder};

use crate::registry::Registry;

/// Какие патентованные кодеки есть у ОС (COMPAT-MEDIA-OSDEC-WIN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsCodecStatus {
    /// Декодер H.264 найден.
    pub h264: bool,
    /// Декодер HEVC найден (на Windows — расширение «HEVC Video Extensions»).
    pub hevc: bool,
    /// Декодер AAC найден.
    pub aac: bool,
}

impl OsCodecStatus {
    /// На Windows N/KN без Media Feature Pack нет ни H.264, ни AAC: видео из
    /// обычных сайтов не играет, и пользователю нужна подсказка об установке.
    #[must_use]
    pub fn media_feature_pack_missing(self) -> bool {
        !self.h264 && !self.aac
    }
}

/// Адрес страницы «Дополнительные компоненты» в Параметрах Windows (там ставится
/// Media Feature Pack).
pub const WINDOWS_OPTIONAL_FEATURES_URI: &str = "ms-settings:optionalfeatures";

/// Состояние декодеров ОС; `None` — на этой платформе бэкенда ОС нет.
#[must_use]
pub fn os_codec_status() -> Option<OsCodecStatus> {
    #[cfg(windows)]
    {
        let a = mf::availability();
        Some(OsCodecStatus { h264: a.h264, hevc: a.hevc, aac: a.aac })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

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

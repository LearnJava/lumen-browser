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

#[cfg(target_os = "macos")]
mod apple;
mod annexb;
mod av1;
mod esds;
#[cfg(windows)]
mod mf;
mod profile;
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

/// Чего не хватает ОС, чтобы открыть видео (COMPAT-MEDIA-OSDEC-WIN-BAR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MissingOsCodec {
    /// H.264 без Media Feature Pack (Windows N/KN).
    MediaFeaturePack,
    /// HEVC без расширения «HEVC Video Extensions» (Microsoft Store).
    HevcExtension,
}

impl MissingOsCodec {
    /// Адрес, где недостающий компонент ставится.
    #[must_use]
    pub const fn install_uri(self) -> &'static str {
        match self {
            Self::MediaFeaturePack => WINDOWS_OPTIONAL_FEATURES_URI,
            Self::HevcExtension => WINDOWS_STORE_HEVC_URI,
        }
    }
}

/// Поиск «HEVC Video Extensions» в Microsoft Store.
pub const WINDOWS_STORE_HEVC_URI: &str = "ms-windows-store://search/?query=HEVC%20Video%20Extensions";

/// Какой компонент ОС недостаёт видео с кодеком `video` при состоянии `status`.
/// `None` — кодек не зависит от ОС или декодер есть.
#[must_use]
pub fn missing_for_video_codec(video: lumen_media_demux::Codec, status: OsCodecStatus) -> Option<MissingOsCodec> {
    match video {
        lumen_media_demux::Codec::H264 if !status.h264 => Some(MissingOsCodec::MediaFeaturePack),
        lumen_media_demux::Codec::Hevc if !status.hevc => Some(MissingOsCodec::HevcExtension),
        _ => None,
    }
}

/// Причина, по которой `bytes` не открылись: первая видеодорожка зависит от
/// декодера ОС, которого нет. `None` — причина другая (или на платформе нет
/// бэкенда ОС). Зовётся только после сбоя `VideoDecoder::open`.
#[must_use]
pub fn missing_os_codec(bytes: &[u8]) -> Option<MissingOsCodec> {
    let status = os_codec_status()?;
    let demux = lumen_media_demux::Demuxer::open(bytes).ok()?;
    let video = demux.tracks().iter().find(|t| t.kind == lumen_media_demux::TrackKind::Video)?;
    missing_for_video_codec(video.codec, status)
}

/// Состояние декодеров ОС; `None` — на этой платформе бэкенда ОС нет или он не
/// требует от пользователя ставить компоненты. На macOS H.264/AAC входят в систему,
/// поэтому плашка «нужен компонент ОС» не нужна и подсказок об установке нет
/// (бэкенд VideoToolbox/AudioToolbox — `apple/`).
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

/// Сессия воспроизведения поверх буфера `MediaSource` (COMPAT-MSE): кадры берутся из
/// `source`, пока страница их добавляет; когда они кончаются раньше `endOfStream()`,
/// на экране остаётся последний кадр.
///
/// # Errors
/// Диагностика: в буфере нет видеокадров или для видео нет декодера.
pub fn open_mse(source: lumen_media_mse::SharedMediaSource) -> Result<Box<dyn VideoDecodeSession>, String> {
    let reader = lumen_media_mse::MseReader::new(source);
    Ok(Box::new(session::NativeSession::open_mse(registry::builtin(), reader)?))
}

#[cfg(test)]
mod tests;

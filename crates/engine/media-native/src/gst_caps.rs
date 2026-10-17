//! Описания конвейеров и «caps» GStreamer для дорожек MP4 (COMPAT-MEDIA-OSDEC-LINUX).
//!
//! Чистые строковые функции: что именно скажем `appsrc`, проверяется на любой ОС.
//! Пакеты MP4 уже с префиксами длины NAL (`avc`/`hvc1`), поэтому в Annex B их
//! не переводим — так же отдаёт кадры `qtdemux`.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use lumen_media_demux::{Codec, TrackInfo};

/// Видео: пакеты → `decodebin` (сам выберет аппаратный VA-API или программный
/// декодер по рангу) → RGBA → `appsink`. Преобразование цвета делает GStreamer
/// (матрица и диапазон — из потока), строки RGBA без выравнивания.
pub(crate) const VIDEO_PIPELINE: &str = "appsrc name=src format=time stream-type=stream block=false max-bytes=0 \
     ! decodebin ! videoconvert ! video/x-raw,format=RGBA \
     ! appsink name=sink sync=false max-buffers=4 drop=false";

/// Звук: пакеты → `decodebin` → S16LE с частотой и числом каналов дорожки.
pub(crate) fn audio_pipeline(sample_rate: u32, channels: u16) -> String {
    format!(
        "appsrc name=src format=time stream-type=stream block=false max-bytes=0 \
         ! decodebin ! audioconvert ! audioresample \
         ! audio/x-raw,format=S16LE,layout=interleaved,rate={sample_rate},channels={channels} \
         ! appsink name=sink sync=false max-buffers=16 drop=false"
    )
}

/// Шестнадцатеричная запись байтов для поля `(buffer)` в строке caps.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Caps входа `appsrc` для видеодорожки H.264 / HEVC.
///
/// # Errors
/// Кодек не H.264/HEVC.
pub(crate) fn video_caps(track: &TrackInfo) -> Result<String, String> {
    let mut caps = match track.codec {
        Codec::H264 => {
            // `avc3` — параметры в потоке, а не в `avcC`.
            let format = if track.codec_string.to_ascii_lowercase().starts_with("avc3") { "avc3" } else { "avc" };
            format!("video/x-h264,stream-format=(string){format},alignment=(string)au")
        }
        Codec::Hevc => {
            let format = if track.codec_string.to_ascii_lowercase().starts_with("hev1") { "hev1" } else { "hvc1" };
            format!("video/x-h265,stream-format=(string){format},alignment=(string)au")
        }
        other => return Err(format!("GStreamer-бэкенд не декодирует видео {other:?}")),
    };
    if !track.extradata.is_empty() {
        caps.push_str(",codec_data=(buffer)");
        caps.push_str(&hex(&track.extradata));
    }
    if track.width > 0 && track.height > 0 {
        caps.push_str(",width=(int)");
        caps.push_str(&track.width.to_string());
        caps.push_str(",height=(int)");
        caps.push_str(&track.height.to_string());
    }
    Ok(caps)
}

/// Caps входа `appsrc` для звуковой дорожки AAC (сырые кадры + `AudioSpecificConfig`).
///
/// # Errors
/// Кодек не AAC или у дорожки нет частоты / каналов.
pub(crate) fn aac_caps(track: &TrackInfo) -> Result<String, String> {
    if track.codec != Codec::Aac {
        return Err(format!("GStreamer-бэкенд не декодирует звук {:?}", track.codec));
    }
    if track.sample_rate == 0 || track.channels == 0 {
        return Err("у дорожки AAC нет частоты или числа каналов".to_string());
    }
    let mut caps = format!(
        "audio/mpeg,mpegversion=(int)4,stream-format=(string)raw,rate=(int){},channels=(int){}",
        track.sample_rate, track.channels
    );
    if !track.extradata.is_empty() {
        caps.push_str(",codec_data=(buffer)");
        caps.push_str(&hex(&track.extradata));
    }
    Ok(caps)
}

/// Caps декодера для проверки «умеет ли GStreamer этот кодек»: что `appsrc`
/// предъявит декодеру.
pub(crate) const H264_PROBE_CAPS: &str = "video/x-h264,stream-format=(string)avc,alignment=(string)au";
/// См. [`H264_PROBE_CAPS`].
pub(crate) const HEVC_PROBE_CAPS: &str = "video/x-h265,stream-format=(string)hvc1,alignment=(string)au";
/// См. [`H264_PROBE_CAPS`].
pub(crate) const AAC_PROBE_CAPS: &str = "audio/mpeg,mpegversion=(int)4,stream-format=(string)raw";

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_media_demux::TrackKind;

    fn track(codec: Codec, codec_string: &str, extradata: &[u8]) -> TrackInfo {
        TrackInfo {
            id: 1,
            kind: if codec == Codec::Aac { TrackKind::Audio } else { TrackKind::Video },
            codec,
            codec_string: codec_string.to_string(),
            extradata: extradata.to_vec(),
            width: 640,
            height: 360,
            sample_rate: 44_100,
            channels: 2,
        }
    }

    #[test]
    fn h264_caps_carry_avcc_and_size() {
        let caps = video_caps(&track(Codec::H264, "avc1.64001f", &[1, 0x64, 0, 0x1f, 0xff])).unwrap();
        assert!(caps.starts_with("video/x-h264,stream-format=(string)avc,alignment=(string)au"), "{caps}");
        assert!(caps.contains("codec_data=(buffer)0164001fff"), "{caps}");
        assert!(caps.ends_with("width=(int)640,height=(int)360"), "{caps}");
    }

    #[test]
    fn in_band_parameter_sets_use_the_avc3_and_hev1_formats() {
        assert!(video_caps(&track(Codec::H264, "avc3.640028", &[])).unwrap().contains("stream-format=(string)avc3"));
        assert!(!video_caps(&track(Codec::H264, "avc3.640028", &[])).unwrap().contains("codec_data"));
        assert!(video_caps(&track(Codec::Hevc, "hev1.1.6.L93.B0", &[1])).unwrap().contains("(string)hev1"));
        let hvc1 = video_caps(&track(Codec::Hevc, "hvc1.1.6.L93.B0", &[1])).unwrap();
        assert!(hvc1.starts_with("video/x-h265,stream-format=(string)hvc1"), "{hvc1}");
    }

    #[test]
    fn other_video_codecs_are_refused() {
        assert!(video_caps(&track(Codec::Av1, "av01.0.05M.08", &[])).is_err());
    }

    #[test]
    fn aac_caps_carry_audio_specific_config() {
        let caps = aac_caps(&track(Codec::Aac, "mp4a.40.2", &[0x12, 0x10])).unwrap();
        assert_eq!(
            caps,
            "audio/mpeg,mpegversion=(int)4,stream-format=(string)raw,rate=(int)44100,channels=(int)2,codec_data=(buffer)1210"
        );
        let mut silent = track(Codec::Aac, "mp4a.40.2", &[]);
        silent.channels = 0;
        assert!(aac_caps(&silent).is_err());
        assert!(aac_caps(&track(Codec::Mp3, "mp3", &[])).is_err());
    }

    #[test]
    fn audio_pipeline_pins_rate_and_channels() {
        let p = audio_pipeline(48_000, 6);
        assert!(p.contains("rate=48000,channels=6"), "{p}");
        assert!(p.contains("format=S16LE"));
    }
}

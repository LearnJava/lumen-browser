//! Какие профили кодеков обещают декодеры ОС (Media Foundation, VideoToolbox, …):
//! общий для платформ разбор строки `codecs=` (RFC 6381). `canPlayType` и
//! `decodingInfo` отвечают по нему одинаково на всех ОС.

#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

/// `avc1.PPCCLL`: профили, которые умеет декодер ОС (Baseline/Main/High 8 бит).
/// Строка без параметров («avc1») — годится: профиль выяснится по `avcC`.
pub(crate) fn h264_profile_supported(codec_string: &str) -> bool {
    let rest = codec_string.trim().to_ascii_lowercase();
    let Some(params) = rest.strip_prefix("avc1.").or_else(|| rest.strip_prefix("avc3.")) else {
        return rest == "avc1" || rest == "avc3";
    };
    match params.get(..2).and_then(|p| u8::from_str_radix(p, 16).ok()) {
        Some(profile_idc) => matches!(profile_idc, 0x42 | 0x4D | 0x64),
        None => false,
    }
}

/// `hvc1.<профиль>.…`: Main (1) и Main 10 (2); профиль может начинаться с
/// буквы пространства профилей (`A`/`B`/`C`) — таких потоков в вебе нет.
pub(crate) fn hevc_profile_supported(codec_string: &str) -> bool {
    let lower = codec_string.trim().to_ascii_lowercase();
    let Some(params) = lower.strip_prefix("hvc1").or_else(|| lower.strip_prefix("hev1")) else {
        return false;
    };
    match params.strip_prefix('.') {
        None => params.is_empty(),
        Some(p) => matches!(p.split('.').next(), Some("1" | "2")),
    }
}

/// `mp4a.40.2` (LC), `.5` (HE-AAC), `.29` (HE-AAC v2), `mp4a.67` (MPEG-2 AAC LC).
pub(crate) fn aac_object_supported(codec_string: &str) -> bool {
    matches!(codec_string.trim().to_ascii_lowercase().as_str(), "mp4a.40.2" | "mp4a.40.5" | "mp4a.40.29" | "mp4a.67")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_profiles() {
        assert!(h264_profile_supported("avc1.42e01e"));
        assert!(h264_profile_supported("avc1.4D401F"));
        assert!(h264_profile_supported("avc1.640028"));
        assert!(h264_profile_supported("avc1"));
        // High 10 / High 4:2:2 / Extended / мусор.
        assert!(!h264_profile_supported("avc1.6e0028"));
        assert!(!h264_profile_supported("avc1.7a0028"));
        assert!(!h264_profile_supported("avc1.580028"));
        assert!(!h264_profile_supported("avc1.zz"));
        assert!(!h264_profile_supported("hvc1.1.6.L93.B0"));
    }

    #[test]
    fn hevc_profiles() {
        assert!(hevc_profile_supported("hvc1.1.6.L93.B0"));
        assert!(hevc_profile_supported("hev1.2.4.L120.B0"));
        assert!(hevc_profile_supported("hvc1"));
        assert!(!hevc_profile_supported("hvc1.4.10.L93.B0"));
        assert!(!hevc_profile_supported("avc1.42e01e"));
    }

    #[test]
    fn aac_objects() {
        assert!(aac_object_supported("mp4a.40.2"));
        assert!(aac_object_supported("mp4a.40.5"));
        assert!(aac_object_supported("MP4A.40.29"));
        assert!(!aac_object_supported("mp4a.40.34"));
        assert!(!aac_object_supported("mp4a.69"));
    }
}

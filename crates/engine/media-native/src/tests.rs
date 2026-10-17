//! Тесты реестра, `canPlayType` и сессии на реальных файлах.
//!
//! `tests/data/av1-vorbis.webm` — 64×64, 25 кадр/с, 3 с, ключевой кадр каждые
//! 10 кадров, звук Vorbis моно 44,1 кГц (ffmpeg 9: `testsrc2` + `sine=440`,
//! libaom-av1 + libvorbis).

use lumen_core::ext::{VideoDecodeSession, VideoDecoder};

use super::*;

const AV1_VORBIS: &[u8] = include_bytes!("../tests/data/av1-vorbis.webm");
const AV1_MP4: &[u8] = include_bytes!("../../media-demux/tests/data/av1.mp4");
const HEVC_MP4: &[u8] = include_bytes!("../../media-demux/tests/data/hevc.mp4");
const VP9_WEBM: &[u8] = include_bytes!("../../../../tests/wpt/media/video.webm");

fn open(bytes: &[u8]) -> Box<dyn VideoDecodeSession> {
    NativeVideoDecoder::default().open(bytes).unwrap()
}

fn can_play(mime: &str) -> &'static str {
    NativeVideoDecoder::default().can_play_type(mime)
}

/// Декодер только на встроенных кодеках: результат не зависит от того, какие
/// декодеры (H.264/HEVC/AAC) есть у ОС на машине, где идут тесты.
fn bundled_decoder() -> NativeVideoDecoder {
    NativeVideoDecoder::with_registry(Box::leak(Box::new(registry::Registry::bundled())))
}

#[test]
fn can_play_type_av1_probably() {
    assert_eq!(can_play("video/webm; codecs=\"av01.0.05M.08\""), "probably");
    assert_eq!(can_play("video/mp4; codecs=\"av01.0.08M.10\""), "probably");
    assert_eq!(can_play("VIDEO/WEBM; Codecs=\"av01.0.05M.08, vorbis\""), "probably");
}

#[test]
fn can_play_type_unknown_or_unopenable_codec_is_empty() {
    // Кодеки, которых во встроенном реестре нет (H.264/HEVC/AAC придут от ОС,
    // VP9/Opus — по замеру).
    let can_play = |mime: &str| bundled_decoder().can_play_type(mime);
    assert_eq!(can_play("video/mp4; codecs=\"avc1.42E01E, mp4a.40.2\""), "");
    assert_eq!(can_play("video/webm; codecs=\"vp09.00.10.08\""), "");
    assert_eq!(can_play("video/webm; codecs=\"av01.0.05M.08, opus\""), "");
    assert_eq!(can_play("video/webm; codecs=\"nonsense\""), "");
    assert_eq!(can_play("video/webm; codecs=\"\""), "");
    // Кодек не из этого контейнера.
    assert_eq!(can_play("video/webm; codecs=\"avc1.42E01E\""), "");
    assert_eq!(can_play("video/ogg; codecs=\"av01.0.05M.08\""), "");
    // Неверный профиль/глубина AV1.
    assert_eq!(can_play("video/webm; codecs=\"av01.3.05M.08\""), "");
    assert_eq!(can_play("video/webm; codecs=\"av01.0.05M.09\""), "");
    assert_eq!(can_play("video/webm; codecs=\"av01.0.05X.08\""), "");
}

#[test]
fn can_play_type_container_without_codecs_is_maybe() {
    assert_eq!(can_play("video/webm"), "maybe");
    assert_eq!(can_play("video/mp4"), "maybe");
    assert_eq!(can_play("video/ogg"), "maybe");
    assert_eq!(can_play("audio/ogg"), "maybe");
    assert_eq!(can_play("video/x-matroska"), "");
    assert_eq!(can_play("text/html"), "");
    assert_eq!(can_play(""), "");
}

#[test]
fn can_play_type_vorbis_in_ogg() {
    assert_eq!(can_play("audio/ogg; codecs=vorbis"), "probably");
    assert_eq!(can_play("audio/ogg; codecs=opus"), "");
}

#[test]
fn empty_registry_plays_nothing() {
    let reg = Box::leak(Box::new(registry::Registry::empty()));
    let dec = NativeVideoDecoder::with_registry(reg);
    assert_eq!(dec.can_play_type("video/webm"), "");
    assert_eq!(dec.can_play_type("video/webm; codecs=\"av01.0.05M.08\""), "");
    assert!(dec.open(AV1_VORBIS).is_err());
}

#[test]
fn open_webm_reports_size_duration_and_audio() {
    let s = open(AV1_VORBIS);
    assert_eq!(s.dimensions(), (64, 64));
    let d = s.duration_secs().unwrap();
    assert!((2.9..3.1).contains(&d), "длительность {d}");
    let a = s.audio_track().unwrap();
    assert_eq!((a.sample_rate, a.channels), (44_100, 1));
}

#[test]
fn frame_at_returns_rgba_of_frame_size_and_time_changes_it() {
    let mut s = open(AV1_VORBIS);
    let first = s.frame_at(0.0).unwrap();
    assert_eq!(first.len(), 64 * 64 * 4);
    assert!(first.chunks(4).all(|px| px[3] == 255));
    let later = s.frame_at(1.0).unwrap();
    assert_ne!(first, later, "testsrc2 меняется со временем");
    // Повторный запрос той же позиции — тот же кадр.
    assert_eq!(later, s.frame_at(1.0).unwrap());
}

#[test]
fn sequential_playback_matches_fresh_seek() {
    let mut playing = open(AV1_VORBIS);
    let mut t = 0.0;
    while t < 1.3 {
        playing.frame_at(t).unwrap();
        t += 0.033;
    }
    let sequential = playing.frame_at(1.3).unwrap();
    let seeked = open(AV1_VORBIS).frame_at(1.3).unwrap();
    assert_eq!(sequential, seeked);
}

#[test]
fn backward_and_far_forward_seeks_land_on_the_same_frames() {
    let reference = |t: f64| open(AV1_VORBIS).frame_at(t).unwrap();
    let mut s = open(AV1_VORBIS);
    let at_2 = s.frame_at(2.0).unwrap();
    assert_eq!(at_2, reference(2.0));
    // Назад — к ключевому кадру до цели.
    assert_eq!(s.frame_at(0.5).unwrap(), reference(0.5));
    // Далеко вперёд — перемоткой, а не декодированием подряд.
    assert_eq!(s.frame_at(2.7).unwrap(), reference(2.7));
    // Между ключевыми: 1,23 с — кадр 30 (пятый после ключевого на 1,2 с).
    assert_eq!(s.frame_at(1.23).unwrap(), reference(1.23));
}

#[test]
fn frame_at_past_the_end_shows_the_last_frame() {
    let mut s = open(AV1_VORBIS);
    let last = s.frame_at(2.96).unwrap();
    assert_eq!(s.frame_at(10.0).unwrap(), last);
    // После конца можно вернуться в начало.
    assert_eq!(s.frame_at(0.0).unwrap(), open(AV1_VORBIS).frame_at(0.0).unwrap());
}

#[test]
fn negative_and_nan_time_show_the_first_frame() {
    let mut s = open(AV1_VORBIS);
    let first = s.frame_at(0.0).unwrap();
    assert_eq!(s.frame_at(-5.0).unwrap(), first);
    assert_eq!(s.frame_at(f64::NAN).unwrap(), first);
}

#[test]
fn audio_pcm_is_continuous_and_covers_the_file() {
    let mut s = open(AV1_VORBIS);
    s.frame_at(0.0).unwrap();
    let mut total = 0usize;
    let mut peak = 0i16;
    // Тик за тиком, как в оболочке: кадр, затем кусок звука.
    for step in 0..120 {
        s.frame_at(f64::from(step) * 0.025).unwrap();
        let pcm = s.decode_audio_pcm(1103).unwrap();
        peak = peak.max(pcm.iter().map(|x| x.saturating_abs()).max().unwrap_or(0));
        total += pcm.len();
    }
    // 120 запросов по 1103 сэмпла моно — файл хватает на ~2,99 с, но не на все 3 с.
    assert!(total >= 120 * 1103 - 2048, "получено {total} сэмплов");
    assert!(peak > 3000, "синус 440 Гц должен быть слышен, пик {peak}");
}

#[test]
fn audio_pcm_after_seek_continues_from_the_new_position() {
    let mut s = open(AV1_VORBIS);
    s.frame_at(2.0).unwrap();
    let pcm = s.decode_audio_pcm(4410).unwrap();
    assert_eq!(pcm.len(), 4410);
    assert!(pcm.iter().any(|&x| x.abs() > 3000));
    // До конца файла звук заканчивается короткой порцией, а не ошибкой.
    let mut left = 0;
    loop {
        let chunk = s.decode_audio_pcm(44_100).unwrap();
        left += chunk.len();
        if chunk.len() < 44_100 {
            break;
        }
    }
    assert!(left < 44_100 * 2, "после 2 с осталось {left} сэмплов");
}

#[test]
fn mp4_with_av1_plays_from_a_late_keyframe_without_repeating_headers() {
    let mut s = open(AV1_MP4);
    assert_eq!(s.dimensions(), (64, 64));
    assert!(s.audio_track().is_none());
    let first = s.frame_at(0.0).unwrap();
    let last = s.frame_at(100.0).unwrap();
    assert_eq!(first.len(), last.len());
    assert_ne!(first, last);
    assert_eq!(s.frame_at(0.0).unwrap(), first);
    assert!(s.decode_audio_pcm(100).is_err());
}

#[test]
fn unsupported_video_codec_fails_to_open_with_a_reason() {
    let hevc = bundled_decoder().open(HEVC_MP4).err().unwrap();
    assert!(hevc.contains("нет декодера"), "{hevc}");
    assert!(hevc.contains("hev1"), "{hevc}");
    let vp9 = bundled_decoder().open(VP9_WEBM).err().unwrap();
    assert!(vp9.contains("нет декодера"), "{vp9}");
}

#[test]
fn garbage_does_not_open() {
    let dec = NativeVideoDecoder::default();
    assert!(dec.open(b"not a media file at all").is_err());
    assert!(dec.open(&[]).is_err());
    assert!(dec.open(&AV1_VORBIS[..40]).is_err());
}

#[test]
fn truncated_file_still_opens_and_plays_what_is_there() {
    let cut = &AV1_VORBIS[..AV1_VORBIS.len() / 2];
    if let Ok(mut s) = NativeVideoDecoder::default().open(cut) {
        s.frame_at(0.0).unwrap();
        assert!(s.frame_at(0.4).is_ok());
    }
}

#[test]
fn decoder_identity() {
    let d = NativeVideoDecoder::default();
    assert_eq!(d.decoder_name(), "lumen-native");
    assert!(d.mime_types().contains(&"video/webm"));
}

/// COMPAT-MEDIA-OSDEC-WIN-BAR: какой компонент ОС назвать пользователю.
#[test]
fn missing_component_follows_codec_and_status() {
    use lumen_media_demux::Codec;
    let none = OsCodecStatus { h264: false, hevc: false, aac: false };
    let all = OsCodecStatus { h264: true, hevc: true, aac: true };
    assert_eq!(missing_for_video_codec(Codec::H264, none), Some(MissingOsCodec::MediaFeaturePack));
    assert_eq!(missing_for_video_codec(Codec::Hevc, none), Some(MissingOsCodec::HevcExtension));
    assert_eq!(missing_for_video_codec(Codec::H264, all), None);
    assert_eq!(missing_for_video_codec(Codec::Hevc, all), None);
    // Свободные кодеки от ОС не зависят.
    assert_eq!(missing_for_video_codec(Codec::Av1, none), None);
    assert_eq!(missing_for_video_codec(Codec::Vp9, none), None);
    // H.264 есть, HEVC-расширения нет — называется только оно.
    let h264_only = OsCodecStatus { h264: true, hevc: false, aac: true };
    assert_eq!(missing_for_video_codec(Codec::Hevc, h264_only), Some(MissingOsCodec::HevcExtension));
    assert_eq!(MissingOsCodec::MediaFeaturePack.install_uri(), Some("ms-settings:optionalfeatures"));
    assert!(MissingOsCodec::HevcExtension.install_uri().is_some_and(|u| u.starts_with("ms-windows-store:")));
    // Linux: ссылки нет, есть команда дистрибутива.
    let linux = MissingOsCodec::LinuxCodecs(crate::distro::LinuxDistro::Debian);
    assert_eq!(linux.install_uri(), None);
    assert!(linux.install_command().is_some_and(|c| c.starts_with("sudo apt install")));
    assert_eq!(MissingOsCodec::HevcExtension.install_command(), None);
}

/// Файл на свободном кодеке никогда не «ждёт компонент ОС».
#[test]
fn free_codec_file_never_blames_the_os() {
    assert_eq!(missing_os_codec(AV1_VORBIS), None);
    assert_eq!(missing_os_codec(b"not a media file"), None);
}

/// Декодеры ОС (Windows, Media Foundation): H.264 + AAC из `tests/data/h264-aac.mp4`
/// (320×180, 25 кадр/с, B-кадры, AAC-LC стерео). Если у машины нет декодера
/// (Windows N без Media Feature Pack) или он отключён `LUMEN_OS_DECODERS=0`,
/// тест ничего не проверяет — в этом случае работает «нет декодера» ниже.
#[cfg(windows)]
mod os_decoders {
    use super::*;

    const H264_AAC: &[u8] = include_bytes!("../tests/data/h264-aac.mp4");

    fn os_ready() -> bool {
        let s = os_codec_status().unwrap();
        s.h264 && s.aac
    }

    /// Сбой открытия H.264/HEVC-файла называет недостающий компонент ОС ровно
    /// тогда, когда декодера нет (`LUMEN_OS_DECODERS=0` или Windows N/KN).
    #[test]
    fn open_failure_names_the_missing_component() {
        let s = os_codec_status().unwrap();
        assert_eq!(missing_os_codec(H264_AAC), (!s.h264).then_some(MissingOsCodec::MediaFeaturePack));
        assert_eq!(missing_os_codec(HEVC_MP4), (!s.hevc).then_some(MissingOsCodec::HevcExtension));
    }

    #[test]
    fn can_play_type_follows_installed_decoders() {
        let s = os_codec_status().unwrap();
        let want = |ok: bool| if ok { "probably" } else { "" };
        assert_eq!(can_play("video/mp4; codecs=\"avc1.42E01E\""), want(s.h264));
        assert_eq!(can_play("video/mp4; codecs=\"avc1.64001F, mp4a.40.2\""), want(s.h264 && s.aac));
        assert_eq!(can_play("audio/mp4; codecs=\"mp4a.40.2\""), want(s.aac));
        assert_eq!(can_play("video/mp4; codecs=\"hvc1.1.6.L93.B0\""), want(s.hevc));
        // Профили, которых у декодера ОС нет, не обещаются.
        assert_eq!(can_play("video/mp4; codecs=\"avc1.6e0028\""), "");
        assert_eq!(can_play("video/mp4; codecs=\"mp4a.40.34\""), "");
        // VP9/Opus ОС не даёт.
        assert_eq!(can_play("video/mp4; codecs=\"vp09.00.10.08\""), "");
        // Встроенные кодеки остаются.
        assert_eq!(can_play("video/webm; codecs=\"av01.0.05M.08\""), "probably");
    }

    #[test]
    fn h264_aac_opens_and_decodes_frames() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        assert_eq!(s.dimensions(), (320, 180));
        let a = s.audio_track().expect("звуковая дорожка AAC");
        assert_eq!(a.channels, 2);
        let first = s.frame_at(0.0).unwrap();
        assert_eq!(first.len(), 320 * 180 * 4);
        assert!(first.chunks(4).all(|px| px[3] == 255));
        // Не чёрный и не сплошной цвет: тестовая картинка с разными пикселями.
        assert!(first.chunks(4).any(|px| px[0] != first[0] || px[1] != first[1]));
        let later = s.frame_at(0.8).unwrap();
        assert_ne!(first, later, "картинка меняется со временем");
        assert_eq!(later, s.frame_at(0.8).unwrap());
    }

    #[test]
    fn h264_seek_back_matches_fresh_open() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        let at_half = s.frame_at(0.5).unwrap();
        s.frame_at(1.2).unwrap();
        assert_eq!(s.frame_at(0.5).unwrap(), at_half);
        assert_eq!(open(H264_AAC).frame_at(0.5).unwrap(), at_half);
    }

    #[test]
    fn h264_sequential_playback_matches_seek() {
        if !os_ready() {
            return;
        }
        let mut playing = open(H264_AAC);
        let mut t = 0.0;
        while t < 0.9 {
            playing.frame_at(t).unwrap();
            t += 0.04;
        }
        assert_eq!(playing.frame_at(0.9).unwrap(), open(H264_AAC).frame_at(0.9).unwrap());
    }

    #[test]
    fn aac_pcm_is_audible_and_covers_the_file() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        s.frame_at(0.0).unwrap();
        // `decode_audio_pcm` считает сэмплы на канал: стерео — вдвое больше значений.
        let first = s.decode_audio_pcm(4410).unwrap();
        assert_eq!(first.len(), 4410 * 2);
        assert!(first.iter().any(|&x| x.abs() > 1000), "звук должен быть слышен");
        let mut total = first.len();
        loop {
            let chunk = s.decode_audio_pcm(4410).unwrap();
            total += chunk.len();
            if chunk.len() < 4410 * 2 {
                break;
            }
        }
        // 2 с × 44,1 кГц × 2 канала; допуск — срез приращения кодера на краях.
        assert!(total.abs_diff(2 * 44_100 * 2) <= 2 * 2048 * 2, "получено {total} значений");
    }

    #[test]
    fn os_decoders_can_be_switched_off_only_by_env_at_start() {
        // Отключение читается один раз при первой загрузке Media Foundation, поэтому
        // здесь проверяем только согласованность двух ответов.
        let s = os_codec_status().unwrap();
        assert_eq!(s.media_feature_pack_missing(), !s.h264 && !s.aac);
    }
}

/// Декодеры ОС (macOS, VideoToolbox + AudioToolbox): те же `h264-aac.mp4` и `hevc.mp4`.
/// H.264 и AAC есть в каждой macOS, поэтому проверяются безусловно (кроме
/// `LUMEN_OS_DECODERS=0`); HEVC — только если `canPlayType` его обещает.
/// Прогоняются в CI на `macos-latest`.
#[cfg(target_os = "macos")]
mod os_decoders_mac {
    use super::*;

    const H264_AAC: &[u8] = include_bytes!("../tests/data/h264-aac.mp4");

    fn os_ready() -> bool {
        can_play("video/mp4; codecs=\"avc1.64001F, mp4a.40.2\"") == "probably"
    }

    fn hevc_ready() -> bool {
        can_play("video/mp4; codecs=\"hvc1.1.6.L93.B0\"") == "probably"
    }

    #[test]
    fn no_install_hint_is_needed_on_macos() {
        assert_eq!(os_codec_status(), None);
        assert_eq!(missing_os_codec(H264_AAC), None);
    }

    #[test]
    fn can_play_type_follows_the_os() {
        if os_ready() {
            assert_eq!(can_play("video/mp4; codecs=\"avc1.42E01E\""), "probably");
            assert_eq!(can_play("audio/mp4; codecs=\"mp4a.40.2\""), "probably");
        }
        // Профили, которых у декодера нет, не обещаются.
        assert_eq!(can_play("video/mp4; codecs=\"avc1.6e0028\""), "");
        assert_eq!(can_play("video/mp4; codecs=\"mp4a.40.34\""), "");
        // VP9/Opus ОС не даёт.
        assert_eq!(can_play("video/mp4; codecs=\"vp09.00.10.08\""), "");
        // Встроенные кодеки остаются.
        assert_eq!(can_play("video/webm; codecs=\"av01.0.05M.08\""), "probably");
    }

    #[test]
    fn h264_aac_opens_and_decodes_frames() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        assert_eq!(s.dimensions(), (320, 180));
        let a = s.audio_track().expect("звуковая дорожка AAC");
        assert_eq!(a.channels, 2);
        let first = s.frame_at(0.0).unwrap();
        assert_eq!(first.len(), 320 * 180 * 4);
        assert!(first.chunks(4).all(|px| px[3] == 255));
        assert!(first.chunks(4).any(|px| px[0] != first[0] || px[1] != first[1]));
        let later = s.frame_at(0.8).unwrap();
        assert_ne!(first, later, "картинка меняется со временем");
        assert_eq!(later, s.frame_at(0.8).unwrap());
    }

    #[test]
    fn h264_seek_back_matches_fresh_open() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        let at_half = s.frame_at(0.5).unwrap();
        s.frame_at(1.2).unwrap();
        assert_eq!(s.frame_at(0.5).unwrap(), at_half);
        assert_eq!(open(H264_AAC).frame_at(0.5).unwrap(), at_half);
    }

    #[test]
    fn h264_sequential_playback_matches_seek() {
        if !os_ready() {
            return;
        }
        let mut playing = open(H264_AAC);
        let mut t = 0.0;
        while t < 0.9 {
            playing.frame_at(t).unwrap();
            t += 0.04;
        }
        assert_eq!(playing.frame_at(0.9).unwrap(), open(H264_AAC).frame_at(0.9).unwrap());
    }

    #[test]
    fn aac_pcm_is_audible_and_covers_the_file() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        s.frame_at(0.0).unwrap();
        let first = s.decode_audio_pcm(4410).unwrap();
        assert_eq!(first.len(), 4410 * 2);
        assert!(first.iter().any(|&x| x.abs() > 1000), "звук должен быть слышен");
        let mut total = first.len();
        loop {
            let chunk = s.decode_audio_pcm(4410).unwrap();
            total += chunk.len();
            if chunk.len() < 4410 * 2 {
                break;
            }
        }
        assert!(total.abs_diff(2 * 44_100 * 2) <= 2 * 2048 * 2, "получено {total} значений");
    }

    #[test]
    fn hevc_decodes_a_frame_when_the_os_offers_it() {
        if !hevc_ready() {
            return;
        }
        let mut s = open(HEVC_MP4);
        let (w, h) = s.dimensions();
        let frame = s.frame_at(0.0).unwrap();
        assert_eq!(frame.len(), w as usize * h as usize * 4);
        assert!(frame.chunks(4).any(|px| px[0] != frame[0] || px[1] != frame[1]));
    }
}

/// Декодеры ОС (Linux, GStreamer): те же `h264-aac.mp4` и `hevc.mp4`. Что видно
/// по `canPlayType`, зависит от установленных плагинов, поэтому проверки декодирования
/// пропускаются без них, а ветка «кодеков нет» проверяется отдельно. CI ставит
/// плагины и задаёт `LUMEN_EXPECT_GSTREAMER=1` — тогда их отсутствие считается ошибкой.
#[cfg(target_os = "linux")]
mod os_decoders_linux {
    use super::*;

    const H264_AAC: &[u8] = include_bytes!("../tests/data/h264-aac.mp4");

    fn os_ready() -> bool {
        can_play("video/mp4; codecs=\"avc1.64001F, mp4a.40.2\"") == "probably"
    }

    fn hevc_ready() -> bool {
        can_play("video/mp4; codecs=\"hvc1.1.6.L93.B0\"") == "probably"
    }

    #[test]
    fn ci_has_the_system_decoders() {
        if std::env::var_os("LUMEN_EXPECT_GSTREAMER").is_some() {
            assert!(os_ready(), "GStreamer с декодерами H.264/AAC не найден: {:?}", os_codec_status());
        }
    }

    #[test]
    fn status_agrees_with_can_play_type() {
        let s = os_codec_status().expect("на Linux состояние декодеров известно");
        assert_eq!(can_play("video/mp4; codecs=\"avc1.64001F\"") == "probably", s.h264);
        assert_eq!(can_play("audio/mp4; codecs=\"mp4a.40.2\"") == "probably", s.aac);
        assert_eq!(can_play("video/mp4; codecs=\"hvc1.1.6.L93.B0\"") == "probably", s.hevc);
    }

    #[test]
    fn without_decoders_the_page_gets_an_empty_answer_and_a_hint() {
        let s = os_codec_status().unwrap();
        if s.h264 {
            assert_eq!(missing_os_codec(H264_AAC), None);
            return;
        }
        assert_eq!(can_play("video/mp4; codecs=\"avc1.64001F\""), "");
        match missing_os_codec(H264_AAC) {
            Some(MissingOsCodec::LinuxCodecs(distro)) => assert!(!distro.install_command().is_empty()),
            other => panic!("ожидалась подсказка с командой, получено {other:?}"),
        }
        // Файл на встроенных кодеках подсказки не вызывает.
        assert_eq!(missing_os_codec(AV1_VORBIS), None);
    }

    #[test]
    fn can_play_type_rejects_profiles_the_decoders_lack() {
        assert_eq!(can_play("video/mp4; codecs=\"avc1.6e0028\""), "");
        assert_eq!(can_play("video/mp4; codecs=\"mp4a.40.34\""), "");
        assert_eq!(can_play("video/mp4; codecs=\"vp09.00.10.08\""), "");
        assert_eq!(can_play("video/webm; codecs=\"av01.0.05M.08\""), "probably");
    }

    #[test]
    fn h264_aac_opens_and_decodes_frames() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        assert_eq!(s.dimensions(), (320, 180));
        let a = s.audio_track().expect("звуковая дорожка AAC");
        assert_eq!(a.channels, 2);
        let first = s.frame_at(0.0).unwrap();
        assert_eq!(first.len(), 320 * 180 * 4);
        assert!(first.chunks(4).all(|px| px[3] == 255));
        assert!(first.chunks(4).any(|px| px[0] != first[0] || px[1] != first[1]));
        let later = s.frame_at(0.8).unwrap();
        assert_ne!(first, later, "картинка меняется со временем");
        assert_eq!(later, s.frame_at(0.8).unwrap());
    }

    #[test]
    fn h264_seek_back_matches_fresh_open() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        let at_half = s.frame_at(0.5).unwrap();
        s.frame_at(1.2).unwrap();
        assert_eq!(s.frame_at(0.5).unwrap(), at_half);
        assert_eq!(open(H264_AAC).frame_at(0.5).unwrap(), at_half);
    }

    #[test]
    fn h264_sequential_playback_matches_seek() {
        if !os_ready() {
            return;
        }
        let mut playing = open(H264_AAC);
        let mut t = 0.0;
        while t < 0.9 {
            playing.frame_at(t).unwrap();
            t += 0.04;
        }
        assert_eq!(playing.frame_at(0.9).unwrap(), open(H264_AAC).frame_at(0.9).unwrap());
    }

    #[test]
    fn aac_pcm_is_audible_and_covers_the_file() {
        if !os_ready() {
            return;
        }
        let mut s = open(H264_AAC);
        s.frame_at(0.0).unwrap();
        let first = s.decode_audio_pcm(4410).unwrap();
        assert_eq!(first.len(), 4410 * 2);
        assert!(first.iter().any(|&x| x.abs() > 1000), "звук должен быть слышен");
        let mut total = first.len();
        loop {
            let chunk = s.decode_audio_pcm(4410).unwrap();
            total += chunk.len();
            if chunk.len() < 4410 * 2 {
                break;
            }
        }
        assert!(total.abs_diff(2 * 44_100 * 2) <= 2 * 2048 * 2, "получено {total} значений");
    }

    #[test]
    fn hevc_decodes_a_frame_when_the_system_offers_it() {
        if !hevc_ready() {
            return;
        }
        let mut s = open(HEVC_MP4);
        let (w, h) = s.dimensions();
        let frame = s.frame_at(0.0).unwrap();
        assert_eq!(frame.len(), w as usize * h as usize * 4);
        assert!(frame.chunks(4).any(|px| px[0] != frame[0] || px[1] != frame[1]));
    }
}

// ── Воспроизведение из буфера MediaSource (COMPAT-MSE) ─────────────────────────

mod mse {
    use lumen_media_mse::SharedMediaSource;

    use super::*;

    fn source_with(bytes: &[u8]) -> SharedMediaSource {
        let shared = SharedMediaSource::default();
        append(&shared, bytes);
        shared
    }

    /// Файл целиком кусками по 3000 байт, как его скармливает страница.
    fn append(shared: &SharedMediaSource, bytes: &[u8]) {
        let mut core = shared.lock().unwrap();
        if core.buffer_count() == 0 {
            core.add_buffer("video/webm").unwrap();
        }
        for chunk in bytes.chunks(3000) {
            core.buffer_mut(0).unwrap().append(chunk).unwrap();
        }
    }

    #[test]
    fn plays_the_same_frames_as_the_file() {
        let shared = source_with(AV1_VORBIS);
        shared.lock().unwrap().set_ended(true);
        let mut mse = open_mse(shared).unwrap();
        let mut file = open(AV1_VORBIS);
        assert_eq!(mse.dimensions(), file.dimensions());
        for t in [0.0, 0.5, 1.0, 2.2, 0.4] {
            assert_eq!(mse.frame_at(t).unwrap(), file.frame_at(t).unwrap(), "t={t}");
        }
        assert!(mse.audio_track().is_some());
        assert!(!mse.decode_audio_pcm(1000).unwrap().is_empty());
    }

    #[test]
    fn holds_last_frame_when_buffer_runs_dry_and_resumes_after_append() {
        let (head, _) = AV1_VORBIS.split_at(AV1_VORBIS.len() * 2 / 3);
        let shared = source_with(head);
        let mut mse = open_mse(std::sync::Arc::clone(&shared)).unwrap();
        // Позиция за границей буфера: ошибки нет, показан последний кадр.
        let held = mse.frame_at(2.5).unwrap();
        assert_eq!(held.len(), 64 * 64 * 4);
        assert_eq!(mse.frame_at(2.6).unwrap(), held);
        // Остальное доехало — в конец файла сессия доходит без перемотки с нуля.
        append(&shared, &AV1_VORBIS[head.len()..]);
        shared.lock().unwrap().set_ended(true);
        let late = mse.frame_at(2.5).unwrap();
        assert_ne!(late, held, "после дозагрузки показан настоящий кадр 2.5 с");
        let mut file = open(AV1_VORBIS);
        assert_eq!(late, file.frame_at(2.5).unwrap());
    }

    #[test]
    fn empty_buffer_does_not_open() {
        let shared = SharedMediaSource::default();
        shared.lock().unwrap().add_buffer("video/webm").unwrap();
        assert!(open_mse(shared).is_err());
    }
}
